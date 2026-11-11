"""Writes the op-amp macro models into the bundled part JSON files from the data sheets' typ values.

    python tools/gen_opamp_models.py

The numbers below are typed in from the data sheets named in DS (2026-11-11); docs/model-policy.md says how the models
are made, docs/models/opamps.md compares them with the data sheets (crates/cli/tests/opamp_models.rs measures them).
After a change: cargo test -p cli --test opamp_models -- --include-ignored (writes the table again).
"""
import json, math, os, sys

EX = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "crates", "part-lib", "examples")

# typ values from the data sheets (docs/models/opamps.md lists the sections)
PARTS = {
    "LM358_SOIC-8": dict(name="LM358", aol_db=100, gbw=0.7e6, sr=0.3e6, vos=3e-3, ib=-20e-9, iq=350e-6, rin="2Meg",
                         hu=1.4, hl=0.005, isc=40e-3, en=40e-9, ro=50, pm=None),
    "LM324_SOIC-14": dict(name="LM324", aol_db=100, gbw=1.2e6, sr=0.5e6, vos=3e-3, ib=-20e-9, iq=175e-6, rin="2Meg",
                          hu=1.4, hl=0.005, isc=40e-3, en=35e-9, ro=50, pm=None),
    "TL072_SOIC-8": dict(name="TL072", aol_db=106.02, gbw=5.25e6, sr=20e6, vos=3e-3, ib=65e-12, iq=1.4e-3, rin="1e12",
                         hu=1.4, hl=1.4, isc=26e-3, en=37e-9, ro=100, pm=56),
    "NE5532_SOIC-8": dict(name="NE5532", aol_db=100, gbw=12e6, sr=5e6, vos=0.5e-3, ib=200e-9, iq=3e-3, rin="300k",
                          hu=2, hl=2, isc=38e-3, en=5e-9, ro=50, pm=None),
    "MCP6001_SOT-23-5": dict(name="MCP6001", aol_db=112, gbw=1.0e6, sr=0.6e6, vos=0.0, ib=1e-12, iq=100e-6, rin="1e13",
                             hu=0.010, hl=0.010, isc=23e-3, en=28e-9, ro=50, pm=None),
    "MCP6002_SOIC-8": dict(name="MCP6002", aol_db=112, gbw=1.0e6, sr=0.6e6, vos=0.0, ib=1e-12, iq=100e-6, rin="1e13",
                           hu=0.010, hl=0.010, isc=23e-3, en=28e-9, ro=50, pm=None),
}

K4T = 4 * 1.380649e-23 * 300.15  # the simulator's default 27 °C


def g(x):
    return f"{x:.6g}"


def model(p):
    cp = 1e-9
    gm = 2 * math.pi * p["gbw"] * cp
    if p["pm"] is not None:
        # with the second pole the gain at gbw is sin(pm) of the one-pole gain: raise gm so that unity stays at gbw
        gm /= math.sin(math.radians(p["pm"]))
    rp = 10 ** (p["aol_db"] / 20) / gm
    imax = p["sr"] * cp
    # the noise of a 1 kΩ resistor, scaled to the data sheet's input voltage noise at 1 kHz
    en_gain = p["en"] / math.sqrt(K4T * 1e3)
    n = p["name"]
    lines = [
        f".subckt {n} inp inn out vp vn",
        "* input: offset (data sheet typ), voltage noise, bias currents (+ = into the pin), resistance",
        f"Vos inp i1 DC {g(p['vos'])}",
        f"En i1 i2 nz 0 {g(en_gain)}",
        "Rnz nz 0 1k",
        f"Ibp inp vn DC {g(p['ib'])}",
        f"Ibn inn vn DC {g(p['ib'])}",
        f"Rin inp inn {p['rin']}",
        "* gain stage: gm into Cp || Rp; the current is limited to SR * Cp (slew rate)",
        f"Bgm 0 n0 I=limit({g(gm)}*V(i2,inn),{g(-imax)},{g(imax)})",
        f"Rp n0 0 {g(rp)}",
        f"Cp n0 0 {g(cp)}",
    ]
    src = "n0"
    if p["pm"] is not None:
        # a second pole that leaves the data sheet's phase margin at the unity-gain frequency
        f2 = p["gbw"] / math.tan(math.radians(90 - p["pm"]))
        lines += [
            "* second pole (phase margin)",
            "E2 p2a 0 n0 0 1",
            "R2 p2a p2 1k",
            f"C2 p2 0 {g(1 / (2 * math.pi * 1e3 * f2))}",
        ]
        src = "p2"
    lines += [
        "* output swing: clamped hu below vp and hl above vn",
        "Eu u 0 vp 0 1",
        f"Vhu u uh DC {g(p['hu'])}",
        f"Ex1 x1 0 {src} uh 1000",
        "D1 x1 y1 DLIM",
        "Ry1 y1 0 1k",
        "Ey1 y1k 0 y1 0 0.001",
        f"Ec1 n1 0 {src} y1k 1",
        "El l 0 vn 0 1",
        "Vtop u ut DC -0.5",
        "Dtop n0 ut DLIM",
        "Vbot l lb DC 0.5",
        "Dbot lb n0 DLIM",
        f"Vhl lh l DC {g(p['hl'])}",
        "Ex2 x2 0 lh n1 1000",
        "D2 x2 y2 DLIM",
        "Ry2 y2 0 1k",
        "Ey2 y2k 0 0 y2 0.001",
        "Ec2 n 0 n1 y2k 1",
        "* output: ro, current limited to the short-circuit current, taken from vp (source) and returned to vn (sink)",
        # x = the output current; split smoothly (step) so that the sum is x with a smooth slope at x = 0
        f"Bsrc vp out I=limit((V(n)-V(out))/{p['ro']},{g(-p['isc'])},{g(p['isc'])})*step(limit((V(n)-V(out))/{p['ro']},{g(-p['isc'])},{g(p['isc'])}),1u)",
        f"Bsnk out vn I=-limit((V(n)-V(out))/{p['ro']},{g(-p['isc'])},{g(p['isc'])})*step(-limit((V(n)-V(out))/{p['ro']},{g(-p['isc'])},{g(p['isc'])}),1u)",
        "* quiescent current of one amplifier",
        f"Iq vp vn DC {g(p['iq'])}",
        ".model DLIM D(IS=1e-12 N=1)",
        f".ends {n}",
    ]
    return "\n".join(lines)


DS = {
    "LM358": ("https://www.ti.com/lit/ds/symlink/lm358.pdf", "TI SLOS068AB の 5.7 節(B 版ではない LM358)"),
    "LM324": ("https://www.ti.com/lit/ds/symlink/lm324.pdf", "TI SLOS066AE の 6.7・6.10 節"),
    "TL072": ("https://www.ti.com/lit/ds/symlink/tl072.pdf", "TI SLOS080W の 5.8・5.9 節(TL07xC。短絡電流は C の表に無いので TL07xH の 5.7 節の値)"),
    "NE5532": ("https://www.ti.com/lit/ds/symlink/ne5532.pdf", "TI SLOS075K の 5.5・5.6 節"),
    "MCP6001": ("https://ww1.microchip.com/downloads/en/DeviceDoc/MCP6001-1R-1U-2-4-1-MHz-Low-Power-Op-Amp-DS20001733L.pdf", "Microchip DS20001733L の DC・AC の表"),
}
DS["MCP6002"] = DS["MCP6001"]

DESC = {
    "LM358": "デュアルオペアンプ (SOIC-8)。チャネル1のみ記号化。単電源 3〜32 V、入力は負の電源まで。",
    "LM324": "クワッドオペアンプ (SOIC-14)。チャネル1のみ記号化 (ピン1=OUT1 2=IN1- 3=IN1+ 4=V+ 11=V-)。単電源で使える。",
    "TL072": "JFET入力デュアルオペアンプ (SOIC-8)。チャネル1のみ記号化。入力電流が pA と小さい。",
    "NE5532": "低雑音デュアルオペアンプ (SOIC-8)。チャネル1のみ記号化。オーディオ用。",
    "MCP6001": "レールtoレール入出力オペアンプ (SOT-23-5, ピン1=OUT 2=VSS 3=IN+ 4=IN- 5=VDD)。単一電源 1.8〜6 V。",
    "MCP6002": "レールtoレール入出力デュアルオペアンプ (SOIC-8)。チャネル1のみ記号化。単一電源 1.8〜6 V。",
}


def eng(v, unit):
    for k, pre in ((1e6, "M"), (1e3, "k"), (1, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n"), (1e-12, "p")):
        if abs(v) >= k:
            return f"{v / k:.3g} {pre}{unit}"
    return f"0 {unit}"


def summary(p):
    s = f"開ループ利得 {p['aol_db']:.0f} dB、GBW {eng(p['gbw'], 'Hz')}"
    if p["pm"] is not None:
        s += f"(位相余裕 {p['pm']}°)"
    s += f"、スルーレート {p['sr'] / 1e6:.3g} V/µs、電源電流 {eng(p['iq'], 'A')}/回路"
    return s


def limits(p):
    n = p["name"]
    url, where = DS[n]
    vos = f"入力オフセット {eng(p['vos'], 'V')}" if p["vos"] else "入力オフセット 0 (データシートに typ が無い)"
    return [
        f"データシートの typ の値から作った等価モデルです (メーカーのモデルではありません)。出典: {where}。データシートとの比べは docs/models/opamps.md。",
        f"再現しているもの: {summary(p)}、{vos}、入力バイアス電流 {eng(abs(p['ib']), 'A')}、"
        f"出力の振れ幅 (電源から {eng(p['hu'], 'V')} / {eng(p['hl'], 'V')})、出力の短絡電流 {eng(p['isc'], 'A')}、1 kHz の入力換算雑音 {eng(p['en'], 'V')}/√Hz。",
        "扱わないもの: 入力できる電圧の範囲 (超えたときの動き)、CMRR と PSRR、1/f 雑音 (低い周波数でも 1 kHz と同じ雑音)、温度による変化、"
        "電源電圧による GBW の変化、オフセットとバイアス電流のばらつき (typ の値で固定)、チャネル2。",
        "最終の確認は、メーカーの公式モデルで行ってください (公開されていれば「部品の取り込み」で読めます)。",
        "電源ピンをつながないと、出力は 0V に張り付きます。",
    ]


for f, p in PARTS.items():
    path = f"{EX}/{f}.part.json"
    j = json.load(open(path, encoding="utf-8"))
    j["sim"]["text"] = model(p)
    j["sim"]["limits"] = limits(p)
    j["description"] = DESC[p["name"]] + summary(p) + "。データシートから作った等価モデル"
    j["datasheet"] = DS[p["name"]][0]
    if "params" in j["sim"]:
        j["sim"]["params"]["gbw"] = float(p["gbw"])
        j["sim"]["params"]["gain_dc"] = float(round(10 ** (p["aol_db"] / 20)))
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(j, ensure_ascii=False, indent=2) + "\n")
    print("wrote", f)
