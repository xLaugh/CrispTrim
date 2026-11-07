//! Lecture : son (cpal) avec horloge partagée, aperçu monté qui saute les coupes, images vidéo (ffmpeg).

use crate::engine::media::{scaled_size, tool, MediaInfo};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

pub const CHANNELS: u16 = 2;
const NONE: u64 = u64::MAX;

/// État partagé entre l'interface et le fil audio. Positions en échantillons (par canal).
pub struct Shared {
    pub rate: u32,
    pos: AtomicU64,
    playing: AtomicBool,
    len: AtomicU64,
    listen_end: AtomicU64,
    buffer: Mutex<Option<Arc<Vec<i16>>>>,
    /// Coupes à sauter pendant la lecture (aperçu monté), triées.
    skips: Mutex<Vec<(u64, u64)>>,
}

impl Shared {
    /// Avance de `frames` échantillons, en sautant les coupes ; remplit `out` si donné.
    fn render(&self, frames: usize, mut out: Option<(&mut [f32], usize)>) {
        let buf = self.buffer.lock().ok().and_then(|b| b.clone());
        let skips = self.skips.lock().map(|s| s.clone()).unwrap_or_default();
        let len = self.len.load(Ordering::Relaxed);
        let start = self.pos.load(Ordering::Relaxed);
        let mut pos = start;
        let was_playing = self.playing.load(Ordering::Relaxed);
        let mut playing = was_playing;
        let listen_end = self.listen_end.load(Ordering::Relaxed);
        let mut k = skips.partition_point(|s| s.1 <= pos);
        for i in 0..frames {
            if playing {
                if listen_end == NONE {
                    while k < skips.len() && skips[k].1 <= pos {
                        k += 1;
                    }
                    if k < skips.len() && skips[k].0 <= pos {
                        pos = skips[k].1;
                    }
                } else if pos >= listen_end {
                    playing = false;
                    self.listen_end.store(NONE, Ordering::Relaxed);
                }
                if pos >= len {
                    pos = len;
                    playing = false;
                }
            }
            if let Some((o, ch)) = out.as_mut() {
                let (l, r) = match (&buf, playing) {
                    (Some(b), true) => {
                        let j = pos as usize * CHANNELS as usize;
                        (b.get(j).copied().unwrap_or(0), b.get(j + 1).copied().unwrap_or(0))
                    }
                    _ => (0, 0),
                };
                let (l, r) = (l as f32 / 32768.0, r as f32 / 32768.0);
                let frame = &mut o[i * *ch..(i + 1) * *ch];
                for (c, s) in frame.iter_mut().enumerate() {
                    *s = match c {
                        0 => l,
                        1 => r,
                        _ => 0.0,
                    };
                }
                if *ch == 1 {
                    frame[0] = (l + r) * 0.5;
                }
            }
            if playing {
                pos += 1;
            }
        }
        // L'interface a pu se déplacer pendant ce temps : on ne remplace la position que si elle n'a pas bougé.
        let _ = self.pos.compare_exchange(start, pos, Ordering::Relaxed, Ordering::Relaxed);
        if was_playing && !playing {
            self.playing.store(false, Ordering::Relaxed);
        }
    }
}

pub struct Player {
    pub shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    has_buffer: bool,
    last: std::time::Instant,
    carry: f64,
}

impl Player {
    pub fn new() -> Player {
        let host = cpal::default_host();
        let device = host.default_output_device();
        let config = device.as_ref().and_then(|d| d.default_output_config().ok());
        let rate = config.as_ref().map(|c| c.sample_rate().0).unwrap_or(48000);
        let shared = Arc::new(Shared {
            rate,
            pos: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            len: AtomicU64::new(0),
            listen_end: AtomicU64::new(NONE),
            buffer: Mutex::new(None),
            skips: Mutex::new(Vec::new()),
        });
        let stream = (|| {
            let device = device?;
            let config = config?;
            let ch = config.channels() as usize;
            let sh = shared.clone();
            let err = |e| eprintln!("audio : {e}");
            let cfg: cpal::StreamConfig = config.clone().into();
            let s = match config.sample_format() {
                cpal::SampleFormat::F32 => device
                    .build_output_stream(&cfg, move |out: &mut [f32], _| sh.render(out.len() / ch, Some((out, ch))), err, None)
                    .ok()?,
                cpal::SampleFormat::I16 => {
                    let mut tmp = Vec::new();
                    device
                        .build_output_stream(
                            &cfg,
                            move |out: &mut [i16], _| {
                                tmp.resize(out.len(), 0.0f32);
                                sh.render(out.len() / ch, Some((&mut tmp, ch)));
                                for (o, t) in out.iter_mut().zip(&tmp) {
                                    *o = (t * 32767.0) as i16;
                                }
                            },
                            err,
                            None,
                        )
                        .ok()?
                }
                _ => return None,
            };
            s.play().ok()?;
            Some(s)
        })();
        Player { shared, stream, has_buffer: false, last: std::time::Instant::now(), carry: 0.0 }
    }

    pub fn rate(&self) -> u32 {
        self.shared.rate
    }

    /// Change de fichier : durée et son (si déjà décodé).
    pub fn load(&mut self, duration: f64, buffer: Option<Arc<Vec<i16>>>) {
        self.pause();
        self.has_buffer = buffer.is_some();
        *self.shared.buffer.lock().unwrap() = buffer;
        self.shared.len.store((duration * self.rate() as f64) as u64, Ordering::Relaxed);
        self.shared.pos.store(0, Ordering::Relaxed);
    }

    pub fn set_buffer(&mut self, buffer: Arc<Vec<i16>>) {
        self.has_buffer = true;
        *self.shared.buffer.lock().unwrap() = Some(buffer);
    }

    pub fn has_buffer(&self) -> bool {
        self.has_buffer
    }

    pub fn time(&self) -> f64 {
        self.shared.pos.load(Ordering::Relaxed) as f64 / self.rate() as f64
    }

    pub fn seek(&mut self, t: f64) {
        self.shared.listen_end.store(NONE, Ordering::Relaxed);
        let s = (t.max(0.0) * self.rate() as f64) as u64;
        self.shared.pos.store(s.min(self.shared.len.load(Ordering::Relaxed)), Ordering::Relaxed);
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn play(&mut self) {
        if self.shared.pos.load(Ordering::Relaxed) >= self.shared.len.load(Ordering::Relaxed) {
            self.shared.pos.store(0, Ordering::Relaxed);
        }
        self.last = std::time::Instant::now();
        self.carry = 0.0;
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn pause(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn toggle(&mut self) {
        if self.playing() {
            self.pause()
        } else {
            self.play()
        }
    }

    /// Écoute une coupe : de `from` à `to` sans rien sauter.
    pub fn listen(&mut self, from: f64, to: f64) {
        self.seek(from);
        self.shared.listen_end.store((to * self.rate() as f64) as u64, Ordering::Relaxed);
        self.play();
    }

    pub fn set_skips(&self, skips: &[(f64, f64)]) {
        let r = self.rate() as f64;
        *self.shared.skips.lock().unwrap() = skips.iter().map(|&(s, e)| ((s * r) as u64, (e * r) as u64)).collect();
    }

    /// Sans carte son ou sans son décodé : l'interface fait avancer l'horloge elle-même.
    pub fn tick(&mut self) {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.25);
        self.last = now;
        if self.playing() && (self.stream.is_none() || !self.has_buffer) {
            self.carry += dt * self.rate() as f64;
            let n = self.carry.floor();
            self.carry -= n;
            self.shared.render(n as usize, None);
        }
    }
}

// --------------------------------------------------------------------------- vidéo

pub struct Frame {
    pub gen: u64,
    pub t: f64,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

enum Cmd {
    Start { gen: u64, path: PathBuf, t: f64, w: u32, h: u32, fps: f64 },
    Stop,
}

/// Décodeur vidéo en tâche de fond : un ffmpeg qui sort des images RGBA à partir d'un instant.
pub struct Video {
    tx: Sender<Cmd>,
    rx: Receiver<Frame>,
    gen: u64,
    pub start_t: f64,
    pending: Option<Frame>,
    pub last_t: f64,
    pub active: bool,
}

impl Video {
    pub fn new() -> Video {
        let (tx, crx) = channel::<Cmd>();
        let (ftx, rx) = sync_channel::<Frame>(6);
        std::thread::spawn(move || video_thread(crx, ftx));
        Video { tx, rx, gen: 0, start_t: 0.0, pending: None, last_t: -1.0, active: false }
    }

    pub fn start(&mut self, info: &MediaInfo, t: f64) {
        self.gen += 1;
        self.pending = None;
        self.start_t = t;
        self.last_t = -1.0;
        self.active = true;
        let (w, h) = scaled_size(info, 960);
        let _ = self.tx.send(Cmd::Start { gen: self.gen, path: info.path.clone(), t, w, h, fps: info.fps.as_f64() });
    }

    pub fn stop(&mut self) {
        self.gen += 1;
        self.pending = None;
        self.active = false;
        let _ = self.tx.send(Cmd::Stop);
    }

    /// Dernière image dont l'instant est ≤ `t` (si `first`, la toute première suffit).
    pub fn take(&mut self, t: f64, first: bool) -> Option<Frame> {
        let mut best = None;
        loop {
            let f = match self.pending.take() {
                Some(f) => f,
                None => match self.rx.try_recv() {
                    Ok(f) => f,
                    Err(_) => break,
                },
            };
            if f.gen != self.gen {
                continue;
            }
            if first || f.t <= t + 0.001 {
                self.last_t = f.t;
                best = Some(f);
                if first {
                    break;
                }
            } else {
                self.pending = Some(f);
                break;
            }
        }
        best
    }
}

fn video_thread(rx: Receiver<Cmd>, tx: SyncSender<Frame>) {
    let mut child: Option<Child> = None;
    let mut cmd = rx.recv().ok();
    while let Some(c) = cmd.take() {
        if let Some(mut ch) = child.take() {
            let _ = ch.kill();
            let _ = ch.wait();
        }
        let Cmd::Start { gen, path, t, w, h, fps } = c else {
            cmd = rx.recv().ok();
            continue;
        };
        let spawned = tool("ffmpeg")
            .args(["-nostdin", "-v", "error", "-ss", &format!("{t:.3}"), "-i"])
            .arg(&path)
            .args(["-an", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut ch) = spawned else {
            cmd = rx.recv().ok();
            continue;
        };
        let mut out = ch.stdout.take().unwrap();
        child = Some(ch);
        let size = (w * h * 4) as usize;
        let mut n = 0u64;
        'frames: loop {
            let mut buf = vec![0u8; size];
            if out.read_exact(&mut buf).is_err() {
                break;
            }
            let mut f = Frame { gen, t: t + n as f64 / fps.max(1.0), w, h, rgba: buf };
            n += 1;
            loop {
                match rx.try_recv() {
                    Ok(c) => {
                        cmd = Some(c);
                        break 'frames;
                    }
                    Err(TryRecvError::Disconnected) => return,
                    Err(TryRecvError::Empty) => {}
                }
                match tx.try_send(f) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        f = back;
                        std::thread::sleep(std::time::Duration::from_millis(4));
                    }
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
        }
        if cmd.is_none() {
            cmd = rx.recv().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(len: u64, skips: Vec<(u64, u64)>) -> Shared {
        Shared {
            rate: 100,
            pos: AtomicU64::new(0),
            playing: AtomicBool::new(true),
            len: AtomicU64::new(len),
            listen_end: AtomicU64::new(NONE),
            buffer: Mutex::new(None),
            skips: Mutex::new(skips),
        }
    }

    #[test]
    fn preview_skips_cuts_and_stops_at_end() {
        let s = shared(1000, vec![(100, 300), (500, 600)]);
        s.render(150, None);
        // 150 échantillons joués : 0..100 puis 300..350
        assert_eq!(s.pos.load(Ordering::Relaxed), 350);
        s.render(200, None);
        assert_eq!(s.pos.load(Ordering::Relaxed), 650);
        s.render(1000, None);
        assert_eq!(s.pos.load(Ordering::Relaxed), 1000);
        assert!(!s.playing.load(Ordering::Relaxed));
    }

    #[test]
    fn listening_ignores_cuts_and_pauses_at_end() {
        let s = shared(1000, vec![(100, 300)]);
        s.pos.store(50, Ordering::Relaxed);
        s.listen_end.store(250, Ordering::Relaxed);
        s.render(400, None);
        assert_eq!(s.pos.load(Ordering::Relaxed), 250);
        assert!(!s.playing.load(Ordering::Relaxed));
        assert_eq!(s.listen_end.load(Ordering::Relaxed), NONE);
    }
}
