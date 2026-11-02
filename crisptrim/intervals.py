"""Opérations sur des listes d'intervalles (start, end) en secondes."""

from __future__ import annotations

Interval = tuple[float, float]


def merge(items: list[Interval], gap: float = 0.0) -> list[Interval]:
    """Fusionne les intervalles qui se chevauchent ou sont séparés de moins de `gap`."""
    out: list[Interval] = []
    for s, e in sorted(i for i in items if i[1] > i[0]):
        if out and s - out[-1][1] <= gap:
            out[-1] = (out[-1][0], max(out[-1][1], e))
        else:
            out.append((s, e))
    return out


def pad(items: list[Interval], amount: float, lo: float, hi: float) -> list[Interval]:
    return merge([(max(lo, s - amount), min(hi, e + amount)) for s, e in items])


def subtract(items: list[Interval], cuts: list[Interval]) -> list[Interval]:
    """Retire `cuts` de `items`."""
    cuts = merge(cuts)
    out: list[Interval] = []
    for s, e in merge(items):
        cur = s
        for cs, ce in cuts:
            if ce <= cur or cs >= e:
                continue
            if cs > cur:
                out.append((cur, cs))
            cur = max(cur, ce)
            if cur >= e:
                break
        if cur < e:
            out.append((cur, e))
    return out


def invert(items: list[Interval], lo: float, hi: float) -> list[Interval]:
    return subtract([(lo, hi)], items)


def total(items: list[Interval]) -> float:
    return sum(e - s for s, e in items)
