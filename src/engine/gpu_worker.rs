//! Module carte graphique : `crisptrim-gpu.exe` (ce programme compilé avec CUDA) transcrit pour
//! `CrispTrim.exe` (version processeur, qui démarre sur tous les PC).
//!
//! Sur un PC NVIDIA, CrispTrim.exe télécharge une fois le module et les bibliothèques NVIDIA, le
//! lance en arrière-plan (modèle gardé en mémoire d'un fichier à l'autre) et dialogue par lignes JSON :
//!   → {"source": "...", "model": "large", "language": "fr"}
//!   ← {"phase": "loading"} | {"phase": "download", "a": .., "b": ..} | {"progress": 0.42}
//!   ← {"words": [...]} ou {"error": "..."}
//! Au moindre problème, la transcription repasse sur le processeur.

use super::transcribe::{Phase, Word};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

// ============================================================================ côté module (worker)

/// `crisptrim-gpu.exe --gpu-worker` : boucle de requêtes sur l'entrée standard.
pub fn worker_main() -> i32 {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut send = |v: Value| {
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    // Le module n'a d'intérêt que sur la carte graphique.
    if !super::whisper::uses_gpu(&super::whisper::best_device()) {
        send(json!({"fatal": "carte graphique indisponible"}));
        return 2;
    }
    send(json!({"ready": true}));
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let source = PathBuf::from(req["source"].as_str().unwrap_or_default());
        let model = req["model"].as_str().unwrap_or("large").to_string();
        let language = req["language"].as_str().unwrap_or("fr").to_string();
        let data = crate::data_dir();
        let result = super::media::load_audio(&source, super::media::SAMPLE_RATE)
            .map_err(anyhow::Error::msg)
            .and_then(|audio| {
                super::transcribe::transcribe_local(&source, &audio, &model, &language, &data, &data.join("cache"), |p| {
                    send(match p {
                        Phase::Download(a, b) => json!({"phase": "download", "a": a, "b": b}),
                        Phase::DownloadGpu(a, b) => json!({"phase": "download_gpu", "a": a, "b": b}),
                        Phase::Loading => json!({"phase": "loading"}),
                        Phase::Transcribing(f) => json!({"progress": f}),
                    })
                })
            });
        send(match result {
            Ok(words) => json!({"words": words}),
            Err(e) => json!({"error": e.to_string()}),
        });
    }
    0
}

// ============================================================================ côté CrispTrim.exe

struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

enum State {
    NotStarted,
    Running(Worker),
    /// Échec : processeur jusqu'à la fin de la session.
    Broken,
}

static WORKER: Mutex<State> = Mutex::new(State::NotStarted);

fn module_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("gpu")
}

/// Module de cette version : `CRISPTRIM_GPU_EXE` (essais), sinon `gpu/crisptrim-gpu-<version>.exe`.
fn module_exe(data_dir: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("CRISPTRIM_GPU_EXE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    module_dir(data_dir).join(format!("crisptrim-gpu-{}.exe", super::updates::VERSION))
}

fn download_module(exe: &Path, phase: &mut dyn FnMut(Phase)) -> anyhow::Result<()> {
    let url = format!(
        "https://github.com/{}/releases/download/v{}/crisptrim-gpu.exe",
        super::updates::REPO,
        super::updates::VERSION
    );
    let resp = ureq::get(&url).call()?;
    let total: u64 = resp.header("content-length").and_then(|s| s.parse().ok()).unwrap_or(0);
    std::fs::create_dir_all(exe.parent().unwrap())?;
    let part = exe.with_extension("part");
    {
        let mut rd = resp.into_reader();
        let mut out = std::fs::File::create(&part)?;
        let mut buf = vec![0u8; 1 << 20];
        let mut done = 0u64;
        loop {
            let n = rd.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            phase(Phase::DownloadGpu(done, total));
        }
    }
    std::fs::rename(&part, exe)?;
    // anciennes versions du module
    if let Ok(rd) = std::fs::read_dir(exe.parent().unwrap()) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.starts_with("crisptrim-gpu-") && p != exe {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(())
}

/// Télécharge si besoin le module et les bibliothèques NVIDIA, puis le lance.
fn start(data_dir: &Path, phase: &mut dyn FnMut(Phase)) -> anyhow::Result<Worker> {
    let libs = match super::whisper::cuda::find_libs(data_dir) {
        Some(d) => d,
        None => super::whisper::cuda::download(data_dir, |a, b| phase(Phase::DownloadGpu(a, b)))?,
    };
    let exe = module_exe(data_dir);
    if !exe.is_file() {
        download_module(&exe, phase)?;
    }
    phase(Phase::Loading);
    // cudart est chargé dès le lancement du module : son dossier passe en tête du PATH.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![libs.clone()];
    parts.extend(std::env::split_paths(&path));
    let mut cmd = Command::new(&exe);
    cmd.arg("--gpu-worker")
        .env("PATH", std::env::join_paths(parts)?)
        .env("CRISPTRIM_HOME", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut w = Worker { child, stdin, stdout };
    let first = read(&mut w)?;
    if first["ready"].as_bool() != Some(true) {
        anyhow::bail!("{}", first["fatal"].as_str().unwrap_or("module carte graphique indisponible"));
    }
    Ok(w)
}

fn read(w: &mut Worker) -> anyhow::Result<Value> {
    let mut line = String::new();
    if w.stdout.read_line(&mut line)? == 0 {
        anyhow::bail!("le module carte graphique s'est arrêté");
    }
    Ok(serde_json::from_str(&line)?)
}

/// Transcrit avec le module carte graphique. `Err` → l'appelant transcrit sur le processeur.
pub fn transcribe(source: &Path, model: &str, language: &str, data_dir: &Path, phase: &mut dyn FnMut(Phase)) -> anyhow::Result<Vec<Word>> {
    let mut state = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if matches!(*state, State::Broken) {
        anyhow::bail!("module carte graphique indisponible");
    }
    if !matches!(*state, State::Running(_)) {
        match start(data_dir, phase) {
            Ok(w) => *state = State::Running(w),
            Err(e) => {
                *state = State::Broken;
                return Err(e);
            }
        }
    }
    let State::Running(w) = &mut *state else { unreachable!() };
    let req = json!({"source": source.to_string_lossy(), "model": model, "language": language});
    let run = (|| -> anyhow::Result<Vec<Word>> {
        writeln!(w.stdin, "{req}")?;
        w.stdin.flush()?;
        loop {
            let v = read(w)?;
            if let Some(words) = v.get("words") {
                return Ok(serde_json::from_value(words.clone())?);
            }
            if let Some(e) = v["error"].as_str() {
                anyhow::bail!("{e}");
            }
            if let Some(f) = v["progress"].as_f64() {
                phase(Phase::Transcribing(f));
            } else {
                match v["phase"].as_str() {
                    Some("loading") => phase(Phase::Loading),
                    Some("download") => phase(Phase::Download(v["a"].as_u64().unwrap_or(0), v["b"].as_u64().unwrap_or(0))),
                    Some("download_gpu") => phase(Phase::DownloadGpu(v["a"].as_u64().unwrap_or(0), v["b"].as_u64().unwrap_or(0))),
                    _ => {}
                }
            }
        }
    })();
    if run.is_err() {
        *state = State::Broken; // module planté : processeur pour la suite
    }
    run
}
