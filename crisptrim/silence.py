"""Détection des zones de parole par énergie (indépendante de la langue)."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from .intervals import Interval

FRAME = 0.01  # 10 ms


@dataclass
class SilenceParams:
    threshold_db: float | None = None  # None = seuil automatique
    min_silence: float = 0.5           # une pause plus courte est gardée
    min_sound: float = 0.08            # ignore les clics plus courts


def frame_db(audio: np.ndarray, sr: int, frame: float = FRAME) -> np.ndarray:
    hop = max(1, int(sr * frame))
    n = len(audio) // hop
    if n == 0:
        return np.zeros(0, dtype=np.float32)
    x = audio[: n * hop].reshape(n, hop).astype(np.float64)
    rms = np.sqrt(np.mean(x * x, axis=1))
    return (20 * np.log10(rms + 1e-10)).astype(np.float32)


def auto_threshold(db: np.ndarray) -> float:
    """Seuil placé entre le bruit de fond (percentile 10) et la voix (percentile 95)."""
    if db.size == 0:
        return -45.0
    floor = max(float(np.percentile(db, 10)), -80.0)
    peak = float(np.percentile(db, 95))
    thr = floor + 0.3 * (peak - floor)
    return float(np.clip(thr, -60.0, peak - 6.0))


def _runs(mask: np.ndarray) -> list[tuple[int, int]]:
    """Plages [début, fin) où mask est vrai."""
    if mask.size == 0:
        return []
    d = np.diff(np.concatenate(([0], mask.astype(np.int8), [0])))
    return list(zip(np.flatnonzero(d == 1), np.flatnonzero(d == -1)))


def detect_speech(audio: np.ndarray, sr: int, params: SilenceParams) -> tuple[list[Interval], float]:
    """Renvoie les zones sonores et le seuil (dBFS) utilisé."""
    db = frame_db(audio, sr)
    thr = params.threshold_db if params.threshold_db is not None else auto_threshold(db)
    loud = db > thr

    # Les pauses courtes font partie de la phrase.
    min_sil = int(round(params.min_silence / FRAME))
    for s, e in _runs(~loud):
        if e - s < min_sil and s > 0 and e < loud.size:
            loud[s:e] = True

    min_snd = int(round(params.min_sound / FRAME))
    speech = [(s * FRAME, e * FRAME) for s, e in _runs(loud) if e - s >= min_snd]
    return speech, thr
