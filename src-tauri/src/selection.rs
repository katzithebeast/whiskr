//! The text selected in the app in front — read through UI Automation, the
//! accessibility interface screen readers use. Nothing is copied, no keys are
//! sent, the clipboard is never touched.
//!
//! Works in most apps (Word, Notepad, Edge/Chrome, Outlook, VS Code…). Where it
//! doesn't, the caller decides whether a Ctrl+C fallback is allowed.

#[cfg(windows)]
pub fn focused_selection() -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, UIA_TextPatternId};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut el: IUIAutomationElement = uia.GetFocusedElement().ok()?;
        let walker = uia.ControlViewWalker().ok();
        // the focused element itself, or (e.g. in browsers) one of its parents, has the text
        for _ in 0..4 {
            if let Ok(tp) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                if let Ok(ranges) = tp.GetSelection() {
                    let n = ranges.Length().unwrap_or(0);
                    let mut out = String::new();
                    for i in 0..n {
                        if let Ok(r) = ranges.GetElement(i) {
                            if let Ok(t) = r.GetText(20_000) {
                                out.push_str(&t.to_string());
                            }
                        }
                    }
                    let out = out.trim().to_string();
                    return (!out.is_empty()).then_some(out);
                }
            }
            match walker.as_ref().and_then(|w| w.GetParentElement(&el).ok()) {
                Some(p) => el = p,
                None => break,
            }
        }
        None
    }
}

#[cfg(not(windows))]
pub fn focused_selection() -> Option<String> {
    None
}
