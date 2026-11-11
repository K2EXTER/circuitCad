"""Collect the earlier versions of rebuilt bundled models from the git history into crates/part-lib/legacy_models.json.

A user's library keeps its own copy of each bundled part. When a bundled model is rebuilt, the copy is replaced once, but
only when its model text is one of the versions circuitCad shipped earlier (never one the user edited): those versions
are listed in legacy_models.json (`install_examples` in crates/part-lib/src/lib.rs). Run this after rebuilding models,
before committing them:

  python tools/collect_legacy_models.py                 the parts whose model differs from HEAD (the usual case)
  python tools/collect_legacy_models.py --since REV     the parts whose model differs from REV
  python tools/collect_legacy_models.py NAME [NAME ...] these parts (example names, e.g. LM358_SOIC-8)
  add --check to only report what would change (exit code 1 when something is missing)

For each part it walks the part file's history and adds every earlier `sim.text` and `description`
that differs from the current one. Entries already in the file are kept; nothing is removed.
"""

import json
import os
import subprocess
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
EXAMPLES = "crates/part-lib/examples"
OUT = os.path.join(ROOT, "crates/part-lib/legacy_models.json")


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, check=True).stdout.decode("utf-8")


def part_at(rev, path):
    """The part JSON of `path` at `rev` (None when it did not exist or did not parse)."""
    try:
        return json.loads(git("show", f"{rev}:{path}"))
    except (subprocess.CalledProcessError, json.JSONDecodeError):
        return None


def current(name):
    with open(os.path.join(ROOT, EXAMPLES, name + ".part.json"), encoding="utf-8") as f:
        return json.load(f)


def model_text(part):
    return ((part or {}).get("sim") or {}).get("text")


def changed_since(rev):
    """Example names whose model text differs from the one at `rev` (parts new since then have no earlier version)."""
    names = []
    for f in sorted(os.listdir(os.path.join(ROOT, EXAMPLES))):
        if not f.endswith(".part.json"):
            continue
        name = f[: -len(".part.json")]
        old = model_text(part_at(rev, f"{EXAMPLES}/{f}"))
        if old is not None and old != model_text(current(name)):
            names.append(name)
    return names


def history(name):
    """Every earlier version of the part file, oldest first. Renames are not followed on purpose: git's rename detection
    also takes a new part copied from a similar one (LM324 from LM358) for the same file, and the copy's source models
    were never shipped as this part. Bundled part files have never been renamed (check with `git log --diff-filter=R`)."""
    path = f"{EXAMPLES}/{name}.part.json"
    shas = git("log", "--format=%H", "--", path).split()
    return [p for p in (part_at(sha, path) for sha in reversed(shas)) if p]


def collect(names, table):
    """Adds the earlier versions of `names` to `table`; returns what was added, per part."""
    added = {}
    for name in names:
        now = current(name)
        entry = table.setdefault(name, {"texts": [], "descriptions": []})
        for old in history(name):
            text, desc = model_text(old), old.get("description")
            if text and text != model_text(now) and text not in entry["texts"]:
                entry["texts"].append(text)
                added.setdefault(name, []).append("model")
            if desc and desc != now.get("description") and desc not in entry["descriptions"]:
                entry["descriptions"].append(desc)
                added.setdefault(name, []).append("description")
        if not entry["texts"]:
            del table[name]  # no earlier model: nothing to replace
    return added


def main(argv):
    check = "--check" in argv
    args = [a for a in argv if a != "--check"]
    if args[:1] == ["--since"]:
        names = changed_since(args[1])
    elif args:
        names = args
    else:
        names = changed_since("HEAD")
    with open(OUT, encoding="utf-8") as f:
        table = json.load(f)
    added = collect(names, table)
    for name in names:
        what = added.get(name)
        print(f"{name}: " + (f"{what.count('model')} model(s), {what.count('description')} description(s) added" if what else "nothing new"))
    if check:
        return 1 if added else 0
    if added:
        with open(OUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(table, ensure_ascii=False, indent=1) + "\n")
        print("wrote", os.path.relpath(OUT, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
