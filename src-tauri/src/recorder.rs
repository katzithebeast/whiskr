//! Microphone capture.
//!
//! cpal streams are not `Send` on every platform, so the stream lives on its own
//! thread and is driven through a small command channel. After a dictation the
//! stream stays open for a short "warm" period, so back-to-back dictations don't
//! lose their first syllable while the device spins up.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

/// How long the mic stays open (but not recording) after a dictation.
const WARM_FOR: Duration = Duration::from_secs(45);

pub type LevelFn = Arc<dyn Fn(f32) + Send + Sync>;

pub struct Clip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Clip {
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.max(1) as f32
    }
    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

enum Cmd {
    Start(Option<String>, Sender<Result<(), String>>),
    Stop(Sender<Clip>),
    Cancel,
}

pub struct Recorder {
    tx: Sender<Cmd>,
}

struct Shared {
    recording: AtomicBool,
    buffer: Mutex<Vec<f32>>,
    /// Last few hundred ms of audio while the mic is warm but idle. Seeds the next
    /// recording so the first syllable isn't lost when you start talking right
    /// as you press the key.
    preroll: Mutex<std::collections::VecDeque<f32>>,
    level: LevelFn,
}

/// Seconds of audio kept from just before the key press.
const PREROLL_SECS: f32 = 0.35;

impl Recorder {
    pub fn new(level: LevelFn) -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let shared = Arc::new(Shared {
            recording: AtomicBool::new(false),
            buffer: Mutex::new(Vec::with_capacity(16_000 * 60)),
            preroll: Mutex::new(std::collections::VecDeque::new()),
            level,
        });

        std::thread::Builder::new()
            .name("whiskr-mic".into())
            .spawn(move || {
                // (stream, device name it was opened with, sample rate)
                let mut open: Option<(Stream, Option<String>, u32)> = None;
                loop {
                    let cmd = if open.is_some() && !shared.recording.load(Ordering::SeqCst) {
                        match rx.recv_timeout(WARM_FOR) {
                            Ok(c) => c,
                            Err(RecvTimeoutError::Timeout) => {
                                open = None; // close the mic after the warm period
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match rx.recv() {
                            Ok(c) => c,
                            Err(_) => break,
                        }
                    };

                    match cmd {
                        Cmd::Start(device, reply) => {
                            let reuse = matches!(&open, Some((_, d, _)) if *d == device);
                            if !reuse {
                                open = None;
                                match open_stream(device.as_deref(), shared.clone()) {
                                    Ok((s, sr)) => open = Some((s, device, sr)),
                                    Err(e) => {
                                        let _ = reply.send(Err(e));
                                        continue;
                                    }
                                }
                            }
                            {
                                let mut buf = shared.buffer.lock();
                                buf.clear();
                                // Only reuse pre-roll from a stream that was already warm.
                                if reuse {
                                    buf.extend(shared.preroll.lock().drain(..));
                                }
                            }
                            shared.preroll.lock().clear();
                            shared.recording.store(true, Ordering::SeqCst);
                            let _ = reply.send(Ok(()));
                        }
                        Cmd::Stop(reply) => {
                            shared.recording.store(false, Ordering::SeqCst);
                            let samples = std::mem::take(&mut *shared.buffer.lock());
                            let sample_rate = open.as_ref().map(|o| o.2).unwrap_or(16_000);
                            let _ = reply.send(Clip { samples, sample_rate });
                        }
                        Cmd::Cancel => {
                            shared.recording.store(false, Ordering::SeqCst);
                            shared.buffer.lock().clear();
                        }
                    }
                }
            })
            .expect("spawn mic thread");

        Self { tx }
    }

    pub fn start(&self, device: Option<String>) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::Start(device, tx)).map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Mikrofon neodpovídá".to_string())?
    }

    pub fn stop(&self) -> Clip {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Cmd::Stop(tx)).is_err() {
            return Clip { samples: vec![], sample_rate: 16_000 };
        }
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or(Clip { samples: vec![], sample_rate: 16_000 })
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(Cmd::Cancel);
    }
}

pub fn list_inputs() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

fn open_stream(device: Option<&str>, shared: Arc<Shared>) -> Result<(Stream, u32), String> {
    let host = cpal::default_host();
    let dev = match device {
        Some(name) => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
            .or_else(|| host.default_input_device()),
        None => host.default_input_device(),
    }
    .ok_or_else(|| "Nenašla jsem žádný mikrofon".to_string())?;

    let config = dev
        .default_input_config()
        .map_err(|e| format!("Mikrofon nejde otevřít: {e}"))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    // Level meter state: emit ~30× per second.
    let chunk = (sample_rate / 30).max(1) as usize;
    let preroll_max = (sample_rate as f32 * PREROLL_SECS) as usize;

    macro_rules! build {
        ($t:ty, $to_f32:expr) => {{
            let shared = shared.clone();
            let mut acc = 0.0f32;
            let mut n = 0usize;
            dev.build_input_stream(
                &stream_config,
                move |data: &[$t], _| {
                    let rec = shared.recording.load(Ordering::Relaxed);
                    let mut buf = if rec { Some(shared.buffer.lock()) } else { None };
                    let mut pre = if rec { None } else { Some(shared.preroll.lock()) };
                    for frame in data.chunks(channels) {
                        let mut s = 0.0f32;
                        for v in frame {
                            s += $to_f32(*v);
                        }
                        let s = s / channels as f32;
                        if let Some(b) = buf.as_mut() {
                            b.push(s);
                        } else if let Some(p) = pre.as_mut() {
                            p.push_back(s);
                            if p.len() > preroll_max {
                                p.pop_front();
                            }
                        }
                        acc += s * s;
                        n += 1;
                        if n >= chunk {
                            if rec {
                                (shared.level)((acc / n as f32).sqrt());
                            }
                            acc = 0.0;
                            n = 0;
                        }
                    }
                },
                |e| eprintln!("mic stream error: {e}"),
                None,
            )
        }};
    }

    let stream = match format {
        SampleFormat::F32 => build!(f32, |v: f32| v),
        SampleFormat::I16 => build!(i16, |v: i16| v as f32 / i16::MAX as f32),
        SampleFormat::U16 => build!(u16, |v: u16| (v as f32 - 32768.0) / 32768.0),
        SampleFormat::I32 => build!(i32, |v: i32| v as f32 / i32::MAX as f32),
        other => return Err(format!("Nepodporovaný formát mikrofonu: {other:?}")),
    }
    .map_err(|e| format!("Mikrofon nejde otevřít: {e}"))?;

    stream.play().map_err(|e| format!("Mikrofon nejde spustit: {e}"))?;
    Ok((stream, sample_rate))
}

/// Downmixed clip -> 16 kHz mono 16-bit WAV, which is what Whisper wants anyway
/// (and keeps uploads tiny).
pub fn to_wav_16k(clip: &Clip) -> Result<Vec<u8>, String> {
    const TARGET: u32 = 16_000;
    let resampled: Vec<f32> = if clip.sample_rate == TARGET || clip.samples.is_empty() {
        clip.samples.clone()
    } else {
        let ratio = clip.sample_rate as f64 / TARGET as f64;
        // Cheap anti-aliasing: moving average over ~one output period before
        // decimating (48 kHz → 16 kHz would otherwise fold hiss into speech).
        let width = (ratio.round() as usize).max(1);
        let smoothed: Vec<f32> = if width > 1 {
            let mut out = Vec::with_capacity(clip.samples.len());
            let mut acc = 0.0f32;
            for (i, &v) in clip.samples.iter().enumerate() {
                acc += v;
                if i >= width {
                    acc -= clip.samples[i - width];
                }
                out.push(acc / width.min(i + 1) as f32);
            }
            out
        } else {
            clip.samples.clone()
        };
        let src = &smoothed;
        let out_len = (src.len() as f64 / ratio) as usize;
        (0..out_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let idx = pos as usize;
                let frac = (pos - idx as f64) as f32;
                let a = src[idx.min(src.len() - 1)];
                let b = src[(idx + 1).min(src.len() - 1)];
                a + (b - a) * frac
            })
            .collect()
    };

    // Gentle normalisation so quiet mics still transcribe well.
    let peak = resampled.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = if peak > 0.0 && peak < 0.5 { (0.7 / peak).min(8.0) } else { 1.0 };

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::with_capacity(resampled.len() * 2 + 44));
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        for s in resampled {
            let v = (s * gain).clamp(-1.0, 1.0);
            w.write_sample((v * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
        }
        w.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}
