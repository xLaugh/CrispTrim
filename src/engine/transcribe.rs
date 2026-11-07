//! Mots horodatés. V1 Rust : relit les transcriptions CrisperWhisper déjà faites (cache disque de
//! CrispTrim, ou export JSON de la ligne de commande). Le modèle lui-même n'est pas encore porté.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
