//! Connections: log in / out of Claude Code, Codex and Cursor Agent per
//! profile, and one library of MCP servers that can be handed to Claude Code,
//! Codex, Cursor and Claude Desktop with a click.

use crate::agents::{self, Tool};
use crate::{brain, claude_desktop_configs, mcp_exe, profile_key, refresh_tools, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct McpDef {
    pub name: String,
    /// "stdio" | "http"
    #[serde(default = "stdio")]
    pub kind: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: String,
    /// "KEY=value" lines
    #[serde(default)]
    pub env: Vec<String>,
}

fn stdio() -> String {
    "stdio".into()
}

/// Whiskr's own server (dictations, recordings, dictionary).
pub fn whiskr_def() -> McpDef {
    McpDef { name: "whiskr".into(), kind: "stdio".into(), command: mcp_exe().to_string_lossy().to_string(), ..Default::default() }
}

fn env_map(def: &McpDef) -> BTreeMap<String, String> {
    def.env
        .iter()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// JSON shape used by Claude Desktop and Cursor.
fn to_json(def: &McpDef, desktop: bool) -> Value {
    if def.kind == "http" {
        if desktop {
            // Claude Desktop only runs local servers: bridge with mcp-remote.
            return json!({ "command": "npx", "args": ["-y", "mcp-remote", def.url] });
        }
        return json!({ "url": def.url });
    }
    let mut v = json!({ "command": def.command, "args": def.args });
    let env = env_map(def);
    if !env.is_empty() {
        v["env"] = json!(env);
    }
    v
}

fn from_json(name: &str, v: &Value) -> McpDef {
    let mut d = McpDef { name: name.into(), kind: "stdio".into(), ..Default::default() };
    if let Some(u) = v["url"].as_str().or_else(|| v["serverUrl"].as_str()) {
        d.kind = "http".into();
        d.url = u.into();
        return d;
    }
    d.command = v["command"].as_str().unwrap_or("").into();
    d.args = v["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    // mcp-remote bridge back to a plain URL
    if d.command == "npx" && d.args.iter().any(|a| a == "mcp-remote") {
        if let Some(u) = d.args.iter().find(|a| a.starts_with("http")) {
            return McpDef { name: name.into(), kind: "http".into(), url: u.clone(), ..Default::default() };
        }
    }
    if let Some(env) = v["env"].as_object() {
        d.env = env.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(""))).collect();
    }
    d
}

// ───────────────────────────── running CLIs ─────────────────────────────

fn run(tool: &Tool, args: &[&str], profile_key: &str, input: Option<&str>, secs: u64) -> Result<String, String> {
    let mut cmd = agents::build(tool, args, profile_key, None);
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    if let (Some(text), Some(mut si)) = (input, child.stdin.take()) {
        let t = text.to_string();
        std::thread::spawn(move || {
            let _ = si.write_all(t.as_bytes());
        });
    }
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut o = String::new();
        let _ = so.read_to_string(&mut o);
        let mut e = String::new();
        let _ = se.read_to_string(&mut e);
        let _ = tx.send((o, e));
    });
    let (out, err) = match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(x) => x,
        Err(_) => {
            let _ = child.kill();
            return Err("Nástroj neodpověděl včas".into());
        }
    };
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if ok {
        Ok(out)
    } else {
        let why = err.lines().chain(out.lines()).rev().find(|l| !l.trim().is_empty()).unwrap_or("chyba").trim().to_string();
        Err(why)
    }
}

fn tool_for(app: &AppHandle, engine: &str) -> Option<Tool> {
    let state = app.state::<AppState>();
    if state.tools.lock().is_empty() {
        refresh_tools(app);
    }
    let rt = state.settings.lock().ask_runtime.clone();
    let tools = state.tools.lock();
    agents::pick(&tools, engine, &rt).cloned()
}

// ───────────────────────────── accounts ─────────────────────────────

#[derive(Serialize)]
pub struct Account {
    engine: String,
    tools: Vec<Tool>,
    /// profile id → logged in (None = unknown)
    logins: BTreeMap<String, Option<bool>>,
}

#[derive(Serialize)]
pub struct Accounts {
    accounts: Vec<Account>,
    groq: bool,
}

fn cursor_logged_in(tool: &Tool) -> Option<bool> {
    let out = run(tool, &["status"], "", None, 15).ok()?.to_lowercase();
    Some(out.contains("logged in") && !out.contains("not logged in"))
}

#[tauri::command]
pub async fn connections_status(app: AppHandle, refresh: bool) -> Result<Accounts, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        if refresh || state.tools.lock().is_empty() {
            refresh_tools(&app);
        }
        let (rt, profiles, groq) = {
            let s = state.settings.lock();
            (s.ask_runtime.clone(), s.profiles.clone(), !s.api_key.trim().is_empty())
        };
        let tools = state.tools.lock().clone();
        let mut accounts = vec![];
        std::thread::scope(|sc| {
            let mut handles = vec![];
            for engine in agents::ENGINES {
                let found: Vec<Tool> = tools.iter().filter(|t| t.engine == engine).cloned().collect();
                let tool = agents::pick(&tools, engine, &rt).cloned();
                let profiles = profiles.clone();
                handles.push(sc.spawn(move || {
                    let mut logins = BTreeMap::new();
                    if let Some(t) = &tool {
                        if engine == "cursor" {
                            let st = cursor_logged_in(t);
                            for p in &profiles {
                                logins.insert(p.id.clone(), st);
                            }
                        } else {
                            let res: Vec<(String, Option<bool>)> = std::thread::scope(|s2| {
                                let hs: Vec<_> = profiles.iter().map(|p| {
                                    let t = t.clone();
                                    let id = p.id.clone();
                                    let key = p.key.clone();
                                    s2.spawn(move || (id, agents::check_login(&t, &key)))
                                }).collect();
                                hs.into_iter().filter_map(|h| h.join().ok()).collect()
                            });
                            logins.extend(res);
                        }
                    }
                    Account { engine: engine.to_string(), tools: found, logins }
                }));
            }
            for h in handles {
                if let Ok(a) = h.join() {
                    accounts.push(a);
                }
            }
        });
        Ok(Accounts { accounts, groq })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn account_logout(app: AppHandle, engine: String, profile: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let tool = tool_for(&app, &engine).ok_or("Nástroj nenalezen")?;
        let key = profile_key(&app.state::<AppState>().settings.lock(), Some(&profile));
        let args: &[&str] = match engine.as_str() {
            "claude" => &["auth", "logout"],
            "codex" => &["logout"],
            _ => &["logout"],
        };
        run(&tool, args, &key, None, 30).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ───────────────────────────── MCP targets ─────────────────────────────

fn read_json(p: &PathBuf) -> Value {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn write_json(p: &PathBuf, v: &Value) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    if p.exists() {
        let _ = std::fs::copy(p, p.with_extension("json.whiskr-backup"));
    }
    std::fs::write(p, serde_json::to_string_pretty(v).unwrap()).map_err(|e| e.to_string())
}

fn desktop_files() -> Vec<PathBuf> {
    let found: Vec<PathBuf> = claude_desktop_configs().into_iter().filter(|c| c.parent().map(|d| d.exists()).unwrap_or(false)).collect();
    found
}

fn cursor_file() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor").join("mcp.json"))
}

fn json_servers(p: &PathBuf) -> BTreeMap<String, Value> {
    read_json(p)["mcpServers"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
}

fn set_json_server(p: &PathBuf, def: &McpDef, on: bool, desktop: bool) -> Result<(), String> {
    let mut v = read_json(p);
    if !v["mcpServers"].is_object() {
        v["mcpServers"] = json!({});
    }
    let m = v["mcpServers"].as_object_mut().unwrap();
    if on {
        m.insert(def.name.clone(), to_json(def, desktop));
    } else {
        m.remove(&def.name);
    }
    write_json(p, &v)
}

fn codex_names(out: &str) -> Vec<String> {
    if let Ok(v) = serde_json::from_str::<Value>(out.trim()) {
        if let Some(a) = v.as_array() {
            return a.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect();
        }
        if let Some(o) = v.as_object() {
            return o.keys().cloned().collect();
        }
    }
    // text table: first column, skip header
    out.lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next().map(String::from))
        .filter(|n| !n.is_empty() && !n.starts_with('-'))
        .collect()
}

#[derive(Serialize, Default)]
pub struct Target {
    available: bool,
    installed: Vec<String>,
    note: String,
}

#[derive(Serialize)]
pub struct McpStatus {
    targets: BTreeMap<String, Target>,
    /// Servers configured somewhere that aren't in the library yet.
    found: Vec<McpDef>,
    /// Connectors that come with the Claude account (claude.ai …).
    claude_ai: Vec<String>,
    whiskr: McpDef,
}

#[tauri::command]
pub async fn mcp_status(app: AppHandle, profile: String) -> Result<McpStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = profile_key(&app.state::<AppState>().settings.lock(), Some(&profile));
        let mut targets = BTreeMap::new();
        let mut found: BTreeMap<String, McpDef> = BTreeMap::new();
        let mut claude_ai = vec![];

        let claude = tool_for(&app, "claude");
        let codex = tool_for(&app, "codex");
        let (t_claude, t_codex) = std::thread::scope(|sc| {
            let h1 = sc.spawn(|| {
                let Some(t) = &claude else { return Target { note: "Claude Code není nainstalovaný".into(), ..Default::default() } };
                let names = brain::mcp_servers(t, &key);
                Target { available: true, installed: names, note: String::new() }
            });
            let h2 = sc.spawn(|| {
                let Some(t) = &codex else { return Target { note: "Codex není nainstalovaný".into(), ..Default::default() } };
                match run(t, &["mcp", "list", "--json"], &key, None, 30).or_else(|_| run(t, &["mcp", "list"], &key, None, 30)) {
                    Ok(out) => Target { available: true, installed: codex_names(&out), note: String::new() },
                    Err(e) => Target { available: true, installed: vec![], note: e },
                }
            });
            (h1.join().unwrap_or_default(), h2.join().unwrap_or_default())
        });
        let mut t_claude = t_claude;
        claude_ai.extend(t_claude.installed.iter().filter(|n| n.starts_with("claude.ai ")).cloned());
        t_claude.installed.retain(|n| !n.starts_with("claude.ai "));
        targets.insert("claude".to_string(), t_claude);
        targets.insert("codex".to_string(), t_codex);

        // Cursor (IDE + agent share ~/.cursor/mcp.json)
        let mut t_cursor = Target::default();
        if let Some(p) = cursor_file() {
            t_cursor.available = p.parent().map(|d| d.exists()).unwrap_or(false) || tool_for(&app, "cursor").is_some();
            for (name, v) in json_servers(&p) {
                t_cursor.installed.push(name.clone());
                found.entry(name.clone()).or_insert_with(|| from_json(&name, &v));
            }
        }
        if !t_cursor.available {
            t_cursor.note = "Cursor není nainstalovaný".into();
        }
        targets.insert("cursor".to_string(), t_cursor);

        // Claude Desktop
        let mut t_desk = Target::default();
        let files = desktop_files();
        t_desk.available = !files.is_empty();
        if let Some(p) = files.first() {
            for (name, v) in json_servers(p) {
                t_desk.installed.push(name.clone());
                found.entry(name.clone()).or_insert_with(|| from_json(&name, &v));
            }
        } else {
            t_desk.note = "Claude Desktop není nainstalovaný".into();
        }
        targets.insert("desktop".to_string(), t_desk);

        found.remove("whiskr");
        Ok(McpStatus { targets, found: found.into_values().collect(), claude_ai, whiskr: whiskr_def() })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Add (on) or remove (off) one server in one tool.
#[tauri::command]
pub async fn mcp_install(app: AppHandle, def: McpDef, target: String, profile: String, on: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let name = def.name.trim().to_string();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err("Název konektoru musí být jedno slovo (bez mezer)".into());
        }
        let key = profile_key(&app.state::<AppState>().settings.lock(), Some(&profile));
        match target.as_str() {
            "claude" => {
                let t = tool_for(&app, "claude").ok_or("Claude Code není nainstalovaný")?;
                let _ = run(&t, &["mcp", "remove", "-s", "user", &name], &key, None, 30);
                if !on {
                    return Ok(());
                }
                let mut a: Vec<String> = vec!["mcp".into(), "add".into(), "-s".into(), "user".into()];
                if def.kind == "http" {
                    a.extend(["--transport".into(), "http".into(), name.clone(), def.url.clone()]);
                } else {
                    for e in &def.env {
                        a.extend(["-e".into(), e.clone()]);
                    }
                    a.push(name.clone());
                    a.push("--".into());
                    a.push(def.command.clone());
                    a.extend(def.args.iter().cloned());
                }
                let refs: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
                run(&t, &refs, &key, None, 60).map(|_| ())
            }
            "codex" => {
                let t = tool_for(&app, "codex").ok_or("Codex není nainstalovaný")?;
                let _ = run(&t, &["mcp", "remove", &name], &key, None, 30);
                if !on {
                    return Ok(());
                }
                let mut a: Vec<String> = vec!["mcp".into(), "add".into(), name.clone()];
                if def.kind == "http" {
                    a.extend(["--url".into(), def.url.clone()]);
                } else {
                    for e in &def.env {
                        a.extend(["--env".into(), e.clone()]);
                    }
                    a.push("--".into());
                    a.push(def.command.clone());
                    a.extend(def.args.iter().cloned());
                }
                let refs: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
                run(&t, &refs, &key, None, 60).map(|_| ())
            }
            "cursor" => {
                let p = cursor_file().ok_or("Nenašla jsem domovskou složku")?;
                set_json_server(&p, &def, on, false)
            }
            "desktop" => {
                let files = desktop_files();
                if files.is_empty() {
                    return Err("Claude Desktop není nainstalovaný".into());
                }
                for p in files {
                    set_json_server(&p, &def, on, true)?;
                }
                Ok(())
            }
            _ => Err("Neznámý cíl".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let d = McpDef { name: "gh".into(), kind: "stdio".into(), command: "npx".into(), args: vec!["-y".into(), "gh-mcp".into()], url: String::new(), env: vec!["TOKEN=abc".into()] };
        let back = from_json("gh", &to_json(&d, false));
        assert_eq!(back, d);
        let h = McpDef { name: "sentry".into(), kind: "http".into(), url: "https://mcp.sentry.dev/mcp".into(), ..Default::default() };
        assert_eq!(from_json("sentry", &to_json(&h, true)), h);
        assert_eq!(from_json("sentry", &to_json(&h, false)), h);
    }

    #[test]
    fn codex_list_parsing() {
        assert_eq!(codex_names(r#"[{"name":"whiskr"},{"name":"gh"}]"#), vec!["whiskr", "gh"]);
        assert_eq!(codex_names("Name     Command\nwhiskr   C:/x.exe\n"), vec!["whiskr"]);
    }
}
