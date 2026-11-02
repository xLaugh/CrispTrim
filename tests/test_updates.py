import json

from crisptrim import __version__
from crisptrim.updates import TTL, check, is_newer, parse_version

REL = {"tag": "v9.0.0", "name": "CrispTrim 9", "url": "https://github.com/xLaugh/CrispTrim/releases/tag/v9.0.0",
       "notes": "Nouveautés", "published": ""}


def test_versions():
    assert parse_version("v1.2.3") == (1, 2, 3)
    assert parse_version("0.2") == (0, 2, 0)
    assert parse_version("v1.0.0-beta") == (1, 0, 0)
    assert parse_version("nimporte") is None
    assert is_newer("v0.1.1", "0.1.0") and is_newer("v0.10.0", "0.9.9")
    assert not is_newer("v0.1.0", "0.1.0") and not is_newer("v0.0.9", "0.1.0") and not is_newer("x", "0.1.0")


def test_new_version_and_cache(tmp_path):
    cache = tmp_path / "update.json"
    calls = []
    fetch = lambda: calls.append(1) or REL  # noqa: E731
    st = check(cache, fetch, now=1000)
    assert st["available"] and st["latest"] == "v9.0.0" and st["current"] == __version__
    check(cache, fetch, now=1000 + TTL - 1)  # encore en cache
    assert len(calls) == 1
    check(cache, fetch, now=1000 + TTL + 1)  # cache expiré
    assert len(calls) == 2


def test_offline_and_no_release(tmp_path):
    def offline():
        raise OSError("hors ligne")

    st = check(tmp_path / "a.json", offline, now=0)
    assert not st["available"] and st["latest"] is None and st["error"]

    # hors ligne avec un ancien résultat : on le garde
    cache = tmp_path / "b.json"
    cache.write_text(json.dumps({"checked": 0, "release": REL}), encoding="utf-8")
    st = check(cache, offline, now=TTL * 10)
    assert st["available"] and st["error"]

    st = check(tmp_path / "c.json", lambda: None, now=0)  # aucune release publiée
    assert not st["available"] and st["latest"] is None and not st["error"]


def test_same_version_not_available(tmp_path):
    rel = {**REL, "tag": f"v{__version__}"}
    assert not check(tmp_path / "d.json", lambda: rel, now=0)["available"]
