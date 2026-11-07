//! Lecture des fichiers médias avec ffmpeg / ffprobe.

use super::Fps;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SAMPLE_RATE: u32 = 16_000;

pub const MEDIA_EXT: [&str; 12] = ["mp4", "mov", "mkv", "mxf", "avi", "webm", "m4v", "wav", "mp3", "m4a", "aac", "flac"];

/// Commande sans fenêtre de console qui clignote (Windows, CREATE_NO_WINDOW = 0x08000000 comme le lanceur).
/// Comme le lanceur (`tool_env` met `<dossier CrispTrim>\bin` en tête du PATH), le FFmpeg installé
/// par CrispTrim passe avant celui du système.
pub fn tool(name: &str) -> Command {
    let bundled = crate::data_dir().join("bin").join(format!("{name}.exe"));
    let program = if matches!(name, "ffmpeg" | "ffprobe") && bundled.is_file() { bundled } else { PathBuf::from(name) };
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // gardés pour le diagnostic (codec, format de pixels)
pub struct MediaInfo {
    pub path: PathBuf,
    pub duration: f64,
    pub has_video: bool,
    pub fps: Fps,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
    pub audio_channels: u32,
    pub audio_rate: u32,
    pub video_codec: String,
    pub pix_fmt: String,
}

impl MediaInfo {
    pub fn total_frames(&self) -> i64 {
        super::silence::pyround(self.duration * self.fps.as_f64()) as i64
    }
}

pub fn is_media(p: &Path) -> bool {
    p.extension().map(|e| MEDIA_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

pub fn require_ffmpeg() -> Result<(), String> {
    for t in ["ffmpeg", "ffprobe"] {
        let ok = tool(t).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return Err(format!("{t} est introuvable dans le PATH. Installe ffmpeg : https://ffmpeg.org/download.html"));
        }
    }
    Ok(())
}

fn num(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

pub fn probe(path: &Path) -> Result<MediaInfo, String> {
    let path = super::transcribe::resolved(path);
    let out = tool("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(&path)
        .output()
        .map_err(|e| format!("ffprobe : {e}"))?;
    if !out.status.success() {
        return Err(format!("ffprobe n'a pas pu lire le fichier : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let data: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let empty = vec![];
    let streams = data["streams"].as_array().unwrap_or(&empty);
    let video = streams.iter().find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 0);
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");

    let mut fps = Fps::new(25, 1);
    if let Some(v) = video {
        let mut raw = v["avg_frame_rate"].as_str().or(v["r_frame_rate"].as_str()).unwrap_or("25/1").to_string();
        if raw == "0/0" || raw == "0/1" {
            raw = v["r_frame_rate"].as_str().unwrap_or("25/1").to_string();
        }
        fps = Fps::parse(&raw).unwrap_or(Fps::new(25, 1)).snap();
    }
    let duration = num(&data["format"]["duration"])
        .or_else(|| audio.and_then(|a| num(&a["duration"])))
        .or_else(|| video.and_then(|v| num(&v["duration"])))
        .unwrap_or(0.0);
    Ok(MediaInfo {
        path,
        duration,
        has_video: video.is_some(),
        fps,
        width: video.and_then(|v| v["width"].as_u64()).unwrap_or(1920) as u32,
        height: video.and_then(|v| v["height"].as_u64()).unwrap_or(1080) as u32,
        has_audio: audio.is_some(),
        audio_channels: audio.map(|a| a["channels"].as_u64().unwrap_or(2) as u32).unwrap_or(0),
        audio_rate: audio.map(|a| num(&a["sample_rate"]).unwrap_or(48000.0) as u32).unwrap_or(48000),
        video_codec: video.and_then(|v| v["codec_name"].as_str()).unwrap_or("").to_string(),
        pix_fmt: video.and_then(|v| v["pix_fmt"].as_str()).unwrap_or("").to_string(),
    })
}

/// Décode la piste audio en mono f32 à `sr` Hz.
pub fn load_audio(path: &Path, sr: u32) -> Result<Vec<f32>, String> {
    let bytes = decode_audio(path, sr, 1, "f32le")?;
    Ok(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// Décode la piste audio pour la lecture : i16 entrelacé, `channels` canaux à `sr` Hz.
pub fn load_playback_audio(path: &Path, sr: u32, channels: u16) -> Result<Vec<i16>, String> {
    let bytes = decode_audio(path, sr, channels, "s16le")?;
    Ok(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
}

fn decode_audio(path: &Path, sr: u32, channels: u16, fmt: &str) -> Result<Vec<u8>, String> {
    let out = tool("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", &channels.to_string(), "-ar", &sr.to_string(), "-f", fmt, "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg : {e}"))?;
    if !out.status.success() {
        return Err(format!("ffmpeg n'a pas pu décoder l'audio : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// Taille d'image réduite (largeur donnée, hauteur paire).
pub fn scaled_size(info: &MediaInfo, width: u32) -> (u32, u32) {
    let w = width.min(info.width.max(2)) & !1;
    let h = ((info.height as f64 * w as f64 / info.width.max(1) as f64 / 2.0).round() as u32 * 2).max(2);
    (w, h)
}

/// Une image RGBA à l'instant `t`.
pub fn frame_at(info: &MediaInfo, t: f64, width: u32) -> Option<(u32, u32, Vec<u8>)> {
    if !info.has_video {
        return None;
    }
    let (w, h) = scaled_size(info, width);
    let mut child = tool("ffmpeg")
        .args(["-nostdin", "-v", "error", "-ss", &format!("{t:.3}"), "-i"])
        .arg(&info.path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let ok = child.stdout.as_mut()?.read_exact(&mut buf).is_ok();
    let _ = child.wait();
    ok.then_some((w, h, buf))
}

/// Vignette prise à 10 % de la vidéo (au plus à 3 s du début).
pub fn thumbnail(info: &MediaInfo) -> Option<(u32, u32, Vec<u8>)> {
    frame_at(info, (info.duration * 0.1).min(3.0), 240)
}
