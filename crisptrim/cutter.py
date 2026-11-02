"""Calcule les segments à garder à partir des zones sonores et des mots transcrits."""

from __future__ import annotations

from dataclasses import dataclass, field
from fractions import Fraction

from .fillers import is_cuttable
from .intervals import Interval, invert, merge, pad, subtract, total
from .transcribe import Word


@dataclass
class CutParams:
    margin: float = 0.15        # marge gardée avant/après chaque phrase
    min_keep: float = 0.25      # un segment gardé plus court est supprimé
    cut_fillers: bool = True
    cut_noises: bool = True     # bâillements, toux, souffles ([yawn], [cough]…)
    filler_pad: float = 0.03    # marge autour d'un « euh » collé à un mot
    max_filler_extend: float = 1.0  # la coupe d'un « euh » ne déborde pas plus loin
    extra_fillers: set[str] = field(default_factory=set)


@dataclass
class Region:
    start: float
    end: float
    kind: str  # "keep" | "silence" | "filler"
    text: str = ""


@dataclass
class CutResult:
    duration: float
    keep: list[Interval]
    regions: list[Region]
    fillers: list[Word]

    @property
    def kept_duration(self) -> float:
        return total(self.keep)

    def stats(self) -> dict:
        removed = self.duration - self.kept_duration
        return {
            "duration": round(self.duration, 3),
            "kept": round(self.kept_duration, 3),
            "removed": round(removed, 3),
            "removed_pct": round(100 * removed / self.duration, 1) if self.duration else 0.0,
            "segments": len(self.keep),
            "fillers": len(self.fillers),
        }


def filler_cuts(words: list[Word], duration: float, p: CutParams) -> tuple[list[Interval], list[Word]]:
    """Pour chaque « euh », coupe aussi les pauses qui l'entourent, en gardant une marge naturelle."""
    flags = [is_cuttable(w.word, p.extra_fillers, p.cut_noises) for w in words]
    cuts: list[Interval] = []
    fillers: list[Word] = []
    for i, w in enumerate(words):
        if not flags[i]:
            continue
        fillers.append(w)
        prev_end = next((words[j].end for j in range(i - 1, -1, -1) if not flags[j]), 0.0)
        next_start = next((words[j].start for j in range(i + 1, len(words)) if not flags[j]), duration)

        left = prev_end + p.margin if prev_end + p.margin < w.start else max(prev_end, w.start - p.filler_pad)
        right = next_start - p.margin if next_start - p.margin > w.end else min(next_start, w.end + p.filler_pad)
        left = max(left, w.start - p.max_filler_extend)
        right = min(right, w.end + p.max_filler_extend)
        cuts.append((left, right))
    return merge(cuts), fillers


def compute_cuts(
    duration: float, speech: list[Interval], words: list[Word] | None, p: CutParams
) -> CutResult:
    keep = pad(speech, p.margin, 0.0, duration)

    cuts: list[Interval] = []
    fillers: list[Word] = []
    if words and p.cut_fillers:
        cuts, fillers = filler_cuts(words, duration, p)
        keep = subtract(keep, cuts)

    keep = [seg for seg in merge(keep) if seg[1] - seg[0] >= p.min_keep]

    regions = [Region(s, e, "keep") for s, e in keep]
    for s, e in invert(keep, 0.0, duration):
        is_fill = any(cs < e and ce > s for cs, ce in cuts)
        text = " ".join(f.word for f in fillers if s <= f.start < e) if is_fill else ""
        regions.append(Region(s, e, "filler" if is_fill else "silence", text))
    regions.sort(key=lambda r: r.start)
    return CutResult(duration, keep, regions, fillers)


def to_frames(keep: list[Interval], fps: Fraction, total_frames: int) -> list[tuple[int, int]]:
    """Aligne les segments sur les images de la vidéo : [début, fin) en numéros d'image."""
    out: list[tuple[int, int]] = []
    for s, e in keep:
        a = max(0, int(round(s * fps)))
        b = min(total_frames, int(round(e * fps)))
        if b <= a:
            continue
        if out and a <= out[-1][1]:
            out[-1] = (out[-1][0], max(out[-1][1], b))
        else:
            out.append((a, b))
    return out
