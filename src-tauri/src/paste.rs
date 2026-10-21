//! Put text where the cursor is: clipboard + paste shortcut, then restore the
//! user's original clipboard. Works in practically every app.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread::sleep;
use std::time::Duration;

#[cfg(target_os = "macos")]
const PASTE_MOD: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PASTE_MOD: Key = Key::Control;

#[cfg(target_os = "windows")]
fn v_key() -> Key {
    // Virtual-key code for V, independent of keyboard layout (QWERTZ etc.).
    Key::Other(0x56)
}
#[cfg(not(target_os = "windows"))]
fn v_key() -> Key {
    Key::Unicode('v')
}

pub fn insert(text: &str) -> Result<(), String> {
    let previous = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok());
    insert_restoring(text, previous)
}

/// Paste `text`, then put `previous` back into the clipboard.
pub fn insert_restoring(text: &str, previous: Option<String>) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Schránka: {e}"))?;

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Schránka: {e}"))?;
    sleep(Duration::from_millis(40));

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("Klávesnice: {e:?}"))?;
    // If the user is still holding modifiers from the hotkey, Ctrl+Shift+V would
    // paste as plain text or do something else entirely. Let go of them first —
    // but only the ones really held: a stray Win/Alt key-up can open the Start
    // menu or focus an app's menu bar.
    for k in held_modifiers() {
        let _ = enigo.key(k, Direction::Release);
    }
    sleep(Duration::from_millis(15));
    enigo.key(PASTE_MOD, Direction::Press).map_err(|e| format!("{e:?}"))?;
    let r = enigo.key(v_key(), Direction::Click);
    let _ = enigo.key(PASTE_MOD, Direction::Release);
    r.map_err(|e| format!("Vložení: {e:?}"))?;

    // Give the target app time to read the clipboard, then put the old one back
    // (only if nobody changed it in the meantime).
    sleep(Duration::from_millis(350));
    if let Some(prev) = previous {
        if clipboard.get_text().ok().as_deref() == Some(text) {
            let _ = clipboard.set_text(prev);
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn c_key() -> Key {
    Key::Other(0x43)
}
#[cfg(not(target_os = "windows"))]
fn c_key() -> Key {
    Key::Unicode('c')
}

/// Copy whatever is selected in the focused app.
/// Returns (selected text, the clipboard as it was before) — the caller restores it.
pub fn read_selection() -> Result<(Option<String>, Option<String>), String> {
    read_selection_wait(12)
}

/// Same, waiting at most `tries` × 40 ms for the app to copy.
pub fn read_selection_wait(tries: u32) -> Result<(Option<String>, Option<String>), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Schránka: {e}"))?;
    let original = clipboard.get_text().ok();
    // A marker tells "nothing selected" apart from "selected the same text again".
    let marker = format!("\u{2063}whiskr-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    clipboard.set_text(marker.clone()).map_err(|e| format!("Schránka: {e}"))?;
    sleep(Duration::from_millis(30));

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("Klávesnice: {e:?}"))?;
    for k in held_modifiers() {
        let _ = enigo.key(k, Direction::Release);
    }
    sleep(Duration::from_millis(15));
    enigo.key(PASTE_MOD, Direction::Press).map_err(|e| format!("{e:?}"))?;
    let r = enigo.key(c_key(), Direction::Click);
    let _ = enigo.key(PASTE_MOD, Direction::Release);
    r.map_err(|e| format!("Kopírování: {e:?}"))?;

    let mut selection = None;
    for _ in 0..tries {
        sleep(Duration::from_millis(40));
        if let Ok(t) = clipboard.get_text() {
            if t != marker {
                if !t.trim().is_empty() {
                    selection = Some(t);
                }
                break;
            }
        }
    }
    Ok((selection, original))
}

pub fn set_clipboard(text: Option<String>) {
    if let (Some(t), Ok(mut c)) = (text, arboard::Clipboard::new()) {
        let _ = c.set_text(t);
    }
}

#[cfg(target_os = "windows")]
fn held_modifiers() -> Vec<Key> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    let down = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
    let mut keys = Vec::new();
    if down(VK_SHIFT) {
        keys.push(Key::Shift);
    }
    if down(VK_MENU) {
        keys.push(Key::Alt);
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        keys.push(Key::Meta);
    }
    keys
}

#[cfg(not(target_os = "windows"))]
fn held_modifiers() -> Vec<Key> {
    vec![Key::Shift]
}

/// Dictating sentence after sentence should not glue them together
/// ("…test.Další věta"). Add one space after a finished sentence.
pub fn with_trailing_space(text: &str) -> String {
    let t = text.trim_end();
    match t.chars().last() {
        Some('.' | '!' | '?' | '…' | ':' | ';' | ',') if !t.contains('\n') => format!("{t} "),
        _ => t.to_string(),
    }
}

pub fn copy(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .map_err(|e| e.to_string())
}
