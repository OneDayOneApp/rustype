#!/usr/bin/env python3
"""Draw Rustype's app icon and wordmark with Pillow.

Design: a dark squircle with a rust-orange glow, a white microphone and
orange waveform bars. The wordmark pairs Avenir Next Bold with the mark.
"""
import math

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps

ROOT = __import__("pathlib").Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
ASSETS.mkdir(exist_ok=True)

RUST_LIGHT = (255, 138, 76)
RUST = (232, 69, 44)
INK = (12, 13, 16)
INK_TOP = (38, 42, 49)
WHITE = (247, 247, 250)

SS = 2048  # supersampled canvas


def vertical_gradient(size, top, bottom):
    grad = Image.new("RGB", (1, size[1]))
    for y in range(size[1]):
        t = y / max(1, size[1] - 1)
        grad.putpixel((0, y), tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)))
    return grad.resize(size, Image.BILINEAR)


def rounded_mask(size, radius):
    mask = Image.new("L", size, 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255)
    return mask


def draw_mark(canvas_size, transparent_bg=False):
    """Draw the mic + waveform mark; returns RGBA image."""
    img = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    s = canvas_size / 2048.0
    d = ImageDraw.Draw(img)

    # Orange glow behind the microphone.
    glow = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse(
        [int(740 * s), int(620 * s), int(1308 * s), int(1380 * s)],
        fill=(255, 110, 60, 55),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(150 * s))
    img.alpha_composite(glow)

    # Two restrained telegraph arcs per side; less is more.
    cx, cy = 1024, 920
    arcs = [(560, 28, 200)]
    for radius, width, alpha in arcs:
        for a0, a1 in ((-40, 40), (140, 220)):
            steps = 120
            for i in range(steps + 1):
                angle = math.radians(a0 + (a1 - a0) * i / steps)
                x = cx + radius * math.cos(angle)
                y = cy + radius * math.sin(angle)
                d.ellipse(
                    [
                        int((x - width / 2) * s),
                        int((y - width / 2) * s),
                        int((x + width / 2) * s),
                        int((y + width / 2) * s),
                    ],
                    fill=RUST_LIGHT + (alpha,),
                )

    # Microphone capsule.
    d.rounded_rectangle(
        [int(900 * s), int(560 * s), int(1148 * s), int(1140 * s)],
        radius=int(124 * s),
        fill=WHITE + (255,),
    )

    # Cradle (U shape).
    d.arc(
        [int(820 * s), int(820 * s), int(1228 * s), int(1360 * s)],
        start=0,
        end=180,
        fill=WHITE + (255,),
        width=int(56 * s),
    )
    # Stem and base.
    d.rounded_rectangle(
        [int(1002 * s), int(1330 * s), int(1046 * s), int(1462 * s)],
        radius=int(22 * s),
        fill=WHITE + (255,),
    )
    d.rounded_rectangle(
        [int(880 * s), int(1458 * s), int(1168 * s), int(1512 * s)],
        radius=int(27 * s),
        fill=WHITE + (255,),
    )
    return img


def build_icon():
    """Squircle with a transparent margin, like a macOS app icon."""
    canvas = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))
    inset = 92
    size = SS - inset * 2
    tile = vertical_gradient((size, size), (30, 33, 38), INK).convert("RGBA")

    glow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse(
        [size * 0.28, size * 0.30, size * 0.72, size * 0.78],
        fill=(232, 69, 44, 70),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(180))
    tile.alpha_composite(glow)

    mark = draw_mark(size)
    tile.alpha_composite(mark)

    border = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    bd = ImageDraw.Draw(border)
    bd.rounded_rectangle(
        [2, 2, size - 3, size - 3],
        radius=int(size * 0.22),
        outline=(255, 255, 255, 30),
        width=4,
    )
    tile.alpha_composite(border)
    tile.putalpha(rounded_mask((size, size), int(size * 0.22)))
    canvas.alpha_composite(tile, (inset, inset))

    icon = canvas.resize((1024, 1024), Image.LANCZOS)
    icon.save(ASSETS / "icon-1024.png")
    print("wrote", ASSETS / "icon-1024.png")

    mark_only = draw_mark(1024)
    mark_only.save(ASSETS / "mark-1024.png")
    print("wrote", ASSETS / "mark-1024.png")


def build_logo():
    W, H = 2600, 760
    icon = Image.open(ASSETS / "icon-1024.png").convert("RGBA").resize((560, 560), Image.LANCZOS)
    font = ImageFont.truetype("/System/Library/Fonts/Avenir Next.ttc", 300, index=0)

    def compose(first_color, path):
        logo = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        logo.alpha_composite(icon, (70, 100))
        x = 760
        baseline_y = (H - 300) // 2 - 26
        for text, color in (("Rust", first_color), ("ype", (255, 107, 61, 255))):
            layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
            ld = ImageDraw.Draw(layer)
            ld.text((x, baseline_y), text, font=font, fill=color)
            logo.alpha_composite(layer)
            x += int(ld.textlength(text, font=font)) - 6
        logo.save(path)
        print("wrote", path)

    compose((21, 22, 26, 255), ASSETS / "logo.png")
    compose((247, 247, 250, 255), ASSETS / "logo-dark.png")


def build_icns():
    import subprocess

    iconset = ASSETS / "icon.iconset"
    iconset.mkdir(exist_ok=True)
    src = ASSETS / "icon-1024.png"
    pairs = [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ]
    for size, name in pairs:
        subprocess.run(
            ["sips", "-z", str(size), str(size), str(src), "--out", str(iconset / name)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    subprocess.run(
        ["iconutil", "-c", "icns", str(iconset), "-o", str(ASSETS / "icon.icns")],
        check=True,
    )
    print("wrote", ASSETS / "icon.icns")


if __name__ == "__main__":
    build_icon()
    build_logo()
    build_icns()
