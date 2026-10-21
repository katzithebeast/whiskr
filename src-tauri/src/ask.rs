//! Search and "ask" over recordings.
//!
//! No vector database: transcripts are split into short passages and ranked by
//! word overlap with the question (with Czech-friendly prefix matching). The best
//! passages go to the LLM, which answers with citations like [[ID@MM:SS]] that
//! the UI turns into clickable jumps in the player.

use crate::notes::{self, fmt_ts, Recording};
use crate::groq;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

const STOP: &[&str] = &[
    "a", "i", "o", "u", "v", "ve", "s", "se", "z", "ze", "k", "ke", "na", "do", "od", "po", "pro", "pri", "při", "je", "jsou",
    "byl", "bylo", "byla", "jsem", "jsme", "jste", "to", "ten", "ta", "tu", "ty", "co", "jak", "kdy", "kde", "kdo", "proc",
    "proč", "ale", "nebo", "že", "ze", "tak", "už", "uz", "jen", "jsi", "mi", "mě", "me", "ho", "jim", "jí", "the", "and",
    "or", "of", "to", "in", "on", "for", "is", "are", "was", "what", "who", "when", "how", "did", "we", "you", "it",
];

/// Lowercase word stems (first 5 chars) — crude, but works well for Czech endings.
pub fn stems(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2 && !STOP.contains(w))
        .map(|w| w.chars().take(5).collect())
        .collect()
}

#[derive(Serialize, Clone, Debug)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub created: String,
    pub time: f64,
    pub speaker: String,
    pub snippet: String,
    pub score: f32,
}

struct Passage {
    id: String,
    title: String,
    created: String,
    start: f64,
    speaker: String,
    text: String,
    score: f32,
}

/// Passages of ~4 utterances (overlapping) per recording, scored against the query.
fn passages(rec: &Recording, q: &HashSet<String>) -> Vec<Passage> {
    let mut out = vec![];
    let u = &rec.utterances;
    let created = rec.created.format("%Y-%m-%d %H:%M").to_string();
    let mut i = 0;
    while i < u.len() {
        let end = (i + 4).min(u.len());
        let text: String = u[i..end]
            .iter()
            .map(|x| format!("[{}] {}: {}", fmt_ts(x.start), rec.name_of(&x.speaker), x.text))
            .collect::<Vec<_>>()
            .join("\n");
        let words = stems(&text);
        let found: HashSet<&String> = words.iter().filter(|w| q.contains(*w)).collect();
        let hits = words.iter().filter(|w| q.contains(*w)).count();
        // distinct matches matter most; repeats help a little
        let score = found.len() as f32 * 2.0 + (hits as f32).sqrt();
        out.push(Passage {
            id: rec.id.clone(),
            title: rec.title.clone(),
            created: created.clone(),
            start: u[i].start,
            speaker: rec.name_of(&u[i].speaker),
            text,
            score,
        });
        i += 3;
    }
    out
}

pub fn summary_text(rec: &Recording) -> String {
    let Some(s) = &rec.summary else { return String::new() };
    let mut t = format!("Summary: {}\n", s.overview);
    if !s.action_items.is_empty() {
        t.push_str(&format!("Tasks: {}\n", s.action_items.join("; ")));
    }
    if !s.decisions.is_empty() {
        t.push_str(&format!("Decisions: {}\n", s.decisions.join("; ")));
    }
    for sec in &s.sections {
        t.push_str(&format!("{}: {}\n", sec.title, sec.items.join("; ")));
    }
    t
}

pub fn load_all(root: &Path) -> Vec<Recording> {
    let mut v: Vec<Recording> = std::fs::read_dir(root)
        .map(|it| it.flatten().filter_map(|e| notes::load(&e.path())).filter(|r| r.status == "done").collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.created.cmp(&a.created));
    v
}

/// Full-text search across all recordings (titles, summaries, transcripts).
pub fn search(root: &Path, query: &str, limit: usize) -> Vec<Hit> {
    let q: HashSet<String> = stems(query).into_iter().collect();
    let needle = query.trim().to_lowercase();
    if q.is_empty() && needle.is_empty() {
        return vec![];
    }
    let mut hits: Vec<Hit> = vec![];
    for rec in load_all(root) {
        let title_bonus = if rec.title.to_lowercase().contains(&needle) { 3.0 } else { 0.0 };
        for (i, u) in rec.utterances.iter().enumerate() {
            let low = u.text.to_lowercase();
            let words = stems(&u.text);
            let distinct: HashSet<&String> = words.iter().filter(|w| q.contains(*w)).collect();
            let exact = !needle.is_empty() && low.contains(&needle);
            if distinct.is_empty() && !exact {
                continue;
            }
            let score = distinct.len() as f32 * 2.0 + if exact { 4.0 } else { 0.0 } + title_bonus;
            let _ = i;
            hits.push(Hit {
                id: rec.id.clone(),
                title: rec.title.clone(),
                created: rec.created.to_rfc3339(),
                time: u.start,
                speaker: rec.name_of(&u.speaker),
                snippet: u.text.chars().take(240).collect(),
                score,
            });
        }
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(b.created.cmp(&a.created)));
    hits.truncate(limit);
    hits
}

const ASK_SYSTEM: &str = "You answer questions about the user's recorded meetings, calls and notes, using ONLY the material \
provided. Answer in the language of the question (usually Czech), concisely and concretely. After every claim, cite where it \
comes from as [[RECORDING_ID@MM:SS]] using the ids and timestamps in the material (e.g. [[20261020-150000@12:34]]). \
If the material doesn't contain the answer, say so plainly — never guess or invent. You may use short bullet lists.";

/// Budget for the material we send (free-tier token limits are small).
const BUDGET: usize = 14_000;

pub async fn ask(
    client: &Client,
    key: &str,
    root: &Path,
    id: Option<&str>,
    question: &str,
    history: &[(String, String)],
) -> Result<String, String> {
    let q: HashSet<String> = stems(question).into_iter().collect();
    let recs: Vec<Recording> = match id {
        Some(id) => vec![notes::load(&root.join(id)).ok_or("Nahrávka nenalezena")?],
        None => load_all(root),
    };
    if recs.is_empty() {
        return Err("Zatím nemáš žádné zpracované nahrávky".into());
    }

    let mut material = String::new();
    // Summaries first: cheap and they answer most "what was decided" questions.
    let mut summaries = String::new();
    for r in recs.iter().take(if id.is_some() { 1 } else { 25 }) {
        let s = summary_text(r);
        if !s.is_empty() {
            summaries.push_str(&format!("### [{}] «{}» {}\n{}\n", r.id, r.title, r.created.format("%Y-%m-%d"), s));
        }
    }
    let summary_budget = if id.is_some() { 3000 } else { 6000 };
    if summaries.len() > summary_budget {
        summaries = summaries.chars().take(summary_budget).collect();
    }
    material.push_str(&summaries);

    // Then the best-matching transcript passages (or the whole transcript if it fits).
    let single_full = id.is_some() && {
        let total: usize = recs[0].utterances.iter().map(|u| u.text.len() + 24).sum();
        total + material.len() < BUDGET
    };
    if single_full {
        let r = &recs[0];
        material.push_str(&format!("\n### Full transcript of [{}]\n", r.id));
        for u in &r.utterances {
            material.push_str(&format!("[{}] {}: {}\n", fmt_ts(u.start), r.name_of(&u.speaker), u.text));
        }
    } else {
        let mut all: Vec<Passage> = recs.iter().flat_map(|r| passages(r, &q)).filter(|p| p.score > 0.0).collect();
        all.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        material.push_str("\n### Relevant transcript passages\n");
        for p in all {
            let block = format!("--- [{}] «{}» {} (from {} {})\n{}\n", p.id, p.title, p.created, fmt_ts(p.start), p.speaker, p.text);
            if material.len() + block.len() > BUDGET {
                break;
            }
            material.push_str(&block);
        }
    }

    let mut convo = String::new();
    for (q, a) in history.iter().rev().take(4).rev() {
        convo.push_str(&format!("User: {q}\nAssistant: {}\n", a.chars().take(600).collect::<String>()));
    }
    let user = format!(
        "Material:\n{material}\n\n{}Question: {question}",
        if convo.is_empty() { String::new() } else { format!("Earlier in this chat:\n{convo}\n") }
    );
    groq::chat_text(client, key, ASK_SYSTEM, &user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn czech_stems_match_word_forms() {
        let q: HashSet<String> = stems("Co jsme slíbili Acme ohledně e-mailů?").into_iter().collect();
        let t = stems("Pošleme Acme návrh napojení e-mailu do pátku, slíbil Filip");
        assert!(t.iter().any(|w| q.contains(w)), "{q:?} vs {t:?}");
    }
}
