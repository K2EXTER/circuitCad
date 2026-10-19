"""Writes FastHenry input files and `cases.json` for the inductance checks of the pcb-core tests.

    python tools/fasthenry_cases.py OUTDIR

`cases.json` (also copied to tools/inductance_cases.json when run with --update) lists every case as
{"name", "kind": "self" | "mutual", "w": width in mm, "drill": via drill in mm, "paths": [[[x, y, layer], ...], ...]}: one
polyline per trace, drawn from its supply end to its far end (the way the program reads it); layer 0 is the top copper, 1 the
bottom copper 1.6 mm below it, and two points at the same x, y on different layers are a via. A `self` case has one trace, a
`mutual` case two. Optional keys: "layers" (copper layers, default 2; layer k is at z = -1.6 k / (layers - 1)), "freq" (Hz, default
1 kHz), and "plane": {"layer": k, "margin": mm}, a copper pour on layer k that covers the traces and `margin` mm beyond them.
With a plane, a `loop` case is one trace whose far end is shorted to the plane and whose near end is the port against the
plane (the plane carries the return current), and a `mutual` case is two such traces; the plane's own current spreads at the
frequency of the case, so planes are checked at 10 MHz, where it runs under the trace instead of across the whole board. One FastHenry
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

    def add(name, kind, w, paths, drill=0.3, layers=2, freq=FREQ, plane=None):
        norm = [[[p[0], p[1], p[2] if len(p) > 2 else 0] for p in path] for path in paths]
        c = {"name": name, "kind": kind, "w": w, "drill": drill, "layers": layers, "freq": freq, "paths": norm}
        if plane:
            c["plane"] = plane
        out.append(c)

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
    # vias and two layers (layer 0 = top, 1 = bottom, 1.6 mm apart)
    for d in (0.3, 0.6, 1.0):
        add(f"via d={d:g} with 1 mm stubs", "self", 0.25, [[(0, 0, 0), (1, 0, 0), (1, 0, 1), (2, 0, 1)]], d)
    add("top 30 + via + bottom 30 straight", "self", 0.25, [[(0, 0, 0), (30, 0, 0), (30, 0, 1), (60, 0, 1)]])
    add("top 50 + via + bottom 50 back (U-turn)", "self", 0.25, [[(0, 0, 0), (50, 0, 0), (50, 0, 1), (0, 0, 1)]])
    add("top 50 + via + bottom 50 back with 2.5 mm jog", "self", 0.25, [[(0, 0, 0), (50, 0, 0), (50, 0, 1), (50, 2.5, 1), (0, 2.5, 1)]])
    add("top 50 + via + bottom 50 sideways", "self", 0.25, [[(0, 0, 0), (50, 0, 0), (50, 0, 1), (50, 50, 1)]])
    add("stacked pair l=100", "mutual", 0.25, [[(0, 0, 0), (100, 0, 0)], [(0, 0, 1), (100, 0, 1)]])
    add("stacked pair l=100 s=2.5", "mutual", 0.25, [[(0, 0, 0), (100, 0, 0)], [(0, 2.5, 1), (100, 2.5, 1)]])
    add("stacked pair l=100 s=10", "mutual", 0.25, [[(0, 0, 0), (100, 0, 0)], [(0, 10, 1), (100, 10, 1)]])
    add("stacked antiparallel l=100", "mutual", 0.25, [[(0, 0, 0), (100, 0, 0)], [(100, 0, 1), (0, 0, 1)]])
    add("stacked crossing 90deg", "mutual", 0.25, [[(0, 0, 0), (100, 0, 0)], [(50, -25, 1), (50, 25, 1)]])
    add("stacked w=1 l=30", "mutual", 1.0, [[(0, 0, 0), (30, 0, 0)], [(0, 0, 1), (30, 0, 1)]])
    # vias beside each other: two top-via-bottom paths (1 mm stubs), the second one `d` away, going the same way or back
    stub = [(0, 0, 0), (1, 0, 0), (1, 0, 1), (2, 0, 1)]
    for d in (1.27, 2.54, 5.0, 10.0):
        other = [(x, y + d, layer) for x, y, layer in stub]
        add(f"via pair d={d:g} same way", "mutual", 0.25, [stub, other])
    add("via pair d=2.54 back (supply and return)", "mutual", 0.25, [stub, [(x, y + 2.54, layer) for x, y, layer in reversed(stub)]])
    add("via pair d=2.54 drill 0.6", "mutual", 0.25, [stub, [(x, y + 2.54, layer) for x, y, layer in stub]], drill=0.6)
    # one path with two vias (down, along, up): they couple, with opposite currents
    for d in (1.27, 2.54, 5.0):
        add(f"path with two vias {d:g} apart", "self", 0.25, [[(0, 0, 0), (1, 0, 0), (1, 0, 1), (1 + d, 0, 1), (1 + d, 0, 0), (2 + d, 0, 0)]])
    # a hole through several layers of a 4-layer board, and a part of it
    add("via through 4 layers", "self", 0.25, [[(0, 0, 0), (1, 0, 0), (1, 0, 3), (2, 0, 3)]], layers=4)
    add("via from layer 0 to 1 of 4", "self", 0.25, [[(0, 0, 0), (1, 0, 0), (1, 0, 1), (2, 0, 1)]], layers=4)
    add("via from layer 1 to 3 of 4", "self", 0.25, [[(0, 0, 1), (1, 0, 1), (1, 0, 3), (2, 0, 3)]], layers=4)
    add("via pair d=2.54 through 4 layers", "mutual", 0.25, [[(0, 0, 0), (1, 0, 0), (1, 0, 3), (2, 0, 3)], [(0, 2.54, 0), (1, 2.54, 0), (1, 2.54, 3), (2, 2.54, 3)]], layers=4)
    # traces over a copper pour (the pour carries the return current): 2 layers (pour 1.6 mm below), 4 layers (0.53 / 1.07 mm below)
    f = 1e7
    add("loop over plane l=50 h=1.6", "loop", 0.25, [[(0, 0, 0), (50, 0, 0)]], plane={"layer": 1, "margin": 20}, freq=f)
    add("loop over plane l=50 w=1 h=1.6", "loop", 1.0, [[(0, 0, 0), (50, 0, 0)]], plane={"layer": 1, "margin": 20}, freq=f)
    add("loop over plane l=50 h=0.53", "loop", 0.25, [[(0, 0, 0), (50, 0, 0)]], layers=4, plane={"layer": 1, "margin": 20}, freq=f)
    add("loop over plane l=50 h=1.07", "loop", 0.25, [[(0, 0, 0), (50, 0, 0)]], layers=4, plane={"layer": 2, "margin": 20}, freq=f)
    add("L loop over plane 30+30 h=1.6", "loop", 0.25, [chain((0, 0), [(0, 30), (90, 30)])], plane={"layer": 1, "margin": 20}, freq=f)
    add("pair over plane l=50 s=2.5 h=1.6", "mutual", 0.25, [[(0, 0, 0), (50, 0, 0)], [(0, 2.5, 0), (50, 2.5, 0)]], plane={"layer": 1, "margin": 20}, freq=f)
    add("pair over plane l=50 s=10 h=1.6", "mutual", 0.25, [[(0, 0, 0), (50, 0, 0)], [(0, 10, 0), (50, 10, 0)]], plane={"layer": 1, "margin": 20}, freq=f)
    add("pair over inner plane l=50 s=2.5 h=0.53", "mutual", 0.25, [[(0, 0, 0), (50, 0, 0)], [(0, 2.5, 0), (50, 2.5, 0)]], layers=4, plane={"layer": 1, "margin": 20}, freq=f)
    return out


def fname(name):
    return name.replace(" ", "_").replace("=", "").replace("+", "p").replace("(", "").replace(")", "").replace(".", "_")


def zof(case, layer):
    return -1.6 * layer / (case.get("layers", 2) - 1)


def inp(case):
    freq = case.get("freq", FREQ)
    s = f"* {case['name']}\n.units mm\n.default sigma={SIGMA}\n"
    plane = case.get("plane")
    ports = []
    equivs = []
    if plane:
        pts = [q for path in case["paths"] for q in path]
        m = plane["margin"]
        x0, x1 = min(q[0] for q in pts) - m, max(q[0] for q in pts) + m
        y0, y1 = min(q[1] for q in pts) - m, max(q[1] for q in pts) + m
        zp = zof(case, plane["layer"])
        s += f"G1 x1={x0} y1={y0} z1={zp} x2={x1} y2={y0} z2={zp} x3={x1} y3={y1} z3={zp} thick={T} seg1={round((x1 - x0) / 2)} seg2={round(y1 - y0)} sigma={SIGMA} nhinc=1\n"
        for k, path in enumerate(case["paths"]):
            s += f"+ Nps{k} {path[0][0]},{path[0][1]},{zp}\n+ Npe{k} {path[-1][0]},{path[-1][1]},{zp}\n"
    for k, path in enumerate(case["paths"]):
        tag = "ab"[k]
        for i, (x, y, layer) in enumerate(path):
            s += f"N{tag}{i} x={x} y={y} z={zof(case, layer)}\n"
        for i in range(len(path) - 1):
            a, b = path[i], path[i + 1]
            if a[0] == b[0] and a[1] == b[1]:
                # a via: a solid square bar with the same geometric mean distance as a round barrel of the drill diameter
                side = round(0.87 * case["drill"], 4)
                s += f"E{tag}{i} N{tag}{i} N{tag}{i + 1} w={side} h={side}\n"
            else:
                s += f"E{tag}{i} N{tag}{i} N{tag}{i + 1} w={case['w']} h={T} nwinc=12 nhinc=4\n"
        if plane:
            equivs.append(f".equiv N{tag}{len(path) - 1} Npe{k}\n")
            ports.append(f".external N{tag}0 Nps{k}\n")
        else:
            ports.append(f".external N{tag}0 N{tag}{len(path) - 1}\n")
    return s + "".join(equivs) + "".join(ports) + f".freq fmin={freq} fmax={freq} ndec=1\n.end\n"


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
