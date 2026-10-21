//! Windows global shortcuts via a low-level keyboard hook (like AutoHotkey).
//!
//! Why not RegisterHotKey: combos can be silently taken by other apps, Alt+Shift
//! collides with the keyboard-layout switch, and Right Alt is AltGr (Ctrl+Alt) on
//! Czech keyboards. The hook sees the physical keys (scan codes, so QWERTZ/QWERTY
//! doesn't matter), never conflicts, and tells us reliably about press AND release.
//!
//! The hook callback must return within milliseconds, so it only queues events;
//! a worker thread runs the actual actions.

use parking_lot::Mutex;
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::keyspec::{Binding, ALT, CTRL, SHIFT, WIN};
/// Unassigned virtual key, used to "mask" Alt/Win so swallowing the main key
/// doesn't open an app's menu bar, the Start menu, or switch the keyboard layout.
const MASK_VK: u16 = 0xE8;

#[derive(Default)]
struct HookState {
    bindings: Vec<Binding>,
    /// Escape cancels a dictation — only while one is running.
    escape: bool,
    suspended: bool,
    /// Binding currently held down: (id, scan, vk)
    held: Option<(String, u32, u32)>,
    /// Another key was typed since the modifiers went down. Shift-only combos
    /// (Shift+Space) then don't fire — so "Ahoj světe" typed fast with Shift
    /// still held never opens anything.
    dirty: bool,
    /// The Whiskr Ask window: Shift+Backspace hides it while it's in front.
    ask_hwnd: isize,
    /// When the held key last went down / repeated. Windows sends auto-repeats
    /// every ~30 ms, so a long silence means we missed its release.
    held_seen: Option<std::time::Instant>,
}

pub enum Event {
    Pressed(String),
    Released(String),
}

static STATE: OnceLock<Mutex<HookState>> = OnceLock::new();
static TX: OnceLock<Mutex<Sender<Event>>> = OnceLock::new();

fn state() -> &'static Mutex<HookState> {
    STATE.get_or_init(|| Mutex::new(HookState::default()))
}

/// Start the hook thread and the worker that runs `handler`.
pub fn start(handler: impl Fn(Event) + Send + 'static) {
    let (tx, rx) = mpsc::channel::<Event>();
    let _ = TX.set(Mutex::new(tx));
    std::thread::Builder::new()
        .name("whiskr-keys-worker".into())
        .spawn(move || {
            while let Ok(ev) = rx.recv() {
                handler(ev);
            }
        })
        .expect("keys worker");
    std::thread::Builder::new()
        .name("whiskr-keys-hook".into())
        .spawn(|| unsafe {
            let hmod = GetModuleHandleW(std::ptr::null());
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0);
            if hook.is_null() {
                eprintln!("keyboard hook failed");
                return;
            }
            // A low-level hook needs a message loop on its thread.
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        })
        .expect("keys hook");
}

pub fn set_bindings(b: Vec<Binding>) {
    let mut s = state().lock();
    s.bindings = b;
    s.held = None;
}

pub fn set_ask_window(h: isize) {
    state().lock().ask_hwnd = h;
}

pub fn set_escape(on: bool) {
    state().lock().escape = on;
}

// ─── lock for gaming: Whiskr lets every key through ───
/// Locked by hand (tray, app, Ctrl+Alt+Shift+L).
static LOCKED: AtomicBool = AtomicBool::new(false);
/// A game from the user's list is in front right now (checked twice a second).
static GAME_FRONT: AtomicBool = AtomicBool::new(false);
static GAMES: Mutex<Vec<String>> = parking_lot::const_mutex(Vec::new());

pub fn set_locked(on: bool) {
    LOCKED.store(on, Ordering::Relaxed);
    let mut s = state().lock();
    s.held = None;
}
pub fn is_locked() -> bool {
    LOCKED.load(Ordering::Relaxed)
}
pub fn game_in_front() -> bool {
    GAME_FRONT.load(Ordering::Relaxed)
}
pub fn set_games(list: Vec<String>) {
    *GAMES.lock() = list.into_iter().map(|g| g.trim().to_lowercase()).filter(|g| !g.is_empty()).collect();
}

/// Exe name of the window in front, lowercase ("league of legends.exe").
pub fn foreground_exe() -> String {
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
        windows_sys::Win32::Foundation::CloseHandle(h);
        if !ok {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize]).rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
    }
}

/// Watch which app is in front; `on_change(game_now)` when a game comes / goes.
pub fn watch_games(on_change: impl Fn(bool) + Send + 'static) {
    std::thread::Builder::new()
        .name("whiskr-game-watch".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let exe = foreground_exe();
            let game = !exe.is_empty() && GAMES.lock().iter().any(|g| *g == exe);
            if GAME_FRONT.swap(game, Ordering::Relaxed) != game {
                crate::dlog::log(format!("game in front: {game} ({exe})"));
                on_change(game);
            }
        })
        .ok();
}

pub fn set_suspended(on: bool) {
    let mut s = state().lock();
    s.suspended = on;
    s.held = None;
}

fn down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

// Our own record of the modifiers, from the physical key events this hook
// sees. Windows' GetAsyncKeyState was seen to report Shift as "up" while it
// was held (right after the chat took focus), so Shift+Space leaked through.
// A modifier counts as down if Windows says so, OR we saw it go down and not
// up within the last 2 s (it auto-repeats while held, so a missed release
// can't leave it stuck for longer).
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
static MOD_DOWN: [AtomicBool; 4] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
static MOD_AT: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

fn now_ms() -> u64 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
}

/// 0 = Ctrl, 1 = Alt, 2 = Shift, 3 = Win
fn mod_slot(vk: u32) -> Option<usize> {
    match vk {
        0x11 | 0xA2 | 0xA3 => Some(0),
        0x12 | 0xA4 | 0xA5 => Some(1),
        0x10 | 0xA0 | 0xA1 => Some(2),
        0x5B | 0x5C => Some(3),
        _ => None,
    }
}

fn track_modifier(vk: u32, is_down: bool) {
    if let Some(i) = mod_slot(vk) {
        MOD_DOWN[i].store(is_down, Ordering::Relaxed);
        if is_down {
            MOD_AT[i].store(now_ms(), Ordering::Relaxed);
        }
    }
}

fn mod_down(i: usize, vks: &[u16]) -> bool {
    vks.iter().any(|&v| down(v)) || (MOD_DOWN[i].load(Ordering::Relaxed) && now_ms().saturating_sub(MOD_AT[i].load(Ordering::Relaxed)) < 2000)
}

fn current_mods() -> u8 {
    let mut m = 0;
    if mod_down(0, &[VK_CONTROL]) {
        m |= CTRL;
    }
    if mod_down(1, &[VK_MENU]) {
        m |= ALT;
    }
    if mod_down(2, &[VK_SHIFT]) {
        m |= SHIFT;
    }
    if mod_down(3, &[VK_LWIN, VK_RWIN]) {
        m |= WIN;
    }
    m
}

fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn send(ev: Event) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().send(ev);
    }
}

/// A window of this process (Whiskr) is in front.
fn own_window_in_front() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, &mut pid);
        pid == GetCurrentProcessId()
    }
}

/// The Ask window is the foreground window.
pub fn ask_in_front() -> bool {
    let h = state().lock().ask_hwnd;
    h != 0 && unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() } as isize == h
}

/// Send one Backspace to the app in front (a Shift+Backspace tap we held back).
pub fn send_backspace() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0x08, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(0), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

/// Is a key physically down right now?
pub fn key_down(vk: u16) -> bool {
    down(vk)
}

fn tap_mask_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: MASK_VK, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(0), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = &*(lparam as *const KBDLLHOOKSTRUCT);
        let msg = wparam as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        // Ignore keys we (or other tools) inject, e.g. our own Ctrl+V.
        if (k.flags & LLKHF_INJECTED) == 0 && is_down && is_modifier(k.vkCode) && current_mods() == 0 {
            // a fresh modifier press starts a clean combo
            state().lock().dirty = false;
        }
        if (k.flags & LLKHF_INJECTED) == 0 && (is_down || is_up) && is_modifier(k.vkCode) {
            track_modifier(k.vkCode, is_down);
        }
        if (k.flags & LLKHF_INJECTED) == 0 && (is_down || is_up) && !is_modifier(k.vkCode) {
            if let Some(swallow) = handle_key(k.vkCode, k.scanCode, is_down) {
                if swallow {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Returns Some(true) to swallow the key.
fn handle_key(vk: u32, scan: u32, is_down: bool) -> Option<bool> {
    let mut s = state().lock();
    if s.suspended {
        return None;
    }
    if is_down {
        // Ctrl+Alt+Shift+L: lock / unlock — works even while locked.
        if vk == 0x4C && current_mods() == CTRL | ALT | SHIFT {
            drop(s);
            tap_mask_key();
            send(Event::Pressed("lock_toggle".into()));
            return Some(true);
        }
        // Locked (by hand or a game is in front): every key goes to the app.
        if LOCKED.load(Ordering::Relaxed) || GAME_FRONT.load(Ordering::Relaxed) {
            return None;
        }
        // Auto-repeat of the key we're already holding.
        if let Some((id, hs, hv)) = s.held.clone() {
            // Auto-repeats arrive every ~30-500 ms (the first one after the
            // Windows "repeat delay", at most 1 s). So the same key again within
            // 1.5 s = still held. Longer silence = its release slipped past us
            // and this is a brand-new press. (Asking Windows whether the key is
            // down does NOT work here: keys we swallow never count as pressed.)
            let fresh = s.held_seen.map_or(true, |t| t.elapsed() < std::time::Duration::from_millis(1500));
            if fresh && hs == scan && hv == vk {
                s.held_seen = Some(std::time::Instant::now());
                return Some(true);
            }
            if !fresh && hs == scan && hv == vk {
                crate::dlog::log(format!("hook: stale held key {id}, release was missed"));
                // its release never reached us (focus change, secure desktop…):
                // forget it, otherwise the next press is eaten as a "repeat"
                // and Shift+Space seems to do nothing.
                s.held = None;
                s.held_seen = None;
                drop(s);
                send(Event::Released(id));
                return handle_key(vk, scan, is_down);
            }
        }
        // Shift+Backspace: tap = close the chat (or a normal backspace
        // elsewhere — re-sent on release), hold = talk into the chat.
        // Only when Shift went down on its own (typing "Ahoj" + Shift+Backspace
        // to fix a typo stays a plain backspace).
        if vk == 0x08 && current_mods() == SHIFT && !s.dirty && s.ask_hwnd != 0 && !fullscreen_foreground() {
            s.held = Some(("ask_voice".into(), scan, vk));
            s.held_seen = Some(std::time::Instant::now());
            drop(s);
            send(Event::Pressed("ask_voice".into()));
            return Some(true);
        }
        // Shift+Space held + R → a brand new Ask chat.
        if vk == 0x52 && matches!(&s.held, Some((id, _, _)) if id == "ask") {
            drop(s);
            send(Event::Pressed("ask_new".into()));
            return Some(true);
        }
        if vk == 0x1B && s.escape && current_mods() == 0 {
            drop(s);
            send(Event::Pressed("escape".into()));
            return Some(true);
        }
        let mods = current_mods();
        let dirty = s.dirty;
        let hit = s
            .bindings
            .iter()
            .find(|b| b.mods == mods && (b.scan.map(|x| x == scan).unwrap_or(false) || b.vk.map(|x| x == vk).unwrap_or(false)))
            .filter(|b| !(b.mods == SHIFT && (dirty || fullscreen_foreground())))
            .cloned();
        if vk == 0x20 && mods == 0 && own_window_in_front() {
            crate::dlog::log(format!("hook Space with no modifiers while Whiskr is in front (async shift={})", down(VK_SHIFT)));
        }
        if vk == 0x20 && mods != 0 {
            crate::dlog::log(format!(
                "hook Space mods={mods} hit={} dirty={} held={:?}",
                hit.is_some(),
                dirty,
                s.held.as_ref().map(|h| h.0.clone())
            ));
        }
        if hit.is_none() {
            s.dirty = true;
        }
        if let Some(b) = hit {
            s.held = Some((b.id.clone(), scan, vk));
            s.held_seen = Some(std::time::Instant::now());
            drop(s);
            // Swallowing the key means the system saw "modifiers pressed and released
            // alone", which can open a menu bar / Start menu or switch the keyboard
            // layout (Alt+Shift, Ctrl+Shift). A harmless masking key prevents that.
            if mods != 0 {
                tap_mask_key();
            }
            send(Event::Pressed(b.id));
            return Some(true);
        }
        None
    } else {
        match &s.held {
            Some((id, hs, hv)) if *hs == scan && *hv == vk => {
                let id = id.clone();
                s.held = None;
                s.held_seen = None;
                drop(s);
                send(Event::Released(id));
                Some(true)
            }
            _ => None,
        }
    }
}


/// A game, video or presentation in fullscreen: Shift-only shortcuts stay
/// with the app (Shift+Space = jump/sprint in games).
///
/// Strict on purpose — an earlier, looser check ("window covers the monitor")
/// also matched live-wallpaper apps, maximized borderless windows and Whiskr
/// itself, and Shift+Space then silently did nothing.
fn fullscreen_foreground() -> bool {
    fullscreen_reason().is_some()
}

/// Why the foreground counts as fullscreen (for the debug log), or None.
pub fn fullscreen_reason() -> Option<String> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId};
    unsafe {
        // What Windows itself thinks (the same signal it uses to hold back notifications).
        let mut quns = 0;
        if SHQueryUserNotificationState(&mut quns) != 0 {
            return None;
        }
        if quns == QUNS_RUNNING_D3D_FULL_SCREEN || quns == QUNS_PRESENTATION_MODE {
            return Some(format!("windows state {quns}"));
        }
        if quns != QUNS_BUSY {
            return None;
        }
        // "busy" = some window fills the screen. Make sure it's a real app.
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == GetCurrentProcessId() {
            return None; // Whiskr itself
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if ["WorkerW", "Progman", "Shell_TrayWnd", "Windows.UI.Core.CoreWindow"].contains(&class.as_str()) {
            return None; // desktop, taskbar, Start
        }
        let mut exe = String::new();
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !h.is_null() {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                exe = String::from_utf16_lossy(&buf[..len as usize]).rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
            }
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
        // live wallpapers sit full-screen on the desktop
        if ["wallpaper32.exe", "wallpaper64.exe", "lively.exe", "livelywpf.exe", "wallpaperengine.exe", "explorer.exe"].contains(&exe.as_str()) {
            return None;
        }
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let m = mi.rcMonitor;
        if r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom {
            Some(format!("{exe} [{class}]"))
        } else {
            None
        }
    }
}
