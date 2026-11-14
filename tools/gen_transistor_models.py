"""Writes the bundled transistor models (M9) from their data sheets: 2N3904 and 2N3906 (NPN, PNP: Gummel-Poon) and the MOSFETs 2N7000,
AO3400A and AO3401A (P channel) (level 1 with the series resistance RD).

    python tools/gen_transistor_models.py        (needs scipy for the fits)

Every number below was read on 2026-11-12 (AO3401A and 2N3906 on 2026-11-14) from the data sheet named next to it, from the tables and from the graphs rendered
as pictures (graph readings: the pixel position of the curve, good to about 1 %). The fits use the simulator's own equations
(crates/sim-core/src/dc/devices.rs: bjt_eval, mos_eval). docs/model-policy.md says how bundled models are made;
crates/cli/tests/integration/transistor_models.rs measures the models and writes docs/models/transistors.md and the on-screen tables.
After a change: python tools/collect_legacy_models.py, cargo test -p cli --test integration transistor_models -- --include-ignored,
cargo test -p app refresh_sample_models -- --ignored, then the samples' tests (docs/model-policy.md).
"""
import json, math, os, sys

from scipy.optimize import fsolve, least_squares, minimize

EX = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "crates", "part-lib", "examples")
VT = 0.025865

# ------------------------------------------------------------------ bipolar transistors (Gummel-Poon), 25 C
# Per part: hfe_norm (the normalised DC current gain curve, VCE = 1 V) scaled by hfe_peak; vbe_on (VBE at VCE = 1 V),
# vbe_sat and vce_sat (IC/IB = 10) from the "ON" voltages graph; cibo / cobo (pF at the reverse voltage) from the capacitance
# graph; ft_min (no typ is given: the model is as slow as the min allows, TF = 1 / (2 pi fT)); hoe (min, max at IC = 1 mA,
# VCE = 10 V: VAF = IC / hoe - VCE with the geometric mean). A PNP is fitted with magnitudes (the equations are the same).
BJT = {
    "2N3904_TO-92": dict(
        name="2N3904",
        src="onsemi 2N3903/2N3904 データシート (2N3903/D Rev. 9, 2021-08)",
        url="https://www.onsemi.com/pdf/datasheet/2n3903-d.pdf",
        desc="NPN 汎用トランジスタ (TO-92, ピン1=E 2=B 3=C)。データシート (onsemi) の表と代表特性から作った等価モデル",
        figs=("図 15", "図 17", "図 3"),
        # Figure 15. The data sheet has no typical hFE: the curve's peak is placed at 173, the geometric mean of the table's
        # min 100 and max 300 at 10 mA (the table's min at 0.1 / 1 / 50 / 100 mA stay below it)
        hfe_peak=173.0,
        hfe_norm=[(0.1e-3, 0.443), (0.2e-3, 0.538), (0.5e-3, 0.676), (1e-3, 0.773), (2e-3, 0.868), (5e-3, 0.979), (10e-3, 0.988), (20e-3, 0.859), (50e-3, 0.493), (100e-3, 0.27)],
        # Figure 17. Its printed axis labels sit off the grid lines; the grid lines are a plain log axis from 1 to 200 mA and
        # were used. Below 5 mA the drawn VBE curve is flatter than any junction can be, so it is not used.
        vbe_on=[(5e-3, 0.684), (10e-3, 0.706), (20e-3, 0.738), (50e-3, 0.79), (100e-3, 0.869)],
        vbe_sat=[(10e-3, 0.745), (20e-3, 0.787), (50e-3, 0.848), (100e-3, 0.927)],
        vce_sat=[(20e-3, 0.117), (50e-3, 0.194), (100e-3, 0.36)],
        # Figure 3: Cibo (emitter-base, collector open), Cobo (collector-base, emitter open)
        cibo=[(0.1, 4.36e-12), (0.2, 4.24e-12), (0.5, 3.76e-12), (1.0, 3.30e-12), (2.0, 2.89e-12), (3.0, 2.67e-12), (5.0, 2.41e-12)],
        cobo=[(0.1, 3.55e-12), (0.2, 3.46e-12), (0.5, 3.18e-12), (1.0, 2.78e-12), (2.0, 2.39e-12), (5.0, 1.96e-12), (10.0, 1.69e-12), (20.0, 1.48e-12), (30.0, 1.39e-12)],
        ft_min=300e6,
        hoe=(1e-6, 40e-6),
    ),
    "2N3906_TO-92": dict(
        name="2N3906",
        pnp=True,
        src="onsemi 2N3906 データシート (2N3906/D Rev. 4, 2010-02)",
        url="https://www.onsemi.com/pdf/datasheet/2n3906-d.pdf",
        desc="PNP 汎用トランジスタ (TO-92, ピン1=E 2=B 3=C)。データシート (onsemi) の表と代表特性から作った等価モデル",
        figs=("図 13", "図 15", "図 3"),
        # Figure 13 (read 2026-11-14). As for the 2N3904, no typical hFE: the peak (7 to 10 mA) is placed at 173, the geometric
        # mean of the table's min 100 and max 300 at 10 mA (the table's min 60 / 80 / 60 / 30 at 0.1 / 1 / 50 / 100 mA stay below)
        hfe_peak=173.0,
        hfe_norm=[(0.1e-3, 0.774), (0.2e-3, 0.806), (0.5e-3, 0.854), (1e-3, 0.883), (2e-3, 0.921), (5e-3, 0.971), (10e-3, 0.988), (20e-3, 0.936), (50e-3, 0.511), (100e-3, 0.22)],
        # Figure 15. Its printed axis labels (1, 2, 5, 10, 20, 50, 100, 200 mA) sit on grid lines, but the grid lines between
        # them are not a plain log axis: the current is read on a log scale between the two nearest labels.
        vbe_on=[(5e-3, 0.71), (10e-3, 0.727), (20e-3, 0.756), (50e-3, 0.81), (100e-3, 0.868)],
        vbe_sat=[(10e-3, 0.757), (20e-3, 0.796), (50e-3, 0.85), (100e-3, 0.903)],
        vce_sat=[(20e-3, 0.1), (50e-3, 0.136), (100e-3, 0.245)],
        # Figure 3: the two curves meet above about 4 V; the one that goes on to 40 V is Cobo (VEBO is 5 V)
        cibo=[(0.1, 6.05e-12), (0.2, 5.53e-12), (0.5, 4.72e-12), (1.0, 4.24e-12), (2.0, 3.78e-12), (3.0, 3.59e-12)],
        cobo=[(0.1, 6.48e-12), (0.2, 6.04e-12), (0.5, 5.21e-12), (1.0, 4.57e-12), (2.0, 3.96e-12), (3.0, 3.59e-12), (5.0, 3.19e-12), (10.0, 2.71e-12), (20.0, 2.48e-12), (30.0, 2.45e-12)],
        ft_min=250e6,
        hoe=(3e-6, 60e-6),
    ),
}


def vaf_of(b):
    return round(1e-3 / math.sqrt(b["hoe"][0] * b["hoe"][1]) - 10.0, -1)

def bjt_ev(p, vbe, vbc):
    ef, er = math.exp(min(vbe / VT, 200)), math.exp(min(vbc / VT, 200))
    iff, ir = p["IS"] * (ef - 1), p["IS"] * (er - 1)
    q2 = iff / p["IKF"]
    h = 2 / (1 + math.sqrt(1 + 4 * q2)) if q2 > 0 else 1
    ile = p["ISE"] * (math.exp(min(vbe / (p["NE"] * VT), 200)) - 1)
    k = 1 - vbc / p["VAF"]
    return (iff - ir) * k * h - ir / p["BR"], iff / p["BF"] + ile + ir / p["BR"]


def bjt_on(p, ic, vce=1.0):
    from scipy.optimize import brentq
    v = brentq(lambda v: bjt_ev(p, v, v - vce)[0] - ic, 0.2, 1.3)
    ib = bjt_ev(p, v, v - vce)[1]
    return v + ib * p["RB"] + (ic + ib) * p["RE"], ic / ib


def bjt_sat(p, ic):
    ib = ic / 10
    f = lambda x: [(bjt_ev(p, x[0], x[1])[0] - ic) / ic, (bjt_ev(p, x[0], x[1])[1] - ib) / ib]
    best = min((fsolve(f, g, full_output=True)[0] for g in ((0.75, 0.65), (0.8, 0.7), (0.7, 0.6), (0.85, 0.8))), key=lambda x: sum(abs(t) for t in f(x)))
    if sum(abs(t) for t in f(best)) > 1e-6:
        raise ValueError("no saturation point")
    vbe, vbc = best
    ie = ic + ib
    return vbe + ib * p["RB"] + ie * p["RE"], (vbe - vbc) + ic * p["RC"] + ie * p["RE"]


BJT_NAMES = ["IS", "BF", "ISE", "NE", "IKF", "RB", "RE", "BR", "RC"]


def bjt_unpack(x, b):
    p = {k: (x[i] if k == "NE" else 10 ** x[i]) for i, k in enumerate(BJT_NAMES)}
    p["VAF"] = vaf_of(b)
    return p


def bjt_cost(x, b):
    p = bjt_unpack(x, b)
    # physical ranges: the leakage's emission coefficient and the reverse gain
    if not (1.2 <= p["NE"] <= 3.0 and 0.5 <= p["BR"] <= 50.0):
        return 1e6
    try:
        e = sum(4 * math.log(bjt_on(p, ic)[1] / (b["hfe_peak"] * n)) ** 2 for ic, n in b["hfe_norm"])
        e += sum(((bjt_on(p, ic)[0] - v) / 0.01) ** 2 for ic, v in b["vbe_on"])
        e += sum(((bjt_sat(p, ic)[0] - v) / 0.015) ** 2 for ic, v in b["vbe_sat"])
        e += sum(((bjt_sat(p, ic)[1] - v) / 0.02) ** 2 for ic, v in b["vce_sat"])
        return e
    except Exception:
        return 1e6


def fit_cap(points):
    """CJ0, VJ, M of C(V) = CJ0 / (1 + V/VJ)^M (reverse voltage V): a grid over VJ and M, CJ0 in log."""
    best = None
    for vj10 in range(1, 13):
        vj = vj10 / 10
        for m100 in range(10, 71):
            mm = m100 / 100
            logs = [math.log(c) + mm * math.log(1 + v / vj) for v, c in points]
            lc0 = sum(logs) / len(logs)
            err = sum((x - lc0) ** 2 for x in logs)
            if best is None or err < best[0]:
                best = (err, math.exp(lc0), vj, mm)
    return best[1:]


def g(x):
    return f"{x:.4g}"


def bjt_model(b):
    # a few starting points; the best fit is kept (Nelder-Mead finds local minima)
    starts = [
        [math.log10(2e-14), math.log10(200), math.log10(1e-13), 1.6, math.log10(0.05), 1.0, math.log10(0.3), math.log10(5), 0.0],
        [math.log10(3e-14), math.log10(300), math.log10(5e-12), 1.8, math.log10(0.02), 0.8, math.log10(0.5), math.log10(30), 0.4],
        [math.log10(1e-14), math.log10(250), math.log10(1e-12), 2.0, math.log10(0.03), 0.5, math.log10(0.2), math.log10(10), 0.2],
    ]
    best = None
    for x0 in starts:
        r = minimize(bjt_cost, x0, args=(b,), method="Nelder-Mead", options={"maxiter": 20000, "maxfev": 20000, "xatol": 1e-7, "fatol": 1e-10})
        r = minimize(bjt_cost, r.x, args=(b,), method="Nelder-Mead", options={"maxiter": 20000, "maxfev": 20000, "xatol": 1e-8, "fatol": 1e-11})
        if best is None or r.fun < best.fun:
            best = r
    print(b["name"], "fit cost", round(best.fun, 3))
    p = bjt_unpack(best.x, b)
    vaf = p["VAF"]
    cje, vje, mje = fit_cap(b["cibo"])
    cjc, vjc, mjc = fit_cap(b["cobo"])
    tf = 1 / (2 * math.pi * b["ft_min"])
    kind = "PNP" if b.get("pnp") else "NPN"
    fh, fon, fcap = b["figs"]
    hoe = f"{b['hoe'][0] * 1e6:g}〜{b['hoe'][1] * 1e6:g} µmho"
    text = (f".model {b['name']} {kind}(IS={g(p['IS'])} BF={g(p['BF'])} NF=1 VAF={g(vaf)} IKF={g(p['IKF'])} ISE={g(p['ISE'])} NE={p['NE']:.3f} "
            f"BR={g(p['BR'])} RB={g(p['RB'])} RE={g(p['RE'])} RC={g(p['RC'])} CJE={g(cje)} VJE={vje:g} MJE={mje:g} CJC={g(cjc)} VJC={vjc:g} MJC={mjc:g} TF={g(tf)})")
    limits = [
        f"データシートの表と代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {b['src']}。データシートとの比べは docs/models/transistors.md。",
        f"直流: {fh} (hFE の電流による変化。相対値)・{fon} (VBE、VBE(sat)、VCE(sat)) に、IS・BF・ISE・NE・IKF・BR・RB・RE・RC を当てはめました。データシートに hFE の typ が無いので、山の高さは表の min 100 と max 300 (10 mA) の幾何平均 173 に置きました。実物の hFE は、同じ型番でも 100〜300 と大きく違います。",
        f"容量: {fcap} の Cibo・Cobo に CJE・CJC を当てはめました。速さ: fT の min {b['ft_min'] / 1e6:g} MHz から TF を決めました (実物は、ふつうこれより速い)。Early 電圧 VAF {g(vaf)} V は、hoe ({hoe}、1 mA) の中ほどから。",
        "扱わないもの: 蓄積時間 (逆方向の蓄積 TR は入れていない。飽和から抜ける速さは実物と違います)、雑音の 1/f 成分、温度によるパラメータの変化はシミュレータの標準の式、耐圧・定格電流・許容損失を超えても壊れません。",
        "最終の確認は、メーカーの公式モデルで行ってください (公開されていれば「部品の取り込み」で読めます)。",
    ]
    return text, limits, p


# ------------------------------------------------------------------ MOSFETs (level 1 + RD)
MOS = {
    "2N7000_TO-92": dict(
        name="2N7000",
        src="onsemi 2N7000/2N7002/NDS7002A データシート (NDS7002A/D Rev. 11, 2025-03)",
        url="https://www.onsemi.com/pdf/datasheet/nds7002a-d.pdf",
        # VGS(th) typ 2.1 V at ID = 1 mA (VDS = VGS); RDS(on) typ 1.2 ohm (10 V, 500 mA) and 1.8 ohm (4.5 V, 75 mA)
        vth=(2.1, 1e-3),
        rds=[(10.0, 0.5, 1.2), (4.5, 0.075, 1.8)],
        # Ciss 20, Coss 11, Crss 4 pF typ (VDS = 25 V, VGS = 0)
        caps=(20e-12, 11e-12, 4e-12),
        gfs=(0.32, 10.0, 0.2),
        desc="Nチャネル MOSFET 60V 200mA (TO-92)。データシート (onsemi) の typ から作った等価モデル (レベル1 + 直列抵抗)",
    ),
    "AO3400A_SOT-23": dict(
        name="AO3400A",
        src="Alpha & Omega AO3400A データシート (Rev. 3.1, 2023-07)",
        url="https://www.aosmd.com/res/datasheets/AO3400A.pdf",
        # VGS(th) typ 1.05 V at 250 uA; RDS(on) typ 18 mohm (10 V, 5.7 A), 19 (4.5 V, 5 A), 24 (2.5 V, 3 A)
        vth=(1.05, 250e-6),
        rds=[(10.0, 5.7, 0.018), (4.5, 5.0, 0.019), (2.5, 3.0, 0.024)],
        # Ciss 630, Coss 75, Crss 50 pF typ (VDS = 15 V, VGS = 0)
        caps=(630e-12, 75e-12, 50e-12),
        gfs=(33.0, 5.0, 5.7),
        desc="Nチャネル MOSFET 30V (SOT-23)。ロジックレベル駆動。データシート (Alpha & Omega) の typ から作った等価モデル (レベル1 + 直列抵抗)",
    ),
    # P channel: the numbers are the magnitudes (the data sheet writes VGS, ID and VDS negative); the model is a PMOS with VTO < 0
    "AO3401A_SOT-23": dict(
        name="AO3401A",
        pch=True,
        src="Alpha & Omega AO3401A データシート (Rev. 3.1, 2023-12)",
        url="https://www.aosmd.com/res/datasheets/AO3401A.pdf",
        # VGS(th) typ -0.9 V at ID = -250 uA; RDS(on) typ 41 mohm (-10 V, -4.0 A), 47 (-4.5 V, -3.5 A), 60 (-2.5 V, -2.5 A)
        vth=(0.9, 250e-6),
        rds=[(10.0, 4.0, 0.041), (4.5, 3.5, 0.047), (2.5, 2.5, 0.060)],
        # Ciss 645, Coss 80, Crss 55 pF typ (VDS = -15 V, VGS = 0)
        caps=(645e-12, 80e-12, 55e-12),
        gfs=(17.0, 5.0, 4.0),
        desc="Pチャネル MOSFET -30V (SOT-23)。ハイサイドスイッチ用。データシート (Alpha & Omega) の typ から作った等価モデル (レベル1 + 直列抵抗)",
    ),
}


def mos_id(kp, vto, vgs, vds):
    vov = vgs - vto
    if vov <= 0:
        return 0.0
    return kp * (vov * vds - 0.5 * vds * vds) if vds < vov else 0.5 * kp * vov * vov


def mos_rds(kp, vto, rd, vgs, i):
    """VDS / ID with ID forced through the drain (RD in series)."""
    from scipy.optimize import brentq
    vov = vgs - vto
    if vov <= 0 or 0.5 * kp * vov * vov <= i:
        return 1e6  # the channel cannot carry i at this gate voltage
    v = brentq(lambda v: mos_id(kp, vto, vgs, v) - i, 1e-12, vov)
    return v / i + rd


def mos_model(p):
    vth, ith = p["vth"]

    def res(x):
        kp, rd = 10 ** x[0], 10 ** x[1]
        vto = vth - math.sqrt(2 * ith / kp)  # the threshold point is in saturation (VDS = VGS)
        return [math.log(mos_rds(kp, vto, rd, vgs, i) / r) for vgs, i, r in p["rds"]]

    # start where the channel alone could carry three times the largest current at the lowest gate voltage
    vmin, imax = min(v for v, _, _ in p["rds"]), max(i for _, i, _ in p["rds"])
    kp0 = 6 * imax / (vmin - vth) ** 2
    r = least_squares(res, [math.log10(kp0), math.log10(min(r for _, _, r in p["rds"]) / 3)])
    kp, rd = 10 ** r.x[0], 10 ** r.x[1]
    vto = vth - math.sqrt(2 * ith / kp)
    ciss, coss, crss = p["caps"]
    kind, sign = ("PMOS", -1) if p.get("pch") else ("NMOS", 1)
    text = f".model {p['name']} {kind}(VTO={g(sign * vto)} KP={g(kp)} RD={g(rd)} CGS={g(ciss - crss)} CGD={g(crss)})"
    limits = [
        f"データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {p['src']}。データシートとの比べは docs/models/transistors.md。",
        f"レベル1 (2乗則) に直列抵抗 RD を足し、しきい値 VGS(th) と、ゲート電圧ごとのオン抵抗 RDS(on) ({len(p['rds'])} 点) に合わせました。ゲートの容量は Ciss・Crss の typ (CGS = Ciss − Crss、CGD = Crss) の一定の値です。",
        "扱わないもの: ボディダイオード (ソースからドレインへの逆向きのダイオード)、出力容量 Coss のうちドレイン・ソース間の分、容量の電圧による変化、チャネル長変調 (LAMBDA。飽和領域の電流は VDS によらない)、温度によるオン抵抗の増加、耐圧・定格電流を超えたときの破壊。大電流の飽和領域では、2乗則なので実物より電流が大きく出ます。",
        "最終の確認は、メーカーの公式モデルで行ってください (公開されていれば「部品の取り込み」で読めます)。",
    ]
    return text, limits


def write(f, text, limits, src, url=None, desc=None):
    path = os.path.join(EX, f + ".part.json")
    j = json.load(open(path, encoding="utf-8"))
    j["sim"]["text"] = text
    j["sim"]["limits"] = limits
    j["sim"]["origin"] = {"kind": "equivalent", "source": src}
    if desc:
        j["description"] = desc
    if url:
        j["datasheet"] = url
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(j, ensure_ascii=False, indent=2) + "\n")
    print("wrote", f, text)


for f, b in BJT.items():
    text, limits, _ = bjt_model(b)
    write(f, text, limits, b["src"], b["url"], b["desc"])
for f, p in MOS.items():
    text, limits = mos_model(p)
    write(f, text, limits, p["src"], p["url"], p["desc"])
