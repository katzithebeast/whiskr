//! Long recordings (meetings, calls, lectures) straight to disk.
//!
//! Two separate tracks, both 16 kHz mono 16-bit WAV:
//!   * `mic.wav`    — your microphone (= "Já")
//!   * `system.wav` — what the computer plays (the others in a call), via
//!     WASAPI loopback on Windows.
//! Keeping them apart is what lets Whiskr tell you and the others apart with
//! certainty. Loopback delivers nothing while the PC is silent, so the system
//! track is padded with silence to stay aligned with wall-clock time.

use crate::recorder::LevelFn;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const TARGET_RATE: u32 = 16_000;

pub struct Finished {
    pub dir: PathBuf,
    pub seconds: f64,
    pub has_system: bool,
}

pub struct Session {
    pub started: Instant,
    pub has_system: bool,
    dir: PathBuf,
    stop_tx: Sender<()>,
    thread: JoinHandle<Result<f64, String>>,
}

impl Session {
    pub fn start(dir: PathBuf, mic: Option<String>, system: bool, level: LevelFn) -> Result<Session, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mic_path = dir.join("mic.wav");
        let sys_path = dir.join("system.wav");
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<bool, String>>();

        let thread = std::thread::Builder::new()
            .name("whiskr-meeting".into())
            .spawn(move || -> Result<f64, String> {
                let host = cpal::default_host();
                let mut mic_track = match open_mic(&host, mic.as_deref(), &mic_path) {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.clone()));
                        return Err(e);
                    }
                };
                // System audio is a bonus: if it can't be opened, record the mic alone.
                let mut sys_track = if system { open_system(&host, &sys_path).ok() } else { None };
                let _ = ready_tx.send(Ok(sys_track.is_some()));
                let started = Instant::now();

                loop {
                    let done = match stop_rx.recv_timeout(Duration::from_millis(150)) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
                        Err(RecvTimeoutError::Timeout) => false,
                    };
                    let elapsed = started.elapsed().as_secs_f64();
                    let mut lvl = drain(&mut mic_track, elapsed)?;
                    if let Some(t) = sys_track.as_mut() {
                        lvl = lvl.max(drain(t, elapsed)?);
                    }
                    level(lvl);
                    if done {
                        break;
                    }
                }

                let seconds = started.elapsed().as_secs_f64();
                finish(mic_track, seconds)?;
                if let Some(t) = sys_track {
                    finish(t, seconds)?;
                }
                Ok(seconds)
            })
            .map_err(|e| e.to_string())?;

        match ready_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(has_system)) => Ok(Session { started: Instant::now(), has_system, dir, stop_tx, thread }),
            Ok(Err(e)) => {
                let _ = thread.join();
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
            Err(_) => Err("Zvukové zařízení neodpovídá".into()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn stop(self) -> Result<Finished, String> {
        let _ = self.stop_tx.send(());
        let seconds = self.thread.join().map_err(|_| "Nahrávání spadlo".to_string())??;
        Ok(Finished { dir: self.dir, seconds, has_system: self.has_system })
    }
}

// ───────────────────────── capture ─────────────────────────

/// Streaming resampler to 16 kHz: moving-average anti-alias + linear interpolation.
struct Resampler {
    ratio: f64,
    t: f64,
    prev: f32,
    hist: VecDeque<f32>,
    sum: f32,
    width: usize,
}

impl Resampler {
    fn new(rate: u32) -> Self {
        let ratio = rate as f64 / TARGET_RATE as f64;
        Self { ratio, t: 0.0, prev: 0.0, hist: VecDeque::new(), sum: 0.0, width: (ratio.round() as usize).max(1) }
    }

    fn feed(&mut self, input: &[f32], out: &mut Vec<i16>) {
        if input.is_empty() {
            return;
        }
        let mut y = Vec::with_capacity(input.len());
        for &x in input {
            self.hist.push_back(x);
            self.sum += x;
            if self.hist.len() > self.width {
                self.sum -= self.hist.pop_front().unwrap_or(0.0);
            }
            y.push(self.sum / self.hist.len() as f32);
        }
        let n = y.len() as f64;
        // t is the read position; -1.0 ≤ t means "between prev and y[0]".
        while self.t < n - 1.0 {
            let i = self.t.floor();
            let frac = (self.t - i) as f32;
            let a = if i < 0.0 { self.prev } else { y[i as usize] };
            let b = y[(i + 1.0) as usize];
            let v = (a + (b - a) * frac).clamp(-1.0, 1.0);
            out.push((v * i16::MAX as f32) as i16);
            self.t += self.ratio;
        }
        self.t -= n;
        self.prev = *y.last().unwrap();
    }
}

struct Track {
    buf: Arc<Mutex<Vec<f32>>>,
    rs: Resampler,
    writer: hound::WavWriter<BufWriter<std::fs::File>>,
    written: u64,
    pad_gaps: bool,
    stream: Stream,
    last_flush: Instant,
}

fn write_all(t: &mut Track, samples: &[i16]) -> Result<(), String> {
    for &s in samples {
        t.writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    t.written += samples.len() as u64;
    Ok(())
}

/// Move captured audio from the device callback into the file. Returns RMS level.
fn drain(t: &mut Track, elapsed: f64) -> Result<f32, String> {
    let chunk = std::mem::take(&mut *t.buf.lock());
    let rms = if chunk.is_empty() {
        0.0
    } else {
        (chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32).sqrt()
    };
    let mut out = Vec::with_capacity(chunk.len() / 2);
    t.rs.feed(&chunk, &mut out);
    write_all(t, &out)?;
    // Keep the track aligned with real time when the device sends nothing
    // (loopback during silence). 300 ms of slack for audio still in flight.
    if t.pad_gaps {
        let expected = ((elapsed - 0.3).max(0.0) * TARGET_RATE as f64) as u64;
        if t.written < expected {
            let gap = vec![0i16; (expected - t.written) as usize];
            write_all(t, &gap)?;
        }
    }
    // Crash safety: keep the WAV header valid on disk every few seconds, so a
    // crash or power cut loses at most the last moments, never the recording.
    if t.last_flush.elapsed() >= Duration::from_secs(3) {
        t.writer.flush().map_err(|e| e.to_string())?;
        t.last_flush = Instant::now();
    }
    Ok(rms)
}

fn finish(mut t: Track, seconds: f64) -> Result<(), String> {
    let _ = t.stream.pause();
    let rest = std::mem::take(&mut *t.buf.lock());
    let mut out = vec![];
    t.rs.feed(&rest, &mut out);
    write_all(&mut t, &out)?;
    if t.pad_gaps {
        let expected = (seconds * TARGET_RATE as f64) as u64;
        if t.written < expected {
            let gap = vec![0i16; (expected - t.written) as usize];
            write_all(&mut t, &gap)?;
        }
    }
    let Track { writer, stream, .. } = t;
    drop(stream);
    writer.finalize().map_err(|e| e.to_string())
}

fn open_mic(host: &cpal::Host, name: Option<&str>, path: &Path) -> Result<Track, String> {
    let dev = match name {
        Some(n) => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|x| x == n).unwrap_or(false)))
            .or_else(|| host.default_input_device()),
        None => host.default_input_device(),
    }
    .ok_or("Nenašla jsem žádný mikrofon")?;
    let cfg = dev.default_input_config().map_err(|e| format!("Mikrofon: {e}"))?;
    open_track(&dev, cfg, path, false).map_err(|e| format!("Mikrofon: {e}"))
}

fn open_system(host: &cpal::Host, path: &Path) -> Result<Track, String> {
    if !cfg!(target_os = "windows") {
        return Err("Zvuk z počítače umím nahrávat jen na Windows".into());
    }
    // On WASAPI, an input stream on an output device = loopback capture.
    let dev = host.default_output_device().ok_or("Nenašla jsem výstupní zařízení")?;
    let cfg = dev.default_output_config().map_err(|e| format!("Zvuk z počítače: {e}"))?;
    open_track(&dev, cfg, path, true).map_err(|e| format!("Zvuk z počítače: {e}"))
}

fn open_track(dev: &cpal::Device, cfg: cpal::SupportedStreamConfig, path: &Path, pad_gaps: bool) -> Result<Track, String> {
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    let format = cfg.sample_format();
    let stream_cfg: cpal::StreamConfig = cfg.into();
    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(rate as usize)));

    macro_rules! build {
        ($t:ty, $to_f32:expr) => {{
            let buf = buf.clone();
            dev.build_input_stream(
                &stream_cfg,
                move |data: &[$t], _| {
                    let mut b = buf.lock();
                    for frame in data.chunks(channels) {
                        let mut s = 0.0f32;
                        for v in frame {
                            s += $to_f32(*v);
                        }
                        b.push(s / channels as f32);
                    }
                },
                |e| eprintln!("meeting stream error: {e}"),
                None,
            )
        }};
    }
    let stream = match format {
        SampleFormat::F32 => build!(f32, |v: f32| v),
        SampleFormat::I16 => build!(i16, |v: i16| v as f32 / i16::MAX as f32),
        SampleFormat::U16 => build!(u16, |v: u16| (v as f32 - 32768.0) / 32768.0),
        SampleFormat::I32 => build!(i32, |v: i32| v as f32 / i32::MAX as f32),
        other => return Err(format!("nepodporovaný formát {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;

    let spec = hound::WavSpec { channels: 1, sample_rate: TARGET_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    Ok(Track { buf, rs: Resampler::new(rate), writer, written: 0, pad_gaps, stream, last_flush: Instant::now() })
}

// ───────────────────────── crash recovery ─────────────────────────

/// Fix the size fields of a 16-bit PCM WAV written by us that was never
/// finalised (app killed mid-recording). Returns the recovered length in seconds.
pub fn repair_wav(path: &Path) -> Result<f64, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut head = [0u8; 44];
    if len < 44 || f.read_exact(&mut head).is_err() || &head[0..4] != b"RIFF" || &head[36..40] != b"data" {
        return Err("Nahrávka je poškozená".into());
    }
    let data = (len - 44) & !1; // whole 16-bit samples
    f.seek(SeekFrom::Start(4)).map_err(|e| e.to_string())?;
    f.write_all(&((data + 36) as u32).to_le_bytes()).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(40)).map_err(|e| e.to_string())?;
    f.write_all(&(data as u32).to_le_bytes()).map_err(|e| e.to_string())?;
    let rate = u32::from_le_bytes([head[24], head[25], head[26], head[27]]).max(1);
    Ok(data as f64 / 2.0 / rate as f64)
}

// ───────────────────────── reading back ─────────────────────────

/// Whole track as 16 kHz mono i16.
pub fn read_track(path: &Path) -> Result<Vec<i16>, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int if spec.bits_per_sample == 16 => {
            r.samples::<i16>().filter_map(Result::ok).map(|s| s as f32 / i16::MAX as f32).collect()
        }
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / max).collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().filter_map(Result::ok).collect(),
    };
    let ch = spec.channels.max(1) as usize;
    let mono: Vec<f32> = if ch == 1 { raw } else { raw.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect() };
    if spec.sample_rate == TARGET_RATE {
        return Ok(mono.iter().map(|v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect());
    }
    let mut rs = Resampler::new(spec.sample_rate);
    let mut out = Vec::with_capacity(mono.len() * TARGET_RATE as usize / spec.sample_rate as usize + 1);
    rs.feed(&mono, &mut out);
    Ok(out)
}

/// RMS energy in 100 ms windows (10 values per second), 0..1.
pub fn energy(samples: &[i16]) -> Vec<f32> {
    samples
        .chunks((TARGET_RATE / 10) as usize)
        .map(|c| {
            let s: f64 = c.iter().map(|&v| (v as f64 / 32768.0).powi(2)).sum();
            (s / c.len().max(1) as f64).sqrt() as f32
        })
        .collect()
}

/// 16 kHz mono 16-bit WAV in memory, for uploading.
pub fn wav_bytes(samples: &[i16]) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: TARGET_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cur = std::io::Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
    {
        let mut w = hound::WavWriter::new(&mut cur, spec).map_err(|e| e.to_string())?;
        for &s in samples {
            w.write_sample(s).map_err(|e| e.to_string())?;
        }
        w.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cur.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_duration_across_chunks() {
        let mut rs = Resampler::new(48_000);
        let mut out = vec![];
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        for c in tone.chunks(1234) {
            rs.feed(c, &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "got {}", out.len());
        assert!(out.iter().any(|&v| v.abs() > 5000));
    }

    #[test]
    fn repair_unfinalised_wav() {
        let dir = std::env::temp_dir().join("whiskr-test-repair");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("crash.wav");
        let s: Vec<i16> = (0..32_000).map(|i| (i % 500) as i16).collect();
        let mut bytes = wav_bytes(&s).unwrap();
        // simulate a crash: header says 0 bytes of data
        bytes[4..8].copy_from_slice(&36u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(&p, bytes).unwrap();
        let secs = repair_wav(&p).unwrap();
        assert!((secs - 2.0).abs() < 0.01);
        assert_eq!(read_track(&p).unwrap(), s);
    }

    #[test]
    fn energy_is_ten_per_second() {
        let s = vec![1000i16; 32_000];
        assert_eq!(energy(&s).len(), 20);
    }

    #[test]
    fn wav_roundtrip() {
        let s: Vec<i16> = (0..16_000).map(|i| (i % 300) as i16).collect();
        let bytes = wav_bytes(&s).unwrap();
        let dir = std::env::temp_dir().join("whiskr-test-wav");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_track(&p).unwrap(), s);
    }
}
