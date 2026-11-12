"""Compare the simulator (sim-core) before and after a change: answers, waveforms and speed.

A snapshot holds:
  bench.txt     the output of `tests_bench` (operating-point strategies over every example, transients, fault scan ...)
  waves_*.tsv   every signal of the combined demo without / with its board wiring and of the op-amp circuit, at every point
  waves.txt     those three runs' best time and points
  examples.tsv  every bundled example's transient (the settings `xcheck_tran` uses): best time, points, retries, rejected
                steps and a fingerprint of all its values
  ngspice.tsv   (with --ngspice) ngspice's own transient time and points for the same examples

Usage:
    python tools/sim_compare.py snapshot before            # the working tree as it is now
    python tools/sim_compare.py snapshot base --rev HEAD   # a commit, built in its own worktree (the working tree is not touched)
    ... change the code ...
    python tools/sim_compare.py snapshot after
    python tools/sim_compare.py compare before after
    python tools/sim_compare.py clean                      # remove the worktrees made by --rev (snapshots stay)

Snapshot options:
    --rev REV        build and measure a commit instead of the working tree (git worktree under target/sim-compare/,
                     with its own target directory: the first build takes a few minutes, later ones are incremental)
    --ngspice [EXE]  also time ngspice on the same examples (default F:\\Tools\\ngspice\\Spice64\\bin\\ngspice_con.exe or
                     $NGSPICE)
    --no-bench       skip tests_bench (about 30 s)
    --only A,B       only these examples (also leaves the demos and the bench out)
    --repeat N       runs per case, the best time is kept (default 3)

Compare options:
    --time-share F   report an example whose time changed by more than this share (default 0.15)
    --time-floor S   ... and by more than this many seconds (default 0.0005)

Snapshots live in target/sim-compare/<name>/ (not in git). Builds use `-p sim-core -p app` only: a release build of the
whole workspace fails while the MCP server holds ccad.exe.
"""
import argparse
import difflib
import os
import re
import shutil
import statistics
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = os.path.join(ROOT, "target", "sim-compare")
DEFAULT_NGSPICE = r"F:\Tools\ngspice\Spice64\bin\ngspice_con.exe"
TIME = re.compile(r"\d+(\.\d+)?(e-?\d+)? ?(ms|s|µs|us)\b")


def run(cmd, cwd, env=None, out=None):
    print("$", " ".join(cmd), flush=True)
    r = subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
    if out:
        with open(out, "w", encoding="utf-8") as f:
            f.write(r.stdout)
    if r.returncode != 0:
        tail = "\n".join(r.stdout.splitlines()[-30:])
        sys.exit(f"failed ({r.returncode}): {' '.join(cmd)}\n{tail}")
    return r.stdout


def worktree(rev):
    """A detached worktree of `rev` and its own target directory."""
    sha = run(["git", "rev-parse", "--short", rev], ROOT).strip()
    path = os.path.join(BASE, "wt", sha)
    if not os.path.isdir(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        run(["git", "worktree", "add", "--detach", path, rev], ROOT)
        add_snapshot_test(path)
    return path, os.path.join(BASE, "wt-target")


def add_snapshot_test(path):
    """A commit from before the snapshot test existed gets this tree's copy (it only reads the simulator's public API)."""
    test = os.path.join("crates", "app", "src", "tests", "sim_snapshot.rs")
    if os.path.isfile(os.path.join(path, test)):
        return
    shutil.copy(os.path.join(ROOT, test), os.path.join(path, test))
    mod = os.path.join(path, "crates", "app", "src", "tests", "mod.rs")
    s = open(mod, encoding="utf-8").read()
    open(mod, "w", encoding="utf-8").write(s.replace("mod flow;\n", "mod flow;\nmod sim_snapshot;\n", 1))
    xt = os.path.join(path, "crates", "app", "src", "xcheck_tran.rs")
    s = open(xt, encoding="utf-8").read()
    open(xt, "w", encoding="utf-8").write(s.replace("\nfn own_dynamics(", "\npub(crate) fn own_dynamics(", 1))


def snapshot(a):
    out = os.path.join(BASE, a.name)
    if os.path.isdir(out):
        shutil.rmtree(out)
    os.makedirs(out)
    src, target = (worktree(a.rev) if a.rev else (ROOT, None))
    env = dict(os.environ)
    if target:
        env["CARGO_TARGET_DIR"] = target
    env["SIM_SNAPSHOT_DIR"] = out
    env["SIM_SNAPSHOT_REPEAT"] = str(a.repeat)
    if a.only:
        env["SIM_SNAPSHOT_ONLY"] = a.only
    if a.ngspice is not None:
        exe = a.ngspice or os.environ.get("NGSPICE") or DEFAULT_NGSPICE
        if not os.path.isfile(exe):
            sys.exit(f"ngspice not found: {exe}")
        env["SIM_SNAPSHOT_NGSPICE"] = exe
    if not a.no_bench and not a.only:
        run(["cargo", "test", "--release", "-p", "sim-core", "--lib", "tests_bench", "--", "--ignored", "--nocapture", "--test-threads=1"], src, env, os.path.join(out, "bench.txt"))
    log = run(["cargo", "test", "--release", "-p", "app", "sim_snapshot", "--", "--ignored", "--nocapture", "--test-threads=1"], src, env)
    if "sim_snapshot ... ok" not in log and "1 passed" not in log:
        sys.exit("the snapshot test did not run (is crates/app/src/tests/sim_snapshot.rs in this revision?)")
    rev = run(["git", "rev-parse", "--short", "HEAD"], src).strip()
    dirty = "" if a.rev else (" + uncommitted changes" if run(["git", "status", "--porcelain", "--untracked-files=no"], src).strip() else "")
    with open(os.path.join(out, "meta.txt"), "w", encoding="utf-8") as f:
        f.write(f"{rev}{dirty}\n")
    print(f"snapshot '{a.name}' ({rev}{dirty}) in {out}")


def table(path):
    if not os.path.isfile(path):
        return None
    with open(path, encoding="utf-8") as f:
        lines = f.read().strip().split("\n")
    head = lines[0].split("\t")
    return {l.split("\t")[0]: dict(zip(head, l.split("\t"))) for l in lines[1:] if l}


def bench_lines(path):
    if not os.path.isfile(path):
        return None
    keep = []
    with open(path, encoding="utf-8") as f:
        for l in f:
            l = l.rstrip("\n")
            if re.search(r"Compiling|Finished|Running|running \d+ test|^warning|filtered out", l):
                continue
            keep.append(TIME.sub("T", l))
    return keep


def ms(x):
    return f"{float(x) * 1e3:.2f} ms"


def compare(a):
    da, db = os.path.join(BASE, a.a), os.path.join(BASE, a.b)
    for d in (da, db):
        if not os.path.isdir(d):
            sys.exit(f"no snapshot {d}")
    meta = lambda d: open(os.path.join(d, "meta.txt"), encoding="utf-8").read().strip() if os.path.isfile(os.path.join(d, "meta.txt")) else "?"
    print(f"== {a.a} ({meta(da)})  ->  {a.b} ({meta(db)})")

    # bench: every line but the times
    ba, bb = bench_lines(os.path.join(da, "bench.txt")), bench_lines(os.path.join(db, "bench.txt"))
    if ba is not None and bb is not None:
        diff = [l for l in difflib.unified_diff(ba, bb, lineterm="", n=0) if not l.startswith(("---", "+++", "@@"))]
        print(f"\n[bench] {'same (all but the times)' if not diff else f'{len(diff)} lines differ (times left out):'}")
        for l in diff[:60]:
            print("  " + l)
    else:
        print("\n[bench] not in both snapshots")

    # demo waveforms
    wa, wb = table_waves(da), table_waves(db)
    if wa and wb:
        print("\n[demo waveforms]")
        for case in ("plain", "postlayout", "opamp"):
            fa, fb = os.path.join(da, f"waves_{case}.tsv"), os.path.join(db, f"waves_{case}.tsv")
            if not (os.path.isfile(fa) and os.path.isfile(fb)):
                continue
            same = open(fa, "rb").read() == open(fb, "rb").read()
            (ta, pa), (tb, pb) = wa.get(case, ("?", "?")), wb.get(case, ("?", "?"))
            print(f"  {case:<11} {'identical' if same else 'DIFFER':<10} {float(ta):.3f} s -> {float(tb):.3f} s, {pa} -> {pb} points" + ("" if same else f"  ({wave_diff(fa, fb)})"))

    # every example
    ea, eb = table(os.path.join(da, "examples.tsv")), table(os.path.join(db, "examples.tsv"))
    if ea and eb:
        common = [k for k in ea if k in eb]
        changed_wave = [k for k in common if ea[k]["fingerprint"] != eb[k]["fingerprint"]]
        slower, faster = [], []
        for k in common:
            ta, tb = float(ea[k]["seconds"]), float(eb[k]["seconds"])
            if abs(tb - ta) > max(a.time_share * ta, a.time_floor):
                (slower if tb > ta else faster).append(k)
        tot = lambda e: sum(float(e[k]["seconds"]) for k in common)
        print(f"\n[examples] {len(common)} in both; total {tot(ea):.3f} s -> {tot(eb):.3f} s")
        print(f"  waveform changed: {len(changed_wave)}" + ("" if not changed_wave else " -> " + ", ".join(changed_wave)))
        for label, ks in (("slower", slower), ("faster", faster)):
            if ks:
                print(f"  {label} (> {a.time_share:.0%} and > {a.time_floor * 1e3:g} ms):")
                for k in sorted(ks, key=lambda k: -abs(float(eb[k]['seconds']) - float(ea[k]['seconds']))):
                    print(f"    {k:<26} {ms(ea[k]['seconds'])} -> {ms(eb[k]['seconds'])}, points {ea[k]['points']} -> {eb[k]['points']}, retries {ea[k]['retries']} -> {eb[k]['retries']}")
        for k in changed_wave:
            if k not in slower and k not in faster:
                print(f"    {k:<26} (waveform only) {ms(ea[k]['seconds'])} -> {ms(eb[k]['seconds'])}, points {ea[k]['points']} -> {eb[k]['points']}")
        errs = [k for k in common if eb[k]["fingerprint"].startswith("ERR") and not ea[k]["fingerprint"].startswith("ERR")]
        if errs:
            print("  NEW FAILURES: " + ", ".join(f"{k} ({eb[k]['fingerprint']})" for k in errs))
        only = sorted(set(ea) ^ set(eb))
        if only:
            print("  in one snapshot only: " + ", ".join(only))

    # ngspice
    for name, d, e in ((a.a, da, ea), (a.b, db, eb)):
        ng = table(os.path.join(d, "ngspice.tsv"))
        if not ng or not e:
            continue
        rows = []
        for k, v in ng.items():
            try:
                t = float(v["seconds"])
            except ValueError:
                continue
            if v.get("note") or k not in e or t <= 0:
                continue
            rows.append((k, float(e[k]["seconds"]), t, e[k]["points"], v["points"]))
        if not rows:
            continue
        r = sorted(x[1] / x[2] for x in rows)
        print(f"\n[ngspice, {name}] {len(rows)} examples: ours {sum(x[1] for x in rows):.3f} s, ngspice {sum(x[2] for x in rows):.3f} s; "
              f"ours/ngspice median {statistics.median(r):.2f}, faster in {sum(1 for x in r if x < 1)}")
        for k, o, t, po, pt in sorted(rows, key=lambda x: -x[1] / x[2])[:8]:
            if o / t < 1.5:
                break
            print(f"  {k:<26} ours {ms(o)} ({po} points), ngspice {ms(t)} ({pt} points): x{o / t:.1f}")
        failed = [k for k, v in ng.items() if v.get("note")]
        if failed:
            print("  ngspice failed: " + ", ".join(failed))


def table_waves(d):
    p = os.path.join(d, "waves.txt")
    if not os.path.isfile(p):
        return None
    out = {}
    for l in open(p, encoding="utf-8").read().strip().split("\n"):
        c = l.split("\t")
        out[c[0]] = (c[1], c[2])
    return out


def wave_diff(fa, fb):
    """Where two waveform files first part, and the largest difference on the shared time points."""
    la, lb = open(fa, encoding="utf-8").read().split("\n"), open(fb, encoding="utf-8").read().split("\n")
    if la[0] != lb[0]:
        return "different signals"
    ra = {r.split("\t")[0]: r.split("\t")[1:] for r in la[1:] if r}
    rb = {r.split("\t")[0]: r.split("\t")[1:] for r in lb[1:] if r}
    names = la[0].split("\t")[1:]
    shared = [t for t in ra if t in rb]
    worst = (0.0, "", "")
    for t in shared:
        for i, (x, y) in enumerate(zip(ra[t], rb[t])):
            d = abs(float(x) - float(y))
            if d > worst[0]:
                worst = (d, names[i], t)
    first = next((i for i, (x, y) in enumerate(zip(la, lb)) if x != y), None)
    t0 = la[first].split("\t")[0] if first is not None and first < len(la) else "?"
    return f"first difference at t={t0}; on {len(shared)} shared points the largest is {worst[0]:.3g} in {worst[1]} at t={worst[2]}"


def clean(_a):
    wt = os.path.join(BASE, "wt")
    if os.path.isdir(wt):
        for d in os.listdir(wt):
            run(["git", "worktree", "remove", "--force", os.path.join(wt, d)], ROOT)
    run(["git", "worktree", "prune"], ROOT)
    print("worktrees removed (their target directory target/sim-compare/wt-target is kept for the next --rev)")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("snapshot")
    s.add_argument("name")
    s.add_argument("--rev")
    s.add_argument("--ngspice", nargs="?", const="")
    s.add_argument("--no-bench", action="store_true")
    s.add_argument("--only")
    s.add_argument("--repeat", type=int, default=3)
    c = sub.add_parser("compare")
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--time-share", type=float, default=0.15)
    c.add_argument("--time-floor", type=float, default=0.0005)
    sub.add_parser("clean")
    a = p.parse_args()
    {"snapshot": snapshot, "compare": compare, "clean": clean}[a.cmd](a)


if __name__ == "__main__":
    main()
