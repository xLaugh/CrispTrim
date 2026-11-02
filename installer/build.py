"""Construit dist/CrispTrim-Setup.exe (PyInstaller, un seul fichier, sans console).

    uv run --no-project --python 3.12 --with pyinstaller python installer/build.py
"""

from __future__ import annotations

import re
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HERE = ROOT / "installer"
BUILD = ROOT / "build"
FILES = ["pyproject.toml", "uv.lock", "README.md", "LICENSE"]


def version() -> str:
    return re.search(r'__version__ = "(.+?)"', (ROOT / "crisptrim" / "__init__.py").read_text(encoding="utf-8"))[1]


def payload(out: Path) -> None:
    """Le code de l'appli, embarqué dans l'installateur."""
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name in FILES:
            z.write(ROOT / name, name)
        for f in sorted((ROOT / "crisptrim").rglob("*")):
            if f.is_file() and "__pycache__" not in f.parts and f.suffix != ".pyc":
                z.write(f, f.relative_to(ROOT).as_posix())


def version_file(v: str, out: Path) -> None:
    nums = tuple(int(x) for x in v.split(".")) + (0,) * (4 - len(v.split(".")))
    out.write_text(f"""VSVersionInfo(
  ffi=FixedFileInfo(filevers={nums}, prodvers={nums}, mask=0x3f, flags=0x0, OS=0x40004,
                    fileType=0x1, subtype=0x0, date=(0, 0)),
  kids=[
    StringFileInfo([StringTable('040C04B0', [
      StringStruct('CompanyName', 'xLaugh'),
      StringStruct('FileDescription', 'CrispTrim : installation et lancement'),
      StringStruct('FileVersion', '{v}'),
      StringStruct('InternalName', 'CrispTrim'),
      StringStruct('LegalCopyright', 'xLaugh, GNU GPL v3'),
      StringStruct('OriginalFilename', 'CrispTrim-Setup.exe'),
      StringStruct('ProductName', 'CrispTrim'),
      StringStruct('ProductVersion', '{v}')])]),
    VarFileInfo([VarStruct('Translation', [0x040C, 1200])])
  ]
)
""", encoding="utf-8")


def main() -> int:
    v = version()
    BUILD.mkdir(exist_ok=True)
    payload(HERE / "payload.zip")
    version_file(v, BUILD / "version.txt")
    sep = ";" if sys.platform == "win32" else ":"
    data = [HERE / "payload.zip", HERE / "crisptrim.ico", HERE / "crisptrim.png"]
    cmd = [
        sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--onefile", "--windowed",
        "--name", "CrispTrim-Setup", "--icon", str(HERE / "crisptrim.ico"),
        "--version-file", str(BUILD / "version.txt"),
        "--distpath", str(ROOT / "dist"), "--workpath", str(BUILD / "pyi"), "--specpath", str(BUILD),
        *[a for d in data for a in ("--add-data", f"{d}{sep}.")],
        str(HERE / "launcher.py"),
    ]
    subprocess.run(cmd, check=True)
    exe = ROOT / "dist" / "CrispTrim-Setup.exe"
    print(f"\n{exe} ({exe.stat().st_size / 1e6:.1f} Mo), version {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
