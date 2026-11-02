"""Lecture des fichiers médias avec ffmpeg / ffprobe."""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16_000

# Cadences standard : les vidéos de téléphone (cadence variable) sont ramenées à la plus proche.
STANDARD_FPS = [
    Fraction(24000, 1001), Fraction(24), Fraction(25), Fraction(30000, 1001), Fraction(30),
    Fraction(48), Fraction(50), Fraction(60000, 1001), Fraction(60), Fraction(100),
    Fraction(120000, 1001), Fraction(120),
]


@dataclass
class MediaInfo:
    path: Path
    duration: float
    has_video: bool
    fps: Fraction
    width: int
    height: int
    has_audio: bool
    audio_channels: int
    audio_rate: int
    video_codec: str = ""
    pix_fmt: str = ""

    @property
    def browser_playable(self) -> bool:
        """Le navigateur (WebView2/Chrome) sait-il lire la vidéo telle quelle ?"""
        if not self.has_video:
            return True
        return self.video_codec in {"h264", "vp9", "av1"} and self.pix_fmt in {"yuv420p", "yuvj420p", ""}

    @property
    def total_frames(self) -> int:
        return int(round(self.duration * self.fps))


def require_ffmpeg() -> None:
    for tool in ("ffmpeg", "ffprobe"):
        if shutil.which(tool) is None:
            raise RuntimeError(f"{tool} est introuvable dans le PATH. Installe ffmpeg : https://ffmpeg.org/download.html")


def snap_fps(fps: Fraction) -> Fraction:
    best = min(STANDARD_FPS, key=lambda f: abs(float(f) - float(fps)))
    return best if abs(float(best) - float(fps)) / float(best) < 0.01 else fps


def probe(path: str | Path) -> MediaInfo:
    path = Path(path).resolve()
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json", "-show_format", "-show_streams", str(path)],
        capture_output=True, text=True, encoding="utf-8", check=True,
    )
    data = json.loads(out.stdout)
    streams = data.get("streams", [])
    video = next(
        (s for s in streams if s["codec_type"] == "video" and not s.get("disposition", {}).get("attached_pic")),
        None,
    )
    audio = next((s for s in streams if s["codec_type"] == "audio"), None)

    fps = Fraction(25)
    if video:
        raw = video.get("avg_frame_rate") or video.get("r_frame_rate") or "25/1"
        if raw in ("0/0", "0/1"):
            raw = video.get("r_frame_rate", "25/1")
        fps = snap_fps(Fraction(raw))

    duration = float(data.get("format", {}).get("duration") or (audio or video or {}).get("duration") or 0)
    return MediaInfo(
        path=path,
        duration=duration,
        has_video=video is not None,
        fps=fps,
        width=int(video["width"]) if video else 1920,
        height=int(video["height"]) if video else 1080,
        has_audio=audio is not None,
        audio_channels=int(audio.get("channels", 2)) if audio else 0,
        audio_rate=int(audio.get("sample_rate", 48000)) if audio else 48000,
        video_codec=video.get("codec_name", "") if video else "",
        pix_fmt=video.get("pix_fmt", "") if video else "",
    )


def make_proxy(info: MediaInfo, out: Path) -> Path:
    """Proxy H.264 540p lisible par le navigateur (pour ProRes, HEVC 10 bits…)."""
    if out.exists() and out.stat().st_size > 0:
        return out
    tmp = out.with_suffix(".part.mp4")
    enc = ["-c:v", "h264_nvenc", "-preset", "p4", "-cq", "28"]
    base = ["ffmpeg", "-nostdin", "-y", "-v", "error", "-i", str(info.path), "-vf", "scale=-2:540,format=yuv420p"]
    tail = ["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart", str(tmp)]
    if subprocess.run(base + enc + tail).returncode != 0:
        subprocess.run(base + ["-c:v", "libx264", "-preset", "veryfast", "-crf", "28"] + tail, check=True)
    tmp.replace(out)
    return out


def make_thumbnail(info: MediaInfo, out: Path, width: int = 240) -> Path | None:
    """Vignette JPEG prise à 10 % de la vidéo (au plus à 3 s du début)."""
    if not info.has_video:
        return None
    if out.exists() and out.stat().st_size > 0:
        return out
    t = min(3.0, info.duration * 0.1)
    cmd = ["ffmpeg", "-nostdin", "-y", "-v", "error", "-ss", f"{t:.2f}", "-i", str(info.path),
           "-frames:v", "1", "-vf", f"scale={width}:-2", "-q:v", "4", str(out)]
    return out if subprocess.run(cmd).returncode == 0 and out.exists() else None


def load_audio(path: str | Path, sr: int = SAMPLE_RATE) -> np.ndarray:
    """Décode la piste audio en mono float32 à `sr` Hz."""
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-i", str(path), "-vn", "-ac", "1", "-ar", str(sr), "-f", "f32le", "-"]
    out = subprocess.run(cmd, capture_output=True, check=True)
    return np.frombuffer(out.stdout, dtype=np.float32).copy()
