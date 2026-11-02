//! Questions that ask for a guess before the learner does a step ("R2 を 30k にすると、MID の電圧は?"). The right answer is not written
//! down: it is the option whose number is nearest to what the simulation gives when the step is done, so a lesson whose numbers change
//! cannot go on telling the old answer (the tests check that every question has an option close to the real number).

use super::*;

pub(in crate::schematic) struct Prediction {
    /// The lesson, and the step (index) the question is asked on.
    pub lesson: &'static str,
    pub step: usize,
    pub question: &'static str,
    /// What the learner reads, and the number it means (in `unit`).
    pub options: &'static [(&'static str, f64)],
    /// The number the question is about, from the analysis on the screen.
    pub measure: fn(&SchEditor) -> Option<f64>,
    pub unit: &'static str,
    /// Why it is so.
    pub explain: &'static str,
}

/// What the learner guessed and how it turned out (kept in the running lesson).
#[derive(Clone, Debug)]
pub(in crate::schematic) struct Verdict {
    pub right: bool,
    pub chosen: usize,
    pub actual: f64,
}

impl Prediction {
    /// The option that is nearest to `value`.
    pub(in crate::schematic) fn nearest(&self, value: f64) -> usize {
        self.options.iter().enumerate().min_by(|a, b| (a.1 .1 - value).abs().total_cmp(&(b.1 .1 - value).abs())).map_or(0, |(i, _)| i)
    }

    pub(in crate::schematic) fn verdict(&self, chosen: usize, actual: f64) -> Verdict {
        Verdict { right: self.nearest(actual) == chosen, chosen, actual }
    }

    /// The number as the verdict writes it ("3.75V").
    pub(in crate::schematic) fn show(&self, v: f64) -> String {
        format!("{:.2}{}", v, self.unit)
    }
}

pub(in crate::schematic) fn for_step(lesson: &str, step: usize) -> Option<&'static Prediction> {
    PREDICTIONS.iter().find(|p| p.lesson == lesson && p.step == step)
}

fn node_v(e: &SchEditor, node: &str) -> Option<f64> {
    e.dc.as_ref()?.outcome.as_ref().ok()?.voltage(node)
}

pub(in crate::schematic) const PREDICTIONS: &[Prediction] = &[
    Prediction {
        lesson: "divider",
        step: 1,
        question: "いま R1 = R2 = 10k で、MID は 2.5V です。R2 だけを 30k にすると、MID の電圧はどうなると思いますか?",
        options: &[("2.5V のまま", 2.5), ("約 3.75V に上がる", 3.75), ("約 1.25V に下がる", 1.25), ("5V になる", 5.0)],
        measure: |e| node_v(e, "MID"),
        unit: "V",
        explain: "出力は 電源 × R2 ÷ (R1 + R2) です。R2 を大きくすると、R2 にかかる電圧の割合が増えます (5 × 30k ÷ 40k = 3.75V)。R2 をどれだけ大きくしても、電源の 5V は超えません。",
    },
    Prediction {
        lesson: "led",
        step: 1,
        question: "R1 を 330Ω から 100Ω に小さくすると、LED に流れる電流 (いま約 9mA) はどうなると思いますか?",
        options: &[("9mA のまま", 9.0), ("約 30mA (3 倍)", 30.0), ("約 3mA (1/3)", 3.0), ("約 90mA (10 倍)", 90.0)],
        measure: |e| Some(e.dc.as_ref()?.outcome.as_ref().ok()?.element("D1")?.current.abs() * 1e3),
        unit: "mA",
        explain: "電流は (電源 − LED の電圧) ÷ R1 です。LED の電圧は 2V ほどで、あまり変わりません。R1 を 1/3.3 にすると、電流はほぼ 3 倍になります (10 倍にはなりません。LED の電圧が、電流とともに少し上がるためです)。LED は 20mA あたりが定格の目安です。",
    },
    Prediction {
        lesson: "zener",
        step: 1,
        question: "負荷 R2 を 1kΩ から 100Ω (重い負荷) にすると、出力 VOUT (いま約 5.2V) はどうなると思いますか?",
        options: &[("5.2V のまま (ツェナーが支える)", 5.2), ("約 3.6V に下がる", 3.6), ("約 4.5V に下がる", 4.5), ("約 9V に上がる", 9.0)],
        measure: |e| node_v(e, "VOUT"),
        unit: "V",
        explain: "R1 (150Ω) と R2 (100Ω) で分けると、9V × 100 ÷ 250 = 3.6V です。ツェナーの電圧 (約 5.1V) に届かないので、ツェナーに電流が流れず、安定化が働きません。ツェナーは、負荷に流す電流のあとに、まだ余りがあるときだけ、電圧を支えます。",
    },
    Prediction {
        lesson: "emitter_follower",
        step: 1,
        question: "負荷 R2 を 220Ω から 47Ω (必要な電流は約 5 倍) にすると、出力 VOUT (いま約 4.4V) はどうなると思いますか?",
        options: &[("ほとんど変わらない (約 4.4V)", 4.4), ("約 3.5V に下がる", 3.5), ("約 2.2V (半分) に下がる", 2.2), ("約 0.7V に下がる", 0.7)],
        measure: |e| node_v(e, "VOUT"),
        unit: "V",
        explain: "トランジスタが、ベースに流れる小さな電流を 100 倍ほどにして、負荷へ流してくれます。ツェナーの電圧 (約 5.1V) からベース-エミッタ間の 0.7V ほどを引いた電圧のまま、ほとんど変わりません。ツェナーに負荷を直接つないだ前の回路 (重い負荷で崩れる) との違いです。",
    },
    Prediction {
        lesson: "coin_cell",
        step: 1,
        question: "R1 を 100Ω から 10Ω にして、LED に流れる電流を増やすと、電池の端子電圧 VBAT (いま約 2.86V) はどうなると思いますか?",
        options: &[("2.86V のまま", 2.86), ("約 2.5V に下がる", 2.5), ("約 1.5V に下がる", 1.5), ("約 3.0V に上がる", 3.0)],
        measure: |e| node_v(e, "VBAT"),
        unit: "V",
        explain: "電池には内部抵抗があります。電流が増えると、内部抵抗での電圧降下が増えて、端子の電圧が下がります。理想の電圧源 (内部抵抗 0) なら、電流を増やしても電圧は変わりません。これを小さくするのが、安定化電源の役目です。",
    },
    Prediction {
        lesson: "diode_curve",
        step: 2,
        question: "R1 を 150Ω から 1.5kΩ にして、D1 の電流を約 1/10 にすると、D1 のアノードの電圧 A1 (いま約 0.72V) はどうなると思いますか?",
        options: &[("0.72V のまま", 0.72), ("約 0.63V (少し下がる)", 0.63), ("約 0.07V (1/10 に下がる)", 0.07), ("0V になる", 0.0)],
        measure: |e| node_v(e, "A1"),
        unit: "V",
        explain: "ダイオードの電圧は、電流の対数に比例します。電流が 10 分の 1 になっても、電圧は 0.1V ほどしか下がりません (電流が 10 倍変わるごとに、約 0.06〜0.1V)。だから、ダイオードは「電流が変わっても、電圧がほぼ一定の部品」として使えます。",
    },
];
