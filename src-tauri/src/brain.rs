//! "The brain": who does Whiskr's thinking (meeting summaries, turning speech
//! into tasks, the morning briefing, meeting briefs).
//!
//! Claude (through the Claude Code CLI, on the user's subscription) when it's
//! installed, Groq as the fast free fallback. Dictation cleanup stays on Groq —
//! it has to be instant.

use crate::agents::{self, Tool};
use crate::groq;
use reqwest::Client;
use serde_json::Value;
use std::io::{Read, Write};
use std::process::Stdio;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Engine {
    Claude { tool: Tool, profile_key: String, model: String },
    Groq { key: String },
}

#[derive(Clone, Debug)]
pub struct Brain {
    pub primary: Engine,
    /// Used when the primary fails (Claude logged out, offline CLI, …).
    pub fallback: Option<Engine>,
}

/// What Claude may use. `None` = no tools at all (pure text in → text out).
#[derive(Clone, Debug, Default)]
pub struct Access {
    pub tools: Option<Vec<String>>,
}

impl Engine {
    pub fn label(&self) -> &'static str {
        match self {
            Engine::Claude { .. } => "Claude",
            Engine::Groq { .. } => "Groq",
        }
    }
}

impl Brain {
    pub fn label(&self) -> &'static str {
        self.primary.label()
    }

    /// Claude can take a whole long transcript at once; Groq's free tier can't.
    pub fn big_context(&self) -> bool {
        matches!(self.primary, Engine::Claude { .. })
    }

    pub async fn text(&self, http: &Client, system: &str, user: &str, access: &Access, timeout: Duration) -> Result<(String, &'static str), String> {
        match run(&self.primary, http, system, user, access, timeout).await {
            Ok(t) => Ok((t, self.primary.label())),
            Err(e) => match &self.fallback {
                Some(f) => {
                    eprintln!("brain: {} failed ({e}), falling back to {}", self.primary.label(), f.label());
                    let t = run(f, http, system, user, access, timeout).await.map_err(|e2| format!("{e} / {e2}"))?;
                    Ok((t, f.label()))
                }
                None => Err(e),
            },
        }
    }

    pub async fn json(&self, http: &Client, system: &str, user: &str, timeout: Duration) -> Result<Value, String> {
        let first = run_json(&self.primary, http, system, user, timeout).await;
        match (first, &self.fallback) {
            (Ok(v), _) => Ok(v),
            (Err(e), Some(f)) => {
                eprintln!("brain: {} failed ({e}), falling back to {}", self.primary.label(), f.label());
                run_json(f, http, system, user, timeout).await
            }
            (Err(e), None) => Err(e),
        }
    }
}

async fn run(engine: &Engine, http: &Client, system: &str, user: &str, access: &Access, timeout: Duration) -> Result<String, String> {
    match engine {
        Engine::Groq { key } => groq::chat_text(http, key, system, user).await,
        Engine::Claude { tool, profile_key, model } => {
            let (tool, key, model, system, user, access) =
                (tool.clone(), profile_key.clone(), model.clone(), system.to_string(), user.to_string(), access.clone());
            tauri::async_runtime::spawn_blocking(move || claude_once(&tool, &key, &model, &system, &user, &access, timeout))
                .await
                .map_err(|e| e.to_string())?
        }
    }
}

async fn run_json(engine: &Engine, http: &Client, system: &str, user: &str, timeout: Duration) -> Result<Value, String> {
    match engine {
        Engine::Groq { key } => groq::chat_json(http, key, system, user).await,
        Engine::Claude { .. } => {
            let sys = format!("{system} Reply with one JSON object only — no prose, no code fences.");
            let t = run(engine, http, &sys, user, &Access::default(), timeout).await?;
            extract_json(&t).ok_or_else(|| format!("Claude nevrátil JSON: {}", t.chars().take(200).collect::<String>()))
        }
    }
}

/// First `{ … }` object in a reply (tolerates code fences and chatter).
pub fn extract_json(t: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(t.trim()) {
        if v.is_object() {
            return Some(v);
        }
    }
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    (end > start).then(|| serde_json::from_str::<Value>(&t[start..=end]).ok()).flatten().filter(|v| v.is_object())
}

/// One non-interactive Claude Code run. The system prompt goes in as a short
/// single-line flag; everything long goes through stdin (no quoting limits).
pub fn claude_once(tool: &Tool, profile_key: &str, model: &str, system: &str, user: &str, access: &Access, timeout: Duration) -> Result<String, String> {
    let sys_line = system.replace(['\n', '\r'], " ");
    let mut args: Vec<String> = ["-p", "--output-format", "json", "--no-session-persistence", "--system-prompt"].iter().map(|s| s.to_string()).collect();
    args.push(sys_line);
    match &access.tools {
        None => args.extend(["--tools".into(), String::new()]),
        Some(list) => {
            // Only what's listed (and the user's own allow rules); never a shell or edits.
            args.extend(["--permission-mode".into(), "dontAsk".into()]);
            args.extend(["--disallowedTools".into(), "Bash,Edit,Write,NotebookEdit".into()]);
            if !list.is_empty() {
                args.push("--allowedTools".into());
                args.push(list.join(","));
            }
        }
    }
    if !model.trim().is_empty() {
        args.extend(["--model".into(), model.trim().to_string()]);
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let mut cmd = agents::build_with(tool, &refs, profile_key, None, false);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Claude Code se nespustil: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        let input = user.to_string();
        std::thread::spawn(move || {
            let _ = si.write_all(input.as_bytes());
        });
    }
    let mut so = child.stdout.take().ok_or("no stdout")?;
    let mut se = child.stderr.take().ok_or("no stderr")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = so.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let err_thread = std::thread::spawn(move || {
        let mut e = String::new();
        let _ = se.read_to_string(&mut e);
        e
    });
    let out = match rx.recv_timeout(timeout) {
        Ok(o) => o,
        Err(_) => {
            let _ = child.kill();
            return Err("Claude neodpověděl včas".into());
        }
    };
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = err_thread.join().unwrap_or_default();
    parse_result(&out).ok_or_else(|| {
        let why = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        if why.is_empty() {
            format!("Claude Code skončil s chybou ({status})")
        } else {
            format!("Claude Code: {why}")
        }
    })?
}

/// `--output-format json` → the final answer, or the error it reports.
fn parse_result(out: &str) -> Option<Result<String, String>> {
    // The JSON result is the last JSON line (some setups print warnings before it).
    let v: Value = out.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(|v| v.get("result").is_some() || v.get("is_error").is_some()))?;
    let text = v["result"].as_str().unwrap_or("").to_string();
    if v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str().map_or(false, |s| s.starts_with("error")) {
        let msg = if text.is_empty() { v["subtype"].as_str().unwrap_or("error").to_string() } else { text };
        return Some(Err(format!("Claude: {}", friendly(&msg))));
    }
    Some(Ok(text))
}

fn friendly(msg: &str) -> String {
    let low = msg.to_lowercase();
    if low.contains("login") || low.contains("log in") || low.contains("authenticat") || low.contains("api key") {
        "nejsi přihlášený — v Nastavení → Whiskr Ask klikni na „Přihlásit“".into()
    } else if low.contains("limit") {
        format!("narazil jsi na limit předplatného ({})", msg.chars().take(120).collect::<String>())
    } else {
        msg.chars().take(300).collect()
    }
}

/// `claude mcp list` → names of the user's MCP servers / claude.ai connectors.
pub fn mcp_servers(tool: &Tool, profile_key: &str) -> Vec<String> {
    let mut cmd = agents::build(tool, &["mcp", "list"], profile_key, None);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else { return vec![] };
    let mut so = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    let out = match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(s) => s,
        Err(_) => {
            let _ = child.kill();
            return vec![];
        }
    };
    let _ = child.wait();
    parse_mcp_list(&out)
}

pub fn parse_mcp_list(out: &str) -> Vec<String> {
    let mut v = vec![];
    for line in out.lines() {
        let l = line.trim();
        // "claude.ai Gmail: https://… - ✓ Connected"   "iris: node server.js - ✗ Failed"
        let Some((name, rest)) = l.split_once(": ") else { continue };
        if name.is_empty() || name.contains("  ") || name.starts_with("Checking") || !(rest.contains(" - ") || rest.contains("http")) {
            continue;
        }
        if !v.contains(&name.to_string()) {
            v.push(name.to_string());
        }
    }
    v
}

/// Server name → the permission rule that allows all of its tools.
pub fn mcp_rule(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("mcp__{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_json_result() {
        let out = "warning: something\n{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ahoj\"}\n";
        assert_eq!(parse_result(out), Some(Ok("ahoj".into())));
        let bad = "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"Invalid API key · Please run /login\"}";
        assert!(matches!(parse_result(bad), Some(Err(e)) if e.contains("přihlášený")));
        assert_eq!(parse_result("garbage"), None);
    }

    #[test]
    fn finds_json_in_chatter() {
        let v = extract_json("Sure!\n```json\n{\"tasks\": [1]}\n```").unwrap();
        assert_eq!(v["tasks"][0], 1);
        assert!(extract_json("no json").is_none());
    }

    #[test]
    fn parses_mcp_list() {
        let out = "Checking MCP server health...\n\nclaude.ai Gmail: https://gmail.mcp.claude.com/mcp - ✓ Connected\nclaude.ai Google Calendar: https://gcal.mcp.claude.com/mcp - ✓ Connected\niris: node C:/iris/server.js - ✗ Failed to connect\n";
        assert_eq!(parse_mcp_list(out), vec!["claude.ai Gmail", "claude.ai Google Calendar", "iris"]);
        assert_eq!(mcp_rule("claude.ai Google Calendar"), "mcp__claude_ai_Google_Calendar");
    }
}
