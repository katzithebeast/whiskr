//! The everyday-assistant part of Whiskr: tasks & reminders by voice, the
//! morning briefing, a brief before a meeting, and noticing calls.
//! Thinking goes to Claude (the user's subscription) when it's installed.

use crate::brain::{self, Access, Brain, Engine};
use crate::briefing::{self, Briefing};
use crate::tasks::{self, Task};
use crate::{agents, ask, bar_state, calls, groq, notes, profile_key, recorder, refresh_tools, show_main, AppState};
use chrono::{Local, NaiveTime, Timelike};
use parking_lot::Mutex;
use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

// ───────────────────────────── the brain ─────────────────────────────

fn brain_profile_id(s: &crate::Settings) -> String {
    if s.brain_profile.is_empty() || !s.profiles.iter().any(|p| p.id == s.brain_profile) {
        s.active_profile.clone()
    } else {
        s.brain_profile.clone()
    }
}

/// Claude when installed (and not switched off), Groq as fallback.
pub fn make_brain(app: &AppHandle) -> Option<Brain> {
    let state = app.state::<AppState>();
    let s = state.settings.lock().clone();
    let groq = (!s.api_key.trim().is_empty()).then(|| Engine::Groq { key: s.api_key.clone() });
    let claude = if s.brain == "groq" {
        None
    } else {
        let tools = state.tools.lock();
        agents::pick(&tools, "claude", &s.ask_runtime).cloned().map(|tool| Engine::Claude {
            tool,
            profile_key: profile_key(&s, Some(&brain_profile_id(&s))),
            model: s.brain_model.clone(),
        })
    };
    match (claude, groq) {
        (Some(c), g) => Some(Brain { primary: c, fallback: g }),
        (None, Some(g)) => Some(Brain { primary: g, fallback: None }),
        _ => None,
    }
}

/// Wait for the first tool detection (it runs in the background at start).
fn ensure_tools(app: &AppHandle) {
    let empty = app.state::<AppState>().tools.lock().is_empty();
    if empty {
        refresh_tools(app);
    }
}

#[derive(serde::Serialize)]
pub struct BrainStatus {
    engine: String,
    profile: String,
    fallback: bool,
}

#[tauri::command]
pub async fn brain_status(app: AppHandle) -> BrainStatus {
    let a = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || ensure_tools(&a)).await;
    let state = app.state::<AppState>();
    let profile = {
        let s = state.settings.lock();
        let id = brain_profile_id(&s);
        s.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default()
    };
    match make_brain(&app) {
        Some(b) => BrainStatus { engine: b.label().into(), profile, fallback: b.fallback.is_some() },
        None => BrainStatus { engine: String::new(), profile, fallback: false },
    }
}

/// The user's MCP servers / claude.ai connectors (for the briefing).
#[tauri::command]
pub async fn brain_connectors(app: AppHandle) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_tools(&app);
        let state = app.state::<AppState>();
        let (tool, key) = {
            let s = state.settings.lock();
            let tools = state.tools.lock();
            let tool = agents::pick(&tools, "claude", &s.ask_runtime).cloned().ok_or("Claude Code jsem nenašla")?;
            (tool, profile_key(&s, Some(&brain_profile_id(&s))))
        };
        Ok(brain::mcp_servers(&tool, &key))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ───────────────────────────── notifications ─────────────────────────────

#[cfg_attr(not(windows), allow(dead_code))]
pub const AUMID: &str = "app.whiskr.desktop";

/// Unpackaged apps need their notification id registered once, or Windows
/// silently drops the toasts.
#[cfg(windows)]
pub fn register_toasts(icon_png: Option<std::path::PathBuf>) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    if let Ok((k, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(format!(r"Software\Classes\AppUserModelId\{AUMID}")) {
        let _ = k.set_value("DisplayName", &"Whiskr");
        if let Some(p) = icon_png {
            let _ = k.set_value("IconUri", &p.to_string_lossy().to_string());
        }
    }
}

/// A Windows notification. `buttons` = (label, action); the action comes back
/// to `on_toast`.
pub fn toast(app: &AppHandle, title: &str, body: &str, buttons: &[(&str, String)]) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::{Sound, Toast};
        let app2 = app.clone();
        let mut t = Toast::new(AUMID).title(title).text1(body).sound(Some(Sound::Reminder));
        for (label, action) in buttons {
            t = t.add_button(label, action);
        }
        let t = t.on_activated(move |action| {
            on_toast(&app2, action.as_deref());
            Ok(())
        });
        if let Err(e) = t.show() {
            eprintln!("toast failed: {e:?}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, buttons);
        eprintln!("[toast] {title}: {body}");
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn on_toast(app: &AppHandle, action: Option<&str>) {
    match action.and_then(|a| a.split_once(':')) {
        Some(("done", id)) => {
            edit_tasks(app, |v| {
                if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                    t.done = true;
                    t.done_at = Some(Local::now());
                }
            });
        }
        Some(("snooze", id)) => {
            edit_tasks(app, |v| {
                if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                    t.due = Some(Local::now() + chrono::Duration::hours(1));
                    t.has_time = true;
                    t.notified = false;
                }
            });
        }
        Some(("record", _)) => {
            if let Err(e) = crate::start_meeting(app.clone(), None) {
                bar_state(app, "error", Some(&e));
            }
        }
        _ => {
            show_main(app);
            let _ = app.emit_to("main", "whiskr:goto", "today");
        }
    }
}

// ───────────────────────────── tasks ─────────────────────────────

fn tasks_path(state: &AppState) -> std::path::PathBuf {
    state.data_dir.join("tasks.json")
}

/// Change the task list, save it, tell the UI.
pub fn edit_tasks<R>(app: &AppHandle, f: impl FnOnce(&mut Vec<Task>) -> R) -> R {
    let state = app.state::<AppState>();
    let (r, snapshot) = {
        let mut v = state.tasks.lock();
        let r = f(&mut v);
        tasks::save(&tasks_path(&state), &v);
        (r, v.clone())
    };
    let _ = app.emit_to("main", "whiskr:tasks", &snapshot);
    r
}

pub fn load_tasks(state: &AppState) {
    *state.tasks.lock() = tasks::load(&tasks_path(state));
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> Vec<Task> {
    state.tasks.lock().clone()
}

/// Add from text ("zítra v 9 zavolat Chrisovi"). With `smart`, the brain
/// splits it and reads the time; the task shows up instantly either way.
#[tauri::command]
pub fn add_task(app: AppHandle, text: String, smart: bool, source: Option<String>, source_id: Option<String>) -> Result<Task, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Prázdný úkol".into());
    }
    let mut t = Task::plain(&text, source.as_deref().unwrap_or("manual"));
    t.source_id = source_id;
    let added = t.clone();
    edit_tasks(&app, |v| v.insert(0, t));
    if smart {
        refine_task(&app, added.clone(), false);
    }
    Ok(added)
}

#[tauri::command]
pub fn update_task(app: AppHandle, task: Task) {
    edit_tasks(&app, |v| {
        if let Some(t) = v.iter_mut().find(|t| t.id == task.id) {
            let mut task = task;
            if task.done && !t.done {
                task.done_at = Some(Local::now());
            }
            if task.due != t.due {
                task.notified = task.due.map_or(false, |d| d <= Local::now());
            }
            *t = task;
        }
    });
}

#[tauri::command]
pub fn delete_task(app: AppHandle, id: String) {
    edit_tasks(&app, |v| v.retain(|t| t.id != id));
}

/// Ask the brain to read times and split the sentence; replace the raw task.
fn refine_task(app: &AppHandle, raw: Task, announce: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let a2 = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || ensure_tools(&a2)).await;
        let Some(brain) = make_brain(&app) else { return };
        let http = app.state::<AppState>().http.clone();
        let said = raw.said.clone();
        let reply = brain.json(&http, tasks::PARSE_SYSTEM, &tasks::parse_prompt(Local::now(), &said), Duration::from_secs(120)).await;
        let Ok(v) = reply else { return };
        let mut parsed = tasks::from_reply(&v, &said, &raw.source);
        for p in parsed.iter_mut() {
            p.source_id = raw.source_id.clone();
        }
        let first = parsed.first().cloned();
        let replaced = edit_tasks(&app, |list| {
            // Only if the user hasn't touched it in the meantime.
            let Some(i) = list.iter().position(|t| t.id == raw.id && t.text == raw.text && !t.done && t.due == raw.due) else { return false };
            list.splice(i..=i, parsed.clone());
            true
        });
        if replaced && announce {
            if let Some(t) = first {
                let msg = match t.due {
                    Some(d) => format!("⏰ {} — {}", tasks::when_label(d, t.has_time, Local::now()), t.text),
                    None => format!("✓ {}", t.text),
                };
                bar_state(&app, "notify", Some(&msg));
            }
        }
    });
}

/// Voice: Alt+Shift+T, say the task.
pub async fn process_task(app: &AppHandle, clip: recorder::Clip) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    if clip.seconds() < 0.3 || clip.peak() < 0.004 {
        return Ok(None);
    }
    let (key, language, dictionary) = {
        let s = state.settings.lock();
        (s.api_key.clone(), s.language.clone(), s.dictionary.clone())
    };
    let wav = recorder::to_wav_16k(&clip)?;
    let said = groq::transcribe(&state.http, &key, wav, &language, &dictionary).await?;
    if said.trim().is_empty() || groq::is_hallucination(&said) {
        return Ok(None);
    }
    let t = Task::plain(said.trim().trim_end_matches('.'), "voice");
    let shown = t.text.clone();
    edit_tasks(app, |v| v.insert(0, t.clone()));
    refine_task(app, t, true);
    Ok(Some(shown))
}

// ───────────────────────────── background loop ─────────────────────────────

#[derive(Default)]
pub struct Watch {
    /// Call currently going on (app label).
    call: Option<String>,
    on_polls: u32,
    off_polls: u32,
    offered: bool,
    /// The running recording was started during this call → stop with it.
    auto_rec: bool,
    /// Offer shown on the bar right now.
    pub offer: Option<String>,
    /// Briefing attempts today: (date, count, last try)
    brief_tries: (String, u32, Option<chrono::DateTime<Local>>),
    brief_running: bool,
}

pub fn start_loop(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        let mut tick: u64 = 0;
        loop {
            if tick % 4 == 0 {
                reminders(&app);
                maybe_briefing(&app);
            }
            watch_calls(&app);
            tick += 1;
            std::thread::sleep(Duration::from_secs(4));
        }
    });
}

fn reminders(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Local::now();
    let need = state.tasks.lock().iter().any(|t| !t.done && !t.notified && t.due.map_or(false, |d| d <= now));
    if !need {
        return;
    }
    let fired = edit_tasks(app, |v| tasks::due_now(v, now));
    for t in fired {
        bar_state(app, "notify", Some(&format!("⏰ {}", t.text)));
        toast(app, "⏰ Připomínka", &t.text, &[("✓ Hotovo", format!("done:{}", t.id)), ("Za hodinu", format!("snooze:{}", t.id))]);
    }
}

// ───────────────────────────── calls ─────────────────────────────

fn watch_calls(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (on, autostop) = {
        let s = state.settings.lock();
        (s.meeting_detect, s.meeting_autostop)
    };
    if !on {
        return;
    }
    let cur = calls::current();
    let recording = state.meeting.lock().is_some();
    let paused = *state.paused.lock();
    let mut w = state.watch.lock();
    match cur {
        Some(label) => {
            w.off_polls = 0;
            if w.call.as_deref() != Some(&label) {
                w.call = Some(label.clone());
                w.on_polls = 0;
                w.offered = false;
            }
            w.on_polls += 1;
            if recording && !w.auto_rec && w.on_polls == 1 {
                // Already recording when the call started: end together.
                w.auto_rec = true;
            }
            // ~8 s of call before asking (no popups for a quick mic check).
            if w.on_polls == 2 && !w.offered && !recording && !paused {
                w.offered = true;
                w.offer = Some(label.clone());
                drop(w);
                show_offer(app, &label);
            }
        }
        None => {
            if w.call.is_none() {
                return;
            }
            w.off_polls += 1;
            if w.off_polls < 3 {
                return;
            }
            // Call over (~12 s without it).
            let stop = w.auto_rec && recording && autostop;
            let had_offer = w.offer.take().is_some();
            let label = w.call.take().unwrap_or_default();
            *w = Watch { brief_tries: std::mem::take(&mut w.brief_tries), brief_running: w.brief_running, ..Default::default() };
            drop(w);
            if had_offer {
                bar_interactive(app, false);
                bar_state(app, "hide", None);
            }
            if stop {
                match crate::stop_meeting(app.clone()) {
                    Ok(()) => bar_state(app, "notify", Some(&format!("{label} skončil — nahrávka se zpracovává"))),
                    Err(e) => bar_state(app, "error", Some(&e)),
                }
            }
        }
    }
}

fn bar_interactive(app: &AppHandle, on: bool) {
    if let Some(bar) = app.get_webview_window("bar") {
        let _ = bar.set_ignore_cursor_events(!on);
    }
}

fn show_offer(app: &AppHandle, label: &str) {
    crate::place_bar(app);
    bar_interactive(app, true);
    let state = app.state::<AppState>();
    let (sounds, style) = {
        let s = state.settings.lock();
        (s.sounds, s.bar_style.clone())
    };
    let _ = app.emit_to("bar", "whiskr:bar", json!({ "phase": "offer", "message": label, "sounds": sounds, "style": style }));
}

/// Recording started (from the offer, the shortcut or the tray).
pub fn on_meeting_started(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut w = state.watch.lock();
    let had_offer = w.offer.take().is_some();
    w.auto_rec = w.call.is_some();
    drop(w);
    if had_offer {
        bar_interactive(app, false);
    }
}

pub fn on_meeting_stopped(app: &AppHandle) {
    app.state::<AppState>().watch.lock().auto_rec = false;
}

#[tauri::command]
pub fn offer_answer(app: AppHandle, accept: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let had = state.watch.lock().offer.take().is_some();
    bar_interactive(&app, false);
    if !had {
        return Ok(());
    }
    if accept {
        let r = crate::start_meeting(app.clone(), None).map(|_| ());
        if let Err(e) = &r {
            bar_state(&app, "error", Some(e));
        }
        r
    } else {
        bar_state(&app, "hide", None);
        Ok(())
    }
}

// ───────────────────────────── morning briefing ─────────────────────────────

fn briefing_path(state: &AppState) -> std::path::PathBuf {
    state.data_dir.join("briefing.json")
}

#[tauri::command]
pub fn get_briefing(state: State<'_, AppState>) -> Option<Briefing> {
    briefing::load(&briefing_path(&state))
}

#[tauri::command]
pub fn refresh_briefing(app: AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut w = state.watch.lock();
        if w.brief_running {
            return Err("Briefing se právě připravuje".into());
        }
        w.brief_running = true;
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move { generate_briefing(&a, true).await });
    Ok(())
}

fn maybe_briefing(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (on, at) = {
        let s = state.settings.lock();
        (s.briefing, s.briefing_time.clone())
    };
    if !on {
        return;
    }
    let now = Local::now();
    let at = NaiveTime::parse_from_str(&at, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(8, 0, 0).unwrap());
    // Not before the time, and not late in the evening (PC turned on at 22:00).
    if now.time() < at || now.hour() >= 20 {
        return;
    }
    let today = now.format("%Y-%m-%d").to_string();
    if briefing::load(&briefing_path(&state)).map_or(false, |b| b.date == today && b.error.is_none()) {
        return;
    }
    {
        let mut w = state.watch.lock();
        if w.brief_running {
            return;
        }
        if w.brief_tries.0 != today {
            w.brief_tries = (today.clone(), 0, None);
        }
        let (_, n, last) = &w.brief_tries;
        // Up to 3 tries, 10 minutes apart (network may not be up right after boot).
        if *n >= 3 || last.map_or(false, |l| now - l < chrono::Duration::minutes(10)) {
            return;
        }
        w.brief_tries.1 += 1;
        w.brief_tries.2 = Some(now);
        w.brief_running = true;
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move { generate_briefing(&a, false).await });
}

async fn generate_briefing(app: &AppHandle, manual: bool) {
    let _ = app.emit_to("main", "whiskr:briefing-busy", true);
    let a2 = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || ensure_tools(&a2)).await;
    let state = app.state::<AppState>();
    let now = Local::now();
    let tasks_now = state.tasks.lock().clone();
    let root = state.recordings_dir.clone();
    let recent: Vec<notes::Recording> = tauri::async_runtime::spawn_blocking(move || {
        ask::load_all(&root).into_iter().filter(|r| now - r.created < chrono::Duration::days(3)).take(6).collect()
    })
    .await
    .unwrap_or_default();
    let (extra, all_tools, chosen) = {
        let s = state.settings.lock();
        (s.briefing_extra.clone(), s.briefing_all_tools, s.briefing_tools.clone())
    };
    let mut b = Briefing { date: now.format("%Y-%m-%d").to_string(), generated: Some(now), ..Default::default() };
    match make_brain(app) {
        Some(brain) => {
            let claude_tool = match &brain.primary {
                Engine::Claude { tool, profile_key, .. } => Some((tool.clone(), profile_key.clone())),
                _ => None,
            };
            let access = match claude_tool {
                Some((tool, key)) => {
                    let servers = if all_tools {
                        tauri::async_runtime::spawn_blocking(move || brain::mcp_servers(&tool, &key)).await.unwrap_or_default()
                    } else {
                        chosen
                    };
                    let mut rules: Vec<String> = servers.iter().map(|s| brain::mcp_rule(s)).collect();
                    rules.extend(["WebSearch".to_string(), "WebFetch".to_string()]);
                    Access { tools: Some(rules) }
                }
                None => Access::default(),
            };
            let with_tools = access.tools.is_some();
            let prompt = briefing::briefing_prompt(now, &tasks_now, &recent, with_tools, &extra);
            match brain.text(&state.http, briefing::BRIEFING_SYSTEM, &prompt, &access, Duration::from_secs(8 * 60)).await {
                Ok((text, engine)) => {
                    b.text = text.trim().to_string();
                    b.engine = engine.to_string();
                }
                Err(e) => {
                    b.text = briefing::plain_briefing(now, &tasks_now);
                    b.error = Some(e);
                }
            }
        }
        None => b.text = briefing::plain_briefing(now, &tasks_now),
    }
    briefing::save(&briefing_path(&state), &b);
    state.watch.lock().brief_running = false;
    let _ = app.emit_to("main", "whiskr:briefing", &b);
    let _ = app.emit_to("main", "whiskr:briefing-busy", false);
    if !manual {
        let open = state.settings.lock().briefing_open;
        toast(app, "☀️ Ranní briefing", "Je připravený — klikni pro zobrazení.", &[]);
        if open && b.error.is_none() {
            show_main(app);
            let _ = app.emit_to("main", "whiskr:goto", "today");
        }
    }
}

// ───────────────────────────── brief before a meeting ─────────────────────────────

#[derive(serde::Serialize)]
pub struct BriefMaterial {
    prompt: String,
    count: usize,
    titles: Vec<String>,
}

#[tauri::command]
pub async fn brief_material(app: AppHandle, query: String) -> Result<BriefMaterial, String> {
    let state = app.state::<AppState>();
    let root = state.recordings_dir.clone();
    let q = query.clone();
    let recs = tauri::async_runtime::spawn_blocking(move || briefing::related(&ask::load_all(&root), &q, 6))
        .await
        .map_err(|e| e.to_string())?;
    let tasks_now = state.tasks.lock().clone();
    if recs.is_empty() {
        return Err(format!("O „{query}“ zatím nemám žádné nahrávky. Zkus jiné slovo (jméno, firmu, projekt)."));
    }
    let titles = recs.iter().map(|r| format!("{} · {}", r.created.format("%-d. %-m."), r.title)).collect();
    Ok(BriefMaterial { prompt: briefing::brief_prompt(&query, &recs, &tasks_now, Local::now()), count: recs.len(), titles })
}

pub type WatchState = Mutex<Watch>;
