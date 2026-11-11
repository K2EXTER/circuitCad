"""Writes the bundled linear regulators (78xx, 78L05, AMS1117, LM317, MCP1700) and the TL431 with equivalent models built on
their data sheets' typ values (docs/model-policy.md).

    python tools/gen_regulator_models.py

The numbers are typed in from the data sheets named in each entry (read 2026-11-11). A few come from the data sheets' typical
characteristic figures where the table has none (said so in `src` and in the part's notes). crates/cli/tests/regulator_models.rs
measures the models (docs/models/regulators.md); after a change: cargo test -p cli --test regulator_models -- --include-ignored
"""
import json, math, os, sys

EX = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "crates", "part-lib", "examples")
K4T = 4 * 1.380649e-23 * 300.15  # the simulator's default 27 C


def g(x):
    return f"{x:.6g}"


def noise_gain(density):
    return density / math.sqrt(K4T * 1e3)


MC = "onsemi MC7800 シリーズ データシート (MC7800/D Rev. 29) の MC78xxC の表"
AMS = "Advanced Monolithic Systems AMS1117 データシートの ELECTRICAL CHARACTERISTICS"
REGS = {
    # vout: output (LM317: OUT - ADJ); vin0: the data sheet's test input; line: dVout/dVin (V/V); rout: load regulation (ohm);
    # vdo0 + rdo * I: dropout; iq: ground current; ilim: current limit at small and large input - output differential (V, A);
    # rr: ripple rejection (dB) at rr_f; noise: (rms V, f low, f high) or density V/rtHz
    "LM7805_TO-220": dict(name="LM7805", vout=5.0, vin0=10.0, line=0.5e-3 / 12.5, rout=1.3e-3 / 0.995, vdo0=1.6, rdo=0.4, iq=3.2e-3,
                          ilim=[(10.0, 2.2), (35.0, 0.6)], rr=83.0, rr_f=120, noise=(10e-6 * 5.0, 10, 100e3),
                          src=MC + " (MC7805C)", fig="ドロップアウトは、表の 2.0 V (1 A) と図 14 (25 °C、負荷電流ごとの曲線) から 1.6 V + 0.4 Ω × 電流 とした。電流制限は、表の「ピーク電流 2.2 A」と「入力 35 V の短絡電流 0.6 A」を、入出力の差 10 V と 35 V で直線につないだ (図の曲線は直線ではない)"),
    "LM7809_TO-220": dict(name="LM7809", vout=9.0, vin0=15.0, line=6.2e-3 / 15.0, rout=1.5e-3 / 1.495, vdo0=1.6, rdo=0.4, iq=3.4e-3,
                          ilim=[(10.0, 2.2), (35.0, 0.2)], rr=61.0, rr_f=120, noise=(10e-6 * 9.0, 10, 100e3),
                          src=MC + " (MC7809C)", fig="ドロップアウトは、表の 2.0 V (1 A) と図 14 (25 °C、負荷電流ごとの曲線) から 1.6 V + 0.4 Ω × 電流 とした。電流制限は、表の「ピーク電流 2.2 A」と「入力 35 V の短絡電流 0.2 A」を、入出力の差 10 V と 35 V (短絡は出力 0 V) で直線につないだ"),
    "LM7812_TO-220": dict(name="LM7812", vout=12.0, vin0=19.0, line=3.8e-3 / 15.5, rout=8.1e-3 / 1.495, vdo0=1.6, rdo=0.4, iq=3.4e-3,
                          ilim=[(10.0, 2.2), (35.0, 0.2)], rr=60.0, rr_f=120, noise=(10e-6 * 12.0, 10, 100e3),
                          src=MC + " (MC7812C)", fig="ドロップアウトは、表の 2.0 V (1 A) と図 14 (25 °C、負荷電流ごとの曲線) から 1.6 V + 0.4 Ω × 電流 とした。電流制限は、表の「ピーク電流 2.2 A」と「入力 35 V の短絡電流 0.2 A」を、入出力の差 10 V と 35 V (短絡は出力 0 V) で直線につないだ"),
    "L78L05_TO-92": dict(name="L78L05", vout=5.0, vin0=10.0, line=32e-3 / 13.0, rout=8e-3 / 0.039, vdo0=1.7, rdo=0.0, iq=3.53e-3,
                         ilim=[(5.0, 0.13), (40.0, 0.13)], rr=55.0, rr_f=120, noise=(125e-6, 10, 100e3),
                         src="TI UA78L データシート SLVS010X (2023-06 改訂) の 6.7 節 (UA78L05、新しいチップ。ST の L78L05 と同じ 78L05 の仕様)",
                         fig="電流制限は表に無いので、図 6-19 (ICL と温度) の 25 °C の値を読んだ (約 130 mA)"),
    "AMS1117-3.3_SOT-223": dict(name="AMS1117-3.3", vout=3.3, vin0=4.8, line=0.5e-3 / 10.5, rout=3e-3 / 0.8, vdo0=1.1, rdo=0.0, iq=5e-3,
                                ilim=[(1.5, 1.1), (12.0, 1.1)], rr=72.0, rr_f=120, noise=(3.3 * 0.003e-2, 10, 10e3),
                                src=AMS + " (AMS1117-3.3)", fig=""),
    "AMS1117-5.0_SOT-223": dict(name="AMS1117-5.0", vout=5.0, vin0=6.5, line=0.5e-3 / 10.5, rout=5e-3 / 0.8, vdo0=1.1, rdo=0.0, iq=5e-3,
                                ilim=[(1.5, 1.1), (12.0, 1.1)], rr=68.0, rr_f=120, noise=(5.0 * 0.003e-2, 10, 10e3),
                                src=AMS + " (AMS1117-5.0)", fig=""),
    "MCP1700-3302_SOT-23": dict(name="MCP1700-3302", vout=3.3, vin0=4.3, line=0.75e-2 * 3.3, rout=0.01 * 3.3 / 0.2499, vdo0=0.0,
                                rdo=0.178 / 0.25, iq=1.6e-6, ilim=[(1.0, 0.408), (6.0, 0.408)], rr=44.0, rr_f=100, noise=3e-6,
                                src="Microchip MCP1700 データシート DS20001826C の DC CHARACTERISTICS (VIN = 4.3 V、ILOAD = 100 µA)",
                                fig="ライン・レギュレーションは表の typ「±0.75 %/V」をそのまま使った (表の値。代表特性の図は見ていない)"),
    "LM317_TO-220": dict(name="LM317", vout=1.25, vin0=None, line=0.01e-2 * 1.25, rout=0.001 * 1.25 / 1.49, vdo0=1.6, rdo=0.4,
                         iq=0.0, iadj=50e-6, vrange=(1.2, 1.3), ilim=[(15.0, 2.2), (40.0, 0.4)], rr=65.0, rr_f=120, rr_vout=10.0, noise=(1.25 * 0.003e-2, 10, 10e3),
                         src="TI LM317 データシート SLVS044Z (2025-04 改訂) の 6.6 節 (新しいチップの値がある行はそれ)",
                         fig="ドロップアウトは表に無いので、図 6-18 (25 °C) を読んで 1.6 V + 0.4 Ω × 電流 とした (20 mA で約 1.6 V、1 A で約 2.0 V)。電流制限は表の 2.2 A (差 15 V 以下) と 0.4 A (差 40 V) を直線につないだ"),
}


def regulator(f, p):
    n = p["name"]
    adj = n == "LM317"
    ref = "adj" if adj else "gnd"
    head = f".subckt {n} in adj out" if adj else f".subckt {n} in gnd out"
    vin0 = p["vin0"] if p["vin0"] is not None else 6.25
    k_rr = 10 ** (-p["rr"] / 20)
    if adj:
        # the LM317's ripple rejection is given at the output set to rr_vout: on the reference it is that much smaller
        k_rr *= p["vout"] / p["rr_vout"]
    # at DC the input reaches the output by the line regulation; above ~1 Hz only by the (flat) ripple rejection
    k_dc = p["line"] - k_rr
    if isinstance(p["noise"], tuple):
        rms, fl, fh = p["noise"]
        density = rms / math.sqrt(fh - fl)
    else:
        density = p["noise"]
    ro = p["rout"]
    (d1, i1), (d2, i2) = p["ilim"]
    slope = (i2 - i1) / (d2 - d1) if d2 != d1 else 0.0
    lines = [
        head,
        "* line regulation through a slow path (the data sheet's DC figure) and the ripple from the input (flat, the ripple rejection)",
        f"Elf lfa {ref} in {ref} 1",
        "Rlf lfa lf 1.59Meg",
        f"Clf lf {ref} 100n",
        "* output noise (taken as flat over the data sheet's band): a 1 k resistor's noise, scaled",
        f"Rnz nz {ref} 1k",
        f"Btgt t0 {ref} V={g(p['vout'])}+{g(k_dc)}*(V(lf,{ref})-{g(vin0)})+{g(k_rr)}*(V(in,{ref})-{g(vin0)})" + ("+V(vt," + ref + ")" if "vrange" in p else ""),
        f"En tgt t0 nz {ref} {g(noise_gain(density))}",
    ]
    if "vrange" in p:
        # the data sheet's min and max of the output (part to part), for the variation analyses; 0 V otherwise
        lo, hi = p["vrange"]
        what = "vref" if adj else "vout"
        lines[-2:-2] = [
            "* the output's difference from part to part (0 V; the variation analyses move it between the data sheet's min and max)",
            f"Vtol vt {ref} DC 0",
            f"* spread Vtol {g(lo - p['vout'])} {g(hi - p['vout'])} {what} (data sheet: {g(lo)} to {g(hi)} V)",
        ]
    # the ceiling the input leaves: in - dropout (the dropout grows with the output current; the output resistance already
    # drops ro * I, the dropout's own part is the rest)
    rdo = max(p["rdo"] - ro, 0.0)
    lines += ["* the ceiling the input leaves (the dropout)", f"Bdo ind {ref} V=V(in,{ref})-{g(p['vdo0'])}-{g(rdo)}*I(Vs)"]

    def clamp(name, x, ceiling, out):
        # out = min(x, ceiling), softly, the way the earlier bundled models clamped (a high gain into a diode): it converges
        # where smooth formulas did not (the bundled mixed-signal sample's 555)
        return [
            f"Ex{name} xh{name} {ref} {x} {ceiling} 1000",
            f"D{name} xh{name} yh{name} DREG",
            f"Ry{name} yh{name} {ref} 1k",
            f"Ey{name} y{name} {ref} yh{name} {ref} 0.001",
            f"Ec{name} {out} {ref} {x} y{name} 1",
        ]

    lines += ["* the target, no higher than the dropout's ceiling"]
    lines += clamp("1", "tgt", "ind", "c1")
    # the current limit: the output no higher than the output voltage plus ro times the limit (the limit is smaller at a large
    # input - output difference). Compared as currents (amps as volts): the clamp's own offset (its diode's drop / 1000,
    # 0.65 mV) would be 0.5 A through an output resistance of a milliohm, but is 0.65 mA as a current
    ilim = f"limit({g(i1)}+{g(slope)}*(V(in,{ref})-V(out,{ref})-{g(d1)}),{g(min(i1, i2))},{g(max(i1, i2))})"
    lines += [
        "* the current limit: the current the target asks for (amps as volts), no more than the limit",
        f"Bu u {ref} V=(V(c1,{ref})-V(out,{ref}))/{g(ro)}",
        f"Blim cl {ref} V={ilim}",
    ]
    lines += clamp("2", "u", "cl", "uc")
    lines += [f"Bc2 c2 {ref} V=V(out,{ref})+{g(ro)}*V(uc,{ref})"]
    src = "c2"
    lines += [
        "* output through the load regulation's resistance; the output current comes from the input (both ways, as the earlier",
        "* bundled models did: a dead or reversed input then leaves the output near 0 V)",
        f"Eo o {ref} {src} {ref} 1",
        "Vs o os DC 0",
        f"Ro os out {g(ro)}",
        f"Bin in {ref} I=I(Vs)",
    ]
    if adj:
        lines += ["* adjustment pin current (out of ADJ)", f"Iadj in adj DC {g(p['iadj'])}"]
    else:
        # flat above a few volts of input, like a resistor near 0 V: a constant current would pull on an unpowered input, and a
        # sharp switch-on (step) left the bundled mixed-signal sample without a DC solution at -40 C
        lines += ["* ground (quiescent) current: flat above a few volts, like a resistor near 0 V", f"Bq in gnd I={g(p['iq'])}*tanh(V(in,gnd)/2)"]
    lines += [".model DREG D(IS=1e-12 N=1)", f".ends {n}"]
    return "\n".join(lines), density


def regulator_limits(p, density):
    n = p["name"]
    adj = n == "LM317"
    out = f"基準 {p['vout']} V (OUT − ADJ)、ADJ の電流 50 µA" if adj else f"出力 {p['vout']} V"
    at10 = "、出力 10 V の設定で" if adj else ""
    rows = [
        f"データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {p['src']}。データシートとの比べは docs/models/regulators.md。",
        f"再現しているもの: {out}、ライン・レギュレーション、ロード・レギュレーション、ドロップアウト、"
        + ("" if adj else f"グラウンド電流 {g(p['iq'] * 1e3)} mA (入力が数 V 以上で)、")
        + f"電流制限 (入出力の差で変わる)、リップル除去 {g(p['rr'])} dB ({g(p['rr_f'])} Hz{at10}。どの周波数でも同じとした)、出力雑音 {g(density * 1e9)} nV/√Hz (平ら)。",
    ]
    if p["fig"]:
        rows.append(f"データシートの表に無い値: {p['fig']}。")
    if "vrange" in p:
        lo, hi = p["vrange"]
        rows.append(f"{'基準電圧' if adj else '出力電圧'}の部品ごとの違い ({g(lo)}〜{g(hi)} V、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。")
    rows += [
        "扱わないもの: " + ("" if "vrange" in p else "出力電圧の誤差 (出力はちょうど公称値)、") + "温度による変化と熱による停止、負荷や入力が急に変わったときの応答、出力の容量による安定性、出力に電流を押し込まれたとき (実物は吸い込めないが、このモデルは出力抵抗を通して吸い込む)"
        + ("、最小負荷電流 (3.5 mA)" if adj else "")
        + "。",
        "最終の確認は、メーカーの公式モデルで行ってください。",
    ]
    return rows


# ---------------------------------------------------------------- TL431
# TI TL431/TL432 data sheet SLVS543S (revised May 2024), 6.5 Electrical Characteristics, TL431C.
TL = dict(vref=2.495, vref_min=2.44, vref_max=2.55, iref=2e-6, dvref_dvka=-1.4e-3, zka=0.2, ioff=0.1e-6, imin=0.4e-3)


def tl431():
    p = TL
    gk = 1.0 / p["zka"]
    w = 0.3e-3
    # the knee: below it the cathode carries almost nothing; above it gk A/V. At IKA = 10 mA, REF is the data sheet's Vref.
    th = p["vref"] - 10e-3 / gk
    x = f"((V(ref,an)-V(vt,an)-{g(th)}-{g(p['dvref_dvka'])}*(V(cat,an)-{g(p['vref'])}))/{g(w)})"
    return "\n".join([
        ".subckt TL431 cat ref an",
        "* reference input current (typ, at 2.495 V)",
        f"Rref ref an {g(p['vref'] / p['iref'])}",
        "* the cathode current rises with REF above the knee (slope 1 / dynamic impedance; a smooth knee 0.3 mV wide); REF moves with",
        "* the cathode voltage by the data sheet's ratio; the cathode needs about 1.2 V",
        f"Bk cat an I={g(gk * w)}*(0.5*({x}+abs({x}))+ln(1+exp(-abs({x}))))*step(V(cat,an)-1.2,0.2)",
        "* off-state cathode current (typ, at 36 V)",
        f"Rleak cat an {g(36 / p['ioff'])}",
        "* the reference's difference from part to part (0 V; the variation analyses move it between the data sheet's min and max)",
        "Vtol vt an DC 0",
        f"* spread Vtol {g(p['vref_min'] - p['vref'])} {g(p['vref_max'] - p['vref'])} vref (data sheet: {g(p['vref_min'])} to {g(p['vref_max'])} V)",
        ".ends TL431",
    ])


TL_LIMITS = [
    "データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: TI TL431/TL432 データシート SLVS543S (2024-05 改訂) の 6.5 節 (TL431C)。データシートとの比べは docs/models/regulators.md。",
    "再現しているもの: 基準電圧 2.495 V (IKA = 10 mA)、REF の入力電流 2 µA、ΔVref/ΔVKA −1.4 mV/V、動的インピーダンス 0.2 Ω、停止中のカソード電流 0.1 µA。",
    "基準電圧の部品ごとの違い (2.440〜2.550 V、データシートの min と max) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析は typ)。",
    "扱わないもの: 温度による変化、最小カソード電流 (0.4 mA) より下で安定しなくなること (このモデルは小さな電流でも安定します)、周波数特性と容量負荷での発振、耐圧・最大電流を超えたときの破壊。",
    "最終の確認は、メーカーの公式モデルで行ってください。",
]


def write(f, text, limits, params=None):
    path = os.path.join(EX, f + ".part.json")
    j = json.load(open(path, encoding="utf-8"))
    j["sim"]["text"] = text
    j["sim"]["limits"] = limits
    if params is not None and "params" in j["sim"]:
        j["sim"]["params"] = params
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(j, ensure_ascii=False, indent=2) + "\n")
    print("wrote", f)


for f, p in REGS.items():
    text, density = regulator(f, p)
    old = json.load(open(os.path.join(EX, f + ".part.json"), encoding="utf-8"))["sim"].get("params")
    params = None
    if old is not None:
        params = dict(old)
        if "vout" in params:
            params["vout"] = p["vout"]
        if "vref" in params:
            params["vref"] = p["vout"]
        if "dropout" in params:
            params["dropout"] = p["vdo0"] + p["rdo"] * 1.0
    write(f, text, regulator_limits(p, density), params)
write("TL431_SOT-23", tl431(), TL_LIMITS)
