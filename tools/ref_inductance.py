"""Independent reference for partial inductance of rectangular copper bars.

Method (the low-frequency limit of what FastHenry does with volume filaments): the cross-section of every bar is cut into
nw x nt parallel filaments that carry equal current; the partial inductance of the bar is the mean of the filament-pair
mutual inductances, with the exact formula for two parallel, side-by-side filaments of equal length l at distance d:

    M(l, d) = mu0/(2 pi) * l * [ asinh(l/d) - sqrt(1 + (d/l)^2) + d/l ]      (mu0/2pi = 2e-7 H/m)

and, for a filament with itself, the same formula at the geometric mean distance of its own rectangle (0.2235 (a + b)).
Lengths in mm, results in nH.
"""
import math
import sys


def m_par(l, d):
    # 0.2 nH/mm == 2e-7 H/m
    return 0.2 * l * (math.asinh(l / d) - math.sqrt(1 + (d / l) ** 2) + d / l)


def filaments(cx, cy, w, t, nw, nt):
    """Centres of nw x nt filaments in a w x t bar centred at (cx, cy) (x across the width, y across the thickness)."""
    out = []
    for i in range(nw):
        for j in range(nt):
            out.append((cx + (i + 0.5) / nw * w - w / 2, cy + (j + 0.5) / nt * t - t / 2))
    return out


def bar_pair(l, w1, t1, c1, w2, t2, c2, nw=12, nt=4):
    f1, f2 = filaments(c1[0], c1[1], w1, t1, nw, nt), filaments(c2[0], c2[1], w2, t2, nw, nt)
    return sum(m_par(l, math.hypot(a[0] - b[0], a[1] - b[1])) for a in f1 for b in f2) / (len(f1) * len(f2))


def self_bar(l, w, t, nw=12, nt=4):
    f = filaments(0, 0, w, t, nw, nt)
    a, b = w / nw, t / nt
    g = 0.2235 * (a + b)
    total = 0.0
    for p in f:
        for q in f:
            d = math.hypot(p[0] - q[0], p[1] - q[1])
            total += m_par(l, d if d > 1e-12 else g)
    return total / len(f) ** 2


if __name__ == "__main__":
    T = 0.035
    print("case,value_nH")
    for l in (10.0, 30.0, 100.0):
        for w in (0.25, 1.0, 3.0):
            print(f"self l={l:g} w={w:g},{self_bar(l, w, T):.3f}")
    for l in (30.0, 100.0):
        for w in (0.25, 1.0):
            for s in (0.5, 1.0, 2.5, 5.0, 10.0):
                if s <= w:
                    continue
                print(f"mutual l={l:g} w={w:g} s={s:g},{bar_pair(l, w, T, (0, 0), w, T, (s, 0)):.3f}")
    sys.stdout.flush()
