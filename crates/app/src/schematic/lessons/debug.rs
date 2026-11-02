//! Lesson 51: a circuit that does not work. The Zener regulator of lesson 16 is started with one fault put into it on purpose
//! (a value that is off by a decade, a supply that is too low, a wire that is cut); the learner reads the voltages, finds the
//! place and mends it. The fault differs from run to run (the seed is the clock).

use super::conditions::*;
use super::*;

const CIRCUIT: &str = include_str!("../../../../../docs/examples/zener_regulator.schjson");

/// The output of the regulator is good between these voltages (the Zener is 5.1 V).
const GOOD: (f64, f64) = (4.8, 5.5);

/// What can go wrong, and where to look first.
struct Fault {
    /// What it was, said after the learner has mended it.
    cause: &'static str,
    /// The first hint, for the learner who is stuck: the kind of place, not the place.
    area: &'static str,
    apply: fn(&mut Schematic),
}

fn set(s: &mut Schematic, refdes: &str, value: &str) {
    if let Some(c) = s.components.iter_mut().find(|c| c.refdes == refdes) {
        c.value = value.into();
    }
}

fn cut(s: &mut Schematic, a: (i32, i32), b: (i32, i32)) {
    s.wires.retain(|w| !(w.a == a && w.b == b));
}

const FAULTS: &[Fault] = &[
    Fault { cause: "R1 が 150Ω ではなく 15kΩ でした (桁の間違い)。抵抗が大きすぎて、ツェナーに電流が流れませんでした", area: "部品の値の間違いです。電源から出力へ向かって、電圧がどこで大きく下がるかを見ます", apply: |s| set(s, "R1", "15k") },
    Fault { cause: "負荷 R2 が 1kΩ ではなく 5Ω でした。負荷が重すぎて、ツェナーに電流が残りませんでした", area: "部品の値の間違いです。各部品を流れる電流を見て、不自然に大きい所を探します", apply: |s| set(s, "R2", "5") },
    Fault { cause: "電源 V1 が 9V ではなく 3V でした。入力がツェナー電圧 (5.1V) に届かず、ツェナーが働きませんでした", area: "電源の値の間違いです。入力 VIN の電圧を見てみましょう", apply: |s| set(s, "V1", "3") },
    Fault { cause: "R1 と出力 VOUT の間の配線が切れていました。出力に電流が届きませんでした", area: "配線の切れです。ERC を実行すると、つながっていないピンが見つかります", apply: |s| cut(s, (16, -8), (19, -8)) },
    Fault { cause: "ツェナー D1 と GND の間の配線が切れていました。ツェナーが働かず、出力が抵抗の分圧になりました", area: "配線の切れです。ERC を実行すると、つながっていないピンが見つかります", apply: |s| cut(s, (28, -2), (28, -5)) },
];

fn fresh_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

impl SchEditor {
    /// Puts fault number `seed % FAULTS.len()` into the lesson circuit (one fresh copy of it, so it can be called again).
    pub(in crate::schematic) fn debug_inject(&mut self, seed: u64) {
        let Ok(mut sch) = serde_json::from_str::<Schematic>(CIRCUIT) else { return };
        let fault = &FAULTS[(seed % FAULTS.len() as u64) as usize];
        (fault.apply)(&mut sch);
        self.sch = sch;
        self.reset_history();
        self.lesson_base = self.sch.clone();
        self.debug_fault = Some(seed % FAULTS.len() as u64);
    }

    /// What was wrong, once the learner has mended the circuit (for the lesson window).
    pub(in crate::schematic) fn debug_cause(&self) -> Option<&'static str> {
        self.debug_fault.map(|k| FAULTS[k as usize].cause)
    }
}

/// The output voltage of the fresh DC result.
fn vout(e: &SchEditor) -> Option<f64> {
    e.dc.as_ref()?.outcome.as_ref().ok()?.voltage("VOUT")
}

/// The circuit works: a fresh DC result with the output where the Zener holds it.
fn mended(e: &SchEditor) -> bool {
    if !dc_fresh(e) {
        return false;
    }
    let ok = vout(e).is_some_and(|v| (GOOD.0..=GOOD.1).contains(&v));
    if !ok {
        let hint = e.debug_fault.map_or("", |k| FAULTS[k as usize].area);
        let v = vout(e).map_or("測れません".to_string(), |v| format!("{v:.2}V"));
        e.lesson_unmet.replace(Some(Unmet::Note(format!("VOUT がまだ {v} です (5.1V 前後が正しい)。{hint}"))));
    }
    ok
}

/// Mended, and the supply raised to 11 V or more: the output stays where it is (the point of a regulator).
fn mended_at_high_supply(e: &SchEditor) -> bool {
    value_at_least(e, "V1", 11.0) && mended(e)
}

pub(super) const DEBUG: Lesson = Lesson {
    id: "debug_circuit",
    title: "51. 動かない回路を直す (デバッグ)",
    goal: "ツェナーの安定化電源に、故障が 1 つ仕込まれています。出力の電圧を手がかりに原因を探して直します。故障は、始めるたびに変わります",
    summary: "回路が動かないとき、闇雲に部品を替えても直りません。電源から出力へ向かって、電圧と電流を順に見て、「どこまでは正しく、どこから狂っているか」を探します。(1) 出力の電圧が低い・高い・0V のどれか。(2) 入力 (VIN) は正しいか。(3) 抵抗の前後の電圧差から、流れている電流は正しいか。(4) ERC に、つながっていないピンが出ていないか。原因は、ほとんどが、値の桁の間違い (150Ω と 15kΩ)・電源の値・配線の切れ・つなぎ忘れのどれかです。見つけたら、1 つだけ直して、もう一度解析します。",
    limits: &[
        "仕込まれる故障は、この回路に合わせた 5 種類 (値の桁違い・負荷が重すぎる・電源が低い・配線の切れが 2 か所) だけです。部品の向きの間違いや、部品の違い (品番違い) は、仕込みません。",
        "「直った」の判定は、出力 VOUT が 4.8〜5.5V に入っていることです。ほかの場所に別の不具合があっても、出力が合えば直ったことになります。",
        "実物の回路では、故障の原因が 1 つとは限りません。部品の不良や、はんだ不良、テスタの当て方の間違いなど、シミュレーションにない原因があります。ここでは、仕込んだ 1 つだけが原因です。",
    ],
    real: &[],
    circuit: CIRCUIT,
    setup: |e| e.debug_inject(fresh_seed()),
    steps: &[
        Step {
            tab: PanelTab::Analyze,
            title: "DC動作点を解析して、出力の電圧を見る",
            detail: "「DC動作点を解析」を押します。正しい回路なら、出力 VOUT は 5.1V 前後です。いまの値は、それと違うはずです。VIN・VOUT の電圧と、R1・R2 の電流を見て、どこから狂っているか考えます。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "原因を見つけて直し、もう一度解析する",
            detail: "電源から出力へ、電圧を順に見ます。値が変なら部品を選んで左の「値」を直し、配線が切れていたら配線をつなぎ直します(「つながっていないピン」は ERC で探せます)。直したら、もう一度「DC動作点を解析」を押して、VOUT が 5.1V 前後になれば完了です。25 秒迷うと、探す場所のヒントが出ます。",
            done: mended,
        },
        Step {
            tab: PanelTab::Draw,
            title: "V1 を 12 にして、安定化を確かめる",
            detail: "直った回路で、V1 を 12 にして、もう一度「DC動作点を解析」。入力を 9V から 12V に上げても、VOUT はほとんど変わりません。これがツェナーによる安定化です。",
            done: mended_at_high_supply,
        },
    ],
};
