//! Ligne de commande : crisptrim rush1.mp4 rush2.mov -o sortie/

use crate::engine::cutter::{compute_cuts, to_frames, CutParams};
use crate::engine::export::{render_mp4, write_fcpxml, write_json, write_premiere_xml, write_vegas_edl};
use crate::engine::media::{load_audio, probe, require_ffmpeg, SAMPLE_RATE};
use crate::engine::silence::{detect_speech_db, frame_db, SilenceParams};
use crate::engine::text::{write_srt, write_txt};
use crate::engine::transcribe::find_cached;
use crate::engine::updates::VERSION;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

const EXPORTS: [&str; 7] = ["fcpxml", "premiere", "vegas", "mp4", "json", "txt", "srt"];

const HELP: &str = "usage : crisptrim [fichiers…] [options]

Coupe les silences et les hésitations (« euh ») de tes rushes, puis exporte une timeline.
Sans fichier, ouvre la fenêtre.

  -o, --output DOSSIER     dossier de sortie (défaut : à côté du fichier)
  --export LISTE           parmi fcpxml,premiere,vegas,mp4,json,txt,srt (défaut : fcpxml,premiere,json)

silences :
  --threshold-db DB        seuil de silence en dBFS (défaut : automatique)
  --min-silence S          durée minimale d'un silence coupé, en s (0.5)
  --margin S               marge gardée autour de la parole, en s (0.15)
  --min-keep S             segment gardé minimal, en s (0.25)

hésitations (transcriptions CrisperWhisper déjà faites) :
  --no-fillers             coupe seulement les silences
  --model NOM              large | turbo | medium | small (défaut : large)
  --language CODE          code langue (défaut : fr)
  --keep-noises            garder les bruits de bouche ([yawn], [cough]…)
  --extra-filler MOT       mot supplémentaire à couper (répétable)
  --version                affiche la version";

pub fn main(args: Vec<String>) -> i32 {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut export = "fcpxml,premiere,json".to_string();
    let mut sil = SilenceParams::default();
    let mut cut = CutParams::default();
    let mut no_fillers = false;
    let mut model = crate::engine::transcribe::default_model().to_string();
    let mut language = "fr".to_string();
    let mut extra: HashSet<String> = HashSet::new();

    let mut it = args.into_iter();
    macro_rules! value {
        ($name:expr) => {
            match it.next() {
                Some(v) => v,
                None => {
                    eprintln!("{} : valeur manquante", $name);
                    return 2;
                }
            }
        };
    }
    macro_rules! number {
        ($name:expr) => {{
            let v = value!($name);
            match v.parse::<f64>() {
                Ok(n) => n,
                Err(_) => {
                    eprintln!("{} : nombre attendu, reçu « {v} »", $name);
                    return 2;
                }
            }
        }};
    }
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return 0;
            }
            "--version" => {
                println!("crisptrim {VERSION}");
                return 0;
            }
            "-o" | "--output" => output = Some(PathBuf::from(value!("--output"))),
            "--export" => export = value!("--export"),
            "--threshold-db" => sil.threshold_db = Some(number!("--threshold-db")),
            "--min-silence" => sil.min_silence = number!("--min-silence"),
            "--margin" => cut.margin = number!("--margin"),
            "--min-keep" => cut.min_keep = number!("--min-keep"),
            "--no-fillers" => no_fillers = true,
            "--model" => model = value!("--model"),
            "--language" => language = value!("--language"),
            "--keep-noises" => cut.cut_noises = false,
            "--extra-filler" => {
                extra.insert(value!("--extra-filler").to_lowercase());
            }
            "--device" => {
                let _ = value!("--device");
            }
            s if s.starts_with('-') => {
                eprintln!("Option inconnue : {s}\n\n{HELP}");
                return 2;
            }
            _ => inputs.push(PathBuf::from(a)),
        }
    }
    cut.cut_fillers = !no_fillers;
    cut.extra_fillers = extra;

    let exports: HashSet<String> = export.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
    let mut unknown: Vec<&String> = exports.iter().filter(|e| !EXPORTS.contains(&e.as_str())).collect();
    if !unknown.is_empty() {
        unknown.sort();
        eprintln!("Export inconnu : {}", unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        return 2;
    }
    if let Err(e) = require_ffmpeg() {
        eprintln!("{e}");
        return 1;
    }
    let has = |k: &str| exports.contains(k);

    for src in inputs {
        if !src.exists() {
            eprintln!("Introuvable : {}", src.display());
            continue;
        }
        let t0 = Instant::now();
        let info = match probe(&src) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("{} : {e}", src.display());
                continue;
            }
        };
        let fname = info.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if !info.has_audio {
            eprintln!("{fname} : pas de piste audio, ignoré.");
            continue;
        }
        let outdir = output.clone().unwrap_or_else(|| info.path.parent().map(|p| p.to_path_buf()).unwrap_or_default());
        let _ = std::fs::create_dir_all(&outdir);
        println!("▶ {fname}  ({:.1} s, {:.3} i/s)", info.duration, info.fps.as_f64());

        let audio = match load_audio(&info.path, SAMPLE_RATE) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("  {e}");
                continue;
            }
        };
        let (speech, thr) = detect_speech_db(&frame_db(&audio, SAMPLE_RATE), &sil);
        println!("  seuil de silence : {thr:.1} dBFS");

        let mut words = None;
        if !no_fillers {
            let dirs = vec![crate::data_dir().join("cache"), outdir.join(".crisptrim-cache")];
            match find_cached(&info.path, &dirs, &model, &language) {
                Some((w, f)) => {
                    println!("  transcription reprise : {}", f.display());
                    words = Some(w);
                }
                None => {
                    let t0 = std::time::Instant::now();
                    let mut last = -1i64;
                    let r = crate::engine::transcribe::transcribe_file(
                        &info.path, &audio, &model, &language, &crate::data_dir(), &crate::data_dir().join("cache"),
                        |p| {
                            use crate::engine::transcribe::Phase;
                            match p {
                                Phase::Download(a, b) => eprint!("\r  téléchargement du modèle : {} / {} Mo   ", a / 1_000_000, b / 1_000_000),
                                Phase::DownloadGpu(a, b) => eprint!("\r  bibliothèques NVIDIA : {} / {} Mo   ", a / 1_000_000, b / 1_000_000),
                                Phase::Loading => eprintln!("\n  chargement du modèle {model}…"),
                                Phase::Transcribing(f) => {
                                    let pc = (f * 100.0) as i64;
                                    if pc != last {
                                        last = pc;
                                        eprint!("\r  transcription : {pc} %   ");
                                    }
                                }
                            }
                        },
                    );
                    eprintln!();
                    match r {
                        Ok(w) => {
                            println!("  transcription faite en {:.0} s ({} mots)", t0.elapsed().as_secs_f64(), w.len());
                            words = Some(w);
                        }
                        Err(e) => eprintln!("  transcription impossible : {e} — silences seulement."),
                    }
                }
            }
        }

        let result = compute_cuts(info.duration, &speech, words.as_deref(), &cut);
        let frames = to_frames(&result.keep, info.fps, info.total_frames());
        if frames.is_empty() {
            eprintln!("  rien à garder (seuil trop haut ?)");
            continue;
        }
        let st = info.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let stem = |ext: &str| outdir.join(format!("{st}_crisptrim.{ext}"));
        let name = format!("{st} (CrispTrim)");
        let report = |r: std::io::Result<()>| {
            if let Err(e) = r {
                eprintln!("  écriture impossible : {e}");
            }
        };
        if has("fcpxml") {
            report(write_fcpxml(&info, &frames, &stem("fcpxml"), &name));
        }
        if has("premiere") {
            report(write_premiere_xml(&info, &frames, &stem("xml"), &name));
        }
        if has("vegas") {
            report(write_vegas_edl(&info, &frames, &stem("vegas.txt")));
        }
        if has("json") {
            if let Err(e) = write_json(&info, &result, words.as_deref(), thr, &stem("json")) {
                eprintln!("  {e}");
            }
        }
        if has("txt") || has("srt") {
            match &words {
                None => eprintln!("  .txt/.srt ignorés : pas de transcription."),
                Some(w) => {
                    let aligned: Vec<(f64, f64)> = frames.iter().map(|&(a, b)| (info.fps.secs(a), info.fps.secs(b))).collect();
                    if has("txt") {
                        report(write_txt(w, &aligned, &stem("txt")));
                    }
                    if has("srt") {
                        report(write_srt(w, &aligned, &stem("srt")));
                    }
                }
            }
        }
        if has("mp4") {
            println!("  rendu MP4…");
            if let Err(e) = render_mp4(&info, &frames, &stem("mp4")) {
                eprintln!("  {e}");
            }
        }
        let s = result.stats();
        println!(
            "  ✓ {:.1} s → {:.1} s  (−{} %, {} segments, {} hésitations/bruits)  en {:.1} s",
            s.duration, s.kept, s.removed_pct, s.segments, s.fillers, t0.elapsed().as_secs_f64()
        );
    }
    0
}
