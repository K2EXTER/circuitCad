"""Write crates/part-lib/examples/ADC4_SAR_DIP-10.part.json: a 4-bit successive-approximation ADC from digital cells.

python tools/gen_adc4_sar.py (the tables of the `lut` cells are worked out from their logic here, not typed)."""
import json, os, sys
root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

def table(f, n):
    # one word per address, address bit 0 = the first input
    return ",".join(str(int(f(*[(a >> k) & 1 for k in range(n)]))) for a in range(1 << n))

# next bit j: inputs start, tj, cmp, bj -> !start & (tj ? cmp : bj)
bit = table(lambda s, t, c, b: (not s) and (c if t else b), 4)
# end of conversion: inputs start, t0, eoc -> !start & (t0 | eoc)
eoc = table(lambda s, t0, e: (not s) and (t0 or e), 3)
L = [
    "* ADC4_SAR teaching model: a 4-bit successive-approximation converter. A rising CLK edge with START high clears the result and",
    "* starts; the next four edges try D3, D2, D1, D0 in turn: the bit under trial is set, the internal DAC gives code / 16 of the",
    "* supply, and the comparator (AIN above the DAC) keeps or clears the bit. EOC goes high with the fourth result (5 clocks",
    "* after START) and stays high until the next START. code = floor(16 * AIN / supply). No sample-and-hold: AIN is read at each trial.",
    ".subckt ADC4_SAR ain clk start d0 d1 d2 d3 eoc vcc gnd",
    "* the token that walks through the four trials: t3 one clock after START, then t2, t1, t0",
    "Yt3 dff t3 0 gnd vcc 60 0.5 0 0 0 start clk 0 0",
    "Yt2 dff t2 0 gnd vcc 60 0.5 0 0 0 t3 clk 0 0",
    "Yt1 dff t1 0 gnd vcc 60 0.5 0 0 0 t2 clk 0 0",
    "Yt0 dff t0 0 gnd vcc 60 0.5 0 0 0 t1 clk 0 0",
    "* the result: during its trial a bit takes the comparator, otherwise it keeps its value; START clears it",
]
for j in (3, 2, 1, 0):
    L += [f"Yn{j} lut n{j} 0 gnd vcc 60 0.5 0 0 0 table={bit} start t{j} cmp d{j}",
          f"Yd{j} dff d{j} 0 gnd vcc 60 0.5 0 0 0 td=20n n{j} clk 0 0"]
L += [
    f"Yne lut ne 0 gnd vcc 60 0.5 0 0 0 table={eoc} start t0 eoc",
    "Ye dff eoc 0 gnd vcc 60 0.5 0 0 0 td=20n ne clk 0 0",
    "* the code the DAC is given: the result with the bit under trial set",
]
for j in (3, 2, 1, 0):
    L.append(f"Yc{j} or c{j} 0 gnd vcc 60 0.5 0 0 0 d{j} t{j}")
L += [
    "* the DAC (code / 16 of the supply, read without load) and the comparator (high when AIN is above it, 1 mV wide)",
    "Bdac dac gnd V=V(vcc,gnd)*(8*step(V(c3,gnd)-0.5*V(vcc,gnd),1m)+4*step(V(c2,gnd)-0.5*V(vcc,gnd),1m)+2*step(V(c1,gnd)-0.5*V(vcc,gnd),1m)+step(V(c0,gnd)-0.5*V(vcc,gnd),1m))/16",
    "Bcmp cmp gnd V=V(vcc,gnd)*step(V(ain,gnd)-V(dac,gnd),1m)",
    ".ends ADC4_SAR",
]
text = "\n".join(L)
pins = ["AIN", "CLK", "START", "D0", "D1", "D2", "D3", "EOC", "VCC", "GND"]
# DIP-10 numbering: 1..5 down the left, 6..10 up the right
order = ["AIN", "CLK", "START", "EOC", "GND", "D0", "D1", "D2", "D3", "VCC"]
side = {"AIN": "left", "CLK": "left", "START": "left", "EOC": "right", "GND": "bottom", "D0": "right", "D1": "right", "D2": "right", "D3": "right", "VCC": "top"}
part = {
    "name": "ADC4_SAR",
    "category": "ロジックIC",
    "description": "4 ビットの逐次比較型 AD コンバータ (教材用の動作モデル、DIP-10)。START を High にして CLK を入れると、次の 4 クロックで D3 から D0 の順に 1 ビットずつ決め、5 クロック目に EOC が High になります。値は floor(16 × AIN ÷ 電源電圧) (電源 5V なら 1 きざみ 0.3125V)。比較器 1 個で済むかわりに、変換にクロックが要ります",
    "prefix": "U",
    "footprint_name": "DIP-10_W7.62",
    "pins": [{"number": str(i + 1), "name": n, "side": side[n]} for i, n in enumerate(order)],
    "package": {"type": "dual", "pins": 10, "pitch": 2.54, "row_spacing": 7.62, "pad_len": 1.6, "pad_width": 1.6, "drill": 0.8},
    "sim": {
        "kind": "subckt",
        "name": "ADC4_SAR",
        "text": text,
        # the subckt's node order (ain clk start d0 d1 d2 d3 eoc vcc gnd) as pin numbers
        "pins": [str(order.index(n) + 1) for n in pins],
        "params": {},
        "limits": [
            "デジタルセル (行列の外で計算する素子) と、式で書いた DAC と比較器で作った動作モデルです。実在の部品ではなく、教材用です。逐次比較 (二分探索) の動き: 1 クロックに 1 ビットずつ、上の桁から「1 にしてみて、AIN を超えたら 0 に戻す」を繰り返します。",
            "変換の基準 (満量程) は VCC と GND の電位差です。AIN が GND〜VCC の外に出ると、0 か 15 に張り付きます。",
            "サンプルホールドはありません。変換の途中で AIN が動くと、ビットごとに違う時刻の AIN を読んだ値になります (実物の逐次比較型は、変換の始めに入力を保持します)。比較器にヒステリシスやオフセットはなく、AIN の入力抵抗は無限大です。CLK から D0〜D3・EOC の遅れは 20ns の一定値です。",
        ],
        "origin": {"kind": "generic", "source": "ロジックの動作モデル (しきい値は電源の半分、出力抵抗 60 Ω、遅れは一定の見積もり)"},
    },
}
open(os.path.join(root, "crates/part-lib/examples/ADC4_SAR_DIP-10.part.json"), "w", encoding="utf-8", newline="\n").write(json.dumps(part, ensure_ascii=False, indent=2) + "\n")
print("wrote ADC4_SAR_DIP-10")
