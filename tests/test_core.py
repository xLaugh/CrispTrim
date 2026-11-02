from fractions import Fraction
from pathlib import Path
from xml.etree import ElementTree as ET

import numpy as np

from crisptrim.cutter import CutParams, compute_cuts, to_frames
from crisptrim.export import write_fcpxml, write_premiere_xml
from crisptrim.fillers import is_cuttable, is_filler
from crisptrim.intervals import invert, merge, subtract
from crisptrim.media import MediaInfo
from crisptrim.silence import SilenceParams, detect_speech
from crisptrim.transcribe import Word

SR = 16000


def test_intervals():
    assert merge([(0, 1), (0.9, 2), (3, 4)]) == [(0, 2), (3, 4)]
    assert subtract([(0, 10)], [(2, 3), (5, 6)]) == [(0, 2), (3, 5), (6, 10)]
    assert invert([(1, 2)], 0, 3) == [(0, 1), (2, 3)]


def test_fillers_french():
    for w in ["euh", "Euh,", "euuuh", "heu", "hum", "hmm", "mmh", "[UH]", "[um]", "euhm"]:
        assert is_filler(w), w
    for w in ["eu", "bah", "ben", "hein", "humain", "eux", "un", "mais", "[laughter]", "homme"]:
        assert not is_filler(w), w
    assert is_cuttable("[yawn]") and is_cuttable("[cough]")
    assert not is_cuttable("[yawn]", cut_noises=False)
    assert not is_cuttable("[laughter]") and not is_cuttable("[music]")


def _tone(sec):
    t = np.arange(int(sec * SR)) / SR
    return (0.3 * np.sin(2 * np.pi * 220 * t)).astype(np.float32)


def _noise(sec, level=0.001):
    return (np.random.default_rng(0).standard_normal(int(sec * SR)) * level).astype(np.float32)


def test_silence_detection():
    # 1 s parole, 2 s silence, 1 s parole, 0.3 s pause (gardée), 1 s parole
    audio = np.concatenate([_tone(1), _noise(2), _tone(1), _noise(0.3), _tone(1)])
    speech, thr = detect_speech(audio, SR, SilenceParams(min_silence=0.5))
    assert len(speech) == 2
    assert abs(speech[0][0] - 0) < 0.02 and abs(speech[0][1] - 1) < 0.02
    assert abs(speech[1][0] - 3) < 0.02 and abs(speech[1][1] - 5.3) < 0.02


def test_filler_cut_removes_euh_and_pause():
    # « bonjour [pause] euh [pause] je suis là »
    speech = [(0.0, 5.0)]
    words = [Word("bonjour", 0.0, 0.6), Word("euh", 1.2, 1.6), Word("je", 2.4, 2.6), Word("suis", 2.6, 2.9),
             Word("là", 2.9, 3.2)]
    res = compute_cuts(5.0, speech, words, CutParams(margin=0.15))
    assert len(res.fillers) == 1
    # Coupe de 0.75 (0.6 + marge) à 2.25 (2.4 - marge)
    assert res.keep[0] == (0.0, 0.75)
    assert res.keep[1][0] == 2.25
    assert any(r.kind == "filler" and r.text == "euh" for r in res.regions)


def test_to_frames_ntsc():
    fps = Fraction(30000, 1001)
    frames = to_frames([(0.0, 1.0), (1.01, 2.0)], fps, 100)
    assert frames == [(0, 60)]  # les deux segments se touchent une fois alignés sur les images


def _info(tmp: Path, fps=Fraction(30000, 1001), video=True) -> MediaInfo:
    p = tmp / "rush à tester.mp4"
    p.write_bytes(b"")
    return MediaInfo(p, 10.0, video, fps, 1920, 1080, True, 2, 48000)


def test_fcpxml_valid(tmp_path):
    info = _info(tmp_path)
    out = tmp_path / "t.fcpxml"
    write_fcpxml(info, [(0, 30), (60, 120)], out, "test")
    root = ET.parse(out).getroot()
    clips = root.findall(".//asset-clip")
    assert [c.get("offset") for c in clips] == ["0s", "30030/30000s"]
    assert clips[1].get("start") == "60060/30000s"
    assert root.find(".//media-rep").get("src").startswith("file:///")


def test_premiere_xml_valid(tmp_path):
    info = _info(tmp_path, fps=Fraction(25))
    out = tmp_path / "t.xml"
    write_premiere_xml(info, [(0, 25), (50, 100)], out, "test")
    root = ET.parse(out).getroot()
    v = root.findall(".//video/track/clipitem")
    a = root.findall(".//audio/track")
    assert len(v) == 2 and len(a) == 2
    assert [(c.findtext("start"), c.findtext("end"), c.findtext("in"), c.findtext("out")) for c in v] == [
        ("0", "25", "0", "25"), ("25", "75", "50", "100")]
    assert "C%3A" in root.findtext(".//pathurl") or root.findtext(".//pathurl").startswith("file://localhost/")
