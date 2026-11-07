//! Fenêtre de CrispTrim (egui).

mod jobs;
mod panels;
mod player;
mod side;
pub mod theme;
mod timeline;

use crate::engine::media::{is_media, MediaInfo, MEDIA_EXT};
use crate::engine::transcribe::{Word, MODELS};
use crate::engine::updates::{self, UpdateStatus};
use eframe::egui::{self, TextureHandle};
use jobs::{cuts_of, Config, Cut, ExportState, JobRef, Queue, Settings, Stage};
use player::{Player, Video};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use timeline::Timeline;

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Settings,
    Cuts,
    Text,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CutFilter {
    All,
    Filler,
    Silence,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Modal {
    None,
    About,
    Update,
}

/// Copie du fichier sélectionné, rafraîchie quand le moteur change quelque chose.
pub struct View {
    pub id: u64,
    pub stage: Stage,
    pub error: String,
    pub info: Option<MediaInfo>,
    pub db: Arc<Vec<f32>>,
    pub words: Vec<Word>,
    pub has_words: bool,
    pub threshold: f64,
    pub settings: Settings,
    pub export: ExportState,
    pub version: u64,
    pub playback: Option<Arc<Vec<i16>>>,
}

pub struct App {
    config: Arc<Mutex<Config>>,
    queue: Queue,
    jobs: Vec<JobRef>,
    sel: Option<u64>,
    view: Option<View>,
    cuts: Vec<Cut>,
    keep_regions: Vec<(f64, f64)>,
    off_keys: HashMap<u64, HashSet<String>>,
    /// Réglages en cours d'édition (appliqués aux nouveaux fichiers aussi).
    edit: Settings,
    auto: bool,
    threshold_slider: f64,
    settings_changed: Option<Instant>,
    player: Player,
    player_for: Option<u64>,
    video: Video,
    video_tex: Option<TextureHandle>,
    video_shown_t: f64,
    still_wanted: Option<f64>,
    last_video_start: Instant,
    thumbs: HashMap<u64, TextureHandle>,
    tl: Timeline,
    tab: Tab,
    filter: CutFilter,
    path_input: String,
    model: String,
    preview: bool,
    toast: Option<(String, bool, Instant)>,
    update: Option<UpdateStatus>,
    update_rx: Option<Receiver<UpdateStatus>>,
    check_toast: bool,
    modal: Modal,
    skip_version: Option<String>,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    word_now: Option<usize>,
}

fn ui_state_file() -> PathBuf {
    crate::data_dir().join("rust-ui.json")
}

impl App {
    fn new(cc: &eframe::CreationContext, screenshot: Option<PathBuf>, open: Vec<PathBuf>) -> App {
        theme::install_fonts(&cc.egui_ctx);
        theme::style(&cc.egui_ctx);
        let player = Player::new();
        let saved: serde_json::Value =
            std::fs::read_to_string(ui_state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let model = saved["model"].as_str().filter(|m| MODELS.iter().any(|x| x.0 == *m)).unwrap_or("large").to_string();
        let config = Arc::new(Mutex::new(Config {
            model: model.clone(),
            language: "fr".into(),
            cache_dirs: vec![crate::data_dir().join("cache")],
            playback_rate: player.rate(),
        }));
        let ctx = cc.egui_ctx.clone();
        let queue = Queue::new(config.clone(), move || ctx.request_repaint());
        let mut app = App {
            config,
            queue,
            jobs: Vec::new(),
            sel: None,
            view: None,
            cuts: Vec::new(),
            keep_regions: Vec::new(),
            off_keys: HashMap::new(),
            edit: Settings::default(),
            auto: true,
            threshold_slider: -45.0,
            settings_changed: None,
            player,
            player_for: None,
            video: Video::new(),
            video_tex: None,
            video_shown_t: -1.0,
            still_wanted: None,
            last_video_start: Instant::now(),
            thumbs: HashMap::new(),
            tl: Timeline::default(),
            tab: Tab::Settings,
            filter: CutFilter::All,
            path_input: String::new(),
            model,
            preview: true,
            toast: None,
            update: None,
            update_rx: None,
            check_toast: false,
            modal: Modal::None,
            skip_version: saved["skip_version"].as_str().map(String::from),
            screenshot,
            frames: 0,
            shot_requested: false,
            word_now: None,
        };
        if let Err(e) = crate::engine::media::require_ffmpeg() {
            app.toast(&e, true);
        }
        app.add_paths(open);
        app
    }

    fn save_ui_state(&self) {
        let v = serde_json::json!({ "model": self.model, "skip_version": self.skip_version });
        let _ = std::fs::create_dir_all(crate::data_dir());
        let _ = std::fs::write(ui_state_file(), v.to_string());
    }

    pub fn toast(&mut self, msg: &str, error: bool) {
        self.toast = Some((msg.to_string(), error, Instant::now()));
    }

    // ------------------------------------------------------------------ fichiers

    pub fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut first = None;
        for p in paths {
            let p = PathBuf::from(p.to_string_lossy().trim().trim_matches('"'));
            if !p.is_file() {
                self.toast(&format!("Fichier introuvable : {}", p.display()), true);
                continue;
            }
            let job = self.queue.add(p, self.edit.clone());
            let id = job.lock().unwrap().id;
            first.get_or_insert(id);
            self.jobs.push(job);
        }
        if let Some(id) = first {
            self.select(id);
        }
    }

    pub fn pick_files(&mut self) {
        let files = rfd::FileDialog::new()
            .set_title("Choisir des rushes")
            .add_filter("Vidéo et audio", &MEDIA_EXT)
            .add_filter("Tous les fichiers", &["*"])
            .pick_files();
        if let Some(f) = files {
            self.add_paths(f);
        }
    }

    pub fn select(&mut self, id: u64) {
        if self.sel == Some(id) {
            return;
        }
        self.sel = Some(id);
        self.view = None;
        self.tl.fitted = None;
        self.word_now = None;
    }

    pub fn remove(&mut self, id: u64) {
        self.jobs.retain(|j| j.lock().unwrap().id != id);
        self.thumbs.remove(&id);
        if self.sel == Some(id) {
            self.sel = None;
            self.view = None;
            self.player.load(0.0, None);
            self.player_for = None;
            self.video.stop();
            self.video_tex = None;
        }
    }

    pub fn job(&self, id: u64) -> Option<JobRef> {
        self.jobs.iter().find(|j| j.lock().unwrap().id == id).cloned()
    }

    /// Rafraîchit la copie du fichier sélectionné si le moteur l'a changé.
    fn refresh(&mut self) {
        if self.sel.is_none() {
            if let Some(j) = self.jobs.first() {
                let id = j.lock().unwrap().id;
                self.select(id);
            }
        }
        let Some(id) = self.sel else { return };
        let Some(job) = self.job(id) else {
            self.sel = None;
            self.view = None;
            return;
        };
        let j = job.lock().unwrap();
        if self.view.as_ref().map(|v| v.version == j.version && v.id == j.id).unwrap_or(false) {
            return;
        }
        let switched = self.view.as_ref().map(|v| v.id != j.id).unwrap_or(true);
        self.view = Some(View {
            id: j.id,
            stage: j.stage,
            error: j.error.clone(),
            info: j.info.clone(),
            db: j.db.clone(),
            words: j.words.clone().unwrap_or_default(),
            has_words: j.words.is_some(),
            threshold: j.threshold,
            settings: j.settings.clone(),
            export: j.export.clone(),
            version: j.version,
            playback: j.playback.clone(),
        });
        let regions = j.regions.clone();
        drop(j);
        let off = self.off_keys.entry(id).or_default();
        let (keep, cuts) = cuts_of(&regions, off);
        self.keep_regions = keep;
        self.cuts = cuts;
        if switched {
            let s = self.view.as_ref().unwrap().settings.clone();
            self.fill_settings(&s);
        }
        if self.auto {
            if let Some(v) = &self.view {
                self.threshold_slider = v.threshold.round();
            }
        }
    }

    fn fill_settings(&mut self, s: &Settings) {
        self.edit = s.clone();
        self.auto = s.threshold_db.is_none();
        self.threshold_slider = s.threshold_db.unwrap_or_else(|| self.view.as_ref().map(|v| v.threshold.round()).unwrap_or(-45.0));
    }

    /// Les réglages ont bougé : recalcul 180 ms plus tard (comme l'original).
    pub fn on_settings(&mut self) {
        self.edit.threshold_db = if self.auto { None } else { Some(self.threshold_slider) };
        self.settings_changed = Some(Instant::now());
    }

    fn apply_settings(&mut self) {
        let Some(id) = self.sel else { return };
        let Some(job) = self.job(id) else { return };
        let mut j = job.lock().unwrap();
        let need_words = self.edit.cut_fillers && !j.settings.cut_fillers && j.words.is_none();
        j.settings = self.edit.clone();
        if need_words && j.stage == Stage::Ready {
            drop(j);
            let config = self.config.clone();
            let job2 = job.clone();
            std::thread::spawn(move || {
                jobs::ensure_words(&job2, &config);
                let mut j = job2.lock().unwrap();
                j.stage = Stage::Ready;
                jobs::recut(&mut j);
            });
        } else {
            jobs::recut(&mut j);
        }
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
        self.config.lock().unwrap().model = model.to_string();
        self.save_ui_state();
        self.toast(&format!("Modèle « {model} » : utilisé pour les prochains fichiers."), false);
    }

    // ------------------------------------------------------------------ coupes

    pub fn set_cut(&mut self, idx: usize, on: bool) {
        let Some(id) = self.sel else { return };
        let off = self.off_keys.entry(id).or_default();
        let c = &mut self.cuts[idx];
        c.on = on;
        if on {
            off.remove(&c.key);
        } else {
            off.insert(c.key.clone());
        }
    }

    pub fn set_all_cuts(&mut self, on: bool) {
        let Some(id) = self.sel else { return };
        let off = self.off_keys.entry(id).or_default();
        for c in &mut self.cuts {
            c.on = on;
            if on {
                off.remove(&c.key);
            } else {
                off.insert(c.key.clone());
            }
        }
    }

    /// Segments gardés au final : parole + coupes rétablies à la main.
    pub fn final_keep(&self) -> Vec<(f64, f64)> {
        let mut parts: Vec<(f64, f64)> =
            self.keep_regions.iter().copied().chain(self.cuts.iter().filter(|c| !c.on).map(|c| (c.start, c.end))).collect();
        parts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (s, e) in parts {
            if let Some(l) = out.last_mut() {
                if s <= l.1 + 1e-6 {
                    l.1 = l.1.max(e);
                    continue;
                }
            }
            out.push((s, e));
        }
        out
    }

    pub fn edited_time(&self, t: f64) -> f64 {
        let mut acc = 0.0;
        for (s, e) in self.final_keep() {
            if t <= s {
                break;
            }
            acc += t.min(e) - s;
        }
        acc
    }

    pub fn duration(&self) -> f64 {
        self.view.as_ref().and_then(|v| v.info.as_ref()).map(|i| i.duration).unwrap_or(0.0)
    }

    pub fn seek(&mut self, t: f64) {
        let d = self.duration();
        self.player.seek(t.clamp(0.0, d.max(0.0)));
        self.still_wanted = Some(self.player.time());
    }

    pub fn listen(&mut self, idx: usize) {
        let c = &self.cuts[idx];
        let (s, e) = ((c.start - 0.7).max(0.0), (c.end + 0.7).min(self.duration()));
        self.player.listen(s, e);
    }

    pub fn jump_cut(&mut self, dir: i32) {
        if self.cuts.is_empty() {
            return;
        }
        let t = self.player.time();
        let c = if dir > 0 { self.cuts.iter().find(|c| c.start > t + 0.6) } else { self.cuts.iter().rev().find(|c| c.start < t - 0.6) };
        if let Some(start) = c.map(|c| c.start) {
            self.seek((start - 0.5).max(0.0));
            let d = self.duration();
            self.tl.center_on(start, d);
        }
    }

    // ------------------------------------------------------------------ export

    pub fn export(&mut self, format: &str) {
        let Some(id) = self.sel else { return };
        let Some(job) = self.job(id) else { return };
        let keep = self.final_keep();
        match side::run_export(&job, &[format], &keep) {
            Ok(()) => {
                let msg = match format {
                    "fcpxml" => "Timeline DaVinci / Final Cut exportée",
                    "premiere" => "Timeline Premiere exportée",
                    "vegas" => "EDL Vegas exportée (Fichier > Importer > Fichier texte EDL)",
                    "srt" => "Sous-titres exportés",
                    "txt" => "Texte exporté",
                    _ => "",
                };
                if !msg.is_empty() {
                    self.toast(&format!("{msg}, à côté du rush."), false);
                }
            }
            Err(e) => self.toast(&e, true),
        }
    }

    // ------------------------------------------------------------------ mises à jour

    pub fn check_update(&mut self, force: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let file = crate::data_dir().join("update.json");
        std::thread::spawn(move || {
            let _ = tx.send(updates::check(&file, force));
        });
        self.update_rx = Some(rx);
        if force {
            self.update = None;
        }
    }

    // ------------------------------------------------------------------ lecture

    fn sync_player(&mut self, ctx: &egui::Context) {
        self.player.tick();
        let Some(v) = &self.view else { return };
        let Some(info) = v.info.clone() else { return };
        if self.player_for != Some(v.id) {
            self.player.load(info.duration, v.playback.clone());
            self.player_for = Some(v.id);
            self.video.stop();
            self.video_tex = None;
            self.video_shown_t = -1.0;
            self.still_wanted = if info.has_video { Some(0.0) } else { None };
        } else if !self.player.has_buffer() {
            if let Some(b) = &v.playback {
                self.player.set_buffer(b.clone());
                if std::env::var_os("CRISPTRIM_AUTOPLAY").is_some() {
                    std::env::remove_var("CRISPTRIM_AUTOPLAY");
                    self.player.play();
                }
            }
        }
        let skips: Vec<(f64, f64)> = if self.preview { self.cuts.iter().filter(|c| c.on).map(|c| (c.start, c.end)).collect() } else { Vec::new() };
        self.player.set_skips(&skips);

        let t = self.player.time();
        if self.player.playing() {
            // la timeline suit la lecture
            let vd = self.tl.view_dur();
            if !self.tl.dragging && (t > self.tl.start + vd * 0.92 || t < self.tl.start) {
                self.tl.start = t - vd * 0.1;
                self.tl.clamp(info.duration);
            }
            ctx.request_repaint();
        }

        if !info.has_video {
            return;
        }
        let mut first = false;
        if self.player.playing() {
            self.still_wanted = None;
            let late = self.video.last_t >= 0.0 && t - self.video.last_t > 0.6;
            let stalled = self.video.last_t < 0.0 && t - self.video.start_t > 1.0;
            if !self.video.active || t < self.video.start_t - 0.05 || late || stalled {
                self.video.start(&info, t);
                self.last_video_start = Instant::now();
            }
        } else if let Some(want) = self.still_wanted {
            if self.last_video_start.elapsed() > Duration::from_millis(70) || !self.video.active {
                self.video.start(&info, want);
                self.last_video_start = Instant::now();
                self.still_wanted = None;
            }
            ctx.request_repaint_after(Duration::from_millis(30));
        } else {
            first = self.video.last_t < 0.0;
        }
        if let Some(f) = self.video.take(t, first) {
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w as usize, f.h as usize], &f.rgba);
            match &mut self.video_tex {
                Some(tex) if tex.size() == [f.w as usize, f.h as usize] => tex.set(img, egui::TextureOptions::LINEAR),
                _ => self.video_tex = Some(ctx.load_texture("video", img, egui::TextureOptions::LINEAR)),
            }
            self.video_shown_t = f.t;
        }
        if self.video.active && self.video.last_t < 0.0 {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.modal = Modal::None;
        }
        if self.modal != Modal::None || self.view.is_none() {
            return;
        }
        let (space, p, right, left) = ctx.input(|i| {
            (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::P), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::ArrowLeft))
        });
        if space {
            self.player.toggle();
        }
        if p {
            self.preview = !self.preview;
        }
        if right {
            self.jump_cut(1);
        }
        if left {
            self.jump_cut(-1);
        }
    }

    fn drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if dropped.is_empty() {
            return;
        }
        let (media, other): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(|p| is_media(p));
        if !other.is_empty() {
            self.toast(&format!("{} fichier(s) ignoré(s) : ce n'est pas de la vidéo ou de l'audio.", other.len()), media.is_empty());
        }
        self.add_paths(media);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if self.frames == 90 && self.screenshot.is_none() {
            self.check_update(false); // ~1,5 s après l'ouverture
        }
        if let Some(rx) = &self.update_rx {
            if let Ok(st) = rx.try_recv() {
                if std::mem::take(&mut self.check_toast) {
                    if st.available {
                        self.modal = Modal::Update;
                    } else if !st.error.is_empty() && st.latest.is_none() {
                        self.toast("Impossible de joindre GitHub.", true);
                    } else {
                        self.toast("CrispTrim est à jour.", false);
                    }
                }
                self.update = Some(st);
                self.update_rx = None;
            }
        }
        if let Some(t) = self.settings_changed {
            if t.elapsed() >= Duration::from_millis(180) {
                self.settings_changed = None;
                self.apply_settings();
            } else {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        }
        self.refresh();
        self.drops(ctx);
        self.keyboard(ctx);
        self.sync_player(ctx);
        // Les fichiers en cours d'analyse font tourner les animations.
        if self.jobs.iter().any(|j| j.lock().unwrap().stage.busy()) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        panels::top_bar(self, ctx);
        panels::files_panel(self, ctx);
        side::side_panel(self, ctx);
        panels::center(self, ctx);
        side::overlays(self, ctx);

        // --screenshot : environ 120 images, puis on enregistre la fenêtre et on quitte.
        if let Some(path) = self.screenshot.clone() {
            ctx.request_repaint();
            if self.frames >= 120 && !self.shot_requested {
                self.shot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = shot {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                if let Err(e) = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8) {
                    eprintln!("capture : {e}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

pub fn run(screenshot: Option<PathBuf>, open: Vec<PathBuf>) -> Result<(), String> {
    // Logo CrispTrim (installer/crisptrim.png du dépôt) : fenêtre et barre des tâches.
    let icon = {
        let img = image::load_from_memory(include_bytes!("../../assets/crisptrim.png")).map(|i| i.into_rgba8());
        match img {
            Ok(i) => egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() },
            Err(_) => egui::IconData::default(),
        }
    };
    set_app_id();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("CrispTrim {}", updates::VERSION))
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("CrispTrim", options, Box::new(move |cc| Ok(Box::new(App::new(cc, screenshot, open)))))
        .map_err(|e| e.to_string())
}

/// Ouvre un lien dans le navigateur du système.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = crate::engine::media::tool("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Montre un fichier dans l'explorateur.
pub fn reveal(path: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."))).spawn();
}

/// Barre des tâches : CrispTrim a son propre groupe et son icône (même identifiant que la version Python).
#[cfg(windows)]
fn set_app_id() {
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
    }
    let id: Vec<u16> = "xLaugh.CrispTrim".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
}

#[cfg(not(windows))]
fn set_app_id() {}
