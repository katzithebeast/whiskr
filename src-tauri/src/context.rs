//! Which app is the user typing into? Lets Whiskr adapt the tone:
//! e-mail → polite, chat → casual, code/terminal → precise prompt, …

use serde::Serialize;

#[derive(Clone, Default, Debug, Serialize)]
pub struct AppInfo {
    /// Executable name, lowercase, e.g. "slack.exe".
    pub exe: String,
    /// Window title (helps for web apps: "Gmail", "ChatGPT", …).
    pub title: String,
}

#[cfg(windows)]
pub fn foreground() -> AppInfo {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return AppInfo::default();
        }
        let mut title = [0u16; 512];
        let n = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
        let title = String::from_utf16_lossy(&title[..n]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut exe = String::new();
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !h.is_null() {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                exe = path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
            }
            CloseHandle(h);
        }
        AppInfo { exe, title }
    }
}

#[cfg(not(windows))]
pub fn foreground() -> AppInfo {
    AppInfo::default()
}

/// "email" | "chat" | "ai" | "code" | "doc" | "other"
pub fn category(info: &AppInfo) -> &'static str {
    let exe = info.exe.as_str();
    let title = info.title.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| title.contains(w));
    let is = |names: &[&str]| names.iter().any(|n| exe == *n);

    // Web apps first: the browser itself tells us nothing.
    if has(&["gmail", "outlook", "seznam email", "e-mail", "doručená pošta", "inbox", "thunderbird", "proton mail"]) {
        return "email";
    }
    if has(&["chatgpt", "claude", "gemini", "perplexity", "copilot", "mistral", "grok"]) {
        return "ai";
    }
    if has(&["slack", "discord", "whatsapp", "messenger", "telegram", "microsoft teams", "signal", "google chat"]) {
        return "chat";
    }
    if has(&["google docs", "dokumenty google", "notion", "confluence", "obsidian"]) {
        return "doc";
    }
    if is(&["outlook.exe", "olk.exe", "thunderbird.exe", "mailbird.exe", "hxoutlook.exe"]) {
        return "email";
    }
    if is(&["slack.exe", "teams.exe", "ms-teams.exe", "discord.exe", "whatsapp.exe", "telegram.exe", "signal.exe", "messenger.exe"]) {
        return "chat";
    }
    if is(&["claude.exe", "chatgpt.exe"]) {
        return "ai";
    }
    if is(&[
        "code.exe", "cursor.exe", "windsurf.exe", "zed.exe", "idea64.exe", "pycharm64.exe", "webstorm64.exe", "rider64.exe",
        "clion64.exe", "goland64.exe", "devenv.exe", "sublime_text.exe", "notepad++.exe", "windowsterminal.exe",
        "wezterm-gui.exe", "alacritty.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "warp.exe", "conhost.exe", "mintty.exe",
        "opencode.exe",
    ]) {
        return "code";
    }
    if is(&["winword.exe", "notion.exe", "obsidian.exe", "onenote.exe", "soffice.bin", "swriter.exe", "wordpad.exe"]) {
        return "doc";
    }
    "other"
}

/// Extra cleanup instructions for the given kind of app.
pub fn tone_rules(category: &str) -> &'static str {
    match category {
        "email" => "- The text goes into an E-MAIL. Use complete, well-formed sentences and a polite, professional tone \
(still the speaker's own words). Keep a greeting or sign-off only if the speaker said one; put them on their own lines. \
Split into short paragraphs. Never invent a subject line or signature.\n",
        "chat" => "- The text goes into a CHAT MESSAGE (Slack, Teams, WhatsApp…). Keep it short, casual and natural, like a person \
typing in chat. No greeting or sign-off unless spoken. Avoid stiff, formal wording.\n",
        "ai" => "- The text is a PROMPT for an AI assistant. Keep every detail and intent of the speaker, make it clear and \
unambiguous, and use a short list when the speaker lists several requirements. Do not answer the prompt.\n",
        "code" => "- The text goes into a CODE EDITOR or TERMINAL, usually as an instruction for an AI coding agent. Keep technical \
terms, file names, function and variable names, CLI commands and English jargon exactly as meant (e.g. \"package json\" → \
\"package.json\"). Prefer precise, imperative instructions. Use a numbered list when the speaker enumerates steps. \
Never add code fences or quotes unless spoken.\n",
        "doc" => "- The text goes into a DOCUMENT. Use well-formed written sentences and paragraphs.\n",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, title: &str) -> AppInfo {
        AppInfo { exe: exe.into(), title: title.into() }
    }

    #[test]
    fn categories() {
        assert_eq!(category(&app("chrome.exe", "Doručená pošta (3) - filip@… - Gmail")), "email");
        assert_eq!(category(&app("slack.exe", "general - Acme")), "chat");
        assert_eq!(category(&app("cursor.exe", "lib.rs — whiskr")), "code");
        assert_eq!(category(&app("msedge.exe", "ChatGPT")), "ai");
        assert_eq!(category(&app("winword.exe", "Nabídka.docx")), "doc");
        assert_eq!(category(&app("explorer.exe", "Plocha")), "other");
    }
}
