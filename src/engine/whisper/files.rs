//! Fichiers du modèle CrisperWhisper : cache Hugging Face existant, sinon téléchargement
//! dans le dossier de CrispTrim (avec avancement).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FILES: [&str; 4] = ["config.json", "generation_config.json", "tokenizer.json", "model.safetensors"];

pub fn repo(model: &str) -> String {
    format!("nyralabs/CrisperWhisper2.0_{model}")
}

fn complete(dir: &Path) -> bool {
    FILES.iter().all(|f| dir.join(f).is_file())
}

/// Dossier du cache Hugging Face (HF_HOME, sinon ~/.cache/huggingface).
fn hf_hub() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("HF_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(h).join("hub"));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".cache").join("huggingface").join("hub"))
}

/// Modèle déjà présent : téléchargé par la version Python (cache HF) ou par cette version.
pub fn find(model: &str, data_dir: &Path) -> Option<PathBuf> {
    let own = data_dir.join("models").join(format!("CrisperWhisper2.0_{model}"));
    if complete(&own) {
        return Some(own);
    }
    let mut hubs: Vec<PathBuf> = hf_hub().into_iter().collect();
    hubs.push(data_dir.join("models").join("hub")); // HF_HOME de l'installateur Python
    for hub in hubs {
        let snaps = hub.join(format!("models--nyralabs--CrisperWhisper2.0_{model}")).join("snapshots");
        let Ok(rd) = std::fs::read_dir(&snaps) else { continue };
        for e in rd.flatten() {
            if complete(&e.path()) {
                return Some(e.path());
            }
        }
    }
    None
}

/// Télécharge les fichiers manquants. `progress(octets reçus, total)`.
pub fn download(model: &str, data_dir: &Path, mut progress: impl FnMut(u64, u64)) -> anyhow::Result<PathBuf> {
    let dir = data_dir.join("models").join(format!("CrisperWhisper2.0_{model}"));
    std::fs::create_dir_all(&dir)?;
    let base = format!("https://huggingface.co/{}/resolve/main", repo(model));
    // tailles d'abord, pour un pourcentage global
    let mut sizes = Vec::new();
    for f in FILES {
        let r = ureq::head(&format!("{base}/{f}")).call()?;
        let n = r.header("x-linked-size").or(r.header("content-length")).and_then(|s| s.parse().ok()).unwrap_or(0u64);
        sizes.push(n);
    }
    let total: u64 = sizes.iter().sum();
    let mut done = 0u64;
    for (f, size) in FILES.iter().zip(&sizes) {
        let target = dir.join(f);
        if target.is_file() && target.metadata()?.len() == *size {
            done += size;
            continue;
        }
        let part = dir.join(format!("{f}.part"));
        let resp = ureq::get(&format!("{base}/{f}")).call()?;
        let mut rd = resp.into_reader();
        let mut out = std::fs::File::create(&part)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = rd.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        out.flush()?;
        drop(out);
        std::fs::rename(&part, &target)?;
    }
    Ok(dir)
}
