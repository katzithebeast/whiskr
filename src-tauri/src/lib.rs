#[macro_use]
mod dlog;
mod groq;
mod selection;
#[cfg(windows)]
mod screen;
mod updater;
mod history;
mod meeting;
mod notes;
mod paste;
mod recorder;
mod agents;
mod ask;
mod assistant;
mod brain;
mod briefing;
mod calls;
mod mcp_hub;
mod tasks;
mod context;
mod export;
mod templates;
#[cfg(windows)]
mod hotkeys_win;
mod keyspec;
mod settings;
mod voices;

use history::History;
use parking_lot::Mutex;
use recorder::Recorder;
use serde_json::json;
use settings::{Polish, Settings};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt as _;
#[allow(unused_imports)]
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// A press shorter than this switches to hands-free mode instead of hold-to-talk.
const TAP_THRESHOLD: Duration = Duration::from_millis(350);
/// Safety cap for a single dictation (Groq accepts up to 25 MB ≈ 13 min of 16 kHz audio).
const MAX_RECORDING: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Hold,
    HandsFree,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Dictate,
    Command,
    Task,
    /// Hold Shift+Backspace: speak straight into the quick chat.
    Ask,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    Recording { mode: Mode, started: Instant },
    Processing,
}

pub struct AppState {
    phase: Mutex<Phase>,
    recorder: Recorder,
    settings: Mutex<Settings>,
    history: Mutex<History>,
    paused: Mutex<bool>,
    /// action ("dictate" | "command" | "record" | "paste") → registered shortcut
    hotkeys: Mutex<Vec<(String, Shortcut)>>,
    /// What the current voice capture is for.
    kind: Mutex<Kind>,
    /// The app the user was typing into when they pressed the key.
    target: Mutex<context::AppInfo>,
    pause_item: Mutex<Option<MenuItem<Wry>>>,
    lock_item: Mutex<Option<MenuItem<Wry>>>,
    http: reqwest::Client,
    settings_path: PathBuf,
    history_path: PathBuf,
    recordings_dir: PathBuf,
    meeting: Mutex<Option<meeting::Session>>,
    /// Seconds marked important in the running meeting.
    bookmarks: Mutex<Vec<f64>>,
    rec_item: Mutex<Option<MenuItem<Wry>>>,
    /// Whiskr Ask: installed CLIs, the window we came from, run ids.
    tools: Mutex<Vec<agents::Tool>>,
    ask_target: Mutex<isize>,
    /// The window that had the keyboard before the chat opened — it gets it
    /// back when the chat closes.
    ask_return: Mutex<isize>,
    run_seq: std::sync::atomic::AtomicU64,
    data_dir: PathBuf,
    /// Tasks & reminders.
    tasks: Mutex<Vec<tasks::Task>>,
    /// When Shift+Space last opened Ask (for the Shift+Space+R chord).
    ask_just_opened: Mutex<Option<Instant>>,
    /// Call detection, the recording offer, briefing bookkeeping.
    watch: assistant::WatchState,
}

/// Let Esc cancel a running dictation (and only then).
fn escape_active(app: &AppHandle, on: bool) {
    #[cfg(windows)]
    {
        let _ = app;
        hotkeys_win::set_escape(on);
    }
    #[cfg(not(windows))]
    {
        let gs = app.global_shortcut();
        let _ = if on { gs.register(escape()) } else { gs.unregister(escape()) };
    }
}

fn escape() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

fn paste_last(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(*state.phase.lock(), Phase::Idle) {
        return;
    }
    let Some(text) = state.history.lock().items.first().map(|e| e.text.clone()) else {
        bar_state(app, "empty", Some("Zatím nemám co vložit"));
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give the user a moment to let go of Alt+Shift.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let text = paste::with_trailing_space(&text);
        match tauri::async_runtime::spawn_blocking(move || paste::insert(&text)).await {
            Ok(Ok(())) => bar_state(&app, "done", None),
            Ok(Err(e)) => bar_state(&app, "error", Some(&e)),
            Err(e) => bar_state(&app, "error", Some(&e.to_string())),
        }
    });
}

// ───────────────────────────── overlay ─────────────────────────────

fn bar_state(app: &AppHandle, phase: &str, message: Option<&str>) {
    let state = app.state::<AppState>();
    let (sounds, style) = {
        let s = state.settings.lock();
        (s.sounds, s.bar_style.clone())
    };
    // Separate event names: a global `listen` in a webview receives events
    // emitted to *any* window, so the bar would also get the main window's copy.
    let _ = app.emit_to(
        "bar",
        "whiskr:bar",
        json!({ "phase": phase, "message": message, "sounds": sounds, "style": style }),
    );
    let _ = app.emit_to("main", "whiskr:state", json!({ "phase": phase, "message": message }));
}

/// Put the bar at the bottom centre of whichever screen the mouse is on.
fn place_bar(app: &AppHandle) {
    let Some(bar) = app.get_webview_window("bar") else { return };
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| bar.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, bar.outer_size()) else { return };
    let wa = m.work_area();
    let x = wa.position.x + (wa.size.width as i32 - size.width as i32) / 2;
    let y = wa.position.y + wa.size.height as i32 - size.height as i32 - (8.0 * m.scale_factor()) as i32;
    let _ = bar.set_position(PhysicalPosition::new(x, y));
}

/// The bar must never take focus (the text goes into the app you're typing in)
/// and must not show up in Alt+Tab.
#[cfg(windows)]
fn make_bar_passive(bar: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    if let Ok(h) = bar.hwnd() {
        let hwnd = h.0 as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = (ex | (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as isize) & !(WS_EX_APPWINDOW as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        }
    }
}
#[cfg(not(windows))]
fn make_bar_passive(_bar: &tauri::WebviewWindow) {}

// ───────────────────────────── dictation flow ─────────────────────────────

fn on_hotkey_down(app: &AppHandle, kind: Kind) {
    let state = app.state::<AppState>();
    let phase = *state.phase.lock();
    match phase {
        Phase::Idle => {
            if *state.paused.lock() {
                return;
            }
            if state.settings.lock().api_key.trim().is_empty() {
                bar_state(app, "error", Some("Nejdřív vlož Groq klíč v nastavení"));
                show_main(app);
                return;
            }
            *state.kind.lock() = kind;
            *state.target.lock() = context::foreground();
            start_recording(app);
        }
        Phase::Recording { mode: Mode::HandsFree, .. } => finish_recording(app),
        // Key auto-repeat while holding, or still processing: ignore.
        _ => {}
    }
}

fn on_hotkey_up(app: &AppHandle) {
    let state = app.state::<AppState>();
    let phase = *state.phase.lock();
    if let Phase::Recording { mode: Mode::Hold, started } = phase {
        if started.elapsed() < TAP_THRESHOLD && *state.kind.lock() != Kind::Ask {
            // Quick tap → hands-free. Tap again to finish, Esc to cancel.
            *state.phase.lock() = Phase::Recording { mode: Mode::HandsFree, started };
            bar_state(app, "handsfree", (*state.kind.lock() == Kind::Command).then_some("command"));
        } else {
            finish_recording(app);
        }
    }
}

fn start_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mic = state.settings.lock().microphone.clone();
    let kind = *state.kind.lock();
    if kind != Kind::Ask {
        place_bar(app);
    }
    if let Err(e) = state.recorder.start(mic) {
        if kind == Kind::Ask {
            let _ = app.emit_to("ask", "whiskr:ask-voice", json!({ "state": "error", "text": e }));
        } else {
            bar_state(app, "error", Some(&e));
        }
        return;
    }
    let started = Instant::now();
    *state.phase.lock() = Phase::Recording { mode: Mode::Hold, started };
    escape_active(app, true);
    if kind == Kind::Ask {
        // the chat itself shows that it's listening (no second bar on screen)
        let _ = app.emit_to("ask", "whiskr:ask-voice", json!({ "state": "listening" }));
    } else {
        let phase = if kind == Kind::Command { "command" } else { "listening" };
        bar_state(app, phase, (kind == Kind::Task).then_some("task"));
    }

    // Safety cap so a forgotten hands-free session doesn't run forever.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(MAX_RECORDING).await;
        let still_same = matches!(
            *app2.state::<AppState>().phase.lock(),
            Phase::Recording { started: s, .. } if s == started
        );
        if still_same {
            finish_recording(&app2);
        }
    });
}

fn cancel_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    if matches!(*state.phase.lock(), Phase::Recording { .. }) {
        state.recorder.cancel();
        *state.phase.lock() = Phase::Idle;
        escape_active(app, false);
        if *state.kind.lock() == Kind::Ask {
            let _ = app.emit_to("ask", "whiskr:ask-voice", json!({ "state": "cancel" }));
        } else {
            bar_state(app, "cancelled", None);
        }
    }
}

fn finish_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut phase = state.phase.lock();
        if !matches!(*phase, Phase::Recording { .. }) {
            return;
        }
        *phase = Phase::Processing;
    }
    escape_active(app, false);
    let clip = state.recorder.stop();
    let kind = *state.kind.lock();
    let target = state.target.lock().clone();
    let tone_on = state.settings.lock().app_tone;
    let cat = if tone_on { context::category(&target) } else { "other" };
    if kind == Kind::Ask {
        let _ = app.emit_to("ask", "whiskr:ask-voice", json!({ "state": "thinking" }));
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let r = ask_voice_text(&app, clip).await;
            *app.state::<AppState>().phase.lock() = Phase::Idle;
            let payload = match r {
                Ok(Some(t)) => json!({ "state": "text", "text": t }),
                Ok(None) => json!({ "state": "empty" }),
                Err(e) => json!({ "state": "error", "text": e }),
            };
            dlog!("ask voice: {}", payload["state"]);
            let _ = app.emit_to("ask", "whiskr:ask-voice", payload);
        });
        return;
    }
    bar_state(app, "thinking", Some(match kind {
        Kind::Command => "command",
        Kind::Task => "task",
        Kind::Dictate => cat,
        Kind::Ask => "other",
    }));

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match kind {
            Kind::Command => process_command(&app, clip, cat).await,
            Kind::Task => assistant::process_task(&app, clip).await,
            Kind::Dictate => process(&app, clip, cat).await,
            Kind::Ask => Ok(None),
        };
        let state = app.state::<AppState>();
        *state.phase.lock() = Phase::Idle;
        match result {
            Ok(Some(t)) if kind == Kind::Task => bar_state(&app, "notify", Some(&format!("✓ Úkol: {t}"))),
            Ok(Some(_)) => bar_state(&app, "done", None),
            Ok(None) => bar_state(&app, "empty", Some("Nic jsem neslyšela 🙉")),
            Err(e) => bar_state(&app, "error", Some(&e)),
        }
    });
}

/// What was said into the chat (plain transcript, with the user's dictionary).
async fn ask_voice_text(app: &AppHandle, clip: recorder::Clip) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let (seconds, peak) = (clip.seconds(), clip.peak());
    if seconds < 0.3 || peak < 0.004 {
        return Ok(None);
    }
    let (key, language, dictionary) = {
        let s = state.settings.lock();
        (s.api_key.clone(), s.language.clone(), s.dictionary.clone())
    };
    let wav = recorder::to_wav_16k(&clip)?;
    let raw = groq::transcribe(&state.http, &key, wav, &language, &dictionary).await?;
    if raw.trim().is_empty() || (groq::is_hallucination(&raw) && (seconds < 4.0 || peak < 0.08)) {
        return Ok(None);
    }
    Ok(Some(raw.trim().to_string()))
}

async fn process(app: &AppHandle, clip: recorder::Clip, cat: &str) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let seconds = clip.seconds();
    let peak = clip.peak();
    if seconds < 0.3 || peak < 0.004 {
        return Ok(None);
    }

    let (key, language, polish, dictionary) = {
        let s = state.settings.lock();
        (s.api_key.clone(), s.language.clone(), s.polish, s.dictionary.clone())
    };

    let wav = recorder::to_wav_16k(&clip)?;
    let raw = groq::transcribe(&state.http, &key, wav, &language, &dictionary).await?;
    if raw.trim().is_empty() || (groq::is_hallucination(&raw) && (seconds < 4.0 || peak < 0.08)) {
        return Ok(None);
    }

    let text = if polish == Polish::Off {
        raw.clone()
    } else {
        match groq::polish(&state.http, &key, &raw, polish, &dictionary, context::tone_rules(cat)).await {
            Ok(t) if !t.trim().is_empty() => t,
            // Cleanup is a bonus — never lose the dictation because of it.
            _ => raw.clone(),
        }
    };

    let to_insert = paste::with_trailing_space(&text);
    tauri::async_runtime::spawn_blocking(move || paste::insert(&to_insert))
        .await
        .map_err(|e| e.to_string())??;

    let entry = {
        let mut h = state.history.lock();
        let e = h.push(text.clone(), raw, seconds);
        h.save(&state.history_path);
        e
    };
    let _ = app.emit_to("main", "whiskr:history", &entry);
    Ok(Some(text))
}

/// Command mode: take the selected text (if any), apply the spoken instruction, paste the result.
async fn process_command(app: &AppHandle, clip: recorder::Clip, cat: &str) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let seconds = clip.seconds();
    if seconds < 0.3 || clip.peak() < 0.004 {
        return Ok(None);
    }
    // Grab the selection first, while it's still selected in the target app.
    let uia = tauri::async_runtime::spawn_blocking(selection::focused_selection).await.ok().flatten();
    let (selection, original) = match uia {
        Some(t) => (Some(t), arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())),
        None => tauri::async_runtime::spawn_blocking(paste::read_selection)
            .await
            .map_err(|e| e.to_string())??,
    };
    let restore = original.clone();
    let result: Result<Option<String>, String> = async {
        let (key, language, dictionary) = {
            let s = state.settings.lock();
            (s.api_key.clone(), s.language.clone(), s.dictionary.clone())
        };
        let wav = recorder::to_wav_16k(&clip)?;
        let instruction = groq::transcribe(&state.http, &key, wav, &language, &dictionary).await?;
        if instruction.trim().is_empty() || groq::is_hallucination(&instruction) {
            return Ok(None);
        }
        let out = groq::command(&state.http, &key, &instruction, selection.as_deref(), context::tone_rules(cat), &dictionary).await?;
        let to_insert = out.clone();
        tauri::async_runtime::spawn_blocking(move || paste::insert_restoring(&to_insert, original))
            .await
            .map_err(|e| e.to_string())??;
        let entry = {
            let mut h = state.history.lock();
            let e = h.push(out.clone(), format!("⌘ {}", instruction.trim()), seconds);
            h.save(&state.history_path);
            e
        };
        let _ = app.emit_to("main", "whiskr:history", &entry);
        Ok(Some(out))
    }
    .await;
    if !matches!(result, Ok(Some(_))) {
        // Nothing was pasted: put the user's clipboard back.
        paste::set_clipboard(restore);
    }
    result
}

// ───────────────────────────── windows & tray ─────────────────────────────

static MAIN_WAIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called by the main page once its first frame (the intro overlay) is on screen.
#[tauri::command]
fn main_ready(app: AppHandle) {
    if MAIN_WAIT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        show_main(&app);
    }
}

fn show_main(app: &AppHandle) {
    MAIN_WAIT.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() {
            no_dwm_border(h.0 as isize);
        }
        let _ = w.set_focus();
        let _ = app.emit_to("main", "whiskr:main-shown", ());
    }
}

// ─── lock for gaming ───

fn keys_locked() -> bool {
    #[cfg(windows)]
    return hotkeys_win::is_locked();
    #[cfg(not(windows))]
    false
}

fn lock_status() -> serde_json::Value {
    #[cfg(windows)]
    return json!({ "locked": hotkeys_win::is_locked(), "game": hotkeys_win::game_in_front() });
    #[cfg(not(windows))]
    json!({ "locked": false, "game": false })
}

fn set_locked(app: &AppHandle, on: bool) {
    #[cfg(windows)]
    hotkeys_win::set_locked(on);
    dlog!("keys locked: {on}");
    if let Some(item) = app.state::<AppState>().lock_item.lock().as_ref() {
        let _ = item.set_text(if on { "🔓 Odemknout zkratky" } else { "🔒 Zamknout zkratky (hraní)" });
    }
    bar_state(app, "notify", Some(if on { "🔒 Zkratky zamčené — Ctrl+Alt+Shift+L odemkne" } else { "🔓 Zkratky odemčené" }));
    let _ = app.emit_to("main", "whiskr:lock", lock_status());
}

#[tauri::command]
fn get_lock() -> serde_json::Value {
    lock_status()
}

#[tauri::command]
fn toggle_lock(app: AppHandle) -> serde_json::Value {
    set_locked(&app, !keys_locked());
    lock_status()
}

fn set_paused(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    *state.paused.lock() = paused;
    if let Some(item) = state.pause_item.lock().as_ref() {
        let _ = item.set_text(if paused { "Spustit Whiskr" } else { "Pozastavit" });
    }
    if let Some(tray) = app.tray_by_id("whiskr") {
        let _ = tray.set_tooltip(Some(if paused { "Whiskr — pozastaveno" } else { "Whiskr" }));
    }
    let _ = app.emit_to("main", "whiskr:paused", paused);
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Otevřít Whiskr", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pozastavit", true, None::<&str>)?;
    let rec = MenuItem::with_id(app, "record", "Nahrávat schůzku", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Ukončit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let lock = MenuItem::with_id(app, "lock", "🔒 Zamknout zkratky (hraní)", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Zkontrolovat aktualizace", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &rec, &sep, &lock, &pause, &sep2, &update, &quit])?;
    *app.state::<AppState>().pause_item.lock() = Some(pause);
    *app.state::<AppState>().lock_item.lock() = Some(lock);
    *app.state::<AppState>().rec_item.lock() = Some(rec);

    TrayIconBuilder::with_id("whiskr")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Whiskr")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "lock" => set_locked(app, !keys_locked()),
            "update" => {
                show_main(app);
                let _ = app.emit_to("main", "whiskr:update-check", ());
            }
            "pause" => {
                let paused = *app.state::<AppState>().paused.lock();
                set_paused(app, !paused);
            }
            "record" => {
                toggle_meeting(app);
                show_main(app);
                let _ = app.emit_to("main", "whiskr:goto", "recordings");
            }
            "quit" => {
                // Don't lose a running recording: finish it first.
                if app.state::<AppState>().meeting.lock().is_some() {
                    let _ = stop_meeting(app.clone());
                }
                app.exit(0)
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn hotkey_specs(s: &Settings) -> [(&'static str, String); 7] {
    [
        ("task", s.hotkey_task.clone()),
        ("ask", s.hotkey_ask.clone()),
        ("dictate", s.hotkey.clone()),
        ("command", s.hotkey_command.clone()),
        ("record", s.hotkey_record.clone()),
        ("paste", s.hotkey_paste.clone()),
        ("bookmark", s.hotkey_bookmark.clone()),
    ]
}

fn action_name(a: &str) -> &'static str {
    match a {
        "dictate" => "Diktování",
        "command" => "Command mode",
        "record" => "Nahrávání",
        "bookmark" => "Záložka",
        "ask" => "Whiskr Ask",
        "task" => "Úkol",
        _ => "Vložit znovu",
    }
}

/// (Re)register every shortcut.
#[cfg(windows)]
fn register_hotkeys(_app: &AppHandle, s: &Settings) -> Result<(), String> {
    // Our own keyboard hook: no conflicts with other apps, layout-independent.
    let mut bindings: Vec<keyspec::Binding> = vec![];
    for (action, spec) in hotkey_specs(s) {
        if spec.trim().is_empty() {
            continue;
        }
        let b = keyspec::parse(action, &spec).map_err(|e| format!("{}: {e}", action_name(action)))?;
        if let Some(other) = bindings.iter().find(|x| x.mods == b.mods && x.scan == b.scan && x.vk == b.vk) {
            return Err(format!("{} a {} mají stejnou zkratku", action_name(&other.id), action_name(action)));
        }
        bindings.push(b);
    }
    hotkeys_win::set_bindings(bindings);
    Ok(())
}

/// (Re)register every shortcut. On a problem, the ones that work stay registered.
#[cfg(not(windows))]
fn register_hotkeys(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let gs = app.global_shortcut();
    let mut parsed: Vec<(String, Shortcut)> = vec![];
    for (action, spec) in hotkey_specs(s) {
        if spec.trim().is_empty() {
            continue;
        }
        let sc: Shortcut = spec.parse().map_err(|e| format!("{}: neplatná zkratka „{spec}“ ({e})", action_name(action)))?;
        if let Some((other, _)) = parsed.iter().find(|(_, x)| *x == sc) {
            return Err(format!("{} a {} mají stejnou zkratku", action_name(other), action_name(action)));
        }
        parsed.push((action.to_string(), sc));
    }
    for (_, old) in state.hotkeys.lock().drain(..) {
        let _ = gs.unregister(old);
    }
    let mut failed = vec![];
    let mut ok = vec![];
    for (action, sc) in parsed {
        match gs.register(sc) {
            Ok(()) => ok.push((action, sc)),
            Err(_) => failed.push(action_name(&action)),
        }
    }
    *state.hotkeys.lock() = ok;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("Zkratku pro {} už používá jiná aplikace — vyber jinou", failed.join(", ")))
    }
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, pressed: bool) {
    if *shortcut == escape() {
        on_action(app, "escape", pressed);
        return;
    }
    let action = app
        .state::<AppState>()
        .hotkeys
        .lock()
        .iter()
        .find(|(_, s)| s == shortcut)
        .map(|(a, _)| a.clone());
    if let Some(a) = action {
        on_action(app, &a, pressed);
    }
}

fn on_action(app: &AppHandle, action: &str, pressed: bool) {
    match (Some(action), pressed) {
        (Some("escape"), true) => cancel_recording(app),
        (Some("dictate"), true) => on_hotkey_down(app, Kind::Dictate),
        (Some("command"), true) => on_hotkey_down(app, Kind::Command),
        (Some("task"), true) => on_hotkey_down(app, Kind::Task),
        (Some("dictate") | Some("command") | Some("task"), false) => on_hotkey_up(app),
        (Some("record"), true) => toggle_meeting(app),
        (Some("paste"), true) => paste_last(app),
        (Some("ask"), true) => toggle_ask(app),
        (Some("ask_new"), true) => {
            let swap = app.state::<AppState>().settings.lock().ask_space_new;
            open_ask(app, !swap, true)
        }
        (Some("ask_hide"), true) => close_ask(app),
        (Some("lock_toggle"), true) => {
            set_locked(app, !keys_locked());
        }
        (Some("ask_voice"), true) => ask_voice_down(app),
        (Some("ask_voice"), false) => ask_voice_up(app),
        (Some("bookmark"), true) => {
            if let Err(e) = add_bookmark(app.clone()) {
                bar_state(app, "error", Some(&e));
            }
        }
        _ => {}
    }
}

// ─────────────── Shift+Backspace: tap = close / backspace, hold = talk ───────────────

const VOICE_HOLD: Duration = Duration::from_millis(280);
static VOICE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static VOICE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ask_voice_down(app: &AppHandle) {
    let gen = VOICE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    VOICE_ON.store(false, std::sync::atomic::Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(VOICE_HOLD);
        if VOICE_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen {
            return; // released already = a tap
        }
        let state = app.state::<AppState>();
        if *state.paused.lock() || !matches!(*state.phase.lock(), Phase::Idle) {
            return;
        }
        if state.settings.lock().api_key.trim().is_empty() {
            bar_state(&app, "error", Some("Nejdřív vlož Groq klíč v nastavení"));
            return;
        }
        VOICE_ON.store(true, std::sync::atomic::Ordering::SeqCst);
        dlog!("ask voice: hold → listening");
        // the chat appears (as for Shift+Space) and listens right away
        if let Some(w) = app.get_webview_window("ask") {
            if !w.is_visible().unwrap_or(false) {
                open_ask(&app, false, false);
            }
        }
        *state.kind.lock() = Kind::Ask;
        start_recording(&app);
    });
}

fn ask_voice_up(app: &AppHandle) {
    VOICE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if VOICE_ON.swap(false, std::sync::atomic::Ordering::SeqCst) {
        on_hotkey_up(app);
        return;
    }
    // a tap: close the chat when it's in front, otherwise give the app its backspace back
    #[cfg(windows)]
    {
        if hotkeys_win::ask_in_front() {
            dlog!("Shift+Backspace tap → close chat");
            close_ask(app);
        } else {
            hotkeys_win::send_backspace();
        }
    }
}

// ───────────────────────────── commands ─────────────────────────────

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let old = state.settings.lock().clone();
    if hotkey_specs(&old) != hotkey_specs(&settings) {
        if let Err(e) = register_hotkeys(&app, &settings) {
            let _ = register_hotkeys(&app, &old);
            return Err(e);
        }
    }
    if old.autostart != settings.autostart {
        let al = app.autolaunch();
        let r = if settings.autostart { al.enable() } else { al.disable() };
        r.map_err(|e| format!("Automatické spuštění: {e}"))?;
    }
    settings.save(&state.settings_path)?;
    dlog::set_enabled(settings.debug_log);
    #[cfg(windows)]
    hotkeys_win::set_games(settings.game_apps.clone());
    *state.settings.lock() = settings.clone();
    Ok(settings)
}

#[tauri::command]
async fn check_key(state: State<'_, AppState>, key: String) -> Result<(), String> {
    groq::check_key(&state.http, &key).await
}

#[tauri::command]
fn get_history(state: State<'_, AppState>) -> History {
    state.history.lock().clone()
}

#[tauri::command]
fn delete_entry(state: State<'_, AppState>, id: u64) {
    let mut h = state.history.lock();
    h.items.retain(|e| e.id != id);
    h.save(&state.history_path);
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) {
    let mut h = state.history.lock();
    h.items.clear();
    h.save(&state.history_path);
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    paste::copy(&text)
}

#[tauri::command]
fn list_mics() -> Vec<String> {
    recorder::list_inputs()
}

#[tauri::command]
fn get_paused(state: State<'_, AppState>) -> bool {
    *state.paused.lock()
}

#[tauri::command]
fn toggle_paused(app: AppHandle) -> bool {
    let paused = !*app.state::<AppState>().paused.lock();
    set_paused(&app, paused);
    paused
}

/// While the settings screen is capturing a new shortcut, the old ones must not fire.
#[tauri::command]
fn suspend_hotkey(app: AppHandle, suspend: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = &app;
        hotkeys_win::set_suspended(suspend);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let state = app.state::<AppState>();
        let gs = app.global_shortcut();
        if suspend {
            for (_, s) in state.hotkeys.lock().iter() {
                let _ = gs.unregister(*s);
            }
            Ok(())
        } else {
            let settings = state.settings.lock().clone();
            register_hotkeys(&app, &settings)
        }
    }
}

/// Show the bar for a moment so the user sees the chosen style.
#[tauri::command]
fn preview_bar(app: AppHandle) {
    place_bar(&app);
    bar_state(&app, "preview", None);
}

// ───────────────────────────── meeting recorder ─────────────────────────────

fn set_recording_ui(app: &AppHandle, on: bool) {
    let state = app.state::<AppState>();
    if let Some(item) = state.rec_item.lock().as_ref() {
        let _ = item.set_text(if on { "⏹ Zastavit nahrávání" } else { "Nahrávat schůzku" });
    }
    if let Some(tray) = app.tray_by_id("whiskr") {
        let _ = tray.set_tooltip(Some(if on { "Whiskr — nahrává schůzku" } else { "Whiskr" }));
    }
}

#[derive(serde::Serialize, Clone)]
struct MeetingStatus {
    recording: bool,
    seconds: f64,
    has_system: bool,
}

#[tauri::command]
fn meeting_status(state: State<'_, AppState>) -> MeetingStatus {
    match state.meeting.lock().as_ref() {
        Some(s) => MeetingStatus { recording: true, seconds: s.started.elapsed().as_secs_f64(), has_system: s.has_system },
        None => MeetingStatus { recording: false, seconds: 0.0, has_system: false },
    }
}

#[tauri::command]
fn start_meeting(app: AppHandle, with_system: Option<bool>) -> Result<MeetingStatus, String> {
    let state = app.state::<AppState>();
    if state.meeting.lock().is_some() {
        return Err("Už nahrávám".into());
    }
    let (mic, sys) = {
        let s = state.settings.lock();
        (s.microphone.clone(), with_system.unwrap_or(s.rec_system_audio))
    };
    let id = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = state.recordings_dir.join(&id);
    let level_app = app.clone();
    let level: recorder::LevelFn = Arc::new(move |rms: f32| {
        let _ = level_app.emit_to("main", "whiskr:rec-level", rms);
        let _ = level_app.emit_to("bar", "whiskr:rec-level", rms);
    });
    let session = meeting::Session::start(dir.clone(), mic, sys, level)?;
    // Write the entry right away: if the app or PC dies mid-meeting, Whiskr
    // finds it on the next start and recovers the audio.
    let mut rec = notes::Recording::new(id, 0.0, session.has_system);
    rec.template = state.settings.lock().default_template.clone();
    state.bookmarks.lock().clear();
    rec.status = "recording".into();
    rec.stage = String::new();
    let _ = notes::save(&dir, &rec);
    let status = MeetingStatus { recording: true, seconds: 0.0, has_system: session.has_system };
    *state.meeting.lock() = Some(session);
    set_recording_ui(&app, true);
    let _ = app.emit_to("main", "whiskr:meeting", &status);
    let _ = app.emit_to("main", "whiskr:recording", rec.brief());
    place_bar(&app);
    bar_state(&app, "rec_start", None);
    assistant::on_meeting_started(&app);
    Ok(status)
}

#[tauri::command]
fn stop_meeting(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(session) = state.meeting.lock().take() else { return Ok(()) };
    assistant::on_meeting_stopped(&app);
    set_recording_ui(&app, false);
    bar_state(&app, "rec_stop", None);
    let _ = app.emit_to("main", "whiskr:meeting", MeetingStatus { recording: false, seconds: 0.0, has_system: false });
    let fin = session.stop()?;
    let id = fin.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if fin.seconds < 2.0 {
        let _ = std::fs::remove_dir_all(&fin.dir);
        return Err("Nahrávka byla moc krátká".into());
    }
    let mut rec = notes::load(&fin.dir).unwrap_or_else(|| notes::Recording::new(id.clone(), fin.seconds, fin.has_system));
    rec.seconds = fin.seconds;
    rec.has_system = fin.has_system;
    rec.bookmarks = std::mem::take(&mut *state.bookmarks.lock());
    rec.status = "processing".into();
    rec.stage = "transcribe".into();
    notes::save(&fin.dir, &rec)?;
    let _ = app.emit_to("main", "whiskr:recording", rec.brief());
    spawn_processing(&app, id);
    Ok(())
}

/// Toggle from the shortcut or tray.
fn toggle_meeting(app: &AppHandle) {
    let recording = app.state::<AppState>().meeting.lock().is_some();
    let r = if recording { stop_meeting(app.clone()) } else { start_meeting(app.clone(), None).map(|_| ()) };
    if let Err(e) = r {
        bar_state(app, "error", Some(&e));
    }
}

/// After a crash or forced quit: rescue recordings that were never finished
/// and resume processing that was interrupted.
fn recover_recordings(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(entries) = std::fs::read_dir(&state.recordings_dir) else { return };
    for e in entries.flatten() {
        let dir = e.path();
        let Some(mut rec) = notes::load(&dir) else { continue };
        match rec.status.as_str() {
            "recording" => {
                let secs = meeting::repair_wav(&dir.join("mic.wav")).unwrap_or(0.0);
                if rec.has_system {
                    let _ = meeting::repair_wav(&dir.join("system.wav"));
                }
                if secs < 2.0 {
                    let _ = std::fs::remove_dir_all(&dir);
                    continue;
                }
                rec.seconds = secs;
                rec.status = "processing".into();
                rec.stage = "transcribe".into();
                rec.error = Some("Nahrávání bylo přerušené (pád nebo vypnutí) — zachránila jsem, co se nahrálo.".into());
                let _ = notes::save(&dir, &rec);
                spawn_processing(app, rec.id.clone());
            }
            "processing" => spawn_processing(app, rec.id.clone()),
            _ => {}
        }
    }
}

fn spawn_processing(app: &AppHandle, id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let dir = state.recordings_dir.join(&id);
        let Some(mut rec) = notes::load(&dir) else { return };
        let keys = {
            let s = state.settings.lock();
            notes::Keys {
                custom_templates: s.custom_templates.clone(),
                groq: s.api_key.clone(),
                brain: brain::Brain { primary: brain::Engine::Groq { key: s.api_key.clone() }, fallback: None },
                language: s.language.clone(),
                dictionary: s.dictionary.clone(),
                data_dir: data_dir(&state),
            }
        };
        let mut keys = keys;
        if let Some(b) = assistant::make_brain(&app) {
            keys.brain = b;
        }
        let emit_app = app.clone();
        let dir2 = dir.clone();
        let result = notes::process(&state.http, &dir, &mut rec, &keys, |r| {
            let _ = notes::save(&dir2, r);
            let _ = emit_app.emit_to("main", "whiskr:recording", r.brief());
        })
        .await;
        if let Err(e) = result {
            rec.status = "error".into();
            rec.error = Some(e);
            rec.stage = String::new();
        }
        let _ = notes::save(&dir, &rec);
        let _ = app.emit_to("main", "whiskr:recording", rec.brief());
    });
}

fn data_dir(state: &AppState) -> PathBuf {
    state.recordings_dir.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

fn rec_dir(state: &AppState, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return Err("Neplatné ID nahrávky".into());
    }
    Ok(state.recordings_dir.join(id))
}

#[tauri::command]
fn list_recordings(state: State<'_, AppState>) -> Vec<notes::Recording> {
    notes::list(&state.recordings_dir)
}

#[tauri::command]
fn get_recording(state: State<'_, AppState>, id: String) -> Result<notes::Recording, String> {
    notes::load(&rec_dir(&state, &id)?).ok_or_else(|| "Nahrávka nenalezena".into())
}

#[tauri::command]
fn delete_recording(state: State<'_, AppState>, id: String) -> Result<(), String> {
    std::fs::remove_dir_all(rec_dir(&state, &id)?).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_recording(state: State<'_, AppState>, id: String, title: String) -> Result<(), String> {
    let dir = rec_dir(&state, &id)?;
    let mut r = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    r.title = title.trim().chars().take(120).collect();
    notes::save(&dir, &r)
}

#[tauri::command]
fn rename_speaker(state: State<'_, AppState>, id: String, label: String, name: String) -> Result<notes::Recording, String> {
    let dir = rec_dir(&state, &id)?;
    let mut r = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    let name = name.trim();
    let name: String = if name.is_empty() { notes::default_name(&label) } else { name.chars().take(40).collect() };
    // Teach Whiskr this voice, so it's recognised in future recordings.
    if let Some(print) = r.prints.get(&label) {
        let mut book = voices::VoiceBook::load(&data_dir(&state));
        let is_default = name == notes::default_name(&label);
        if label == notes::ME || name.trim().eq_ignore_ascii_case("já") {
            book.learn_me(print);
        } else if !is_default {
            book.learn(&name, print);
        }
        book.save(&data_dir(&state));
    }
    // Same name as another speaker → it's the same person: merge them.
    let twin = r
        .speakers
        .iter()
        .find(|(l, n)| **l != label && n.trim().eq_ignore_ascii_case(name.trim()) && **n != notes::default_name(l))
        .map(|(l, _)| l.clone());
    if let Some(into) = twin {
        for u in r.utterances.iter_mut() {
            if u.speaker == label {
                u.speaker = into.clone();
            }
        }
        if let (Some(a), Some(b)) = (r.prints.get(&into).cloned(), r.prints.get(&label).cloned()) {
            let mixed: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
            r.prints.insert(into.clone(), voices::normalize(mixed));
        }
        r.prints.remove(&label);
        r.speakers.remove(&label);
        notes::merge_adjacent(&mut r.utterances);
    } else {
        r.speakers.insert(label, name);
    }
    notes::save(&dir, &r)?;
    Ok(r)
}

#[tauri::command]
fn reprocess_recording(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let dir = rec_dir(&state, &id)?;
    let mut r = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    r.status = "processing".into();
    r.error = None;
    r.utterances.clear();
    r.summary = None;
    r.speakers.clear();
    notes::save(&dir, &r)?;
    spawn_processing(&app, id);
    Ok(())
}

#[tauri::command]
fn recording_markdown(state: State<'_, AppState>, id: String) -> Result<String, String> {
    Ok(notes::load(&rec_dir(&state, &id)?).ok_or("Nahrávka nenalezena")?.to_markdown())
}

#[tauri::command]
fn open_recording_folder(app: AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<AppState>();
    let dir = rec_dir(&state, &id)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct KnownVoice {
    name: String,
    recordings: u32,
}

#[derive(serde::Serialize)]
struct VoicesInfo {
    model_ready: bool,
    me: Option<u32>,
    voices: Vec<KnownVoice>,
}

#[tauri::command]
fn list_voices(state: State<'_, AppState>) -> VoicesInfo {
    let dir = data_dir(&state);
    let book = voices::VoiceBook::load(&dir);
    VoicesInfo {
        model_ready: voices::model_ready(&dir),
        me: book.me.as_ref().map(|m| m.n),
        voices: book.voices.iter().map(|v| KnownVoice { name: v.name.clone(), recordings: v.n }).collect(),
    }
}

#[tauri::command]
fn forget_voice(state: State<'_, AppState>, name: String) {
    let dir = data_dir(&state);
    let mut book = voices::VoiceBook::load(&dir);
    book.forget(&name);
    book.save(&dir);
}

// ───────────────────────────── working with recordings ─────────────────────────────

/// Mark "this is important" in the running meeting.
#[tauri::command]
fn add_bookmark(app: AppHandle) -> Result<f64, String> {
    let state = app.state::<AppState>();
    let (t, dir) = {
        let m = state.meeting.lock();
        let s = m.as_ref().ok_or("Bookmark jde přidat jen během nahrávání")?;
        (s.started.elapsed().as_secs_f64(), s.dir().to_path_buf())
    };
    let marks = {
        let mut b = state.bookmarks.lock();
        b.push(t);
        b.clone()
    };
    // Keep them on disk too, in case the app dies before the meeting ends.
    if let Some(mut rec) = notes::load(&dir) {
        rec.bookmarks = marks.clone();
        let _ = notes::save(&dir, &rec);
    }
    bar_state(&app, "bookmark", Some(&notes::fmt_ts(t)));
    let _ = app.emit_to("main", "whiskr:bookmarks", marks);
    Ok(t)
}

#[tauri::command]
async fn recording_audio(app: AppHandle, id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let dir = rec_dir(&state, &id)?;
    let rec = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    let path = tauri::async_runtime::spawn_blocking(move || notes::playable_audio(&dir, &rec))
        .await
        .map_err(|e| e.to_string())??;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn ask_recordings(
    state: State<'_, AppState>,
    id: Option<String>,
    question: String,
    history: Vec<(String, String)>,
) -> Result<String, String> {
    let key = state.settings.lock().api_key.clone();
    if key.trim().is_empty() {
        return Err("Chybí Groq klíč".into());
    }
    if let Some(i) = &id {
        rec_dir(&state, i)?;
    }
    ask::ask(&state.http, &key, &state.recordings_dir, id.as_deref(), &question, &history).await
}

#[tauri::command]
async fn search_recordings(state: State<'_, AppState>, query: String) -> Result<Vec<ask::Hit>, String> {
    let root = state.recordings_dir.clone();
    tauri::async_runtime::spawn_blocking(move || ask::search(&root, &query, 40)).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn list_templates(state: State<'_, AppState>) -> Vec<templates::Template> {
    templates::all(&state.settings.lock().custom_templates)
}

/// Re-run only the summary (e.g. with another template).
#[tauri::command]
fn resummarize(app: AppHandle, id: String, template: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let dir = rec_dir(&state, &id)?;
    let mut rec = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    if rec.utterances.is_empty() {
        return Err("Nahrávka nemá přepis".into());
    }
    rec.template = template;
    rec.status = "processing".into();
    rec.stage = "summary".into();
    notes::save(&dir, &rec)?;
    let _ = app.emit_to("main", "whiskr:recording", rec.brief());
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        let (key, custom) = {
            let s = state.settings.lock();
            (s.api_key.clone(), s.custom_templates.clone())
        };
        let brain = assistant::make_brain(&app2).unwrap_or(brain::Brain { primary: brain::Engine::Groq { key }, fallback: None });
        rec.error = None;
        if let Err(e) = notes::summarise(&state.http, &brain, &mut rec, &custom).await {
            rec.error = Some(format!("Shrnutí se nepovedlo: {e}"));
        }
        rec.status = "done".into();
        rec.stage = String::new();
        let _ = notes::save(&dir, &rec);
        let _ = app2.emit_to("main", "whiskr:recording", rec.brief());
    });
    Ok(())
}

/// Fix a transcript line: its text and/or who said it ("__new__" = a new speaker).
#[tauri::command]
fn update_utterance(
    state: State<'_, AppState>,
    id: String,
    index: usize,
    text: Option<String>,
    speaker: Option<String>,
) -> Result<notes::Recording, String> {
    let dir = rec_dir(&state, &id)?;
    let mut r = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    if index >= r.utterances.len() {
        return Err("Řádek nenalezen".into());
    }
    if let Some(t) = text {
        let t = t.trim().to_string();
        if t.is_empty() {
            r.utterances.remove(index);
        } else {
            r.utterances[index].text = t;
        }
    } else if let Some(mut sp) = speaker {
        if sp == "__new__" {
            let used: Vec<String> = r.speakers.keys().cloned().collect();
            sp = (b'A'..=b'Z').map(|c| (c as char).to_string()).find(|l| !used.contains(l)).unwrap_or_else(|| "Z".into());
            r.speakers.insert(sp.clone(), notes::default_name(&sp));
        }
        r.utterances[index].speaker = sp;
        notes::merge_adjacent(&mut r.utterances);
    }
    notes::save(&dir, &r)?;
    Ok(r)
}

#[tauri::command]
fn export_docx(app: AppHandle, id: String) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<AppState>();
    let dir = rec_dir(&state, &id)?;
    let r = notes::load(&dir).ok_or("Nahrávka nenalezena")?;
    let path = export::docx(&r, &dir)?;
    let _ = app.opener().open_path(path.to_string_lossy(), None::<&str>);
    Ok(path.to_string_lossy().to_string())
}

// ───────────────────────────── Whiskr Ask ─────────────────────────────

const TERMINALS: &[&str] = &[
    "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "wezterm-gui.exe", "alacritty.exe", "conhost.exe",
    "mintty.exe", "wsl.exe", "openconsole.exe", "tabby.exe", "hyper.exe", "warp.exe",
];

fn refresh_tools(app: &AppHandle) -> Vec<agents::Tool> {
    let tools = agents::detect();
    *app.state::<AppState>().tools.lock() = tools.clone();
    let _ = app.emit("whiskr:tools", &tools);
    tools
}

fn profile_key(s: &Settings, id: Option<&str>) -> String {
    let id = id.unwrap_or(&s.active_profile);
    s.profiles.iter().find(|p| p.id == id).map(|p| p.key.clone()).unwrap_or_default()
}

/// Where the ask window sits: bottom centre of the screen with the cursor,
/// growing upwards from the bar.
/// Height of the collapsed ask bar (the page measures itself and tells us).
static ASK_COMPACT_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(62);

/// Windows: size and place the chat window — the bar (compact) or the whole
/// chat (expanded) — at the bottom centre of the monitor with the mouse.
/// Straight through Win32, only when something changes.
/// Returns None when there's no window yet, else whether it really moved/resized.
/// `cloak`: hide it from the screen (DWM) before resizing — the page shows it
/// again once it has painted at the new size (ask_uncloak).
#[cfg(windows)]
fn place_ask_raw(expanded: bool, cloak: bool) -> Option<bool> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    static LAST: parking_lot::Mutex<(i32, i32, i32, i32)> = parking_lot::const_mutex((0, 0, 0, 0));
    let h = ASK_HWND.load(AO::SeqCst);
    if h == 0 {
        return None;
    }
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let sf = dx.max(96) as f64 / 96.0;
        let wa = mi.rcWork;
        let (ww, wh) = ((wa.right - wa.left) as f64, (wa.bottom - wa.top) as f64);
        let compact_h = ASK_COMPACT_H.load(AO::Relaxed) as f64;
        let (lw, lh) = if expanded { (820.0, 620.0) } else { (700.0, compact_h) };
        let pw = (lw * sf) as i32;
        let ph = ((lh * sf).min(wh - 40.0 * sf)) as i32;
        let x = wa.left + (ww as i32 - pw) / 2;
        let y = wa.top + wh as i32 - ph - (56.0 * sf) as i32;
        let want = (x, y, pw, ph);
        let mut last = LAST.lock();
        if *last == want {
            return Some(false);
        }
        *last = want;
        if cloak {
            cloak_ask(true);
        }
        SetWindowPos(h as _, std::ptr::null_mut(), x, y, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    Some(true)
}

/// Hide / show the chat window from the compositor only (DWM "cloak"): it
/// stays alive and keeps painting, nothing on screen. Used to resize it
/// between bar and full chat without anyone seeing a half-painted frame.
#[cfg(windows)]
fn cloak_ask(on: bool) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let h = ASK_HWND.load(AO::SeqCst);
    if h != 0 {
        let v: i32 = on as i32;
        unsafe {
            DwmSetWindowAttribute(h as _, DWMWA_CLOAK as u32, &v as *const i32 as *const _, 4);
        }
    }
}

fn place_ask(app: &AppHandle, expanded: bool) {
    #[cfg(windows)]
    if place_ask_raw(expanded, false).is_some() {
        return;
    }
    let Some(w) = app.get_webview_window("ask") else { return };
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let sf = m.scale_factor();
    let wa = m.work_area();
    let compact_h = ASK_COMPACT_H.load(std::sync::atomic::Ordering::Relaxed) as f64;
    let (lw, lh) = if expanded { (820.0, 620.0) } else { (700.0, compact_h) };
    let max_h = wa.size.height as f64 / sf - 40.0;
    let size = tauri::LogicalSize::new(lw, if lh > max_h { max_h } else { lh });
    let _ = w.set_size(size);
    let pw = (size.width * sf) as i32;
    let ph = (size.height * sf) as i32;
    let x = wa.position.x + (wa.size.width as i32 - pw) / 2;
    let y = wa.position.y + wa.size.height as i32 - ph - (56.0 * sf) as i32;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

// ─── the quick chat: one source of truth ───
//
// Shift+Space only flips what the user WANTS (open / closed). A single
// "applier" thread then makes the window match. If Whiskr is busy for a
// moment, presses don't pile up into open-close-open-close: they just flip
// the wish, and the window ends up exactly where the last press wanted it.
static ASK_WANT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Close animation running (the window is still on screen for ~0.2 s).
static ASK_CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ASK_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static ASK_KICK: std::sync::OnceLock<parking_lot::Mutex<std::sync::mpsc::Sender<()>>> = std::sync::OnceLock::new();
use std::sync::atomic::Ordering as AO;

/// Is the chat window on screen? Asked straight from Windows — no waiting for the UI thread.
fn ask_visible(app: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        let h = ASK_HWND.load(AO::SeqCst);
        if h != 0 {
            return unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(h as _) != 0 };
        }
    }
    app.get_webview_window("ask").and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

/// Show the chat window and give it the keyboard — straight through Windows,
/// from whatever thread we're on. (The window library's show()/set_focus()
/// run on the UI thread and, with the big chat open, took 1–3 s each.)
fn ask_show_focus(app: &AppHandle) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_SHOW};
        let h = ASK_HWND.load(AO::SeqCst);
        if h == 0 {
            return;
        }
        unsafe {
            if IsWindowVisible(h as _) == 0 {
                ShowWindow(h as _, SW_SHOW);
            }
            if GetForegroundWindow() as isize == h {
                return;
            }
            let t0 = Instant::now();
            // (no faked Alt key press to force it — that's what malware does;
            // Whiskr just received the shortcut, so Windows normally allows this)
            let ok = SetForegroundWindow(h as _) != 0;
            dlog!("ask_show_focus: SetForegroundWindow ok={ok}");
            dlog!("ask_show_focus: SetForegroundWindow took {} ms", t0.elapsed().as_millis());
        }
        let _ = app;
    }
    #[cfg(not(windows))]
    if let Some(w) = app.get_webview_window("ask") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn start_ask_applier(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = ASK_KICK.set(parking_lot::Mutex::new(tx));
    std::thread::Builder::new()
        .name("whiskr-ask-applier".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                while rx.try_recv().is_ok() {} // coalesce presses that came meanwhile
                let (want, vis, closing) = (ASK_WANT.load(AO::SeqCst), ask_visible(&app), ASK_CLOSING.load(AO::SeqCst));
                dlog!("ask applier: want={want} visible={vis} closing={closing}");
                if want && !vis {
                    let swap = app.state::<AppState>().settings.lock().ask_space_new;
                    open_ask(&app, swap, false);
                } else if want && vis && closing {
                    // pressed again during the close animation: stay open
                    CLOSE_GEN.fetch_add(1, AO::SeqCst);
                    ASK_CLOSING.store(false, AO::SeqCst);
                    let _ = app.emit_to("ask", "whiskr:ask-reopen", ());
                    ask_show_focus(&app);
                } else if !want && vis && !closing {
                    close_ask(&app);
                }
            }
        })
        .ok();
}

fn kick_ask() {
    if let Some(tx) = ASK_KICK.get() {
        let _ = tx.lock().send(());
    }
}

fn toggle_ask(_app: &AppHandle) {
    let was = ASK_WANT.fetch_xor(true, AO::SeqCst);
    dlog!("toggle_ask: want {}", !was);
    kick_ask();
}

/// Shift+Space: show / hide. Shift+Space+R (`fresh`): always a brand new chat.
fn open_ask(app: &AppHandle, fresh: bool, chord: bool) {
    let Some(w) = app.get_webview_window("ask") else { return };
    let visible = ask_visible(app);
    dlog!("open_ask fresh={fresh} chord={chord} visible={visible}");
    if visible {
        // R pressed right after Shift+Space opened it: switch to the other kind of chat.
        let recent = std::mem::take(&mut *app.state::<AppState>().ask_just_opened.lock()).map_or(false, |t| t.elapsed() < Duration::from_millis(1500));
        if chord && recent {
            let _ = app.emit_to("ask", "whiskr:ask-open", json!({ "phase": "show", "fresh": fresh, "switch": true }));
            ask_show_focus(app);
            return;
        }
        close_ask(app);
        return;
    }
    ASK_WANT.store(true, AO::SeqCst);
    ASK_CLOSING.store(false, AO::SeqCst);
    CLOSE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *app.state::<AppState>().ask_just_opened.lock() = Some(Instant::now());
    let state = app.state::<AppState>();
    let target = context::foreground();
    let ours = target.exe.starts_with("whiskr");
    #[cfg(windows)]
    {
        let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize;
        let ask_h = ASK_HWND.load(AO::SeqCst);
        if fg != 0 && fg != ask_h {
            *state.ask_return.lock() = fg;
        }
        if !ours {
            *state.ask_target.lock() = fg;
        }
    }
    let want_sel = state.settings.lock().ask_selection && !ours && !TERMINALS.contains(&target.exe.as_str()) && !target.exe.is_empty();
    let was_visible = false;
    // A chat that continues (the page told us when it hid) opens straight as
    // the full chat — no bar first. Otherwise the small bar.
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let big = !fresh && now_ms < ASK_NEXT_BIG_UNTIL.load(AO::SeqCst);
    dlog!("open_ask: as {}", if big { "chat" } else { "bar" });
    if !was_visible {
        place_ask(app, big);
    }
    CLOAK_GEN.fetch_add(1, AO::SeqCst);
    #[cfg(windows)]
    cloak_ask(false);
    // Show right away but WITHOUT taking focus: the selection (Ctrl+C) still has
    // to go to the app the user was in. The page keeps the panel invisible until
    // the "show" event, so the intro animation is what you actually see.
    #[cfg(windows)]
    {
        let h = ASK_HWND.load(AO::SeqCst);
        let t0 = Instant::now();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(h as _, windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
        }
        dlog!("open_ask: ShowWindow took {} ms", t0.elapsed().as_millis());
    }
    #[cfg(not(windows))]
    let _ = w.show();
    if !was_visible {
        let _ = app.emit_to("ask", "whiskr:ask-open", json!({ "phase": "show", "fresh": fresh, "expanded": big }));
    }
    let gen = CLOSE_GEN.load(std::sync::atomic::Ordering::SeqCst);
    let app2 = app.clone();
    std::thread::spawn(move || {
        let mut selection = None;
        // Wait for the user to let go of Shift (max ~0.7 s). We never fake a
        // Shift release any more — that confused Windows about the real Shift
        // and made the next Shift+Space type a plain space instead.
        if want_sel {
            // read it through UI Automation (no keys sent, clipboard untouched)
            selection = selection::focused_selection().filter(|t| t.chars().count() > 1);
            // optional, off by default: the old way (Ctrl+C into the app)
            let copy_ok = app2.state::<AppState>().settings.lock().ask_selection_copy;
            if selection.is_none() && copy_ok && wait_modifiers_up(700) {
                if let Ok((sel, original)) = paste::read_selection_wait(4) {
                    paste::set_clipboard(original);
                    selection = sel.filter(|t| t.trim().chars().count() > 1);
                }
            }
        }
        dlog!("open_ask thread: want_sel={want_sel} selection={}", selection.is_some());
        // closed again in the meantime: don't steal focus to a hidden window
        if CLOSE_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen {
            dlog!("open_ask thread: closed meanwhile, no focus");
            return;
        }
        if let Some(_w) = app2.get_webview_window("ask") {
            if !ask_visible(&app2) {
                dlog!("open_ask thread: window not visible any more");
                return;
            }
            // Tell the window library it is visible now (we showed it with a raw
            // ShowWindow above). Without this it still thinks "hidden" — it then
            // refuses set_focus and may hide the window again on its next update.
            let t0 = Instant::now();
            ask_show_focus(&app2);
            dlog!("open_ask thread: show+focus took {} ms", t0.elapsed().as_millis());
            #[cfg(windows)]
            {
                let fg = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize;
                dlog!("open_ask thread: focused={} visible={}", fg == ASK_HWND.load(AO::SeqCst), ask_visible(&app2));
            }
        }
        let _ = app2.emit_to(
            "ask",
            "whiskr:ask-open",
            json!({ "phase": "focus", "selection": selection, "app": target.title, "exe": target.exe }),
        );
    });
}

/// true once Shift/Ctrl/Alt/Win are all physically up (or false after `ms`).
fn wait_modifiers_up(ms: u64) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
        let down = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
        let start = Instant::now();
        loop {
            if ![VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].iter().any(|&k| down(k)) {
                return true;
            }
            if start.elapsed() > Duration::from_millis(ms) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ms;
        true
    }
}

#[derive(serde::Serialize)]
struct AskTools {
    tools: Vec<agents::Tool>,
    groq: bool,
}

#[tauri::command]
async fn ask_tools(app: AppHandle, refresh: bool, check_login: bool) -> Result<AskTools, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        let mut tools = if refresh || state.tools.lock().is_empty() { refresh_tools(&app2) } else { state.tools.lock().clone() };
        if check_login {
            let key = profile_key(&state.settings.lock(), None);
            std::thread::scope(|sc| {
                for t in tools.iter_mut() {
                    let key = key.clone();
                    sc.spawn(move || t.logged_in = agents::check_login(t, &key));
                }
            });
        }
        let groq = !state.settings.lock().api_key.trim().is_empty();
        AskTools { tools, groq }
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct AskRunArgs {
    engine: String,
    mode: String,
    prompt: String,
    cwd: Option<String>,
    resume: Option<String>,
    profile: Option<String>,
    #[serde(default)]
    history: Vec<(String, String)>,
    #[serde(default)]
    model: Option<String>,
    /// Attached files (Windows paths, inside ask-workspace/attachments).
    #[serde(default)]
    attachments: Vec<String>,
}

// async = runs off the UI thread: starting WSL/Codex can take seconds and used
// to freeze every window (and Shift+Space) meanwhile.
#[tauri::command]
async fn ask_run(app: AppHandle, req: AskRunArgs) -> Result<u64, String> {
    let state = app.state::<AppState>();
    let id = state.run_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let delegate = req.mode == "delegate";
    let label = agents::engine_label(&req.engine).to_string();
    let emit_app = app.clone();
    let emit = move |ev: agents::Ev| {
        let finished = matches!(ev, agents::Ev::Done { .. } | agents::Ev::Error { .. });
        let failed = matches!(ev, agents::Ev::Error { .. });
        if let agents::Ev::Image { path } = &ev {
            let _ = emit_app.asset_protocol_scope().allow_file(path);
        }
        if let agents::Ev::Error { text } = &ev {
            dlog!("agent run {id} error: {text}");
        }
        // to every window: the quick chat AND the chat page inside the main window
        let _ = emit_app.emit("whiskr:agent", agents::emit_json(&ev, id));
        if finished && delegate {
            let hidden = emit_app.get_webview_window("ask").map(|w| !w.is_visible().unwrap_or(false)).unwrap_or(true);
            if hidden {
                let msg = if failed { format!("{label}: úkol se nepovedl") } else { format!("{label} dokončil úkol ✓") };
                bar_state(&emit_app, "notify", Some(&msg));
            }
        }
    };

    if req.engine == "groq" {
        let key = state.settings.lock().api_key.clone();
        if key.trim().is_empty() {
            return Err("Chybí Groq klíč".into());
        }
        if req.attachments.iter().any(|p| agents::is_image(p)) {
            return Err("Groq obrázky neumí — přepni na Claude nebo GPT.".into());
        }
        let instructions = state.settings.lock().ask_instructions.trim().to_string();
        let http = state.http.clone();
        tauri::async_runtime::spawn(async move {
            let mut user = String::new();
            for (q, a) in req.history.iter().rev().take(6).rev() {
                user.push_str(&format!("User: {q}\nAssistant: {a}\n\n"));
            }
            user.push_str(&req.prompt);
            // text files go in whole (Groq can't open files)
            for p in &req.attachments {
                let name = std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                match std::fs::read(p) {
                    Ok(b) if std::str::from_utf8(&b).is_ok() => {
                        let t: String = String::from_utf8_lossy(&b).chars().take(60_000).collect();
                        user.push_str(&format!("\n\n<soubor nazev=\"{name}\">\n{t}\n</soubor>"));
                    }
                    _ => user.push_str(&format!("\n\n(Soubor {name} nejde přečíst jako text.)")),
                }
            }
            let mut system = String::from("You are Whiskr's quick assistant. Be concise, concrete and practical. Answer in the language the user writes in. Use short Markdown only when it helps.");
            if !instructions.is_empty() {
                system.push_str("\n\nThe user's standing instructions (follow them):\n");
                system.push_str(&instructions);
            }
            let system = system.as_str();
            match groq::chat_text_with(&http, &key, system, &user, req.model.as_deref()).await {
                Ok(t) => {
                    emit(agents::Ev::Text { text: t });
                    emit(agents::Ev::Done { text: String::new() });
                }
                Err(e) => emit(agents::Ev::Error { text: e }),
            }
        });
        return Ok(id);
    }

    let (tool, pkey, access) = {
        let s = state.settings.lock();
        let tools = state.tools.lock();
        let tool = agents::pick(&tools, &req.engine, &s.ask_runtime).cloned().ok_or_else(|| {
            format!("{} jsem na tomhle počítači nenašla. Nainstaluj ho (Windows nebo WSL) a v Nastavení → Whiskr Ask klikni na Najít znovu.", agents::engine_label(&req.engine))
        })?;
        (tool, profile_key(&s, req.profile.as_deref()), s.ask_access.clone())
    };
    let (fast, instructions, data) = {
        let s = state.settings.lock();
        (s.ask_fast, s.ask_instructions.clone(), s.ask_whiskr_data)
    };
    // Whiskr's own dictations & meetings, for Claude Code and Codex (Cursor
    // only reads its own MCP config)
    let mcp = mcp_exe();
    let whiskr_mcp = (data && matches!(req.engine.as_str(), "claude" | "codex") && mcp.exists()).then(|| mcp.to_string_lossy().to_string());
    let run = agents::RunReq {
        engine: req.engine,
        mode: req.mode,
        prompt: req.prompt,
        cwd: req.cwd,
        resume: req.resume,
        profile_key: pkey,
        access,
        model: req.model.unwrap_or_default(),
        fast,
        attachments: req.attachments,
        instructions,
        whiskr_mcp,
    };
    agents::start(id, &tool, run, &state.data_dir.join("ask-workspace"), emit)?;
    Ok(id)
}


/// Windows fades windows in/out on show/hide; that made Ask feel slow and choppy.
#[cfg(windows)]
fn no_dwm_transitions(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
    let on: i32 = 1;
    unsafe {
        DwmSetWindowAttribute(hwnd as _, DWMWA_TRANSITIONS_FORCEDISABLED as u32, &on as *const i32 as *const _, 4);
    }
}

/// Remove the thin Windows 11 frame highlight (it glowed at the top edge when
/// the window was inactive).
#[cfg(windows)]
fn no_dwm_border(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    let none: u32 = 0xFFFF_FFFE;
    let hr = unsafe { DwmSetWindowAttribute(hwnd as _, DWMWA_BORDER_COLOR as u32, &none as *const u32 as *const _, 4) };
    if hr != 0 {
        dlog!("DWM border off failed: 0x{:08x}", hr as u32);
    }
}

static ASK_GLASS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The quick chat: no Windows frame at all — no border line, no rounding of
/// its own (the page's clip region does the rounding). Windows re-applies its
/// frame whenever the window style changes (every show), so call it after each show.
#[cfg(windows)]
fn bare_ask_frame(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE};
    no_dwm_border(hwnd);
    if ASK_GLASS.load(std::sync::atomic::Ordering::Relaxed) {
        return; // frosted glass wants Windows' own rounded corners
    }
    let no_round: i32 = 1; // DWMWCP_DONOTROUND
    unsafe {
        DwmSetWindowAttribute(hwnd as _, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &no_round as *const i32 as *const _, 4);
    }
}

/// Match the window frame (background, Windows 11 border) to the app theme.
#[tauri::command]
fn set_window_theme(app: AppHandle, dark: bool) {
    let Some(w) = app.get_webview_window("main") else { return };
    // transparent window: the page paints its own background (colour / image / opacity)
    let _ = w.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    let _ = w.set_theme(Some(if dark { tauri::Theme::Dark } else { tauri::Theme::Light }));
    #[cfg(windows)]
    if let Ok(h) = w.hwnd() {
        use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
        // no Windows border at all (the page draws its own); DWMWA_COLOR_NONE
        let _ = dark;
        let color: u32 = 0xFFFF_FFFE;
        unsafe {
            DwmSetWindowAttribute(h.0 as _, DWMWA_BORDER_COLOR as u32, &color as *const u32 as *const _, 4);
        }
    }
}

#[tauri::command]
async fn ask_models(app: AppHandle, engine: String) -> Result<Vec<agents::ModelInfo>, String> {
    let state = app.state::<AppState>();
    if engine == "groq" {
        let key = state.settings.lock().api_key.clone();
        let http = state.http.clone();
        return groq::list_chat_models(&http, &key)
            .await
            .map(|v| v.into_iter().map(|(id, name)| agents::ModelInfo { id, name }).collect());
    }
    if engine == "claude" {
        return Ok(agents::claude_models());
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        if state.tools.lock().is_empty() {
            refresh_tools(&app2);
        }
        let (tool, key) = {
            let s = state.settings.lock();
            let tools = state.tools.lock();
            (agents::pick(&tools, &engine, &s.ask_runtime).cloned(), profile_key(&s, None))
        };
        tool.map(|t| agents::list_models(&t, &key)).ok_or_else(|| "Nástroj nenalezen".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// From the ask window: open the main app (optionally on a page).
#[tauri::command]
fn open_main(app: AppHandle, page: Option<String>) {
    ASK_WANT.store(false, AO::SeqCst);
    hide_ask(&app);
    show_main(&app);
    if let Some(p) = page {
        let _ = app.emit_to("main", "whiskr:goto", p);
    }
}

/// Open the ask window with text in the box (and send it).
#[tauri::command]
fn ask_open_with(app: AppHandle, text: String, send: bool) {
    let Some(w) = app.get_webview_window("ask") else { return };
    ASK_WANT.store(true, AO::SeqCst);
    ASK_CLOSING.store(false, AO::SeqCst);
    let _ = &w;
    place_ask(&app, true);
    ask_show_focus(&app);
    let _ = app.emit_to("ask", "whiskr:ask-prefill", json!({ "text": text, "send": send }));
}

#[tauri::command]
fn ask_cancel(run: u64) {
    agents::cancel(run);
}

static CLOAK_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The page switches between the bar and the full chat, or the bar grows (a
/// new line, the model menu). The window really resizes — no clipping region
/// any more (Windows repainted the big chat through the UI thread for every
/// region change, freezing everything for seconds). To hide the one frame
/// where the old picture sits at the wrong place, a visible window is cloaked,
/// resized, and shown again by the page once it has painted (ask_uncloak).
/// Returns true when it cloaked (the page must call ask_uncloak).
#[tauri::command]
async fn ask_layout(app: AppHandle, expanded: bool, height: Option<f64>) -> bool {
    if let Some(h) = height.filter(|h| *h > 30.0 && *h < 500.0) {
        ASK_COMPACT_H.store(h.round() as u32, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(windows)]
    {
        let visible = ask_visible(&app);
        let gen = CLOAK_GEN.fetch_add(1, AO::SeqCst) + 1;
        let moved = place_ask_raw(expanded, visible);
        if moved == Some(true) && visible {
            // safety net: never stay cloaked if the page doesn't answer
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                if CLOAK_GEN.load(AO::SeqCst) == gen {
                    dlog!("ask_layout: page did not uncloak, doing it");
                    cloak_ask(false);
                }
            });
            return true;
        }
        if moved.is_some() {
            return false;
        }
    }
    place_ask(&app, expanded);
    false
}

static ASK_NEXT_BIG_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The page, when it hides: "the next opening continues the chat" for ttl_ms
/// (0 = it starts with the bar). Lets open_ask show the full chat directly.
#[tauri::command]
async fn ask_next_open(expanded: bool, ttl_ms: u64) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    ASK_NEXT_BIG_UNTIL.store(if expanded { now + ttl_ms } else { 0 }, AO::SeqCst);
}

/// The page has painted at the new size: show it.
#[tauri::command]
async fn ask_uncloak() {
    CLOAK_GEN.fetch_add(1, AO::SeqCst);
    #[cfg(windows)]
    cloak_ask(false);
}

// ───────────────────────────── updates ─────────────────────────────

static UPDATE: parking_lot::Mutex<Option<updater::UpdateInfo>> = parking_lot::const_mutex(None);

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<updater::UpdateInfo, String> {
    let http = app.state::<AppState>().http.clone();
    let info = updater::check(&http).await?;
    dlog!("update check: current {} latest {} available {}", info.current, info.latest, info.available);
    *UPDATE.lock() = Some(info.clone());
    Ok(info)
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let http = app.state::<AppState>().http.clone();
    let cached = UPDATE.lock().clone();
    let info = match cached {
        Some(i) if i.available => i,
        _ => {
            let i = updater::check(&http).await?;
            if !i.available {
                return Err("Už máš nejnovější verzi".into());
            }
            i
        }
    };
    let recording = app.state::<AppState>().meeting.lock().is_some();
    if recording {
        return Err("Právě nahráváš schůzku — aktualizuj, až skončí".into());
    }
    let path = updater::download(&app, &http, &info).await?;
    dlog!("update downloaded: {}", path.display());
    updater::run_installer(&path)?;
    // let the installer start, then get out of its way
    tokio::time::sleep(Duration::from_millis(600)).await;
    app.exit(0);
    Ok(())
}

/// Look for a new version now and then; tell the main window (and once via the bar).
fn watch_updates(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(25)).await;
        let mut told = String::new();
        loop {
            let http = app.state::<AppState>().http.clone();
            match updater::check(&http).await {
                Ok(info) => {
                    *UPDATE.lock() = Some(info.clone());
                    if info.available {
                        let _ = app.emit_to("main", "whiskr:update", &info);
                        if told != info.latest {
                            told = info.latest.clone();
                            bar_state(&app, "notify", Some(&format!("✨ Nová verze Whiskru {} — Aktualizovat najdeš v aplikaci", info.latest)));
                        }
                    }
                }
                Err(e) => dlog!("update check failed: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// Open a picture an agent generated in the default image viewer.
#[tauri::command]
fn open_image(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::Path::new(&path);
    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
    if !["png", "jpg", "jpeg", "webp", "gif"].contains(&ext.as_str()) || !p.is_file() {
        return Err("Obrázek nenalezen".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// Open a file attached in the chat (only from the attachments folder).
#[tauri::command]
fn open_attachment(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::Path::new(&path);
    if !p.is_file() || !p.starts_with(attach_dir(&app)) {
        return Err("Soubor nenalezen".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// Diagnostic line from a page (goes to whiskr-debug.log).
#[tauri::command]
async fn dbg_log(window: tauri::WebviewWindow, msg: String) {
    dlog!("[{}] {}", window.label(), msg);
}

fn hide_ask_later(app: &AppHandle, gen: u64, ms: u64) {
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        if CLOSE_GEN.load(std::sync::atomic::Ordering::SeqCst) == gen {
            hide_ask(&app2);
        }
    });
}

/// Diagnostics: note in the log whenever the UI thread is stuck for a while.
fn watch_main_thread(app: AppHandle) {
    std::thread::Builder::new()
        .name("whiskr-ui-watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(400));
            let t0 = Instant::now();
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            if app.run_on_main_thread(move || {
                let _ = tx.send(());
            }).is_err() {
                return;
            }
            if rx.recv_timeout(Duration::from_millis(400)).is_err() {
                let _ = rx.recv_timeout(Duration::from_secs(30));
                dlog!("UI thread was busy for {} ms", t0.elapsed().as_millis());
            }
        })
        .ok();
}

/// Shift+Space reached the chat page itself (it had the keyboard) — act like the shortcut.
#[tauri::command]
async fn ask_toggle(app: AppHandle) {
    dlog!("ask_toggle from page");
    toggle_ask(&app);
}

/// The page started its closing animation: hide the window after `ms`
/// (unless it's opened again / the close is cancelled meanwhile).
#[tauri::command]
async fn ask_hide_after(app: AppHandle, ms: u64) {
    ASK_WANT.store(false, AO::SeqCst);
    ASK_CLOSING.store(true, AO::SeqCst);
    let gen = CLOSE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    hide_ask_later(&app, gen, ms.clamp(0, 600));
}

/// The page decided to stay open after all: drop the pending safety hide.
#[tauri::command]
async fn ask_close_cancel(app: AppHandle) {
    dlog!("ask_close_cancel");
    ASK_WANT.store(true, AO::SeqCst);
    ASK_CLOSING.store(false, AO::SeqCst);
    CLOSE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // it stays open: make sure it has the keyboard
    ask_show_focus(&app);
}

#[tauri::command]
async fn ask_hide(app: AppHandle) {
    // the page closed it itself: the safety hide isn't needed any more
    ASK_WANT.store(false, AO::SeqCst);
    CLOSE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    hide_ask(&app);
}

/// Hide right away; the page then quietly resets for a smooth next opening.
static CLOSE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Ask the page to play its closing animation (it then calls ask_hide);
/// if the page never answers, hide anyway.
fn close_ask(app: &AppHandle) {
    ASK_WANT.store(false, AO::SeqCst);
    if !ask_visible(app) {
        return;
    }
    ASK_CLOSING.store(true, AO::SeqCst);
    let gen = CLOSE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    dlog!("close_ask gen={gen}");
    let _ = app.emit_to("ask", "whiskr:ask-close", ());
    // The page plays the close animation and then asks us to hide (ask_hide_after).
    // If it doesn't answer in time (busy), hide anyway — never later than this.
    hide_ask_later(app, gen, 650);
}

fn hide_ask(app: &AppHandle) {
    dlog!("hide_ask");
    if let Some(w) = app.get_webview_window("ask") {
        // (its last frame is the end of the leave animation — fully transparent —
        // so showing it again later never flashes old content)
        #[cfg(windows)]
        let _ = &w;
        #[cfg(windows)]
        let ask_h = ASK_HWND.load(AO::SeqCst);
        #[cfg(windows)]
        let had_keyboard = ask_h != 0 && unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize == ask_h;
        #[cfg(windows)]
        let t0 = Instant::now();
        #[cfg(windows)]
        if ask_h != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(ask_h as _, windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE);
            }
            dlog!("hide_ask: hide {} ms", t0.elapsed().as_millis());
        }
        #[cfg(not(windows))]
        let _ = w.hide();
        // Give the keyboard back to where it was. Otherwise it stays on the
        // hidden chat: the next Shift+Space lands in the invisible page instead
        // of reaching Whiskr's shortcut, and nothing opens.
        #[cfg(windows)]
        if had_keyboard {
            use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, IsWindowVisible, SetForegroundWindow};
            let back = *app.state::<AppState>().ask_return.lock();
            let ok = back != 0 && unsafe { IsWindow(back as _) != 0 && IsWindowVisible(back as _) != 0 && SetForegroundWindow(back as _) != 0 };
            dlog!("hide_ask: keyboard back to previous window: {ok}");
        }
        dlog!("hide_ask: visible after = {}", ask_visible(app));
        let _ = app.emit_to("ask", "whiskr:ask-hidden", ());
    }
    ASK_CLOSING.store(false, AO::SeqCst);
    // Shift+Space pressed again while it was closing? Then open it again now.
    if ASK_WANT.load(AO::SeqCst) {
        dlog!("hide_ask: reopened meanwhile");
        kick_ask();
    }
}

/// Hide the ask window and paste the text where the user was typing.
#[tauri::command]
fn ask_insert(app: AppHandle, text: String) {
    ASK_WANT.store(false, AO::SeqCst);
    hide_ask(&app);
    let target = *app.state::<AppState>().ask_target.lock();
    std::thread::spawn(move || {
        #[cfg(windows)]
        if target != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(target as _);
            }
        }
        let _ = target;
        std::thread::sleep(Duration::from_millis(160));
        let _ = paste::insert(&text);
    });
}

#[tauri::command]
fn ask_login(app: AppHandle, engine: String, runtime: String, profile: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let tools = state.tools.lock().clone();
    let tool = agents::pick(&tools, &engine, &runtime).ok_or("Nástroj nenalezen")?;
    let key = profile_key(&state.settings.lock(), Some(&profile));
    agents::open_login(tool, &key)
}

/// Continue in the ChatGPT / Claude app (web) with the text prefilled.
#[tauri::command]
fn ask_open_in(app: AppHandle, target: String, text: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let q: String = text.chars().take(6000).collect();
    let enc: String = q
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    let url = match target.as_str() {
        "chatgpt" => format!("https://chatgpt.com/?q={enc}"),
        _ => format!("https://claude.ai/new?q={enc}"),
    };
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn ask_store_get(state: State<'_, AppState>) -> String {
    std::fs::read_to_string(state.data_dir.join("ask.json")).unwrap_or_else(|_| "{}".into())
}

#[tauri::command]
fn ask_store_set(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let _ = std::fs::create_dir_all(&state.data_dir);
    std::fs::write(state.data_dir.join("ask.json"), data).map_err(|e| e.to_string())
}

// ───────── attachments in the chat ─────────

#[derive(serde::Serialize)]
struct Attachment {
    name: String,
    path: String,
    image: bool,
    size: u64,
}

fn attach_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().data_dir.join("ask-workspace").join("attachments")
}

/// A free file name in the attachments folder ("2026-10-21_2301_screen.png").
fn attach_target(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let dir = attach_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Přílohy: {e}"))?;
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect::<String>()
        .trim()
        .chars()
        .rev()
        .take(80)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let clean = if clean.is_empty() { "soubor".to_string() } else { clean };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut p = dir.join(format!("{stamp}_{clean}"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{stamp}-{i}_{clean}"));
        i += 1;
    }
    Ok(p)
}

fn attachment(app: &AppHandle, p: &std::path::Path, name: &str) -> Attachment {
    let _ = app.asset_protocol_scope().allow_file(p);
    let path = p.to_string_lossy().to_string();
    Attachment { name: name.to_string(), image: agents::is_image(&path), size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0), path }
}

const ATTACH_MAX: u64 = 30 * 1024 * 1024;

/// Files dropped on / picked for the chat: copied next to the chat's workspace
/// (so the agent may open them and they stay with the conversation).
#[tauri::command]
async fn ask_attach_paths(app: AppHandle, paths: Vec<String>) -> Result<Vec<Attachment>, String> {
    let mut out = vec![];
    for src in paths.iter().take(10) {
        let sp = std::path::Path::new(src);
        let meta = std::fs::metadata(sp).map_err(|e| format!("{src}: {e}"))?;
        if meta.is_dir() {
            return Err("Složky přikládat nejde — vyber soubory.".into());
        }
        if meta.len() > ATTACH_MAX {
            return Err(format!("{} je moc velký (max 30 MB).", sp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
        }
        let name = sp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "soubor".into());
        let dst = attach_target(&app, &name)?;
        std::fs::copy(sp, &dst).map_err(|e| format!("{name}: {e}"))?;
        out.push(attachment(&app, &dst, &name));
    }
    Ok(out)
}

/// A pasted picture / file (base64 from the page).
#[tauri::command]
async fn ask_attach_data(app: AppHandle, name: String, data: String) -> Result<Attachment, String> {
    let bytes = b64_decode(&data).ok_or("Vložená data nejdou přečíst")?;
    if bytes.len() as u64 > ATTACH_MAX {
        return Err("Soubor je moc velký (max 30 MB).".into());
    }
    let dst = attach_target(&app, &name)?;
    std::fs::write(&dst, &bytes).map_err(|e| format!("Přílohy: {e}"))?;
    Ok(attachment(&app, &dst, &name))
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.split_once(',').map_or(s, |(h, d)| if h.contains("base64") { d } else { s });
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[tauri::command]
async fn pick_files(app: AppHandle) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_files(move |f| {
        let _ = tx.send(f.map(|v| v.into_iter().map(|p| p.to_string()).collect::<Vec<_>>()));
    });
    rx.await.ok().flatten().unwrap_or_default()
}

/// /screen: a picture of the monitor you're on (the chat itself is hidden
/// from it for that moment), saved as an attachment.
#[tauri::command]
async fn ask_screenshot(app: AppHandle, embedded: bool) -> Result<Attachment, String> {
    #[cfg(windows)]
    let (hwnd, hide) = if embedded {
        (app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0), false)
    } else {
        (ASK_HWND.load(AO::SeqCst), true)
    };
    #[cfg(not(windows))]
    let (hwnd, hide) = (0isize, !embedded);
    let shot = tauri::async_runtime::spawn_blocking(move || capture_screen_png(hwnd, hide)).await.map_err(|e| e.to_string())??;
    let dst = attach_target(&app, "obrazovka.png")?;
    std::fs::write(&dst, shot).map_err(|e| format!("Snímek: {e}"))?;
    dlog!("ask_screenshot: {}", dst.display());
    Ok(attachment(&app, &dst, "obrazovka.png"))
}

#[cfg(windows)]
fn capture_screen_png(hwnd: isize, hide: bool) -> Result<Vec<u8>, String> {
    if hide {
        cloak_ask(true);
    }
    let r = screen::capture(hwnd);
    if hide {
        cloak_ask(false);
    }
    r
}

#[cfg(not(windows))]
fn capture_screen_png(_hwnd: isize, _hide: bool) -> Result<Vec<u8>, String> {
    Err("Snímek obrazovky zatím umím jen ve Windows.".into())
}

/// Attachments older than 60 days go (chats keep working, the pictures just disappear).
fn prune_attachments(dir: PathBuf) {
    let Ok(it) = std::fs::read_dir(&dir) else { return };
    let old = std::time::SystemTime::now() - Duration::from_secs(60 * 24 * 3600);
    for e in it.flatten() {
        if e.metadata().and_then(|m| m.modified()).map_or(false, |t| t < old) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |f| {
        let _ = tx.send(f.map(|p| p.to_string()));
    });
    rx.await.ok().flatten()
}

/// Frosted glass: Windows blurs whatever is behind the window (Acrylic).
/// Used when the background is a colour with some transparency + blur.
#[tauri::command]
fn set_glass(app: AppHandle, window: String, on: bool) -> Result<(), String> {
    if window == "ask" {
        ASK_GLASS.store(on, std::sync::atomic::Ordering::Relaxed);
    }
    use tauri::window::{Effect, EffectsBuilder};
    let w = app.get_webview_window(&window).ok_or("no window")?;
    if on {
        let fx = EffectsBuilder::new().effect(Effect::Acrylic).build();
        if w.set_effects(fx).is_err() {
            // older Windows: plain blur
            w.set_effects(EffectsBuilder::new().effect(Effect::Blur).build()).map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        {
            if let Ok(h) = w.hwnd() {
                // round the window itself, so the glass has no square corners
                use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE};
                let round: i32 = 2;
                unsafe {
                    DwmSetWindowAttribute(h.0 as _, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &round as *const i32 as *const _, 4);
                }
            }
        }
    } else {
        let _ = w.set_effects(None::<tauri::utils::config::WindowEffectsConfig>);
    }
    Ok(())
}

/// Pick a background image and keep a copy in the app's data folder.
#[tauri::command]
async fn pick_background(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Obrázky", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
        .pick_file(move |f| {
            let _ = tx.send(f.and_then(|p| p.into_path().ok()));
        });
    let Some(src) = rx.await.ok().flatten() else { return Ok(None) };
    let dir = app.state::<AppState>().data_dir.join("backgrounds");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let dst = dir.join(format!("bg-{}.{ext}", chrono::Local::now().format("%Y%m%d%H%M%S")));
    std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
    Ok(Some(dst.to_string_lossy().to_string()))
}

// ───────────────────────────── MCP / Claude ─────────────────────────────

const MCP_NAME: &str = if cfg!(windows) { "whiskr-mcp.exe" } else { "whiskr-mcp" };

/// whiskr-mcp next to the running Whiskr.exe (changes with every build folder).
fn mcp_bundled() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(MCP_NAME))).filter(|p| p.exists())
}

/// A stable copy, so Claude's config keeps working when Whiskr moves or updates.
fn mcp_stable() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Whiskr").join(MCP_NAME))
}

/// Refresh the stable copy from the bundled one (best effort — it may be in use by Claude).
fn install_mcp() {
    let (Some(src), Some(dst)) = (mcp_bundled(), mcp_stable()) else { return };
    let same = match (std::fs::metadata(&src), std::fs::metadata(&dst)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() <= b.modified().ok(),
        _ => false,
    };
    if !same {
        if let Some(dir) = dst.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::copy(&src, &dst);
    }
}

fn mcp_exe() -> PathBuf {
    mcp_stable()
        .filter(|p| p.exists())
        .or_else(mcp_bundled)
        .unwrap_or_else(|| PathBuf::from(MCP_NAME))
}

#[derive(serde::Serialize)]
struct McpInfo {
    path: String,
    exists: bool,
    desktop_json: String,
    claude_code_cmd: String,
    claude_desktop_configs: Vec<String>,
}

/// Every place Claude Desktop may keep its config (standard install + Microsoft Store).
fn claude_desktop_configs() -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(c) = dirs::config_dir() {
        out.push(c.join("Claude").join("claude_desktop_config.json"));
    }
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        if let Ok(it) = std::fs::read_dir(local.join("Packages")) {
            for e in it.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") {
                    let dir = e.path().join("LocalCache").join("Roaming").join("Claude");
                    if dir.exists() {
                        out.push(dir.join("claude_desktop_config.json"));
                    }
                }
            }
        }
    }
    out
}

#[tauri::command]
fn mcp_info() -> McpInfo {
    let p = mcp_exe();
    let path = p.to_string_lossy().to_string();
    let desktop_json = serde_json::to_string_pretty(&json!({ "mcpServers": { "whiskr": { "command": path } } })).unwrap_or_default();
    McpInfo {
        exists: p.exists(),
        claude_code_cmd: format!("claude mcp add whiskr -- \"{path}\""),
        desktop_json,
        claude_desktop_configs: claude_desktop_configs()
            .into_iter()
            .filter(|c| c.parent().map(|d| d.exists()).unwrap_or(false))
            .map(|c| c.to_string_lossy().to_string())
            .collect(),
        path,
    }
}

/// Add Whiskr to Claude Desktop's config (keeps everything else, makes a backup).
#[tauri::command]
fn connect_claude_desktop() -> Result<Vec<String>, String> {
    let exe = mcp_exe();
    if !exe.exists() {
        return Err("Chybí whiskr-mcp.exe vedle Whiskr.exe".into());
    }
    let mut targets: Vec<PathBuf> = claude_desktop_configs()
        .into_iter()
        .filter(|c| c.parent().map(|d| d.exists()).unwrap_or(false))
        .collect();
    if targets.is_empty() {
        let Some(first) = claude_desktop_configs().into_iter().next() else {
            return Err("Nenašla jsem složku Claude Desktop".into());
        };
        std::fs::create_dir_all(first.parent().unwrap()).map_err(|e| e.to_string())?;
        targets.push(first);
    }
    let mut done = vec![];
    for cfg in targets {
        let mut v: serde_json::Value = match std::fs::read_to_string(&cfg) {
            Ok(s) if !s.trim().is_empty() => {
                let _ = std::fs::write(cfg.with_extension("json.whiskr-backup"), &s);
                serde_json::from_str(&s).map_err(|e| format!("{}: neplatný JSON ({e})", cfg.display()))?
            }
            _ => json!({}),
        };
        if !v.is_object() {
            v = json!({});
        }
        if !v["mcpServers"].is_object() {
            v["mcpServers"] = json!({});
        }
        v["mcpServers"]["whiskr"] = json!({ "command": exe.to_string_lossy() });
        std::fs::write(&cfg, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
        done.push(cfg.to_string_lossy().to_string());
    }
    Ok(done)
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

// ───────────────────────────── entry ─────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Another Whiskr.exe (e.g. a newer build from a different folder) was started
/// while this one runs: hand over to it instead of showing the old version.
fn takeover(app: &AppHandle, args: &[String]) -> bool {
    let Some(other) = args.first().map(PathBuf::from) else { return false };
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let Ok(me) = std::env::current_exe() else { return false };
    if !other.is_file() || canon(&other) == canon(&me) {
        return false;
    }
    if app.state::<AppState>().meeting.lock().is_some() {
        // never drop a running recording; tell the user instead
        bar_state(app, "notify", Some("Nahrávání běží — novou verzi spusť po jeho skončení"));
        return false;
    }
    // Start the other Whiskr directly and tell it to wait until we've quit
    // (no cmd.exe / ping delay tricks — that pattern is what malware uses).
    let path = canon(&other).to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let ok = std::process::Command::new(&path)
        .args(["--after", &std::process::id().to_string()])
        .spawn()
        .is_ok();
    if ok {
        app.exit(0);
        return true;
    }
    false
}

/// Started by an older Whiskr handing over (`--after <pid>`): wait until it
/// has really quit, so the single-instance check lets us in.
fn wait_for_previous() {
    let args: Vec<String> = std::env::args().collect();
    let Some(pid) = args.iter().position(|a| a == "--after").and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u32>().ok()) else { return };
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 10_000);
            CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

pub fn run() {
    wait_for_previous();
    dlog::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if takeover(app, &args) {
                return;
            }
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    on_shortcut(app, shortcut, event.state() == ShortcutState::Pressed);
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let settings_path = config_dir.join("settings.json");
            let history_path = data_dir.join("history.json");
            let recordings_dir = data_dir.join("recordings");
            let _ = std::fs::create_dir_all(&recordings_dir);
            // Recordings interrupted by a crash/quit mid-processing.
            for r in notes::list(&recordings_dir) {
                if r.status == "processing" {
                    let dir = recordings_dir.join(&r.id);
                    if let Some(mut full) = notes::load(&dir) {
                        full.status = "error".into();
                        full.error = Some("Zpracování bylo přerušeno — klikni na Zpracovat znovu".into());
                        let _ = notes::save(&dir, &full);
                    }
                }
            }
            let settings = Settings::load(&settings_path);
            let history = History::load(&history_path);
            let first_run = settings.api_key.trim().is_empty();
            let hotkey_settings = settings.clone();

            let level_app = handle.clone();
            let level: recorder::LevelFn = Arc::new(move |rms: f32| {
                let _ = level_app.emit_to("bar", "whiskr:level", rms);
            });

            app.manage(AppState {
                phase: Mutex::new(Phase::Idle),
                recorder: Recorder::new(level),
                settings: Mutex::new(settings),
                history: Mutex::new(history),
                paused: Mutex::new(false),
                hotkeys: Mutex::new(vec![]),
                kind: Mutex::new(Kind::Dictate),
                target: Mutex::new(context::AppInfo::default()),
                pause_item: Mutex::new(None),
                lock_item: Mutex::new(None),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(60))
                    .connect_timeout(Duration::from_secs(8))
                    .build()?,
                settings_path,
                history_path,
                recordings_dir,
                meeting: Mutex::new(None),
                bookmarks: Mutex::new(vec![]),
                rec_item: Mutex::new(None),
                tools: Mutex::new(vec![]),
                ask_target: Mutex::new(0),
                ask_return: Mutex::new(0),
                run_seq: std::sync::atomic::AtomicU64::new(1),
                data_dir: data_dir.clone(),
                tasks: Mutex::new(vec![]),
                ask_just_opened: Mutex::new(None),
                watch: Mutex::new(assistant::Watch::default()),
            });
            assistant::load_tasks(&handle.state::<AppState>());
            #[cfg(windows)]
            {
                let icon = data_dir.join("whiskr-icon.png");
                let _ = std::fs::write(&icon, include_bytes!("../icons/128x128.png"));
                assistant::register_toasts(Some(icon));
            }
            assistant::start_loop(&handle);

            build_tray(&handle)?;
            {
                let state = handle.state::<AppState>();
                let _ = std::fs::create_dir_all(&state.recordings_dir);
                let _ = handle.asset_protocol_scope().allow_directory(&state.recordings_dir, true);
                let bg = state.data_dir.join("backgrounds");
                let _ = std::fs::create_dir_all(&bg);
                let _ = handle.asset_protocol_scope().allow_directory(&bg, true);
                let att = state.data_dir.join("ask-workspace").join("attachments");
                let _ = std::fs::create_dir_all(&att);
                let _ = handle.asset_protocol_scope().allow_directory(&att, true);
                std::thread::spawn(move || prune_attachments(att));
            }
            std::thread::spawn(install_mcp);
            {
                let det = handle.clone();
                std::thread::spawn(move || refresh_tools(&det));
            }
            if let Some(ask) = app.get_webview_window("ask") {
                #[cfg(windows)]
                if let Ok(h) = ask.hwnd() {
                    ASK_HWND.store(h.0 as isize, AO::SeqCst);
                    hotkeys_win::set_ask_window(h.0 as isize);
                    no_dwm_transitions(h.0 as isize);
                    bare_ask_frame(h.0 as isize);
                    // WebView2 paints a blank frame the first time a window is
                    // shown. Do that first show now, cloaked (DWM doesn't put it on
                    // screen, no activation), so Shift+Space never sees it.
                    let hwnd = h.0 as isize;
                    cloak_ask(true);
                    std::thread::spawn(move || {
                        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
                        std::thread::sleep(Duration::from_millis(1200));
                        unsafe {
                            if IsWindowVisible(hwnd as _) != 0 || ASK_WANT.load(AO::SeqCst) {
                                cloak_ask(false);
                                return; // already opened by the user
                            }
                            ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
                            std::thread::sleep(Duration::from_millis(350));
                            if !ASK_WANT.load(AO::SeqCst) {
                                ShowWindow(hwnd as _, SW_HIDE);
                            } else {
                                cloak_ask(false);
                            }
                        }
                    });
                }
                let hide_on_blur = handle.clone();
                ask.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        let _ = hide_on_blur.emit_to("ask", "whiskr:ask-blur", ());
                    }
                });
            }

            if let Some(bar) = app.get_webview_window("bar") {
                let _ = bar.set_ignore_cursor_events(true);
                make_bar_passive(&bar);
            }
            place_bar(&handle);

            #[cfg(windows)]
            {
                let keys_app = handle.clone();
                hotkeys_win::start(move |ev| match ev {
                    hotkeys_win::Event::Pressed(id) => {
                        dlog!("key pressed: {id}");
                        on_action(&keys_app, &id, true)
                    }
                    hotkeys_win::Event::Released(id) => on_action(&keys_app, &id, false),
                });
                hotkeys_win::set_games(app.state::<AppState>().settings.lock().game_apps.clone());
                let game_app = handle.clone();
                hotkeys_win::watch_games(move |game| {
                    if !hotkeys_win::is_locked() {
                        bar_state(&game_app, "notify", Some(if game { "🎮 Hra vepředu — zkratky Whiskru pauznuté" } else { "Zkratky Whiskru zase fungují" }));
                    }
                    let _ = game_app.emit_to("main", "whiskr:lock", lock_status());
                });
            }
            if let Err(e) = register_hotkeys(&handle, &hotkey_settings) {
                // Tell the user once the bar is up; whatever could be registered works.
                let warn = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(4500)).await;
                    bar_state(&warn, "error", Some(&e));
                });
            }

            // Rescue recordings interrupted by a crash or shutdown.
            let rescue = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                recover_recordings(&rescue);
            });

            dlog::set_enabled(app.state::<AppState>().settings.lock().debug_log);
            watch_updates(handle.clone());
            start_ask_applier(handle.clone());
            watch_main_thread(handle.clone());
            let started_in_background = std::env::args().any(|a| a == "--background");
            if first_run || !started_in_background {
                // Don't show the window until the page has painted its intro
                // overlay (it calls main_ready) — otherwise a grey, empty window
                // flashes first. Fallback: show after 4 s regardless.
                MAIN_WAIT.store(true, std::sync::atomic::Ordering::SeqCst);
                let late = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    if MAIN_WAIT.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        show_main(&late);
                    }
                });
            }

            // Little hello from the cat once the bar has loaded.
            let hello = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                bar_state(&hello, "hello", None);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window just tucks Whiskr into the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            check_key,
            get_history,
            delete_entry,
            clear_history,
            copy_text,
            list_mics,
            get_paused,
            toggle_paused,
            get_lock,
            toggle_lock,
            suspend_hotkey,
            preview_bar,
            meeting_status,
            start_meeting,
            stop_meeting,
            list_recordings,
            get_recording,
            delete_recording,
            rename_recording,
            rename_speaker,
            reprocess_recording,
            recording_markdown,
            open_recording_folder,
            list_voices,
            add_bookmark,
            recording_audio,
            ask_recordings,
            search_recordings,
            list_templates,
            resummarize,
            update_utterance,
            export_docx,
            forget_voice,
            mcp_info,
            connect_claude_desktop,
            ask_tools,
            ask_run,
            ask_cancel,
            ask_open_with,
            set_window_theme,
            open_main,
            ask_models,
            pick_background,
            set_glass,
            mcp_hub::connections_status,
            mcp_hub::account_logout,
            mcp_hub::mcp_status,
            mcp_hub::mcp_install,
            ask_layout,
            ask_hide,
            ask_close_cancel,
            ask_hide_after,
            ask_uncloak,
            ask_next_open,
            ask_toggle,
            dbg_log,
            check_update,
            install_update,
            open_image,
            main_ready,
            ask_insert,
            ask_login,
            ask_open_in,
            ask_store_get,
            ask_store_set,
            pick_folder,
            pick_files,
            ask_attach_paths,
            ask_attach_data,
            ask_screenshot,
            open_attachment,
            app_version,
            assistant::brain_status,
            assistant::brain_connectors,
            assistant::list_tasks,
            assistant::add_task,
            assistant::update_task,
            assistant::delete_task,
            assistant::offer_answer,
            assistant::get_briefing,
            assistant::refresh_briefing,
            assistant::brief_material
        ])
        .run(tauri::generate_context!())
        .expect("error while running Whiskr");
}
