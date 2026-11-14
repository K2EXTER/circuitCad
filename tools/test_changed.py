"""Run the tests (and clippy) of only the crates a change can affect.

    python tools/test_changed.py            # what differs from origin/main: commits not pushed, edits, new files
    python tools/test_changed.py --list     # only say which crates and why
    python tools/test_changed.py --base HEAD --no-clippy

A changed file in crates/<name>/ picks that crate and every workspace crate that depends on it (from `cargo metadata`).
A changed file elsewhere picks the crates whose sources name it: its file name, or a folder of two levels or more that holds
it, written in a string of the source (`docs/examples` is read by app, sim-core and the cli tests). Cargo.toml, Cargo.lock, .cargo/ and the CI files pick
everything. Anything else (most of docs/, tools/) picks nothing.

This is for while working. Before a push, and in CI, the whole workspace still runs (`cargo test --workspace`).
"""

import argparse
import json
import os
import re
import subprocess
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
    if subprocess.run(["cargo", "test", *ps], cwd=ROOT).returncode != 0:
        sys.exit(1)
    if not a.no_clippy and subprocess.run(["cargo", "clippy", *ps, "--all-targets"], cwd=ROOT).returncode != 0:
        sys.exit(1)


if __name__ == "__main__":
    main()
