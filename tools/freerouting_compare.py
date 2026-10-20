"""Compare the routes in a circuitCad board with what Freerouting makes of the same placement.

The board's pads (and its rules) are written as a Specctra DSN file without any tracks, Freerouting
(https://github.com/freerouting/freerouting, a Java program; not part of this repository) routes it, and the SES it
writes is read back. Printed: track length, number of tracks, vias and tracks at an angle that is not 0/45/90 degrees,
for the board as it is and for Freerouting's result. Freerouting's result is also saved as a .pcbjson next to the
DSN, so `ccad` (`board open ...`, `board drc`) can check it with circuitCad's own DRC.

    python tools/freerouting_compare.py --pcb board.pcbjson --jar freerouting-2.5.0.jar [--skip-nets GND]

Needs Java and the Freerouting jar (download it yourself: it is about 65 MB). The tracks of the board are left out of
the DSN; the track width and the via size are the commonest ones in the board. Nets in --skip-nets are not routed (a
net that the board carries with a pour, GND for example). Files go to --out-dir (default: next to the board).
"""
import argparse
import json
import math
import os
import re
import subprocess
import sys
import time


def um(v):
    return round(v * 1000, 1)


def to_dsn(b, path):
    pads = b["pads"]
    width = max(set(t["width"] for t in b["tracks"]), key=[t["width"] for t in b["tracks"]].count) if b["tracks"] else 0.25
    via = b["vias"][0] if b["vias"] else {"diameter": 0.6, "drill": 0.3}
    clr = b["rules"]["min_clearance"]
    xs = [p["x"] for p in b["outline"]]
    ys = [p["y"] for p in b["outline"]]
    stacks = {}
    out = []
    o = out.append
    o('(pcb board\n (parser (string_quote ") (space_in_quoted_tokens on) (host_cad "cmp") (host_version "1"))')
    o(" (resolution um 10)\n (unit um)\n (structure")
    o("  (layer F.Cu (type signal) (property (index 0)))\n  (layer B.Cu (type signal) (property (index 1)))")
    pts = " ".join(f"{um(p['x'])} {um(-p['y'])}" for p in b["outline"])
    o(f"  (boundary (path pcb 0 {pts} {um(b['outline'][0]['x'])} {um(-b['outline'][0]['y'])}))")
    vname = f"Via_{um(via['diameter'])}:{um(via['drill'])}"
    o(f'  (via "{vname}")')
    o(f"  (rule (width {um(width)}) (clearance {um(clr)}))\n )")

    def shape_str(layer, sh):
        if "Circle" in sh:
            return f"(shape (circle {layer} {um(sh['Circle']['d'])}))"
        k = "Rect" if "Rect" in sh else "Oval"
        w, h = sh[k]["w"], sh[k]["h"]
        if k == "Rect":
            return f"(shape (rect {layer} {um(-w/2)} {um(-h/2)} {um(w/2)} {um(h/2)}))"
        if w >= h:
            r = (w - h) / 2
            return f"(shape (path {layer} {um(h)} {um(-r)} 0 {um(r)} 0))"
        r = (h - w) / 2
        return f"(shape (path {layer} {um(w)} 0 {um(-r)} 0 {um(r)}))"

    def stack_for(p):
        sh = p["shape"]
        thru = (p["drill"] or 0) > 0
        key = (json.dumps(sh, sort_keys=True), thru, p["side"])
        if key not in stacks:
            stacks[key] = f"PS{len(stacks)}"
        return stacks[key]

    comps = []
    for i, p in enumerate(pads):
        stack_for(p)
    o(" (placement")
    byimg = {}
    for i, p in enumerate(pads):
        byimg.setdefault(stack_for(p), []).append(i)
    for st, idx in byimg.items():
        o(f'  (component "I_{st}"')
        for i in idx:
            p = pads[i]
            o(f'   (place "P{i}" {um(p["pos"]["x"])} {um(-p["pos"]["y"])} front 0)')
        o("  )")
    o(" )\n (library")
    for st in byimg:
        o(f'  (image "I_{st}" (pin {st} 1 0 0))')
    for (sh, thru, side), st in stacks.items():
        sh = json.loads(sh)
        o(f"  (padstack {st}")
        if thru:
            o("   " + shape_str("F.Cu", sh))
            o("   " + shape_str("B.Cu", sh))
        else:
            o("   " + shape_str("F.Cu" if side == "Top" else "B.Cu", sh))
        o("   (attach off)\n  )")
    o(f'  (padstack "{vname}" (shape (circle F.Cu {um(via["diameter"])})) (shape (circle B.Cu {um(via["diameter"])})) (attach off))')
    o(" )\n (network")
    nets = {}
    for i, p in enumerate(pads):
        if p.get("net") and p["net"] not in os.environ.get("SKIP_NETS","").split(","):
            nets.setdefault(p["net"], []).append(f"P{i}-1")
    names = {}
    for n, pl in nets.items():
        safe = re.sub(r"[^A-Za-z0-9_]", "_", n)
        names[safe] = n
        if len(pl) < 2:
            continue
        o(f'  (net "{safe}" (pins {" ".join(pl)}))')
    allnets = " ".join(f'"{re.sub(r"[^A-Za-z0-9_]", "_", n)}"' for n, pl in nets.items() if len(pl) >= 2)
    o(f'  (class default {allnets} (circuit (use_via "{vname}")) (rule (width {um(width)}) (clearance {um(clr)})))')
    o(" )\n (wiring)\n)")
    open(path, "w", encoding="utf-8").write("\n".join(out))
    return width, via, names


def parse_ses(path, width_default, via):
    txt = open(path, encoding="utf-8").read()
    toks = re.findall(r'\(|\)|"[^"]*"|[^\s()]+', txt)
    # simple s-expr parse
    pos = 0

    def parse():
        nonlocal pos
        t = toks[pos]
        pos += 1
        if t == "(":
            lst = []
            while toks[pos] != ")":
                lst.append(parse())
            pos += 1
            return lst
        return t.strip('"')

    root = parse()
    res = [x for x in root if isinstance(x, list) and x and x[0] == "routes"][0]
    resolution = [x for x in res if isinstance(x, list) and x[0] == "resolution"][0]
    unit, val = resolution[1], float(resolution[2])
    scale = {"um": 1e-3, "mm": 1.0, "mil": 0.0254, "inch": 25.4}[unit] / val
    tracks, vias = [], []
    netout = [x for x in res if isinstance(x, list) and x[0] == "network_out"][0]
    for net in netout[1:]:
        if not (isinstance(net, list) and net[0] == "net"):
            continue
        for w in net[2:]:
            if not isinstance(w, list):
                continue
            if w[0] == "wire":
                path = [x for x in w if isinstance(x, list) and x[0] == "path"][0]
                layer, wd = path[1], float(path[2]) * scale
                c = [float(v) * scale for v in path[3:]]
                pts = [(c[i], -c[i + 1]) for i in range(0, len(c), 2)]
                for a, bb in zip(pts, pts[1:]):
                    tracks.append({"side": "Top" if layer == "F.Cu" else "Bottom", "a": {"x": a[0], "y": a[1]}, "b": {"x": bb[0], "y": bb[1]}, "width": wd})
            elif w[0] == "via":
                vias.append({"pos": {"x": float(w[2]) * scale, "y": -float(w[3]) * scale}, "diameter": via["diameter"], "drill": via["drill"]})
    return tracks, vias


def metrics(tracks, vias):
    L = sum(math.hypot(t["a"]["x"] - t["b"]["x"], t["a"]["y"] - t["b"]["y"]) for t in tracks)
    odd = 0
    n = 0
    for t in tracks:
        dx, dy = t["b"]["x"] - t["a"]["x"], t["b"]["y"] - t["a"]["y"]
        if math.hypot(dx, dy) < 1e-6:
            continue
        n += 1
        ang = math.degrees(math.atan2(abs(dy), abs(dx)))
        if min(abs(ang), abs(ang - 45), abs(ang - 90)) > 0.5:
            odd += 1
    return {"len_mm": round(L, 1), "segs": n, "vias": len(vias), "odd_angle_segs": odd}


def main():
    ap = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    ap.add_argument("--pcb", required=True, help="the board (.pcbjson, e.g. from `ccad board save`)")
    ap.add_argument("--jar", required=True, help="freerouting-x.y.z.jar")
    ap.add_argument("--skip-nets", default="", help="comma separated nets that are not routed (poured nets)")
    ap.add_argument("--out-dir")
    ap.add_argument("--passes", default="40")
    a = ap.parse_args()
    out_dir = a.out_dir or os.path.dirname(os.path.abspath(a.pcb))
    os.makedirs(out_dir, exist_ok=True)
    name = os.path.splitext(os.path.basename(a.pcb))[0]
    base = os.path.join(out_dir, name)
    os.environ["SKIP_NETS"] = a.skip_nets
    b = json.load(open(a.pcb, encoding="utf-8"))
    ours = metrics(b["tracks"], b["vias"])
    width, via, _ = to_dsn(b, base + ".dsn")
    if os.path.exists(base + ".ses"):
        os.remove(base + ".ses")
    t0 = time.time()
    r = subprocess.run(["java", "-jar", a.jar, "--gui.enabled=false", "-de", base + ".dsn", "-do", base + ".ses", "-mp", a.passes, "-l", "en"],
                       cwd=out_dir, capture_output=True, text=True, timeout=900)
    seconds = time.time() - t0
    if not os.path.exists(base + ".ses"):
        print(json.dumps({"name": name, "error": "Freerouting wrote no .ses", "tail": (r.stdout + r.stderr)[-600:]}))
        sys.exit(1)
    tracks, vias = parse_ses(base + ".ses", width, via)
    b2 = dict(b)
    b2["tracks"], b2["vias"] = tracks, vias
    json.dump(b2, open(base + ".freerouting.pcbjson", "w"))
    print(json.dumps({"name": name, "board": ours, "freerouting": metrics(tracks, vias), "freerouting_seconds": round(seconds, 1), "saved": base + ".freerouting.pcbjson"}, indent=1))


if __name__ == "__main__":
    main()
