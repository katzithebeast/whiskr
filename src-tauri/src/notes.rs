//! Meeting recordings: storage on disk + the processing pipeline
//! (transcribe → who-said-what → summary, action items, decisions).

use crate::{brain, groq, meeting, templates, voices};
use chrono::{DateTime, Local};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const ME: &str = "ME";
pub const OTHERS: &str = "OTHERS";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Utterance {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Summary {
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub action_items: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    /// Template-specific parts ("Potřeby klienta", "Klíčové pojmy", …).
    #[serde(default)]
    pub sections: Vec<Section>,
    /// What was said at the moments the user bookmarked.
    #[serde(default)]
    pub highlights: Vec<Highlight>,
    /// Template actually used (the auto pick, if "auto").
    #[serde(default)]
    pub template: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Section {
    pub title: String,
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Highlight {
    pub time: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub created: DateTime<Local>,
    pub seconds: f64,
    pub has_system: bool,
    /// "processing" | "done" | "error"
    pub status: String,
    /// machine-readable stage while processing: upload | transcribe | speakers | summary
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub error: Option<String>,
    /// "whiskr-voices" | "groq"
    #[serde(default)]
    pub engine: String,
    /// Voice print (centroid) per speaker label, so names can be learned.
    #[serde(default)]
    pub prints: BTreeMap<String, Vec<f32>>,
    /// raw speaker label → display name ("A" → "Petra", "ME" → "Já")
    #[serde(default)]
    pub speakers: BTreeMap<String, String>,
    #[serde(default)]
    pub utterances: Vec<Utterance>,
    #[serde(default)]
    pub summary: Option<Summary>,
    /// Summary template id, or "auto".
    #[serde(default = "auto")]
    pub template: String,
    /// Seconds the user marked as important while recording.
    #[serde(default)]
    pub bookmarks: Vec<f64>,
}

fn auto() -> String {
    "auto".into()
}

impl Recording {
    pub fn new(id: String, seconds: f64, has_system: bool) -> Self {
        let created = Local::now();
        Self {
            title: format!("Nahrávka {}", created.format("%-d. %-m. %H:%M")),
            id,
            created,
            seconds,
            has_system,
            status: "processing".into(),
            stage: "transcribe".into(),
            error: None,
            engine: String::new(),
            prints: BTreeMap::new(),
            speakers: BTreeMap::new(),
            utterances: vec![],
            summary: None,
            template: auto(),
            bookmarks: vec![],
        }
    }

    pub fn name_of(&self, label: &str) -> String {
        self.speakers.get(label).cloned().unwrap_or_else(|| default_name(label))
    }

    /// Light copy for list views.
    pub fn brief(&self) -> Recording {
        let mut r = self.clone();
        r.utterances.clear();
        r.prints.clear();
        r
    }

    pub fn to_markdown(&self) -> String {
        let mut md = format!(
            "# {}\n\n{} · {}\n\n",
            self.title,
            self.created.format("%-d. %-m. %Y %H:%M"),
            fmt_dur(self.seconds)
        );
        let names: Vec<String> = {
            let mut seen = vec![];
            for u in &self.utterances {
                let n = self.name_of(&u.speaker);
                if !seen.contains(&n) {
                    seen.push(n);
                }
            }
            seen
        };
        if !names.is_empty() {
            md.push_str(&format!("**Účastníci:** {}\n\n", names.join(", ")));
        }
        if let Some(s) = &self.summary {
            if !s.overview.is_empty() {
                md.push_str(&format!("## Shrnutí\n\n{}\n\n", s.overview));
            }
            if !s.action_items.is_empty() {
                md.push_str("## Úkoly\n\n");
                for a in &s.action_items {
                    md.push_str(&format!("- [ ] {a}\n"));
                }
                md.push('\n');
            }
            if !s.decisions.is_empty() {
                md.push_str("## Rozhodnutí\n\n");
                for d in &s.decisions {
                    md.push_str(&format!("- {d}\n"));
                }
                md.push('\n');
            }
            for sec in &s.sections {
                if sec.items.is_empty() {
                    continue;
                }
                md.push_str(&format!("## {}\n\n", sec.title));
                for i in &sec.items {
                    md.push_str(&format!("- {i}\n"));
                }
                md.push('\n');
            }
            if !s.highlights.is_empty() {
                md.push_str("## ★ Označené momenty\n\n");
                for h in &s.highlights {
                    md.push_str(&format!("- **[{}]** {}\n", fmt_ts(h.time), h.text));
                }
                md.push('\n');
            }
        }
        md.push_str("## Přepis\n\n");
        for u in &self.utterances {
            md.push_str(&format!("**[{}] {}:** {}\n\n", fmt_ts(u.start), self.name_of(&u.speaker), u.text));
        }
        md
    }
}

pub fn default_name(label: &str) -> String {
    match label {
        ME => "Já".into(),
        OTHERS => "Ostatní".into(),
        l => format!("Mluvčí {l}"),
    }
}

pub fn fmt_ts(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn fmt_dur(s: f64) -> String {
    let m = (s / 60.0).round() as u64;
    if m >= 60 {
        format!("{} h {} min", m / 60, m % 60)
    } else {
        format!("{} min", m.max(1))
    }
}

// ───────────────────────── storage ─────────────────────────

pub fn meta_path(dir: &Path) -> PathBuf {
    dir.join("meta.json")
}

pub fn save(dir: &Path, rec: &Recording) -> Result<(), String> {
    let json = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    let tmp = dir.join("meta.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, meta_path(dir)).map_err(|e| e.to_string())?;
    if rec.status == "done" {
        let _ = std::fs::write(dir.join("prepis.md"), rec.to_markdown());
    }
    Ok(())
}

pub fn load(dir: &Path) -> Option<Recording> {
    std::fs::read_to_string(meta_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn list(root: &Path) -> Vec<Recording> {
    let mut out: Vec<Recording> = std::fs::read_dir(root)
        .map(|it| it.flatten().filter_map(|e| load(&e.path())).map(|r| r.brief()).collect())
        .unwrap_or_default();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

// ───────────────────────── processing ─────────────────────────

pub struct Keys {
    pub custom_templates: Vec<templates::Template>,
    pub groq: String,
    /// Who writes the summary (Claude when connected, else Groq).
    pub brain: brain::Brain,
    pub language: String,
    pub dictionary: Vec<String>,
    /// Where the voice model and remembered voices live.
    pub data_dir: PathBuf,
}

/// Normalise a track's energy so the mic and the (much louder, digital) system
/// track can be compared fairly: divide by the typical level of speech.
fn normalise(e: &[f32]) -> Vec<f32> {
    let mut active: Vec<f32> = e.iter().copied().filter(|v| *v > 0.003).collect();
    if active.is_empty() {
        return vec![0.0; e.len()];
    }
    active.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ref_level = active[active.len() * 3 / 4].max(1e-4);
    e.iter().map(|v| v / ref_level).collect()
}

/// Seconds within [start, end] where the mic clearly dominates, and the total.
fn mic_dominance(mic: &[f32], sys: &[f32], start: f64, end: f64) -> (f64, f64) {
    let a = (start * 10.0) as usize;
    let b = ((end * 10.0) as usize).max(a + 1);
    let (mut dom, mut total) = (0.0, 0.0);
    for i in a..b {
        let m = mic.get(i).copied().unwrap_or(0.0);
        let s = sys.get(i).copied().unwrap_or(0.0);
        if m < 0.15 && s < 0.15 {
            continue; // silence
        }
        total += 0.1;
        if m > s * 1.3 {
            dom += 0.1;
        }
    }
    (dom, total)
}

/// One transcribed track: segments with absolute times, plus word timings.
struct Track {
    segs: Vec<groq::Segment>,
    words: Vec<groq::Word>,
}

async fn transcribe_track(client: &Client, keys: &Keys, samples: &[i16]) -> Result<Track, String> {
    // 10-minute chunks stay well under Groq's 25 MB upload limit.
    let chunk = (meeting::TARGET_RATE * 600) as usize;
    let mut out = Track { segs: vec![], words: vec![] };
    for (ci, part) in samples.chunks(chunk).enumerate() {
        let offset = (ci * chunk) as f64 / meeting::TARGET_RATE as f64;
        let rms = (part.iter().map(|&v| (v as f64 / 32768.0).powi(2)).sum::<f64>() / part.len().max(1) as f64).sqrt();
        if rms < 0.002 {
            continue; // nothing but silence here
        }
        let wav = meeting::wav_bytes(part)?;
        let (segs, words) = groq::transcribe_segments(client, &keys.groq, wav, &keys.language, &keys.dictionary).await?;
        for mut s in segs {
            if s.no_speech > 0.6 || groq::is_hallucination(&s.text) {
                continue;
            }
            s.start += offset;
            s.end += offset;
            out.segs.push(s);
        }
        for mut w in words {
            w.start += offset;
            w.end += offset;
            out.words.push(w);
        }
    }
    Ok(out)
}

fn label_name(i: usize) -> String {
    let letters = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    if i < letters.len() {
        (letters[i] as char).to_string()
    } else {
        format!("S{}", i + 1)
    }
}

/// Split each transcript segment between speakers using word timings.
fn attribute(track: &Track, label_of: impl Fn(f64) -> Option<String>, fallback: &str) -> Vec<Utterance> {
    let mut out = vec![];
    for s in &track.segs {
        let words: Vec<&groq::Word> = track
            .words
            .iter()
            .filter(|w| (w.start + w.end) / 2.0 >= s.start - 0.05 && (w.start + w.end) / 2.0 <= s.end + 0.05)
            .collect();
        let tokens: Vec<&str> = s.text.split_whitespace().collect();
        let whole = |out: &mut Vec<Utterance>| {
            let sp = label_of((s.start + s.end) / 2.0).unwrap_or_else(|| fallback.to_string());
            out.push(Utterance { speaker: sp, start: s.start, end: s.end, text: s.text.clone() });
        };
        if words.is_empty() || words.len() != tokens.len() {
            // Can't align words to text: one speaker for the whole segment (majority vote).
            let mut votes: BTreeMap<String, usize> = BTreeMap::new();
            for w in &words {
                if let Some(l) = label_of((w.start + w.end) / 2.0) {
                    *votes.entry(l).or_default() += 1;
                }
            }
            match votes.into_iter().max_by_key(|v| v.1) {
                Some((sp, _)) => out.push(Utterance { speaker: sp, start: s.start, end: s.end, text: s.text.clone() }),
                None => whole(&mut out),
            }
            continue;
        }
        let labels: Vec<String> =
            words.iter().map(|w| label_of((w.start + w.end) / 2.0).unwrap_or_else(|| fallback.to_string())).collect();
        // Ignore one-word blips: a lone word between two words of another speaker.
        let mut smooth = labels.clone();
        for i in 1..labels.len().saturating_sub(1) {
            if labels[i - 1] == labels[i + 1] && labels[i] != labels[i - 1] {
                smooth[i] = labels[i - 1].clone();
            }
        }
        let mut i = 0;
        while i < tokens.len() {
            let mut j = i;
            while j + 1 < tokens.len() && smooth[j + 1] == smooth[i] {
                j += 1;
            }
            out.push(Utterance {
                speaker: smooth[i].clone(),
                start: words[i].start,
                end: words[j].end,
                text: tokens[i..=j].join(" "),
            });
            i = j + 1;
        }
    }
    out
}

pub async fn process(
    client: &Client,
    dir: &Path,
    rec: &mut Recording,
    keys: &Keys,
    mut update: impl FnMut(&Recording),
) -> Result<(), String> {
    rec.status = "processing".into();
    if !rec.error.as_deref().unwrap_or("").starts_with("Nahrávání bylo přerušené") {
        rec.error = None;
    }
    rec.stage = "transcribe".into();
    update(rec);
    if keys.groq.trim().is_empty() {
        return Err("Chybí Groq klíč".into());
    }

    let mic = meeting::read_track(&dir.join("mic.wav"))?;
    let sys = if rec.has_system { meeting::read_track(&dir.join("system.wav")).ok() } else { None };
    let mic_e = normalise(&meeting::energy(&mic));
    let sys_e = sys.as_ref().map(|s| normalise(&meeting::energy(s)));

    // 1) What was said (Groq Whisper, per track).
    let mut mic_t = transcribe_track(client, keys, &mic).await?;
    let sys_t = match &sys {
        Some(s) => Some(transcribe_track(client, keys, s).await?),
        None => None,
    };
    // Speakers on (no headphones): the mic also hears the others. Drop those echoes.
    if let Some(se) = &sys_e {
        mic_t.segs.retain(|s| {
            let (dom, total) = mic_dominance(&mic_e, se, s.start, s.end);
            !(total > 0.5 && dom / total < 0.25)
        });
    }

    // 2) Who said it (Whiskr Voices, on-device).
    rec.stage = if voices::model_ready(&keys.data_dir) { "speakers" } else { "model" }.into();
    update(rec);
    let voices_result = match voices::ensure_model(client, &keys.data_dir).await {
        Ok(model) => {
            rec.stage = "speakers".into();
            update(rec);
            let data_dir = keys.data_dir.clone();
            let mic_regions: Vec<(f64, f64)> = mic_t.segs.iter().map(|s| (s.start, s.end)).collect();
            let sys_regions: Vec<(f64, f64)> =
                sys_t.as_ref().map(|t| t.segs.iter().map(|s| (s.start, s.end)).collect()).unwrap_or_default();
            let has_sys = sys.is_some();
            let mic_audio = mic.clone();
            let sys_audio = sys.clone();
            tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
                let emb = voices::Embedder::load(&model)?;
                let book = voices::VoiceBook::load(&data_dir);
                if has_sys {
                    let d = voices::diarize(&emb, sys_audio.as_deref().unwrap_or(&[]), &voices::merge_regions(sys_regions), None);
                    // Learn your voice from the mic track (that's always you).
                    let me = voices::diarize(&emb, &mic_audio, &voices::merge_regions(mic_regions), Some(1));
                    Ok((d, me.centroids.into_iter().next(), book))
                } else {
                    let d = voices::diarize(&emb, &mic_audio, &voices::merge_regions(mic_regions), None);
                    Ok((d, None, book))
                }
            })
            .await
            .map_err(|e| e.to_string())?
        }
        Err(e) => Err(e),
    };

    let mut utts: Vec<Utterance> = vec![];
    rec.prints.clear();
    match voices_result {
        Ok((d, my_print, mut book)) => {
            rec.engine = "whiskr-voices".into();
            if let Some(p) = &my_print {
                book.learn_me(p);
            }
            // Map clusters → labels: you, someone remembered, or A, B, C…
            let mut names: Vec<String> = vec![];
            let mut next = 0usize;
            let me_label = if sys.is_none() {
                // Mic-only meeting: the cluster that sounds most like you is "Já".
                d.centroids
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, book.is_me(c)))
                    .filter(|(_, s)| *s >= voices::VoiceBook::me_threshold())
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                    .map(|x| x.0)
            } else {
                None
            };
            for (i, c) in d.centroids.iter().enumerate() {
                let label = if Some(i) == me_label {
                    ME.to_string()
                } else {
                    let l = label_name(next);
                    next += 1;
                    if let Some((name, _)) = book.recognise(c) {
                        if !rec.speakers.values().any(|n| *n == name) {
                            rec.speakers.insert(l.clone(), name);
                        }
                    }
                    l
                };
                rec.prints.insert(label.clone(), c.clone());
                names.push(label);
            }
            if let Some(p) = my_print {
                rec.prints.insert(ME.into(), p);
            }
            book.save(&keys.data_dir);

            let label_of = |t: f64| voices::speaker_at(&d, t).map(|i| names[i].clone());
            if let Some(st) = &sys_t {
                utts.extend(attribute(&mic_t, |_| Some(ME.to_string()), ME));
                utts.extend(attribute(st, label_of, OTHERS));
            } else {
                utts.extend(attribute(&mic_t, label_of, "A"));
            }
        }
        Err(e) => {
            // Without voices: you vs. the others (calls), or one block (mic only).
            rec.engine = "groq".into();
            rec.error = Some(format!("Rozlišení hlasů se nepovedlo: {e}"));
            let mic_label = if sys_t.is_some() { ME } else { "A" };
            utts.extend(attribute(&mic_t, |_| Some(mic_label.to_string()), mic_label));
            if let Some(st) = &sys_t {
                utts.extend(attribute(st, |_| Some(OTHERS.to_string()), OTHERS));
            }
        }
    }
    utts.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    merge_adjacent(&mut utts);
    rec.utterances = utts;
    for u in &rec.utterances {
        rec.speakers.entry(u.speaker.clone()).or_insert_with(|| default_name(&u.speaker));
    }
    // Keep prints only for speakers that actually appear in the transcript (plus you).
    let present: Vec<String> = rec.utterances.iter().map(|u| u.speaker.clone()).collect();
    rec.prints.retain(|k, _| present.contains(k) || k == ME);

    // Summary is a bonus: a failure here must not lose the transcript.
    rec.stage = "summary".into();
    update(rec);
    if !rec.utterances.is_empty() {
        if let Err(e) = summarise(client, &keys.brain, rec, &keys.custom_templates).await {
            rec.error = Some(format!("Shrnutí se nepovedlo: {e}"));
        }
    }
    rec.status = "done".into();
    rec.stage = String::new();
    update(rec);
    Ok(())
}

pub fn merge_adjacent(utts: &mut Vec<Utterance>) {
    let mut merged: Vec<Utterance> = Vec::with_capacity(utts.len());
    for u in utts.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.speaker == u.speaker && u.start - last.end < 1.5 && last.text.len() < 1200 {
                last.text.push(' ');
                last.text.push_str(&u.text);
                last.end = u.end;
                continue;
            }
        }
        merged.push(u);
    }
    *utts = merged;
}

const SUMMARY_SYSTEM: &str = "You summarise meeting and call transcripts. Reply with JSON only. \
Write in the same language as the transcript (usually Czech). Be concrete and brief; never invent facts.";

/// What was said around each bookmark (±30 s), for the summary prompt.
fn bookmark_excerpts(rec: &Recording) -> String {
    let mut out = String::new();
    for &b in &rec.bookmarks {
        let lines: Vec<String> = rec
            .utterances
            .iter()
            .filter(|u| u.end >= b - 30.0 && u.start <= b + 10.0)
            .map(|u| format!("  [{}] {}: {}", fmt_ts(u.start), rec.name_of(&u.speaker), u.text))
            .collect();
        if !lines.is_empty() {
            out.push_str(&format!("- Bookmark at {}:\n{}\n", fmt_ts(b), lines.join("\n")));
        }
    }
    out
}

pub async fn summarise(client: &Client, brain: &brain::Brain, rec: &mut Recording, custom: &[templates::Template]) -> Result<(), String> {
    let timeout = std::time::Duration::from_secs(15 * 60);
    let lines: Vec<String> = rec
        .utterances
        .iter()
        .map(|u| format!("[{}] {} ({}): {}", fmt_ts(u.start), rec.name_of(&u.speaker), u.speaker, u.text))
        .collect();
    // Free-tier token limits are small: summarise long meetings in parts first.
    // Claude takes even a long meeting in one go.
    let part_limit = if brain.big_context() { 600_000 } else { 14_000 };
    let mut parts: Vec<String> = vec![];
    let mut cur = String::new();
    for l in lines {
        if cur.len() + l.len() > part_limit && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push_str(&l);
        cur.push('\n');
    }
    if !cur.is_empty() {
        parts.push(cur);
    }

    let material = if parts.len() == 1 {
        parts.remove(0)
    } else {
        let mut notes = String::new();
        for (i, p) in parts.iter().enumerate() {
            let v = brain.json(
                client,
                SUMMARY_SYSTEM,
                &format!(
                    "Part {} of {} of a longer transcript. Return {{\"notes\": [\"…\"]}} with the key points, \
tasks (who/what/when) and decisions from this part, each prefixed with the timestamp.\n\n{}",
                    i + 1,
                    parts.len(),
                    p
                ),
                timeout,
            )
            .await?;
            for n in v["notes"].as_array().into_iter().flatten() {
                if let Some(s) = n.as_str() {
                    notes.push_str("- ");
                    notes.push_str(s);
                    notes.push('\n');
                }
            }
        }
        format!("(Notes extracted from a long transcript, in order.)\n{notes}")
    };

    let template_part = match templates::find(&rec.template, custom) {
        Some(t) => format!("Recording type: {} — {}\n\"template\": \"{}\"", t.name, t.instructions, t.id),
        None => format!(
            "First decide which template fits best and put its id in \"template\":\n{}\nThen follow that template's idea for \"sections\".",
            templates::menu(custom)
        ),
    };
    let marks = bookmark_excerpts(rec);
    let marks_part = if marks.is_empty() {
        "\"highlights\": []".to_string()
    } else {
        format!(
            "\"highlights\": one entry per bookmark below — {{\"time\": \"MM:SS\", \"text\": \"what was important at that moment, 1–2 sentences\"}}.\n\
The user pressed a bookmark at these moments while recording (excerpts):\n{marks}"
        )
    };
    let prompt = format!(
        "Return JSON with exactly these keys:\n\
- \"title\": short title of the recording (max 6 words)\n\
- \"template\": id of the template used\n\
- \"overview\": 2–5 sentence summary\n\
- \"action_items\": list of tasks as strings, e.g. \"Petra — poslat nabídku do pátku\"\n\
- \"decisions\": list of decisions made\n\
- \"topics\": 2–6 short topic tags\n\
- \"sections\": list of {{\"title\": \"…\", \"items\": [\"…\"]}} as the template asks (skip empty ones)\n\
- {marks_part}\n\
- \"speaker_names\": object mapping speaker labels (the code in parentheses, e.g. \"A\", \"OTHERS\") to a real first name, \
ONLY when the transcript makes it unambiguous (someone is addressed or introduces themselves). Never map \"ME\". Omit if unsure.\n\n\
{template_part}\n\n\
Transcript:\n{material}"
    );
    let v: Value = brain.json(client, SUMMARY_SYSTEM, &prompt, timeout).await?;
    let list = |k: &str| -> Vec<String> {
        v[k].as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    };
    // Only replace the automatic title, never one the user wrote.
    if rec.title.starts_with("Nahrávka ") {
        if let Some(t) = v["title"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
            rec.title = t.to_string();
        }
    }
    let sections = v["sections"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let title = x["title"].as_str()?.trim().to_string();
                    let items: Vec<String> = x["items"]
                        .as_array()
                        .map(|i| i.iter().filter_map(|y| y.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
                        .unwrap_or_default();
                    (!title.is_empty() && !items.is_empty()).then_some(Section { title, items })
                })
                .collect()
        })
        .unwrap_or_default();
    let highlights = v["highlights"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let text = x["text"].as_str()?.trim().to_string();
                    let time = parse_ts(&x["time"]);
                    (!text.is_empty()).then_some(Highlight { time, text })
                })
                .collect()
        })
        .unwrap_or_default();
    rec.summary = Some(Summary {
        overview: v["overview"].as_str().unwrap_or("").trim().to_string(),
        action_items: list("action_items"),
        decisions: list("decisions"),
        topics: list("topics"),
        sections,
        highlights,
        template: v["template"].as_str().unwrap_or(&rec.template).to_string(),
    });
    if let Some(map) = v["speaker_names"].as_object() {
        for (label, name) in map {
            let Some(name) = name.as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() < 40) else { continue };
            if label == ME {
                continue;
            }
            // Only replace names the user hasn't changed themselves.
            if let Some(cur) = rec.speakers.get(label) {
                if *cur == default_name(label) {
                    rec.speakers.insert(label.clone(), name.to_string());
                }
            }
        }
    }
    Ok(())
}

fn parse_ts(v: &Value) -> f64 {
    if let Some(n) = v.as_f64() {
        return n;
    }
    let s = v.as_str().unwrap_or("0");
    let p: Vec<f64> = s.trim().split(':').map(|x| x.trim().parse().unwrap_or(0.0)).collect();
    match p.as_slice() {
        [h, m, sec] => h * 3600.0 + m * 60.0 + sec,
        [m, sec] => m * 60.0 + sec,
        [sec] => *sec,
        _ => 0.0,
    }
}

/// One audio file for the player: mic and computer audio mixed.
pub fn playable_audio(dir: &Path, rec: &Recording) -> Result<PathBuf, String> {
    let out = dir.join("play.wav");
    if out.exists() {
        return Ok(out);
    }
    let mic = meeting::read_track(&dir.join("mic.wav"))?;
    let mix: Vec<i16> = if rec.has_system {
        match meeting::read_track(&dir.join("system.wav")) {
            Ok(sys) => (0..mic.len().max(sys.len()))
                .map(|i| {
                    let a = *mic.get(i).unwrap_or(&0) as i32;
                    let b = *sys.get(i).unwrap_or(&0) as i32;
                    (a + b).clamp(i16::MIN as i32, i16::MAX as i32) as i16
                })
                .collect(),
            Err(_) => mic,
        }
    } else {
        mic
    };
    std::fs::write(&out, meeting::wav_bytes(&mix)?).map_err(|e| e.to_string())?;
    Ok(out)
}
