"""Test de bout en bout (sans modèle) : nécessite ffmpeg."""
import shutil
import subprocess

import pytest

from crisptrim.cli import main
from crisptrim.media import probe

pytestmark = pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="ffmpeg absent")


def test_cli_silences_only(tmp_path):
    src = tmp_path / "t.mp4"
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25:duration=6",
        "-f", "lavfi", "-i",
        "aevalsrc='0.3*sin(2*PI*220*t)*(between(t,0,2)+between(t,4,6))':s=48000:d=6:c=stereo",
        "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-shortest", str(src),
    ], check=True)
    assert main([str(src), "--no-fillers", "--export", "fcpxml,premiere,vegas,json,mp4", "-o", str(tmp_path)]) == 0
    for ext in ("fcpxml", "xml", "json", "mp4"):
        assert (tmp_path / f"t_crisptrim.{ext}").exists()
    # 2 s + 2 s de parole + marges (0.15 s de chaque côté du silence) ≈ 4.3 s
    assert abs(probe(tmp_path / "t_crisptrim.mp4").duration - 4.3) < 0.15


def test_vegas_edl(tmp_path):
    from fractions import Fraction

    from crisptrim.export import write_vegas_edl
    from crisptrim.media import MediaInfo

    info = MediaInfo(path=tmp_path / "Vidéo été.mp4", duration=10.0, has_video=True, fps=Fraction(25),
                     width=1920, height=1080, has_audio=True, audio_channels=2, audio_rate=48000)
    out = tmp_path / "t.vegas.txt"
    write_vegas_edl(info, [(0, 50), (100, 150)], out)
    raw = out.read_bytes()
    assert raw.startswith(b"\xef\xbb\xbf") and b"\r\n" in raw
    rows = [line.split(";") for line in raw.decode("utf-8-sig").splitlines()]
    assert len(rows[0]) == 28 and all(len(r) == 28 for r in rows)
    v2 = rows[3]  # 2e segment, piste vidéo
    assert v2[1] == "1" and v2[10] == "VIDEO" and v2[2] == "2000.0000" and v2[13] == "4000.0000"
    assert rows[4][1] == "2" and rows[4][10] == "AUDIO" and "Vidéo été.mp4" in rows[4][11]


def test_mp4_falls_back_without_gpu(tmp_path, monkeypatch):
    """Encodeur matériel listé mais inutilisable (pas de carte NVIDIA) : repli sur libx264."""
    from crisptrim import export

    src = tmp_path / "s.mp4"
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x120:rate=25:duration=2",
        "-f", "lavfi", "-i", "sine=f=440:d=2", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac",
        "-shortest", str(src),
    ], check=True)
    monkeypatch.setattr(export, "_video_encoder", lambda: ["-c:v", "h264_absent_sans_gpu"])
    out = tmp_path / "o.mp4"
    export.render_mp4(probe(src), [(0, 25)], out)
    assert abs(probe(out).duration - 1.0) < 0.1
