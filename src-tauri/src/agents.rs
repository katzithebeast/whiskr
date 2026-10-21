//! Whiskr Ask: talk to Claude Code, Codex and Cursor Agent through their CLIs.
//!
//! Why CLIs: they log in with the user's own subscriptions (Claude Pro/Max,
//! ChatGPT Plus), so asking and delegating costs nothing extra. Whiskr finds
//! them on Windows and inside WSL, sends the prompt through stdin (no quoting
//! trouble), reads their JSONL event stream and turns it into a few simple
//! events for the chat window.
//!
//! Profiles (personal / work) are separate config folders:
//! CLAUDE_CONFIG_DIR=~/.claude-<key>, CODEX_HOME=~/.codex-<key>.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const ENGINES: [&str; 3] = ["claude", "codex", "cursor"];

fn bin_name(engine: &str) -> &'static str {
    match engine {
        "claude" => "claude",
        "codex" => "codex",
        _ => "cursor-agent",
    }
}

pub fn engine_label(engine: &str) -> &'static str {
    match engine {
        "claude" => "Claude",
        "codex" => "GPT (Codex)",
        "cursor" => "Cursor",
        _ => "Groq",
    }
}

// ───────────────────────────── settings types ─────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// "" = the CLI's default folder (~/.claude, ~/.codex); otherwise
    /// ~/.claude-<key> and ~/.codex-<key>.
    #[serde(default)]
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    /// Windows path, e.g. C:\Users\me\code\iris
    pub path: String,
    /// Profile id this project belongs to ("" = whatever is active).
    #[serde(default)]
    pub profile: String,
}

pub fn default_profiles() -> Vec<Profile> {
    vec![
        Profile { id: "personal".into(), name: "Osobní".into(), key: String::new() },
        Profile { id: "work".into(), name: "Práce".into(), key: "work".into() },
    ]
}

// ───────────────────────────── detection ─────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct Tool {
    pub engine: String,
    /// "windows" | "wsl"
    pub runtime: String,
    pub path: String,
    /// None = unknown (not checked)
    pub logged_in: Option<bool>,
}

#[cfg(windows)]
fn windows_candidates(name: &str) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".local").join("bin"));
        dirs.push(h.join("AppData").join("Roaming").join("npm"));
        dirs.push(h.join("AppData").join("Local").join("cursor-agent"));
        dirs.push(h.join(".cursor").join("bin"));
    }
    let mut out = vec![];
    for d in dirs {
        for ext in ["exe", "cmd", "bat"] {
            let p = d.join(format!("{name}.{ext}"));
            if p.is_file() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn wsl_available() -> bool {
    cfg!(windows) && std::path::Path::new(r"C:\Windows\System32\wsl.exe").exists()
}

fn wsl_command(script: &str) -> Command {
    let mut c = Command::new("wsl.exe");
    // -i: nvm & co. usually live in .bashrc
    c.args(["-e", "bash", "-lic", script]);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// Run with a timeout; returns stdout.
fn output_with_timeout(mut cmd: Command, secs: u64) -> Option<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(s) => {
            let _ = child.wait();
            Some(s)
        }
        Err(_) => {
            let _ = child.kill();
            None
        }
    }
}

/// Run with a timeout; returns stdout + stderr together.
fn output_both_with_timeout(mut cmd: Command, secs: u64) -> Option<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut e = String::new();
        let _ = err.read_to_string(&mut e);
        let _ = tx.send(e);
    });
    let (tx2, rx2) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        let _ = tx2.send(s);
    });
    let deadline = std::time::Duration::from_secs(secs);
    match (rx2.recv_timeout(deadline), rx.recv_timeout(std::time::Duration::from_secs(2))) {
        (Ok(o), e) => {
            let _ = child.wait();
            Some(format!("{o}\n{}", e.unwrap_or_default()))
        }
        _ => {
            let _ = child.kill();
            None
        }
    }
}

/// Find every installed CLI (Windows first, then WSL).
pub fn detect() -> Vec<Tool> {
    let mut tools = vec![];
    #[cfg(windows)]
    for e in ENGINES {
        let names: &[&str] = if e == "cursor" { &["cursor-agent", "agent"] } else { &[bin_name(e)] };
        if let Some(p) = names.iter().flat_map(|n| windows_candidates(n)).next() {
            tools.push(Tool { engine: e.into(), runtime: "windows".into(), path: p.to_string_lossy().into(), logged_in: None });
        }
    }
    #[cfg(not(windows))]
    for e in ENGINES {
        if let Some(p) = which(bin_name(e)) {
            tools.push(Tool { engine: e.into(), runtime: "native".into(), path: p, logged_in: None });
        }
    }
    if wsl_available() {
        let script = "for c in claude codex cursor-agent; do p=$(command -v $c 2>/dev/null) && echo \"WHISKR:$c=$p\"; done";
        if let Some(out) = output_with_timeout(wsl_command(script), 12) {
            for line in out.lines() {
                if let Some(rest) = line.trim().strip_prefix("WHISKR:") {
                    if let Some((name, path)) = rest.split_once('=') {
                        let engine = if name == "cursor-agent" { "cursor" } else { name };
                        tools.push(Tool { engine: engine.into(), runtime: "wsl".into(), path: path.into(), logged_in: None });
                    }
                }
            }
        }
    }
    tools
}

#[cfg(not(windows))]
fn which(name: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).map(|d| d.join(name)).find(|p| p.is_file()).map(|p| p.to_string_lossy().to_string())
    })
}

/// Pick the tool for an engine: preferred runtime first, then any.
pub fn pick<'a>(tools: &'a [Tool], engine: &str, prefer: &str) -> Option<&'a Tool> {
    tools
        .iter()
        .find(|t| t.engine == engine && (prefer == "auto" || t.runtime == prefer))
        .or_else(|| tools.iter().find(|t| t.engine == engine))
}

/// Login status for one tool + profile (Claude and Codex only).
pub fn check_login(tool: &Tool, profile_key: &str) -> Option<bool> {
    let args: Vec<&str> = match tool.engine.as_str() {
        "claude" => vec!["auth", "status", "--json"],
        "codex" => vec!["login", "status"],
        _ => return None,
    };
    // Codex prints its status on stderr: read both.
    let mut cmd = build(tool, &args, profile_key, None);
    cmd.stderr(Stdio::piped());
    let out = output_both_with_timeout(cmd, 20)?;
    let low = out.to_lowercase();
    Some(match tool.engine.as_str() {
        "claude" => low.contains("\"loggedin\": true") || low.contains("\"loggedin\":true"),
        _ => low.contains("logged in") && !low.contains("not logged in"),
    })
}

// ───────────────────────────── building commands ─────────────────────────────

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn env_for(tool: &Tool, profile_key: &str) -> Vec<(&'static str, String)> {
    if profile_key.is_empty() {
        return vec![];
    }
    let (var, dir) = match tool.engine.as_str() {
        "claude" => ("CLAUDE_CONFIG_DIR", format!(".claude-{profile_key}")),
        "codex" => ("CODEX_HOME", format!(".codex-{profile_key}")),
        _ => return vec![],
    };
    if tool.runtime == "wsl" {
        vec![(var, format!("$HOME/{dir}"))]
    } else {
        let home = dirs::home_dir().unwrap_or_default();
        let p = home.join(dir);
        let _ = std::fs::create_dir_all(&p);
        vec![(var, p.to_string_lossy().to_string())]
    }
}

/// Where Codex saves generated pictures for this profile (Windows runtime only).
fn codex_images_dir(tool: &Tool, profile_key: &str) -> Option<std::path::PathBuf> {
    if tool.engine != "codex" || tool.runtime == "wsl" {
        return None;
    }
    let home = dirs::home_dir()?;
    let base = if profile_key.is_empty() {
        std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".codex"))
    } else {
        home.join(format!(".codex-{profile_key}"))
    };
    Some(base.join("generated_images"))
}

/// Pictures Codex made in WSL since `since`: find them inside WSL, copy them
/// over to Windows (into `into`) and return the Windows paths.
fn wsl_new_images(profile_key: &str, since: std::time::SystemTime, into: &std::path::Path) -> Vec<String> {
    let secs = since.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).saturating_sub(5);
    let home = if profile_key.is_empty() { "${CODEX_HOME:-$HOME/.codex}".to_string() } else { format!("$HOME/.codex-{profile_key}") };
    let script = format!(r#"find "{home}/generated_images" -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' \) -newermt @{secs} 2>/dev/null | while read -r f; do wslpath -w "$f"; done"#);
    let Some(out) = output_with_timeout(wsl_command(&script), 20) else { return vec![] };
    let _ = std::fs::create_dir_all(into);
    let mut res = vec![];
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let src = std::path::PathBuf::from(line);
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "obrazek.png".into());
        let dst = into.join(&name);
        match std::fs::copy(&src, &dst) {
            Ok(_) => res.push(dst.to_string_lossy().to_string()),
            Err(e) => crate::dlog!("codex image copy failed {line}: {e}"),
        }
    }
    res
}

/// Image files under `dir` written after `since`.
fn new_images(dir: &std::path::Path, since: std::time::SystemTime) -> Vec<String> {
    fn walk(d: &std::path::Path, since: std::time::SystemTime, out: &mut Vec<(std::time::SystemTime, String)>, depth: u8) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4 {
                    walk(&p, since, out, depth + 1);
                }
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if !["png", "jpg", "jpeg", "webp", "gif"].contains(&ext.as_str()) {
                continue;
            }
            if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                if m >= since {
                    out.push((m, p.to_string_lossy().to_string()));
                }
            }
        }
    }
    let mut v = vec![];
    walk(dir, since, &mut v, 0);
    v.sort();
    v.into_iter().map(|(_, p)| p).collect()
}

/// A ready-to-spawn command. `cwd` is a Windows path (WSL translates it).
pub fn build(tool: &Tool, args: &[&str], profile_key: &str, cwd: Option<&str>) -> Command {
    build_with(tool, args, profile_key, cwd, false)
}

/// `full_shell`: in WSL, load the whole interactive shell (.bashrc — nvm,
/// aliases…). Agents doing work in a project need it; a quick question
/// doesn't, and skipping it saves a second or two per message.
pub fn build_with(tool: &Tool, args: &[&str], profile_key: &str, cwd: Option<&str>, full_shell: bool) -> Command {
    let env = env_for(tool, profile_key);
    if tool.runtime == "wsl" {
        let mut script = String::new();
        if !full_shell {
            // the tool's own folder first (node from nvm lives next to npm tools)
            if let Some(dir) = std::path::Path::new(&tool.path).parent() {
                script.push_str(&format!("export PATH={}:\"$PATH\"; ", sh_quote(&dir.to_string_lossy())));
            }
        }
        for (k, v) in &env {
            // $HOME must expand, so double quotes
            script.push_str(&format!("export {k}=\"{v}\"; "));
        }
        if !env.is_empty() {
            script.push_str(&format!("mkdir -p \"${}\" 2>/dev/null; ", env[0].0));
        }
        script.push_str("exec ");
        script.push_str(&sh_quote(&tool.path));
        for a in args {
            script.push(' ');
            if *a == "__PROMPT_FROM_STDIN__" {
                script.push_str("\"$(cat)\"");
            } else {
                script.push_str(&sh_quote(a));
            }
        }
        let mut c = Command::new("wsl.exe");
        match cwd {
            Some(d) => c.args(["--cd", d]),
            None => c.args(["--cd", "~"]),
        };
        c.args(["-e", "bash", if full_shell { "-lic" } else { "-lc" }, &script]);
        #[cfg(windows)]
        c.creation_flags(CREATE_NO_WINDOW);
        c
    } else {
        let mut c = Command::new(&tool.path);
        c.args(args.iter().filter(|a| **a != "__PROMPT_FROM_STDIN__"));
        for (k, v) in env {
            c.env(k, v);
        }
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        #[cfg(windows)]
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
}

// ───────────────────────────── runs ─────────────────────────────

#[derive(Deserialize, Clone, Debug)]
pub struct RunReq {
    pub engine: String,
    /// "ask" | "delegate"
    pub mode: String,
    pub prompt: String,
    /// Windows path of the project (delegate) — ask runs in a neutral folder.
    pub cwd: Option<String>,
    /// Session to continue.
    pub resume: Option<String>,
    /// Profile key ("" = default login).
    #[serde(default)]
    pub profile_key: String,
    /// "safe" | "full"
    #[serde(default)]
    pub access: String,
    /// Model id / alias ("" = the tool's default).
    #[serde(default)]
    pub model: String,
    /// Quick answers: no MCP servers for Claude, low reasoning for Codex.
    #[serde(default)]
    pub fast: bool,
}

const ASK_SYSTEM: &str = "You are answering from the Whiskr quick-ask window. Be concise, concrete and practical. Answer in the language the user writes in. Use short Markdown (lists, code blocks) only when it helps.";

/// CLI arguments for one run. The prompt always goes through stdin.
pub fn args_for(req: &RunReq, runtime: &str) -> Vec<String> {
    let ask = req.mode != "delegate";
    let full = req.access == "full";
    let mut a: Vec<String> = vec![];
    let s = |x: &str| x.to_string();
    match req.engine.as_str() {
        "claude" => {
            a.extend(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"].map(s));
            if !req.model.is_empty() {
                a.extend([s("--model"), req.model.clone()]);
            }
            if let Some(id) = &req.resume {
                a.extend([s("--resume"), id.clone()]);
            }
            if ask {
                if req.fast {
                    // don't start every MCP server / connector for a quick question
                    a.push(s("--strict-mcp-config"));
                }
                a.extend(["--tools", "WebSearch,WebFetch", "--allowedTools", "WebSearch,WebFetch"].map(s));
                a.extend([s("--append-system-prompt"), s(ASK_SYSTEM)]);
            } else if full {
                a.extend(["--permission-mode", "bypassPermissions"].map(s));
            } else {
                a.extend(["--permission-mode", "acceptEdits"].map(s));
            }
        }
        "codex" => {
            a.push(s("exec"));
            if let Some(id) = &req.resume {
                a.extend([s("resume"), id.clone()]);
            }
            a.extend(["--json", "--skip-git-repo-check"].map(s));
            if !req.model.is_empty() {
                a.extend([s("-m"), req.model.clone()]);
            }
            if ask {
                a.extend(["-c", "sandbox_mode=read-only"].map(s));
                if req.fast {
                    a.extend(["-c", "model_reasoning_effort=\"low\""].map(s));
                }
            } else if full {
                a.push(s("--dangerously-bypass-approvals-and-sandbox"));
            } else {
                a.extend(["-c", "sandbox_mode=workspace-write"].map(s));
            }
            a.push(s("-"));
        }
        _ => {
            // cursor-agent takes the prompt as an argument
            a.extend(["-p", "--output-format", "stream-json"].map(s));
            if !req.model.is_empty() {
                a.extend([s("--model"), req.model.clone()]);
            }
            if let Some(id) = &req.resume {
                a.extend([s("--resume"), id.clone()]);
            }
            if !ask && full {
                a.push(s("--force"));
            }
            if runtime == "wsl" {
                a.push(s("__PROMPT_FROM_STDIN__"));
            } else {
                a.push(req.prompt.clone());
            }
        }
    }
    a
}

/// What the chat window gets.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ev {
    /// Streaming piece of the current answer.
    Delta { text: String },
    /// A whole message (engines that don't stream).
    Text { text: String },
    /// Tool use / progress: "▶ npm test", "✎ src/app.ts"
    Step { label: String },
    Session { id: String },
    /// A picture the agent made (Codex image_gen), as a local file path.
    Image { path: String },
    /// Final answer (may repeat the streamed text — the UI keeps what it has).
    Done { text: String },
    Error { text: String },
}

#[derive(Default)]
pub struct ParseState {
    streamed: bool,
    finished: bool,
}

fn short(s: &str, n: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n).collect::<String>())
    } else {
        one
    }
}

fn tool_label(name: &str, input: &Value) -> String {
    let pick = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let file = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string();
    match name {
        "Bash" => format!("▶ {}", short(pick("command"), 70)),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => format!("✎ {}", file(pick("file_path"))),
        "Read" => format!("📄 {}", file(pick("file_path"))),
        "Grep" | "Glob" => format!("🔍 {}", short(pick("pattern"), 50)),
        "WebSearch" => format!("🌐 {}", short(pick("query"), 60)),
        "WebFetch" => format!("🌐 {}", short(pick("url"), 60)),
        "TodoWrite" => "☑ plán úkolů".into(),
        "Task" | "Agent" => format!("🤖 {}", short(pick("description"), 50)),
        other => format!("⚙ {other}"),
    }
}

pub fn parse_line(engine: &str, line: &str, st: &mut ParseState) -> Vec<Ev> {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return vec![] };
    let t = v["type"].as_str().unwrap_or("");
    let text_of = |x: &Value| x.as_str().unwrap_or("").to_string();
    let mut out = vec![];
    match engine {
        "claude" => match t {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = v["session_id"].as_str() {
                    out.push(Ev::Session { id: id.into() });
                }
            }
            "stream_event" => {
                let e = &v["event"];
                if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                    st.streamed = true;
                    out.push(Ev::Delta { text: text_of(&e["delta"]["text"]) });
                } else if e["type"] == "message_start" && st.streamed {
                    // a new assistant message after tool use — separate it
                    out.push(Ev::Delta { text: "\n\n".into() });
                }
            }
            "assistant" => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str() {
                        Some("tool_use") => out.push(Ev::Step { label: tool_label(c["name"].as_str().unwrap_or("?"), &c["input"]) }),
                        Some("text") if !st.streamed => out.push(Ev::Text { text: text_of(&c["text"]) }),
                        _ => {}
                    }
                }
            }
            "result" => {
                st.finished = true;
                if let Some(id) = v["session_id"].as_str() {
                    out.push(Ev::Session { id: id.into() });
                }
                let text = text_of(&v["result"]);
                if v["is_error"].as_bool().unwrap_or(false) {
                    out.push(Ev::Error { text: if text.is_empty() { "Claude skončil chybou".into() } else { text } });
                } else {
                    out.push(Ev::Done { text });
                }
            }
            _ => {}
        },
        "codex" => match t {
            "thread.started" => {
                if let Some(id) = v["thread_id"].as_str() {
                    out.push(Ev::Session { id: id.into() });
                }
            }
            "item.started" | "item.completed" => {
                let it = &v["item"];
                let started = t == "item.started";
                match it["type"].as_str().unwrap_or("") {
                    "agent_message" if !started => out.push(Ev::Text { text: text_of(&it["text"]) }),
                    "command_execution" if started => out.push(Ev::Step { label: format!("▶ {}", short(it["command"].as_str().unwrap_or(""), 70)) }),
                    "file_change" if !started => {
                        let files: Vec<String> = it["changes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|c| c["path"].as_str())
                            .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
                            .collect();
                        out.push(Ev::Step { label: format!("✎ {}", if files.is_empty() { "soubory".into() } else { files.join(", ") }) });
                    }
                    "web_search" if started => out.push(Ev::Step { label: format!("🌐 {}", short(it["query"].as_str().unwrap_or(""), 60)) }),
                    "mcp_tool_call" if started => out.push(Ev::Step { label: format!("⚙ {}", it["tool"].as_str().unwrap_or("nástroj")) }),
                    "error" if !started => out.push(Ev::Step { label: format!("⚠ {}", short(it["message"].as_str().unwrap_or(""), 80)) }),
                    _ => {}
                }
            }
            "turn.completed" => {
                st.finished = true;
                out.push(Ev::Done { text: String::new() });
            }
            "turn.failed" => {
                st.finished = true;
                out.push(Ev::Error { text: text_of(&v["error"]["message"]) });
            }
            "error" => {
                let m = text_of(&v["message"]);
                if m.starts_with("Reconnecting") {
                    out.push(Ev::Step { label: "↻ znovu se připojuju…".into() });
                } else {
                    st.finished = true;
                    out.push(Ev::Error { text: m });
                }
            }
            _ => {}
        },
        _ => match t {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = v["session_id"].as_str() {
                    out.push(Ev::Session { id: id.into() });
                }
            }
            "assistant" => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    if c["type"] == "text" {
                        out.push(Ev::Text { text: text_of(&c["text"]) });
                    }
                }
            }
            "tool_call" if v["subtype"] == "started" => {
                let tc = &v["tool_call"];
                let (kind, args) = tc.as_object().and_then(|o| o.iter().next()).map(|(k, a)| (k.as_str(), &a["args"])).unwrap_or(("", &Value::Null));
                let path = args["path"].as_str().unwrap_or("");
                let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let label = match kind {
                    "shellToolCall" => format!("▶ {}", short(args["command"].as_str().unwrap_or(""), 70)),
                    "writeToolCall" | "editToolCall" => format!("✎ {file}"),
                    "readToolCall" => format!("📄 {file}"),
                    _ => format!("⚙ {}", kind.trim_end_matches("ToolCall")),
                };
                out.push(Ev::Step { label });
            }
            "result" => {
                st.finished = true;
                if v["is_error"].as_bool().unwrap_or(false) {
                    out.push(Ev::Error { text: text_of(&v["result"]) });
                } else {
                    out.push(Ev::Done { text: String::new() });
                }
            }
            _ => {}
        },
    }
    out
}

/// Turn the last bit of stderr into something a human can act on.
fn explain_failure(engine: &str, stderr: &str, code: Option<i32>) -> String {
    let low = stderr.to_lowercase();
    let name = engine_label(engine);
    if low.contains("not logged in") || low.contains("login") && low.contains("please") || low.contains("unauthorized") || low.contains("401") || low.contains("invalid api key") {
        return format!("{name} není přihlášený pro tenhle profil — v Nastavení → Whiskr Ask klikni na „Přihlásit“.");
    }
    if low.contains("rate limit") || low.contains("usage limit") || low.contains("429") {
        return format!("{name}: vyčerpaný limit předplatného. Zkus jiný model nebo chvíli počkej.");
    }
    let tail: String = stderr.lines().rev().filter(|l| !l.trim().is_empty()).take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    if tail.is_empty() {
        format!("{name} skončil bez odpovědi (kód {}).", code.map(|c| c.to_string()).unwrap_or("?".into()))
    } else {
        format!("{name}: {}", short(&tail, 300))
    }
}

struct Running {
    pid: u32,
    cancelled: bool,
}

fn running() -> &'static Mutex<HashMap<u64, Running>> {
    static R: OnceLock<Mutex<HashMap<u64, Running>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start a run in the background; `emit` gets every event (and a final Done/Error).
pub fn start(id: u64, tool: &Tool, req: RunReq, ask_dir: &std::path::Path, emit: impl Fn(Ev) + Send + Sync + 'static) -> Result<(), String> {
    let args = args_for(&req, &tool.runtime);
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let ask_dir_s = ask_dir.to_string_lossy().to_string();
    let cwd = match (&req.cwd, req.mode.as_str()) {
        (Some(c), _) if !c.trim().is_empty() => Some(c.clone()),
        (_, "delegate") => return Err("Vyber projekt, ve kterém má agent pracovat".into()),
        _ => {
            let _ = std::fs::create_dir_all(ask_dir);
            Some(ask_dir_s)
        }
    };
    let mut cmd = build_with(tool, &refs, &req.profile_key, cwd.as_deref(), req.mode == "delegate");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{} se nepodařilo spustit: {e}", engine_label(&req.engine)))?;
    let pid = child.id();
    running().lock().unwrap().insert(id, Running { pid, cancelled: false });

    // prompt → stdin (cursor on Windows got it as an argument)
    let feed = !(req.engine == "cursor" && tool.runtime != "wsl");
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = req.prompt.clone();
        std::thread::spawn(move || {
            if feed {
                let _ = stdin.write_all(prompt.as_bytes());
            }
            drop(stdin);
        });
    }
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let buf = stderr_buf.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = err.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = buf.lock().unwrap();
                b.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if b.len() > 16_000 {
                    let cut = b.len() - 8_000;
                    let cut = (cut..b.len()).find(|i| b.is_char_boundary(*i)).unwrap_or(b.len());
                    b.drain(..cut);
                }
            }
        });
    }
    let stdout = child.stdout.take().ok_or("Žádný výstup")?;
    let engine = req.engine.clone();
    crate::dlog!("agent run {id}: {} ({}) mode={} model={:?} args={:?}", engine, tool.runtime, req.mode, req.model, args);
    let images_dir = codex_images_dir(tool, &req.profile_key);
    let wsl_images = (tool.engine == "codex" && tool.runtime == "wsl").then(|| (req.profile_key.clone(), ask_dir.join("images")));
    let started = std::time::SystemTime::now();
    // Long silence (e.g. Codex drawing a picture prints nothing for a minute):
    // say it's still working instead of looking frozen.
    let last_line = Arc::new(Mutex::new(std::time::Instant::now()));
    let emit = Arc::new(emit);
    {
        let last = last_line.clone();
        let emit = emit.clone();
        std::thread::spawn(move || {
            let mut told = false;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !running().lock().unwrap().contains_key(&id) {
                    break;
                }
                let quiet = last.lock().unwrap().elapsed().as_secs();
                if quiet >= 20 && !told {
                    told = true;
                    emit(Ev::Step { label: "⏳ pořád pracuje…".into() });
                } else if quiet < 20 {
                    told = false;
                }
            }
        });
    }
    std::thread::spawn(move || {
        let mut st = ParseState::default();
        let mut errored = false;
        let flush_images = |emit: &dyn Fn(Ev)| {
            let mut found = vec![];
            if let Some(dir) = &images_dir {
                found = new_images(dir, started);
            } else if let Some((key, into)) = &wsl_images {
                found = wsl_new_images(key, started, into);
            }
            crate::dlog!("agent run {id}: images found {}", found.len());
            for p in found {
                crate::dlog!("agent run {id}: image {p}");
                emit(Ev::Image { path: p });
            }
        };
        let mut images_done = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            *last_line.lock().unwrap() = std::time::Instant::now();
            let ty = serde_json::from_str::<Value>(&line).ok().map(|v| {
                let t = v["type"].as_str().unwrap_or("?").to_string();
                let it = v["item"]["type"].as_str().map(|x| format!("/{x}")).unwrap_or_default();
                format!("{t}{it}")
            });
            match ty.as_deref() {
                Some(t) if !t.contains("error") && !t.contains("failed") => crate::dlog!("agent run {id}: {t}"),
                _ => crate::dlog!("agent run {id}: {}", short(&line, 600)),
            }
            for ev in parse_line(&engine, &line, &mut st) {
                if matches!(ev, Ev::Error { .. }) {
                    errored = true;
                }
                // pictures go before the final "done", or the chat stops listening
                if matches!(ev, Ev::Done { .. } | Ev::Error { .. }) && !images_done {
                    images_done = true;
                    flush_images(&*emit);
                }
                emit(ev);
            }
        }
        let status = child.wait().ok();
        if !images_done {
            images_done = true;
            flush_images(&*emit);
        }
        let _ = images_done;
        {
            let err = stderr_buf.lock().unwrap();
            let tail: String = err.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
            crate::dlog!("agent run {id}: exit {:?} finished={} errored={errored} stderr tail: {}", status.and_then(|s| s.code()), st.finished, tail.replace('\n', " | "));
        }
        let cancelled = running().lock().unwrap().remove(&id).map(|r| r.cancelled).unwrap_or(false);
        if cancelled {
            emit(Ev::Error { text: "Zastaveno".into() });
        } else if !st.finished && !errored {
            let err = stderr_buf.lock().unwrap().clone();
            if status.map(|s| s.success()).unwrap_or(false) && err.trim().is_empty() {
                emit(Ev::Done { text: String::new() });
            } else {
                emit(Ev::Error { text: explain_failure(&engine, &err, status.and_then(|s| s.code())) });
            }
        }
    });
    Ok(())
}

pub fn cancel(id: u64) {
    let pid = {
        let mut m = running().lock().unwrap();
        match m.get_mut(&id) {
            Some(r) => {
                r.cancelled = true;
                r.pid
            }
            None => return,
        }
    };
    #[cfg(windows)]
    {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(CREATE_NO_WINDOW);
        let _ = c.output();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").arg(pid.to_string()).output();
    }
}

/// Open a terminal where the user logs in for a profile.
pub fn open_login(tool: &Tool, profile_key: &str) -> Result<(), String> {
    let args: Vec<&str> = match tool.engine.as_str() {
        "claude" => vec!["auth", "login"],
        "codex" => vec!["login"],
        _ => vec!["login"],
    };
    #[cfg(windows)]
    {
        let env = env_for(tool, profile_key);
        let inner = if tool.runtime == "wsl" {
            let mut s = String::new();
            for (k, v) in &env {
                s.push_str(&format!("export {k}=\"{v}\"; "));
            }
            s.push_str(&sh_quote(&tool.path));
            for a in &args {
                s.push(' ');
                s.push_str(a);
            }
            format!("wsl.exe -e bash -lic \"{}\"", s.replace('"', "\\\""))
        } else {
            let mut s = String::new();
            for (k, v) in &env {
                s.push_str(&format!("set \"{k}={v}\" && "));
            }
            s.push_str(&format!("\"{}\"", tool.path));
            for a in &args {
                s.push(' ');
                s.push_str(a);
            }
            s
        };
        let mut c = Command::new("cmd.exe");
        c.raw_arg(format!("/C start \"Whiskr — přihlášení\" cmd.exe /K \"{inner}\""));
        c.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (tool, profile_key, args);
        Err("Přihlášení z Whiskru zatím jen na Windows".into())
    }
}

pub fn emit_json(ev: &Ev, run: u64) -> Value {
    let mut v = serde_json::to_value(ev).unwrap_or(json!({}));
    v["run"] = json!(run);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_stream() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ahoj"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ahoj"},{"type":"tool_use","name":"Bash","input":{"command":"npm test"}}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ahoj","session_id":"s1"}"#,
        ];
        let mut st = ParseState::default();
        let evs: Vec<Ev> = lines.iter().flat_map(|l| parse_line("claude", l, &mut st)).collect();
        assert_eq!(evs[0], Ev::Session { id: "s1".into() });
        assert_eq!(evs[1], Ev::Delta { text: "ahoj".into() });
        assert_eq!(evs[2], Ev::Step { label: "▶ npm test".into() });
        assert!(matches!(evs.last(), Some(Ev::Done { .. })));
        assert!(st.finished);
    }

    #[test]
    fn codex_stream() {
        let lines = [
            r#"{"type":"thread.started","thread_id":"t1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"error","message":"Reconnecting... 2/5"}"#,
            r#"{"type":"item.started","item":{"id":"i1","type":"command_execution","command":"ls -la","status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"i2","type":"file_change","changes":[{"path":"src/a.rs","kind":"update"}]}}"#,
            r#"{"type":"item.completed","item":{"id":"i3","type":"agent_message","text":"Hotovo"}}"#,
            r#"{"type":"turn.completed","usage":{}}"#,
        ];
        let mut st = ParseState::default();
        let evs: Vec<Ev> = lines.iter().flat_map(|l| parse_line("codex", l, &mut st)).collect();
        assert_eq!(evs[0], Ev::Session { id: "t1".into() });
        assert_eq!(evs[2], Ev::Step { label: "▶ ls -la".into() });
        assert_eq!(evs[3], Ev::Step { label: "✎ a.rs".into() });
        assert_eq!(evs[4], Ev::Text { text: "Hotovo".into() });
        assert!(matches!(evs[5], Ev::Done { .. }));
    }

    #[test]
    fn cursor_stream() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"c1","model":"x"}"#,
            r#"{"type":"tool_call","subtype":"started","call_id":"1","tool_call":{"readToolCall":{"args":{"path":"/a/README.md"}}},"session_id":"c1"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Ok"}]},"session_id":"c1"}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Ok","session_id":"c1"}"#,
        ];
        let mut st = ParseState::default();
        let evs: Vec<Ev> = lines.iter().flat_map(|l| parse_line("cursor", l, &mut st)).collect();
        assert_eq!(evs[1], Ev::Step { label: "📄 README.md".into() });
        assert_eq!(evs[2], Ev::Text { text: "Ok".into() });
    }

    #[test]
    fn args_never_carry_prompt_for_stdin_engines() {
        let req = RunReq { engine: "codex".into(), mode: "ask".into(), prompt: "tajné \"uvozovky\"".into(), cwd: None, resume: Some("t1".into()), profile_key: String::new(), access: String::new(), model: String::new(), fast: false };
        let a = args_for(&req, "windows");
        assert_eq!(&a[..3], &["exec", "resume", "t1"]);
        assert!(!a.iter().any(|x| x.contains("tajné")));
        assert_eq!(a.last().unwrap(), "-");
    }

    #[test]
    fn real_claude_capture_parses() {
        let path = "/tmp/claude-0/cl.jsonl";
        let Ok(s) = std::fs::read_to_string(path) else { return };
        let mut st = ParseState::default();
        let evs: Vec<Ev> = s.lines().flat_map(|l| parse_line("claude", l, &mut st)).collect();
        assert!(evs.iter().any(|e| matches!(e, Ev::Delta { text } if text.contains("ahoj"))));
        assert!(st.finished);
    }
}

#[cfg(test)]
mod login_tests {
    use super::*;

    /// Codex reports "Logged in …" on stderr — make sure we see it.
    #[test]
    #[ignore]
    fn codex_status_on_stderr() {
        let tool = Tool { engine: "codex".into(), runtime: "native".into(), path: "/home/claude/.npm-global/bin/codex".into(), logged_in: None };
        std::env::set_var("CODEX_HOME", "/tmp/cx");
        assert_eq!(check_login(&tool, ""), Some(true));
        std::env::set_var("CODEX_HOME", "/tmp/cx-none");
        assert_eq!(check_login(&tool, ""), Some(false));
    }
}


// ───────────────────────────── models ─────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// Claude Code takes aliases that always point at the newest model of a family.
pub fn claude_models() -> Vec<ModelInfo> {
    [("fable", "Fable 5.1"), ("opus", "Opus 5.5"), ("sonnet", "Sonnet 5.5"), ("haiku", "Haiku 5.5")]
        .iter()
        .map(|(id, name)| ModelInfo { id: id.to_string(), name: name.to_string() })
        .collect()
}

/// `codex debug models` → the models the account can pick (visibility "list").
pub fn parse_codex_models(out: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<Value>(out.trim()) else { return vec![] };
    let mut list: Vec<(i64, ModelInfo)> = v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"].as_str().unwrap_or("list") == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?.to_string();
            let name = m["display_name"].as_str().unwrap_or(&id).to_string();
            Some((m["priority"].as_i64().unwrap_or(99), ModelInfo { id, name }))
        })
        .collect();
    list.sort_by_key(|(p, _)| *p);
    list.into_iter().map(|(_, m)| m).collect()
}

/// `cursor-agent models` → one model per line ("id - Name" or just "id").
pub fn parse_cursor_models(out: &str) -> Vec<ModelInfo> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.to_lowercase().starts_with("available") && !l.starts_with("Tip"))
        .filter_map(|l| {
            let l = l.trim_start_matches(['-', '*', '•', ' ']);
            let (id, name) = match l.split_once(" - ") {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (l.split_whitespace().next()?, l),
            };
            (!id.is_empty() && !id.contains(':')).then(|| ModelInfo { id: id.to_string(), name: name.trim_end_matches(" (current)").trim_end_matches(" (default)").to_string() })
        })
        .take(40)
        .collect()
}

pub fn list_models(tool: &Tool, profile_key: &str) -> Vec<ModelInfo> {
    match tool.engine.as_str() {
        "claude" => claude_models(),
        "codex" => output_with_timeout(build(tool, &["debug", "models"], profile_key, None), 30).map(|o| parse_codex_models(&o)).unwrap_or_default(),
        _ => output_with_timeout(build(tool, &["models"], profile_key, None), 30).map(|o| parse_cursor_models(&o)).unwrap_or_default(),
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn codex_catalog() {
        let out = r#"{"models":[{"slug":"b","display_name":"B","visibility":"list","priority":2},{"slug":"h","display_name":"H","visibility":"hide","priority":0},{"slug":"a","display_name":"A","visibility":"list","priority":1}]}"#;
        let m = parse_codex_models(out);
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn cursor_lines() {
        let m = parse_cursor_models("Available models:\n\nauto - Auto (current)\nsonnet-4.5 - Claude Sonnet 4.5\ngpt-5\n");
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].id, "auto");
        assert_eq!(m[0].name, "Auto");
        assert_eq!(m[2].id, "gpt-5");
    }

    #[test]
    fn model_flags() {
        let mut r = RunReq { engine: "claude".into(), mode: "ask".into(), prompt: "x".into(), cwd: None, resume: None, profile_key: String::new(), access: String::new(), model: "opus".into(), fast: true };
        assert!(args_for(&r, "windows").windows(2).any(|w| w[0] == "--model" && w[1] == "opus"));
        r.engine = "codex".into();
        assert!(args_for(&r, "windows").windows(2).any(|w| w[0] == "-m" && w[1] == "opus"));
    }
}
