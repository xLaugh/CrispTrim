//! Mots horodatés : transcriptions déjà faites (cache disque, partagé avec la version Python),
//! sinon transcription CrisperWhisper en Rust (`engine::whisper`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::whisper::{self, Transcriber};

/// Étape en cours d'une transcription, pour l'affichage.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Téléchargement du modèle : octets reçus, total.
    Download(u64, u64),
    /// Téléchargement des bibliothèques NVIDIA (une seule fois) : octets reçus, total.
    DownloadGpu(u64, u64),
    Loading,
    /// Fraction du fichier transcrite (0..1).
    Transcribing(f64),
}

/// Modèle chargé une fois, gardé d'un fichier à l'autre.
static LOADED: Mutex<Option<Transcriber>> = Mutex::new(None);

/// Modèle par défaut : large sur carte graphique, turbo sur processeur (plus rapide).
pub fn default_model() -> &'static str {
    if whisper::gpu_available() { "large" } else { "turbo" }
}

/// Transcrit `audio` (16 kHz mono) et enregistre le résultat dans `cache_dir`
/// sous le même nom que la version Python.
pub fn transcribe_file(
    source: &Path,
    audio: &[f32],
    model: &str,
    language: &str,
    data_dir: &Path,
    cache_dir: &Path,
    mut phase: impl FnMut(Phase),
) -> anyhow::Result<Vec<Word>> {
    let mut slot = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().map_or(true, |t| t.model_name != model) {
        *slot = None;
        let dir = match whisper::files::find(model, data_dir) {
            Some(d) => d,
            None => whisper::files::download(model, data_dir, |a, b| phase(Phase::Download(a, b)))?,
        };
        // Carte NVIDIA : bibliothèques CUDA téléchargées une fois si besoin (sinon processeur).
        if whisper::gpu_available() && whisper::cuda::find_libs(data_dir).is_none() {
            if let Err(e) = whisper::cuda::download(data_dir, |a, b| phase(Phase::DownloadGpu(a, b))) {
                eprintln!("bibliothèques CUDA : {e} — transcription sur le processeur");
            }
        }
        phase(Phase::Loading);
        let device = whisper::best_device();
        let loaded = match Transcriber::load(&dir, model, language, &device) {
            Err(e) if whisper::uses_gpu(&device) => {
                eprintln!("carte graphique : {e} — transcription sur le processeur");
                Transcriber::load(&dir, model, language, &candle_core::Device::Cpu)?
            }
            r => r?,
        };
        *slot = Some(loaded);
    }
    let t = slot.as_mut().unwrap();
    phase(Phase::Transcribing(0.0));
    let words = t.transcribe(audio, &mut |f| phase(Phase::Transcribing(f)))?;
    if let Some(key) = cache_key(source, model, language) {
        let _ = std::fs::create_dir_all(cache_dir);
        let f = cache_dir.join(format!("{}.{key}.words.json", stem(source)));
        if let Ok(s) = serde_json::to_string(&words) {
            let _ = std::fs::write(f, s);
        }
    }
    Ok(words)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Word {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

pub const MODELS: [(&str, &str); 4] =
    [("large", "large · précis"), ("turbo", "turbo · rapide"), ("medium", "medium"), ("small", "small")];

/// Chemin tel que Python l'écrit avec `Path.resolve()` (sans le préfixe `\\?\`).
pub fn resolved(path: &Path) -> PathBuf {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p
}

/// Même clé que `Transcriber.cache_key` : sha1("chemin|taille|mtime_ns|modèle|langue")[:16].
pub fn cache_key(source: &Path, model: &str, language: &str) -> Option<String> {
    let meta = std::fs::metadata(source).ok()?;
    let mtime_ns = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let raw = format!("{}|{}|{}|{}|{}", resolved(source).display(), meta.len(), mtime_ns, model, language);
    let hex = sha1_smol::Sha1::from(raw.as_bytes()).digest().to_string();
    Some(hex[..16].to_string())
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn read_words(file: &Path) -> Option<Vec<Word>> {
    let txt = std::fs::read_to_string(file).ok()?;
    if let Ok(w) = serde_json::from_str::<Vec<Word>>(&txt) {
        return Some(w);
    }
    // Export JSON de la ligne de commande : { "words": [...] }
    let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let words: Vec<Word> = serde_json::from_value(v.get("words")?.clone()).ok()?;
    (!words.is_empty()).then_some(words)
}

/// Cherche une transcription existante du fichier : modèle choisi d'abord, puis les autres.
pub fn find_cached(source: &Path, cache_dirs: &[PathBuf], model: &str, language: &str) -> Option<(Vec<Word>, PathBuf)> {
    let mut models: Vec<&str> = vec![model];
    models.extend(MODELS.iter().map(|m| m.0).filter(|m| *m != model));
    let st = stem(source);
    for m in models {
        let Some(key) = cache_key(source, m, language) else { continue };
        for dir in cache_dirs {
            let f = dir.join(format!("{st}.{key}.words.json"));
            if let Some(w) = read_words(&f) {
                return Some((w, f));
            }
        }
    }
    // Export JSON de la ligne de commande, à côté du rush.
    if let Some(parent) = source.parent() {
        let f = parent.join(format!("{st}_crisptrim.json"));
        if let Some(w) = read_words(&f) {
            return Some((w, f));
        }
    }
    None
}
