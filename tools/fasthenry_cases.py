"""Writes FastHenry input files and `cases.json` for the inductance checks of the pcb-core tests.

    python tools/fasthenry_cases.py OUTDIR

`cases.json` (also copied to tools/inductance_cases.json when run with --update) lists every case as
{"name", "kind": "self" | "mutual", "w": width in mm, "paths": [[[x, y], ...], ...]}: one polyline per trace, drawn from its
supply end to its far end (the way the program reads it). A `self` case has one trace, a `mutual` case two. One FastHenry
file per case: units mm, copper 5.8e4 S/mm, 35 um thick, 12 x 4 filaments across the section, one frequency of 1 kHz where the
current is uniform in the section; every polyline is a chain of straight segments with one port from its first to its last
node. FastHenry2 writes Z = R + j w L of the ports to Zc.mat; the inductance is Im(Z) / w and the mutual inductance Im(Z12) / w.
"""
import json
import math
import os
import sys

T = 0.035
SIGMA = 5.8e4  # S/mm
FREQ = 1e3


def rot(deg, length):
    a = math.radians(deg)
    return (length * math.cos(a), length * math.sin(a))


def chain(start, legs):
    """Polyline from `start` through legs given as (angle in degrees, length)."""
    pts = [start]
    for ang, ln in legs:
        dx, dy = rot(ang, ln)
        pts.append((round(pts[-1][0] + dx, 6), round(pts[-1][1] + dy, 6)))
    return pts


def cases():
    out = []

    def add(name, kind, w, paths):
        out.append({"name": name, "kind": kind, "w": w, "paths": [[list(p) for p in path] for path in paths]})

    # parallel traces (as in the first FastHenry check)
    for l in (10.0, 30.0, 100.0):
        for w in (0.25, 1.0, 3.0):
            add(f"self l={l:g} w={w:g}", "self", w, [[(0, 0), (l, 0)]])
    for l in (30.0, 100.0):
        for w in (0.25, 1.0):
            for sp in (0.5, 1.0, 2.5, 5.0, 10.0):
                if sp > w:
                    add(f"mutual l={l:g} w={w:g} s={sp:g}", "mutual", w, [[(0, 0), (l, 0)], [(0, sp), (l, sp)]])
    # one trace with a corner
    for a, b, w in ((50, 50, 0.25), (30, 70, 0.25), (10, 90, 0.25), (50, 50, 1.0)):
        add(f"corner90 {a}+{b} w={w:g}", "self", w, [chain((0, 0), [(0, a), (90, b)])])
    for turn in (45, 90, 135):
        add(f"turn{turn} 50+50 w=0.25", "self", 0.25, [chain((0, 0), [(0, 50), (turn, 50)])])
    add("dogleg 40+14(45deg)+40 w=0.25", "self", 0.25, [chain((0, 0), [(0, 40), (45, 14.142), (0, 40)])])
    add("staircase 3x(20+20) w=0.25", "self", 0.25, [chain((0, 0), [(0, 20), (90, 20)] * 3)])
    # two traces at an angle, antiparallel, and a pair that goes round a corner
    for ang in (15, 30, 45, 90):
        add(f"angled pair {ang}deg l=50 s=2.5", "mutual", 0.25, [[(0, 0), (50, 0)], chain((0, 2.5), [(ang, 50)])])
    add("antiparallel l=100 s=2.5", "mutual", 0.25, [[(0, 0), (100, 0)], [(100, 2.5), (0, 2.5)]])
    for sp in (2.5, 0.5):
        add(f"L pair inside s={sp:g}", "mutual", 0.25, [chain((0, 0), [(0, 50), (90, 50)]), chain((0, sp), [(0, 50 - sp), (90, 50 - sp)])])
        add(f"L pair outside s={sp:g}", "mutual", 0.25, [chain((0, 0), [(0, 50), (90, 50)]), chain((0, -sp), [(0, 50 + sp), (90, 50 + sp)])])
    add("two L pair far s=10", "mutual", 0.25, [chain((0, 0), [(0, 50), (90, 50)]), chain((0, 10), [(0, 40), (90, 40)])])
    return out


def fname(name):
    return name.replace(" ", "_").replace("=", "").replace("+", "p").replace("(", "").replace(")", "").replace(".", "_")


def inp(case):
    s = f"* {case['name']}\n.units mm\n.default sigma={SIGMA}\n"
    ports = []
    for k, path in enumerate(case["paths"]):
        tag = "ab"[k]
        for i, (x, y) in enumerate(path):
            s += f"N{tag}{i} x={x} y={y} z=0\n"
        for i in range(len(path) - 1):
            s += f"E{tag}{i} N{tag}{i} N{tag}{i + 1} w={case['w']} h={T} nwinc=12 nhinc=4\n"
        ports.append(f".external N{tag}0 N{tag}{len(path) - 1}\n")
    return s + "".join(ports) + f".freq fmin={FREQ} fmax={FREQ} ndec=1\n.end\n"


def main(out, update):
    os.makedirs(out, exist_ok=True)
    cs = cases()
    for c in cs:
        with open(os.path.join(out, fname(c["name"]) + ".inp"), "w", encoding="ascii", newline="\n") as f:
            f.write(inp(c))
    text = json.dumps(cs, indent=1)
    for dest in [os.path.join(out, "cases.json")] + ([os.path.join(os.path.dirname(os.path.abspath(__file__)), "inductance_cases.json")] if update else []):
        with open(dest, "w", encoding="ascii", newline="\n") as f:
            f.write(text + "\n")
    print(f"{len(cs)} cases in {out}")


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    main(args[0] if args else "fasthenry_cases", "--update" in sys.argv)
