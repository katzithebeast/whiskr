use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ITEMS: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub at: DateTime<Local>,
    /// Final text that was inserted.
    pub text: String,
    /// What the speech model heard, before cleanup.
    pub raw: String,
    /// Seconds of audio.
    pub seconds: f32,
    pub words: usize,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct History {
    pub items: Vec<Entry>,
    /// Lifetime word count (survives trimming and clearing).
    #[serde(default)]
    pub total_words: u64,
    #[serde(default)]
    pub total_seconds: f64,
}

impl History {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn push(&mut self, text: String, raw: String, seconds: f32) -> Entry {
        let words = text.split_whitespace().count();
        let entry = Entry {
            id: chrono::Utc::now().timestamp_millis() as u64,
            at: Local::now(),
            text,
            raw,
            seconds,
            words,
        };
        self.total_words += words as u64;
        self.total_seconds += seconds as f64;
        self.items.insert(0, entry.clone());
        self.items.truncate(MAX_ITEMS);
        entry
    }
}
