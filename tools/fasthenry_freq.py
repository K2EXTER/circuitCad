"""FastHenry inputs for the frequency dependence of a trace (skin and proximity effect): one file per case, 1 kHz to 100 MHz.

    python tools/fasthenry_freq.py OUTDIR

The cross-section is cut into 7 x 11 filaments, each twice as thin as its neighbour towards the surface (`rh`, `rw`), so that the
skin depth of copper at 100 MHz (6.6 um, against a 35 um trace) is resolved. Cases: one straight trace of several widths, and a loop
of two traces carrying opposite currents (the proximity effect crowds the current to the facing edges).
"""
import os
import sys

T = 0.035
SIGMA = 5.8e4  # S/mm
FREQS = "fmin=1e3 fmax=1e8 ndec=0.5"  # 1 kHz, 31.6 kHz, 1 MHz, 31.6 MHz ... (see FREQS_HZ)


def seg(name, a, b, w):
    return f"E{name} {a} {b} w={w} h={T} nhinc=7 rh=2 nwinc=11 rw=2\n"


def single(l, w):
    s = f"* one trace l={l} w={w}\n.units mm\n.default sigma={SIGMA}\nN1 x=0 y=0 z=0\nN2 x={l} y=0 z=0\n" + seg(1, "N1", "N2", w)
    return s + ".external N1 N2\n.freq fmin=1e3 fmax=1e8 ndec=1\n.end\n"


def loop(l, w, sp):
    s = f"* loop of two traces l={l} w={w} spacing={sp}\n.units mm\n.default sigma={SIGMA}\n"
    s += f"N1 x=0 y=0 z=0\nN2 x={l} y=0 z=0\nN3 x={l} y={sp} z=0\nN4 x=0 y={sp} z=0\n"
    s += seg(1, "N1", "N2", w) + f"E2 N2 N3 w={w} h={T} nhinc=7 rh=2 nwinc=11 rw=2\n" + seg(3, "N3", "N4", w)
    return s + ".external N1 N4\n.freq fmin=1e3 fmax=1e8 ndec=1\n.end\n"


def cases():
    out = []
    for w in (0.25, 1.0, 3.0):
        out.append((f"freq_single_l50_w{w:g}", single(50.0, w), {"kind": "single", "l": 50.0, "w": w}))
    for sp in (2.5, 0.5):
        out.append((f"freq_loop_l50_w0_25_s{sp:g}".replace(".", "_"), loop(50.0, 0.25, sp), {"kind": "loop", "l": 50.0, "w": 0.25, "s": sp}))
    return out


def main(out):
    os.makedirs(out, exist_ok=True)
    for name, text, _ in cases():
        with open(os.path.join(out, name + ".inp"), "w", encoding="ascii", newline="\n") as f:
            f.write(text)
    print(f"{len(cases())} files in {out}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "fasthenry_freq")
