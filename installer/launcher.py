"""CrispTrim.exe : installe, lance, met à jour et désinstalle CrispTrim sur Windows.

Le même programme sert de « CrispTrim-Setup.exe » (téléchargé depuis les Releases) et de lanceur
installé dans %LOCALAPPDATA%\\CrispTrim. Il n'utilise que la bibliothèque standard de Python.

    CrispTrim.exe                 installe (ou lance, s'il tourne depuis le dossier d'installation)
    CrispTrim.exe --update v0.2.0 met à jour le code puis relance l'appli
    CrispTrim.exe --uninstall     désinstalle
    Options : --yes (sans questions), --no-model, --no-desktop

Variables pour les tests : CRISPTRIM_HOME (dossier d'installation), CRISPTRIM_UV_CACHE, CRISPTRIM_HF_HOME.
"""

from __future__ import annotations

import json
import os
import queue
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import urllib.request
import zipfile
from pathlib import Path

APP = "CrispTrim"
REPO = "xLaugh/CrispTrim"
PUBLISHER = "xLaugh"
UV_VERSION = "0.13.5"
UV_URL = f"https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/uv-x86_64-pc-windows-msvc.zip"
FFMPEG_VERSION = "9.0.2"  # build « essentials » de Gyan, publié sur GitHub (rapide)
FFMPEG_URL = f"https://github.com/GyanD/codexffmpeg/releases/download/{FFMPEG_VERSION}/ffmpeg-{FFMPEG_VERSION}-essentials_build.zip"
MODEL_REPO = "nyralabs/CrisperWhisper2.0_large"
MODEL_BYTES = 2.95e9
DEPS_BYTES = 4.6e9  # PyTorch CUDA + dépendances, une fois décompressés
NEED_FREE = 12e9
PORT = 8765
TAG_RE = re.compile(r"^v\d+\.\d+(\.\d+)?$")

RES = Path(getattr(sys, "_MEIPASS", Path(__file__).parent))  # icône et code embarqués
NO_WINDOW = 0x08000000  # CREATE_NO_WINDOW : pas de console qui clignote


# --------------------------------------------------------------------------- dossiers


class Paths:
    def __init__(self) -> None:
        me = Path(sys.executable).resolve()
        if os.environ.get("CRISPTRIM_HOME"):
            self.home = Path(os.environ["CRISPTRIM_HOME"])
        elif getattr(sys, "frozen", False) and (me.parent / "install.json").exists():
            self.home = me.parent  # lanceur installé : son propre dossier
        else:
            self.home = Path(os.environ["LOCALAPPDATA"]) / APP
        self.bin = self.home / "bin"
        self.app = self.home / "app"
        self.env = self.home / "env"
        self.python = self.home / "python"
        self.uvcache = Path(os.environ.get("CRISPTRIM_UV_CACHE") or self.home / "uv-cache")
        self.models = Path(os.environ.get("CRISPTRIM_HF_HOME") or self.home / "models")
        self.logs = self.home / "logs"
        self.exe = self.home / f"{APP}.exe"
        self.ico = self.home / "crisptrim.ico"
        self.state = self.home / "install.json"
        self.uv = self.bin / "uv.exe"
        self.pythonw = self.env / "Scripts" / "pythonw.exe"
        self.python_exe = self.env / "Scripts" / "python.exe"


P = Paths()


def read_state() -> dict:
    try:
        return json.loads(P.state.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def is_installed() -> bool:
    return bool(read_state().get("version")) and P.pythonw.exists() and (P.app / "pyproject.toml").exists()


def payload_version() -> str:
    """Version du code embarqué dans ce programme."""
    try:
        with zipfile.ZipFile(RES / "payload.zip") as z:
            text = z.read("crisptrim/__init__.py").decode("utf-8")
        return re.search(r'__version__ = "(.+?)"', text)[1]
    except (OSError, KeyError, TypeError):
        return "?"


def code_version() -> str:
    """Version du code installé (crisptrim/__init__.py)."""
    try:
        return re.search(r'__version__ = "(.+?)"', (P.app / "crisptrim" / "__init__.py").read_text(encoding="utf-8"))[1]
    except (OSError, TypeError):
        return read_state().get("version", "?")


def vtuple(v: str) -> tuple:
    m = re.match(r"v?(\d+)\.(\d+)(?:\.(\d+))?", v or "")
    return (int(m[1]), int(m[2]), int(m[3] or 0)) if m else (0, 0, 0)


def clean_environ() -> dict:
    """Environnement sans les variables internes de PyInstaller.

    Sinon un CrispTrim.exe relancé plus tard (mise à jour) se croit sous-processus de celui-ci
    et cherche son dossier temporaire, déjà supprimé : « Failed to load Python DLL ».
    """
    drop = ("VIRTUAL_ENV", "PYTHONHOME", "PYTHONPATH", "TCL_LIBRARY", "TK_LIBRARY", "_MEIPASS2")
    return {k: v for k, v in os.environ.items() if k not in drop and not k.startswith("_PYI_")}


def tool_env() -> dict:
    env = clean_environ()
    env.update(
        UV_PYTHON_INSTALL_DIR=str(P.python),
        UV_PYTHON_PREFERENCE="only-managed",
        UV_PYTHON_INSTALL_REGISTRY="0",  # ne touche pas au registre Python de Windows
        UV_PYTHON_INSTALL_BIN="0",       # ni à ~/.local/bin
        UV_CACHE_DIR=str(P.uvcache),
        UV_PROJECT_ENVIRONMENT=str(P.env),
        UV_NO_PROGRESS="1",
        HF_HOME=str(P.models),
        HF_HUB_DISABLE_SYMLINKS_WARNING="1",
        HF_HUB_DISABLE_TELEMETRY="1",
        CRISPTRIM_HOME=str(P.home),
        CRISPTRIM_LAUNCHER=str(P.exe),
        PYTHONUTF8="1",
        PATH=str(P.bin) + os.pathsep + os.environ.get("PATH", ""),
    )
    return env


def dir_size(path: Path) -> int:
    total = 0
    for root, _dirs, files in os.walk(path):
        for f in files:
            try:
                total += os.lstat(os.path.join(root, f)).st_size
            except OSError:
                pass
    return total


def retry(fn, message: str, seconds: float = 15) -> None:
    """Windows bloque parfois un dossier quelques instants (processus qui se ferme, antivirus)."""
    end = time.time() + seconds
    while True:
        try:
            return fn()
        except PermissionError as exc:
            if time.time() > end:
                raise RuntimeError(message) from exc
            time.sleep(0.5)


def wait_pid(pid: int, timeout: float = 30) -> None:
    """Attend la fin d'un processus (l'appli qui se ferme avant sa mise à jour)."""
    import ctypes

    k = ctypes.windll.kernel32
    h = k.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
    if h:
        k.WaitForSingleObject(h, int(timeout * 1000))
        k.CloseHandle(h)


def app_processes() -> list[int]:
    """Processus lancés depuis le dossier d'installation (l'appli, uv, ffmpeg…), sauf ce lanceur."""
    script = ("Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and "
              "$_.ExecutablePath.StartsWith($env:CT_HOME, 'OrdinalIgnoreCase') -and $_.Name -ne 'CrispTrim.exe' } "
              "| ForEach-Object { $_.ProcessId }")
    out = powershell(script, CT_HOME=str(P.home).rstrip("\\/") + os.sep)
    return [int(x) for x in out.split() if x.isdigit() and int(x) != os.getpid()]


def port_busy() -> bool:
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", PORT)) == 0


# --------------------------------------------------------------------------- lancement de l'appli


def cleanup() -> None:
    """Supprime les copies de secours d'une installation ou d'une mise à jour réussie."""
    shutil.rmtree(P.home / "app.old", ignore_errors=True)
    shutil.rmtree(P.home / "app.new", ignore_errors=True)
    old_exe = P.home / f"{APP}.old.exe"
    try:
        old_exe.unlink(missing_ok=True)
    except OSError:
        pass  # encore en cours d'exécution : ce sera pour la prochaine fois


def launch_app() -> None:
    cleanup()
    P.logs.mkdir(parents=True, exist_ok=True)
    log = P.logs / "crisptrim.log"
    if log.exists() and log.stat().st_size > 2_000_000:
        log.replace(P.logs / "crisptrim.old.log")
    out = open(log, "a", encoding="utf-8")
    out.write(f"\n===== {time.strftime('%Y-%m-%d %H:%M:%S')} lancement =====\n")
    out.flush()
    # Dossier de travail : la racine, jamais app/ (sinon app/ reste verrouillé pendant les mises à jour).
    spawn_detached([str(P.pythonw), "-m", "crisptrim.app.server"], 0x00000008 | 0x00000200,  # détaché
                   cwd=P.home, env=tool_env(), stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)


BREAKAWAY = 0x01000000  # CREATE_BREAKAWAY_FROM_JOB


def spawn_detached(cmd, flags: int, **kw) -> subprocess.Popen:
    """Lance un processus qui survit à ce lanceur.

    Le .exe PyInstaller place ses sous-processus dans un « job » Windows qui peut les arrêter avec lui :
    on en sort quand Windows l'autorise, sinon on lance normalement.
    """
    try:
        return subprocess.Popen(cmd, creationflags=flags | BREAKAWAY, **kw)
    except OSError:
        return subprocess.Popen(cmd, creationflags=flags, **kw)


# --------------------------------------------------------------------------- étapes


class Cancelled(Exception):
    pass


class Job:
    """Exécute les étapes dans un fil et envoie l'avancement à l'interface par une file."""

    def __init__(self) -> None:
        self.q: queue.Queue = queue.Queue()
        self.cancelled = False
        self.proc: subprocess.Popen | None = None

    def emit(self, *msg) -> None:
        self.q.put(msg)

    def check(self) -> None:
        if self.cancelled:
            raise Cancelled()

    def cancel(self) -> None:
        self.cancelled = True
        if self.proc and self.proc.poll() is None:
            self.proc.kill()

    # -- briques

    def download(self, url: str, dest: Path, label: str) -> None:
        tmp = dest.with_suffix(dest.suffix + ".part")
        for attempt in range(3):
            try:
                req = urllib.request.Request(url, headers={"User-Agent": f"{APP}-Setup"})
                with urllib.request.urlopen(req, timeout=30) as r, open(tmp, "wb") as f:
                    total = int(r.headers.get("Content-Length") or 0)
                    done = 0
                    while chunk := r.read(1 << 18):
                        self.check()
                        f.write(chunk)
                        done += len(chunk)
                        frac = done / total if total else None
                        self.emit("progress", frac, f"{label} · {done / 1e6:.0f} Mo" + (f" / {total / 1e6:.0f} Mo" if total else ""))
                tmp.replace(dest)
                return
            except Cancelled:
                raise
            except Exception as exc:  # noqa: BLE001
                if attempt == 2:
                    raise RuntimeError(f"Téléchargement impossible : {url}\n{exc}") from exc
                self.emit("progress", None, f"Nouvel essai ({exc.__class__.__name__})…")
                time.sleep(2)

    def run(self, cmd: list[str], label: str, watch: Path | None = None, expected: float = 0) -> None:
        """Lance une commande ; l'avancement est estimé d'après la taille du dossier surveillé."""
        log = P.logs / "setup.log"
        log.parent.mkdir(parents=True, exist_ok=True)
        with open(log, "a", encoding="utf-8") as out:
            out.write(f"\n> {' '.join(cmd)}\n")
            out.flush()
            start = dir_size(watch) if watch else 0
            self.proc = subprocess.Popen(cmd, cwd=P.home, env=tool_env(), stdout=subprocess.PIPE,
                                         stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                                         creationflags=NO_WINDOW, text=True, encoding="utf-8", errors="replace")
            last = [""]

            def reader():
                for line in self.proc.stdout:
                    out.write(line)
                    if line.strip():
                        last[0] = line.strip()[:90]

            t = threading.Thread(target=reader, daemon=True)
            t.start()
            while self.proc.poll() is None:
                self.check()
                frac, extra = None, ""
                if watch and expected:
                    got = max(0, dir_size(watch) - start)
                    frac = min(0.97, got / expected)
                    extra = f" · {got / 1e9:.1f} Go"
                self.emit("progress", frac, f"{label}{extra}" + (f"\n{last[0]}" if last[0] else ""))
                time.sleep(1)
            t.join(5)
            self.check()
            if self.proc.returncode != 0:
                raise RuntimeError(f"{label} a échoué (code {self.proc.returncode}).\n{last[0]}\nDétails : {log}")

    # -- étapes de l'installation

    def prepare(self) -> None:
        P.home.mkdir(parents=True, exist_ok=True)
        free = shutil.disk_usage(P.home).free
        if free < NEED_FREE and not is_installed():
            raise RuntimeError(f"Pas assez de place : il faut environ {NEED_FREE / 1e9:.0f} Go libres "
                               f"sur ce disque ({free / 1e9:.1f} Go disponibles).")
        for d in (P.bin, P.logs):
            d.mkdir(exist_ok=True)
        if (RES / "crisptrim.ico").exists():
            shutil.copy2(RES / "crisptrim.ico", P.ico)
        me = Path(sys.executable).resolve()
        if getattr(sys, "frozen", False) and me != P.exe.resolve():
            try:
                if P.exe.exists():
                    P.exe.replace(P.home / f"{APP}.old.exe")
            except OSError:
                pass
            shutil.copy2(me, P.exe)

    def get_uv(self) -> None:
        if P.uv.exists():
            return
        z = P.bin / "uv.zip"
        self.download(UV_URL, z, "Téléchargement de uv")
        with zipfile.ZipFile(z) as zf:
            for name in zf.namelist():
                if name.endswith(("uv.exe", "uvw.exe")):
                    (P.bin / Path(name).name).write_bytes(zf.read(name))
        z.unlink(missing_ok=True)

    def get_ffmpeg(self) -> None:
        if (P.bin / "ffmpeg.exe").exists() and (P.bin / "ffprobe.exe").exists():
            return
        z = P.bin / "ffmpeg.zip"
        self.download(FFMPEG_URL, z, "Téléchargement de FFmpeg")
        self.emit("progress", None, "Décompression de FFmpeg…")
        with zipfile.ZipFile(z) as zf:
            for name in zf.namelist():
                if name.endswith(("/bin/ffmpeg.exe", "/bin/ffprobe.exe")):
                    (P.bin / Path(name).name).write_bytes(zf.read(name))
        z.unlink(missing_ok=True)
        if not (P.bin / "ffmpeg.exe").exists():
            raise RuntimeError("FFmpeg introuvable dans l'archive téléchargée.")

    def put_code(self, source_zip: Path, prefix: str = "") -> None:
        """Remplace home/app par le code de l'archive (en gardant l'ancien en cas d'échec)."""
        new, old = P.home / "app.new", P.home / "app.old"
        shutil.rmtree(new, ignore_errors=True)
        with zipfile.ZipFile(source_zip) as zf:
            for info in zf.infolist():
                name = info.filename[len(prefix):] if info.filename.startswith(prefix) else None
                if not name or info.is_dir():
                    continue
                target = new / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(zf.read(info))
        if not (new / "pyproject.toml").exists():
            raise RuntimeError("Archive du code invalide.")
        shutil.rmtree(old, ignore_errors=True)
        if P.app.exists():
            retry(lambda: P.app.replace(old), "Le dossier de l'appli est encore utilisé : ferme CrispTrim et réessaie.")
        new.replace(P.app)

    def restore_code(self) -> None:
        old = P.home / "app.old"
        if old.exists():
            shutil.rmtree(P.app, ignore_errors=True)
            old.replace(P.app)

    def sync(self, label: str) -> None:
        self.run([str(P.uv), "sync", "--project", str(P.app), "--locked", "--extra", "asr", "--python", "3.12"],
                 label, watch=P.uvcache, expected=DEPS_BYTES)

    def get_model(self) -> None:
        code = ("from huggingface_hub import snapshot_download; "
                f"snapshot_download('{MODEL_REPO}', allow_patterns=['*.json', '*.txt', '*.safetensors'])")
        self.run([str(P.python_exe), "-c", code], "Téléchargement du modèle CrisperWhisper",
                 watch=P.models, expected=MODEL_BYTES)

    def write_state(self, version: str) -> None:
        P.state.write_text(json.dumps({"version": version, "installed": time.strftime("%Y-%m-%d %H:%M:%S")}),
                           encoding="utf-8")


# --------------------------------------------------------------------------- Windows : raccourcis et registre

UNINSTALL_KEY = rf"Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP}"


def powershell(script: str, **env) -> str:
    e = os.environ.copy()
    e.update({k: str(v) for k, v in env.items()})
    r = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script], env=e,
                       capture_output=True, text=True, creationflags=NO_WINDOW)
    return r.stdout.strip()


def shortcut_dirs() -> tuple[Path, Path]:
    out = powershell("[Environment]::GetFolderPath('Programs'); [Environment]::GetFolderPath('Desktop')")
    programs, desktop = (out.splitlines() + ["", ""])[:2]
    return Path(programs), Path(desktop)


def make_shortcut(lnk: Path) -> None:
    powershell(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:LNK); $s.TargetPath = $env:TGT; "
        "$s.WorkingDirectory = $env:WD; $s.IconLocation = $env:ICO; "
        "$s.Description = 'Coupe les silences et les euh de tes rushes'; $s.Save()",
        LNK=lnk, TGT=P.exe, WD=P.home, ICO=f"{P.ico},0",
    )


def create_shortcuts(desktop: bool) -> None:
    programs, desk = shortcut_dirs()
    if programs.is_dir():
        make_shortcut(programs / f"{APP}.lnk")
    if desktop and desk.is_dir():
        make_shortcut(desk / f"{APP}.lnk")


def remove_shortcuts() -> None:
    for d in shortcut_dirs():
        lnk = d / f"{APP}.lnk"
        if lnk.exists():
            lnk.unlink()


def register_uninstall(version: str) -> None:
    import winreg

    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        values = {
            "DisplayName": APP, "DisplayVersion": version, "Publisher": PUBLISHER,
            "DisplayIcon": str(P.ico), "InstallLocation": str(P.home),
            "UninstallString": f'"{P.exe}" --uninstall', "QuietUninstallString": f'"{P.exe}" --uninstall --yes',
            "URLInfoAbout": f"https://github.com/{REPO}", "HelpLink": f"https://github.com/{REPO}/issues",
        }
        for name, val in values.items():
            winreg.SetValueEx(k, name, 0, winreg.REG_SZ, val)
        winreg.SetValueEx(k, "NoModify", 0, winreg.REG_DWORD, 1)
        winreg.SetValueEx(k, "NoRepair", 0, winreg.REG_DWORD, 1)
        winreg.SetValueEx(k, "EstimatedSize", 0, winreg.REG_DWORD, int(9e6))  # Ko


def unregister_uninstall() -> None:
    import winreg

    try:
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY)
    except OSError:
        pass


# --------------------------------------------------------------------------- interface (tkinter)

BG, PANEL, BORDER = "#0b0d12", "#151922", "#262c39"
TEXT, MUTED, FAINT = "#e9ebf1", "#8b93a3", "#5d6575"
ACCENT, ACCENT_HI, OK, DANGER = "#7c6cff", "#8f81ff", "#3ddc97", "#ff5d6c"
FONT = "Segoe UI"


class Window:
    def __init__(self, title: str) -> None:
        import tkinter as tk
        from tkinter import ttk

        self.tk, self.ttk = tk, ttk
        try:
            import ctypes

            ctypes.windll.shcore.SetProcessDpiAwareness(1)
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("xLaugh.CrispTrim.Setup")
        except Exception:  # noqa: BLE001
            pass
        self.root = tk.Tk()
        self.root.title(title)
        self.root.configure(bg=BG)
        self.root.resizable(False, False)
        w, h = 600, 460
        x = (self.root.winfo_screenwidth() - w) // 2
        y = (self.root.winfo_screenheight() - h) // 3
        self.root.geometry(f"{w}x{h}+{x}+{y}")
        if (RES / "crisptrim.ico").exists():
            self.root.iconbitmap(default=str(RES / "crisptrim.ico"))
        self.dark_titlebar()
        style = ttk.Style(self.root)
        style.theme_use("clam")
        style.configure("C.Horizontal.TProgressbar", troughcolor=PANEL, background=ACCENT,
                        bordercolor=PANEL, lightcolor=ACCENT, darkcolor=ACCENT, thickness=8)
        self.logo = None
        if (RES / "crisptrim.png").exists():
            self.logo = tk.PhotoImage(file=str(RES / "crisptrim.png")).subsample(4, 4)
        self.body = tk.Frame(self.root, bg=BG)
        self.body.pack(fill="both", expand=True, padx=28, pady=24)

    def dark_titlebar(self) -> None:
        try:
            import ctypes

            self.root.update_idletasks()
            hwnd = ctypes.windll.user32.GetParent(self.root.winfo_id())
            on = ctypes.c_int(1)
            ctypes.windll.dwmapi.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(on), ctypes.sizeof(on))
        except Exception:  # noqa: BLE001
            pass

    # -- éléments

    def clear(self) -> None:
        for w in self.body.winfo_children():
            w.destroy()

    def header(self, title: str, subtitle: str) -> None:
        tk = self.tk
        row = tk.Frame(self.body, bg=BG)
        row.pack(fill="x", pady=(0, 18))
        if self.logo:
            tk.Label(row, image=self.logo, bg=BG).pack(side="left", padx=(0, 14))
        txt = tk.Frame(row, bg=BG)
        txt.pack(side="left", fill="x")
        tk.Label(txt, text=title, bg=BG, fg=TEXT, font=(FONT, 17, "bold"), anchor="w").pack(fill="x")
        tk.Label(txt, text=subtitle, bg=BG, fg=MUTED, font=(FONT, 10), anchor="w").pack(fill="x")

    def text(self, s: str, fg: str = MUTED, size: int = 10, pady=(0, 10), parent=None) -> None:
        self.tk.Label(parent or self.body, text=s, bg=BG if parent is None else PANEL, fg=fg,
                      font=(FONT, size), justify="left", anchor="w", wraplength=500 if parent else 540).pack(fill="x", pady=pady)

    def check(self, label: str, value: bool):
        var = self.tk.BooleanVar(value=value)
        self.tk.Checkbutton(self.body, text=label, variable=var, bg=BG, fg=TEXT, activebackground=BG,
                            activeforeground=TEXT, selectcolor=PANEL, font=(FONT, 10), anchor="w",
                            highlightthickness=0, bd=0).pack(fill="x", pady=2)
        return var

    def button(self, parent, label: str, cmd, primary: bool = False):
        b = self.tk.Button(parent, text=label, command=cmd, relief="flat", bd=0, cursor="hand2",
                           font=(FONT, 10, "bold" if primary else "normal"), padx=18, pady=8,
                           bg=ACCENT if primary else PANEL, fg="#ffffff" if primary else TEXT,
                           activebackground=ACCENT_HI if primary else BORDER, activeforeground="#ffffff")
        b.pack(side="right", padx=(8, 0))
        return b

    def footer(self):
        f = self.tk.Frame(self.body, bg=BG)
        f.pack(side="bottom", fill="x")
        return f

    def card(self):
        c = self.tk.Frame(self.body, bg=PANEL, highlightbackground=BORDER, highlightthickness=1)
        c.pack(fill="x", pady=(4, 14))
        inner = self.tk.Frame(c, bg=PANEL)
        inner.pack(fill="x", padx=14, pady=12)
        return inner


class StepsView:
    """Liste d'étapes avec coches, barre de progression et détail."""

    def __init__(self, win: Window, steps: list[str]) -> None:
        tk, ttk = win.tk, win.ttk
        self.win = win
        self.labels = []
        box = win.card()
        for s in steps:
            lab = tk.Label(box, text=f"○  {s}", bg=PANEL, fg=FAINT, font=(FONT, 10), anchor="w")
            lab.pack(fill="x", pady=1)
            self.labels.append((lab, s))
        self.bar = ttk.Progressbar(win.body, style="C.Horizontal.TProgressbar", mode="determinate", maximum=1000)
        self.bar.pack(fill="x", pady=(2, 8))
        self.detail = tk.Label(win.body, text="", bg=BG, fg=MUTED, font=(FONT, 9), justify="left",
                               anchor="nw", wraplength=540, height=3)
        self.detail.pack(fill="x")
        self.indeterminate = False

    def step(self, i: int, state: str) -> None:
        lab, s = self.labels[i]
        icon, color = {"run": ("●", TEXT), "done": ("✓", OK), "skip": ("–", FAINT), "error": ("✕", DANGER)}[state]
        lab.config(text=f"{icon}  {s}", fg=color)
        if state == "run":
            self.set(0)

    def set(self, frac) -> None:
        if frac is None:
            if not self.indeterminate:
                self.bar.config(mode="indeterminate")
                self.bar.start(15)
                self.indeterminate = True
            return
        if self.indeterminate:
            self.bar.stop()
            self.bar.config(mode="determinate")
            self.indeterminate = False
        self.bar["value"] = int(frac * 1000)


# --------------------------------------------------------------------------- écrans


class Setup:
    def __init__(self, args: list[str]) -> None:
        self.args = args
        self.auto = "--yes" in args
        self.version = payload_version()
        self.installed = read_state().get("version") if is_installed() else None
        self.win = Window(f"Installation de {APP}")
        self.job: Job | None = None
        self.win.root.protocol("WM_DELETE_WINDOW", self.close)
        self.welcome()
        if self.auto:
            self.win.root.after(300, self.start)

    def close(self) -> None:
        if self.job and not getattr(self, "finished", False):
            from tkinter import messagebox

            if not messagebox.askyesno(APP, "Arrêter l'installation ? Tu pourras la reprendre plus tard."):
                return
            self.job.cancel()
        self.win.root.destroy()

    def welcome(self) -> None:
        w = self.win
        w.clear()
        if self.installed and vtuple(self.installed) >= vtuple(self.version):
            w.header(f"{APP} est installé", f"Version {self.installed}")
            w.text("Tu peux le lancer, ou le réparer si quelque chose ne marche plus "
                   "(les fichiers manquants sont retéléchargés).")
            f = w.footer()
            w.button(f, f"Lancer {APP}", self.launch_and_quit, primary=True)
            w.button(f, "Réparer", self.start)
            return
        if self.installed:
            w.header(f"Mettre à jour {APP}", f"Version {self.installed} → {self.version}")
            w.text("Seul le code change : la mise à jour prend quelques secondes.")
        else:
            w.header(f"Installer {APP}", f"Version {self.version} · par {PUBLISHER}")
            w.text("Coupe automatiquement les silences et les « euh » de tes rushes, en local.", fg=TEXT)
            c = w.card()
            w.text(f"Dossier : {P.home}", parent=c, pady=(0, 4))
            w.text("Téléchargements : PyTorch (~3 Go), modèle de transcription (~3 Go), FFmpeg. "
                   "Compte 10 à 20 minutes selon ta connexion, et ~10 Go d'espace disque.", parent=c, pady=0)
        self.desktop = w.check("Créer un raccourci sur le Bureau", "--no-desktop" not in self.args)
        self.model = w.check("Télécharger le modèle de transcription maintenant (recommandé)",
                             "--no-model" not in self.args)
        w.text("CrispTrim est un logiciel libre (GNU GPL v3). Le modèle CrisperWhisper de Nyra Health "
               "est réservé à un usage non commercial.", fg=FAINT, size=9, pady=(10, 0))
        f = w.footer()
        w.button(f, "Installer" if not self.installed else "Mettre à jour", self.start, primary=True)
        w.button(f, "Annuler", self.win.root.destroy)

    def start(self) -> None:
        desktop = getattr(self, "desktop", None)
        model = getattr(self, "model", None)
        want_desktop = desktop.get() if desktop else False
        want_model = model.get() if model else False
        w = self.win
        w.clear()
        w.header(f"Installation de {APP}", f"Version {self.version}")
        steps = ["Préparation", "uv (gestionnaire Python)", "FFmpeg", f"Code de {APP}",
                 "Python, PyTorch et dépendances", "Modèle de transcription", "Raccourcis et désinstallation"]
        view = StepsView(w, steps)
        f = w.footer()
        self.cancel_btn = w.button(f, "Annuler", self.close)
        job = self.job = Job()

        def work():
            plan = [
                job.prepare, job.get_uv, job.get_ffmpeg,
                lambda: job.put_code(RES / "payload.zip"),
                lambda: job.sync("Installation de Python, PyTorch et des dépendances"),
                job.get_model if want_model else None,
                lambda: (create_shortcuts(want_desktop), register_uninstall(self.version)),
            ]
            for i, fn in enumerate(plan):
                if fn is None:
                    job.emit("step", i, "skip")
                    continue
                job.emit("step", i, "run")
                try:
                    fn()
                except Cancelled:
                    job.emit("cancelled")
                    return
                except Exception as exc:  # noqa: BLE001
                    traceback.print_exc()
                    job.emit("step", i, "error")
                    job.emit("error", str(exc))
                    return
                job.emit("step", i, "done")
            job.write_state(self.version)
            job.emit("done")

        threading.Thread(target=work, daemon=True).start()
        self.pump(view)

    def pump(self, view: StepsView) -> None:
        try:
            while True:
                msg = self.job.q.get_nowait()
                kind = msg[0]
                if kind == "step":
                    view.step(msg[1], msg[2])
                elif kind == "progress":
                    view.set(msg[1])
                    view.detail.config(text=msg[2], fg=MUTED)
                elif kind == "error":
                    self.failed(view, msg[1])
                    return
                elif kind == "cancelled":
                    self.win.root.destroy()
                    return
                elif kind == "done":
                    self.done()
                    return
        except queue.Empty:
            pass
        self.win.root.after(120, lambda: self.pump(view))

    def failed(self, view: StepsView, err: str) -> None:
        view.set(0)
        view.detail.config(text=err, fg=DANGER)
        self.finished = True
        self.cancel_btn.destroy()
        f = self.win.body.winfo_children()[-1]
        self.win.button(f, "Réessayer", self.start, primary=True)
        self.win.button(f, "Fermer", self.win.root.destroy)
        if self.auto:
            print(err, file=sys.stderr)
            self.win.root.after(500, lambda: (self.win.root.destroy(), sys.exit(1)))

    def done(self) -> None:
        self.finished = True
        w = self.win
        w.clear()
        w.header(f"{APP} est installé", f"Version {self.version}")
        w.text("Tu le trouves dans le menu Démarrer" + (" et sur le Bureau." if getattr(self, "desktop", None)
               and self.desktop.get() else "."), fg=TEXT)
        w.text("Pour le désinstaller : Paramètres > Applications > CrispTrim.")
        f = w.footer()
        w.button(f, f"Lancer {APP}", self.launch_and_quit, primary=True)
        w.button(f, "Fermer", w.root.destroy)
        if self.auto:
            w.root.after(300, w.root.destroy)

    def launch_and_quit(self) -> None:
        launch_app()
        self.win.root.destroy()

    def mainloop(self) -> None:
        self.win.root.mainloop()


class Update:
    """Met à jour le code vers une release GitHub, puis relance l'appli."""

    def __init__(self, tag: str, pid: int = 0) -> None:
        self.tag, self.pid = tag, pid
        self.win = Window(f"Mise à jour de {APP}")
        self.win.root.protocol("WM_DELETE_WINDOW", lambda: None)  # pas d'interruption au milieu
        self.win.header(f"Mise à jour de {APP}", f"Version {code_version()} → {tag.lstrip('v')}")
        self.view = StepsView(self.win, ["Fermeture de l'appli", "Téléchargement du code",
                                         "Mise à jour des dépendances", "Redémarrage"])
        self.job = Job()
        threading.Thread(target=self.work, daemon=True).start()
        self.pump()

    def work(self) -> None:
        job, i = self.job, 0
        try:
            job.emit("step", 0, "run")
            job.emit("progress", None, "Attente de la fermeture de CrispTrim…")
            if self.pid:
                wait_pid(self.pid)
            for _ in range(60):  # l'appli se ferme d'elle-même
                if not port_busy():
                    break
                time.sleep(0.5)
            time.sleep(1)
            job.emit("step", 0, "done")
            i = 1
            job.emit("step", 1, "run")
            z = Path(tempfile.gettempdir()) / f"crisptrim-{self.tag}.zip"
            job.download(f"https://github.com/{REPO}/archive/refs/tags/{self.tag}.zip", z, "Code")
            with zipfile.ZipFile(z) as zf:
                prefix = zf.namelist()[0].split("/")[0] + "/"
            job.put_code(z, prefix)
            z.unlink(missing_ok=True)
            job.emit("step", 1, "done")
            i = 2
            job.emit("step", 2, "run")
            job.get_uv()
            job.sync("Mise à jour des dépendances")
            job.emit("step", 2, "done")
            i = 3
            job.emit("step", 3, "run")
            version = self.tag.lstrip("v")
            job.write_state(version)
            try:
                register_uninstall(version)
            except OSError:
                pass
            launch_app()
            job.emit("step", 3, "done")
            job.emit("done")
        except Exception as exc:  # noqa: BLE001
            traceback.print_exc()
            if i >= 2:
                job.restore_code()
            job.emit("step", i, "error")
            job.emit("error", f"{exc}\n\nL'ancienne version a été gardée.")

    def pump(self) -> None:
        try:
            while True:
                msg = self.job.q.get_nowait()
                if msg[0] == "step":
                    self.view.step(msg[1], msg[2])
                elif msg[0] == "progress":
                    self.view.set(msg[1])
                    self.view.detail.config(text=msg[2])
                elif msg[0] == "done":
                    self.win.root.after(800, self.win.root.destroy)
                    return
                elif msg[0] == "error":
                    self.view.set(0)
                    self.view.detail.config(text=msg[1], fg=DANGER)
                    self.win.root.protocol("WM_DELETE_WINDOW", self.win.root.destroy)
                    f = self.win.footer()
                    self.win.button(f, f"Relancer {APP}", lambda: (launch_app(), self.win.root.destroy()), primary=True)
                    return
        except queue.Empty:
            pass
        self.win.root.after(120, self.pump)

    def mainloop(self) -> None:
        self.win.root.mainloop()


def uninstall(auto: bool) -> int:
    from tkinter import Tk, messagebox

    root = Tk()
    root.withdraw()
    if (RES / "crisptrim.ico").exists():
        root.iconbitmap(default=str(RES / "crisptrim.ico"))
    size = dir_size(P.home) / 1e9 if P.home.exists() else 0
    if not auto and not messagebox.askyesno(
        f"Désinstaller {APP}",
        f"Désinstaller {APP} ?\n\nL'application, le modèle et le cache seront supprimés ({size:.1f} Go).\n"
        "Tes vidéos et tes exports ne sont pas touchés.",
    ):
        return 0
    log = open(Path(tempfile.gettempdir()) / f"{APP}-uninstall.log", "w", encoding="utf-8")
    running = app_processes()
    print("processus ouverts :", running, file=log, flush=True)
    if running:
        if not auto and not messagebox.askyesno(APP, f"{APP} est ouvert. Le fermer et continuer ?"):
            return 1
        for pid in running:
            subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True, creationflags=NO_WINDOW)
        for _ in range(20):  # attend qu'ils aient vraiment disparu
            time.sleep(0.5)
            if not app_processes():
                break
    os.chdir(tempfile.gettempdir())
    remove_shortcuts()
    unregister_uninstall()
    # Tout sauf ce programme, qui tourne : supprimé tout de suite.
    for item in P.home.iterdir():
        if item.name.lower() == P.exe.name.lower():
            continue
        try:
            shutil.rmtree(item) if item.is_dir() else item.unlink()
        except OSError as exc:
            print("reste :", item, exc, file=log, flush=True)
    print("terminé", file=log, flush=True)
    if not auto:
        messagebox.showinfo(APP, f"{APP} a été désinstallé.")
    # Ce programme et ce qui résiste : supprimés juste après sa fermeture (10 essais).
    home = str(P.home)
    script = (f'for /l %i in (1,1,10) do (if exist "{home}" (ping 127.0.0.1 -n 3 > nul '
              f'& rmdir /s /q "{home}"))')
    out = open(Path(tempfile.gettempdir()) / f"{APP}-uninstall-cmd.log", "w")
    spawn_detached(f"cmd /c {script}", NO_WINDOW, stdout=out, stderr=subprocess.STDOUT,
                   stdin=subprocess.DEVNULL, cwd=tempfile.gettempdir())
    return 0


# --------------------------------------------------------------------------- entrée


def main() -> int:
    args = sys.argv[1:]
    # Ne garde aucun dossier de l'installation ouvert (Windows interdit alors de le renommer).
    os.chdir(P.home if P.home.exists() else tempfile.gettempdir())
    if "--version" in args:
        print(payload_version())
        return 0
    if "--uninstall" in args:
        return uninstall("--yes" in args)
    if "--update" in args:
        i = args.index("--update")
        tag = args[i + 1] if i + 1 < len(args) else ""
        if not TAG_RE.match(tag):
            return 2
        pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
        Update(tag, pid).mainloop()
        return 0
    from_home = getattr(sys, "frozen", False) and Path(sys.executable).resolve() == P.exe.resolve()
    if ("--launch" in args or from_home) and is_installed():
        launch_app()
        return 0
    s = Setup(args)
    s.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
