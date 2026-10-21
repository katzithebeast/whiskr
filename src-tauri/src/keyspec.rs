//! Shortcut specs ("ctrl+shift+Space") → modifiers + physical key.
//! Shared by the Windows keyboard hook; testable on any OS.

pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
pub const WIN: u8 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub id: String,
    pub mods: u8,
    pub scan: Option<u32>,
    pub vk: Option<u32>,
}

/// Physical key (browser `KeyboardEvent.code`) → PC/AT scan code.
fn scan_of(code: &str) -> Option<u32> {
    let letters = [
        ("Q", 0x10), ("W", 0x11), ("E", 0x12), ("R", 0x13), ("T", 0x14), ("Y", 0x15), ("U", 0x16), ("I", 0x17), ("O", 0x18),
        ("P", 0x19), ("A", 0x1E), ("S", 0x1F), ("D", 0x20), ("F", 0x21), ("G", 0x22), ("H", 0x23), ("J", 0x24), ("K", 0x25),
        ("L", 0x26), ("Z", 0x2C), ("X", 0x2D), ("C", 0x2E), ("V", 0x2F), ("B", 0x30), ("N", 0x31), ("M", 0x32),
    ];
    let c = code.trim();
    let up = c.to_uppercase();
    if let Some(l) = up.strip_prefix("KEY") {
        return letters.iter().find(|(n, _)| *n == l).map(|x| x.1);
    }
    if let Some(d) = up.strip_prefix("DIGIT") {
        let n: u32 = d.parse().ok()?;
        return Some(if n == 0 { 0x0B } else { 0x01 + n });
    }
    Some(match up.as_str() {
        "BACKQUOTE" => 0x29,
        "MINUS" => 0x0C,
        "EQUAL" => 0x0D,
        "BACKSPACE" => 0x0E,
        "TAB" => 0x0F,
        "BRACKETLEFT" => 0x1A,
        "BRACKETRIGHT" => 0x1B,
        "ENTER" => 0x1C,
        "SEMICOLON" => 0x27,
        "QUOTE" => 0x28,
        "BACKSLASH" => 0x2B,
        "COMMA" => 0x33,
        "PERIOD" => 0x34,
        "SLASH" => 0x35,
        "SPACE" => 0x39,
        "F1" => 0x3B,
        "F2" => 0x3C,
        "F3" => 0x3D,
        "F4" => 0x3E,
        "F5" => 0x3F,
        "F6" => 0x40,
        "F7" => 0x41,
        "F8" => 0x42,
        "F9" => 0x43,
        "F10" => 0x44,
        "F11" => 0x57,
        "F12" => 0x58,
        "INSERT" => 0x52,
        "DELETE" => 0x53,
        "HOME" => 0x47,
        "END" => 0x4F,
        "PAGEUP" => 0x49,
        "PAGEDOWN" => 0x51,
        "ARROWUP" => 0x48,
        "ARROWDOWN" => 0x50,
        "ARROWLEFT" => 0x4B,
        "ARROWRIGHT" => 0x4D,
        _ => return None,
    })
}

/// Keys without a stable scan code: match by virtual key instead.
fn vk_of(code: &str) -> Option<u32> {
    let up = code.trim().to_uppercase();
    if let Some(n) = up.strip_prefix("NUMPAD").and_then(|d| d.parse::<u32>().ok()) {
        return Some(0x60 + n);
    }
    if let Some(n) = up.strip_prefix('F').and_then(|d| d.parse::<u32>().ok()) {
        if (13..=24).contains(&n) {
            return Some(0x7C + n - 13);
        }
    }
    match up.as_str() {
        "PAUSE" => Some(0x13),
        "PRINTSCREEN" => Some(0x2C),
        "SCROLLLOCK" => Some(0x91),
        _ => None,
    }
}

/// Parse "ctrl+shift+Space" / "alt+shift+KeyR".
pub fn parse(id: &str, spec: &str) -> Result<Binding, String> {
    let mut mods = 0u8;
    let mut key: Option<&str> = None;
    for part in spec.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => mods |= CTRL,
            "alt" | "option" => mods |= ALT,
            "shift" => mods |= SHIFT,
            "super" | "win" | "cmd" | "command" | "meta" => mods |= WIN,
            _ => {
                if key.is_some() {
                    return Err(format!("neplatná zkratka „{spec}“"));
                }
                key = Some(part);
            }
        }
    }
    let key = key.ok_or_else(|| format!("zkratka „{spec}“ nemá hlavní klávesu"))?;
    let scan = scan_of(key);
    let vk = vk_of(key);
    if scan.is_none() && vk.is_none() {
        return Err(format!("klávesu „{key}“ neznám"));
    }
    Ok(Binding { id: id.to_string(), mods, scan, vk })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_specs() {
        let b = parse("record", "alt+shift+KeyR").unwrap();
        assert_eq!(b.mods, ALT | SHIFT);
        assert_eq!(b.scan, Some(0x13));
        let b = parse("dictate", "ctrl+shift+Space").unwrap();
        assert_eq!(b.mods, CTRL | SHIFT);
        assert_eq!(b.scan, Some(0x39));
        assert_eq!(parse("x", "F9").unwrap().scan, Some(0x43));
        assert_eq!(parse("x", "ctrl+Numpad5").unwrap().vk, Some(0x65));
        assert_eq!(parse("x", "Digit1").unwrap().scan, Some(0x02));
        assert!(parse("x", "ctrl+shift").is_err());
    }
}
