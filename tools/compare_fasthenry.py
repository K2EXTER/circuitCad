import csv, json, math, os, re, sys

S = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, r"F:\circuitCad\tools")
import fasthenry_cases as fc

w = 2 * math.pi * 1000.0
ours_file = sys.argv[1] if len(sys.argv) > 1 else "ours2.csv"
write_csv = len(sys.argv) > 2


def read_mat(path):
    txt = open(path, encoding="ascii", errors="replace").read().splitlines()
    mat = []
    for l in txt:
        nums = re.findall(r"([-+]?[0-9.]+(?:e[-+]?\d+)?)\s+([-+][0-9.]+(?:e[-+]?\d+)?)j", l)
        if nums and not l.startswith(("Row", "Imped")):
            mat.append([(float(a), float(b)) for a, b in nums])
    return mat


ours = {r[0]: float(r[1]) for r in csv.reader(open(os.path.join(S, ours_file))) if len(r) == 2 and re.match(r"^-?[0-9.]+$", r[1])}
rows = []
for c in fc.cases():
    if "plane" in c:
        continue  # FastHenry2 3.6 did not run plane inputs here; see ref_microstrip.py
    p = os.path.join(S, "fh2", fc.fname(c["name"]) + ".mat")
    mat = read_mat(p)
    fh = mat[0][0][1] / w * 1e9 if c["kind"] == "self" else 0.5 * (mat[0][1][1] + mat[1][0][1]) / w * 1e9
    rows.append((c["name"], c["kind"], fh, ours[c["name"]]))
print(f"{'case':36s} {'FastHenry':>10s} {'ours':>10s}   diff")
worst = {}
for name, kind, fh, o in rows:
    d = (o / fh - 1) * 100 if abs(fh) > 1e-6 else float("nan")
    group = kind
    if not any(name.startswith(x) for x in ("self l", "mutual l")):
        print(f"{name:36s} {fh:10.3f} {o:10.3f}  {d:+6.2f}%")
        worst[name.split()[0]] = max(worst.get(name.split()[0], 0), abs(d) if d == d else 0)
print("worst |diff| per group:", {k: round(v, 2) for k, v in worst.items()})
if write_csv:
    with open(r"F:\circuitCad\tools\fasthenry_results.csv", "w", encoding="ascii", newline="\n") as f:
        f.write("case,fasthenry_nH,ours_nH\n")
        for name, kind, fh, o in rows:
            f.write(f"{name},{fh:.3f},{o:.3f}\n")
