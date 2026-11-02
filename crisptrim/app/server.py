"""Serveur local de CrispTrim : API du moteur + interface web.

Lancement : uv run crisptrim-app   (ouvre http://127.0.0.1:8765)
"""

from __future__ import annotations

import base64
import os
import queue
import subprocess
import sys
import threading
import traceback
import uuid
import webbrowser
from dataclasses import asdict, dataclass, field
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from .. import REPO_URL, __author__, __version__
from ..cutter import CutParams, compute_cuts, to_frames
from ..export import render_mp4, write_fcpxml, write_premiere_xml, write_vegas_edl
from ..intervals import merge
from ..media import SAMPLE_RATE, MediaInfo, load_audio, make_proxy, make_thumbnail, probe, require_ffmpeg
from ..silence import FRAME, SilenceParams, detect_speech, frame_db
from ..text import write_srt, write_txt
from ..transcribe import Transcriber, Word
from ..updates import check as check_update

STATIC = Path(__file__).parent / "static"
DATA_DIR = Path(os.environ.get("LOCALAPPDATA", Path.home() / ".cache")) / "CrispTrim"
CACHE_DIR = DATA_DIR / "cache"


# --------------------------------------------------------------------------- état


class Settings(BaseModel):
    threshold_db: float | None = None
    min_silence: float = 0.5
    margin: float = 0.15
    min_keep: float = 0.25
    cut_silences: bool = True
    cut_fillers: bool = True
    cut_noises: bool = True


@dataclass
class Job:
    id: str
    path: Path
    stage: str = "attente"         # attente | analyse | transcription | aperçu | prêt | erreur
    error: str = ""
    info: MediaInfo | None = None
    audio: np.ndarray | None = None
    words: list[Word] | None = None
    peaks: str = ""                # dB par 10 ms, encodés en base64 (uint8 = -dB)
    threshold: float = 0.0
    regions: list[dict] = field(default_factory=list)
    stats: dict = field(default_factory=dict)
    settings: Settings = field(default_factory=Settings)
    media_path: Path | None = None
    thumb_path: Path | None = None
    export: dict = field(default_factory=lambda: {"state": "", "files": [], "error": ""})
    version: int = 0

    def public(self, full: bool = False, peaks: bool = False) -> dict:
        d = {
            "id": self.id, "name": self.path.name, "path": str(self.path), "stage": self.stage,
            "error": self.error, "stats": self.stats, "export": self.export, "version": self.version,
            "settings": self.settings.model_dump(), "has_words": self.words is not None,
            "media_ready": self.media_path is not None, "thumb": self.thumb_path is not None,
            "kind": "video" if self.info and self.info.has_video else "audio" if self.info else "",
        }
        if self.info:
            d["info"] = {
                "duration": self.info.duration, "fps": float(self.info.fps), "has_video": self.info.has_video,
                "width": self.info.width, "height": self.info.height,
            }
        if full:
            d.update(regions=self.regions, threshold=self.threshold,
                     words=[asdict(w) for w in self.words or []], frame=FRAME)
        if peaks:
            d["peaks"] = self.peaks
        return d


class State:
    def __init__(self) -> None:
        self.jobs: dict[str, Job] = {}
        self.model = "large"
        self.language = "fr"
        self._transcriber: Transcriber | None = None
        self.work: queue.Queue[Job] = queue.Queue()
        threading.Thread(target=self._worker, daemon=True).start()

    def transcriber(self) -> Transcriber:
        t = self._transcriber
        if t is None or t.model_name != self.model or t.language != self.language:
            t = self._transcriber = Transcriber(self.model, self.language)
        return t

    # Un seul fil de travail : le GPU traite un fichier à la fois.
    def _worker(self) -> None:
        while True:
            job = self.work.get()
            try:
                self._process(job)
            except Exception as exc:  # noqa: BLE001
                traceback.print_exc()
                job.stage, job.error = "erreur", str(exc)
                job.version += 1

    def _process(self, job: Job) -> None:
        job.stage = "analyse"
        job.version += 1
        info = job.info = probe(job.path)
        if not info.has_audio:
            raise RuntimeError("Ce fichier n'a pas de piste audio.")
        job.thumb_path = make_thumbnail(info, CACHE_DIR / f"{job.path.stem}.{job.id}.thumb.jpg")
        job.version += 1
        job.audio = load_audio(info.path)
        db = frame_db(job.audio, SAMPLE_RATE)
        job.peaks = base64.b64encode(np.clip(-db, 0, 255).astype(np.uint8).tobytes()).decode()
        recut(job)  # premier résultat (silences seuls) visible tout de suite

        if not info.browser_playable:
            job.stage = "aperçu"
            job.version += 1
            job.media_path = make_proxy(info, CACHE_DIR / f"{job.path.stem}.{job.id}.proxy.mp4")
        else:
            job.media_path = info.path

        if job.settings.cut_fillers:
            ensure_words(job)
        job.stage = "prêt"
        recut(job)

    def add(self, path: Path, settings: Settings | None = None) -> Job:
        job = Job(id=uuid.uuid4().hex[:8], path=path, settings=settings or Settings())
        self.jobs[job.id] = job
        self.work.put(job)
        return job


def ensure_words(job: Job) -> None:
    if job.words is None and job.audio is not None:
        job.stage = "transcription"
        job.version += 1
        job.words = STATE.transcriber().transcribe(job.audio, job.path, cache_dir=CACHE_DIR)


def recut(job: Job) -> None:
    s = job.settings
    if job.info is None or job.audio is None:
        return
    duration = job.info.duration
    speech, thr = detect_speech(
        job.audio, SAMPLE_RATE, SilenceParams(threshold_db=s.threshold_db, min_silence=s.min_silence)
    )
    if not s.cut_silences:
        speech = [(0.0, duration)]
    params = CutParams(margin=s.margin, min_keep=s.min_keep, cut_fillers=s.cut_fillers, cut_noises=s.cut_noises)
    res = compute_cuts(duration, speech, job.words, params)
    job.threshold = thr
    job.regions = [
        {"start": round(r.start, 3), "end": round(r.end, 3), "kind": r.kind, "text": r.text} for r in res.regions
    ]
    job.stats = res.stats()
    job.version += 1


STATE = State()


# --------------------------------------------------------------------------- API

app = FastAPI(title="CrispTrim", version=__version__)


class AddBody(BaseModel):
    paths: list[str]
    settings: Settings | None = None


class ExportBody(BaseModel):
    formats: list[str]
    keep: list[tuple[float, float]]
    outdir: str | None = None


class ConfigBody(BaseModel):
    model: str | None = None
    language: str | None = None


def get_job(job_id: str) -> Job:
    job = STATE.jobs.get(job_id)
    if not job:
        raise HTTPException(404, "Fichier inconnu")
    return job


@app.get("/api/config")
def get_config():
    return {"model": STATE.model, "language": STATE.language}


@app.post("/api/config")
def set_config(body: ConfigBody):
    if body.model:
        STATE.model = body.model
    if body.language:
        STATE.language = body.language
    return get_config()


WINDOW = None  # fenêtre pywebview, si l'app tourne en fenêtre native

MEDIA_EXT = "*.mp4;*.mov;*.mkv;*.mxf;*.avi;*.webm;*.m4v;*.wav;*.mp3;*.m4a;*.aac;*.flac"


@app.post("/api/pick")
def pick_files():
    """Ouvre la boîte de dialogue « Ouvrir » (le navigateur ne donne pas les chemins)."""
    if WINDOW is not None:
        import webview

        paths = WINDOW.create_file_dialog(
            webview.FileDialog.OPEN, allow_multiple=True,
            file_types=(f"Vidéo et audio ({MEDIA_EXT})", "Tous les fichiers (*.*)"),
        )
        return {"paths": list(paths or [])}

    import tkinter as tk
    from tkinter import filedialog

    root = tk.Tk()
    root.withdraw()
    root.attributes("-topmost", True)
    paths = filedialog.askopenfilenames(
        title="Choisir des rushes",
        filetypes=[("Vidéo et audio", "*.mp4 *.mov *.mkv *.mxf *.avi *.webm *.m4v *.wav *.mp3 *.m4a *.aac *.flac"),
                   ("Tous les fichiers", "*.*")],
    )
    root.destroy()
    return {"paths": list(paths)}


@app.post("/api/jobs")
def add_jobs(body: AddBody):
    added = []
    for p in body.paths:
        path = Path(p.strip().strip('"'))
        if not path.is_file():
            raise HTTPException(400, f"Fichier introuvable : {p}")
        added.append(STATE.add(path.resolve(), body.settings).public())
    return {"jobs": added}


@app.get("/api/jobs")
def list_jobs():
    return {"jobs": [j.public() for j in STATE.jobs.values()]}


@app.get("/api/jobs/{job_id}")
def job_detail(job_id: str, peaks: int = 0):
    return get_job(job_id).public(full=True, peaks=bool(peaks))


@app.delete("/api/jobs/{job_id}")
def remove_job(job_id: str):
    STATE.jobs.pop(job_id, None)
    return {"ok": True}


@app.post("/api/jobs/{job_id}/settings")
def update_settings(job_id: str, body: Settings):
    job = get_job(job_id)
    need_words = body.cut_fillers and not job.settings.cut_fillers and job.words is None
    job.settings = body
    if need_words and job.stage == "prêt":
        def run():
            try:
                ensure_words(job)
            finally:
                job.stage = "prêt"
                recut(job)
        threading.Thread(target=run, daemon=True).start()
    else:
        recut(job)
    return job.public(full=True)


@app.get("/api/jobs/{job_id}/media")
def job_media(job_id: str):
    job = get_job(job_id)
    if job.media_path is None:
        raise HTTPException(409, "Aperçu pas encore prêt")
    return FileResponse(job.media_path)


@app.get("/api/jobs/{job_id}/thumb")
def job_thumb(job_id: str):
    job = get_job(job_id)
    if job.thumb_path is None:
        raise HTTPException(404, "Pas de vignette")
    return FileResponse(job.thumb_path, headers={"Cache-Control": "max-age=3600"})


@app.post("/api/jobs/{job_id}/export")
def export_job(job_id: str, body: ExportBody):
    job = get_job(job_id)
    if job.info is None:
        raise HTTPException(409, "Analyse pas terminée")
    info = job.info
    keep = merge([(float(a), float(b)) for a, b in body.keep])
    frames = to_frames(keep, info.fps, info.total_frames)
    if not frames:
        raise HTTPException(400, "Rien à exporter")
    outdir = Path(body.outdir) if body.outdir else info.path.parent
    outdir.mkdir(parents=True, exist_ok=True)
    stem = outdir / f"{info.path.stem}_crisptrim"
    name = f"{info.path.stem} (CrispTrim)"
    files = []
    if {"txt", "srt"} & set(body.formats):
        if not job.words:
            raise HTTPException(400, "Pas de transcription : active « Couper les hésitations » pour transcrire.")
        aligned = [(float(a / info.fps), float(b / info.fps)) for a, b in frames]
        if "txt" in body.formats:
            write_txt(job.words, aligned, stem.with_suffix(".txt"))
            files.append(str(stem.with_suffix(".txt")))
        if "srt" in body.formats:
            write_srt(job.words, aligned, stem.with_suffix(".srt"))
            files.append(str(stem.with_suffix(".srt")))
    if "fcpxml" in body.formats:
        write_fcpxml(info, frames, stem.with_suffix(".fcpxml"), name)
        files.append(str(stem.with_suffix(".fcpxml")))
    if "premiere" in body.formats:
        write_premiere_xml(info, frames, stem.with_suffix(".xml"), name)
        files.append(str(stem.with_suffix(".xml")))
    if "vegas" in body.formats:
        write_vegas_edl(info, frames, stem.with_suffix(".vegas.txt"))
        files.append(str(stem.with_suffix(".vegas.txt")))
    previous = [f for f in job.export.get("files", []) if f not in files]
    job.export = {"state": "fait", "files": previous + files, "error": ""}
    if "mp4" in body.formats:
        job.export["state"] = "rendu"
        mp4 = str(stem.with_suffix(".mp4"))

        def run():
            try:
                render_mp4(info, frames, stem.with_suffix(".mp4"))
                job.export["files"] = [f for f in job.export["files"] if f != mp4] + [mp4]
                job.export["state"] = "fait"
            except Exception as exc:  # noqa: BLE001
                job.export.update(state="erreur", error=str(exc))
            job.version += 1

        threading.Thread(target=run, daemon=True).start()
    job.version += 1
    return job.public()


@app.get("/api/version")
def version_info():
    return {"version": __version__, "author": __author__, "repo_url": REPO_URL, "license": "GPL-3.0"}


@app.get("/api/update")
def update_status(force: int = 0):
    return check_update(DATA_DIR / "update.json", force=bool(force))


class UrlBody(BaseModel):
    url: str


# Liens que l'interface peut ouvrir dans le navigateur du système (crédits, GitHub).
OPEN_HOSTS = {"github.com", "huggingface.co", "ffmpeg.org", "fastapi.tiangolo.com", "pywebview.flowrl.com", "www.gnu.org"}


@app.post("/api/open-url")
def open_url(body: UrlBody):
    from urllib.parse import urlparse

    u = urlparse(body.url)
    if u.scheme != "https" or u.hostname not in OPEN_HOSTS:
        raise HTTPException(400, "Lien non autorisé")
    webbrowser.open(body.url)
    return {"ok": True}


class RevealBody(BaseModel):
    path: str


@app.post("/api/reveal")
def reveal(body: RevealBody):
    p = Path(body.path)
    if sys.platform == "win32":
        subprocess.Popen(["explorer", "/select,", str(p)] if p.is_file() else ["explorer", str(p)])
    elif sys.platform == "darwin":
        subprocess.Popen(["open", "-R", str(p)])
    else:
        subprocess.Popen(["xdg-open", str(p.parent if p.is_file() else p)])
    return {"ok": True}


@app.middleware("http")
async def no_cache_ui(request, call_next):
    # L'interface change avec les mises à jour : WebView2 ne doit pas garder une ancienne version.
    response = await call_next(request)
    if not request.url.path.startswith("/api/"):
        response.headers["Cache-Control"] = "no-cache"
    return response


app.mount("/", StaticFiles(directory=STATIC, html=True), name="static")


def main() -> None:
    import argparse

    import uvicorn

    ap = argparse.ArgumentParser(prog="crisptrim-app")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--browser", action="store_true", help="ouvrir dans le navigateur au lieu d'une fenêtre")
    ap.add_argument("--no-browser", action="store_true", help="serveur seul (développement)")
    args = ap.parse_args()
    require_ffmpeg()
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    url = f"http://127.0.0.1:{args.port}"
    print(f"CrispTrim : {url}")

    if args.browser or args.no_browser:
        if args.browser:
            threading.Timer(1.0, lambda: webbrowser.open(url)).start()
        uvicorn.run(app, host="127.0.0.1", port=args.port, log_level="warning")
        return

    # Fenêtre native (WebView2) : le serveur tourne dans un fil, la fenêtre dans le fil principal.
    import logging
    import socket

    # À la fermeture, WebView2 coupe ses connexions : asyncio l'affiche comme une erreur, sans conséquence.
    logging.getLogger("asyncio").setLevel(logging.CRITICAL)

    import webview

    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=args.port, log_level="warning"))
    threading.Thread(target=server.run, daemon=True).start()
    for _ in range(100):
        with socket.socket() as s:
            if s.connect_ex(("127.0.0.1", args.port)) == 0:
                break
        threading.Event().wait(0.1)

    global WINDOW
    WINDOW = webview.create_window(
        f"CrispTrim {__version__}", url, width=1440, height=900, min_size=(1000, 640), background_color="#0e1014",
    )
    webview.start(bind_drop, WINDOW)
    server.should_exit = True


MEDIA_SUFFIXES = {e.lstrip("*") for e in MEDIA_EXT.split(";")}


def bind_drop(window) -> None:
    """Glisser-déposer : WebView2 donne le chemin complet des fichiers déposés dans la fenêtre."""
    import json

    from webview.dom import DOMEventHandler

    def on_drop(event: dict) -> None:
        files = (event.get("dataTransfer") or {}).get("files") or []
        paths = [f.get("pywebviewFullPath") for f in files]
        media = [p for p in paths if p and Path(p).suffix.lower() in MEDIA_SUFFIXES]
        ignored = len(files) - len(media)
        window.evaluate_js(f"window.onNativeDrop && window.onNativeDrop({json.dumps(media)}, {ignored})")

    window.dom.document.events.drop += DOMEventHandler(on_drop, True, True)


if __name__ == "__main__":
    main()
