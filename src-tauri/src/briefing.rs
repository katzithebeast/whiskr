//! Morning briefing and "brief before a meeting".
//!
//! Whiskr gathers what it knows locally (open tasks, reminders, yesterday's
//! meetings, past meetings with a client) and lets Claude write it up. Claude
//! runs with the user's own connectors (calendar, mail…), so it can add what
//! today holds — and whatever the user's own scheduled tasks produce.

use crate::ask;
use crate::notes::Recording;
use crate::tasks::{self, Task};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Briefing {
    /// "2026-10-21"
    pub date: String,
    pub text: String,
    pub generated: Option<DateTime<Local>>,
    /// "Claude" | "Groq" | "" (local only)
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn load(path: &Path) -> Option<Briefing> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn save(path: &Path, b: &Briefing) {
    if let Ok(s) = serde_json::to_string_pretty(b) {
        let _ = std::fs::write(path, s);
    }
}

pub const BRIEFING_SYSTEM: &str = "You are Whiskr, a warm, practical personal assistant. You write the user's short morning briefing in Czech (unless told otherwise), in Markdown. Read-only: never send, create, edit or delete anything.";

pub fn briefing_prompt(now: DateTime<Local>, tasks_list: &[Task], recent: &[Recording], with_tools: bool, extra: &str) -> String {
    let open = tasks::summary_for_prompt(tasks_list, now, 25);
    let mut meetings = String::new();
    for r in recent {
        let s = ask::summary_text(r);
        if !s.is_empty() {
            meetings.push_str(&format!("### {} — {}\n{}\n", r.created.format("%-d. %-m. %H:%M"), r.title, s));
        }
    }
    let tools_part = if with_tools {
        "Use the tools you have (calendar, e-mail, chat, documents…) to add what's relevant for TODAY: meetings with times, \
important unread or waiting e-mails/messages, deadlines. If a tool isn't available or fails, just skip that part silently."
    } else {
        "You have no tools — work only with the material below."
    };
    let extra = extra.trim();
    let extra_part = if extra.is_empty() { String::new() } else { format!("\nThe user's own instructions for the briefing:\n{extra}\n") };
    format!(
        "Today is {} ({}). Write my morning briefing.\n\n{tools_part}\n{extra_part}\n\
Everything is about me, the user — talk to me in the second person. In meeting notes \"Já\" / \"ME\" is me; tasks that belong to other people are what they owe me (put them under \"Navazuje\" as waiting-for), never list them as my to-dos.
Format (keep it short — readable in 1 minute; skip empty sections):\n\
**Dnes** — the day at a glance in 1–2 sentences.\n\
### 📅 Program — today's meetings/events with times.\n\
### ✅ Úkoly — overdue first, then today's; max 7, most important first.\n\
### 📌 Navazuje na včerejšek — follow-ups from recent meetings that are still open.\n\
### 💡 Tip — at most one concrete suggestion for the day (optional).\n\n\
## Whiskr material\n### Open tasks (with due dates)\n{}\n\n### Recent meetings (summaries)\n{}",
        now.format("%Y-%m-%d"),
        now.format("%A"),
        if open.is_empty() { "(none)".into() } else { open },
        if meetings.is_empty() { "(none)".to_string() } else { meetings },
    )
}

/// Local-only briefing when no AI is available.
pub fn plain_briefing(now: DateTime<Local>, tasks_list: &[Task]) -> String {
    let open = tasks::summary_for_prompt(tasks_list, now, 10);
    if open.is_empty() {
        "**Dnes** — žádné otevřené úkoly. Hezký den! ☀️".into()
    } else {
        format!("### ✅ Úkoly\n{open}")
    }
}

// ───────────────────────── brief before a meeting ─────────────────────────

/// Recordings about `query`, newest first, with how well they match.
pub fn related(recs: &[Recording], query: &str, limit: usize) -> Vec<Recording> {
    let q: HashSet<String> = ask::stems(query).into_iter().collect();
    if q.is_empty() {
        return vec![];
    }
    let score = |text: &str| ask::stems(text).iter().filter(|w| q.contains(*w)).count() as f32;
    let mut scored: Vec<(f32, &Recording)> = recs
        .iter()
        .map(|r| {
            let names: Vec<String> = r.speakers.values().cloned().collect();
            let mut s = score(&r.title) * 4.0 + score(&names.join(" ")) * 3.0 + score(&ask::summary_text(r)) * 2.0;
            if let Some(sum) = &r.summary {
                s += score(&sum.topics.join(" ")) * 2.0;
            }
            let transcript: String = r.utterances.iter().map(|u| u.text.as_str()).collect::<Vec<_>>().join(" ");
            s += score(&transcript).min(6.0) * 0.5;
            (s, r)
        })
        .filter(|(s, _)| *s >= 1.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let mut top: Vec<Recording> = scored.into_iter().take(limit).map(|(_, r)| r.clone()).collect();
    top.sort_by(|a, b| b.created.cmp(&a.created));
    top
}

pub fn brief_prompt(query: &str, recs: &[Recording], tasks_list: &[Task], now: DateTime<Local>) -> String {
    let q: HashSet<String> = ask::stems(query).into_iter().collect();
    let mut material = String::new();
    for r in recs {
        let names: Vec<String> = r.speakers.values().cloned().collect();
        material.push_str(&format!(
            "### {} — «{}» (účastníci: {})\n{}\n",
            r.created.format("%-d. %-m. %Y"),
            r.title,
            names.join(", "),
            ask::summary_text(r)
        ));
    }
    let related_tasks: Vec<String> = tasks_list
        .iter()
        .filter(|t| !t.done && ask::stems(&t.text).iter().any(|w| q.contains(w)))
        .map(|t| match t.due {
            Some(d) => format!("- {} [{}]", t.text, tasks::when_label(d, t.has_time, now)),
            None => format!("- {}", t.text),
        })
        .collect();
    format!(
        "Today is {}. I have a meeting about / with: **{query}**. Write me a brief I can read in 30 seconds:\n\
**Naposledy** — when we last talked and about what (1–2 sentences).\n\
### Co jsem slíbil — my open promises and tasks.\n\
### Co slíbili oni — what they owe me.\n\
### Otevřené otázky — what's still unresolved.\n\
### Na co se zeptat — 2–3 suggested questions or points.\n\
Use only the material below; if something isn't there, leave the section out. Mention dates.\n\n\
## Earlier meetings (newest first)\n{material}\n## My open tasks mentioning it\n{}",
        now.format("%-d. %-m. %Y"),
        if related_tasks.is_empty() { "(none)".into() } else { related_tasks.join("\n") }
    )
}
