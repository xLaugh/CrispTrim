//! Transcription CrisperWhisper 2.0 en Rust (candle), port fidèle du chemin suivi par la version
//! Python : `transcribe(audio, language="fr", mode="verbatim", word_timestamps=True)` avec les
//! réglages par défaut (fenêtres de 30 s tous les 26 s, contexte des 12 derniers mots, réparation
//! des boucles, relance si trop peu de mots, reprise des fins prématurées, Viterbi mot à mot).

pub mod cuda;
pub mod files;
pub mod mel;
pub mod model;
pub mod timing;

use crate::engine::transcribe::Word;
use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, Tensor};
use model::{Config, Whisper};
use std::path::Path;
use timing::Timed;
use tokenizers::Tokenizer;

/// Fin de texte : même identifiant dans tous les vocabulaires Whisper.
const EOT: u32 = 50257;
const MAX_NEW_TOKENS: usize = 256;
const CHUNK_S: f64 = 30.0;
const STRIDE_S: f64 = 26.0;
const CONTEXT_WORDS: usize = 12;
const DROP_WORDS: usize = 2;
/// n-gramme → répétitions consécutives qui signalent une boucle (`DEFAULT_REPAIR_THRESHOLDS`).
const REPAIR: [(usize, usize); 5] = [(1, 8), (2, 8), (3, 4), (4, 3), (5, 3)];
const MAX_REPAIRS: usize = 3;
const FALLBACK_TEMPERATURES: [f64; 4] = [0.4, 0.6, 0.8, 1.0];
const SAMPLES_PER_TEMP: usize = 3;

/// Carte graphique NVIDIA utilisable par ce programme (compilé avec CUDA, pilote présent).
pub fn gpu_available() -> bool {
    cfg!(feature = "cuda") && cuda::has_driver()
}

/// Où tourne le modèle : carte graphique si ses bibliothèques sont prêtes, sinon processeur.
/// Un échec de CUDA (bibliothèque absente, carte trop ancienne…) ne fait jamais planter : processeur.
pub fn best_device() -> Device {
    #[cfg(feature = "cuda")]
    {
        static DEV: std::sync::OnceLock<Option<Device>> = std::sync::OnceLock::new();
        if let Some(d) = DEV.get() {
            return d.clone().unwrap_or(Device::Cpu);
        }
        if gpu_available() {
            if let Some(dir) = cuda::find_libs(&crate::data_dir()) {
                cuda::use_dir(&dir);
                let d = std::panic::catch_unwind(|| Device::new_cuda(0).ok()).ok().flatten();
                let _ = DEV.set(d.clone());
                return d.unwrap_or(Device::Cpu);
            }
        }
    }
    Device::Cpu
}

pub fn uses_gpu(dev: &Device) -> bool {
    !matches!(dev, Device::Cpu)
}

#[derive(serde::Deserialize)]
struct GenConfig {
    #[serde(default)]
    alignment_heads: Vec<(usize, usize)>,
    #[serde(default)]
    suppress_tokens: Vec<i64>,
    #[serde(default)]
    lang_to_id: std::collections::HashMap<String, u32>,
}

pub struct Transcriber {
    model: Whisper,
    tok: Tokenizer,
    heads: Vec<(usize, usize)>,
    suppress: Vec<u32>,
    /// Amorce de contrôle : <|startoftranscript|> <|langue|> <|transcribe|> <|notimestamps|>.
    prefix: [u32; 4],
    /// Jetons retirés au décodage du texte (<|…|>, marqueurs d'amorce CrisperWhisper).
    special: std::collections::HashSet<u32>,
    /// Horodatages <|0.00|>..<|30.00|> : texte vide.
    timestamps: std::ops::RangeInclusive<u32>,
    pub model_name: String,
}

/// Avancement : fraction (0..1) du fichier.
pub type Progress<'a> = &'a mut dyn FnMut(f64);

struct Decode {
    ids: Vec<u32>,
    attn: Vec<Vec<f32>>,
}

/// Générateur pseudo-aléatoire (relance par échantillonnage) : xorshift64*.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03 | 1)
    }
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Transcriber {
    pub fn load(dir: &Path, model_name: &str, language: &str, device: &Device) -> Result<Self> {
        let config: Config = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)?;
        let gen: GenConfig = serde_json::from_str(&std::fs::read_to_string(dir.join("generation_config.json"))?)?;
        let tok = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| anyhow!("tokenizer.json : {e}"))?;
        // Les identifiants de contrôle diffèrent selon le modèle (turbo = vocabulaire large-v3).
        let id = |t: &str| tok.token_to_id(t).with_context(|| format!("jeton {t} absent du modèle"));
        let lang = format!("<|{language}|>");
        let lang_id = match gen.lang_to_id.get(&lang) {
            Some(&i) => i,
            None => id(&lang)?,
        };
        let prefix = [id("<|startoftranscript|>")?, lang_id, id("<|transcribe|>")?, id("<|notimestamps|>")?];
        let timestamps = id("<|0.00|>")?..=id("<|30.00|>")?;
        let marker = regex::Regex::new(r"^(<\|.*\|>|\[(verbatim|intended)_\d+\]|<(ctx|ectx|htx|ehtx|vtx|evtx)>)$").unwrap();
        let special = tok.get_vocab(true).into_iter().filter(|(t, _)| marker.is_match(t)).map(|(_, i)| i).collect();
        // Calcul en float16 sur carte graphique (comme la version Python), float32 sur processeur.
        let dtype = if uses_gpu(device) { DType::F16 } else { DType::F32 };
        let model = Whisper::load(&dir.join("model.safetensors"), config, device, dtype)?;
        Ok(Self {
            model,
            tok,
            heads: gen.alignment_heads,
            suppress: gen.suppress_tokens.into_iter().filter(|&t| t >= 0).map(|t| t as u32).collect(),
            prefix,
            special,
            timestamps,
            model_name: model_name.to_string(),
        })
    }

    // ------------------------------------------------------------------ jetons

    fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self.tok.encode(text, false).map_err(|e| anyhow!("{e}"))?.get_ids().to_vec())
    }

    /// Contrôles Whisper (<|…|>, horodatages) et marqueurs d'amorce CrisperWhisper.
    fn is_special(&self, id: u32) -> bool {
        self.special.contains(&id)
    }

    /// `decode_tokens(ids, skip_special=True)`.
    fn decode(&self, ids: &[u32]) -> String {
        let kept: Vec<u32> = ids.iter().copied().filter(|&i| !self.is_special(i)).collect();
        self.tok.decode(&kept, false).unwrap_or_default()
    }

    /// `tokenizer.decode([id])` sans retirer les spéciaux ; horodatages → "".
    fn piece(&self, id: u32) -> String {
        if self.timestamps.contains(&id) {
            return String::new();
        }
        self.tok.decode(&[id], false).unwrap_or_default()
    }

    fn prompt(&self, mode: &str, context: Option<&str>) -> Result<Vec<u32>> {
        let mut text: String = (1..=5).map(|i| format!("[{mode}_{i}]")).collect();
        if let Some(c) = context {
            text.push_str(&format!(" <ctx> {c} <ectx>"));
        }
        let mut ids = self.encode(&text)?;
        ids.extend(self.prefix);
        Ok(ids)
    }

    fn word_count(&self, ids: &[u32]) -> usize {
        self.decode(ids).split_whitespace().count()
    }

    // ------------------------------------------------------------------ modèle

    fn encode_audio(&mut self, mel: &[Vec<f32>]) -> Result<Tensor> {
        let n_mels = mel.len();
        let flat: Vec<f32> = mel.iter().flatten().copied().collect();
        let t = Tensor::from_vec(flat, (1, n_mels, mel::N_FRAMES), &self.model.device)?.to_dtype(self.model.dtype)?;
        Ok(self.model.encoder.forward(&t)?)
    }

    fn mean_heads(rows: &[Tensor], row: usize) -> Result<Vec<f32>> {
        let mut acc: Option<Vec<f32>> = None;
        for t in rows {
            let r: Vec<f32> = t.get(row)?.to_dtype(DType::F32)?.to_vec1()?;
            acc = Some(match acc {
                None => r,
                Some(mut a) => {
                    a.iter_mut().zip(&r).for_each(|(x, y)| *x += y);
                    a
                }
            });
        }
        let n = rows.len() as f32;
        Ok(acc.unwrap_or_default().into_iter().map(|x| x / n).collect())
    }

    fn masked(&self, logits: &Tensor, extra_ban: &[u32]) -> Result<Vec<f32>> {
        let mut l: Vec<f32> = logits.to_vec1()?;
        for &t in self.suppress.iter().chain(extra_ban) {
            if let Some(x) = l.get_mut(t as usize) {
                *x = f32::NEG_INFINITY;
            }
        }
        Ok(l)
    }

    fn argmax(l: &[f32]) -> u32 {
        let mut best = 0;
        for (i, &v) in l.iter().enumerate() {
            if v > l[best] {
                best = i;
            }
        }
        best as u32
    }

    fn softmax(l: &[f32]) -> Vec<f64> {
        let m = l.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as f64;
        let e: Vec<f64> = l.iter().map(|&x| ((x as f64) - m).exp()).collect();
        let s: f64 = e.iter().sum();
        e.into_iter().map(|x| x / s).collect()
    }

    /// Décodage glouton (ou échantillonné si `temperature`), avec l'attention de chaque jeton produit.
    /// `ban_first` : interdit au tout premier pas ; `eot_until` : fin de texte interdite avant ce
    /// nombre de jetons (reprise des fins prématurées) — renvoie alors aussi P(fin) à l'arrêt.
    fn generate(
        &mut self,
        xa: &Tensor,
        prompt: &[u32],
        max_new: usize,
        ban_first: Option<u32>,
        temperature: Option<(f64, &mut Rng)>,
        eot_until: usize,
        mut on_token: Option<&mut dyn FnMut()>,
    ) -> Result<(Decode, Option<f64>)> {
        let max_new = max_new.min(self.model.config.max_target_positions.saturating_sub(prompt.len()));
        self.model.decoder.reset();
        let mut ids = Vec::new();
        let mut attn = Vec::new();
        let mut input: Vec<u32> = prompt.to_vec();
        let mut temp = temperature;
        let mut stop_prob = None;
        for step in 0..max_new {
            let (logits, rows) = self.model.decoder.step(&input, xa, &self.heads)?;
            let last = input.len() - 1;
            let mut ban: Vec<u32> = Vec::new();
            if step == 0 {
                ban.extend(ban_first);
            }
            if step < eot_until {
                ban.push(EOT);
            }
            let l = self.masked(&logits, &ban)?;
            let next = match temp.as_mut() {
                Some((t, rng)) => {
                    let scaled: Vec<f32> = l.iter().map(|&x| (x as f64 / *t) as f32).collect();
                    let p = Self::softmax(&scaled);
                    let r = rng.next_f64();
                    let mut acc = 0.0;
                    let mut pick = Self::argmax(&l);
                    for (i, &pi) in p.iter().enumerate() {
                        acc += pi;
                        if r < acc {
                            pick = i as u32;
                            break;
                        }
                    }
                    pick
                }
                None => Self::argmax(&l),
            };
            attn.push(Self::mean_heads(&rows, last)?);
            ids.push(next);
            if let Some(f) = on_token.as_mut() {
                f();
            }
            if next == EOT {
                if eot_until > 0 {
                    stop_prob = Some(Self::softmax(&l)[EOT as usize]);
                }
                break;
            }
            input = vec![next];
        }
        Ok((Decode { ids, attn }, stop_prob))
    }

    /// Attention « forcée » : un seul passage sur amorce + jetons (après une relance).
    fn teacher_forced(&mut self, xa: &Tensor, prompt: &[u32], gen: &[u32]) -> Result<Vec<Vec<f32>>> {
        self.model.decoder.reset();
        let all: Vec<u32> = prompt.iter().chain(gen).copied().collect();
        let (_, rows) = self.model.decoder.step(&all, xa, &self.heads)?;
        (0..gen.len()).map(|k| Self::mean_heads(&rows, prompt.len() - 1 + k)).collect()
    }

    /// P(fin de texte) juste après `content` (logits masqués).
    fn eot_probability(&mut self, xa: &Tensor, prompt: &[u32], content: &[u32]) -> Result<Option<f64>> {
        if content.is_empty() {
            return Ok(None);
        }
        self.model.decoder.reset();
        let all: Vec<u32> = prompt.iter().chain(content).copied().collect();
        let (logits, _) = self.model.decoder.step(&all, xa, &self.heads)?;
        let l = self.masked(&logits, &[])?;
        Ok(Some(Self::softmax(&l)[EOT as usize]))
    }

    // ------------------------------------------------------------------ garde-fous

    fn find_loop(ids: &[u32], reps: &[(usize, usize)]) -> Option<(usize, Vec<u32>)> {
        for i in 0..ids.len() {
            for &(n, r) in reps {
                if i + n * r > ids.len() {
                    continue;
                }
                let g = &ids[i..i + n];
                if (1..r).all(|k| &ids[i + k * n..i + (k + 1) * n] == g) {
                    return Some((i, g.to_vec()));
                }
            }
        }
        None
    }

    /// Décodage principal avec réparation des boucles (`generate_with_repair_and_attention`).
    fn decode_with_repair(&mut self, xa: &Tensor, prompt: &[u32], on_token: &mut dyn FnMut()) -> Result<Decode> {
        let (mut d, _) = self.generate(xa, prompt, MAX_NEW_TOKENS, None, None, 0, Some(&mut *on_token))?;
        for _ in 0..MAX_REPAIRS {
            let Some((s, gram)) = Self::find_loop(&d.ids, &REPAIR) else { break };
            let keep = s + gram.len();
            d.ids.truncate(keep);
            d.attn.truncate(keep);
            let remaining = MAX_NEW_TOKENS.saturating_sub(d.ids.len());
            if remaining == 0 {
                break;
            }
            let p2: Vec<u32> = prompt.iter().chain(&d.ids).copied().collect();
            let (tail, _) = self.generate(xa, &p2, remaining, Some(gram[0]), None, 0, Some(&mut *on_token))?;
            d.ids.extend(tail.ids);
            d.attn.extend(tail.attn);
        }
        Ok(d)
    }

    /// Secondes de parole active dans la fenêtre (énergie du mel, percentiles 10 / 95).
    fn speech_energy(mel: &[Vec<f32>]) -> (Vec<f32>, f64, f64) {
        let frames = mel[0].len();
        let n = mel.len() as f32;
        let e: Vec<f32> = (0..frames).map(|f| mel.iter().map(|r| r[f]).sum::<f32>() / n).collect();
        let floor = percentile(&e, 10.0);
        let peak = percentile(&e, 95.0);
        (e, floor, peak)
    }

    fn speech_seconds_from(mel: &[Vec<f32>], from: usize) -> f64 {
        let (e, floor, peak) = Self::speech_energy(mel);
        if peak - floor < 1e-3 || from >= e.len() {
            return 0.0;
        }
        let thr = floor + 0.2 * (peak - floor);
        e[from..].iter().filter(|&&x| x as f64 > thr).count() as f64 * 0.01
    }

    /// Relance si le décodage a produit trop peu de mots pour la parole entendue (`fallback.py`).
    fn coverage_fallback(&mut self, xa: &Tensor, prompt: &[u32], sibling: &[u32], d: Decode, mel: &[Vec<f32>]) -> Result<Decode> {
        let sec = Self::speech_seconds_from(mel, 0);
        let under = |n: usize| sec >= 5.0 && (n as f64) < 0.5 * sec;
        let base_n = self.word_count(&d.ids);
        if !under(base_n) {
            return Ok(d);
        }
        if let Ok((r, _)) = self.generate(xa, sibling, MAX_NEW_TOKENS, None, None, 0, None) {
            let mut rid = r.ids;
            if rid.last() == Some(&EOT) {
                rid.pop();
            }
            let ref_n = self.word_count(&rid);
            if !(ref_n >= 8 && (base_n as f64) < 0.5 * ref_n as f64) {
                return Ok(d);
            }
        }
        let (mut best, mut best_n, mut sampled) = (d.ids.clone(), base_n, false);
        let mut seed = 0u64;
        'outer: for t in FALLBACK_TEMPERATURES {
            for _ in 0..SAMPLES_PER_TEMP {
                let mut rng = Rng::new(seed);
                seed += 1;
                let (mut s, _) = self.generate(xa, prompt, MAX_NEW_TOKENS, None, Some((t, &mut rng)), 0, None)?;
                if s.ids.last() == Some(&EOT) {
                    s.ids.pop();
                }
                let eight: Vec<(usize, usize)> = (1..=5).map(|n| (n, 8)).collect();
                if Self::find_loop(&s.ids, &eight).is_some() {
                    continue;
                }
                let n = self.word_count(&s.ids);
                if n > best_n {
                    (best, best_n, sampled) = (s.ids.clone(), n, true);
                }
                if !under(n) {
                    (best, sampled) = (s.ids, true);
                    break 'outer;
                }
            }
        }
        if !sampled {
            return Ok(d);
        }
        let attn = self.teacher_forced(xa, prompt, &best)?;
        Ok(Decode { ids: best, attn })
    }

    // ------------------------------------------------------------------ mots

    fn word_timings(&self, d: &Decode, mel: &[Vec<f32>]) -> Vec<(String, Option<Timed>)> {
        let pieces: Vec<String> = d.ids.iter().map(|&i| self.piece(i)).collect();
        let groups = timing::group_tokens_into_words(&d.ids, &pieces);
        let texts: Vec<String> = groups
            .iter()
            .map(|g| self.decode(&g.iter().map(|&i| d.ids[i]).collect::<Vec<_>>()).trim().to_string())
            .collect();
        let timed = timing::extract(&groups, &texts, &d.attn, mel);
        texts.into_iter().zip(timed.into_iter().chain(std::iter::repeat(None))).collect()
    }

    /// Fenêtre audio → mel [n_mels][3000] (complétée à 30 s).
    fn features(&self, chunk: &[f32]) -> Vec<Vec<f32>> {
        let mut a = chunk[..chunk.len().min(mel::N_SAMPLES)].to_vec();
        a.resize(mel::N_SAMPLES, 0.0);
        mel::log_mel(&a, self.model.config.num_mel_bins, 0)
    }

    /// Une fenêtre : décodage, garde-fous, horodatages (+ reprise des fins prématurées).
    fn window(&mut self, chunk: &[f32], context: Option<&str>, longform: bool, is_last: bool, report: &mut dyn FnMut(f64)) -> Result<(Decode, Vec<(String, Option<Timed>)>)> {
        let prompt = self.prompt("verbatim", context)?;
        let sibling = self.prompt("intended", context)?;
        let mel = self.features(chunk);
        let xa = self.encode_audio(&mel)?;
        // avancement dans la fenêtre : 30 % pour l'encodeur, puis au fil des jetons (~180 par fenêtre)
        report(0.3);
        let mut tokens = 0usize;
        let d = self.decode_with_repair(&xa, &prompt, &mut || {
            tokens += 1;
            report(0.3 + 0.65 * (tokens as f64 / 180.0).min(1.0));
        })?;
        let mut d = self.coverage_fallback(&xa, &prompt, &sibling, d, &mel)?;
        let mut words = self.word_timings(&d, &mel);
        if longform {
            let last_end = words.iter().filter_map(|(_, t)| t.as_ref().map(|t| t.end)).fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))));
            if let Some(last_end) = last_end {
                let tail_min = if is_last { 2.0 } else { 4.0 };
                let gap = Self::speech_seconds_from(&mel, (last_end / 0.01).max(0.0) as usize);
                if gap >= tail_min {
                    let mut content = d.ids.clone();
                    while content.last() == Some(&EOT) {
                        content.pop();
                    }
                    if let Some(p) = self.eot_probability(&xa, &prompt, &content)? {
                        if p < 0.7 {
                            let (ext, prob) = self.generate(&xa, &prompt, MAX_NEW_TOKENS, None, None, content.len() + 1, None)?;
                            let mut ext_ids = ext.ids;
                            if ext_ids.last() == Some(&EOT) {
                                ext_ids.pop();
                            }
                            if prob.is_some_and(|p| p >= 0.9) && ext_ids.len() > content.len() {
                                ext_ids.push(EOT);
                                if ext_ids.len() != d.ids.len() {
                                    let attn = self.teacher_forced(&xa, &prompt, &ext_ids)?;
                                    d = Decode { ids: ext_ids, attn };
                                    words = self.word_timings(&d, &mel);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok((d, words))
    }

    /// Transcrit un signal 16 kHz mono. `progress` reçoit l'avancement (0..1).
    pub fn transcribe(&mut self, audio: &[f32], progress: Progress) -> Result<Vec<Word>> {
        let sr = mel::SAMPLE_RATE as f64;
        let duration = audio.len() as f64 / sr;
        progress(0.0);
        if duration <= CHUNK_S {
            let (_, words) = self.window(audio, None, false, true, &mut |f| progress(f))?;
            progress(1.0);
            return Ok(words
                .into_iter()
                .filter_map(|(_, t)| t.map(|t| Word { word: t.word, start: t.start, end: t.end }))
                .collect());
        }
        let ch = (CHUNK_S * sr) as usize;
        let st = (STRIDE_S * sr) as usize;
        let mut starts = Vec::new();
        let mut s = 0;
        while s < audio.len() {
            starts.push(s);
            if s + ch >= audio.len() {
                break;
            }
            s += st;
        }
        let n = starts.len();
        let mut confirmed: Vec<String> = Vec::new();
        let mut out: Vec<Timed> = Vec::new();
        for (i, &s) in starts.iter().enumerate() {
            let is_last = i == n - 1;
            let chunk = &audio[s..(s + ch).min(audio.len())];
            let ctx = if i == 0 || confirmed.is_empty() {
                None
            } else {
                Some(confirmed[confirmed.len().saturating_sub(CONTEXT_WORDS)..].join(" "))
            };
            let (_, words) = self.window(chunk, ctx.as_deref(), true, is_last, &mut |f| progress((i as f64 + f) / n as f64))?;
            if !words.is_empty() {
                let keep = drop_index(&words, is_last);
                let start_sec = i as f64 * STRIDE_S;
                for (text, t) in words.into_iter().take(keep) {
                    confirmed.push(text);
                    if let Some(t) = t {
                        out.push(Timed { word: t.word, start: round3(t.start + start_sec), end: round3(t.end + start_sec) });
                    }
                }
            }
            progress((i + 1) as f64 / n as f64);
        }
        timing::monotonize(&mut out);
        Ok(out.into_iter().map(|t| Word { word: t.word, start: t.start, end: t.end }).collect())
    }
}

fn round3(x: f64) -> f64 {
    // round() de Python (arrondi au pair) à 3 décimales
    let y = x * 1000.0;
    let r = y.round();
    let r = if (y - y.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - y.signum() } else { r };
    r / 1000.0
}

/// Mots gardés en fin de fenêtre (`timestamp_aware_drop`) : au plus les 2 derniers, à partir
/// du premier qui commence dans le recouvrement (≥ 26 s).
fn drop_index(words: &[(String, Option<Timed>)], is_last: bool) -> usize {
    let n = words.len();
    if is_last {
        return n;
    }
    let legacy = n.saturating_sub(DROP_WORDS);
    if words.iter().all(|(_, t)| t.is_none()) {
        return legacy;
    }
    let first_overlap = words.iter().position(|(_, t)| t.as_ref().is_some_and(|t| t.start >= STRIDE_S)).unwrap_or(n);
    legacy.max(first_overlap)
}

fn percentile(v: &[f32], q: f64) -> f64 {
    let mut s: Vec<f64> = v.iter().map(|&x| x as f64).collect();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if s.is_empty() {
        return 0.0;
    }
    let pos = q / 100.0 * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_detection_matches_python() {
        let ids = [5, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 9];
        assert_eq!(Transcriber::find_loop(&ids, &REPAIR), Some((1, vec![1, 2])));
        assert_eq!(Transcriber::find_loop(&[1, 2, 3, 4], &REPAIR), None);
    }

    #[test]
    fn python_rounding() {
        assert_eq!(round3(1.0005), 1.0); // 1.0005 n'est pas exact en binaire : comme Python
        assert_eq!(round3(2.5), 2.5);
    }

    #[test]
    fn overlap_drop() {
        let w = |s: f64| ("x".to_string(), Some(Timed { word: "x".into(), start: s, end: s + 0.2 }));
        let words = vec![w(1.0), w(25.0), w(26.2), w(27.0)];
        assert_eq!(drop_index(&words, false), 2);
        assert_eq!(drop_index(&words, true), 4);
        let early = vec![w(1.0), w(2.0), w(3.0)];
        assert_eq!(drop_index(&early, false), 3);
    }
}
