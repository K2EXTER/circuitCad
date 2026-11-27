"""Run the tests (and clippy) of only the crates a change can affect.

    python tools/test_changed.py            # what differs from origin/main: commits not pushed, edits, new files
    python tools/test_changed.py --list     # only say which crates and why
    python tools/test_changed.py --base HEAD --no-clippy

A changed file in crates/<name>/ picks that crate and every workspace crate that depends on it (from `cargo metadata`).
A changed file elsewhere picks the crates whose sources name it: its file name, or a folder of two levels or more that holds
it, written in a string of the source (`docs/examples` is read by app, sim-core and the cli tests). Cargo.toml, Cargo.lock, .cargo/ and the CI files pick
everything. Anything else (most of docs/, tools/) picks nothing.

The test binaries are built once and then run at the same time, clippy beside them (`run_tests`): one line a binary, a failing
one's whole output. This is for while working. Before a push, and in CI, the whole workspace still runs (`cargo test --workspace`).

At the end, the old builds are taken out of target/ (`tools/clean_target.py --auto`, about 2 s): it would grow by gigabytes a day.
"""

import argparse
import json
import os
import re
import subprocess
import time
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVERYTHING = ("Cargo.toml", "Cargo.lock", ".cargo/", ".github/", "rust-toolchain")


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout


def changed_files(base):
    files = set()
    try:
        files.update(git("diff", "--name-only", f"{base}...HEAD").split())
    except subprocess.CalledProcessError:
        sys.exit(f"git does not know {base}; give another with --base")
    files.update(git("diff", "--name-only", "HEAD").split())
    files.update(git("ls-files", "--others", "--exclude-standard").split())
    return sorted(f for f in files if f)


def workspace():
    """{crate name: folder (relative, with /)} and {crate: set of workspace crates it depends on}."""
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout)
    folders, deps = {}, {}
    for p in meta["packages"]:
        folders[p["name"]] = os.path.relpath(os.path.dirname(p["manifest_path"]), ROOT).replace(os.sep, "/")
    for p in meta["packages"]:
        deps[p["name"]] = {d["name"] for d in p["dependencies"] if d["name"] in folders}
    return folders, deps


def sources(folder):
    """The string literals of a crate's Rust files, where the names of the files it reads are (`include_str!("../../docs/..")`,
    `.join("../../docs/examples")`); a comment that only mentions a file does not count."""
    text = []
    for root, _, files in os.walk(os.path.join(ROOT, folder)):
        for f in files:
            if f.endswith(".rs"):
                with open(os.path.join(root, f), encoding="utf-8", errors="replace") as h:
                    code = h.read()
                code = re.sub(r"//[^\n]*", "", code)
                text.extend(re.findall(r'"((?:[^"\\]|\\.)*)"', code))
    return "\n".join(text)


def pick(files, folders, deps):
    """{crate: reason} for the crates the files touch directly."""
    picked = {}
    texts = None
    for f in files:
        if f.startswith(EVERYTHING) or f in EVERYTHING:
            return {c: f"{f} changed (everything)" for c in folders}
        owner = next((c for c, d in folders.items() if f.startswith(d + "/")), None)
        if owner:
            picked.setdefault(owner, f)
            continue
        if texts is None:
            texts = {c: sources(d) for c, d in folders.items()}
        parts = f.split("/")
        names = ["/".join(parts[:k]) for k in range(2, len(parts))]
        if "." in parts[-1] and len(parts[-1]) > 6:
            names.append(parts[-1])
        for c, t in texts.items():
            hit = next((n for n in names if n in t), None)
            if hit:
                picked.setdefault(c, f"{f} (its source names {hit})")
    return picked


def with_dependents(picked, deps):
    """The picked crates and every crate that depends on one of them, with the reason."""
    out = dict(picked)
    grew = True
    while grew:
        grew = False
        for c, ds in deps.items():
            if c in out:
                continue
            via = next((d for d in ds if d in out), None)
            if via:
                out[c] = f"depends on {via}"
                grew = True
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--base", default="origin/main", help="compare with this commit (default origin/main)")
    ap.add_argument("--list", action="store_true", help="only print the crates and why")
    ap.add_argument("--no-clippy", action="store_true", help="skip clippy")
    a = ap.parse_args()
    files = changed_files(a.base)
    folders, deps = workspace()
    crates = with_dependents(pick(files, folders, deps), deps)
    print(f"{len(files)} changed file(s) since {a.base}")
    if not crates:
        print("no crate is affected: nothing to test")
        return
    for c in sorted(crates):
        print(f"  {c:12} {crates[c]}")
    if a.list:
        return
    ps = [x for c in sorted(crates) for x in ("-p", c)]
    ok = run_tests(sorted(crates), ps, not a.no_clippy)
    clean_target()
    if not ok:
        sys.exit(1)


def clean_target():
    """The old builds out of target/, quietly (a line when something went). A failure here never fails the tests."""
    try:
        subprocess.run([sys.executable, "-I", os.path.join(ROOT, "tools", "clean_target.py"), "--auto"], cwd=ROOT, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        print(f"clean_target.py did not run: {e}")


def run_tests(crates, ps, clippy):
    """The test binaries built once, then all run at the same time (`cargo test` runs them one after another: the app's 850 tests,
    then part-lib's, then sch-core's ...), clippy beside them. Each binary's result in a line; a failing one's whole output. The doc
    tests are left to the full run (`cargo test --workspace`, CI): the crates have none."""
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from test_times import binaries

    t0 = time.monotonic()
    found = binaries(crates)
    built = time.monotonic() - t0
    print(f"built {len(found)} test binaries in {built:.0f} s")
    jobs = [(target, subprocess.Popen([exe], cwd=folder, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")) for target, folder, exe in found]
    lint = subprocess.Popen(["cargo", "clippy", *ps, "--all-targets", "--quiet"], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace") if clippy else None
    ok = True
    for target, job in jobs:
        out, _ = job.communicate()
        result = next((l for l in reversed(out.splitlines()) if l.startswith("test result:")), "(no result line)")
        print(f"  {target:14} {result}")
        if job.returncode != 0:
            ok = False
            print(out[-20000:])
    print(f"tests done in {time.monotonic() - t0:.0f} s")
    if lint:
        out, _ = lint.communicate()
        errors = [l for l in out.splitlines() if l.startswith("error")]
        print(f"clippy: {len(errors)} error(s), {sum(1 for l in out.splitlines() if l.startswith('warning'))} warning line(s)")
        if lint.returncode != 0:
            ok = False
            print(out[-20000:])
    return ok


if __name__ == "__main__":
    main()
