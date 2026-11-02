"""Exports texte : transcription (.txt) et sous-titres (.srt) recalés sur le montage."""

from __future__ import annotations

import re
from pathlib import Path

from .fillers import is_cuttable
from .intervals import Interval
from .transcribe import Word


def clean(text: str, capitalize: bool = True) -> str:
    text = re.sub(r"\s+([,.;:!?…])", r"\1", text)
    text = re.sub(r"\s{2,}", " ", text).strip()
    return text[:1].upper() + text[1:] if capitalize else text


def kept_words(words: list[Word], keep: list[Interval]) -> list[Word]:
    """Mots prononcés qui restent dans le montage (sans hésitations ni bruits)."""
    out, k = [], 0
    for w in words:
        if is_cuttable(w.word):
            continue
        mid = (w.start + w.end) / 2
        while k < len(keep) and keep[k][1] < mid:
            k += 1
        if k < len(keep) and keep[k][0] <= mid <= keep[k][1]:
            out.append(w)
    return out


def retime(t: float, keep: list[Interval]) -> float:
    """Temps dans la vidéo d'origine → temps dans le montage."""
    acc = 0.0
    for s, e in keep:
        if t <= s:
            return acc
        if t <= e:
            return acc + t - s
        acc += e - s
    return acc


def paragraphs(words: list[Word], gap: float = 1.5) -> list[tuple[float, str]]:
    paras: list[tuple[float, list[str]]] = []
    prev = None
    for w in words:
        if not paras or (prev is not None and w.start - prev > gap):
            paras.append((w.start, []))
        paras[-1][1].append(w.word)
        prev = w.end
    return [(s, clean(" ".join(ws))) for s, ws in paras]


def write_txt(words: list[Word], keep: list[Interval], out: Path, timestamps: bool = True) -> None:
    lines = []
    for start, text in paragraphs(kept_words(words, keep)):
        t = retime(start, keep)
        lines.append(f"[{int(t // 60):02d}:{int(t % 60):02d}] {text}" if timestamps else text)
    out.write_text("\n\n".join(lines) + "\n", encoding="utf-8")


def _srt_time(t: float) -> str:
    ms = int(round(max(t, 0) * 1000))
    h, ms = divmod(ms, 3_600_000)
    m, ms = divmod(ms, 60_000)
    s, ms = divmod(ms, 1000)
    return f"{h:02d}:{m:02d}:{s:02d},{ms:03d}"


def _two_lines(text: str, width: int) -> str:
    if len(text) <= width:
        return text
    mid = len(text) // 2
    cut = min((i for i, c in enumerate(text) if c == " "), key=lambda i: abs(i - mid), default=-1)
    return text if cut < 0 else text[:cut] + "\n" + text[cut + 1:]


def subtitle_cues(
    words: list[Word], keep: list[Interval], width: int = 42, max_dur: float = 5.0, gap: float = 0.8
) -> list[tuple[float, float, str]]:
    """Regroupe les mots en sous-titres (2 lignes max), calés sur le montage."""
    cues: list[tuple[float, float, str]] = []
    cur: list[Word] = []
    sentence_start = True

    def flush():
        nonlocal sentence_start
        if cur:
            text = clean(" ".join(w.word for w in cur), capitalize=sentence_start)
            cues.append((retime(cur[0].start, keep), retime(cur[-1].end, keep), _two_lines(text, width)))
            sentence_start = bool(re.search(r"[.!?…]$", cur[-1].word))
            cur.clear()

    for w in kept_words(words, keep):
        if cur:
            length = len(" ".join(x.word for x in cur)) + 1 + len(w.word)
            # Les écarts se mesurent dans le montage : les pauses coupées ne comptent plus.
            pause = retime(w.start, keep) - retime(cur[-1].end, keep)
            span = retime(w.end, keep) - retime(cur[0].start, keep)
            # Au-delà d'une ligne, on coupe de préférence sur une pause de la voix ou une virgule.
            cur_len = length - 1 - len(w.word)
            natural = w.start - cur[-1].end > 0.3 or re.search(r"[,;:]$", cur[-1].word)
            if pause > gap or length > 2 * width or span > max_dur or (cur_len >= width and natural):
                flush()
        cur.append(w)
        if re.search(r"[.!?…]$", w.word) and len(" ".join(x.word for x in cur)) > width / 2:
            flush()
    flush()

    # Pas de chevauchement, durée minimale lisible.
    fixed = []
    for i, (s, e, t) in enumerate(cues):
        nxt = cues[i + 1][0] if i + 1 < len(cues) else None
        e = max(e, s + 0.7)
        if nxt is not None:
            e = min(e, nxt - 0.04)
        fixed.append((s, max(e, s + 0.1), t))
    return fixed


def write_srt(words: list[Word], keep: list[Interval], out: Path) -> None:
    blocks = [
        f"{i}\n{_srt_time(s)} --> {_srt_time(e)}\n{t}\n"
        for i, (s, e, t) in enumerate(subtitle_cues(words, keep), start=1)
    ]
    out.write_text("\n".join(blocks), encoding="utf-8")
