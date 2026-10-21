"""Builds the live demo GIF from the frames recorded by tools/screenrec.ps1 (see gen_demo_live.ps1).

usage: python -I tools/gen_demo_live.py <frames dir> <out.gif> [width] [fps]
Crops the taskbar off, draws the mouse cursor from frames.csv, puts the caption set by Mark() (marks.csv)
in a bar under the picture. Needs Pillow.
"""
import csv
import os
import sys

from PIL import Image, ImageDraw, ImageFont

TASKBAR = 48  # px of the Windows taskbar at the bottom of the 1080 p screen
BAR = 64
SPEED = 1.0   # playback speed (2.0 = twice as fast)


def cursor(d, x, y, down):
    pts = [(0, 0), (0, 17), (4, 13), (7, 20), (10, 19), (7, 12), (12, 12)]
    poly = [(x + a, y + b) for a, b in pts]
    d.polygon([(px + 1.5, py + 1.5) for px, py in poly], fill=(0, 0, 0, 90))
    d.polygon(poly, fill=(255, 255, 255), outline=(0, 0, 0))
    if down:
        d.ellipse([x - 14, y - 14, x + 14, y + 14], outline=(255, 90, 60), width=3)


def main():
    frames, out = sys.argv[1], sys.argv[2]
    width = int(sys.argv[3]) if len(sys.argv) > 3 else 1100
    keep_fps = float(sys.argv[4]) if len(sys.argv) > 4 else 8
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    font = ImageFont.truetype(os.path.join(root, "crates", "app", "assets", "fonts", "BIZUDGothic-Regular.ttf"), 22)
    rows = list(csv.DictReader(open(os.path.join(frames, "frames.csv"))))
    marks = []
    mp = os.path.join(frames, "marks.csv")
    if os.path.exists(mp):
        for line in open(mp, encoding="utf-8").read().splitlines():
            t, _, text = line.partition("\t")
            marks.append((int(t), text))
    marks.sort()

    first_t = int(rows[0]["t_ms"])
    step = 1000.0 / keep_fps
    seq, durs, next_t = [], [], 0.0
    for r in rows:
        t = int(r["t_ms"])
        if t < next_t:
            continue
        next_t = t + step
        im = Image.open(os.path.join(frames, "%05d.jpg" % int(r["n"]))).convert("RGB")
        im = im.crop((0, 0, im.width, im.height - TASKBAR))
        sx = width / im.width
        h = round(im.height * sx)
        im = im.resize((width, h), Image.LANCZOS).convert("RGBA")
        d = ImageDraw.Draw(im, "RGBA")
        cursor(d, int(int(r["x"]) * sx), int(int(r["y"]) * sx), int(r["down"]))
        canvas = Image.new("RGB", (width, h + BAR), (24, 28, 36))
        canvas.paste(im.convert("RGB"), (0, 0))
        text = ""
        for mt, mtext in marks:
            if mt <= t:
                text = mtext
        if text:
            cd = ImageDraw.Draw(canvas)
            w = cd.textlength(text, font=font)
            cd.text(((width - w) / 2, h + (BAR - 22) / 2 - 2), text, font=font, fill=(240, 244, 250))
        seq.append(canvas)
        durs.append(0)
        if len(seq) > 1:
            durs[-2] = int((t - last_t) / SPEED)
        last_t = t
    durs[-1] = 2500
    pal = [f.convert("P", palette=Image.ADAPTIVE, colors=64) for f in seq]
    pal[0].save(out, save_all=True, append_images=pal[1:], duration=durs, loop=0, optimize=True)
    print("wrote", out, "%d frames, %d KB, %.1f s" % (len(seq), os.path.getsize(out) // 1024, sum(durs) / 1000))


main()
