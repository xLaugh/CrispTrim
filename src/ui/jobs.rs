//! Les rushes : analyse en tâche de fond (un fichier à la fois), recalcul instantané des coupes.

use crate::engine::cutter::{compute_cuts, CutParams, Kind, Region, Stats};
use crate::engine::media::{load_audio, load_playback_audio, probe, thumbnail, MediaInfo, SAMPLE_RATE};
use crate::engine::silence::{detect_speech_db, frame_db, SilenceParams};
use crate::engine::transcribe::{find_cached, Word};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Waiting,
    Analysis,
    Preview,
    Transcription,
    Ready,
    Error,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Waiting => "En file d'attente",
            Stage::Analysis => "Analyse de l'audio…",
            Stage::Preview => "Création de l'aperçu…",
            Stage::Transcription => "Recherche des hésitations (CrisperWhisper)…",
            Stage::Ready => "Prêt",
            Stage::Error => "Erreur",
        }
    }

    pub fn busy(self) -> bool {
        !matches!(self, Stage::Ready | Stage::Error)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub threshold_db: Option<f64>,
    pub min_silence: f64,
    pub margin: f64,
    pub min_keep: f64,
    pub cut_silences: bool,
    pub cut_fillers: bool,
    pub cut_noises: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { threshold_db: None, min_silence: 0.5, margin: 0.15, min_keep: 0.25, cut_silences: true, cut_fillers: true, cut_noises: true }
    }
}

#[derive(Clone, Default)]
pub struct ExportState {
    /// "" | "rendu" | "fait" | "erreur"
    pub state: String,
    pub files: Vec<String>,
    pub error: String,
}

pub struct Job {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub stage: Stage,
    pub error: String,
    pub info: Option<MediaInfo>,
    pub db: Arc<Vec<f32>>,
    pub words: Option<Vec<Word>>,
    pub words_source: Option<PathBuf>,
    pub threshold: f64,
    pub regions: Vec<Region>,
    pub stats: Stats,
    pub settings: Settings,
    pub thumb: Option<(u32, u32, Vec<u8>)>,
    pub playback: Option<Arc<Vec<i16>>>,
    pub export: ExportState,
    pub version: u64,
}

pub type JobRef = Arc<Mutex<Job>>;

impl Job {
    pub fn kind_is_audio(&self) -> bool {
        self.info.as_ref().map(|i| !i.has_video).unwrap_or(false)
    }
}

/// Recalcule silences et coupes avec les réglages du fichier.
pub fn recut(job: &mut Job) {
    let Some(info) = &job.info else { return };
    if job.db.is_empty() {
        return;
    }
    let s = &job.settings;
    let duration = info.duration;
    let (mut speech, thr) = detect_speech_db(&job.db, &SilenceParams { threshold_db: s.threshold_db, min_silence: s.min_silence, ..Default::default() });
    if !s.cut_silences {
        speech = vec![(0.0, duration)];
    }
    let params = CutParams { margin: s.margin, min_keep: s.min_keep, cut_fillers: s.cut_fillers, cut_noises: s.cut_noises, ..Default::default() };
    let res = compute_cuts(duration, &speech, job.words.as_deref(), &params);
    job.threshold = thr;
    job.stats = res.stats();
    job.regions = res
        .regions
        .into_iter()
        .map(|r| Region { start: (r.start * 1000.0).round() / 1000.0, end: (r.end * 1000.0).round() / 1000.0, ..r })
        .collect();
    job.version += 1;
}

pub struct Queue {
    tx: Sender<JobRef>,
    next_id: u64,
}

pub struct Config {
    pub model: String,
    pub language: String,
    pub cache_dirs: Vec<PathBuf>,
    pub playback_rate: u32,
}

impl Queue {
    pub fn new(config: Arc<Mutex<Config>>, wake: impl Fn() + Send + 'static) -> Queue {
        let (tx, rx) = channel::<JobRef>();
        // Un seul fil de travail : un fichier à la fois.
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                if let Err(e) = process(&job, &config, &wake) {
                    let mut j = job.lock().unwrap();
                    j.stage = Stage::Error;
                    j.error = e;
                    j.version += 1;
                }
                wake();
            }
        });
        Queue { tx, next_id: 1 }
    }

    pub fn add(&mut self, path: PathBuf, settings: Settings) -> JobRef {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let job = Arc::new(Mutex::new(Job {
            id: self.next_id,
            path,
            name,
            stage: Stage::Waiting,
            error: String::new(),
            info: None,
            db: Arc::new(Vec::new()),
            words: None,
            words_source: None,
            threshold: 0.0,
            regions: Vec::new(),
            stats: Stats::default(),
            settings,
            thumb: None,
            playback: None,
            export: ExportState::default(),
            version: 0,
        }));
        self.next_id += 1;
        let _ = self.tx.send(job.clone());
        job
    }
}

fn set_stage(job: &JobRef, stage: Stage) {
    let mut j = job.lock().unwrap();
    j.stage = stage;
    j.version += 1;
}

fn process(job: &JobRef, config: &Arc<Mutex<Config>>, wake: &dyn Fn()) -> Result<(), String> {
    let path = {
        let j = job.lock().unwrap();
        j.path.clone()
    };
    set_stage(job, Stage::Analysis);
    wake();
    let info = probe(&path)?;
    if !info.has_audio {
        return Err("Ce fichier n'a pas de piste audio.".into());
    }
    {
        let mut j = job.lock().unwrap();
        j.info = Some(info.clone());
        j.version += 1;
    }
    let thumb = thumbnail(&info);
    let audio = load_audio(&info.path, SAMPLE_RATE)?;
    let db = frame_db(&audio, SAMPLE_RATE);
    drop(audio);
    {
        let mut j = job.lock().unwrap();
        j.thumb = thumb;
        j.db = Arc::new(db);
        j.stage = Stage::Preview;
        recut(&mut j); // premier résultat (silences seuls) visible tout de suite
    }
    wake();

    let rate = config.lock().unwrap().playback_rate;
    let playback = load_playback_audio(&info.path, rate, crate::ui::player::CHANNELS)?;
    {
        let mut j = job.lock().unwrap();
        j.playback = Some(Arc::new(playback));
        j.version += 1;
    }
    wake();

    let wants_words = job.lock().unwrap().settings.cut_fillers;
    if wants_words {
        ensure_words(job, config);
    }
    let mut j = job.lock().unwrap();
    j.stage = Stage::Ready;
    recut(&mut j);
    Ok(())
}

/// Reprend une transcription CrisperWhisper déjà faite pour ce fichier, s'il y en a une.
pub fn ensure_words(job: &JobRef, config: &Arc<Mutex<Config>>) {
    let path = {
        let mut j = job.lock().unwrap();
        if j.words.is_some() {
            return;
        }
        j.stage = Stage::Transcription;
        j.version += 1;
        j.info.as_ref().map(|i| i.path.clone()).unwrap_or_else(|| j.path.clone())
    };
    let (model, language, mut dirs) = {
        let c = config.lock().unwrap();
        (c.model.clone(), c.language.clone(), c.cache_dirs.clone())
    };
    if let Some(parent) = path.parent() {
        dirs.push(parent.join(".crisptrim-cache"));
    }
    let found = find_cached(&path, &dirs, &model, &language);
    let mut j = job.lock().unwrap();
    if let Some((words, src)) = found {
        j.words = Some(words);
        j.words_source = Some(src);
    }
    j.version += 1;
}

/// Une coupe (silence ou hésitation) de la liste, gardée ou recoupée à la main.
#[derive(Clone, Debug)]
pub struct Cut {
    pub start: f64,
    pub end: f64,
    pub kind: Kind,
    pub text: String,
    pub key: String,
    pub on: bool,
}

pub fn cuts_of(regions: &[Region], off: &HashSet<String>) -> (Vec<(f64, f64)>, Vec<Cut>) {
    let keep = regions.iter().filter(|r| r.kind == Kind::Keep).map(|r| (r.start, r.end)).collect();
    let cuts = regions
        .iter()
        .filter(|r| r.kind != Kind::Keep)
        .map(|r| {
            let key = format!("{:.2}-{:.2}", r.start, r.end);
            Cut { start: r.start, end: r.end, kind: r.kind, text: r.text.clone(), on: !off.contains(&key), key }
        })
        .collect();
    (keep, cuts)
}
