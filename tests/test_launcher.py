"""Lanceur Windows (installer/launcher.py) : les parties testables sans installer pour de vrai."""

import importlib.util
import os
import sys
import zipfile
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture()
def launcher(tmp_path, monkeypatch):
    monkeypatch.setenv("CRISPTRIM_HOME", str(tmp_path / "home"))
    spec = importlib.util.spec_from_file_location("crisptrim_launcher", ROOT / "installer" / "launcher.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_versions_and_tags(launcher):
    assert launcher.vtuple("v0.10.1") > launcher.vtuple("0.9.9")
    assert launcher.vtuple("0.1") == (0, 1, 0)
    assert launcher.TAG_RE.match("v1.2.3") and launcher.TAG_RE.match("v1.2")
    for bad in ("1.2.3", "v1.2.3; calc", "v1.2.3/../x", ""):
        assert not launcher.TAG_RE.match(bad)


def test_pyinstaller_variables_are_dropped(launcher, monkeypatch):
    monkeypatch.setenv("_PYI_APPLICATION_HOME_DIR", r"C:\Temp\_MEI1234")
    monkeypatch.setenv("_MEIPASS2", r"C:\Temp\_MEI1234")
    monkeypatch.setenv("TCL_LIBRARY", r"C:\Temp\_MEI1234\tcl")
    env = launcher.tool_env()
    assert not any(k.startswith("_PYI_") or k in ("_MEIPASS2", "TCL_LIBRARY") for k in env)
    home = Path(os.environ["CRISPTRIM_HOME"])
    assert env["UV_PROJECT_ENVIRONMENT"] == str(home / "env")
    assert env["PATH"].startswith(str(home / "bin"))
    assert env["UV_PYTHON_INSTALL_REGISTRY"] == "0"


def test_put_code_swaps_and_restores(launcher, tmp_path):
    P = launcher.P
    P.home.mkdir(parents=True)
    (P.app / "crisptrim").mkdir(parents=True)
    (P.app / "pyproject.toml").write_text("ancien", encoding="utf-8")
    (P.app / "crisptrim" / "__init__.py").write_text('__version__ = "0.1.0"\n', encoding="utf-8")

    z = tmp_path / "CrispTrim-0.2.0.zip"  # même forme que les archives de tag GitHub
    with zipfile.ZipFile(z, "w") as zf:
        zf.writestr("CrispTrim-0.2.0/pyproject.toml", "nouveau")
        zf.writestr("CrispTrim-0.2.0/crisptrim/__init__.py", '__version__ = "0.2.0"\n')

    job = launcher.Job()
    job.put_code(z, "CrispTrim-0.2.0/")
    assert (P.app / "pyproject.toml").read_text(encoding="utf-8") == "nouveau"
    assert launcher.code_version() == "0.2.0"
    assert (P.home / "app.old" / "pyproject.toml").read_text(encoding="utf-8") == "ancien"

    job.restore_code()  # en cas d'échec de la mise à jour
    assert launcher.code_version() == "0.1.0"

    launcher.cleanup()
    assert not (P.home / "app.old").exists()


def test_invalid_archive_keeps_current_code(launcher, tmp_path):
    P = launcher.P
    (P.app).mkdir(parents=True)
    (P.app / "pyproject.toml").write_text("actuel", encoding="utf-8")
    z = tmp_path / "vide.zip"
    with zipfile.ZipFile(z, "w") as zf:
        zf.writestr("x/README.md", "rien")
    with pytest.raises(RuntimeError):
        launcher.Job().put_code(z, "x/")
    assert (P.app / "pyproject.toml").read_text(encoding="utf-8") == "actuel"


@pytest.mark.skipif(sys.platform != "win32", reason="chemins Windows")
def test_payload_contains_the_app(tmp_path):
    spec = importlib.util.spec_from_file_location("crisptrim_build", ROOT / "installer" / "build.py")
    build = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build)
    out = tmp_path / "payload.zip"
    build.payload(out)
    names = set(zipfile.ZipFile(out).namelist())
    assert {"pyproject.toml", "uv.lock", "LICENSE", "crisptrim/__init__.py", "crisptrim/app/server.py",
            "crisptrim/app/static/index.html", "crisptrim/app/static/crisptrim.ico"} <= names
    assert not any("__pycache__" in n for n in names)
