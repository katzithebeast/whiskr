//! Whiskr Voices — our own speaker separation ("who said what"), on-device.
//!
//! 1. A small speaker-embedding network (CAM++, 28 MB, downloaded once) turns
//!    every ~1.5 s of speech into a 192-number voice print.
//! 2. Prints are grouped with average-linkage clustering: one group = one person.
//! 3. Groups are matched against voices Whiskr already knows (you, and anyone
//!    you've named before), so names carry over between recordings.
//!
//! Runs in pure Rust (tract), no services, no accounts, works on modest CPUs.

use rustfft::{num_complex::Complex32, FftPlanner};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tract_onnx::prelude::*;

pub const MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
pub const MODEL_FILE: &str = "voice-campplus.onnx";
const MODEL_MIN_BYTES: u64 = 20_000_000;

/// Average-linkage cosine-distance threshold for "same person" (tuned on multi-speaker audio).
const CLUSTER_DISTANCE: f32 = 0.62;
/// Similarity needed to recognise a remembered voice.
const KNOWN_SIMILARITY: f32 = 0.55;
const WIN: f64 = 1.5;
/// Feature frames per window (1.5 s at 10 ms hop, 25 ms frames).
const FRAMES: usize = 148;
const MAX_WINDOWS: f64 = 1200.0;

// ───────────────────────── model ─────────────────────────

pub fn model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join(MODEL_FILE)
}

pub fn model_ready(data_dir: &Path) -> bool {
    std::fs::metadata(model_path(data_dir)).map(|m| m.len() >= MODEL_MIN_BYTES).unwrap_or(false)
}

/// Download the voice model once (28 MB).
pub async fn ensure_model(client: &reqwest::Client, data_dir: &Path) -> Result<PathBuf, String> {
    let path = model_path(data_dir);
    if model_ready(data_dir) {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let res = client
        .get(MODEL_URL)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("Model hlasů se nepodařilo stáhnout: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("Model hlasů se nepodařilo stáhnout (HTTP {})", res.status()));
    }
    let bytes = res.bytes().await.map_err(|e| format!("Stahování modelu hlasů: {e}"))?;
    if (bytes.len() as u64) < MODEL_MIN_BYTES {
        return Err("Stažený model hlasů je neúplný".into());
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

type Plan = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

pub struct Embedder {
    plan: Arc<Plan>,
    fbank: Fbank,
}

impl Embedder {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut model = tract_onnx::onnx().model_for_path(path).map_err(|e| format!("Model hlasů: {e}"))?;
        // Fixed input length: tract's optimiser mishandles this graph with a symbolic
        // time axis, and our windows are ~1.5 s anyway (shorter ones get tiled).
        model
            .set_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec![1.to_dim(), FRAMES.to_dim(), 80.to_dim()]))
            .map_err(|e| e.to_string())?;
        let plan = model
            .into_optimized()
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("Model hlasů: {e}"))?;
        Ok(Self { plan: plan.into(), fbank: Fbank::new() })
    }

    /// L2-normalised voice print of a stretch of 16 kHz audio (−1..1).
    pub fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let feats = self.fbank.compute(samples);
        let frames = feats.len() / 80;
        if frames < 30 {
            return None;
        }
        // global-mean normalisation
        let mut mean = [0f32; 80];
        for f in feats.chunks(80) {
            for (m, v) in mean.iter_mut().zip(f) {
                *m += v;
            }
        }
        mean.iter_mut().for_each(|m| *m /= frames as f32);
        let rows: Vec<&[f32]> = feats.chunks(80).collect();
        // centre-crop or tile to exactly FRAMES rows
        let off = frames.saturating_sub(FRAMES) / 2;
        let data: Vec<f32> = (0..FRAMES)
            .flat_map(|i| {
                let r = rows[(off + i) % frames];
                r.iter().zip(mean.iter()).map(|(v, m)| v - m).collect::<Vec<f32>>()
            })
            .collect();
        let input = Tensor::from_shape(&[1, FRAMES, 80], &data).ok()?;
        let out = match self.plan.run(tvec![input.into()]) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("voice embed failed: {e:?}");
                return None;
            }
        };
        let e: Vec<f32> = out[0].to_array_view::<f32>().ok()?.iter().copied().collect();
        Some(normalize(e))
    }
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    v.iter_mut().for_each(|x| *x /= n);
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut d, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        d += x * y;
        na += x * x;
        nb += y * y;
    }
    d / (na.sqrt() * nb.sqrt()).max(1e-9)
}

// ───────────────────────── features (Kaldi-style fbank) ─────────────────────────

struct Fbank {
    window: Vec<f32>,
    mel: Vec<Vec<(usize, f32)>>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl Fbank {
    fn new() -> Self {
        let window: Vec<f32> = (0..400)
            .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 399.0).cos()).powf(0.85) as f32)
            .collect();
        let mel = |f: f64| 1127.0 * (1.0 + f / 700.0).ln();
        let (lo, hi) = (mel(20.0), mel(8000.0));
        let delta = (hi - lo) / 81.0;
        let mut banks = vec![];
        for b in 0..80 {
            let (l, c, r) = (lo + b as f64 * delta, lo + (b + 1) as f64 * delta, lo + (b + 2) as f64 * delta);
            let mut w = vec![];
            for k in 0..256 {
                let m = mel(k as f64 * 16000.0 / 512.0);
                let v = ((m - l) / (c - l)).min((r - m) / (r - c));
                if v > 0.0 {
                    w.push((k, v as f32));
                }
            }
            banks.push(w);
        }
        Self { window, mel: banks, fft: FftPlanner::new().plan_fft_forward(512) }
    }

    /// 80-dim log-mel frames (25 ms / 10 ms), flattened.
    fn compute(&self, x: &[f32]) -> Vec<f32> {
        if x.len() < 400 {
            return vec![];
        }
        let n = 1 + (x.len() - 400) / 160;
        let mut out = Vec::with_capacity(n * 80);
        let mut buf = vec![Complex32::new(0.0, 0.0); 512];
        let mut frame = [0f32; 400];
        for i in 0..n {
            frame.copy_from_slice(&x[i * 160..i * 160 + 400]);
            let mean = frame.iter().sum::<f32>() / 400.0;
            frame.iter_mut().for_each(|v| *v -= mean);
            for j in (1..400).rev() {
                frame[j] -= 0.97 * frame[j - 1];
            }
            frame[0] -= 0.97 * frame[0];
            for (j, b) in buf.iter_mut().enumerate() {
                *b = Complex32::new(if j < 400 { frame[j] * self.window[j] } else { 0.0 }, 0.0);
            }
            self.fft.process(&mut buf);
            for bank in &self.mel {
                let e: f32 = bank.iter().map(|&(k, w)| buf[k].norm_sqr() * w).sum();
                out.push(e.max(f32::EPSILON).ln());
            }
        }
        out
    }
}

// ───────────────────────── diarization ─────────────────────────

pub struct Window {
    pub start: f64,
    pub end: f64,
    pub label: usize,
}

pub struct Diarized {
    pub windows: Vec<Window>,
    /// One normalised centroid per label.
    pub centroids: Vec<Vec<f32>>,
}

/// Cut speech regions into overlapping windows of ~1.5 s.
fn make_windows(regions: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let speech: f64 = regions.iter().map(|(a, b)| (b - a).max(0.0)).sum();
    let hop = (speech / MAX_WINDOWS).max(0.75);
    let mut out = vec![];
    for &(a, b) in regions {
        let len = b - a;
        if len < 0.5 {
            continue;
        }
        if len <= WIN {
            out.push((a, b));
            continue;
        }
        let mut t = a;
        while t + WIN <= b + 1e-6 {
            out.push((t, t + WIN));
            t += hop;
        }
        if out.last().map(|w| w.1 < b - 0.4).unwrap_or(false) {
            out.push((b - WIN, b));
        }
    }
    out
}

/// Merge overlapping/near speech regions (from transcript segments).
pub fn merge_regions(mut r: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    r.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out: Vec<(f64, f64)> = vec![];
    for (a, b) in r {
        match out.last_mut() {
            Some(last) if a <= last.1 + 0.2 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

pub fn diarize(emb: &Embedder, audio: &[i16], regions: &[(f64, f64)], max_speakers: Option<usize>) -> Diarized {
    let rate = 16_000.0;
    let wins = make_windows(regions);
    // Voice prints in parallel on all CPU cores.
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8);
    let per = wins.len().div_ceil(threads).max(1);
    let results: Vec<Option<Vec<f32>>> = std::thread::scope(|sc| {
        let handles: Vec<_> = wins
            .chunks(per)
            .map(|part| {
                sc.spawn(move || {
                    part.iter()
                        .map(|&(a, b)| {
                            let i0 = ((a * rate) as usize).min(audio.len());
                            let i1 = ((b * rate) as usize).min(audio.len());
                            let seg: Vec<f32> = audio[i0..i1].iter().map(|&v| v as f32 / 32768.0).collect();
                            emb.embed(&seg)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    let mut kept = vec![];
    let mut prints = vec![];
    for (w, e) in wins.into_iter().zip(results) {
        if let Some(e) = e {
            kept.push(w);
            prints.push(e);
        }
    }
    if prints.is_empty() {
        return Diarized { windows: vec![], centroids: vec![] };
    }
    let mut labels = cluster(&prints, CLUSTER_DISTANCE);
    if let Some(m) = max_speakers {
        labels = limit_clusters(&prints, labels, m);
    }
    labels = absorb_tiny(&prints, labels);
    let (labels, centroids) = relabel(&prints, labels);
    let windows = kept.into_iter().zip(labels).map(|((start, end), label)| Window { start, end, label }).collect();
    Diarized { windows, centroids }
}

/// Speaker at time t (window whose centre is closest among those covering t).
pub fn speaker_at(d: &Diarized, t: f64) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for w in &d.windows {
        let c = (w.start + w.end) / 2.0;
        let covers = t >= w.start - 0.25 && t <= w.end + 0.25;
        let dist = (c - t).abs() + if covers { 0.0 } else { 10.0 };
        if best.map(|b| dist < b.0).unwrap_or(true) {
            best = Some((dist, w.label));
        }
    }
    best.filter(|b| b.0 < 12.0).map(|b| b.1)
}

/// Majority speaker over [a, b].
pub fn speaker_over(d: &Diarized, a: f64, b: f64) -> Option<usize> {
    let mut votes = vec![0f64; d.centroids.len()];
    for w in &d.windows {
        let ov = (w.end.min(b) - w.start.max(a)).max(0.0);
        if ov > 0.0 {
            votes[w.label] += ov;
        }
    }
    let best = votes.iter().enumerate().max_by(|x, y| x.1.partial_cmp(y.1).unwrap());
    match best {
        Some((i, v)) if *v > 0.0 => Some(i),
        _ => speaker_at(d, (a + b) / 2.0),
    }
}

/// Average-linkage agglomerative clustering (nearest-neighbour chain, O(n²)).
/// Returns a cluster id per item; clusters closer than `threshold` (cosine distance) are merged.
pub fn cluster(x: &[Vec<f32>], threshold: f32) -> Vec<usize> {
    let n = x.len();
    if n <= 1 {
        return vec![0; n];
    }
    // condensed distance matrix
    let idx = |i: usize, j: usize| -> usize {
        let (i, j) = if i < j { (i, j) } else { (j, i) };
        i * n - i * (i + 1) / 2 + (j - i - 1)
    };
    let mut d = vec![0f32; n * (n - 1) / 2];
    for i in 0..n {
        for j in i + 1..n {
            d[idx(i, j)] = 1.0 - x[i].iter().zip(&x[j]).map(|(a, b)| a * b).sum::<f32>();
        }
    }
    let mut size = vec![1usize; n];
    let mut active = vec![true; n];
    let mut merges: Vec<(f32, usize, usize)> = Vec::with_capacity(n - 1);
    let mut chain: Vec<usize> = vec![];
    let mut remaining = n;
    while remaining > 1 {
        if chain.is_empty() {
            chain.push((0..n).find(|&i| active[i]).unwrap());
        }
        loop {
            let a = *chain.last().unwrap();
            let prev = if chain.len() >= 2 { Some(chain[chain.len() - 2]) } else { None };
            let mut best = (f32::INFINITY, usize::MAX);
            if let Some(p) = prev {
                best = (d[idx(a, p)], p);
            }
            for k in 0..n {
                if k != a && active[k] {
                    let v = d[idx(a, k)];
                    if v < best.0 {
                        best = (v, k);
                    }
                }
            }
            let b = best.1;
            if Some(b) == prev {
                chain.pop();
                chain.pop();
                // merge b into a (a keeps the id)
                let (na, nb) = (size[a] as f32, size[b] as f32);
                for k in 0..n {
                    if active[k] && k != a && k != b {
                        let v = (na * d[idx(a, k)] + nb * d[idx(b, k)]) / (na + nb);
                        d[idx(a, k)] = v;
                    }
                }
                merges.push((best.0, a, b));
                size[a] += size[b];
                active[b] = false;
                remaining -= 1;
                break;
            }
            chain.push(b);
        }
    }
    // Cut the dendrogram at the threshold.
    merges.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let nx = p[c];
            p[c] = r;
            c = nx;
        }
        r
    }
    for (dist, a, b) in merges {
        if dist > threshold {
            break;
        }
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[rb] = ra;
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}

fn centroids_of(x: &[Vec<f32>], labels: &[usize]) -> Vec<(usize, Vec<f32>, usize)> {
    let mut map: std::collections::BTreeMap<usize, (Vec<f32>, usize)> = Default::default();
    for (v, &l) in x.iter().zip(labels) {
        let e = map.entry(l).or_insert_with(|| (vec![0.0; v.len()], 0));
        e.0.iter_mut().zip(v).for_each(|(a, b)| *a += b);
        e.1 += 1;
    }
    map.into_iter().map(|(l, (c, n))| (l, normalize(c), n)).collect()
}

/// Merge clusters too small to be a real participant (< ~3 s) into the closest big one.
fn absorb_tiny(x: &[Vec<f32>], mut labels: Vec<usize>) -> Vec<usize> {
    let min = 3.max(x.len() / 50);
    let cs = centroids_of(x, &labels);
    let big: Vec<&(usize, Vec<f32>, usize)> = cs.iter().filter(|c| c.2 >= min).collect();
    if big.is_empty() {
        return labels;
    }
    for (i, l) in labels.iter_mut().enumerate() {
        if cs.iter().any(|c| c.0 == *l && c.2 < min) {
            let best = big.iter().max_by(|a, b| cosine(&x[i], &a.1).partial_cmp(&cosine(&x[i], &b.1)).unwrap()).unwrap();
            *l = best.0;
        }
    }
    labels
}

/// When the user says how many people spoke: keep merging the closest clusters.
fn limit_clusters(x: &[Vec<f32>], mut labels: Vec<usize>, max: usize) -> Vec<usize> {
    loop {
        let cs = centroids_of(x, &labels);
        if cs.len() <= max.max(1) {
            return labels;
        }
        let mut best = (f32::MIN, 0, 0);
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                let s = cosine(&cs[i].1, &cs[j].1);
                if s > best.0 {
                    best = (s, cs[i].0, cs[j].0);
                }
            }
        }
        labels.iter_mut().for_each(|l| {
            if *l == best.2 {
                *l = best.1
            }
        });
    }
}

/// Renumber labels 0.. in order of first appearance; return centroids.
fn relabel(x: &[Vec<f32>], labels: Vec<usize>) -> (Vec<usize>, Vec<Vec<f32>>) {
    let mut order: Vec<usize> = vec![];
    for &l in &labels {
        if !order.contains(&l) {
            order.push(l);
        }
    }
    let new: Vec<usize> = labels.iter().map(|l| order.iter().position(|o| o == l).unwrap()).collect();
    let cs = centroids_of(x, &new).into_iter().map(|c| c.1).collect();
    (new, cs)
}

// ───────────────────────── remembered voices ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Voice {
    pub name: String,
    pub print: Vec<f32>,
    /// How many recordings contributed (for running averages).
    pub n: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VoiceBook {
    /// Your own voice, learned from the mic track of calls.
    #[serde(default)]
    pub me: Option<Voice>,
    #[serde(default)]
    pub voices: Vec<Voice>,
}

impl VoiceBook {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("voices.json")
    }
    pub fn load(data_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(data_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, data_dir: &Path) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(data_dir), s);
        }
    }

    fn blend(v: &mut Voice, print: &[f32]) {
        let n = v.n.min(20) as f32;
        let mixed: Vec<f32> = v.print.iter().zip(print).map(|(a, b)| (a * n + b) / (n + 1.0)).collect();
        v.print = normalize(mixed);
        v.n += 1;
    }

    pub fn learn_me(&mut self, print: &[f32]) {
        match &mut self.me {
            Some(v) => Self::blend(v, print),
            None => self.me = Some(Voice { name: "Já".into(), print: print.to_vec(), n: 1 }),
        }
    }

    pub fn learn(&mut self, name: &str, print: &[f32]) {
        let key = name.trim().to_lowercase();
        match self.voices.iter_mut().find(|v| v.name.to_lowercase() == key) {
            Some(v) => Self::blend(v, print),
            None => self.voices.push(Voice { name: name.trim().to_string(), print: print.to_vec(), n: 1 }),
        }
    }

    pub fn forget(&mut self, name: &str) {
        if name == "Já" || name == "__me__" {
            self.me = None;
        }
        let key = name.trim().to_lowercase();
        self.voices.retain(|v| v.name.to_lowercase() != key);
    }

    pub fn is_me(&self, print: &[f32]) -> f32 {
        self.me.as_ref().map(|m| cosine(&m.print, print)).unwrap_or(0.0)
    }

    /// Best remembered name for a voice print, if similar enough.
    pub fn recognise(&self, print: &[f32]) -> Option<(String, f32)> {
        self.voices
            .iter()
            .map(|v| (v.name.clone(), cosine(&v.print, print)))
            .filter(|(_, s)| *s >= KNOWN_SIMILARITY)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    }

    pub fn me_threshold() -> f32 {
        KNOWN_SIMILARITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_two_obvious_groups() {
        let a = normalize(vec![1.0, 0.1, 0.0]);
        let b = normalize(vec![0.0, 0.1, 1.0]);
        let x = vec![a.clone(), b.clone(), a.clone(), b.clone(), a.clone()];
        let l = cluster(&x, 0.5);
        assert_eq!(l[0], l[2]);
        assert_eq!(l[0], l[4]);
        assert_eq!(l[1], l[3]);
        assert_ne!(l[0], l[1]);
    }

    /// cargo test --lib voices::tests::real -- --ignored --nocapture
    /// with WHISKR_MODEL and WHISKR_WAV set.
    #[test]
    #[ignore]
    fn real() {
        let model = std::env::var("WHISKR_MODEL").unwrap();
        let wav = std::env::var("WHISKR_WAV").unwrap();
        let emb = Embedder::load(Path::new(&model)).unwrap();
        let audio = crate::meeting::read_track(Path::new(&wav)).unwrap();
        // first 3 s print, for comparing with the Python reference
        let seg: Vec<f32> = audio[..24000].iter().map(|&v| v as f32 / 32768.0).collect();
        let e = emb.embed(&seg).unwrap();
        println!("EMB3S {:?}", &e[..6]);
        std::fs::write("/tmp/claude-0/rust_emb.txt", e.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let f = emb.fbank.compute(&seg);
        std::fs::write("/tmp/claude-0/rust_fb.txt", f.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let en = crate::meeting::energy(&audio);
        let mut sorted = en.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let thr = (sorted[sorted.len() * 3 / 10] * 2.0).max(0.01);
        let mut regions = vec![];
        for (i, v) in en.iter().enumerate() {
            if *v > thr {
                regions.push((i as f64 / 10.0, (i + 1) as f64 / 10.0));
            }
        }
        let regions = merge_regions(regions);
        let t0 = std::time::Instant::now();
        let d = diarize(&emb, &audio, &regions, None);
        println!("windows {} speakers {} in {:?}", d.windows.len(), d.centroids.len(), t0.elapsed());
        let s: String = d.windows.iter().map(|w| (b'A' + w.label as u8) as char).collect();
        println!("LABELS {s}");
        let wins = make_windows(&regions);
        let prints: Vec<Vec<f32>> = wins.iter().filter_map(|(a, b)| {
            let seg: Vec<f32> = audio[(*a * 16000.0) as usize..((*b * 16000.0) as usize).min(audio.len())].iter().map(|&v| v as f32 / 32768.0).collect();
            emb.embed(&seg)
        }).collect();
        for th in [0.55f32, 0.6, 0.65, 0.7, 0.75, 0.8] {
            let l = absorb_tiny(&prints, cluster(&prints, th));
            let (l, c) = relabel(&prints, l);
            println!("TH {th} k={} {}", c.len(), l.iter().map(|x| (b'A' + *x as u8) as char).collect::<String>());
        }
        for w in d.windows.iter().step_by(4) {
            print!("{:.1}{} ", w.start, (b'A' + w.label as u8) as char);
        }
        println!();
    }

    #[test]
    fn windows_cover_regions() {
        let w = make_windows(&[(0.0, 4.0), (10.0, 10.8), (20.0, 20.2)]);
        assert!(w.len() >= 5);
        assert!(w.iter().all(|(a, b)| b > a));
        assert!(!w.iter().any(|(a, _)| *a >= 20.0));
    }
}
