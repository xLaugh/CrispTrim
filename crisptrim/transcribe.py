"""Transcription verbatim mot à mot avec CrisperWhisper 2.0 (avec cache disque)."""

from __future__ import annotations

import hashlib
import json
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np

from .media import SAMPLE_RATE


@dataclass
class Word:
    word: str
    start: float
    end: float


class Transcriber:
    """Charge le modèle une seule fois pour traiter plusieurs fichiers."""

    def __init__(self, model: str = "large", language: str = "fr", device: str = "auto"):
        self.model_name = model
        self.language = language
        self.device = device
        self._model = None

    def _load(self):
        if self._model is None:
            try:
                from crisperwhisper import CrisperWhisperModel
            except ImportError as exc:  # pragma: no cover
                raise RuntimeError(
                    "CrisperWhisper n'est pas installé. Lance : uv sync --extra asr"
                ) from exc
            self._model = CrisperWhisperModel(self.model_name, backend="transformers", device=self.device)
        return self._model

    def cache_key(self, source: Path) -> str:
        st = source.stat()
        raw = f"{source.resolve()}|{st.st_size}|{st.st_mtime_ns}|{self.model_name}|{self.language}"
        return hashlib.sha1(raw.encode()).hexdigest()[:16]

    def transcribe(self, audio: np.ndarray, source: Path, cache_dir: Path | None = None) -> list[Word]:
        cache_file = None
        if cache_dir is not None:
            cache_dir.mkdir(parents=True, exist_ok=True)
            cache_file = cache_dir / f"{source.stem}.{self.cache_key(source)}.words.json"
            if cache_file.exists():
                return [Word(**w) for w in json.loads(cache_file.read_text(encoding="utf-8"))]

        result = self._load().transcribe(
            audio, sr=SAMPLE_RATE, language=self.language, mode="verbatim", word_timestamps=True
        )
        words = [
            Word(w.word.strip(), float(w.start), float(w.end))
            for w in (result.words or [])
            if w.word.strip()
        ]
        if cache_file is not None:
            cache_file.write_text(
                json.dumps([asdict(w) for w in words], ensure_ascii=False, indent=1), encoding="utf-8"
            )
        return words
