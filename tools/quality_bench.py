"""Measure how good the placement and the autorouter are, on the shipped sample circuits.

For every docs/examples/*.schjson it runs the same steps as a user would (with `ccad`):

    open  ->  board import  ->  [board pour]  ->  board autoplace  ->  board route  ->  board drc / status / check

and adds up: board area, track length, vias, track count, DRC violations + unrouted connections, bypass-capacitor
warnings and the time `ccad` took. Run it before and after a change to see what the change did:

    cargo build --release -p cli                       # (or --debug: the numbers are the same, only slower)
    python tools/quality_bench.py --save before.json
    ... change the code, rebuild ...
    python tools/quality_bench.py --compare before.json

Options:
    --ccad PATH       the ccad binary (default: target/release/ccad, else target/debug/ccad)
    --no-pour         skip `board pour` (every net is wired, the old flow)
    --route-args S    extra words for `board route`, e.g. "improve=1" or "currents=0"
    --only NAME ...   only these samples (file names without the extension)
    --save FILE       write the per-sample numbers as JSON
    --compare FILE    print the totals of FILE next to this run's, and the samples that changed most

Needs only Python 3 and a built `ccad`. See docs/quality-bench.md for what the numbers have been.
"""
import argparse
import glob
import json
import math
import os
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def find_ccad(given):
    if given:
        return os.path.abspath(given)
    for kind in ("release", "debug"):
        for ext in ("", ".exe"):
            p = os.path.join(ROOT, "target", kind, "ccad" + ext)
            if os.path.exists(p):
                return p
    sys.exit("no ccad binary: run `cargo build --release -p cli` first, or pass --ccad")


def run_script(ccad, text):
    with tempfile.NamedTemporaryFile("w", suffix=".ccad", delete=False, encoding="utf-8") as f:
        f.write(text)
        path = f.name
    try:
        out = subprocess.run([ccad, path], capture_output=True, encoding="utf-8", errors="replace").stdout or ""
    finally:
        os.unlink(path)
    answers = []
    for line in out.strip().split("\n"):
        try:
            answers.append(json.loads(line))
        except ValueError:
            pass
    return answers


def measure(ccad, sample, pour, route_args):
    pcb = tempfile.NamedTemporaryFile(suffix=".pcbjson", delete=False).name
    steps = ["open " + sample.replace("\\", "/"), "board import"]
    if pour:
        steps.append("board pour")
    steps += ["board autoplace", ("board route " + route_args).strip(), "board drc", "board status", "board check", "board save " + pcb.replace("\\", "/")]
    t = time.time()
    answers = run_script(ccad, "\n".join(steps) + "\n")
    seconds = time.time() - t
    try:
        status = next(a for a in answers if "drc_violations" in a and "unrouted" in a)
        board = json.load(open(pcb, encoding="utf-8"))
    except (StopIteration, ValueError, OSError):
        return None
    finally:
        try:
            os.unlink(pcb)
        except OSError:
            pass
    check = next((a for a in answers if "ready" in a), {})
    xs = [p["x"] for p in board["outline"]]
    ys = [p["y"] for p in board["outline"]]
    length = sum(math.hypot(t["a"]["x"] - t["b"]["x"], t["a"]["y"] - t["b"]["y"]) for t in board["tracks"])
    bypass = sum(1 for it in check.get("list", []) if "コンデンサ" in it.get("title", ""))
    return {
        "parts": status["parts"],
        "area": round((max(xs) - min(xs)) * (max(ys) - min(ys)), 1),
        "length": round(length, 1),
        "vias": len(board["vias"]),
        "tracks": len(board["tracks"]),
        "problems": status["drc_violations"] + status["unrouted"],
        "bypass_warnings": bypass,
        "seconds": round(seconds, 2),
    }


TOTALS = ("area", "length", "vias", "tracks", "problems", "bypass_warnings", "seconds")


def totals(rows):
    return {k: round(sum(r[k] for r in rows.values()), 1) for k in TOTALS}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--ccad")
    ap.add_argument("--no-pour", action="store_true")
    ap.add_argument("--route-args", default="")
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--save")
    ap.add_argument("--compare")
    a = ap.parse_args()
    ccad = find_ccad(a.ccad)
    samples = sorted(glob.glob(os.path.join(ROOT, "docs", "examples", "*.schjson")))
    if a.only:
        samples = [s for s in samples if os.path.basename(s).split(".")[0] in a.only]
    rows, skipped = {}, []
    for s in samples:
        name = os.path.basename(s).split(".")[0]
        r = measure(ccad, s, not a.no_pour, a.route_args)
        if r is None:
            skipped.append(name)
            continue
        rows[name] = r
        flag = "" if r["problems"] == 0 else "   <-- DRC/unrouted"
        print(f"{name:28s} area {r['area']:7.0f}  len {r['length']:6.0f}  vias {r['vias']:3d}  tracks {r['tracks']:4d}  {r['seconds']:5.1f}s{flag}", flush=True)
    t = totals(rows)
    print("-" * 78)
    print(f"{len(rows)} samples" + (f" (skipped: {', '.join(skipped)})" if skipped else ""))
    print("total  " + "  ".join(f"{k} {t[k]:g}" for k in TOTALS))
    if a.save:
        json.dump({"pour": not a.no_pour, "route_args": a.route_args, "rows": rows}, open(a.save, "w"), indent=1)
        print("saved", a.save)
    if a.compare:
        old = json.load(open(a.compare))
        common = [n for n in rows if n in old["rows"]]
        if len(common) < len(rows) or len(common) < len(old["rows"]):
            print(f"\n(only the {len(common)} samples that are in both runs are compared)")
        t = totals({n: rows[n] for n in common})
        base = totals({n: old["rows"][n] for n in common})
        print(f"\nagainst {a.compare} (pour={old.get('pour')}, route_args={old.get('route_args')!r}):")
        for k in TOTALS:
            d = t[k] - base[k]
            pct = f"{100.0 * d / base[k]:+.1f}%" if base[k] else ""
            print(f"  {k:16s} {base[k]:>10g} -> {t[k]:<10g} {pct}")
        moved = []
        for n, r in rows.items():
            o = old["rows"].get(n)
            if o and o["length"] > 0:
                moved.append((r["length"] / o["length"] - 1.0, n, o["length"], r["length"]))
        moved.sort()
        if moved:
            print("  track length, biggest changes:")
            shown = moved if len(moved) <= 6 else moved[:3] + moved[-3:]
            for d, n, o, r in shown:
                print(f"    {n:26s} {o:6.0f} -> {r:6.0f}  {100 * d:+.1f}%")


if __name__ == "__main__":
    main()
