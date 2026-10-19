"""Writes FastHenry input files for the cases of ref_inductance.py / the pcb-core inductance test.

    python tools/fasthenry_cases.py OUTDIR

One file per case (units mm, copper 5.8e4 S/mm, 35 um thick, 12 x 4 filaments across the section, one frequency of 1 kHz
where the current is uniform in the section): `self_l<L>_w<W>.inp` has one bar, `mutual_l<L>_w<W>_s<S>.inp` has two parallel
bars `S` apart (centre to centre), both drawn from x = 0 to x = L, each with its own port. FastHenry2 writes the matrix
Z = R + j w L of the ports to Zc.mat next to the input file; the inductance is Im(Z) / w, and the mutual inductance is
Im(Z12) / w. Compare with the output of ref_inductance.py (nH).
"""
import os
import sys

T = 0.035
SIGMA = 5.8e4  # S/mm
FREQ = 1e3


def header(title):
    return f"* {title}\n.units mm\n.default sigma={SIGMA}\n"


def bar(name, n1, n2, w):
    return f"E{name} N{n1} N{n2} w={w} h={T} nwinc=12 nhinc=4\n"


def self_case(l, w):
    s = header(f"self inductance of one bar, l={l} w={w}")
    s += "N1 x=0 y=0 z=0\n" f"N2 x={l} y=0 z=0\n"
    s += bar(1, 1, 2, w)
    s += ".external N1 N2\n" f".freq fmin={FREQ} fmax={FREQ} ndec=1\n.end\n"
    return s


def mutual_case(l, w, sp):
    s = header(f"two parallel bars, l={l} w={w} spacing={sp}")
    s += "N1 x=0 y=0 z=0\n" f"N2 x={l} y=0 z=0\n" "N3 x=0 y=" f"{sp} z=0\n" f"N4 x={l} y={sp} z=0\n"
    s += bar(1, 1, 2, w) + bar(2, 3, 4, w)
    s += ".external N1 N2\n.external N3 N4\n" f".freq fmin={FREQ} fmax={FREQ} ndec=1\n.end\n"
    return s


def main(out):
    os.makedirs(out, exist_ok=True)
    files = []
    for l in (10.0, 30.0, 100.0):
        for w in (0.25, 1.0, 3.0):
            files.append((f"self_l{l:g}_w{w:g}.inp", self_case(l, w)))
    for l in (30.0, 100.0):
        for w in (0.25, 1.0):
            for sp in (0.5, 1.0, 2.5, 5.0, 10.0):
                if sp > w:
                    files.append((f"mutual_l{l:g}_w{w:g}_s{sp:g}.inp", mutual_case(l, w, sp)))
    for name, text in files:
        with open(os.path.join(out, name), "w", encoding="ascii", newline="\n") as f:
            f.write(text)
    print(f"{len(files)} files in {out}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "fasthenry_cases")
