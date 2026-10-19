"""Reference resistance between two pads of a copper pour, by an independent numerical solution (finite differences).

    python tools/ref_pour.py > tools/pour_reference.csv     (also writes tools/pour_cases.json)

The pour is a rectangle of sheet resistance rho / t (copper 1.72e-5 ohm mm over 35 um); every pad is a disc of radius r that
is held at one potential (one pad at 1 V, the other at 0 V), the rest is a square grid of resistors. The current that leaves the
first pad gives R = 1 V / I. A case may list `others`, round pads of other nets on the pour's layer ([x, y, radius]): the copper
within radius + `clear` of each is cut out of the pour, as the pour fill of the program does. The grid disc is not exactly a disc,
so each contact is corrected by the resistance of the ring between its equivalent radius and the nominal one; the grid is 0.125 mm
and the result is good to about 3 % (the steps 0.125 and 0.083 mm differ by 1.4 %, still converging slowly because of the
logarithmic singularity at each contact).
"""
import json
import math
import os
import sys

import numpy as np
import scipy.sparse as sp
import scipy.sparse.linalg as sla

RHO = 1.72e-5  # ohm mm
T = 0.035  # mm
RS = RHO / T
PAD_R = 0.8  # mm


def cases():
    out = []
    # (name, width, height, pad1, pad2)
    for w, h in ((100.0, 50.0), (50.0, 50.0), (30.0, 10.0)):
        cx, cy = w / 2, h / 2
        for d in (5.0, 10.0, 25.0, 50.0, 80.0):
            if d + 4 < w:
                out.append({"name": f"pour {w:g}x{h:g} pads {d:g} apart centred", "w": w, "h": h, "p1": [cx - d / 2, cy], "p2": [cx + d / 2, cy]})
    out.append({"name": "pour 100x50 pads 50 apart near the edge", "w": 100.0, "h": 50.0, "p1": [5.0, 3.0], "p2": [55.0, 3.0]})
    out.append({"name": "pour 100x50 pads in opposite corners", "w": 100.0, "h": 50.0, "p1": [4.0, 4.0], "p2": [96.0, 46.0]})
    out.append({"name": "pour 100x50 pads 10 apart in a corner", "w": 100.0, "h": 50.0, "p1": [4.0, 4.0], "p2": [14.0, 4.0]})
    # narrow strips (a pour used as a power route): the number of squares between the pads decides
    out.append({"name": "pour 100x5 strip pads at the ends", "w": 100.0, "h": 5.0, "p1": [4.0, 2.5], "p2": [96.0, 2.5]})
    out.append({"name": "pour 60x10 strip pads at the ends", "w": 60.0, "h": 10.0, "p1": [4.0, 5.0], "p2": [56.0, 5.0]})
    out.append({"name": "pour 100x20 strip pads at the ends", "w": 100.0, "h": 20.0, "p1": [4.0, 10.0], "p2": [96.0, 10.0]})
    # copper of other nets cut out of the pour (clearance 0.3 mm around 1.6 mm pads)
    wall = [[50.0, 3.8 + 2.54 * k, 0.8] for k in range(17)]  # a row of pads at 2.54 mm: 0.34 mm bridges between the cut-outs
    out.append({"name": "pour 100x50 a wall of pads at 2.54 mm between the contacts", "w": 100.0, "h": 50.0, "p1": [25.0, 25.0], "p2": [75.0, 25.0], "clear": 0.3, "others": wall})
    gap = [q for q in wall if not 19.0 < q[1] < 31.0]
    out.append({"name": "pour 100x50 the same wall with a gap of 12 mm", "w": 100.0, "h": 50.0, "p1": [25.0, 25.0], "p2": [75.0, 25.0], "clear": 0.3, "others": gap})
    out.append({"name": "pour 100x50 a block of 3x3 pads between the contacts", "w": 100.0, "h": 50.0, "p1": [25.0, 25.0], "p2": [75.0, 25.0], "clear": 0.3,
                "others": [[47.46 + 2.54 * i, 22.46 + 2.54 * j, 0.8] for i in range(3) for j in range(3)]})
    out.append({"name": "pour 30x10 strip with five pads cut into it", "w": 30.0, "h": 10.0, "p1": [3.0, 5.0], "p2": [27.0, 5.0], "clear": 0.3,
                "others": [[10.0, 1.0, 0.8], [10.0, 9.0, 0.8], [20.0, 1.0, 0.8], [20.0, 9.0, 0.8], [15.0, 4.0, 0.8]]})
    return out


def solve(c, step=0.125):
    nx, ny = int(round(c["w"] / step)), int(round(c["h"] / step))
    xs = (np.arange(nx) + 0.5) * step
    ys = (np.arange(ny) + 0.5) * step
    X, Y = np.meshgrid(xs, ys, indexing="ij")
    p1 = (X - c["p1"][0]) ** 2 + (Y - c["p1"][1]) ** 2 <= PAD_R**2
    p2 = (X - c["p2"][0]) ** 2 + (Y - c["p2"][1]) ** 2 <= PAD_R**2
    dead = np.zeros((nx, ny), dtype=bool)
    for ox, oy, orad in c.get("others", []):
        dead |= (X - ox) ** 2 + (Y - oy) ** 2 <= (orad + c.get("clear", 0.3)) ** 2
    dead &= ~(p1 | p2)  # the pads' own cells are never cut away
    alive = ~dead
    # the cells that are copper, numbered; a link joins two neighbouring cells that are both copper
    number = -np.ones((nx, ny), dtype=np.int64)
    number[alive] = np.arange(int(alive.sum()))
    n = int(alive.sum())
    g = 1.0 / RS
    rows, cols = [], []
    for a, b in ((number[:-1, :], number[1:, :]), (number[:, :-1], number[:, 1:])):
        ok = (a >= 0) & (b >= 0)
        rows += [a[ok], b[ok]]
        cols += [b[ok], a[ok]]
    r, cc = np.concatenate(rows), np.concatenate(cols)
    A = sp.coo_matrix((np.full(r.size, -g), (r, cc)), shape=(n, n)).tocsr()
    A = A + sp.diags(-np.asarray(A.sum(axis=1)).ravel())
    m1, m2 = p1[alive], p2[alive]
    fixed = m1 | m2
    v = np.zeros(n)
    v[m1] = 1.0
    free = ~fixed
    rhs = -(A[free][:, fixed] @ v[fixed])
    v[free] = sla.spsolve(A[free][:, free].tocsc(), rhs)
    current = (A @ v)[m1].sum()  # current leaving pad 1
    r_fd = 1.0 / abs(current)
    # the staircase disc of the grid is not the disc of radius PAD_R: correct each contact by the resistance of the ring between
    # the equivalent radius (same area) and the nominal one, RS / (2 pi) * ln(r_eq / r)
    corr = 0.0
    for m in (p1, p2):
        r_eq = math.sqrt(m.sum() * step * step / math.pi)
        corr += RS / (2 * math.pi) * math.log(r_eq / PAD_R)
    return r_fd + corr


def main():
    cs = cases()
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "pour_cases.json"), "w", encoding="ascii", newline="\n") as f:
        json.dump(cs, f, indent=1)
        f.write("\n")
    print("case,fd_ohm")
    for c in cs:
        print(f"{c['name']},{solve(c):.6e}")


if __name__ == "__main__":
    main()
