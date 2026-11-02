"""Exports : FCPXML (DaVinci Resolve, Final Cut Pro), XML Premiere Pro, MP4 monté, JSON."""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import tempfile
from dataclasses import asdict
from fractions import Fraction
from pathlib import Path
from urllib.parse import quote
from xml.etree import ElementTree as ET

from .cutter import CutResult
from .media import MediaInfo

Frames = list[tuple[int, int]]


def _pretty(root: ET.Element, doctype: str) -> bytes:
    ET.indent(root, space="  ")
    body = ET.tostring(root, encoding="unicode")
    return f'<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE {doctype}>\n{body}\n'.encode("utf-8")


# --------------------------------------------------------------------------- FCPXML


def _ftime(frames: int, fps: Fraction) -> str:
    """Temps rationnel FCPXML, multiple exact de la durée d'une image."""
    if frames == 0:
        return "0s"
    return f"{frames * fps.denominator}/{fps.numerator}s" if fps.denominator != 1 or frames % fps.numerator else f"{frames // fps.numerator}s"


def write_fcpxml(info: MediaInfo, frames: Frames, out: Path, name: str) -> None:
    fps = info.fps
    frame_dur = f"{fps.denominator}/{fps.numerator}s"
    total = info.total_frames
    tl_len = sum(b - a for a, b in frames)

    root = ET.Element("fcpxml", version="1.9")
    res = ET.SubElement(root, "resources")
    ET.SubElement(res, "format", id="r1", frameDuration=frame_dur, width=str(info.width), height=str(info.height))
    asset = ET.SubElement(
        res, "asset", id="r2", name=info.path.stem, start="0s", duration=_ftime(total, fps),
        hasVideo="1" if info.has_video else "0", hasAudio="1" if info.has_audio else "0", format="r1",
        audioSources="1", audioChannels=str(max(info.audio_channels, 1)), audioRate=str(info.audio_rate),
    )
    ET.SubElement(asset, "media-rep", kind="original-media", src=info.path.as_uri())

    event = ET.SubElement(ET.SubElement(root, "library"), "event", name="CrispTrim")
    project = ET.SubElement(event, "project", name=name)
    seq = ET.SubElement(
        project, "sequence", format="r1", duration=_ftime(tl_len, fps), tcStart="0s", tcFormat="NDF",
        audioLayout="stereo" if info.audio_channels >= 2 else "mono",
        audioRate="48k" if info.audio_rate >= 48000 else "44.1k",
    )
    spine = ET.SubElement(seq, "spine")
    offset = 0
    for a, b in frames:
        ET.SubElement(
            spine, "asset-clip", ref="r2", name=info.path.stem, offset=_ftime(offset, fps),
            start=_ftime(a, fps), duration=_ftime(b - a, fps), format="r1", tcFormat="NDF",
        )
        offset += b - a
    out.write_bytes(_pretty(root, "fcpxml"))


# --------------------------------------------------------------------------- Premiere (xmeml)


def _rate(parent: ET.Element, fps: Fraction) -> None:
    rate = ET.SubElement(parent, "rate")
    ET.SubElement(rate, "timebase").text = str(round(fps))
    ET.SubElement(rate, "ntsc").text = "TRUE" if fps.denominator == 1001 else "FALSE"


def _text(parent: ET.Element, tag: str, value) -> ET.Element:
    el = ET.SubElement(parent, tag)
    el.text = str(value)
    return el


def _pathurl(path: Path) -> str:
    # Format attendu par Premiere : file://localhost/C%3a/dossier/fichier.mp4
    return "file://localhost/" + quote(path.as_posix().lstrip("/"), safe="/")


def write_premiere_xml(info: MediaInfo, frames: Frames, out: Path, name: str) -> None:
    fps, total = info.fps, info.total_frames
    tl_len = sum(b - a for a, b in frames)
    channels = min(max(info.audio_channels, 1), 2) if info.has_audio else 0

    root = ET.Element("xmeml", version="4")
    seq = ET.SubElement(root, "sequence", id="sequence-1")
    _text(seq, "name", name)
    _text(seq, "duration", tl_len)
    _rate(seq, fps)
    media = ET.SubElement(seq, "media")

    def file_el(parent: ET.Element, first: bool) -> None:
        f = ET.SubElement(parent, "file", id="file-1")
        if not first:
            return
        _text(f, "name", info.path.name)
        _text(f, "pathurl", _pathurl(info.path))
        _rate(f, fps)
        _text(f, "duration", total)
        fm = ET.SubElement(f, "media")
        if info.has_video:
            sc = ET.SubElement(ET.SubElement(fm, "video"), "samplecharacteristics")
            _rate(sc, fps)
            _text(sc, "width", info.width)
            _text(sc, "height", info.height)
        if info.has_audio:
            au = ET.SubElement(fm, "audio")
            sc = ET.SubElement(au, "samplecharacteristics")
            _text(sc, "depth", 16)
            _text(sc, "samplerate", info.audio_rate)
            _text(au, "channelcount", channels)

    kinds = (["video"] if info.has_video else []) + ["audio"] * channels

    def clip_id(kind_idx: int, seg_idx: int) -> str:
        return f"clipitem-{kind_idx + 1}-{seg_idx + 1}"

    first_file = True
    tracks: dict[int, ET.Element] = {}
    video_el = ET.SubElement(media, "video") if info.has_video else None
    if video_el is not None:
        fmt = ET.SubElement(ET.SubElement(video_el, "format"), "samplecharacteristics")
        _rate(fmt, fps)
        _text(fmt, "width", info.width)
        _text(fmt, "height", info.height)
        _text(fmt, "pixelaspectratio", "square")
    audio_el = ET.SubElement(media, "audio") if channels else None
    if audio_el is not None:
        _text(audio_el, "numOutputChannels", 2)

    for k, kind in enumerate(kinds):
        parent = video_el if kind == "video" else audio_el
        tracks[k] = ET.SubElement(parent, "track")

    tl = 0
    for i, (a, b) in enumerate(frames):
        for k, kind in enumerate(kinds):
            item = ET.SubElement(tracks[k], "clipitem", id=clip_id(k, i))
            _text(item, "name", info.path.name)
            _text(item, "enabled", "TRUE")
            _text(item, "duration", total)
            _rate(item, fps)
            _text(item, "start", tl)
            _text(item, "end", tl + b - a)
            _text(item, "in", a)
            _text(item, "out", b)
            file_el(item, first_file)
            first_file = False
            if kind == "audio":
                st = ET.SubElement(item, "sourcetrack")
                _text(st, "mediatype", "audio")
                _text(st, "trackindex", k - (1 if info.has_video else 0) + 1)
            for k2, kind2 in enumerate(kinds):
                link = ET.SubElement(item, "link")
                _text(link, "linkclipref", clip_id(k2, i))
                _text(link, "mediatype", kind2)
                _text(link, "trackindex", k2 + 1 if kind2 == "video" else k2 - (1 if info.has_video else 0) + 1)
                _text(link, "clipindex", i + 1)
        tl += b - a
    out.write_bytes(_pretty(root, "xmeml"))


# --------------------------------------------------------------------------- Vegas Pro (EDL texte)

VEGAS_COLUMNS = (
    "ID", "Track", "StartTime", "Length", "PlayRate", "Locked", "Normalized", "StretchMethod", "Looped",
    "OnRuler", "MediaType", "FileName", "Stream", "StreamStart", "StreamLength", "FadeTimeIn", "FadeTimeOut",
    "SustainGain", "CurveIn", "GainIn", "CurveOut", "GainOut", "Layer", "Color", "CurveInR", "CurveOutR",
    "PlayPitch", "LockPitch",
)


def write_vegas_edl(info: MediaInfo, frames: Frames, out: Path) -> None:
    """EDL texte de Vegas Pro (Fichier > Importer > Fichier texte EDL). Temps en millisecondes."""
    fps = info.fps
    ms = lambda f: float(Fraction(f) / fps) * 1000  # noqa: E731
    kinds = (["VIDEO"] if info.has_video else []) + (["AUDIO"] if info.has_audio else [])
    lines = [";".join(f'"{c}"' for c in VEGAS_COLUMNS)]
    event_id, tl = 1, 0
    for a, b in frames:
        start, length, src = ms(tl), ms(b - a), ms(a)
        for track, kind in enumerate(kinds, start=1):
            row = [
                event_id, track, f"{start:.4f}", f"{length:.4f}", "1.000000", "FALSE", "FALSE", 0, "FALSE",
                "FALSE", kind, f'"{info.path}"', 0, f"{src:.4f}", f"{length:.4f}", "0.0000", "0.0000",
                "1.000000", 4, "0.000000", 4, "0.000000", 0, -1, 4, 4, "0.000000", "FALSE",
            ]
            lines.append(";".join(str(v) for v in row))
            event_id += 1
        tl += b - a
    # Vegas est un logiciel Windows : fins de ligne CRLF, encodage UTF-8 avec BOM pour les chemins accentués.
    out.write_text("\r\n".join(lines) + "\r\n", encoding="utf-8-sig", newline="")


# --------------------------------------------------------------------------- MP4 monté


X264 = ["-c:v", "libx264", "-preset", "medium", "-crf", "18"]


def _video_encoder() -> list[str]:
    try:
        enc = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    except OSError:
        enc = ""
    # NVENC peut être compilé dans ffmpeg sans carte NVIDIA : render_mp4 se replie alors sur libx264.
    if "h264_nvenc" in enc:
        return ["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "19", "-b:v", "0"]
    return X264


def _ffmpeg_major() -> int:
    try:
        first = subprocess.run(["ffmpeg", "-version"], capture_output=True, text=True).stdout.split("\n", 1)[0]
        m = re.search(r"version n?(\d+)\.", first)
        return int(m.group(1)) if m else 99  # versions git (« N-12345-… ») : récentes
    except OSError:
        return 99


def render_mp4(info: MediaInfo, frames: Frames, out: Path) -> None:
    """Rend une vidéo nettoyée (réencodage) via ffmpeg trim/concat."""
    fps = info.fps
    parts, labels = [], []
    for i, (a, b) in enumerate(frames):
        s, e = float(Fraction(a) / fps), float(Fraction(b) / fps)
        if info.has_video:
            parts.append(f"[0:v]trim=start={s:.6f}:end={e:.6f},setpts=PTS-STARTPTS[v{i}];")
            labels.append(f"[v{i}]")
        if info.has_audio:
            parts.append(f"[0:a]atrim=start={s:.6f}:end={e:.6f},asetpts=PTS-STARTPTS[a{i}];")
            labels.append(f"[a{i}]")
    v, a = int(info.has_video), int(info.has_audio)
    outs = ("[outv]" if v else "") + ("[outa]" if a else "")
    graph = "".join(parts) + "".join(labels) + f"concat=n={len(frames)}:v={v}:a={a}{outs}"

    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as fh:
        fh.write(graph)
        script = fh.name
    # ffmpeg ≥ 7 lit le graphe avec « -/filter_complex » ; « -filter_complex_script » a disparu en v8.
    script_opt = "-/filter_complex" if _ffmpeg_major() >= 7 else "-filter_complex_script"

    def command(encoder: list[str]) -> list[str]:
        cmd = ["ffmpeg", "-nostdin", "-y", "-v", "error", "-stats", "-i", str(info.path), script_opt, script]
        if v:
            cmd += ["-map", "[outv]", *encoder, "-pix_fmt", "yuv420p", "-r", f"{fps.numerator}/{fps.denominator}"]
        if a:
            cmd += ["-map", "[outa]", "-c:a", "aac", "-b:a", "192k"]
        return cmd + ["-movflags", "+faststart", str(out)]

    try:
        encoder = _video_encoder() if v else X264
        if subprocess.run(command(encoder)).returncode != 0:
            if encoder == X264:
                raise RuntimeError("ffmpeg n'a pas pu rendre la vidéo montée.")
            subprocess.run(command(X264), check=True)  # encodeur matériel indisponible
    finally:
        Path(script).unlink(missing_ok=True)


# --------------------------------------------------------------------------- JSON


def write_json(info: MediaInfo, result: CutResult, words, threshold_db: float, out: Path) -> None:
    data = {
        "source": str(info.path),
        "fps": f"{info.fps.numerator}/{info.fps.denominator}",
        "threshold_db": round(threshold_db, 1),
        "stats": result.stats(),
        "regions": [{**asdict(r), "start": round(r.start, 3), "end": round(r.end, 3)} for r in result.regions],
        "words": [asdict(w) for w in words] if words else [],
    }
    out.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")


def have_ffmpeg() -> bool:
    return shutil.which("ffmpeg") is not None
