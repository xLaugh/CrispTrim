//! Carte graphique NVIDIA : pilote présent ? bibliothèques CUDA (cuBLAS, cuRAND) disponibles ?
//! Elles viennent du CUDA Toolkit, de la version Python (PyTorch), ou d'un téléchargement unique
//! des paquets officiels NVIDIA (~620 Mo) dans le dossier de CrispTrim.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Bibliothèques chargées par candle à l'exécution.
pub const DLLS: [&str; 4] = ["cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll", "curand64_10.dll"];
/// Paquets PyPI de NVIDIA (roues Windows) : (nom, version).
const WHEELS: [(&str, &str); 3] =
    [("nvidia-cuda-runtime-cu12", "12.9.79"), ("nvidia-cublas-cu12", "12.9.1.4"), ("nvidia-curand-cu12", "10.3.10.19")];

/// Pilote NVIDIA installé (nvcuda.dll) : sans lui, inutile d'essayer CUDA.
pub fn has_driver() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    sys.join("System32").join("nvcuda.dll").is_file()
}

fn complete(dir: &Path) -> bool {
    DLLS.iter().all(|d| dir.join(d).is_file())
}

pub fn own_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cuda")
}

/// Dossier contenant déjà les bibliothèques, s'il y en a un.
pub fn find_libs(data_dir: &Path) -> Option<PathBuf> {
    let mut dirs = vec![own_dir(data_dir)];
    if let Some(p) = std::env::var_os("CUDA_PATH") {
        dirs.push(PathBuf::from(p).join("bin"));
    }
    // PyTorch de la version Python installée
    dirs.push(data_dir.join("env").join("Lib").join("site-packages").join("torch").join("lib"));
    dirs.into_iter().find(|d| complete(d))
}

/// Ajoute le dossier au PATH du programme, pour que candle trouve les .dll.
#[cfg_attr(not(feature = "cuda"), allow(dead_code))]
pub fn use_dir(dir: &Path) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if !parts.iter().any(|p| p == dir) {
        parts.insert(0, dir.to_path_buf());
        if let Ok(joined) = std::env::join_paths(parts) {
            std::env::set_var("PATH", joined);
        }
    }
}

fn wheel_url(name: &str, version: &str) -> anyhow::Result<(String, u64)> {
    let v: serde_json::Value = ureq::get(&format!("https://pypi.org/pypi/{name}/{version}/json")).call()?.into_json()?;
    let urls = v["urls"].as_array().cloned().unwrap_or_default();
    let f = urls
        .iter()
        .find(|u| u["filename"].as_str().is_some_and(|f| f.ends_with("win_amd64.whl")))
        .ok_or_else(|| anyhow::anyhow!("pas de paquet Windows pour {name} {version}"))?;
    Ok((f["url"].as_str().unwrap_or_default().to_string(), f["size"].as_u64().unwrap_or(0)))
}

/// Télécharge cuBLAS et cuRAND (paquets NVIDIA) et en extrait les .dll. `progress(reçu, total)`.
pub fn download(data_dir: &Path, mut progress: impl FnMut(u64, u64)) -> anyhow::Result<PathBuf> {
    let dir = own_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let wheels: Vec<(String, u64)> = WHEELS.iter().map(|(n, v)| wheel_url(n, v)).collect::<anyhow::Result<_>>()?;
    let total: u64 = wheels.iter().map(|w| w.1).sum();
    let mut done = 0u64;
    for (url, _) in &wheels {
        let tmp = dir.join("paquet.whl.part");
        {
            let mut rd = ureq::get(url).call()?.into_reader();
            let mut out = std::fs::File::create(&tmp)?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = rd.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                std::io::Write::write_all(&mut out, &buf[..n])?;
                done += n as u64;
                progress(done, total);
            }
        }
        extract_dlls(&tmp, &dir)?;
        std::fs::remove_file(&tmp)?;
    }
    if !complete(&dir) {
        anyhow::bail!("bibliothèques CUDA incomplètes après téléchargement");
    }
    Ok(dir)
}

/// Une roue Python est une archive zip : on garde seulement les .dll de nvidia/*/bin/.
fn extract_dlls(wheel: &Path, dir: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(wheel)?)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
        if name.contains("/bin/") && name.to_lowercase().ends_with(".dll") {
            let file = Path::new(&name).file_name().unwrap().to_owned();
            let mut out = std::fs::File::create(dir.join(file))?;
            std::io::copy(&mut f, &mut out)?;
        }
    }
    Ok(())
}
