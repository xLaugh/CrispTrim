"""Ligne de commande : crisptrim rush1.mp4 rush2.mov -o sortie/"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

from . import __version__
from .cutter import CutParams, compute_cuts, to_frames
from .export import render_mp4, write_fcpxml, write_json, write_premiere_xml, write_vegas_edl
from .media import SAMPLE_RATE, load_audio, probe, require_ffmpeg
from .silence import SilenceParams, detect_speech
from .text import write_srt, write_txt

EXPORTS = ("fcpxml", "premiere", "vegas", "mp4", "json", "txt", "srt")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="crisptrim",
        description="Coupe les silences et les hésitations (« euh ») de tes rushes, puis exporte une timeline.",
    )
    p.add_argument("inputs", nargs="+", type=Path, help="fichiers vidéo ou audio")
    p.add_argument("-o", "--output", type=Path, default=None, help="dossier de sortie (défaut : à côté du fichier)")
    p.add_argument("--export", default="fcpxml,premiere,json",
                   help=f"formats séparés par des virgules parmi {','.join(EXPORTS)} (défaut : fcpxml,premiere,json)")

    g = p.add_argument_group("silences")
    g.add_argument("--threshold-db", type=float, default=None, help="seuil de silence en dBFS (défaut : automatique)")
    g.add_argument("--min-silence", type=float, default=0.5, help="durée minimale d'un silence coupé, en s (0.5)")
    g.add_argument("--margin", type=float, default=0.15, help="marge gardée autour de la parole, en s (0.15)")
    g.add_argument("--min-keep", type=float, default=0.25, help="segment gardé minimal, en s (0.25)")

    g = p.add_argument_group("hésitations (CrisperWhisper)")
    g.add_argument("--no-fillers", action="store_true", help="ne pas transcrire : coupe seulement les silences")
    g.add_argument("--model", default="large", help="large | turbo | medium | small (défaut : large)")
    g.add_argument("--language", default="fr", help="code langue (défaut : fr)")
    g.add_argument("--keep-noises", action="store_true", help="garder les bruits de bouche ([yawn], [cough]…)")
    g.add_argument("--extra-filler", action="append", default=[], help="mot supplémentaire à couper (répétable)")
    g.add_argument("--device", default="auto", help="auto | cuda | cpu")
    p.add_argument("--version", action="version", version=f"crisptrim {__version__}")
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    exports = {e.strip() for e in args.export.split(",") if e.strip()}
    unknown = exports - set(EXPORTS)
    if unknown:
        print(f"Export inconnu : {', '.join(sorted(unknown))}", file=sys.stderr)
        return 2
    require_ffmpeg()

    sil = SilenceParams(threshold_db=args.threshold_db, min_silence=args.min_silence)
    cut = CutParams(
        margin=args.margin, min_keep=args.min_keep, cut_fillers=not args.no_fillers,
        cut_noises=not args.keep_noises,
        extra_fillers={w.lower() for w in args.extra_filler},
    )
    transcriber = None
    if not args.no_fillers:
        from .transcribe import Transcriber
        transcriber = Transcriber(args.model, args.language, args.device)

    for src in args.inputs:
        if not src.exists():
            print(f"Introuvable : {src}", file=sys.stderr)
            continue
        t0 = time.time()
        info = probe(src)
        if not info.has_audio:
            print(f"{src.name} : pas de piste audio, ignoré.", file=sys.stderr)
            continue
        outdir = args.output or info.path.parent
        outdir.mkdir(parents=True, exist_ok=True)
        print(f"▶ {info.path.name}  ({info.duration:.1f} s, {float(info.fps):.3f} i/s)")

        audio = load_audio(info.path)
        speech, thr = detect_speech(audio, SAMPLE_RATE, sil)
        print(f"  seuil de silence : {thr:.1f} dBFS")

        words = None
        if transcriber is not None:
            print(f"  transcription CrisperWhisper ({args.model}, {args.language})…")
            words = transcriber.transcribe(audio, info.path, cache_dir=outdir / ".crisptrim-cache")

        result = compute_cuts(info.duration, speech, words, cut)
        frames = to_frames(result.keep, info.fps, info.total_frames)
        if not frames:
            print("  rien à garder (seuil trop haut ?)", file=sys.stderr)
            continue

        stem = outdir / f"{info.path.stem}_crisptrim"
        name = f"{info.path.stem} (CrispTrim)"
        if "fcpxml" in exports:
            write_fcpxml(info, frames, stem.with_suffix(".fcpxml"), name)
        if "premiere" in exports:
            write_premiere_xml(info, frames, stem.with_suffix(".xml"), name)
        if "vegas" in exports:
            write_vegas_edl(info, frames, stem.with_suffix(".vegas.txt"))
        if "json" in exports:
            write_json(info, result, words, thr, stem.with_suffix(".json"))
        if {"txt", "srt"} & exports:
            if not words:
                print("  .txt/.srt ignorés : pas de transcription (--no-fillers).", file=sys.stderr)
            else:
                aligned = [(float(a / info.fps), float(b / info.fps)) for a, b in frames]
                if "txt" in exports:
                    write_txt(words, aligned, stem.with_suffix(".txt"))
                if "srt" in exports:
                    write_srt(words, aligned, stem.with_suffix(".srt"))
        if "mp4" in exports:
            print("  rendu MP4…")
            render_mp4(info, frames, stem.with_suffix(".mp4"))

        s = result.stats()
        print(
            f"  ✓ {s['duration']:.1f} s → {s['kept']:.1f} s  (−{s['removed_pct']} %, "
            f"{s['segments']} segments, {s['fillers']} hésitations/bruits)  en {time.time() - t0:.1f} s"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
