"""Reference loop inductance of a trace over a copper plane, from the textbook microstrip formula (no solver needed).

    python tools/ref_microstrip.py > tools/plane_reference.csv

For each `loop` case of inductance_cases.json (a trace over a plane that carries the return current): the characteristic impedance
of the line in air (Hammerstad: Z0 = 60 ln(8 h / w + w / 4 h) for w <= h, 120 pi / (w / h + 1.393 + 0.667 ln(w / h + 1.444)) above)
gives the inductance per length L' = Z0 / c, since inductance does not depend on the dielectric; times the trace length. It is
the figure of an infinitely long line, so a short trace differs by its end effects (a few percent).
"""
import json
import math
import os

C = 299.792458  # mm / ns


def z0_air(w, h):
    u = w / h
    if u <= 1:
        return 60 * math.log(8 / u + u / 4)
    return 120 * math.pi / (u + 1.393 + 0.667 * math.log(u + 1.444))


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    cases = json.load(open(os.path.join(here, "inductance_cases.json"), encoding="ascii"))
    print("case,hammerstad_nH")
    for c in cases:
        if c["kind"] != "loop":
            continue
        layers = c.get("layers", 2)
        h = 1.6 * abs(c["plane"]["layer"] - 0) / (layers - 1)
        path = c["paths"][0]
        length = sum(math.dist(path[i][:2], path[i + 1][:2]) for i in range(len(path) - 1))
        print(f"{c['name']},{z0_air(c['w'], h) / C * length:.3f}")


if __name__ == "__main__":
    main()
