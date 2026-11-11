"""Writes the bundled ADR4525 (2.5 V reference) and LP5907-4.5 (low-noise LDO) parts with equivalent models built on their data
sheets' typ values (docs/model-policy.md).

    python tools/gen_reference_models.py

The parts' pins and packages are the ones made for the bench DMM (bench/, 2026-11-10); this script only writes the model, the
model notes and the description. The numbers below are typed in from the data sheets named in DS (read 2026-11-11). Where the
data sheet gives no typ, the max is used and the note says so. crates/cli/tests/regulator_models.rs measures the models
(docs/models/regulators.md); after a change: cargo test -p cli --test regulator_models -- --include-ignored
"""
import json, math, os, sys

EX = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "crates", "part-lib", "examples")
K4T = 4 * 1.380649e-23 * 300.15  # the simulator's default 27 C


def g(x):
    return f"{x:.6g}"


def noise_gain(density):
    """Gain on a 1 kohm resistor's noise voltage that gives `density` V/rtHz."""
    return density / math.sqrt(K4T * 1e3)


# ---------------------------------------------------------------- ADR4525
# ADI ADR4520/ADR4525/ADR4530/ADR4533/ADR4540/ADR4550 data sheet, Rev. 0, Table 4 (ADR4525), VIN = 3 V to 15 V, TA = 25 C.
ADR = dict(
    vout=2.5,
    line_ppm_v=1.0,  # line regulation typ (ppm/V)
    load_src_ppm_ma=30.0,  # load regulation, sourcing 0 to 10 mA, typ
    load_snk_ppm_ma=60.0,  # sinking 0 to -10 mA, typ
    iq=700e-6,  # quiescent current typ
    dropout=0.5,  # dropout max (no typ given)
    i_cap=10e-3,  # output current capacity (sourcing 10 mA, sinking 10 mA)
    rrr_db=90.0,  # ripple rejection at 1 kHz, typ
    en=41.3e-9,  # output noise density at 1 kHz, typ
)


def adr4525():
    p = ADR
    r_src = p["load_src_ppm_ma"] * 1e-6 * p["vout"] / 1e-3
    r_snk = p["load_snk_ppm_ma"] * 1e-6 * p["vout"] / 1e-3
    # ripple: a capacitor from the input into a small resistor gives |H| = 2 pi f R C; at 1 kHz it is 10^(-rrr/20)
    c_r = 1e-9
    r_r = 10 ** (-p["rrr_db"] / 20) / (2 * math.pi * 1e3 * c_r)
    # the data sheet gives the capacity (10 mA each way), not a short-circuit current: the model limits at twice the capacity
    # (exactly at the capacity, a 10 mA load would leave the output voltage undefined)
    cap = 2 * p["i_cap"]
    x_src = f"(V(r2,gnd)-V(vout,gnd))/{g(r_src)}"
    x_snk = f"(V(vout,gnd)-V(r2,gnd))/{g(r_snk)}"
    return "\n".join([
        ".subckt ADR4525 vin gnd vout",
        "* quiescent current (typ), fading out below about 2 V of input",
        f"Bq vin gnd I={g(p['iq'])}*step(V(vin,gnd)-2,0.5)",
        "* ripple from the input: rises 20 dB/decade, the data sheet's ripple rejection at 1 kHz",
        f"Cr vin h {g(c_r)}",
        f"Rr h gnd {g(r_r)}",
        "* output noise (density at 1 kHz): a 1 k resistor's noise, scaled",
        "Rnz nz gnd 1k",
        "* the reference: line regulation from 5 V in; below the dropout it follows the input",
        f"Bref r gnd V=min({g(p['vout'])}*(1+{g(p['line_ppm_v'] * 1e-6)}*(V(vin,gnd)-5))+V(vt,gnd),V(vin,gnd)-{g(p['dropout'])})+V(h,gnd)",
        "* the output's difference from part to part (0 V; the variation analyses move it over the B grade's initial accuracy)",
        "Vtol vt gnd DC 0",
        f"* spread Vtol {g(-p['vout'] * 0.02e-2)} {g(p['vout'] * 0.02e-2)} vout (data sheet: B grade initial accuracy 0.02 %)",
        f"En r2 r nz gnd {g(noise_gain(p['en']))}",
        "* output: sourcing from vin and sinking to gnd through the load regulation's resistance, each up to twice the current capacity",
        f"Bsrc vin vout I=limit({x_src},0,{g(cap)})*step({x_src},1u)",
        f"Bsnk vout gnd I=limit({x_snk},0,{g(cap)})*step({x_snk},1u)",
        ".ends ADR4525",
    ])


ADR_LIMITS = [
    "データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: ADI ADR4520/4525/4530/4533/4540/4550 データシート Rev. 0 の表 4 (ADR4525)。データシートとの比べは docs/models/regulators.md。",
    "再現しているもの: 出力 2.500 V、ライン・レギュレーション 1 ppm/V、ロード・レギュレーション (吐き出し 30 ppm/mA・吸い込み 60 ppm/mA)、電源電流 700 µA、ドロップアウト 0.5 V (データシートは max だけ)、出力電流の能力 ±10 mA (電流の上限は、データシートに短絡電流が無いので、能力の 2 倍の 20 mA で切る)、1 kHz のリップル除去 90 dB、1 kHz の出力雑音 41.3 nV/√Hz。",
    "出力電圧の部品ごとの違い (B グレードの初期誤差 ±0.02%、±0.5 mV) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析はちょうど 2.500 V)。",
    "扱わないもの: A グレードの初期誤差 (±0.04%) とはんだ付けのずれ (±0.02%)、温度係数 (B グレード max 2 ppm/°C。typ が無いので 0)、ヒステリシス、長期のドリフト、1/f 雑音 (0.1〜10 Hz の 1.25 µVp-p)、起動の時間 (125 µs)、出力の容量 (1 µF 以上が必要) による安定性。",
    "最終の確認は、メーカーの公式モデルで行ってください。",
]

# ---------------------------------------------------------------- LP5907-4.5
# TI LP5907 data sheet SNVS798Q (revised July 2025), 5.5 Electrical Characteristics: VIN = VOUT(NOM) + 1 V, VEN = 1.2 V,
# IOUT = 1 mA, CIN = COUT = 1 uF.
LDO = dict(
    vout=4.5,
    line_pct_v=0.02,  # line regulation typ (%/V)
    load_pct_ma=0.001,  # load regulation typ (%/mA)
    iq0=12e-6,  # quiescent current at 0 mA, typ
    iq250=250e-6,  # at 250 mA, typ
    i_off=0.2e-6,  # disabled
    vdo_100=0.05,  # dropout at 100 mA, typ (the output 100 mV below nominal)
    isc=0.5,  # short-circuit current limit, typ
    psrr=[(100, 90), (1e3, 82), (1e4, 65), (1e5, 60)],  # at 20 mA, typ
    noise_rms=10e-6,  # 10 Hz to 100 kHz, 1 mA, typ
    vil=0.4,  # EN low threshold (min) and high threshold (max): the model switches half way
    vih=1.2,
    r_en=1e6,  # EN pull-down
    r_dis=230,  # output discharge when disabled
    t_on=80e-6,  # turn-on time typ (EN to 95 %)
)


def lp5907():
    p = LDO
    gm = 1e-3 / (p["load_pct_ma"] / 100 * p["vout"])  # A/V: 1 mA more load drops the output by load_pct_ma
    rds = p["vdo_100"] / 0.1  # the pass device in dropout
    en_th = (p["vil"] + p["vih"]) / 2
    tau = p["t_on"] / 3  # 95 % in t_on
    # ripple: a flat floor (the 100 Hz figure) plus a high-pass that reaches the 100 kHz figure
    floor = 10 ** (-p["psrr"][0][1] / 20)
    hi = 10 ** (-p["psrr"][3][1] / 20)
    fc = 15e3
    c_h = 1e-9
    r_h = 1 / (2 * math.pi * fc * c_h)
    density = p["noise_rms"] / math.sqrt(1e5 - 10)
    slope = (p["iq250"] - p["iq0"]) / 0.25
    on = "V(ss,gnd)"
    x = f"min(min({g(gm)}*(V(tgt,gnd)-V(out,gnd)),(V(in,gnd)-V(out,gnd))/{g(rds)}),{g(p['isc'])})"
    return "\n".join([
        ".subckt LP5907-4.5 in gnd en out",
        "* enable: 1 Meg pull-down; on half way between the low and high thresholds; soft start to 95 % in the turn-on time",
        f"Ren en gnd {g(p['r_en'])}",
        f"Bon on gnd V=step(V(en,gnd)-{g(en_th)},0.05)",
        # a large capacitor: the resistor's noise on it (sqrt(kT/C)) would otherwise ride on the output (1 nF gave 9 uVrms)
        f"Rss on ss {g(tau / 10e-6)}",
        "Css ss gnd 10u",
        "* line regulation (slow, as the data sheet's DC figure) and the ripple from the input (a floor and a high-pass)",
        "Rlf in lf 1.59Meg",
        "Clf lf gnd 100n",
        f"Rhp_c in hp1 1",
        f"Chp hp1 hp {g(c_h)}",
        f"Rhp hp gnd {g(r_h)}",
        "* output noise (10 Hz to 100 kHz, taken as flat): a 1 k resistor's noise, scaled",
        "Rnz nz gnd 1k",
        f"Btgt t0 gnd V={on}*({g(p['vout'])}*(1+{g(p['line_pct_v'] / 100)}*(V(lf,gnd)-{g(p['vout'] + 1)}))+V(vt,gnd))+{g(floor)}*(V(in,gnd)-{g(p['vout'] + 1)})+{g(hi)}*V(hp,gnd)",
        "* the output's difference from part to part (0 V; the variation analyses move it over the data sheet's 2 %)",
        "Vtol vt gnd DC 0",
        f"* spread Vtol {g(-p['vout'] * 0.02)} {g(p['vout'] * 0.02)} vout (data sheet: output accuracy 2 %)",
        f"En tgt t0 nz gnd {g(noise_gain(density))}",
        "* pass device: the error amplifier's current (the load regulation), no more than the input allows through the pass",
        "* device's resistance (dropout) and the current limit; only from in to out",
        f"Bpass in out I=max({x},0)",
        "* quiescent current: grows with the load; a little when disabled",
        f"Bq in gnd I={on}*({g(p['iq0'])}+{g(slope)}*max({x},0))+{g(p['i_off'])}",
        "* output discharge when disabled",
        f"Bdis out gnd I=(1-{on})*V(out,gnd)/{g(p['r_dis'])}",
        ".ends LP5907-4.5",
    ])


LDO_LIMITS = [
    "データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: TI LP5907 データシート SNVS798Q の 5.5 節 (VIN = 5.5 V、IOUT = 1 mA、CIN = COUT = 1 µF)。データシートとの比べは docs/models/regulators.md。",
    "再現しているもの: 出力 4.5 V、ライン・レギュレーション 0.02 %/V、ロード・レギュレーション 0.001 %/mA、電源電流 (無負荷 12 µA、250 mA で 250 µA、停止中 0.2 µA)、ドロップアウト (100 mA で 50 mV)、電流制限 500 mA、PSRR (100 Hz 90 dB・1 kHz 82 dB・10 kHz 65 dB・100 kHz 60 dB の近似)、出力雑音 10 µVrms (10 Hz〜100 kHz を平らとした)、EN (1 MΩ のプルダウン、0.8 V で切り替え)、起動 80 µs、停止中の放電 230 Ω。",
    "出力電圧の部品ごとの違い (±2%) は、ばらつき・最悪ケースの解析でだけ入ります (ふだんの解析はちょうど 4.5 V)。",
    "扱わないもの: 出力コンデンサの容量・ESR による安定性 (どのコンデンサでも安定に動きます)、負荷の急変での応答 (データシート ±40 mV)、熱による停止、温度による変化。",
    "最終の確認は、メーカーの公式モデルで行ってください。",
]

DESC = {
    "ADR4525_SOIC-8": ("Analog Devices ADR4525 2.5 V 電圧基準 (SOIC-8)。", "シミュレーションモデルなし",
                       "データシートの typ から作った等価モデル (ライン・ロード・レギュレーション、電源電流、リップル除去、雑音)"),
    "LP5907-4.5_SOT-23-5": ("TI LP5907MFX-4.5 低雑音 LDO", "シミュレーションモデルなし",
                            "データシートの typ から作った等価モデル (ドロップアウト、電流制限、PSRR、雑音、EN と起動)"),
}

PARTS = {
    "ADR4525_SOIC-8": (adr4525, ["2", "4", "6"], ADR_LIMITS),
    "LP5907-4.5_SOT-23-5": (lp5907, ["1", "2", "3", "5"], LDO_LIMITS),
}

for f, (make, pins, limits) in PARTS.items():
    path = os.path.join(EX, f + ".part.json")
    j = json.load(open(path, encoding="utf-8"))
    name = j["name"]
    origin = j.get("sim", {}).get("origin")
    j["sim"] = {"kind": "subckt", "name": name, "text": make(), "pins": pins, "limits": limits}
    if origin:
        j["sim"]["origin"] = origin
    start, old, new = DESC[f]
    assert j["description"].startswith(start), (f, j["description"][:40])
    j["description"] = j["description"].replace(old, new)
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(j, ensure_ascii=False, indent=2) + "\n")
    print("wrote", f)
