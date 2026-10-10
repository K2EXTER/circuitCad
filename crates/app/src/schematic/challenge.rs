//! "作ってみよう": a goal instead of a finished circuit. The learner places and wires the parts, and a
//! "判定する" button judges the circuit by what it does (it is simulated with loads, inputs and
//! frequencies of the checker's own choosing), not by comparing it with one model answer. A failed
//! check says what to look at. Each challenge is plain data: its start sheet and its list of checks.

use super::*;

/// One line of the checklist after "判定する".
pub(super) struct CheckResult {
    pub label: String,
    /// None = not judged (an earlier check failed first).
    pub ok: Option<bool>,
    pub detail: String,
}

/// The challenge being worked on.
pub(super) struct ActiveChallenge {
    pub id: &'static str,
    /// 0 = the parts are placed, only wiring is left; 1 = place the parts too.
    pub level: u8,
    pub results: Vec<CheckResult>,
    pub hints: usize,
    pub passed: bool,
}

/// Which part a current check looks at.
#[derive(Clone, Copy)]
enum Who {
    Refdes(&'static str),
    /// The first part of this kind on the sheet (e.g. the LED).
    Kind(Kind),
}

/// One thing the judge tests. `set` = (part number, value) pairs applied to a copy of the learner's circuit
/// first (e.g. a different input voltage); `load` = a resistor from VOUT to GND the checker adds.
enum Check {
    /// No ERC error (wiring slips).
    Erc,
    /// A net with this name exists (the learner names the output so the judge can measure it).
    Named { net: &'static str },
    /// The voltage of `net` is within [lo, hi]. `explain` maps a measured value to advice ({v} = the value).
    /// `gate`: if this fails, the later checks are not judged yet.
    Voltage { label: &'static str, net: &'static str, load: Option<&'static str>, set: &'static [(&'static str, &'static str)], lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)], gate: bool },
    /// The voltage of `net` stays within [lo, hi] while source `vary.0` is set to `vary.1` and to `vary.2`.
    Stay { label: &'static str, net: &'static str, load: Option<&'static str>, vary: (&'static str, &'static str, &'static str), lo: f64, hi: f64, hint: &'static str },
    /// A current (absolute, in amperes) is within [lo, hi].
    Current { label: &'static str, who: Who, set: &'static [(&'static str, &'static str)], lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)] },
    /// AC gain of the net VOUT at `freq` (dB, against the 1 V AC source of the circuit) is within [lo, hi].
    Gain { label: &'static str, freq: f64, lo: f64, hi: f64, explain: &'static [(f64, f64, &'static str)] },
    /// The sheet has at least `min` parts of this kind (e.g. the two capacitors a regulator needs).
    Has { label: &'static str, kind: Kind, min: usize, hint: &'static str },
    /// No part is past its rating (judged with the given load / settings).
    Ratings { label: &'static str, load: Option<&'static str>, set: &'static [(&'static str, &'static str)] },
}

struct Challenge {
    id: &'static str,
    /// The lesson this challenge belongs to.
    lesson: &'static str,
    title: &'static str,
    goal: &'static str,
    /// From a nudge to nearly the answer; shown one at a time.
    hints: &'static [&'static str],
    /// A circuit that passes, as a sample file; `drop` / `values` / `labels` turn it into the answer.
    solution: &'static str,
    drop: &'static [&'static str],
    values: &'static [(&'static str, &'static str)],
    labels: &'static [(&'static str, &'static str, &'static str)],
    /// Net names of the sample to rename (so that the judged output net is called VOUT).
    rename: &'static [(&'static str, &'static str)],
    /// Level 0 / level 1 start sheets: the parts kept (with their positions) and the starting values.
    parts0: &'static [(&'static str, Pt)],
    parts1: &'static [(&'static str, Pt)],
    start_values: &'static [(&'static str, &'static str)],
    /// Ground symbols: (part, pin, dx, dy) from that pin, for level 0 / level 1.
    grounds0: &'static [(&'static str, &'static str, i32, i32)],
    grounds1: &'static [(&'static str, &'static str, i32, i32)],
    checks: &'static [Check],
}

const NO: &[(&str, &str)] = &[];

const CHALLENGES: &[Challenge] = &[
    Challenge {
        id: "zener",
        lesson: "zener",
        title: "ツェナーで 5V を作る",
        goal: "9V の電源から、約 5V (4.7〜5.6V) を作ります。使うのは、抵抗とツェナーダイオードだけです。出力には、負荷として 1kΩ から 330Ω をつなぐので、重い負荷でも電圧が崩れず、入力が 8〜10V に変わっても 5V 付近を保ち、部品が定格を超えない (いちばん厳しいのは 9V で負荷 330Ω のとき) ように、直列の抵抗の値を決めてください。出力のネット (抵抗とツェナーの間の点) には、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → 抵抗 → 出力 (VOUT)。出力と GND の間に、ツェナーを入れます。ツェナーは、ふつうのダイオードと逆向き (カソード = 縞のある側を出力側、アノードを GND 側) です。",
            "抵抗の値は、「学ぶ → 値の決め方」の「ツェナーダイオードで電圧を一定にする」で求められます。入力 9V、ツェナー電圧 5.1V、負荷の電流の最大 (5.1V ÷ 330Ω ≒ 15mA)、ツェナーに残す電流 5mA くらいで計算します。ツェナーは、ライブラリの 1N4733A (5.1V) を使います。",
            "答えの目安: 抵抗は 150Ω 前後 (130〜200Ω)。小さすぎると抵抗が熱で定格を超え (電力 = (9V − 5.1V)² ÷ R が、0805 のチップ抵抗の目安 0.125W 以下 → R ≧ 125Ω)、大きすぎると負荷 330Ω でツェナーに電流が残りません (R ≦ 220Ω)。「答えの回路を見る」で、できあがりを見られます。",
        ],
        solution: include_str!("../../../../docs/examples/zener_regulator.schjson"),
        drop: &["R2"],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("R1", (10, -6)), ("D1", (20, 0))],
        parts1: &[("V1", (0, 0))],
        start_values: NO,
        grounds0: &[("V1", "-", 0, 5), ("D1", "A", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "負荷 1kΩ で、出力が 4.7〜5.6V",
                net: "VOUT",
                load: Some("1k"),
                set: NO,
                lo: 4.7,
                hi: 5.6,
                explain: &[
                    (8.0, 1e9, "出力が {v} V で、電源の電圧に近いままです。ツェナーに電流が流れていません。ツェナーが GND につながっているか、向き (カソード = 縞を出力側) を確かめてください"),
                    (-1e9, 1.5, "出力が {v} V しかありません。ツェナーが、ふつうのダイオードの向き (順方向) になっていませんか。カソード (縞のある側) を出力側、アノードを GND 側にします"),
                    (1.5, 4.7, "出力が {v} V で、低すぎます。直列の抵抗が大きすぎて、ツェナーに電流が残っていないか、ツェナーの電圧が 5.1V でない部品です"),
                    (5.6, 8.0, "出力が {v} V で、高すぎます。ツェナーの電圧が 5.1V より高い部品ではありませんか"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "負荷 330Ω でも、出力が 4.7V 以上",
                net: "VOUT",
                load: Some("330"),
                set: NO,
                lo: 4.7,
                hi: 1e9,
                explain: &[(-1e9, 4.7, "負荷 330Ω では出力が {v} V に落ちます。直列の抵抗を小さくして、負荷が取る電流 (5.1V ÷ 330Ω ≒ 15mA) に加えて、ツェナーに数 mA が残るようにします。目安: 抵抗 ≦ (9V − 5.1V) ÷ (15mA + 数 mA) ≒ 200Ω 前後")],
                gate: false,
            },
            Check::Stay {
                label: "入力が 8〜10V に変わっても、出力が保たれる",
                net: "VOUT",
                load: Some("1k"),
                vary: ("V1", "8", "10"),
                lo: 4.5,
                hi: 5.8,
                hint: "入力が低いとツェナーに電流が残らず (抵抗が大きすぎる)、入力が高いと電流が増えすぎて電圧が上がります (抵抗が小さすぎる)",
            },
            Check::Ratings { label: "部品が定格を超えない", load: Some("330"), set: NO },
        ],
    },
    Challenge {
        id: "divider",
        lesson: "divider",
        title: "分圧で 3.3V を作る",
        goal: "5V の電源から、抵抗 2 本で 3.3V (3.2〜3.4V) を作ります。出力に 100kΩ の負荷 (マイコンの入力など) をつないでも 3.1〜3.5V を保ち、電源から流れる電流が 5mA 以下 (むだに流しすぎない) になるように、2 本の抵抗の値を決めてください。出力 (2 本の抵抗の間の点) には、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → R1 → 出力 (VOUT) → R2 → GND。出力電圧 = 電源 × R2 ÷ (R1 + R2) です。",
            "3.3V ÷ 5V = 0.66 なので、R1 : R2 は、約 1 : 2 (R2 が R1 の約 2 倍) です。「学ぶ → 値の決め方」の「分圧」でも求められます。抵抗の値が大きいほど電流は小さいですが、大きすぎると、出力につないだ負荷の影響で電圧が下がります。小さすぎると電流が流れすぎます。",
            "答えの目安: R1 = 5.1kΩ、R2 = 10kΩ (出力 3.31V)。R2 は、負荷 (100kΩ) の 1/10 以下にしておくと、負荷の影響が小さくなります。合計 1kΩ〜50kΩ くらいが、使いやすい範囲です。",
        ],
        solution: include_str!("../../../../docs/examples/divider_tolerance.schjson"),
        drop: &[],
        values: &[("R1", "5.1k"), ("R2", "10k")],
        labels: &[],
        rename: &[("MID", "VOUT")],
        parts0: &[("V1", (0, 0)), ("R1", (10, -6)), ("R2", (20, 0))],
        parts1: &[("V1", (0, 0))],
        start_values: &[("R1", "10k"), ("R2", "10k")],
        grounds0: &[("V1", "-", 0, 5), ("R2", "2", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "無負荷で、出力が 3.2〜3.4V",
                net: "VOUT",
                load: None,
                set: NO,
                lo: 3.2,
                hi: 3.4,
                explain: &[
                    (-1e9, 3.2, "出力が {v} V で、低すぎます。出力電圧 = 5V × R2 ÷ (R1 + R2) です。R2 を大きく (または R1 を小さく) します"),
                    (3.4, 1e9, "出力が {v} V で、高すぎます。出力電圧 = 5V × R2 ÷ (R1 + R2) です。R2 を小さく (または R1 を大きく) します"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "100kΩ の負荷をつないでも、出力が 3.1〜3.5V",
                net: "VOUT",
                load: Some("100k"),
                set: NO,
                lo: 3.1,
                hi: 3.5,
                explain: &[(-1e9, 3.1, "100kΩ の負荷をつなぐと、出力が {v} V に下がります。負荷が R2 と並列になるので、R2 が負荷に比べて大きいと影響が出ます。R1・R2 を小さくします (R2 は負荷の 1/10 以下が目安)")],
                gate: false,
            },
            Check::Current { label: "電源から流れる電流が 5mA 以下", who: Who::Refdes("V1"), set: NO, lo: 0.0, hi: 0.005, explain: &[(0.005, 1e9, "電源から {mA} mA 流れています。抵抗が小さすぎて、電流がむだに流れ、発熱します。R1・R2 を大きくします")] },
            Check::Ratings { label: "部品が定格を超えない", load: None, set: NO },
        ],
    },
    Challenge {
        id: "led",
        lesson: "led",
        title: "LED を約 10mA で光らせる",
        goal: "5V の電源で、LED を約 10mA (7〜13mA) で光らせます。LED は、電流を自分では制限できないので、直列の抵抗の値を決めてください (LED の順方向電圧は約 2V)。LED が定格を超えない (20mA 以下) ことも、判定されます",
        hints: &[
            "回路の形: 電源の + → 抵抗 → LED のアノード、LED のカソード → GND。LED は向きがあります (三角の向きが、電流の流れる向き)。",
            "抵抗の値は、「学ぶ → 値の決め方」の「LEDの電流制限抵抗」で求められます。抵抗 = (電源電圧 − LED の電圧) ÷ 電流 = (5V − 2V) ÷ 10mA です。",
            "答えの目安: 300Ω 前後 (E24 なら 330Ω)。LED の電圧は電流で少し変わるので、判定は 7〜13mA を合格にしています。",
        ],
        solution: include_str!("../../../../docs/examples/led.schjson"),
        drop: &[],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("R1", (10, -6)), ("D1", (20, -6))],
        parts1: &[("V1", (0, 0))],
        start_values: &[("R1", "1k")],
        grounds0: &[("V1", "-", 0, 5), ("D1", "K", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Current {
                label: "LED の電流が 7〜13mA",
                who: Who::Kind(Kind::Led),
                set: NO,
                lo: 0.007,
                hi: 0.013,
                explain: &[
                    (-1e9, 0.0005, "LED に電流が流れていません ({mA} mA)。LED の向き (アノードを電源側、カソードを GND 側) と、GND につながっているかを確かめてください"),
                    (0.0005, 0.007, "LED の電流が {mA} mA で、少なすぎます。抵抗が大きすぎます。小さくすると、電流が増えます"),
                    (0.013, 1e9, "LED の電流が {mA} mA で、多すぎます。抵抗が小さすぎます。大きくすると、電流が減ります"),
                ],
            },
            Check::Ratings { label: "部品が定格を超えない (LED は 20mA 以下)", load: None, set: NO },
        ],
    },
    Challenge {
        id: "rc_lowpass",
        lesson: "rc_lowpass",
        title: "カットオフ 1kHz のローパスフィルタ",
        goal: "抵抗とコンデンサで、約 1kHz から高い周波数を弱めるフィルタを作ります。1kHz で約 −3dB (−2〜−4dB)、100Hz ではほとんど下がらず (−1dB 以上)、10kHz では十分に下がる (−15dB 以下) ように、R と C の値を決めてください。入力の電源は、AC 解析用の「AC 1」が入っています。出力 (抵抗とコンデンサの間の点) には、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 入力 (電源の +) → 抵抗 → 出力 (VOUT) → コンデンサ → GND。出力は、抵抗とコンデンサの間の点です。",
            "カットオフ周波数 fc = 1 ÷ (2π × R × C) です。「学ぶ → 値の決め方」の「RCフィルタの抵抗」で、コンデンサ (例: 100n) から抵抗を求められます。fc で出力は約 −3dB (約 0.7 倍) になります。",
            "答えの目安: R = 1.6kΩ と C = 100nF (fc ≒ 1.0kHz)、または R = 1kΩ と C = 159nF。R と C の積が同じなら、どの組み合わせでも合格です。判定は、fc が約 0.8〜1.3kHz の範囲を合格にしています。",
        ],
        solution: include_str!("../../../../docs/examples/rc_lowpass.schjson"),
        drop: &[],
        values: NO,
        labels: &[("C1", "1", "VOUT")],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("R1", (10, -6)), ("C1", (20, 0))],
        parts1: &[("V1", (0, 0))],
        start_values: &[("R1", "10k"), ("C1", "1u")],
        grounds0: &[("V1", "-", 0, 5), ("C1", "2", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Gain {
                label: "1kHz で、約 −3dB (−2〜−4dB)",
                freq: 1000.0,
                lo: -4.0,
                hi: -2.0,
                explain: &[
                    (-1e9, -4.0, "1kHz のゲインが {v} dB で、下がりすぎです。カットオフ周波数が 1kHz より低い (R × C が大きすぎる) ので、R か C を小さくします"),
                    (-2.0, 1e9, "1kHz のゲインが {v} dB で、あまり下がっていません。カットオフ周波数が 1kHz より高い (R × C が小さすぎる) ので、R か C を大きくします"),
                ],
            },
            Check::Gain { label: "100Hz で、ほとんど下がらない (−1dB 以上)", freq: 100.0, lo: -1.0, hi: 1.0, explain: &[(-1e9, -1.0, "100Hz のゲインが {v} dB で、低い周波数まで下がっています。カットオフ周波数が低すぎます")] },
            Check::Gain { label: "10kHz で、十分に下がる (−15dB 以下)", freq: 10000.0, lo: -1e9, hi: -15.0, explain: &[(-15.0, 1e9, "10kHz のゲインが {v} dB で、高い周波数が十分に下がっていません。カットオフ周波数が高すぎます")] },
        ],
    },
    Challenge {
        id: "emitter_follower",
        lesson: "emitter_follower",
        title: "ツェナーとトランジスタで、重い負荷でも保つ 4.4V",
        goal: "9V の電源から、ツェナーとトランジスタ (エミッタフォロワ) で約 4.4V (4.2〜4.6V) を作ります。負荷が 220Ω でも、重い 47Ω でも電圧が崩れず、部品が定格を超えない (負荷 100Ω のとき) ようにします。ツェナーが作った電圧を、トランジスタのベースで受け、エミッタを出力にします。出力のネットには、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → 抵抗 → ツェナー (カソード側) = トランジスタのベース。ツェナーのアノード → GND。トランジスタのコレクタ → 電源の +、エミッタ → 出力 (VOUT)。",
            "ツェナーの電圧 (5.1V) から、ベース・エミッタ間の約 0.7V を引いた、約 4.4V が出力になります。ベース側の抵抗は、ツェナーに数 mA 流す値 (300Ω 前後) にします。トランジスタは、ライブラリの 2N3904 を使います。",
            "答えの目安: 抵抗 330Ω、ツェナー 1N4733A、トランジスタ 2N3904。ピンは、データシートで確かめます (2N3904 は、平らな面を手前にして、左から E・B・C)。「答えの回路を見る」で、できあがりを見られます。",
        ],
        solution: include_str!("../../../../docs/examples/emitter_follower.schjson"),
        drop: &["R2"],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("R1", (10, -8)), ("D1", (22, 2)), ("Q1", (34, -2))],
        parts1: &[("V1", (0, 0))],
        start_values: NO,
        grounds0: &[("V1", "-", 0, 5), ("D1", "A", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "負荷 220Ω で、出力が 4.2〜4.6V",
                net: "VOUT",
                load: Some("220"),
                set: NO,
                lo: 4.2,
                hi: 4.6,
                explain: &[
                    (-1e9, 1.0, "出力が {v} V しかありません。トランジスタのつなぎ方 (コレクタが電源、ベースがツェナー、エミッタが出力) とピンの並びを確かめてください"),
                    (1.0, 4.2, "出力が {v} V で、低すぎます。ツェナーの電圧が 5.1V になっていないか (ベース側の抵抗が大きすぎないか) 確かめてください"),
                    (4.6, 1e9, "出力が {v} V で、高すぎます。ベースの電圧より 0.7V 低くなるはずです。つなぎ方を確かめてください"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "負荷 47Ω でも、出力が 4.2〜4.6V",
                net: "VOUT",
                load: Some("47"),
                set: NO,
                lo: 4.2,
                hi: 4.6,
                explain: &[(-1e9, 4.2, "負荷 47Ω では出力が {v} V に落ちます。ツェナーに電流が残っていません。ベース側の抵抗を小さくして、ツェナーに流れる電流を増やします")],
                gate: false,
            },
            Check::Ratings { label: "部品が定格を超えない", load: Some("100"), set: NO },
        ],
    },
    Challenge {
        id: "opamp_inv",
        lesson: "opamp_inv",
        title: "ゲイン −10 の反転増幅回路",
        goal: "オペアンプ (LM358) で、入力を −10 倍にして出力する反転増幅回路を作ります。入力が 0.1V のとき出力が約 −1.0V (−1.05〜−0.95V)、0.2V のとき約 −2.0V (−2.1〜−1.9V) になるように、2 本の抵抗の値を決めてください。電源 (+5V と −5V) の 2 つは置いてあります。電源の + を VCC、− を VEE につなぎます。出力には、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 入力 → R1 → オペアンプの反転入力 (−)。出力 → R2 → 反転入力 (−)。非反転入力 (+) は GND。電源は、オペアンプの V+ に +5V、V− に −5V (電源 V3 の + を GND、− を V− へ)。",
            "ゲイン = −R2 ÷ R1 です。−10 倍にするには、R2 = R1 の 10 倍にします (例: R1 = 1kΩ、R2 = 10kΩ)。抵抗は、1kΩ〜1MΩ くらいが使いやすい範囲です。",
            "答えの目安: R1 = 1kΩ、R2 = 10kΩ。入力は、抵抗 R1 を通して、反転入力 (−) に入れます。「答えの回路を見る」で、できあがりを見られます。",
        ],
        solution: include_str!("../../../../docs/examples/opamp_inverting.schjson"),
        drop: &[],
        values: NO,
        labels: &[],
        rename: &[("OUT", "VOUT")],
        parts0: &[("V1", (0, 0)), ("R1", (14, 0)), ("R2", (26, -10)), ("U1", (28, 2)), ("V2", (44, -6)), ("V3", (44, 10))],
        parts1: &[("V1", (0, 0)), ("V2", (44, -6)), ("V3", (44, 10))],
        start_values: &[("R1", "1k"), ("R2", "1k")],
        grounds0: &[("V1", "-", 0, 5), ("V2", "-", 0, 5), ("V3", "+", 6, 0)],
        grounds1: &[("V1", "-", 0, 5), ("V2", "-", 0, 5), ("V3", "+", 6, 0)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "入力 0.1V のとき、出力が約 −1.0V",
                net: "VOUT",
                load: None,
                set: &[("V1", "0.1")],
                lo: -1.05,
                hi: -0.95,
                explain: &[
                    (0.03, 1e9, "出力が {v} V で、プラスです。反転増幅は、出力の向きが逆 (マイナス) になります。入力を、抵抗を通して、反転入力 (−) に入れ、非反転入力 (+) を GND につなぎます"),
                    (-1e9, -4.0, "出力が {v} V で、電源いっぱいに張り付いています。帰還がかかっていません。出力から反転入力 (−) へ、抵抗 R2 でつなぎます。電源 (VCC・VEE) が正しくつながっているかも確かめます"),
                    (-0.03, 0.03, "出力が {v} V で、ほとんど動きません。入力が反転入力につながっているか、電源 (VCC・VEE) がつながっているかを確かめます"),
                    (-4.0, -1.05, "出力が {v} V で、ゲインが −10 倍より大きいです。ゲイン = −R2 ÷ R1 なので、R2 を小さく (または R1 を大きく) します"),
                    (-0.95, -0.03, "出力が {v} V で、ゲインが −10 倍より小さいです。ゲイン = −R2 ÷ R1 なので、R2 を大きく (または R1 を小さく) します"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "入力 0.2V のとき、出力が約 −2.0V",
                net: "VOUT",
                load: None,
                set: &[("V1", "0.2")],
                lo: -2.1,
                hi: -1.9,
                explain: &[(-1e9, -2.1, "入力 0.2V で出力が {v} V です。ゲインが大きすぎます"), (-1.9, 1e9, "入力 0.2V で出力が {v} V です。ゲインが小さすぎます")],
                gate: false,
            },
            Check::Ratings { label: "部品が定格を超えない", load: None, set: NO },
        ],
    },
    Challenge {
        id: "series_regulator",
        lesson: "series_regulator",
        title: "ツェナー・オペアンプ・トランジスタで、自作の 5V 電源",
        goal: "9V の電源から、基準電圧 (ツェナー 3.3V)・オペアンプ (LM358)・パストランジスタ (2N3904)・出力を分ける 2 本の抵抗で、約 5V (4.8〜5.2V) を作ります。負荷が 100Ω でも 47Ω でも、入力が 8V から 12V に変わっても、出力が保たれ、部品が定格を超えない (いちばん厳しいのは 9V で負荷 47Ω のとき) ようにします。ツェナーに電流を流す抵抗 R1 (1kΩ) は、置いてあります。出力のネットには、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → R1 → ツェナー (カソード側) = 基準電圧。ツェナーのアノードは GND。基準電圧は、オペアンプの + 入力 (3 番ピン) へ。トランジスタは、コレクタを電源 (+)、ベースをオペアンプの出力 (1 番ピン)、エミッタを出力 (VOUT) に。出力を R3 と R4 で分けて、中点をオペアンプの − 入力 (2 番ピン) へ。オペアンプの電源は、8 番ピンを電源 (+)、4 番ピンを GND に。",
            "出力 = 基準 × (1 + R3 ÷ R4) です。3.3V を 5V にするので、R3 ÷ R4 ≒ 0.52。R4 を 10kΩ にすると、R3 は約 5.2kΩ (E24 で 5.1kΩ)。「学ぶ → 値の決め方」の「ツェナーダイオードで電圧を一定にする」で、基準の電流も確かめられます。ツェナーは、ライブラリの 1N4728A (3.3V) です。",
            "答えの目安: R3 = 5.1kΩ、R4 = 10kΩ (出力 4.97V)。ツェナーの電圧が個体差で ±5% 動くので、判定は 4.8〜5.2V にしています。「答えの回路を見る」で、できあがりを見られます。出力が 3.3V のままなら、− 入力を出力に直接つないでいませんか (分ける抵抗が要ります)。",
        ],
        solution: include_str!("../../../../docs/examples/series_regulator.schjson"),
        drop: &["R2"],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("R1", (14, -10)), ("D1", (28, 0)), ("U1", (44, -4)), ("Q1", (62, -8)), ("R3", (62, 10)), ("R4", (50, 14))],
        parts1: &[("V1", (0, 0)), ("R1", (14, -10))],
        start_values: NO,
        grounds0: &[("V1", "-", 0, 5), ("D1", "A", 0, 5), ("R4", "2", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "負荷 100Ω で、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("100"),
                set: NO,
                lo: 4.8,
                hi: 5.2,
                explain: &[
                    (-1e9, 0.5, "出力が {v} V しかありません。トランジスタ (コレクタ = 電源、ベース = オペアンプの出力、エミッタ = 出力) と、オペアンプの電源 (8 番 = 電源、4 番 = GND) のつなぎ方を確かめてください"),
                    (0.5, 3.0, "出力が {v} V で、低すぎます。オペアンプの + 入力に基準電圧 (ツェナー) が入っているか、− 入力が出力の分圧につながっているかを確かめます"),
                    (3.0, 4.2, "出力が {v} V で、基準電圧に近い値です。− 入力を出力に直接つないでいませんか。出力を R3 と R4 で分けて、その中点を − 入力につなぎます。出力 = 基準 × (1 + R3 ÷ R4) です"),
                    (4.2, 4.8, "出力が {v} V で、5V より低いです。出力 = 基準 × (1 + R3 ÷ R4) なので、R3 を大きく (または R4 を小さく) します"),
                    (5.2, 1e9, "出力が {v} V で、5V より高いです。出力 = 基準 × (1 + R3 ÷ R4) なので、R3 を小さく (または R4 を大きく) します"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "負荷 47Ω でも、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("47"),
                set: NO,
                lo: 4.8,
                hi: 5.2,
                explain: &[(-1e9, 4.8, "負荷 47Ω では、出力が {v} V に落ちます。トランジスタのベースに、オペアンプが電流を流せていません。トランジスタの接続を確かめてください")],
                gate: false,
            },
            Check::Stay {
                label: "入力が 8〜12V に変わっても、出力が保たれる",
                net: "VOUT",
                load: Some("100"),
                vary: ("V1", "8", "12"),
                lo: 4.8,
                hi: 5.2,
                hint: "入力が低いと、オペアンプとトランジスタの余裕 (入力より約 2V 低い電圧までしか出せません) が足りなくなります。基準電圧が入力で動いていないかも確かめます (R1 が小さすぎ・大きすぎないか)",
            },
            Check::Ratings { label: "部品が定格を超えない", load: Some("47"), set: NO },
        ],
    },
    Challenge {
        id: "regulator_78xx",
        lesson: "regulator_78xx",
        title: "三端子レギュレータ 7805 で 5V 電源",
        goal: "12V の電源から、三端子レギュレータ LM7805 で 5V (4.8〜5.2V) を作ります。入力と出力に、データシートどおりのコンデンサをつけます。負荷が 100Ω でも 22Ω でも、入力が 8V から 15V に変わっても出力が保たれ、部品が定格を超えない (いちばん厳しいのは 12V で負荷 22Ω のとき) ようにします。出力のネットには、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → 7805 の IN (1 番ピン)。7805 の GND (2 番ピン) は GND へ。OUT (3 番ピン) が出力 (VOUT)。IN と GND の間、OUT と GND の間に、コンデンサをつけます。",
            "コンデンサは、データシートの推奨どおり、入力側に 0.33µF (330n)、出力側に 0.1µF (100n) です。電源の電圧は 12V で、7805 は入力が約 7V 以上あれば 5V を出します。",
            "答えの目安: LM7805、330n、100n。「答えの回路を見る」で、できあがりを見られます。コンデンサは、判定ではシミュレーションに影響しませんが、実物では発振を防ぐために必要です。個数 (2 個以上) を調べます。",
        ],
        solution: include_str!("../../../../docs/examples/regulator_78xx.schjson"),
        drop: &["R1"],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("C1", (14, 4)), ("U1", (30, -4)), ("C2", (46, 4))],
        parts1: &[("V1", (0, 0))],
        start_values: NO,
        grounds0: &[("V1", "-", 0, 5), ("C1", "2", 0, 5), ("U1", "2", 0, 5), ("C2", "2", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "負荷 100Ω で、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("100"),
                set: NO,
                lo: 4.8,
                hi: 5.2,
                explain: &[
                    (-1e9, 0.5, "出力が {v} V しかありません。7805 のつなぎ方 (1 = IN、2 = GND、3 = OUT) を確かめてください。IN は電源の +、GND は GND です"),
                    (0.5, 4.8, "出力が {v} V で、低すぎます。7805 の入力が足りないのかもしれません。つなぎ方を確かめてください"),
                    (5.2, 1e9, "出力が {v} V で、高すぎます。レギュレータが LM7805 (5V) になっていますか"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "負荷 22Ω (約 230mA) でも、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("22"),
                set: NO,
                lo: 4.8,
                hi: 5.2,
                explain: &[(-1e9, 4.8, "重い負荷では、出力が {v} V に落ちます。つなぎ方を確かめてください")],
                gate: false,
            },
            Check::Stay {
                label: "入力が 8〜15V に変わっても、出力が保たれる",
                net: "VOUT",
                load: Some("100"),
                vary: ("V1", "8", "15"),
                lo: 4.8,
                hi: 5.2,
                hint: "7805 は、入力が約 7V より低いと安定化できません (ドロップアウト)。入力の途中にダイオードなどを入れて、電圧を下げていませんか",
            },
            Check::Has { label: "コンデンサが 2 個以上ある (入力側と出力側)", kind: Kind::Capacitor, min: 2, hint: "7805 の入力と GND の間 (330n)、出力と GND の間 (100n) に、コンデンサをつけます" },
            Check::Ratings { label: "部品が定格を超えない", load: Some("22"), set: &[("V1", "12")] },
        ],
    },
    Challenge {
        id: "draw_psu",
        lesson: "protection",
        title: "安定化電源を自分で設計する (最終課題)",
        goal: "12V の電池やACアダプタ (9〜12V でも動くこと) から、5V (4.8〜5.2V) を取り出す電源の回路を、自分で設計します。使える部品: 三端子レギュレータ LM7805、ショットキーダイオード 1N5819、コンデンサ。条件: (1) 負荷 100Ω でも、重い 22Ω (約 230mA) でも、出力が 4.8〜5.2V。(2) 入力が 9V でも 12V でも、5V が保たれる。(3) 電源を逆向きにつないでも、出力に逆向きの電圧が出ない (−12V にしても出力が 0V 付近)。(4) レギュレータの入力と出力に、コンデンサをつける。(5) どの部品も、定格を超えない (いちばん厳しいのは 12V・負荷 22Ω のとき)。出力のネットには、ラベルツールで「VOUT」という名前を付けます",
        hints: &[
            "回路の形: 電源の + → ダイオード → レギュレータの IN。レギュレータの GND は GND へ。OUT が出力 (VOUT)。レギュレータの IN と GND の間、OUT と GND の間に、コンデンサをつけます。",
            "ダイオードは、逆向きの電流を止めるために、電源とレギュレータの間に直列に入れます。向きは、アノードが電源側、カソード (縞のある側) がレギュレータ側です。ダイオードの電圧降下 (約 0.3V) があっても、入力が 9V なら、7805 のドロップアウト (約 2V) に足ります (9 − 0.3 − 5 = 3.7V の余裕)。",
            "コンデンサは、データシートのとおり、入力側に 0.33µF (330n)、出力側に 0.1µF (100n) です。7805 のピンは、1 = IN、2 = GND、3 = OUT です。「学ぶ → 値の決め方」の「3端子レギュレータ (78xx) まわり」と「熱 (温度と放熱板)」で、確かめられます。",
            "答えの目安: 1N5819 (ダイオード)、LM7805、330n と 100n。「答えの回路を見る」で、できあがりを見られます。実際の回路では、さらに、電源の入口にヒューズを入れます (レッスン 21)。ヒューズは、シミュレーションで判定できないので、「値の決め方」の「ヒューズの選び方」で、定格を決めてみましょう。",
        ],
        solution: include_str!("../../../../docs/examples/power_supply.schjson"),
        drop: &[],
        values: NO,
        labels: &[],
        rename: &[],
        parts0: &[("V1", (0, 0)), ("D1", (14, -8)), ("C1", (28, 4)), ("U1", (42, -4)), ("C2", (58, 4))],
        parts1: &[("V1", (0, 0))],
        start_values: NO,
        grounds0: &[("V1", "-", 0, 5), ("C1", "2", 0, 5), ("U1", "2", 0, 5), ("C2", "2", 0, 5)],
        grounds1: &[("V1", "-", 0, 5)],
        checks: &[
            Check::Erc,
            Check::Named { net: "VOUT" },
            Check::Voltage {
                label: "12V・負荷 100Ω で、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("100"),
                set: &[("V1", "12")],
                lo: 4.8,
                hi: 5.2,
                explain: &[
                    (-1e9, 0.5, "出力が {v} V しかありません。電源 → ダイオード → レギュレータの IN と、レギュレータの GND・OUT のつなぎ方 (7805 は 1 = IN、2 = GND、3 = OUT) を確かめてください。ダイオードの向き (アノードが電源側) も確かめます"),
                    (0.5, 4.8, "出力が {v} V で、低すぎます。レギュレータの入力が足りないのかもしれません。つなぎ方を確かめてください"),
                    (5.2, 1e9, "出力が {v} V で、高すぎます。レギュレータが、7805 (5V) になっていますか"),
                ],
                gate: true,
            },
            Check::Voltage {
                label: "負荷 22Ω (約 230mA) でも、出力が 4.8〜5.2V",
                net: "VOUT",
                load: Some("22"),
                set: &[("V1", "12")],
                lo: 4.8,
                hi: 5.2,
                explain: &[(-1e9, 4.8, "重い負荷では、出力が {v} V に落ちます。この電流を流せる部品か (7805 は 1A まで)、ダイオードが細すぎないか確かめてください")],
                gate: false,
            },
            Check::Stay {
                label: "入力が 9〜12V に変わっても、出力が保たれる",
                net: "VOUT",
                load: Some("100"),
                vary: ("V1", "9", "12"),
                lo: 4.8,
                hi: 5.2,
                hint: "入力が低すぎて、レギュレータのドロップアウト (約 2V の余裕) を満たしていないかもしれません。ダイオードを何本も直列にしていませんか",
            },
            Check::Voltage {
                label: "電源を逆向きにつないでも (−12V)、出力に逆向きの電圧が出ない",
                net: "VOUT",
                load: Some("100"),
                set: &[("V1", "-12")],
                lo: -0.3,
                hi: 0.3,
                explain: &[(-1e9, -0.3, "電源を逆にすると、出力に {v} V が出て、つないだ回路を壊します。電源とレギュレータのあいだに、逆向きの電流を止めるダイオードを入れます (アノードが電源側、カソードがレギュレータ側)")],
                gate: false,
            },
            Check::Has { label: "コンデンサが 2 個以上ある (入力側と出力側)", kind: Kind::Capacitor, min: 2, hint: "レギュレータの入力と GND の間、出力と GND の間に、コンデンサをつけます (発振を防ぎ、負荷が変わったときの電圧の乱れを抑えます)" },
            Check::Ratings { label: "部品が定格を超えない", load: Some("22"), set: &[("V1", "12")] },
        ],
    },
];

fn challenge(id: &str) -> Option<&'static Challenge> {
    CHALLENGES.iter().find(|c| c.id == id)
}

/// Whether a challenge of this id exists (items of a learning path that are only a challenge).
#[cfg(test)]
pub fn challenge_exists(id: &str) -> bool {
    challenge(id).is_some()
}

/// The id of the challenge of a lesson, if it has one.
pub fn challenge_of_lesson(lesson: &str) -> Option<&'static str> {
    CHALLENGES.iter().find(|c| c.lesson == lesson).map(|c| c.id)
}

/// The key under which a passed challenge is recorded (see `learn::Progress`).
pub fn progress_key(id: &str) -> String {
    format!("{id}+build")
}

fn set_value(s: &mut Schematic, refdes: &str, value: &str) {
    if let Some(c) = s.components.iter_mut().find(|c| c.refdes == refdes) {
        c.value = value.into();
    }
}

/// The sample turned into a passing answer: parts dropped, values set, output labels added.
fn solution_of(ch: &Challenge) -> Schematic {
    let mut s: Schematic = serde_json::from_str(ch.solution).unwrap_or_default();
    for r in ch.drop {
        if let Some(c) = s.components.iter().find(|c| c.refdes == *r) {
            let pins: Vec<Pt> = c.pins().into_iter().map(|(_, p)| p).collect();
            s.labels.retain(|l| !pins.contains(&l.pos));
            s.wires.retain(|w| !pins.contains(&w.a) && !pins.contains(&w.b));
        }
        s.components.retain(|c| c.refdes != *r);
    }
    for (r, v) in ch.values {
        set_value(&mut s, r, v);
    }
    for l in &mut s.labels {
        if let Some((_, to)) = ch.rename.iter().find(|(from, _)| *from == l.text) {
            l.text = (*to).into();
        }
    }
    for (r, pin, net) in ch.labels {
        if let Some((_, p)) = s.components.iter().find(|c| c.refdes == *r).and_then(|c| c.pins().into_iter().find(|(n, _)| n == pin)) {
            s.labels.push(Label { pos: p, text: (*net).into() });
        }
    }
    s
}

/// The starting sheet of a level: the parts of the level with no wire, and ground symbols.
fn starting_sheet(ch: &Challenge, level: u8) -> Schematic {
    let mut full = solution_of(ch);
    full.wires.clear();
    full.labels.clear();
    let keep = if level == 0 { ch.parts0 } else { ch.parts1 };
    full.components.retain(|c| keep.iter().any(|(r, _)| *r == c.refdes));
    for c in &mut full.components {
        if let Some((_, pos)) = keep.iter().find(|(r, _)| *r == c.refdes) {
            c.pos = *pos;
        }
    }
    for (r, v) in ch.start_values {
        set_value(&mut full, r, v);
    }
    for (r, pin, dx, dy) in if level == 0 { ch.grounds0 } else { ch.grounds1 } {
        let at = full.components.iter().find(|c| c.refdes == *r).and_then(|c| c.pins().into_iter().find(|(n, _)| n == pin)).map(|(_, p)| (p.0 + dx, p.1 + dy));
        if let Some(p) = at {
            full.add(Kind::Ground, p, 0);
        }
    }
    full
}

/// A copy with `set` applied and, if `load` is given, a load resistor (the checker's own, named RTEST) from VOUT to GND.
fn prepared(s: &Schematic, load: Option<&str>, set: &[(&str, &str)]) -> Schematic {
    let mut t = s.clone();
    for (r, v) in set {
        // the first voltage source is meant when the part number is not on the sheet (a learner may renumber)
        if t.components.iter().any(|c| c.refdes == *r) {
            set_value(&mut t, r, v);
        }
    }
    if let Some(ohms) = load {
        let i = t.add(Kind::Resistor, (400, 400), 0);
        t.components[i].refdes = "RTEST".into();
        t.components[i].value = ohms.into();
        let pins = t.components[i].pins();
        for (name, net) in [("1", "VOUT"), ("2", "GND")] {
            if let Some((_, p)) = pins.iter().find(|(n, _)| n == name) {
                t.labels.push(Label { pos: *p, text: net.into() });
            }
        }
    }
    t
}

/// The DC solution of `s`, or why there is none (with the simulator's advice).
fn solve(s: &Schematic) -> Result<sim_core::DcResult, String> {
    let c = sim_core::from_schematic(s).map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "))?;
    sim_core::dc_op(&c).map_err(|e| {
        let mut m = e.to_string();
        for h in sim_core::diagnose(&c, &e) {
            m.push_str(&format!(" ヒント: {h}"));
        }
        m
    })
}

/// The advice for a measured value: the first range of `explain` that contains it, with {v} / {mA} filled in.
fn advice(explain: &[(f64, f64, &str)], v: f64) -> String {
    explain
        .iter()
        .find(|(lo, hi, _)| v >= *lo && v < *hi)
        .map(|(_, _, t)| t.replace("{v}", &format!("{v:.2}")).replace("{mA}", &format!("{:.1}", v * 1e3)))
        .unwrap_or_default()
}

impl SchEditor {
    /// Starts challenge `id` at `level` on a fresh sheet (the running lesson is put away).
    pub fn start_challenge(&mut self, id: &str, level: u8) {
        let Some(ch) = challenge(id) else { return };
        self.new_file();
        self.sch = starting_sheet(ch, level);
        self.reset_history();
        self.fit_pending = true;
        self.lesson = None;
        self.panel_tab = PanelTab::Draw;
        self.challenge = Some(ActiveChallenge { id: ch.id, level, results: vec![], hints: 0, passed: false });
        self.status = format!("「{}」を始めました。回路を作って、「判定する」を押してください", ch.title);
    }

    /// The key of the challenge just passed, for the progress record.
    pub fn finished_challenge(&self) -> Option<String> {
        self.challenge.as_ref().filter(|c| c.passed).map(|c| progress_key(c.id))
    }

    /// "判定する": judges the sheet and keeps the result for the window.
    pub fn run_judge(&mut self) {
        let results = self.judge_challenge();
        let passed = !results.is_empty() && results.iter().all(|r| r.ok == Some(true));
        if let Some(a) = self.challenge.as_mut() {
            a.results = results;
            a.passed = passed;
        }
        self.status = if passed { "判定: すべて合格です".into() } else { "判定: 直すところがあります (✖ の項目を見てください)".into() };
    }

    /// Judges the circuit on the sheet by simulating it.
    pub(super) fn judge_challenge(&self) -> Vec<CheckResult> {
        let Some(ch) = self.challenge.as_ref().and_then(|a| challenge(a.id)) else { return vec![] };
        let mut out: Vec<CheckResult> = vec![];
        // set when a gating check fails: why the rest waits
        let mut waiting: Option<String> = None;
        for check in ch.checks {
            let label = match check {
                Check::Erc => "配線ミスの点検 (ERC) にエラーがない".to_string(),
                Check::Named { net } => format!("出力のネットに「{net}」のラベルがある"),
                Check::Voltage { label, .. } | Check::Stay { label, .. } | Check::Current { label, .. } | Check::Gain { label, .. } | Check::Has { label, .. } | Check::Ratings { label, .. } => label.to_string(),
            };
            if let Some(why) = &waiting {
                out.push(CheckResult { label, ok: None, detail: why.clone() });
                continue;
            }
            let (ok, detail, gate_wait): (bool, String, Option<String>) = match check {
                Check::Erc => {
                    let errors: Vec<String> = erc(&self.sch).into_iter().filter(|p| p.severity == Severity::Error).map(|p| p.message).collect();
                    (errors.is_empty(), errors.first().map(|m| format!("{m} (「作図」タブの ERC の一覧で、場所を確かめられます)")).unwrap_or_default(), None)
                }
                Check::Named { net } => {
                    let named = extract(&self.sch).iter().any(|n| n.name == *net && !n.pins.is_empty());
                    let detail = if named { String::new() } else { format!("出力 (部品の間の点) に、ラベルツールで「{net}」という名前を付けてください。判定は、このネットを測ります") };
                    (named, detail, Some(format!("「{net}」のラベルを付けてから判定します")))
                }
                Check::Voltage { net, load, set, lo, hi, explain, gate, .. } => match solve(&prepared(&self.sch, *load, set)) {
                    Ok(dc) => match dc.voltage(net) {
                        Some(v) => {
                            let ok = v >= *lo && v <= *hi;
                            let detail = if ok { format!("出力 {v:.2} V") } else { advice(explain, v) };
                            (ok, detail, gate.then(|| "前の項目を直してから判定します".to_string()))
                        }
                        None => (false, format!("{net} というネットが見つかりません"), gate.then(|| "前の項目を直してから判定します".to_string())),
                    },
                    Err(e) => (false, format!("回路を解けませんでした: {e}"), gate.then(|| "前の項目を直してから判定します".to_string())),
                },
                Check::Stay { net, load, vary: (src, a, b), lo, hi, hint, .. } => {
                    let at = |v: &str| solve(&prepared(&self.sch, *load, &[(src, v)])).ok().and_then(|d| d.voltage(net));
                    match (at(a), at(b)) {
                        (Some(x), Some(y)) => {
                            let ok = x >= *lo && y <= *hi && y >= *lo && x <= *hi;
                            (ok, if ok { format!("{a}V のとき {x:.2} V、{b}V のとき {y:.2} V") } else { format!("{a}V のとき {x:.2} V、{b}V のとき {y:.2} V と、動きすぎです。{hint}") }, None)
                        }
                        _ => (false, "入力を変えて解けませんでした".to_string(), None),
                    }
                }
                Check::Current { who, set, lo, hi, explain, .. } => {
                    let s = prepared(&self.sch, None, set);
                    let refdes = match who {
                        Who::Refdes(r) => Some(r.to_string()),
                        Who::Kind(k) => s.components.iter().find(|c| c.kind == *k).map(|c| c.refdes.clone()),
                    };
                    match refdes.and_then(|r| solve(&s).ok().and_then(|d| d.element(&r).map(|e| e.current.abs()))) {
                        Some(i) => {
                            let ok = i >= *lo && i <= *hi;
                            (ok, if ok { format!("電流 {:.1} mA", i * 1e3) } else { advice(explain, i) }, None)
                        }
                        None => (false, "電流を測れませんでした (その部品が回路にあるか、回路が解けるかを確かめてください)".to_string(), None),
                    }
                }
                Check::Gain { freq, lo, hi, explain, .. } => {
                    let gain = sim_core::from_schematic(&prepared(&self.sch, None, &[])).ok().and_then(|c| {
                        let st = sim_core::AcSettings { fstart: *freq, fstop: freq * 10.0, sweep: sim_core::AcSweep::Decade(1) };
                        let r = sim_core::ac_analysis(&c, &st).ok()?;
                        r.signal("V(VOUT)").and_then(|s| s.db().first().copied())
                    });
                    match gain {
                        Some(g) => {
                            let ok = g >= *lo && g <= *hi;
                            (ok, if ok { format!("{freq:.0}Hz で {g:.1} dB") } else { advice(explain, g) }, None)
                        }
                        None => (false, "周波数特性を計算できませんでした (入力の電源が「AC 1」か、VOUT のラベルがあるかを確かめてください)".to_string(), None),
                    }
                }
                Check::Has { kind, min, hint, .. } => {
                    let n = self.sch.components.iter().filter(|c| c.kind == *kind).count();
                    (n >= *min, if n >= *min { format!("{n} 個あります") } else { format!("{n} 個しかありません。{hint}") }, None)
                }
                Check::Ratings { load, set, .. } => {
                    let worst = self.with_ratings_of(&prepared(&self.sch, *load, set));
                    let over: Vec<String> = match solve(&worst) {
                        Ok(dc) => sim_core::check_ratings(&worst, &dc).into_iter().filter(|i| i.refdes != "RTEST" && i.level == sim_core::RatingLevel::Over).map(|i| i.message).collect(),
                        Err(_) => vec![],
                    };
                    let cond = load.map(|l| format!("負荷 {l}Ω のとき、")).unwrap_or_default();
                    (over.is_empty(), if over.is_empty() { format!("{cond}いちばん厳しい条件でも、定格の範囲です").replace("のとき、いちばん", "のいちばん") } else { format!("{cond}{}。抵抗を大きくする (または、定格の大きい部品にする) と直ります", over[0]) }, None)
                }
            };
            if !ok {
                waiting = gate_wait.clone();
            }
            out.push(CheckResult { label, ok: Some(ok), detail });
        }
        out
    }

    /// The sheet with the data sheet ratings of library parts filled in for parts that were placed without them.
    fn with_ratings_of(&self, s: &Schematic) -> Schematic {
        let mut t = s.clone();
        for c in &mut t.components {
            if let Some(sym) = c.custom.as_mut()
                && sym.ratings.is_none()
                && let Some(d) = self.library.iter().find(|d| d.name == sym.name) {
                    sym.ratings = d.ratings.clone().filter(|r| !r.is_empty());
                }
        }
        t
    }

    /// Floating window of the running challenge.
    pub fn challenge_window(&mut self, ctx: &egui::Context) {
        let Some(active) = self.challenge.as_ref() else { return };
        let Some(ch) = challenge(active.id) else { return };
        let (level, hints, passed) = (active.level, active.hints, active.passed);
        let mut judge = false;
        let mut new_level = None;
        let mut more_hint = false;
        let mut show_answer = false;
        let mut back = false;
        let mut close = false;
        egui::Window::new(format!("作ってみよう: {}", ch.title)).default_width(400.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(ch.goal);
            ui.horizontal(|ui| {
                ui.label("むずかしさ");
                if ui.selectable_label(level == 0, "配線だけ").on_hover_text("部品は置いてあります。線でつなぐ練習です (やり直すと、最初の状態に戻ります)").clicked() && level != 0 {
                    new_level = Some(0);
                }
                if ui.selectable_label(level == 1, "部品も置く").on_hover_text("電源だけが置いてあります。部品を選んで置き、配線し、値も決めます (やり直すと、最初の状態に戻ります)").clicked() && level != 1 {
                    new_level = Some(1);
                }
            });
            ui.separator();
            let results = self.challenge.as_ref().map(|c| &c.results[..]).unwrap_or(&[]);
            if results.is_empty() {
                ui.label("作り終えたら、「判定する」を押してください。回路を実際にシミュレーションして、次の項目を調べます");
            }
            for r in results.iter() {
                let (mark, color) = match r.ok {
                    Some(true) => ("✔", Color32::from_rgb(60, 160, 90)),
                    Some(false) => ("✖", Color32::from_rgb(230, 90, 70)),
                    None => ("○", Color32::from_gray(140)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(&r.label).strong());
                });
                if !r.detail.is_empty() {
                    ui.indent(("challenge_detail", &r.label), |ui| {
                        ui.label(egui::RichText::new(&r.detail).color(if r.ok == Some(false) { Color32::from_rgb(230, 150, 120) } else { Color32::from_gray(170) }));
                    });
                }
            }
            if passed {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "できました!");
                ui.label("自分で作った回路が、判定のすべての条件を満たしています。値や部品を変えても通るか (答えは 1 つではありません)、条件のぎりぎりを探してみましょう");
            }
            for (k, h) in ch.hints.iter().enumerate().take(hints) {
                ui.separator();
                ui.label(egui::RichText::new(format!("ヒント {}", k + 1)).strong());
                ui.label(*h);
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("判定する").on_hover_text("いまの回路をシミュレーションして、目標を満たしているか調べます。何度でも押せます").clicked() {
                    judge = true;
                }
                if ui.add_enabled(hints < ch.hints.len(), egui::Button::new(format!("ヒントを見る ({hints} / {})", ch.hints.len()))).on_hover_text("行き詰まったら、少しずつヒントを出します。最後のヒントは、答えの目安です").clicked() {
                    more_hint = true;
                }
                if ui.button("答えの回路を見る").on_hover_text("できあがりの回路を、いまの回路と入れ替えます (Ctrl+Z で戻せます)。答えは 1 つではありません。判定を通れば、別の値や形でも正解です").clicked() {
                    show_answer = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("レッスンに戻る").on_hover_text("このチャレンジをやめて、レッスンの手順に戻ります").clicked() {
                    back = true;
                }
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        });
        if let Some(l) = new_level {
            self.start_challenge(ch.id, l);
        } else if judge {
            self.run_judge();
        } else if more_hint {
            if let Some(a) = self.challenge.as_mut() {
                a.hints = (a.hints + 1).min(ch.hints.len());
            }
        } else if show_answer {
            // not through `load_sample`: the history stays, so Ctrl+Z brings the learner's own circuit back
            self.sch = solution_of(ch);
            self.fit_pending = true;
        } else if back {
            let lesson = ch.lesson;
            self.challenge = None;
            if let Some(i) = lesson_ids().iter().position(|l| *l == lesson) {
                self.start_lesson(i);
            }
        } else if close {
            self.challenge = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(s: Schematic, id: &str) -> SchEditor {
        let mut e = SchEditor::default();
        e.library = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).collect();
        e.start_challenge(id, 0);
        e.sch = s;
        e
    }

    fn verdict(id: &str, s: Schematic) -> Vec<(Option<bool>, String)> {
        editor(s, id).judge_challenge().into_iter().map(|r| (r.ok, r.detail)).collect()
    }

    fn solution(id: &str) -> Schematic {
        solution_of(challenge(id).unwrap())
    }

    fn set(s: &mut Schematic, refdes: &str, v: &str) {
        set_value(s, refdes, v);
    }

    /// Every challenge: its answer passes every check, its start sheets are wire-free and judged without a crash.
    #[test]
    fn every_answer_passes_and_every_start_sheet_is_bare() {
        let ids = lesson_ids();
        for ch in CHALLENGES {
            assert!(ids.contains(&ch.lesson), "{}: lesson {} exists", ch.id, ch.lesson);
            assert!(ch.hints.len() >= 3 && ch.hints.iter().all(|h| !h.is_empty()), "{}", ch.id);
            let r = verdict(ch.id, solution(ch.id));
            assert_eq!(r.len(), ch.checks.len(), "{}", ch.id);
            assert!(r.iter().all(|(ok, _)| *ok == Some(true)), "{}: {r:#?}", ch.id);
            for level in [0, 1] {
                let mut e = SchEditor::default();
                e.start_challenge(ch.id, level);
                assert!(e.sch.wires.is_empty() && e.sch.labels.is_empty(), "{} level {level}", ch.id);
                assert!(e.sch.components.iter().any(|c| c.kind.is_power_symbol()), "{}: a ground to start from", ch.id);
                let results = e.judge_challenge();
                assert_eq!(results.len(), ch.checks.len());
                assert!(results.iter().any(|x| x.ok == Some(false)), "{} level {level}: nothing is built yet", ch.id);
                assert_eq!(e.finished_challenge(), None);
            }
            // level 0 has more parts than level 1
            assert!(ch.parts0.len() > ch.parts1.len(), "{}", ch.id);
        }
    }

    #[test]
    fn the_zener_challenge_tells_hot_starved_and_missing_apart() {
        let mut s = solution("zener");
        set(&mut s, "R1", "100");
        s.components.iter_mut().find(|c| c.refdes == "R1").unwrap().footprint = "R_0805".into();
        let r = verdict("zener", s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[5].0, Some(false), "a hot resistor: {r:#?}");
        assert!(r[5].1.contains("R1"), "{}", r[5].1);
        let mut s = solution("zener");
        set(&mut s, "R1", "2k");
        let r = verdict("zener", s);
        assert!(r[2].1.contains("低すぎ") && r[3].0.is_none() && r[5].0.is_none(), "{r:#?}");
        let mut s = solution("zener");
        set(&mut s, "R1", "390");
        let r = verdict("zener", s);
        assert_eq!((r[2].0, r[3].0), (Some(true), Some(false)), "{r:#?}");
        assert!(r[3].1.contains("330Ω") && r[3].1.contains("目安"));
        let mut s = solution("zener");
        s.labels.retain(|l| l.text != "VOUT");
        let r = verdict("zener", s);
        assert!(r[1].0 == Some(false) && r[2..].iter().all(|(ok, _)| ok.is_none()), "{r:#?}");
        let mut s = solution("zener");
        s.components.retain(|c| c.refdes != "D1");
        assert_eq!(verdict("zener", s)[2].0, Some(false));
    }

    #[test]
    fn the_divider_challenge_notices_loading_and_waste() {
        // 51k / 100k: right with no load, but a 100k load pulls it down
        let mut s = solution("divider");
        set(&mut s, "R1", "51k");
        set(&mut s, "R2", "100k");
        let r = verdict("divider", s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[3].0, Some(false), "{r:#?}");
        assert!(r[3].1.contains("並列"), "{}", r[3].1);
        // 100 / 200 ohm: correct ratio, but 5 V across 300 ohm wastes 17 mA
        let mut s = solution("divider");
        set(&mut s, "R1", "100");
        set(&mut s, "R2", "200");
        let r = verdict("divider", s);
        assert_eq!((r[2].0, r[4].0), (Some(true), Some(false)), "{r:#?}");
        assert!(r[4].1.contains("むだ"), "{}", r[4].1);
        // wrong ratio: told which way to go
        let mut s = solution("divider");
        set(&mut s, "R2", "1k");
        let r = verdict("divider", s);
        assert!(r[2].1.contains("低すぎ"), "{r:#?}");
    }

    #[test]
    fn the_led_challenge_reads_the_leds_current() {
        let mut s = solution("led");
        set(&mut s, "R1", "2k");
        assert!(verdict("led", s)[1].1.contains("少なすぎ"));
        let mut s = solution("led");
        set(&mut s, "R1", "100k");
        assert!(verdict("led", s)[1].1.contains("流れていません"));
        let mut s = solution("led");
        set(&mut s, "R1", "50");
        let r = verdict("led", s);
        assert!(r[1].1.contains("多すぎ"), "{r:#?}");
        assert_eq!(r[2].0, Some(false), "55 mA in a LED is past its rating: {r:#?}");
        let mut s = solution("led");
        s.components.retain(|c| c.kind != Kind::Led);
        assert_eq!(verdict("led", s)[1].0, Some(false));
    }

    #[test]
    fn the_filter_challenge_measures_the_gain_at_three_frequencies() {
        // R x C ten times too large: the corner is at 100 Hz
        let mut s = solution("rc_lowpass");
        set(&mut s, "C1", "1.59u");
        let r = verdict("rc_lowpass", s);
        assert!(r[2].1.contains("下がりすぎ") && r[3].0 == Some(false), "{r:#?}");
        // R x C ten times too small: the corner is at 10 kHz
        let mut s = solution("rc_lowpass");
        set(&mut s, "C1", "15.9n");
        let r = verdict("rc_lowpass", s);
        assert!(r[2].1.contains("あまり下がっていません") && r[4].0 == Some(false), "{r:#?}");
        // another R / C pair with the same corner passes (the answer is not unique)
        let mut s = solution("rc_lowpass");
        set(&mut s, "R1", "10k");
        set(&mut s, "C1", "15.9n");
        assert!(verdict("rc_lowpass", s).iter().all(|(ok, _)| *ok == Some(true)));
    }

    #[test]
    fn the_follower_and_the_inverting_amplifier_challenges_diagnose_their_mistakes() {
        // follower: a huge base resistor starves the zener, so the output cannot be held
        let mut s = solution("emitter_follower");
        set(&mut s, "R1", "100k");
        let r = verdict("emitter_follower", s);
        assert!(r.iter().any(|(ok, _)| *ok == Some(false)), "{r:#?}");
        // without the transistor nothing is left on the output net: the judge asks for the VOUT net first
        let mut s = solution("emitter_follower");
        s.components.retain(|c| c.refdes != "Q1");
        let r = verdict("emitter_follower", s);
        assert_eq!(r[1].0, Some(false), "{r:#?}");
        // inverting amplifier: R2 = R1 gives -1, a non-inverting hook-up gives a positive output
        let mut s = solution("opamp_inv");
        set(&mut s, "R2", "1k");
        let r = verdict("opamp_inv", s);
        assert!(r[2].1.contains("ゲイン"), "{r:#?}");
        let mut s = solution("opamp_inv");
        set(&mut s, "R2", "100k");
        assert_eq!(verdict("opamp_inv", s)[2].0, Some(false));
    }

    /// The final challenge: the answer has a protection diode and two capacitors; without them the judge says which check fails.
    #[test]
    fn the_final_challenge_asks_for_reverse_protection_and_capacitors() {
        // no diode: the supply goes straight to the regulator, a backwards supply shows on the output
        let mut s = solution("draw_psu");
        for l in &mut s.labels {
            if l.text == "VPROT" {
                l.text = "VIN".into();
            }
        }
        s.components.retain(|c| c.refdes != "D1");
        let r = verdict("draw_psu", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![5], "only the reverse-connection check fails: {r:#?}");
        assert!(r[5].1.contains("ダイオード"), "{r:#?}");
        // no capacitors: only the capacitor check fails
        let mut s = solution("draw_psu");
        s.components.retain(|c| c.kind != Kind::Capacitor);
        let r = verdict("draw_psu", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![6], "{r:#?}");
        // the final challenge opens from the last lesson and from the learning path by its own id
        assert_eq!(challenge_of_lesson("protection"), Some("draw_psu"));
        assert!(challenge_exists("draw_psu"));
    }

    #[test]
    fn the_series_regulator_and_78xx_challenges_diagnose_their_mistakes() {
        // series regulator: the feedback divider missing its gain (R3 tiny) leaves the output near the reference
        let mut s = solution("series_regulator");
        set(&mut s, "R3", "100");
        let r = verdict("series_regulator", s);
        assert_eq!(r[2].0, Some(false), "{r:#?}");
        assert!(r[2].1.contains("R3"), "{r:#?}");
        // too much gain: the output is too high
        let mut s = solution("series_regulator");
        set(&mut s, "R3", "22k");
        assert_eq!(verdict("series_regulator", s)[2].0, Some(false));
        // 78xx: no capacitors, only that check fails
        let mut s = solution("regulator_78xx");
        s.components.retain(|c| c.kind != Kind::Capacitor);
        let r = verdict("regulator_78xx", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![5], "{r:#?}");
        assert_eq!(challenge_of_lesson("series_regulator"), Some("series_regulator"));
        assert_eq!(challenge_of_lesson("regulator_78xx"), Some("regulator_78xx"));
    }

    #[test]
    fn passing_is_recorded_under_the_challenges_key() {
        let mut e = editor(solution("zener"), "zener");
        e.run_judge();
        assert_eq!(e.finished_challenge().as_deref(), Some("zener+build"));
        assert_eq!(challenge_of_lesson("zener"), Some("zener"));
        assert_eq!(challenge_of_lesson("opamp_inv"), Some("opamp_inv"));
        assert_eq!(challenge_of_lesson("buck"), None);
        // level 0 keeps the circuit's parts, level 1 only the sources
        let names = |e: &SchEditor| e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count();
        e.start_challenge("zener", 0);
        assert_eq!(names(&e), 3);
        e.start_challenge("zener", 1);
        assert_eq!(names(&e), 1);
        e.start_challenge("opamp_inv", 1);
        assert_eq!(names(&e), 3, "the three sources: signal, +5 V and -5 V");
    }
}
