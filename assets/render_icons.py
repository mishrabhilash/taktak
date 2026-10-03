#!/usr/bin/env python3
"""Renders TakTak's icons with Pillow, from the same geometry as assets/icon.svg.

Outputs:
  assets/icon.png                        1024 px app icon (input to `npx tauri icon`)
  src-tauri/icons/tray-template.png      18 px macOS menu-bar template (black on transparent)
  src-tauri/icons/tray-template@2x.png   36 px
  src-tauri/icons/tray-color.png         32 px colored tray icon (Windows/Linux)
  src-tauri/icons/tray-color@2x.png      64 px

Then regenerate the bundle icons:  npx tauri icon assets/icon.png
(and delete the android/ and ios/ folders it creates: TakTak is desktop only).
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SS = 4  # supersampling factor

SKIRT = ((0xE8, 0x69, 0x2F), (0xBF, 0x4A, 0x17))
TOP = ((0xFF, 0xA3, 0x6A), (0xFF, 0x80, 0x40))


def mask(size, rects):
    """An L mask with the given rounded rectangles (x, y, w, h, r in canvas units) filled."""
    m = Image.new("L", (size * SS, size * SS), 0)
    d = ImageDraw.Draw(m)
    for x, y, w, h, r in rects:
        d.rounded_rectangle(
            [x * SS, y * SS, (x + w) * SS - 1, (y + h) * SS - 1], radius=r * SS, fill=255
        )
    return m


def gradient(size, top, bottom, y0, y1):
    """A vertical gradient from `top` at y0 to `bottom` at y1 (canvas units)."""
    n = size * SS
    col = Image.new("RGB", (1, n))
    for y in range(n):
        t = min(max((y / SS - y0) / (y1 - y0), 0.0), 1.0)
        col.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return col.resize((n, n))


def keycap(size, skirt, top, legend, shadow=None, highlight=0):
    """Composites a keycap: optional drop shadow, skirt, top face, highlight ring, T legend.

    Each shape is (x, y, w, h, r) in a `size` canvas; `legend` is a list of such shapes.
    """
    n = size * SS
    out = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    if shadow:
        (dx, dy, blur, alpha) = shadow
        m = mask(size, [skirt])
        m = ImageChops.offset(m, dx * SS, dy * SS).filter(ImageFilter.GaussianBlur(blur * SS))
        black = Image.new("RGBA", (n, n), (0, 0, 0, 255))
        out.paste(black, (0, 0), m.point(lambda v: round(v * alpha)))
    out.paste(gradient(size, *SKIRT, skirt[1], skirt[1] + skirt[3]), (0, 0), mask(size, [skirt]))
    out.paste(gradient(size, *TOP, top[1], top[1] + top[3]), (0, 0), mask(size, [top]))
    if highlight:
        x, y, w, h, r = top
        ring = ImageChops.subtract(
            mask(size, [top]),
            mask(size, [(x + highlight, y + highlight, w - 2 * highlight, h - 2 * highlight,
                         r - highlight)]),
        )
        white = Image.new("RGBA", (n, n), (255, 255, 255, 255))
        out.paste(white, (0, 0), ring.point(lambda v: round(v * 0.25)))
    white = Image.new("RGBA", (n, n), (255, 255, 255, 255))
    out.paste(white, (0, 0), mask(size, legend).point(lambda v: round(v * 0.96)))
    return out.resize((size, size), Image.LANCZOS)


def app_icon():
    # Mirrors assets/icon.svg.
    return keycap(
        1024,
        skirt=(100, 100, 824, 824, 184),
        top=(176, 136, 672, 640, 132),
        legend=[(356, 280, 312, 72, 36), (476, 280, 72, 352, 36)],
        shadow=(0, 16, 18, 0.28),
        highlight=3,
    )


def color_tray(size):
    # The app icon without margins or shadow, in a 32-unit canvas scaled to `size`.
    k = size / 32
    s = lambda *v: tuple(a * k for a in v)  # noqa: E731
    return keycap(
        size,
        skirt=s(1, 1, 30, 30, 7),
        top=s(4, 2, 24, 23, 5),
        legend=[s(10, 7, 12, 3, 1.5), s(14.5, 7, 3, 14, 1.5)],
    )


def template_tray(size):
    """Black-on-transparent keycap outline with a filled top face and a knocked-out T.

    Drawn on an 18-unit canvas (macOS menu-bar icons are 18 pt tall)."""
    k = size / 18
    s = lambda *v: tuple(a * k for a in v)  # noqa: E731
    ring = ImageChops.subtract(
        mask(size, [s(1.5, 2, 15, 14.5, 4)]), mask(size, [s(2.8, 3.3, 12.4, 11.9, 2.9)])
    )
    face = ImageChops.subtract(
        mask(size, [s(4.25, 3.6, 9.5, 9.3, 2.2)]),
        mask(size, [s(6.3, 5.4, 5.4, 1.5, 0.75), s(8.25, 5.4, 1.5, 5.6, 0.75)]),
    )
    alpha = ImageChops.lighter(ring, face)
    n = size * SS
    out = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    out.paste(Image.new("RGBA", (n, n), (0, 0, 0, 255)), (0, 0), alpha)
    return out.resize((size, size), Image.LANCZOS)


def main():
    icons = ROOT / "src-tauri" / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    app_icon().save(ROOT / "assets" / "icon.png", optimize=True)
    template_tray(18).save(icons / "tray-template.png", optimize=True)
    template_tray(36).save(icons / "tray-template@2x.png", optimize=True)
    color_tray(32).save(icons / "tray-color.png", optimize=True)
    color_tray(64).save(icons / "tray-color@2x.png", optimize=True)


if __name__ == "__main__":
    main()
