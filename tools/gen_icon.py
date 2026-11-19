"""Generate the circuitCad logo files from one geometry description.

Usage: python tools/gen_icon.py
Writes: docs/logo.svg, docs/favicon.png, docs/images/og.png, docs/images/en/og.png,
        crates/app/assets/icon.png (window icon), crates/app/assets/icon.ico (exe icon)

The logo is two C's: a round one (schematic / simulation) and a square trace
with pads (board). Geometry is in a 160x160 tile; edit here, not in the outputs.
"""
import math
import os
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DARK, LIGHT, LINE = (11, 110, 79), (62, 207, 154), (201, 209, 219)
SC = 1.1  # content scale about the tile centre
CX, CY = 79, 82  # centre of the artwork before scaling


def tx(x, y):
    return ((x - CX) * SC + 80, (y - CY) * SC + 80)


# round C: centre (50,80) r36, open to the right (angles 45..315 clockwise from 3 o'clock)
ROUND = dict(c=(50, 80), r=36, w=12)
# square C: polyline with pads at both ends
SQUARE = dict(pts=[(135, 50), (105, 50), (105, 110), (135, 110)], w=10, pad_out=9, pad_in=5)


def render(size, with_tile=True):
    k = 8
    s = size * k / 160
    img = Image.new("RGBA", (size * k, size * k), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if with_tile:
        d.rounded_rectangle([0, 0, size * k - 1, size * k - 1], radius=28 * s, fill=(255, 255, 255, 255),
                            outline=LINE + (255,), width=max(1, int(0.8 * s)))

    def P(x, y):
        x, y = tx(x, y)
        return (x * s, y * s)

    def dot(x, y, r, fill):
        px, py = P(x, y)
        d.ellipse([px - r * SC * s, py - r * SC * s, px + r * SC * s, py + r * SC * s], fill=fill)

    cx, cy = ROUND["c"]
    r, w = ROUND["r"], ROUND["w"]
    px, py = P(cx, cy)
    rr = r * SC * s
    d.arc([px - rr, py - rr, px + rr, py + rr], 45, 315, fill=DARK, width=int(w * SC * s))
    for sign in (-1, 1):
        a = math.radians(45)
        dot(cx + r * math.cos(a), cy + sign * r * math.sin(a), w / 2, DARK)

    pts = [P(*p) for p in SQUARE["pts"]]
    d.line(pts, fill=LIGHT, width=int(SQUARE["w"] * SC * s), joint="curve")
    for x, y in (SQUARE["pts"][0], SQUARE["pts"][-1]):
        dot(x, y, SQUARE["w"] / 2, LIGHT)
        dot(x, y, SQUARE["pad_out"], LIGHT)
        dot(x, y, SQUARE["pad_in"], (255, 255, 255, 255))
    return img.resize((size, size), Image.LANCZOS)


def svg():
    cx, cy = ROUND["c"]
    r, w = ROUND["r"], ROUND["w"]
    a = math.radians(45)
    x1, y1 = cx + r * math.cos(a), cy - r * math.sin(a)
    x2, y2 = cx + r * math.cos(a), cy + r * math.sin(a)
    pts = SQUARE["pts"]
    path = "M%g %g " % pts[0] + " ".join("L%g %g" % p for p in pts[1:])
    pads = "".join(
        '<circle cx="%g" cy="%g" r="%g" fill="#3ecf9a"/><circle cx="%g" cy="%g" r="%g" fill="#fff"/>'
        % (x, y, SQUARE["pad_out"], x, y, SQUARE["pad_in"])
        for x, y in (pts[0], pts[-1])
    )
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 160" width="160" height="160">'
        '<title>circuitCad</title>'
        '<rect x="0.5" y="0.5" width="159" height="159" rx="28" fill="#fff" stroke="#c9d1db"/>'
        '<g transform="translate(80 80) scale(%g) translate(%g %g)">'
        '<path d="M%.2f %.2f A%g %g 0 1 0 %.2f %.2f" fill="none" stroke="#0b6e4f" stroke-width="%g" stroke-linecap="round"/>'
        '<path d="%s" fill="none" stroke="#3ecf9a" stroke-width="%g" stroke-linejoin="round" stroke-linecap="round"/>'
        '%s</g></svg>\n'
        % (SC, -CX, -CY, x1, y1, r, r, x2, y2, w, path, SQUARE["w"], pads)
    )


def out(*p):
    path = os.path.join(ROOT, *p)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    return path


# The four tabs of the app as small pictures along the bottom of the OGP image: (screenshot in docs/images[/en], crop box on
# the 1920-wide screenshot, label ja, label en). The screenshots come from tools/gen_screenshots.ps1; run that first.
# A name under guide/ is the walkthrough's picture (docs/first-board.html), the same for both languages: only the drawing shows.
OG_TABS = [
    ("guide/dc.png", (900, 90, 1820, 820), "回路図", "Schematic"),
    ("prototype.png", (650, 90, 1700, 610), "試作", "Prototype"),
    ("guide/board.png", (540, 100, 1560, 910), "基板", "PCB"),
    ("bench.png", (760, 80, 1760, 900), "実機", "Bench"),
]


def og(lang="ja"):
    W, H = 1200, 630
    img = Image.new("RGB", (W, H), (245, 247, 250))
    logo = render(170)
    img.paste(logo, (70, 52), logo)
    d = ImageDraw.Draw(img)
    font = os.path.join(ROOT, "crates", "app", "assets", "fonts", "BIZUDGothic-Regular.ttf")
    big, mid, small = ImageFont.truetype(font, 76), ImageFont.truetype(font, 32), ImageFont.truetype(font, 26)
    d.text((280, 60), "circuitCad", font=big, fill=DARK)
    if lang == "en":
        lines = ("Schematic, breadboard, board order, bench.", "A circuit CAD you learn with.")
    else:
        lines = ("回路図からブレッドボード、基板の発注、実機まで。", "学びながら使える、日本語の回路CAD")
    d.text((284, 158), lines[0], font=mid, fill=(28, 35, 48))
    d.text((284, 204), lines[1], font=mid, fill=(90, 101, 119))
    # the tabs: picture, label, arrows between them
    shots = os.path.join(ROOT, "docs", "images", *(["en"] if lang == "en" else []))
    tw, th, gap, top = 252, 200, 44, 300
    left = (W - (4 * tw + 3 * gap)) // 2
    for k, (name, box, ja, en) in enumerate(OG_TABS):
        x = left + k * (tw + gap)
        folder = os.path.join(ROOT, "docs", "images") if name.startswith("guide/") else shots
        pic = Image.open(os.path.join(folder, *name.split("/"))).convert("RGB").crop(box)
        scale = max(tw / pic.width, th / pic.height)
        pic = pic.resize((round(pic.width * scale), round(pic.height * scale)), Image.LANCZOS)
        pic = pic.crop(((pic.width - tw) // 2, (pic.height - th) // 2, (pic.width - tw) // 2 + tw, (pic.height - th) // 2 + th))
        mask = Image.new("L", (tw, th), 0)
        ImageDraw.Draw(mask).rounded_rectangle([0, 0, tw - 1, th - 1], radius=14, fill=255)
        img.paste(pic, (x, top), mask)
        d.rounded_rectangle([x, top, x + tw - 1, top + th - 1], radius=14, outline=LINE, width=2)
        label = en if lang == "en" else ja
        lw = d.textlength(label, font=small)
        d.text((x + (tw - lw) / 2, top + th + 18), label, font=small, fill=DARK)
        if k < 3:
            ax, ay = x + tw + gap / 2, top + th / 2
            d.polygon([(ax - 9, ay - 13), (ax + 11, ay), (ax - 9, ay + 13)], fill=LIGHT)
    return img


if __name__ == "__main__":
    with open(out("docs", "logo.svg"), "w", encoding="utf-8", newline="\n") as f:
        f.write(svg())
    render(64).save(out("docs", "favicon.png"))
    og().save(out("docs", "images", "og.png"))
    og("en").save(out("docs", "images", "en", "og.png"))
    render(256).save(out("crates", "app", "assets", "icon.png"))
    render(256).save(out("crates", "app", "assets", "icon.ico"), sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print("done")
