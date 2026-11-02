"""Vérifie si une version plus récente est publiée dans les Releases GitHub."""

from __future__ import annotations

import json
import re
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Callable

from . import REPO, REPO_URL, __version__

API = f"https://api.github.com/repos/{REPO}/releases/latest"
TTL = 6 * 3600  # GitHub limite l'API publique à 60 requêtes par heure


def parse_version(v: str) -> tuple[int, int, int] | None:
    """« v1.2.3 », « 1.2 » → (1, 2, 3) / (1, 2, 0). Les suffixes (-beta…) sont ignorés."""
    m = re.match(r"^\s*v?(\d+)\.(\d+)(?:\.(\d+))?", v or "")
    return (int(m[1]), int(m[2]), int(m[3] or 0)) if m else None


def is_newer(latest: str, current: str = __version__) -> bool:
    a, b = parse_version(latest), parse_version(current)
    return a is not None and b is not None and a > b


def fetch_latest(timeout: float = 5) -> dict | None:
    """Dernière release publiée (hors brouillons et préversions), ou None s'il n'y en a pas."""
    req = urllib.request.Request(API, headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": f"CrispTrim/{__version__}",
    })
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            data = json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 404:  # aucune release
            return None
        raise
    return {
        "tag": data.get("tag_name", ""),
        "name": data.get("name") or data.get("tag_name", ""),
        "url": data.get("html_url") or f"{REPO_URL}/releases",
        "notes": (data.get("body") or "").strip(),
        "published": data.get("published_at", ""),
    }


def check(
    cache_file: Path, fetch: Callable[[], dict | None] = fetch_latest, now: float | None = None, force: bool = False
) -> dict:
    """État de mise à jour, avec un cache de quelques heures. Ne lève jamais d'erreur."""
    now = time.time() if now is None else now
    cached = None
    try:
        cached = json.loads(cache_file.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        pass

    error = ""
    if cached and not force and now - cached.get("checked", 0) < TTL:
        release = cached.get("release")
    else:
        try:
            release = fetch()
            cache_file.parent.mkdir(parents=True, exist_ok=True)
            cache_file.write_text(json.dumps({"checked": now, "release": release}), encoding="utf-8")
        except Exception as exc:  # noqa: BLE001 — hors ligne, GitHub indisponible…
            release = cached.get("release") if cached else None
            error = str(exc) or exc.__class__.__name__

    latest = release["tag"] if release else None
    return {
        "current": __version__,
        "latest": latest,
        "available": bool(latest and is_newer(latest)),
        "release": release,
        "error": error,
    }
