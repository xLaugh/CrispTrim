"""Reconnaissance des hésitations en français (« euh », « hum »…) et des marqueurs CrisperWhisper."""

from __future__ import annotations

import re

# Marqueurs produits par CrisperWhisper en mode verbatim.
MARKERS = {"[uh]", "[um]", "[uhm]", "[hm]", "[mm]"}

# Volontairement prudent : « eu », « bah », « ben », « hein » sont de vrais mots.
FILLER_RE = re.compile(
    r"^(?:"
    r"e+u+h+"          # euh, euuh, euhh
    r"|h+e+u+h*"       # heu, heuh
    r"|e+u+h*m+"       # euhm, eum
    r"|h+u+m+"         # hum, humm
    r"|h+m+|m+h+|m{2,}"  # hmm, mh, mmh, mm
    r"|u+h+|u+m+"      # uh, um
    r")$"
)


def normalize(word: str) -> str:
    w = word.strip().lower()
    if w.startswith("[") and w.endswith("]"):
        return w
    return re.sub(r"[^\w]", "", w)


# Événements sonores CrisperWhisper à ne jamais couper ; les autres ([yawn], [cough]…) sont des bruits.
KEEP_EVENTS = {"[laughter]", "[laugh]", "[laughs]", "[music]", "[applause]", "[singing]"}


def is_noise(word: str) -> bool:
    """Bruit de bouche transcrit entre crochets (bâillement, toux, souffle…)."""
    w = normalize(word)
    return w.startswith("[") and w.endswith("]") and w not in MARKERS and w not in KEEP_EVENTS


def is_filler(word: str, extra: set[str] | None = None) -> bool:
    w = normalize(word)
    if not w:
        return False
    if w in MARKERS or (extra and w in extra):
        return True
    return bool(FILLER_RE.match(w))


def is_cuttable(word: str, extra: set[str] | None = None, cut_noises: bool = True) -> bool:
    return is_filler(word, extra) or (cut_noises and is_noise(word))
