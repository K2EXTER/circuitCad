"""Writes the bundled diode models (M9) from their data sheets, with the same fit as the part form's diode template
(crates/part-lib/src/diode.rs: V = N*VT*ln(I/IS + 1) + I*RS through points read off the forward characteristics).

    python tools/gen_diode_models.py

Every number below was read on 2026-11-12 from the data sheet named next to it, from the tables and from the graphs
rendered as pictures (graph readings: the pixel position of the curve, so a reading is good to about 1 %).
docs/model-policy.md says how bundled models are made; crates/cli/tests/integration/diode_models.rs measures the models and writes the
comparison tables (docs/models/diodes.md, crates/part-lib/model_checks.json).
After a change: python tools/collect_legacy_models.py, then cargo test -p cli --test integration diode_models -- --include-ignored,
then cargo test -p app refresh_sample_models -- --ignored (docs/model-policy.md, the steps after a rebuild).
"""
import json, math, os, sys

EX = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "crates", "part-lib", "examples")
VT = 0.025865  # the simulator's kT/q at 27 C (part-lib diode.rs)

VISHAY_4148 = "Vishay 1N4148 データシート (Document Number 81857, Rev. 1.6, 2024-11)"
ONSEMI_400X = "onsemi 1N4001〜1N4007 データシート (1N4001/D Rev. 18, 2024-06)"

PARTS = {
    "1N4148": dict(
        name="1N4148",
        src=VISHAY_4148,
        url="https://www.vishay.com/docs/81857/1n4148.pdf",
        # Fig. 1 (typical VF vs. Tj), the four currents at 25 C
        forward=[(0.1e-3, 0.499), (1e-3, 0.610), (10e-3, 0.730), (100e-3, 0.877)],
        forward_at="図 1 (VF と Tj の代表特性) の 25 °C",
        # V(BR) min 100 V at IR = 100 uA
        bv=(100.0, 100e-6, "V(BR) の最小 100 V (IR = 100 µA)"),
        # CD max 4 pF at VR = 0 (no typ, no curve): one point, VJ and M assumed
        cap=[(0.0, 4e-12)],
        cap_at="CD の最大 4 pF (VR = 0 V)。曲線が無いので VJ = 0.7 V・M = 0.5 と仮定",
        # trr max 8 ns with IF = IR = 10 mA, recovery to 1 mA: TT = trr / ln(1 + IF/IR)
        trr=(8e-9, 10e-3, 10e-3, "trr の最大 8 ns (IF = IR = 10 mA、1 mA まで)"),
        desc="小信号スイッチングダイオード (DO-35)。データシート (Vishay) の代表特性から作った等価モデル",
    ),
    "1N4001_DO-41": dict(
        name="1N4001",
        src=ONSEMI_400X,
        url="https://www.onsemi.com/pdf/datasheet/1n4001-d.pdf",
        # Figure 1 (typical forward voltage), TC = 25 C
        forward=[(0.2, 0.824), (0.3, 0.845), (0.5, 0.871), (0.7, 0.891), (1.0, 0.916), (2.0, 0.983), (4.0, 1.072)],
        forward_at="図 1 (順方向電圧の代表特性) の 25 °C",
        # no breakdown is specified: VRSM (non-repetitive peak), and the table's IR max at the rated voltage
        bv=(60.0, 10e-6, "降伏はデータシートに無いので、非繰り返しの最大逆電圧 VRSM 60 V に置いた (電流は表の IR の最大 10 µA)"),
        # Figure 3 (typical capacitance), TJ = 25 C
        cap=[(1.0, 15.0e-12), (2.0, 12.0e-12), (4.0, 9.1e-12), (10.0, 5.92e-12), (30.0, 3.76e-12), (150.0, 2.35e-12)],
        cap_at="図 3 (容量の代表特性)",
        trr=None,
        desc="汎用整流ダイオード 50V 1A (DO-41、帯の側がカソード K)。データシート (onsemi) の代表特性から作った等価モデル",
    ),
    "1N4007_DO-41": dict(
        name="1N4007",
        src=ONSEMI_400X,
        url="https://www.onsemi.com/pdf/datasheet/1n4001-d.pdf",
        forward=[(0.2, 0.824), (0.3, 0.845), (0.5, 0.871), (0.7, 0.891), (1.0, 0.916), (2.0, 0.983), (4.0, 1.072)],
        forward_at="図 1 (順方向電圧の代表特性) の 25 °C",
        bv=(1200.0, 10e-6, "降伏はデータシートに無いので、非繰り返しの最大逆電圧 VRSM 1200 V に置いた (電流は表の IR の最大 10 µA)"),
        cap=[(1.0, 15.0e-12), (2.0, 12.0e-12), (4.0, 9.1e-12), (10.0, 5.92e-12), (30.0, 3.76e-12), (150.0, 2.35e-12)],
        cap_at="図 3 (容量の代表特性)",
        trr=None,
        desc="整流ダイオード 1000V 1A (DO-41、帯の側がカソード K)。データシート (onsemi) の代表特性から作った等価モデル",
    ),
}


def fit_forward(points):
    """IS, N, RS by least squares on V = a*ln(I) + c + r*I (the same as part-lib diode.rs fit)."""
    rows = [(math.log(i), 1.0, i) for i, _ in points]
    ys = [v for _, v in points]
    # normal equations, 3x3
    m = [[sum(r[a] * r[b] for r in rows) for b in range(3)] + [sum(r[a] * y for r, y in zip(rows, ys))] for a in range(3)]
    for col in range(3):
        piv = max(range(col, 3), key=lambda k: abs(m[k][col]))
        m[col], m[piv] = m[piv], m[col]
        for r in range(3):
            if r != col:
                f = m[r][col] / m[col][col]
                m[r] = [x - f * p for x, p in zip(m[r], m[col])]
    a, c, r = (m[k][3] / m[k][k] for k in range(3))
    assert r >= 0 and a > 0, (a, c, r)
    return math.exp(-c / a), a / VT, r


def fit_cap(points):
    """CJO, VJ, M from C(V) = CJO / (1 + V/VJ)^M: a grid over VJ and M, CJO by least squares in log."""
    if len(points) == 1:
        v, c = points[0]
        return c * (1 + v / 0.7) ** 0.5, 0.7, 0.5
    best = None
    for vj10 in range(1, 13):  # 0.1 .. 1.2 V
        vj = vj10 / 10
        for m100 in range(20, 71):  # 0.20 .. 0.70
            mm = m100 / 100
            logs = [math.log(c) + mm * math.log(1 + v / vj) for v, c in points]
            lc0 = sum(logs) / len(logs)
            err = sum((x - lc0) ** 2 for x in logs)
            if best is None or err < best[0]:
                best = (err, math.exp(lc0), vj, mm)
    return best[1], best[2], best[3]


def sig(x):
    return f"{x:.4g}"


def model(p):
    is_, n, rs = fit_forward(p["forward"])
    cj0, vj, mm = fit_cap(p["cap"])
    bv, ibv, _ = p["bv"]
    text = f".model {p['name']} D(IS={sig(is_)} N={n:.3f} RS={sig(rs)} BV={sig(bv)} IBV={sig(ibv)} CJO={sig(cj0)} VJ={vj:g} M={mm:g}"
    if p["trr"]:
        trr, i_f, i_r, _ = p["trr"]
        text += f" TT={sig(trr / math.log(1 + i_f / i_r))}"
    return text + ")", (is_, n, rs, cj0, vj, mm)


def limits(p, fit):
    is_, n, rs, cj0, vj, mm = fit
    rows = [
        f"データシートの代表特性から作った等価モデルです (メーカーのモデルではありません)。出典: {p['src']}。データシートとの比べは docs/models/diodes.md。",
        f"順方向: {p['forward_at']} を読んだ {len(p['forward'])} 点に、IS・N・RS を当てはめました (部品作成フォームのダイオードと同じ計算)。",
        f"接合容量: {p['cap_at']}。逆電圧で小さくなります。",
        f"降伏: {p['bv'][2]}。",
    ]
    if p["trr"]:
        rows.append(f"逆回復: {p['trr'][3]} から、蓄積時間 TT を決めました (max の値なので、実物はふつう、これより速く戻ります)。")
    else:
        rows.append("逆回復時間は扱いません (データシートに trr が無い。標準の整流用で、数 µs ほど逆向きに流れます。逆回復のレッスンのサンプルは、自分で TT を足しています)。")
    rows += [
        "扱わないもの: 温度による順方向電圧の変化はシミュレータの標準の式 (1 °C あたり約 −2 mV)、逆方向の漏れ電流 (IS の分だけ。実物は電圧と温度で増えます)、自己発熱。耐圧や定格電流を超えても壊れません。",
        "最終の確認は、メーカーの公式モデルで行ってください (公開されていれば「部品の取り込み」で読めます)。",
    ]
    return rows


for f, p in PARTS.items():
    path = os.path.join(EX, f + ".part.json")
    j = json.load(open(path, encoding="utf-8"))
    text, fit = model(p)
    j["sim"]["text"] = text
    j["sim"]["limits"] = limits(p, fit)
    j["sim"]["origin"] = {"kind": "equivalent", "source": p["src"]}
    j["description"] = p["desc"]
    j["datasheet"] = p["url"]
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(j, ensure_ascii=False, indent=2) + "\n")
    print("wrote", f, text)
