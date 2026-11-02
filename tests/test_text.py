from crisptrim.text import kept_words, retime, subtitle_cues, write_srt, write_txt
from crisptrim.transcribe import Word

KEEP = [(0.0, 2.0), (5.0, 8.0)]  # 2 s gardées, 3 s coupées, 3 s gardées
WORDS = [
    Word("Bonjour", 0.2, 0.7), Word("à", 0.7, 0.8), Word("tous.", 0.8, 1.3),
    Word("[UH]", 3.0, 3.5),
    Word("On", 5.2, 5.4), Word("commence", 5.4, 6.0), Word("maintenant.", 6.0, 6.8),
]


def test_retime():
    assert retime(1.0, KEEP) == 1.0
    assert retime(3.0, KEEP) == 2.0      # dans une coupe : collé au segment suivant
    assert retime(6.0, KEEP) == 3.0


def test_kept_words_drop_fillers():
    assert [w.word for w in kept_words(WORDS, KEEP)] == ["Bonjour", "à", "tous.", "On", "commence", "maintenant."]


def test_srt_retimed(tmp_path):
    cues = subtitle_cues(WORDS, KEEP)
    assert [c[2] for c in cues] == ["Bonjour à tous.", "On commence maintenant."]
    assert abs(cues[1][0] - 2.2) < 1e-6  # 5.2 s d'origine → 2.2 s dans le montage
    out = tmp_path / "t.srt"
    write_srt(WORDS, KEEP, out)
    text = out.read_text(encoding="utf-8")
    assert "00:00:02,200 --> " in text and "[UH]" not in text


def test_txt(tmp_path):
    out = tmp_path / "t.txt"
    write_txt(WORDS, KEEP, out)
    assert out.read_text(encoding="utf-8").startswith("[00:00] Bonjour à tous.")
