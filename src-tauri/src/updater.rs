//! Updates from GitHub Releases (the repo can be private: a read-only token is
//! baked in at build time by the release workflow — WHISKR_UPDATE_TOKEN).
//!
//! Check: GET /repos/{repo}/releases/latest → compare its tag with our version
//! → the "*-setup.exe" asset. Install: download it (with progress), start the
//! installer and quit Whiskr; the installer replaces the app and starts it again.

use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

const REPO: &str = match option_env!("WHISKR_REPO") {
    Some(r) => r,
    None => "katzithebeast/whiskr",
};
const TOKEN: Option<&str> = option_env!("WHISKR_UPDATE_TOKEN");

#[derive(Serialize, Clone, Debug, Default)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub notes: String,
    /// GitHub API id of the installer asset.
    #[serde(skip)]
    pub asset_id: u64,
    pub asset_name: String,
    pub size: u64,
    /// Updates are possible in this build (it knows where to look).
    pub enabled: bool,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v0.15.10" → (0, 15, 10)
fn parse(v: &str) -> (u64, u64, u64) {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let mut it = v.split(|c: char| c == '.' || c == '-').map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

pub fn newer(latest: &str, current: &str) -> bool {
    parse(latest) > parse(current)
}

fn req(http: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    let mut r = http
        .get(url)
        .header("User-Agent", format!("Whiskr/{}", current()))
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = TOKEN.filter(|t| !t.is_empty()) {
        r = r.header("Authorization", format!("Bearer {t}"));
    }
    r
}

pub async fn check(http: &reqwest::Client) -> Result<UpdateInfo, String> {
    let mut info = UpdateInfo { current: current().into(), enabled: TOKEN.is_some() || option_env!("WHISKR_REPO").is_some(), ..Default::default() };
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = req(http, &url).header("Accept", "application/vnd.github+json").send().await.map_err(|e| format!("Aktualizace: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Err("Aktualizace: žádná vydaná verze (nebo chybí přístup k repu)".into());
    }
    if !resp.status().is_success() {
        return Err(format!("Aktualizace: GitHub odpověděl {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| format!("Aktualizace: {e}"))?;
    info.latest = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    info.notes = v["body"].as_str().unwrap_or("").chars().take(1500).collect();
    let asset = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str().map_or(false, |n| n.to_lowercase().ends_with("-setup.exe")));
    if let Some(a) = asset {
        info.asset_id = a["id"].as_u64().unwrap_or(0);
        info.asset_name = a["name"].as_str().unwrap_or("").to_string();
        info.size = a["size"].as_u64().unwrap_or(0);
    }
    info.available = info.asset_id != 0 && newer(&info.latest, &info.current);
    Ok(info)
}

/// Download the installer (progress → "whiskr:update-progress" 0..1) and return its path.
pub async fn download(app: &AppHandle, http: &reqwest::Client, info: &UpdateInfo) -> Result<PathBuf, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/assets/{}", info.asset_id);
    let mut resp = req(http, &url)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("Stahování: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Stahování: GitHub odpověděl {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(info.size).max(1);
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut last = 0.0f64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Stahování: {e}"))? {
        bytes.extend_from_slice(&chunk);
        let p = bytes.len() as f64 / total as f64;
        if p - last >= 0.02 {
            last = p;
            let _ = app.emit("whiskr:update-progress", p.min(1.0));
        }
    }
    if bytes.len() < 100_000 || &bytes[..2] != b"MZ" {
        return Err("Stahování: soubor nevypadá jako instalačka".into());
    }
    let name = if info.asset_name.is_empty() { format!("Whiskr_{}_setup.exe", info.latest) } else { info.asset_name.clone() };
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, &bytes).map_err(|e| format!("Uložení: {e}"))?;
    let _ = app.emit("whiskr:update-progress", 1.0);
    Ok(path)
}

/// Start the installer. `/P` = passive (progress only, no questions), `/R` =
/// start Whiskr again when done. The caller quits Whiskr right after.
pub fn run_installer(path: &std::path::Path) -> Result<(), String> {
    let mut c = std::process::Command::new(path);
    c.args(["/P", "/R"]);
    c.spawn().map(|_| ()).map_err(|e| format!("Instalace: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(newer("v0.15.10", "0.15.2"));
        assert!(newer("0.16.0", "0.15.9"));
        assert!(!newer("v0.15.2", "0.15.2"));
        assert!(!newer("0.15.1", "0.15.2"));
    }
}
