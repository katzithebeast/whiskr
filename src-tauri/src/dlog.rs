//! Diagnostic log: `whiskr-debug.log` next to Whiskr.exe (so it can be read
//! from the build folder). OFF unless switched on in Settings → General —
//! nothing is written otherwise. Writing happens on a background thread;
//! calling `dlog` is just a channel send, safe even inside the keyboard hook.

use std::io::Write;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

static TX: OnceLock<Mutex<Sender<String>>> = OnceLock::new();
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn path() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(|d| d.join("whiskr-debug.log"))
}

pub fn init() {
    let (tx, rx) = mpsc::channel::<String>();
    if TX.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("whiskr-dlog".into())
        .spawn(move || {
            let Ok(first) = rx.recv() else { return };
            let Some(p) = path() else { return };
            // keep it small: start over when it grows past 2 MB
            if std::fs::metadata(&p).map(|m| m.len() > 2_000_000).unwrap_or(false) {
                let _ = std::fs::remove_file(&p);
            }
            let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) else { return };
            let _ = writeln!(f, "\n===== Whiskr {} =====", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(f, "{first}");
            while let Ok(line) = rx.recv() {
                let _ = writeln!(f, "{line}");
                let _ = f.flush();
            }
        })
        .ok();
}

pub fn log(msg: impl AsRef<str>) {
    if !ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Some(tx) = TX.get() {
        let t = chrono::Local::now().format("%H:%M:%S%.3f");
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(format!("{t} {}", msg.as_ref()));
        }
    }
}

#[macro_export]
macro_rules! dlog {
    ($($a:tt)*) => { $crate::dlog::log(format!($($a)*)) };
}
