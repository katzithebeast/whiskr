use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How much Whiskr rewrites what you said.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Polish {
    /// Raw transcript, no AI cleanup.
    Off,
    /// Fillers out, self-corrections applied, punctuation fixed.
    Light,
    /// Light + lists, paragraphs and tidier sentences.
    Smart,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub api_key: String,
    /// Dictation (hold to talk), e.g. "ctrl+shift+Space". Empty = off.
    pub hotkey: String,
    /// Command mode: select text, hold, say what to do with it.
    pub hotkey_command: String,
    /// Start / stop a meeting recording.
    pub hotkey_record: String,
    /// Paste the last dictation again.
    pub hotkey_paste: String,
    /// Mark an important moment while recording a meeting.
    pub hotkey_bookmark: String,
    /// Summary template for new recordings ("auto" = Whiskr picks).
    pub default_template: String,
    /// User-defined summary templates.
    pub custom_templates: Vec<crate::templates::Template>,
    /// Adapt the cleanup to the app you're typing in (mail, chat, code…).
    pub app_tone: bool,
    /// "auto" or an ISO-639-1 code like "cs", "en".
    pub language: String,
    pub polish: Polish,
    /// Names, brands, jargon Whiskr should spell correctly.
    pub dictionary: Vec<String>,
    pub sounds: bool,
    pub autostart: bool,
    /// None = system default microphone.
    pub microphone: Option<String>,
    /// UI language: "cs" | "en".
    pub ui_lang: String,
    /// Look of the floating bar: "classic" | "slim" | "mini".
    pub bar_style: String,
    /// Record computer audio (calls) together with the mic.
    pub rec_system_audio: bool,
    /// Whiskr Ask: quick chat / delegate window.
    pub hotkey_ask: String,
    /// "claude" | "codex" | "cursor" | "groq"
    pub ask_engine: String,
    /// "auto" | "windows" | "wsl"
    pub ask_runtime: String,
    /// Delegated agents: "safe" (edit files, no free shell) | "full"
    pub ask_access: String,
    /// Attach the selected text when opening Ask.
    pub ask_selection: bool,
    pub profiles: Vec<crate::agents::Profile>,
    pub active_profile: String,
    pub projects: Vec<crate::agents::Project>,
    /// Say a task / reminder ("zítra v 9 zavolat Chrisovi").
    pub hotkey_task: String,
    /// Who does the thinking: "auto" (Claude if installed, else Groq) | "claude" | "groq"
    pub brain: String,
    /// Profile (account) Claude runs under for Whiskr's own work; "" = active.
    pub brain_profile: String,
    /// Claude model alias ("" = the account's default, "sonnet", "opus", …).
    pub brain_model: String,
    /// Morning briefing.
    pub briefing: bool,
    /// "HH:MM" — not before this time.
    pub briefing_time: String,
    /// Open Whiskr on the Today page when the briefing is ready.
    pub briefing_open: bool,
    /// Let the briefing use every connected MCP server / connector.
    pub briefing_all_tools: bool,
    /// …or only these (server names from `claude mcp list`).
    pub briefing_tools: Vec<String>,
    /// The user's own instructions for the briefing.
    pub briefing_extra: String,
    /// Offer to record when a call starts (Teams, Zoom, Meet…).
    pub meeting_detect: bool,
    /// Stop that recording when the call ends.
    pub meeting_autostop: bool,
    /// "dark" | "light" | "system"
    pub theme: String,
    /// Chosen model per engine in Ask ("" = default).
    pub ask_models: std::collections::BTreeMap<String, String>,
    /// Reveal answers smoothly, character by character.
    pub ask_typewriter: bool,
    /// Characters per second when typing answers out.
    pub ask_type_cps: u32,
    /// How long the chat takes to slide up (ms).
    pub ask_anim_ms: u32,
    /// Quick answers in Ask (no MCP start-up for Claude, low reasoning for Codex).
    pub ask_fast: bool,
    /// Swap: Shift+Space = new chat, Shift+Space+R = continue the last one.
    pub ask_space_new: bool,
    /// Hold Shift+Backspace: send what was said right away (off = just fill the box).
    pub ask_voice_send: bool,
    /// Games: while one of these apps is in front, Whiskr's shortcuts are off.
    pub game_apps: Vec<String>,
    /// Intro animation when the main window opens.
    pub splash: bool,
    /// Picture behind the home page hero: "" = built-in mountains, "none", or a file path.
    pub home_bg: String,
    /// Background of the app and the chat.
    pub look: Look,
    /// Saved looks ("presets").
    pub look_presets: Vec<LookPreset>,
    /// MCP servers Whiskr can hand to Claude Code, Codex, Cursor, Claude Desktop.
    pub mcp_library: Vec<crate::mcp_hub::McpDef>,
    /// Settings format version, for one-time migrations.
    #[serde(default)]
    pub schema: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            hotkey: "ctrl+shift+Space".into(),
            hotkey_command: "alt+shift+KeyE".into(),
            hotkey_record: "alt+shift+KeyR".into(),
            hotkey_paste: "alt+shift+KeyV".into(),
            hotkey_bookmark: "alt+shift+KeyB".into(),
            default_template: "auto".into(),
            custom_templates: vec![],
            app_tone: true,
            language: "cs".into(),
            polish: Polish::Light,
            dictionary: vec!["Whiskr".into()],
            sounds: true,
            autostart: false,
            microphone: None,
            ui_lang: "cs".into(),
            bar_style: "classic".into(),
            rec_system_audio: true,
            hotkey_ask: "shift+Space".into(),
            ask_engine: "claude".into(),
            ask_runtime: "auto".into(),
            ask_access: "safe".into(),
            ask_selection: true,
            profiles: crate::agents::default_profiles(),
            active_profile: "personal".into(),
            projects: vec![],
            hotkey_task: "alt+shift+KeyT".into(),
            brain: "auto".into(),
            brain_profile: String::new(),
            brain_model: String::new(),
            briefing: true,
            briefing_time: "08:00".into(),
            briefing_open: true,
            briefing_all_tools: true,
            briefing_tools: vec![],
            briefing_extra: String::new(),
            meeting_detect: true,
            meeting_autostop: true,
            theme: "dark".into(),
            mcp_library: vec![],
            ask_models: Default::default(),
            ask_typewriter: false,
            ask_type_cps: 90,
            ask_anim_ms: 620,
            ask_fast: true,
            ask_space_new: false,
            ask_voice_send: true,
            game_apps: vec!["league of legends.exe".into()],
            look: Look::default(),
            splash: true,
            home_bg: String::new(),
            look_presets: vec![],
            schema: CURRENT_SCHEMA,
        }
    }
}

const CURRENT_SCHEMA: u32 = 1;

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if s.schema < 1 {
            // v0.1.0 defaulted to auto-detect, which misreads short Czech
            // phrases as other languages. Czech handles mixed-in English fine.
            if s.language == "auto" {
                s.language = "cs".into();
            }
            s.schema = CURRENT_SCHEMA;
            let _ = s.save(path);
        }
        s
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Keep the previous version, just in case.
        if path.exists() {
            let _ = std::fs::copy(path, path.with_extension("prev.json"));
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Look {
    /// "" = theme default, else "#RRGGBB"
    pub color: String,
    /// Image file inside the app data folder ("" = none).
    pub image: String,
    /// Blur of the image, px.
    pub blur: u32,
    /// Darken / lighten the image, 0–90 %.
    pub dim: u32,
    /// Opacity of the chat panel, 30–100 % (lower = see the desktop through).
    pub opacity: u32,
}

impl Default for Look {
    fn default() -> Self {
        Self { color: String::new(), image: String::new(), blur: 0, dim: 35, opacity: 100 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct LookPreset {
    pub name: String,
    pub look: Look,
}
