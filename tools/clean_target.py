"""Remove old build outputs from target/ that Cargo never deletes by itself.

    python tools/clean_target.py              # remove versions older than 3 days that a newer build replaced
    python tools/clean_target.py --dry-run    # only say what would go and how much
    python tools/clean_target.py --days 7
    python tools/clean_target.py --extra      # also remove extra build folders in target/ (another CARGO_TARGET_DIR)
    python tools/clean_target.py --auto       # quietly, one line when something went (tools/test_changed.py runs it at its end)

Cargo names each build of a crate `<name>-<hash>`; the hash changes with the dependencies, the features and the
settings, and the old one stays. With several sessions building in turn, target/debug/deps grows by gigabytes a week
(2026-11-17: 4.7 GB of 7.6 GB untouched for a day or more). A version goes when a newer version of the same name was
built more than --days after it and it is itself older than --days, so a crate built once and still in use (most
dependencies) is never touched. Its files in deps/ and its folders in .fingerprint/, build/ and incremental/ go
together. Removing too much only costs a rebuild of that crate.

rustc's incremental folders (incremental/<crate>-<hash>/) are not named like deps/ and hold one session folder
(s-<time>-<id>-<svh>, with s-<time>-<id>.lock) per build; an older session that rustc did not delete stays beside the
newest and doubles the folder (2026-11-18: 6.1 GB, the larger crates with two sessions of 350-580 MB). All sessions but
the newest go when they are more than an hour old (a build in another session may still be writing the newest).

Each way of building a crate (build, test, clippy, another profile or setting) has its own incremental folder
(<crate>-<base32 hash>, not the deps/ hash), and rustc never removes one: the app's are up to 3 GB each, and a day of
building filled 23 GB with 188 of them (2026-11-27). A folder untouched for --inc-days (1 by default) goes: it only costs that
way of building one rebuild without the cache. Past that, while the incremental folders together are over --max-gb (20 by
default), the least recently used go first, never one touched within the last hour. The folders in use come to about 12 GB
(build, test and clippy of every crate): a cap below that takes one in use, and the app without its cache took 190 s to build
instead of 10.

Loose files right in target/ (screenshots, a quick .dxf or .svg written there by hand) go when older than --days;
Cargo's own .rustc_info.json and CACHEDIR.TAG stay.

Folders in target/ that are not a profile Cargo writes here (debug, release), rust-analyzer's (flycheck*), or a tool's
(sim-compare, doc) are another build folder made by pointing CARGO_TARGET_DIR at it: a whole second copy of every
dependency (target/shot 2.9 GB, target/ui-impl 3.6 GB). They are listed always and removed with --extra.
"""

import argparse
import os
import re
import shutil
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.path.join(ROOT, "target")
PROFILES = ("debug", "release")
KNOWN = set(PROFILES) | {"sim-compare", "doc", "tmp", "package", "flycheck"}
# libfoo-0123456789abcdef.rlib, foo-0123456789abcdef.exe, foo-0123456789abcdef (folder)
NAMED = re.compile(r"^(?:lib)?(?P<name>.+)-(?P<hash>[0-9a-f]{16})(?:\..*)?$")
DAY = 86400.0


def newest(path):
    """Newest modification time of a file, or of a folder and what is in it."""
    t = os.path.getmtime(path)
    if os.path.isdir(path):
        for d, _, files in os.walk(path):
            for f in files:
                try:
                    t = max(t, os.path.getmtime(os.path.join(d, f)))
                except OSError:
                    pass
    return t


def size(path):
    if os.path.isfile(path):
        return os.path.getsize(path)
    total = 0
    for d, _, files in os.walk(path):
        for f in files:
            try:
                total += os.path.getsize(os.path.join(d, f))
            except OSError:
                pass
    return total


def remove(path):
    try:
        if os.path.isdir(path):
            shutil.rmtree(path)
        else:
            os.remove(path)
        return True
    except OSError:
        return False  # a running exe is locked on Windows: it stays


def stale_versions(profile, days, now):
    """(paths, bytes) of the versions in one profile folder that newer builds replaced."""
    # name -> hash -> [paths], and the newest time of each version
    versions = {}
    for sub in ("deps", ".fingerprint", "build", "incremental"):
        folder = os.path.join(profile, sub)
        if not os.path.isdir(folder):
            continue
        for entry in os.listdir(folder):
            m = NAMED.match(entry)
            if m:
                versions.setdefault(m["name"], {}).setdefault(m["hash"], []).append(os.path.join(folder, entry))
    stale = []
    for hashes in versions.values():
        if len(hashes) < 2:
            continue
        times = {h: max(newest(p) for p in paths) for h, paths in hashes.items()}
        latest = max(times.values())
        for h, t in times.items():
            if latest - t > days * DAY and now - t > days * DAY:
                stale.extend(hashes[h])
    return stale


CARGO_FILES = {".rustc_info.json", "CACHEDIR.TAG"}
SESSION_MIN_AGE = 3600.0


def old_sessions(profile, now):
    """Incremental session folders (and their lock files) that a newer session of the same crate replaced."""
    folder = os.path.join(profile, "incremental")
    if not os.path.isdir(folder):
        return []
    stale = []
    for crate in os.listdir(folder):
        path = os.path.join(folder, crate)
        if not os.path.isdir(path):
            continue
        sessions = [e for e in os.listdir(path) if e.startswith("s-") and os.path.isdir(os.path.join(path, e))]
        if len(sessions) < 2:
            continue
        times = {s: newest(os.path.join(path, s)) for s in sessions}
        latest = max(sessions, key=times.get)
        for s in sessions:
            if s == latest or now - times[s] < SESSION_MIN_AGE:
                continue
            stale.append(os.path.join(path, s))
            # s-<time>-<id>-<svh> locks with s-<time>-<id>.lock
            lock = os.path.join(path, s.rsplit("-", 1)[0] + ".lock")
            if os.path.isfile(lock):
                stale.append(lock)
    return stale


def incremental_folders(profile):
    """(newest time, bytes, path) of every crate's incremental folder of a profile."""
    folder = os.path.join(profile, "incremental")
    if not os.path.isdir(folder):
        return []
    out = []
    for crate in os.listdir(folder):
        path = os.path.join(folder, crate)
        if os.path.isdir(path):
            out.append((newest(path), size(path), path))
    return out


def unused_incremental(profile, days, max_bytes, now, taken):
    """Incremental folders not used for `days`, then the least recently used ones while the rest is over `max_bytes`.
    `taken`: paths chosen already (old sessions inside a folder), not counted in the rest."""
    def left(path, n):
        inside = sum(size(x) for x in taken if x.startswith(path + os.sep))
        return n - inside
    folders = sorted(incremental_folders(profile))
    out = [p for t, _, p in folders if now - t > days * DAY]
    rest = [(t, left(p, n), p) for t, n, p in folders if p not in out]
    total = sum(n for _, n, _ in rest)
    for t, n, p in rest:
        if total <= max_bytes:
            break
        if now - t < SESSION_MIN_AGE:
            continue
        out.append(p)
        total -= n
    return out


def loose_files(days, now):
    """Files right in target/ that are not Cargo's and are older than `days`."""
    out = []
    for entry in os.listdir(TARGET):
        path = os.path.join(TARGET, entry)
        if os.path.isfile(path) and entry not in CARGO_FILES and now - os.path.getmtime(path) > days * DAY:
            out.append(path)
    return out


def gb(n):
    return f"{n / 1024 ** 3:.2f} GB"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--days", type=float, default=3.0, help="keep versions built within this many days (default 3)")
    ap.add_argument("--dry-run", action="store_true", help="only report")
    ap.add_argument("--extra", action="store_true", help="also remove extra build folders in target/")
    ap.add_argument("--inc-days", type=float, default=1.0, help="remove incremental folders not used for this many days (default 1)")
    ap.add_argument("--max-gb", type=float, default=20.0, help="keep the incremental folders under this many GB, the least recently used going first (default 20)")
    ap.add_argument("--auto", action="store_true", help="quiet: one line, only when something was removed")
    a = ap.parse_args()
    if not os.path.isdir(TARGET):
        sys.exit("no target/ folder")
    now = time.time()
    verb = "would remove" if a.dry_run else "removed"

    total, kept = 0, 0

    def take(label, paths):
        nonlocal total, kept
        n = sum(size(x) for x in paths)
        if not a.dry_run:
            locked = [x for x in paths if not remove(x)]
            kept += len(locked)
            n -= sum(size(x) for x in locked if os.path.exists(x))
        total += n
        if not a.auto:
            print(f"{label}: {verb} {len(paths)}, {gb(n)}")

    for p in PROFILES:
        profile = os.path.join(TARGET, p)
        if not os.path.isdir(profile):
            continue
        take(f"target/{p} old versions", stale_versions(profile, a.days, now))
        sessions = old_sessions(profile, now)
        folders = unused_incremental(profile, a.inc_days, a.max_gb * 1024 ** 3, now, sessions)
        # a session inside a folder that goes, goes with it
        take(f"target/{p} old incremental sessions", [x for x in sessions if not any(x.startswith(f + os.sep) for f in folders)])
        take(f"target/{p} unused incremental folders", folders)
    take("target/ loose files", loose_files(a.days, now))

    for entry in sorted(os.listdir(TARGET)):
        path = os.path.join(TARGET, entry)
        if not os.path.isdir(path) or entry in KNOWN or entry.startswith("flycheck"):
            continue
        if a.auto:
            continue
        n = size(path)
        if a.extra:
            if a.dry_run or remove(path):
                total += n
                print(f"target/{entry}: {verb} the extra build folder, {gb(n)}")
            else:
                print(f"target/{entry}: could not remove (in use?), {gb(n)}")
        else:
            print(f"target/{entry}: extra build folder, {gb(n)} (--extra removes it)")

    if a.auto:
        if total:
            print(f"target/: {verb} {gb(total)} of old builds (tools/clean_target.py)")
        return
    if kept:
        print(f"{kept} files were in use and stay")
    print(f"total: {verb} {gb(total)}")


if __name__ == "__main__":
    main()
