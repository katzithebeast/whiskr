//! Notice when a call starts (Teams, Zoom, Meet in a browser, …) so Whiskr can
//! offer to record it — and notice when it ends.
//!
//! Windows keeps track of which apps use the microphone right now
//! (Settings → Privacy → Microphone). That list is in the registry:
//! an app whose `LastUsedTimeStop` is 0 is using the mic at this moment.

/// Apps that are calls by themselves: (needle in exe / package name, label).
const CALL_APPS: &[(&str, &str)] = &[
    ("msteams", "Teams"),
    ("ms-teams", "Teams"),
    ("teams.exe", "Teams"),
    ("zoom", "Zoom"),
    ("webex", "Webex"),
    ("ciscocollabhost", "Webex"),
    ("slack", "Slack"),
    ("discord", "Discord"),
    ("skype", "Skype"),
    ("whatsapp", "WhatsApp"),
    ("telegram", "Telegram"),
    ("signal", "Signal"),
    ("gotomeeting", "GoTo"),
    ("facebook.messenger", "Messenger"),
    ("messenger", "Messenger"),
];

const BROWSERS: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc", "zen"];

/// Window title fragments that mean a web meeting: (needle, label).
const WEB_MEETINGS: &[(&str, &str)] = &[
    ("google meet", "Google Meet"),
    ("meet.google.com", "Google Meet"),
    ("meet –", "Google Meet"),
    ("meet - ", "Google Meet"),
    ("microsoft teams", "Teams"),
    ("| microsoft teams", "Teams"),
    ("zoom", "Zoom"),
    ("whereby", "Whereby"),
    ("jitsi", "Jitsi"),
    ("webex", "Webex"),
    ("discord", "Discord"),
    ("hangouts", "Hangouts"),
];

/// Decide whether a call is going on. `mic_apps` = exe paths / package names
/// using the mic now, `titles` = visible window titles.
pub fn classify(mic_apps: &[String], titles: &[String]) -> Option<String> {
    let mut browser_on_mic = false;
    for app in mic_apps {
        let low = app.to_lowercase();
        let name = low.rsplit(['#', '\\', '/']).next().unwrap_or(&low).to_string();
        if name.contains("whiskr") {
            continue;
        }
        if let Some((_, label)) = CALL_APPS.iter().find(|(n, _)| name.contains(n)) {
            return Some(label.to_string());
        }
        if BROWSERS.iter().any(|b| name.starts_with(b)) {
            browser_on_mic = true;
        }
    }
    if browser_on_mic {
        for t in titles {
            let low = t.to_lowercase();
            if let Some((_, label)) = WEB_MEETINGS.iter().find(|(n, _)| low.contains(n)) {
                return Some(label.to_string());
            }
        }
    }
    None
}

#[cfg(windows)]
pub fn mic_apps() -> Vec<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let mut out = vec![];
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(base) = root.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone") else {
        return out;
    };
    let mut check = |key: &RegKey, name: &str| {
        let stop: u64 = key.get_value("LastUsedTimeStop").unwrap_or(1);
        let start: u64 = key.get_value("LastUsedTimeStart").unwrap_or(0);
        if stop == 0 && start > 0 {
            out.push(name.to_string());
        }
    };
    for name in base.enum_keys().flatten() {
        if name == "NonPackaged" {
            if let Ok(np) = base.open_subkey(&name) {
                for exe in np.enum_keys().flatten() {
                    if let Ok(k) = np.open_subkey(&exe) {
                        check(&k, &exe);
                    }
                }
            }
        } else if let Ok(k) = base.open_subkey(&name) {
            check(&k, &name);
        }
    }
    out
}

#[cfg(not(windows))]
pub fn mic_apps() -> Vec<String> {
    // For testing on Linux: a file named in WHISKR_FAKE_CALL ("teams.exe") pretends a call is on.
    std::env::var("WHISKR_FAKE_CALL").ok().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).into_iter().collect()
}

#[cfg(windows)]
pub fn window_titles() -> Vec<String> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible};
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp as *mut Vec<String>);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 && len < 512 {
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 {
                out.push(String::from_utf16_lossy(&buf[..n as usize]));
            }
        }
        1
    }
    let mut v: Vec<String> = vec![];
    unsafe {
        EnumWindows(Some(cb), &mut v as *mut Vec<String> as LPARAM);
    }
    v
}

#[cfg(not(windows))]
pub fn window_titles() -> Vec<String> {
    std::env::var("WHISKR_FAKE_TITLE").ok().into_iter().collect()
}

/// What's going on right now.
pub fn current() -> Option<String> {
    let apps = mic_apps();
    if apps.is_empty() {
        return None;
    }
    classify(&apps, &window_titles())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn desktop_call_apps() {
        assert_eq!(classify(&s(&["MSTeams_8wekyb3d8bbwe"]), &[]), Some("Teams".into()));
        assert_eq!(classify(&s(&[r"C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe"]), &[]), Some("Zoom".into()));
        assert_eq!(classify(&s(&[r"C:#Program Files#Whiskr#Whiskr.exe"]), &[]), None);
    }

    #[test]
    fn browser_needs_meeting_title() {
        let chrome = s(&[r"C:#Program Files#Google#Chrome#Application#chrome.exe"]);
        assert_eq!(classify(&chrome, &s(&["Inbox - Gmail - Google Chrome"])), None);
        assert_eq!(classify(&chrome, &s(&["Meet – abc-defg-hij - Google Chrome"])), Some("Google Meet".into()));
        // A meeting title without anything on the mic is not a call.
        assert_eq!(classify(&[], &s(&["Google Meet"])), None);
    }
}
