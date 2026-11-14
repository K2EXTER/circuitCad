"""How long each test takes, the slowest first.

    python tools/test_times.py                 # every test binary of the workspace, as `cargo test` runs them
    python tools/test_times.py -p app -p cli   # only these crates
    python tools/test_times.py --serial        # one test at a time: each one's own time, without the others slowing it

Builds the tests (`cargo test --no-run`), then runs every test binary from its crate's folder (as cargo does) with libtest's
`--report-time` (an unstable option, allowed by RUSTC_BOOTSTRAP=1 for the test binary only; nothing is rebuilt). Prints the
slowest tests, the time of each binary, and the tests over the limit (`--limit`, 2 s by default): such a test should run its
cases in parallel (`sim_core::par_map`), share what it builds, or be `#[ignore]`d with a reason (docs/code-notes/tests.md).
With the tests running side by side, a test's time includes waiting for the others; `--serial` gives each one's own time.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def binaries(packages):
    """(package, crate folder, test binary) of every test target, built first."""
    ps = [x for p in packages for x in ("-p", p)] or ["--workspace"]
    out = subprocess.run(["cargo", "test", *ps, "--no-run", "--message-format=json"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
    if out.returncode != 0:
        sys.exit(out.stderr[-3000:])
    found = []
    for line in out.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") == "compiler-artifact" and m.get("executable") and m["profile"]["test"]:
            folder = os.path.dirname(m["manifest_path"])
            found.append((m["target"]["name"], folder, m["executable"]))
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("-p", dest="packages", action="append", default=[], help="a crate (repeat for several)")
    ap.add_argument("--serial", action="store_true", help="one test at a time")
    ap.add_argument("--limit", type=float, default=2.0, help="seconds over which a test is listed as too slow (default 2)")
    ap.add_argument("--top", type=int, default=20, help="how many of the slowest to print (default 20)")
    a = ap.parse_args()
    env = dict(os.environ, RUSTC_BOOTSTRAP="1")
    times, per_binary, failed = [], [], []
    for target, folder, exe in binaries(a.packages):
        args = [exe, "-Zunstable-options", "--report-time"] + (["--test-threads=1"] if a.serial else [])
        t0 = time.monotonic()
        out = subprocess.run(args, cwd=folder, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
        wall = time.monotonic() - t0
        n = 0
        for line in out.stdout.splitlines():
            m = re.match(r"test (\S+) \.\.\. (ok|FAILED) <([0-9.]+)s>", line)
            if m:
                times.append((float(m.group(3)), f"{target}::{m.group(1)}"))
                n += 1
                if m.group(2) == "FAILED":
                    failed.append(f"{target}::{m.group(1)}")
        per_binary.append((wall, target, n))
    times.sort(reverse=True)
    print(f"slowest {min(a.top, len(times))} of {len(times)} tests ({'one at a time' if a.serial else 'side by side'}):")
    for t, name in times[: a.top]:
        print(f"  {t:7.2f} s  {name}")
    print("\ntest binaries (wall time):")
    for wall, target, n in sorted(per_binary, reverse=True):
        if n:
            print(f"  {wall:7.2f} s  {target} ({n} tests)")
    print(f"  {sum(w for w, _, _ in per_binary):7.2f} s  in all")
    slow = [(t, n) for t, n in times if t > a.limit]
    print(f"\n{len(slow)} test(s) over {a.limit:g} s" + (":" if slow else ""))
    for t, name in slow:
        print(f"  {t:7.2f} s  {name}")
    if failed:
        print("\nFAILED: " + ", ".join(failed))
        sys.exit(1)


if __name__ == "__main__":
    main()
