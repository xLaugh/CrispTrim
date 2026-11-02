"""Génère l'icône de CrispTrim (installer/crisptrim.ico) : dégradé violet → bleu et ciseaux blancs.

    uv run --with pillow python installer/make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).parent
S = 1024  # dessin en grand, puis réduction propre


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def gradient() -> Image.Image:
    stops = [(0.0, (139, 123, 255)), (0.55, (106, 92, 255)), (1.2, (79, 168, 255))]
    img = Image.new("RGB", (S, S))
    px = img.load()
    for y in range(S):
        for x in range(S):
            t = (x + y) / (2 * S)  # diagonale, comme le CSS (135deg)
            for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
                if t <= t1:
                    px[x, y] = lerp(c0, c1, (t - t0) / (t1 - t0))
                    break
    return img


def main() -> None:
    bg = gradient()
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, S - 1, S - 1), radius=230, fill=255)
    icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    icon.paste(bg, (0, 0), mask)

    font = ImageFont.truetype("C:/Windows/Fonts/seguisym.ttf", 640)
    glyph = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(glyph)
    box = d.textbbox((0, 0), "✂", font=font)
    w, h = box[2] - box[0], box[3] - box[1]
    d.text(((S - w) / 2 - box[0], (S - h) / 2 - box[1]), "✂", font=font, fill=(255, 255, 255, 255))
    icon.alpha_composite(glyph)

    icon.resize((256, 256), Image.LANCZOS).save(HERE / "crisptrim.png")
    sizes = [(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)]
    icon.save(HERE / "crisptrim.ico", sizes=sizes)
    icon.save(HERE.parent / "crisptrim" / "app" / "static" / "crisptrim.ico", sizes=sizes)  # fenêtre et onglet
    print("ok", HERE / "crisptrim.ico")


if __name__ == "__main__":
    main()
