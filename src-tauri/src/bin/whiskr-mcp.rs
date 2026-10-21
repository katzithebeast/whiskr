//! whiskr-mcp — MCP server (stdio) that lets Claude read your Whiskr data:
//! dictation history, meeting recordings (summaries, tasks, transcripts) and
//! your dictionary. Read-only; it never changes anything.
//!
//! Claude Desktop:  { "mcpServers": { "whiskr": { "command": "C:\\…\\whiskr-mcp.exe" } } }
//! Claude Code:     claude mcp add whiskr -- "C:\…\whiskr-mcp.exe"

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const IDENTIFIER: &str = "app.whiskr.desktop";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join(IDENTIFIER)
}
fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join(IDENTIFIER)
}

fn read_json(path: PathBuf) -> Option<Value> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

fn date(v: &Value) -> String {
    // "2026-10-20T18:05:12.123+02:00" → "2026-10-20 18:05"
    let s = v.as_str().unwrap_or("");
    s.get(..16).unwrap_or(s).replace('T', " ")
}

fn ts(sec: f64) -> String {
    let s = sec.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn norm(s: &str) -> String {
    // lowercase + strip Czech diacritics, so "ukoly" finds "úkoly"
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a', 'č' => 'c', 'ď' => 'd', 'é' | 'ě' => 'e', 'í' => 'i', 'ň' => 'n',
            'ó' => 'o', 'ř' => 'r', 'š' => 's', 'ť' => 't', 'ú' | 'ů' => 'u', 'ý' => 'y', 'ž' => 'z',
            c => c,
        })
        .collect()
}

fn matches(text: &str, query: &str) -> bool {
    let t = norm(text);
    norm(query).split_whitespace().all(|w| t.contains(w))
}

fn recordings() -> Vec<Value> {
    let root = data_dir().join("recordings");
    let mut out: Vec<Value> = std::fs::read_dir(root)
        .map(|it| it.flatten().filter_map(|e| read_json(e.path().join("meta.json"))).collect())
        .unwrap_or_default();
    out.sort_by(|a, b| b["created"].as_str().cmp(&a["created"].as_str()));
    out
}

fn speaker_name(rec: &Value, label: &str) -> String {
    rec["speakers"][label].as_str().map(str::to_string).unwrap_or_else(|| match label {
        "ME" => "Já".into(),
        "OTHERS" => "Ostatní".into(),
        l => format!("Mluvčí {l}"),
    })
}

fn participants(rec: &Value) -> Vec<String> {
    let mut names = vec![];
    for u in rec["utterances"].as_array().into_iter().flatten() {
        let n = speaker_name(rec, u["speaker"].as_str().unwrap_or(""));
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

// ───────────────────────── tools ─────────────────────────

fn tool_search_dictations(args: &Value) -> String {
    let query = args["query"].as_str().unwrap_or("").trim();
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 200) as usize;
    let Some(h) = read_json(data_dir().join("history.json")) else {
        return "Historie diktátů je prázdná.".into();
    };
    let items: Vec<String> = h["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| query.is_empty() || matches(e["text"].as_str().unwrap_or(""), query))
        .take(limit)
        .map(|e| format!("- [{}] {}", date(&e["at"]), e["text"].as_str().unwrap_or("").replace('\n', " ")))
        .collect();
    if items.is_empty() {
        return format!("Žádný diktát neodpovídá dotazu „{query}“.");
    }
    format!("Diktáty ({}):\n{}", items.len(), items.join("\n"))
}

fn tool_list_recordings(args: &Value) -> String {
    let query = args["query"].as_str().unwrap_or("").trim();
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
    let recs: Vec<String> = recordings()
        .iter()
        .filter(|r| {
            if query.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {}",
                r["title"].as_str().unwrap_or(""),
                r["summary"]["overview"].as_str().unwrap_or(""),
                str_list(&r["summary"]["topics"]).join(" ")
            );
            matches(&hay, query)
        })
        .take(limit)
        .map(|r| {
            let mut line = format!(
                "- id: {} | {} | {} | {} min | stav: {}",
                r["id"].as_str().unwrap_or(""),
                r["title"].as_str().unwrap_or(""),
                date(&r["created"]),
                (r["seconds"].as_f64().unwrap_or(0.0) / 60.0).round(),
                r["status"].as_str().unwrap_or("")
            );
            let p = participants(r);
            if !p.is_empty() {
                line.push_str(&format!(" | účastníci: {}", p.join(", ")));
            }
            let topics = str_list(&r["summary"]["topics"]);
            if !topics.is_empty() {
                line.push_str(&format!(" | témata: {}", topics.join(", ")));
            }
            line
        })
        .collect();
    if recs.is_empty() {
        return "Žádné nahrávky.".into();
    }
    format!("Nahrávky ({}):\n{}\n\nDetail získáš nástrojem get_recording s id.", recs.len(), recs.join("\n"))
}

fn tool_get_recording(args: &Value) -> Result<String, String> {
    let id = args["id"].as_str().unwrap_or("").trim();
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return Err("Chybí platné id (zjistíš ho přes list_recordings).".into());
    }
    let r = read_json(data_dir().join("recordings").join(id).join("meta.json")).ok_or("Nahrávka nenalezena")?;
    let with_transcript = args["include_transcript"].as_bool().unwrap_or(true);
    let mut md = format!(
        "# {}\n{} · {} min · účastníci: {}\n",
        r["title"].as_str().unwrap_or(""),
        date(&r["created"]),
        (r["seconds"].as_f64().unwrap_or(0.0) / 60.0).round(),
        participants(&r).join(", ")
    );
    let s = &r["summary"];
    if let Some(o) = s["overview"].as_str().filter(|o| !o.is_empty()) {
        md.push_str(&format!("\n## Shrnutí\n{o}\n"));
    }
    let tasks = str_list(&s["action_items"]);
    if !tasks.is_empty() {
        md.push_str("\n## Úkoly\n");
        for t in tasks {
            md.push_str(&format!("- {t}\n"));
        }
    }
    let dec = str_list(&s["decisions"]);
    if !dec.is_empty() {
        md.push_str("\n## Rozhodnutí\n");
        for d in dec {
            md.push_str(&format!("- {d}\n"));
        }
    }
    if with_transcript {
        md.push_str("\n## Přepis\n");
        for u in r["utterances"].as_array().into_iter().flatten() {
            md.push_str(&format!(
                "[{}] {}: {}\n",
                ts(u["start"].as_f64().unwrap_or(0.0)),
                speaker_name(&r, u["speaker"].as_str().unwrap_or("")),
                u["text"].as_str().unwrap_or("")
            ));
        }
    }
    if r["status"].as_str() != Some("done") {
        md.push_str(&format!("\n(Stav nahrávky: {})\n", r["status"].as_str().unwrap_or("")));
    }
    Ok(md)
}

fn tool_search_recordings(args: &Value) -> Result<String, String> {
    let query = args["query"].as_str().unwrap_or("").trim();
    if query.is_empty() {
        return Err("Zadej query.".into());
    }
    let limit = args["limit"].as_u64().unwrap_or(30).clamp(1, 200) as usize;
    let mut hits = vec![];
    'outer: for r in recordings() {
        let title = r["title"].as_str().unwrap_or("").to_string();
        let id = r["id"].as_str().unwrap_or("").to_string();
        for u in r["utterances"].as_array().into_iter().flatten() {
            let text = u["text"].as_str().unwrap_or("");
            if matches(text, query) {
                hits.push(format!(
                    "- {} ({}) [{}] {}: {}",
                    title,
                    id,
                    ts(u["start"].as_f64().unwrap_or(0.0)),
                    speaker_name(&r, u["speaker"].as_str().unwrap_or("")),
                    text
                ));
                if hits.len() >= limit {
                    break 'outer;
                }
            }
        }
    }
    if hits.is_empty() {
        return Ok(format!("V přepisech jsem „{query}“ nenašla."));
    }
    Ok(format!("Výskyty ({}):\n{}", hits.len(), hits.join("\n")))
}

fn tool_get_dictionary() -> String {
    let words = read_json(config_dir().join("settings.json"))
        .map(|s| str_list(&s["dictionary"]))
        .unwrap_or_default();
    if words.is_empty() {
        "Slovník je prázdný.".into()
    } else {
        format!("Slovník Whiskru: {}", words.join(", "))
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "search_dictations",
            "description": "Search the user's Whiskr voice-dictation history (things they dictated into apps: messages, emails, prompts, notes). Empty query returns the most recent ones.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Words to look for (diacritics-insensitive). Optional." },
                "limit": { "type": "integer", "description": "Max results, default 20." }
            }},
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "list_recordings",
            "description": "List meeting/call recordings made with the Whiskr recorder: id, title, date, length, participants, topics. Optionally filter by a query over titles, summaries and topics.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "description": "Default 20." }
            }},
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_recording",
            "description": "Get one recording: summary, action items, decisions and the full transcript with speaker names and timestamps.",
            "inputSchema": { "type": "object", "required": ["id"], "properties": {
                "id": { "type": "string", "description": "Recording id from list_recordings." },
                "include_transcript": { "type": "boolean", "description": "Default true. Set false for just the summary." }
            }},
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "search_recordings",
            "description": "Full-text search across all recording transcripts. Returns matching lines with recording, timestamp and speaker.",
            "inputSchema": { "type": "object", "required": ["query"], "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "description": "Default 30." }
            }},
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_dictionary",
            "description": "The user's Whiskr dictionary: names, brands and terms they use (useful for correct spelling).",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

fn call_tool(name: &str, args: &Value) -> Result<String, String> {
    match name {
        "search_dictations" => Ok(tool_search_dictations(args)),
        "list_recordings" => Ok(tool_list_recordings(args)),
        "get_recording" => tool_get_recording(args),
        "search_recordings" => tool_search_recordings(args),
        "get_dictionary" => Ok(tool_get_dictionary()),
        other => Err(format!("Neznámý nástroj: {other}")),
    }
}

// ───────────────────────── JSON-RPC over stdio ─────────────────────────

fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or("");
    let id = id?; // notifications get no reply
    let result = match method {
        "initialize" => {
            let pv = msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18");
            Ok(json!({
                "protocolVersion": pv,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "whiskr", "title": "Whiskr", "version": VERSION },
                "instructions": "Whiskr is the user's voice app. Use these tools to look up what the user dictated and what was said in their recorded meetings and calls (summaries, tasks, decisions, transcripts). Data is in the user's language, usually Czech."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"]["arguments"].clone();
            let args = if args.is_object() { args } else { json!({}) };
            Ok(match call_tool(name, &args) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("whiskr-mcp {VERSION}");
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.iter().filter_map(handle).collect();
                if out.is_empty() { None } else { Some(Value::Array(out)) }
            }
            Ok(msg) => handle(&msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(r) = reply {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}
