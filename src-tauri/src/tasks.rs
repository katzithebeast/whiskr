//! Tasks & reminders: "Alt+Shift+T — zítra v 9 zavolat Chrisovi" → a task with
//! a reminder. Stored as one small JSON file next to the settings.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    pub created: DateTime<Local>,
    /// When to remind (or the deadline). None = whenever.
    #[serde(default)]
    pub due: Option<DateTime<Local>>,
    /// false = only a day was said ("zítra"), the time is our default.
    #[serde(default)]
    pub has_time: bool,
    /// The reminder already went off.
    #[serde(default)]
    pub notified: bool,
    /// "voice" | "manual" | "meeting" | "ask"
    #[serde(default)]
    pub source: String,
    /// Recording id for tasks taken from a meeting.
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub done_at: Option<DateTime<Local>>,
    /// What the user actually said (shown on hover, used if parsing failed).
    #[serde(default)]
    pub said: String,
}

pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!("{}{:03}", Local::now().format("%Y%m%d%H%M%S%3f"), N.fetch_add(1, Ordering::Relaxed) % 1000)
}

impl Task {
    pub fn plain(text: &str, source: &str) -> Task {
        Task {
            id: new_id(),
            text: text.trim().to_string(),
            done: false,
            created: Local::now(),
            due: None,
            has_time: false,
            notified: false,
            source: source.into(),
            source_id: None,
            done_at: None,
            said: text.trim().to_string(),
        }
    }
}

pub fn load(path: &Path) -> Vec<Task> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(path: &Path, tasks: &[Task]) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(tasks) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// Hour used when only a day was said ("zítra").
pub const DEFAULT_HOUR: u32 = 9;

const WEEKDAYS_CS: [&str; 7] = ["pondělí", "úterý", "středa", "čtvrtek", "pátek", "sobota", "neděle"];

pub const PARSE_SYSTEM: &str = "You turn a short spoken note into to-do items for a personal task list. Reply with JSON only.";

pub fn parse_prompt(now: DateTime<Local>, said: &str) -> String {
    let wd = WEEKDAYS_CS[now.weekday().num_days_from_monday() as usize];
    format!(
        "Now is {} ({wd}), local time {}.\n\
The user said (speech-to-text, may contain small errors):\n\"\"\"{said}\"\"\"\n\n\
Return {{\"tasks\": [{{\"text\": \"…\", \"due\": \"YYYY-MM-DDTHH:MM\" | \"YYYY-MM-DD\" | null}}]}}.\n\
Rules:\n\
- One item per separate thing to do; usually just one. Keep the user's language.\n\
- \"text\" is short and actionable, starts with a verb, without the time words and without \"připomeň mi\" / \"remind me\" / \"úkol\". Fix obvious speech-to-text typos in names.\n\
- \"due\" only when a time or day is said: \"zítra v 9\" → tomorrow 09:00; \"v pátek\" → that date (no time); \"za hodinu\" / \"za 20 minut\" → now + that; \"ráno\" 08:00, \"dopoledne\" 10:00, \"odpoledne\" 14:00, \"večer\" 19:00. Weekday names mean the next such day (today only if a later time today is said).\n\
- Never invent a due date.",
        now.format("%Y-%m-%d"),
        now.format("%H:%M")
    )
}

/// Parse a due value: full local datetime or just a date.
pub fn parse_due(v: &Value) -> Option<(DateTime<Local>, bool)> {
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_end_matches('Z');
    for f in ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Local.from_local_datetime(&n).earliest().map(|d| (d, true));
        }
    }
    let day = NaiveDate::parse_from_str(&s[..s.len().min(10)], "%Y-%m-%d").ok()?;
    let n = day.and_time(NaiveTime::from_hms_opt(DEFAULT_HOUR, 0, 0)?);
    Local.from_local_datetime(&n).earliest().map(|d| (d, false))
}

/// The model's reply → tasks (falls back to the raw sentence).
pub fn from_reply(v: &Value, said: &str, source: &str) -> Vec<Task> {
    let mut out = vec![];
    for t in v["tasks"].as_array().into_iter().flatten() {
        let text = t["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let mut task = Task::plain(text, source);
        task.said = said.trim().to_string();
        if let Some((due, has_time)) = parse_due(&t["due"]) {
            task.due = Some(due);
            task.has_time = has_time;
        }
        out.push(task);
    }
    if out.is_empty() && !said.trim().is_empty() {
        out.push(Task::plain(said, source));
    }
    out
}

/// Reminders that should go off now. Old misses (PC was off) are marked
/// as handled quietly — they still show as overdue in the list.
pub fn due_now(tasks: &mut [Task], now: DateTime<Local>) -> Vec<Task> {
    let mut fire = vec![];
    for t in tasks.iter_mut() {
        if t.done || t.notified {
            continue;
        }
        let Some(due) = t.due else { continue };
        if due <= now {
            t.notified = true;
            if now - due < chrono::Duration::hours(12) {
                fire.push(t.clone());
            }
        }
    }
    fire
}

/// "dnes 14:00", "zítra 9:00", "pá 24. 10."
pub fn when_label(due: DateTime<Local>, has_time: bool, now: DateTime<Local>) -> String {
    let days = (due.date_naive() - now.date_naive()).num_days();
    let day = match days {
        0 => "dnes".to_string(),
        1 => "zítra".to_string(),
        -1 => "včera".to_string(),
        2..=6 => WEEKDAYS_CS[due.weekday().num_days_from_monday() as usize].to_string(),
        _ => due.format("%-d. %-m.").to_string(),
    };
    if has_time {
        format!("{day} {}", due.format("%-H:%M"))
    } else {
        day
    }
}

/// Short plain-text list for the briefing / brief prompts.
pub fn summary_for_prompt(tasks: &[Task], now: DateTime<Local>, limit: usize) -> String {
    let mut open: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();
    open.sort_by_key(|t| (t.due.is_none(), t.due));
    open.iter()
        .take(limit)
        .map(|t| match t.due {
            Some(d) => {
                let late = if d < now { " (PO TERMÍNU)" } else { "" };
                format!("- {} [{}]{late}", t.text, when_label(d, t.has_time, now))
            }
            None => format!("- {}", t.text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reply_to_tasks() {
        let v = json!({"tasks": [{"text": "Zavolat Chrisovi", "due": "2026-10-22T09:00"}, {"text": "Koupit mléko", "due": null}, {"text": "Poslat nabídku", "due": "2026-10-24"}]});
        let t = from_reply(&v, "zítra v 9 zavolat Chrisovi a koupit mléko", "voice");
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].due.unwrap().format("%Y-%m-%d %H:%M").to_string(), "2026-10-22 09:00");
        assert!(t[0].has_time);
        assert!(t[1].due.is_none());
        assert!(!t[2].has_time);
        assert_eq!(t[2].due.unwrap().format("%H:%M").to_string(), "09:00");
    }

    #[test]
    fn falls_back_to_raw_sentence() {
        let t = from_reply(&json!({"oops": 1}), "zavolat mámě", "voice");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].text, "zavolat mámě");
    }

    #[test]
    fn fires_once_and_skips_old() {
        let now = Local::now();
        let mut a = Task::plain("a", "voice");
        a.due = Some(now - chrono::Duration::minutes(1));
        let mut b = Task::plain("b", "voice");
        b.due = Some(now - chrono::Duration::days(2));
        let mut c = Task::plain("c", "voice");
        c.due = Some(now + chrono::Duration::hours(1));
        let mut list = vec![a, b, c];
        let fired = due_now(&mut list, now);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].text, "a");
        assert!(list[1].notified && !list[2].notified);
        assert!(due_now(&mut list, now).is_empty());
    }
}
