"""Draw app-icon.png (1024 px): a sage tile with a plain camera and a four-point sparkle.

Run `python scripts/draw_app_icon.py` and then `python scripts/make_icon.py`.
Only basic shapes: no picture from any game or product.
"""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SIZE = 1024
SCALE = 4  # draw large, then downsample for smooth edges


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))  # type: ignore[return-value]


def sparkle(draw: ImageDraw.ImageDraw, cx: float, cy: float, r: float, fill: tuple[int, int, int, int]) -> None:
    """Four-point star: long points up/down/left/right, narrow waist."""
    w = r * 0.28
    pts = [
        (cx, cy - r), (cx + w, cy - w), (cx + r, cy), (cx + w, cy + w),
        (cx, cy + r), (cx - w, cy + w), (cx - r, cy), (cx - w, cy - w),
    ]
    draw.polygon(pts, fill=fill)


def main() -> None:
    big = SIZE * SCALE
    top, bottom = (0x74, 0x9C, 0x82), (0x3B, 0x5C, 0x4B)
    gradient = Image.new("RGBA", (big, big))
    gd = ImageDraw.Draw(gradient)
    for y in range(big):
        gd.line([(0, y), (big, y)], fill=(*lerp(top, bottom, y / big), 255))

    mask = Image.new("L", (big, big), 0)
    inset = round(big * 0.06)
    ImageDraw.Draw(mask).rounded_rectangle([inset, inset, big - inset, big - inset], radius=round(big * 0.22), fill=255)
    tile = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    tile.paste(gradient, (0, 0), mask)

    d = ImageDraw.Draw(tile)
    u = big / 100
    outline = (0x2A, 0x36, 0x31, 255)
    body = (0xF6, 0xF4, 0xEC, 255)
    lens_ring = (0x56, 0x7E, 0x65, 255)
    lens = (0x24, 0x30, 0x2C, 255)
    stroke = round(3.2 * u)

    # camera body with a top bump (viewfinder)
    d.rounded_rectangle([34 * u, 33 * u, 50 * u, 42 * u], radius=round(3 * u), fill=body, outline=outline, width=stroke)
    d.rounded_rectangle([18 * u, 38 * u, 82 * u, 78 * u], radius=round(9 * u), fill=body, outline=outline, width=stroke)
    # lens
    d.ellipse([36 * u, 44 * u, 64 * u, 72 * u], fill=lens_ring, outline=outline, width=stroke)
    d.ellipse([43 * u, 51 * u, 57 * u, 65 * u], fill=lens)
    d.ellipse([46 * u, 53.5 * u, 50.5 * u, 58 * u], fill=(255, 255, 255, 230))
    # shutter button
    d.rounded_rectangle([67 * u, 43 * u, 75 * u, 48 * u], radius=round(1.5 * u), fill=outline)

    # sparkles, top right
    sparkle(d, 76 * u, 24 * u, 11 * u, (0xFF, 0xF3, 0xB8, 255))
    sparkle(d, 88 * u, 38 * u, 5 * u, (0xFF, 0xFF, 0xFF, 235))

    out = tile.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    out.save(ROOT / "app-icon.png")
    print(f"wrote {ROOT / 'app-icon.png'}")


if __name__ == "__main__":
    main()
