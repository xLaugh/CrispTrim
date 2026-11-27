// Pas de console qui s'ouvre avec la fenêtre (la ligne de commande se rattache à celle du terminal).
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod cli;
mod engine;
mod ui;

use std::path::PathBuf;

/// Dossier de données de CrispTrim (cache de transcription partagé avec la version Python).
/// Même règle que le lanceur (`Paths.__init__`) : CRISPTRIM_HOME, sinon le dossier du programme s'il
/// contient install.json (version installée), sinon %LOCALAPPDATA%\CrispTrim.
pub fn data_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("CRISPTRIM_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        if dir.join("install.json").exists() {
            return dir;
        }
    }
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = env("HOME").unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(windows) {
        env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"))
    } else if cfg!(target_os = "macos") {
        // ~/Library/Application Support/CrispTrim
        home.join("Library").join("Application Support")
    } else {
        // Linux et autres : ~/.local/share/CrispTrim
        env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"))
    };
    base.join("CrispTrim")
}

#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    // ATTACH_PARENT_PROCESS
    unsafe {
        AttachConsole(u32::MAX);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Module carte graphique lancé par CrispTrim.exe (voir engine::gpu_worker).
    if args.first().map(String::as_str) == Some("--gpu-worker") {
        std::process::exit(engine::gpu_worker::worker_main());
    }
    // Mise à jour sans fenêtre : crisptrim --update-to v0.3.3
    if args.first().map(String::as_str) == Some("--update-to") {
        attach_console();
        let tag = args.get(1).cloned().unwrap_or_default();
        match engine::updates::install_update(&tag, &mut |a, b| eprint!("\r  téléchargement : {} / {} Mo   ", a / 1_000_000, b / 1_000_000)) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("\nmise à jour impossible : {e}");
                std::process::exit(1);
            }
        }
    }
    let mut screenshot: Option<PathBuf> = None;
    let mut open: Vec<PathBuf> = Vec::new();
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => screenshot = it.next().map(PathBuf::from),
            "--open" => open.extend(it.next().map(PathBuf::from)),
            // essai : lance la lecture dès que le son est prêt
            "--play" => std::env::set_var("CRISPTRIM_AUTOPLAY", "1"),
            _ => rest.push(a),
        }
    }
    if !rest.is_empty() {
        attach_console();
        std::process::exit(cli::main(rest));
    }
    if let Err(e) = ui::run(screenshot, open) {
        eprintln!("CrispTrim : {e}");
        std::process::exit(1);
    }
}
