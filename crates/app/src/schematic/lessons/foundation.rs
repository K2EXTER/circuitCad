//! The lessons of the foundation path ("土台"): what a circuit is before any part. Two aim at the ideas learners hold most often and keep
//! after teaching (the current is used up on its way, the battery gives a fixed current, a change acts only near where it is made, an open
//! place has no voltage); two give the laws every course teaches next (Kirchhoff, Thevenin). Each step asks for a guess first (`predict.rs`).

use super::conditions::*;
use super::*;

/// The current through `refdes` in mA (its size: the sign only says which way).
pub(super) fn current_ma(e: &SchEditor, refdes: &str) -> Option<f64> {
    Some(e.dc.as_ref()?.outcome.as_ref().ok()?.element(refdes)?.current.abs() * 1e3)
}

fn node(e: &SchEditor, name: &str) -> Option<f64> {
    e.dc.as_ref()?.outcome.as_ref().ok()?.voltage(name)
}

pub(super) const CURRENT_KEPT: Lesson = Lesson {
    id: "basics_current",
    title: "91. はじめに 3: 電流は減らない、電池は電圧を保つ",
    goal: "電流は部品を通っても減らず、1 本の道のどこでも同じであること、1 か所の抵抗を変えると道全体の電流が変わること、電池は決まった電流でなく決まった電圧を出し、つなぐものが増えると出す電流が増えることを、予想してから確かめます",
    summary: "電気の回路でよくある思い込みが 3 つあります。1 つ目は「電流は部品を通るたびに使われて減る」。実際は、電流は 1 本の道 (直列) のどこでも同じです。部品で使われるのは電流ではなく、電圧 (電気を押す力の差) で、そのぶんが熱や光になります。2 つ目は「変えた部品のところだけが変わる」。直列の道では、1 か所が流れにくくなると、道全体の電流が減ります。前にある部品も、後ろにある部品も同じです。3 つ目は「電池は決まった電流を出す」。電池 (電圧源) が保つのは電圧で、電流はつないだものが決めます。別の道を並列に足すと、電池はその分だけ多くの電流を出し、もとの道の電流は変わりません。この回路は、電池 V1 (5V) から R1 → MID → R2 → GND の道と、R3 だけの別の道があります。画面の橙の数字が、部品を流れる電流です。",
    limits: &[
        "電池は内部抵抗のない理想の電圧源です。実物の電池は、出す電流が増えると電圧が少し下がります (レッスン「電池の内部抵抗」)。それでも「電池が保つのは電圧で、電流ではない」ことは同じです。",
        "電流の向きは、電池の + から出て − に戻る向きで描いています。電子の動きは逆向きですが、どこでも同じ量が流れることは変わりません。",
        "抵抗は値が一定の理想の部品です。実物の電球や LED は、電流によって抵抗のように見える値が変わります。それでも、直列の道の電流がどこでも同じことは変わりません。",
    ],
    real: &[
        "用意するもの: 電池 (単3 を 3 本で 4.5V か、5V の電源)、抵抗 1kΩ を 3 本と 3.3kΩ を 1 本、テスタ、ブレッドボード。",
        "電流計を R1 の前と、R1 と R2 の間と、R2 の後ろに順に入れて (回路を切って、その間に挟みます)、どこでも同じ値になることを確かめます。",
        "R3 の道をつないだり外したりしながら、電池の両端の電圧と、R1 の道の電流を測ります。電池の電圧はほとんど変わりません。",
        "安全: 電流計 (電流のレンジ) は必ず直列に入れます。電池に直接当てると、ショートしてヒューズが飛びます。",
    ],
    circuit: include_str!("../../../../../docs/examples/basics_current.schjson"),
    setup: |_| {},
    steps: &[
        Step {
            tab: PanelTab::Analyze,
            title: "DC動作点で、R1 と R2 の電流を見比べる",
            detail: "「DC動作点を解析」を押します。R1 と R2 のそばの橙の数字は、どちらも 2.500mA です。電流は R1 を通っても減っていません。R1 で減ったのは電圧で、IN の 5V が MID では 2.5V になっています (R1 の両端に 2.5V)。R3 の道には、別に 0.5mA が流れています。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "R1 を 3.3k にして、R2 の電流がどうなるか見る",
            detail: "R1 を選び、値を 3.3k にして「DC動作点を解析」。R2 は R1 の後ろにありますが、R2 の電流も R1 と同じに減ります (5V ÷ (3.3k + 1k) ≒ 1.16mA)。直列の道では、1 か所が流れにくくなると、道全体の流れが減ります。",
            done: current_r1_raised,
        },
        Step {
            tab: PanelTab::Draw,
            title: "R1 を 1k に戻し、R3 を 1k にして、R2 の電流と電池の電圧を見る",
            detail: "R1 を 1k に戻し、R3 を 10k から 1k にして「DC動作点を解析」。R3 の道は 5mA に増え、電池 V1 が出す電流は 2.5mA + 5mA = 7.5mA になります。それでも、IN は 5V のまま、R1 と R2 の電流は 2.5mA のままです。電池は電圧を保ち、つないだ道の数だけ電流を出します。決まった量の電流を分け合うのではありません。",
            done: current_r3_lowered,
        },
    ],
};

fn current_r1_raised(e: &SchEditor) -> bool {
    value_between(e, "R1", 3e3, 3.6e3) && dc_fresh(e)
}

fn current_r3_lowered(e: &SchEditor) -> bool {
    value_between(e, "R1", 900.0, 1.1e3) && value_at_most(e, "R3", 1.1e3) && dc_fresh(e)
}

pub(super) const OPEN_VOLTAGE: Lesson = Lesson {
    id: "basics_open",
    title: "92. はじめに 4: 切れた所にも電圧がある、遠くの変化も効く",
    goal: "電流が流れていない所 (切れた所) にも電圧がかかること、電流が流れないと抵抗の両端の電圧は 0 になること、離れた所に部品を足しても、手前の部品の電流と電圧が変わることを、予想してから確かめます",
    summary: "「電流が流れていなければ、電圧も無い」と思いがちです。電圧は 2 点の高さの差で、電流が流れていなくても差はあります。電球が切れた (抵抗がとても大きくなった) ところには、電池の電圧がまるごとかかります。手前の抵抗には電流が流れないので、その両端の電圧は 0 (I × R = 0) になり、切れた所の両端に、電池の 5V が全部かかります。家のコンセントも、何もつないでいなくても 100V あります。もう 1 つの思い込みは「部品を足した所の近くだけが変わる」です。電池から見ると、回路全体が 1 つの抵抗のようなものなので、どこかに道を足すと全体の抵抗が下がり、電池から出る電流が増えて、手前の R1 の電圧と電流も変わります。この回路は、V1 (5V) → R1 (1k) → MID → R2 (1k) → GND で、R3 は MID と GND の間に、まだつないでいない (1G = 10 億 Ω、ほとんど電流の流れない値) 部品です。",
    limits: &[
        "「切れた」状態を、とても大きな抵抗 (1G) で表しています。実物の切れた電球や、外したスイッチの抵抗はもっと大きく、テスタの電圧のレンジ (内部抵抗 10MΩ ほど) をつなぐと、テスタに少し電流が流れます。それでも、表示はほぼ電池の電圧になります。",
        "電池は内部抵抗のない理想の電圧源です。実物の電池では、R3 をつないで電流が増えると、電池の電圧が少し下がります。",
        "線は抵抗 0 の理想の導線です。実物の線の抵抗はとても小さく、この回路の電圧と電流はほとんど変わりません。",
    ],
    real: &[
        "用意するもの: 電池 (4.5V か 5V)、抵抗 1kΩ を 3 本、テスタ、ブレッドボード。",
        "R2 を抜いて (切れた所を作って)、MID と GND の間の電圧と、R1 の両端の電圧を、テスタの電圧のレンジで測ります。MID は電池とほぼ同じ電圧、R1 の両端はほぼ 0V です。",
        "R2 を戻し、MID と GND の間に R3 をつなぐ前と後で、R1 の両端の電圧を測ります。R3 は R1 から離れた所ですが、R1 の電圧は 2.5V から約 3.3V に上がります。",
        "安全: 5V 以下の電池で行います。テスタは、電圧のレンジで 2 点に当てます。",
    ],
    circuit: include_str!("../../../../../docs/examples/basics_open.schjson"),
    setup: |_| {},
    steps: &[
        Step {
            tab: PanelTab::Analyze,
            title: "DC動作点で、MID の電圧と R1 の電流を見る",
            detail: "「DC動作点を解析」を押します。R1 = R2 なので、MID は 5V の半分の 2.5V、R1 と R2 には 2.5mA が流れます。R3 はまだつないでいない (1G) ので、ほとんど流れていません。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "R2 を 1G にして (切れた電球)、MID の電圧を見る",
            detail: "R2 を選び、値を 1G (10 億 Ω。切れたのと同じ) にして「DC動作点を解析」。R1 にも R2 にも電流はほぼ流れません。それでも MID は 5V で、切れた R2 の両端に電池の電圧がまるごとかかります。R1 は電流が流れないので、両端の電圧が 0V (IN と MID が同じ 5V) です。",
            done: open_r2_broken,
        },
        Step {
            tab: PanelTab::Draw,
            title: "R2 を 1k に戻し、R3 を 1k にして、R1 の電流を見る",
            detail: "R2 を 1k に戻し、R3 を 1G から 1k にして (MID と GND の間に、もう 1 本の道を足して)「DC動作点を解析」。R3 は R1 から離れた所ですが、R1 の電流は 2.5mA から約 3.33mA に増え、MID は 2.5V から約 1.67V に下がります。MID から下の抵抗が 500Ω (1k と 1k の並列) になり、回路全体の抵抗が下がったからです。",
            done: open_r3_added,
        },
    ],
};

fn open_r2_broken(e: &SchEditor) -> bool {
    value_at_least(e, "R2", 1e8) && dc_fresh(e)
}

fn open_r3_added(e: &SchEditor) -> bool {
    value_between(e, "R2", 900.0, 1.1e3) && value_at_most(e, "R3", 1.1e3) && dc_fresh(e)
}

pub(super) const KIRCHHOFF: Lesson = Lesson {
    id: "kirchhoff",
    title: "93. キルヒホッフの法則 (電流の和・電圧の和)",
    goal: "分かれ道に入る電流と出る電流の和が等しいこと (電流則)、回路を 1 周したときの電圧の和が 0 になること (電圧則) を、DC動作点の数字で確かめ、1 本の道を変えたときの電流の分かれ方を予想します",
    summary: "キルヒホッフの法則は 2 つあります。電流則 (第 1 法則): 線が分かれる点 (節点) に入る電流の和は、出る電流の和に等しい。水道管の分かれ目で、水が消えたり湧いたりしないのと同じです。電圧則 (第 2 法則): 回路のどの 1 周の道でも、電圧の上がり (電池) と下がり (部品) の和は 0。山道を 1 周して出発点に戻れば、登った高さと下った高さが等しいのと同じです。この回路は、電池 V1 (9V) から R1 (1k) を通って A に来た電流が、R2 (1k) と R3 (2k) の 2 本に分かれて GND へ戻ります。A から下は R2 と R3 の並列で、1k と 2k の並列は 1k × 2k ÷ (1k + 2k) ≒ 667Ω です。全体は 1k + 667Ω ≒ 1.67kΩ なので、R1 の電流は 9V ÷ 1.67kΩ = 5.4mA、A の電圧は 5.4mA × 667Ω = 3.6V です。R2 には 3.6V ÷ 1k = 3.6mA、R3 には 3.6V ÷ 2k = 1.8mA が流れ、3.6 + 1.8 = 5.4mA で、電流則のとおりです。電圧則では、IN → A で R1 が 5.4V 下げ、A → GND で R2 (と R3) が 3.6V 下げ、合わせて 9V で電池の上がりと等しくなります。",
    limits: &[
        "線は抵抗 0 の理想の導線です。実物の線や基板の配線にもわずかな抵抗があり、大きな電流ではそこでも電圧が下がります。それでも、その小さな下がりを含めれば、電圧則はそのまま成り立ちます。",
        "直流の、落ち着いた状態の値です。コンデンサやコイルがあって時間とともに変わる回路でも、各瞬間に 2 つの法則は成り立ちます。",
        "実物の抵抗には許容差 (±5% など) があるので、測った電流の和や電圧の和は、計算と少しずれます。それでも、同じ回路で測った値どうしなら、和は測る誤差の範囲で合います。",
    ],
    real: &[
        "用意するもの: 9V 電池、抵抗 1kΩ を 2 本と 2kΩ (または 2.2kΩ) を 1 本、テスタ、ブレッドボード。",
        "電流則: 電流計を R1 の道、R2 の道、R3 の道に順に入れて、R1 の電流が R2 と R3 の和になることを確かめます。",
        "電圧則: R1 の両端と、A と GND の間の電圧を測り、足すと電池の電圧になることを確かめます。",
        "安全: 電流計は直列に入れます。9V 電池でも、端子をショートさせると電池が熱くなります。",
    ],
    circuit: include_str!("../../../../../docs/examples/kirchhoff.schjson"),
    setup: |_| {},
    steps: &[
        Step {
            tab: PanelTab::Analyze,
            title: "DC動作点で、分かれ道の電流を足してみる",
            detail: "「DC動作点を解析」を押します。R1 の電流 (5.4mA) が、A で R2 (3.6mA) と R3 (1.8mA) に分かれています。3.6 + 1.8 = 5.4 で、入る電流と出る電流が等しくなっています (電流則)。IN は 9V、A は 3.6V なので、R1 の両端は 5.4V。5.4V + 3.6V = 9V で、電池の電圧と等しくなっています (電圧則)。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "R3 を 1k にして、R1 の電流を見る",
            detail: "R3 を選び、値を 1k にして「DC動作点を解析」。A から下は 1k と 1k の並列で 500Ω になり、全体は 1.5kΩ、R1 の電流は 9V ÷ 1.5kΩ = 6mA に増えます。A は 3V になり、R2 と R3 に 3mA ずつ流れて、和は 6mA。どう変えても、分かれ道の電流の和と、1 周の電圧の和の決まりは守られます。",
            done: kirchhoff_r3_lowered,
        },
        Step {
            tab: PanelTab::Draw,
            title: "V1 を 12 にして、R1 と A の電圧の和を見る",
            detail: "V1 を選び、値を 12 にして「DC動作点を解析」。R1 の両端の電圧 (IN − A) と、A の電圧を足すと、ちょうど 12V です。電池の電圧を変えると、各部品の電圧はその割合のまま大きくなり、1 周の和は電池の電圧と等しいままです。",
            done: kirchhoff_v1_raised,
        },
    ],
};

fn kirchhoff_r3_lowered(e: &SchEditor) -> bool {
    value_between(e, "R3", 900.0, 1.1e3) && dc_fresh(e)
}

fn kirchhoff_v1_raised(e: &SchEditor) -> bool {
    value_between(e, "V1", 11.5, 12.5) && dc_fresh(e)
}

/// The voltage across R1 plus the voltage of A (the two drops around the loop through R1 and R2), in volts.
pub(super) fn kirchhoff_loop_sum(e: &SchEditor) -> Option<f64> {
    let (inn, a) = (node(e, "IN")?, node(e, "A")?);
    Some((inn - a) + a)
}

pub(super) const THEVENIN: Lesson = Lesson {
    id: "thevenin",
    title: "94. テブナンの定理 (電池 1 つと抵抗 1 本にまとめる)",
    goal: "電池と抵抗でできた回路は、外から見ると「電池 1 つと抵抗 1 本」と同じに働くこと (テブナンの等価回路) を確かめます。負荷を外したときの電圧 (開放電圧) と、負荷をショートしたときの電流 (短絡電流) から、まとめた電池の電圧と抵抗を求め、同じ負荷で同じ電圧になるかを比べます",
    summary: "テブナンの定理は、「電池と抵抗でできた回路は、2 つの端子の外から見ると、電池 1 つ (電圧 VTH) と抵抗 1 本 (RTH) を直列にしたものと区別できない」というものです。VTH は、端子に何もつながないときの電圧 (開放電圧)。RTH は、開放電圧 ÷ 端子をショートしたときの電流 (短絡電流) です。この回路の左側は、電池 V1 (10V) を R1 (1k) と R2 (1k) で分けた分圧回路で、A に負荷 RL (1k) がつながっています。右側は、まとめた形の練習用で、電池 VTH と抵抗 RTH と、同じ負荷 RL2 (1k) です。VTH と RTH の値は、まだ仮の値 (1V と 1Ω) です。手順で、開放電圧と短絡電流を測ってから、VTH と RTH に入れて、OUT2 が A と同じになるか確かめます。電池の内部抵抗 (レッスン「電池の内部抵抗」) も、テブナンの RTH の 1 つの例です。",
    limits: &[
        "等価回路が同じに働くのは、抵抗と電池 (と電流源) だけの回路で、負荷の側から見たときです。中で熱になる電力は、元の回路と等価回路とで違います。",
        "ダイオードやトランジスタを含む回路は、動作点の近くでだけ、同じようにまとめられます (小信号の等価回路)。",
        "ショートを 0.001Ω で表しています。実物でショートさせると、線やテスタの抵抗 (数 Ω まで) が入るので、短絡電流は計算より少し小さく測れます。",
    ],
    real: &[
        "用意するもの: 9V 電池か 10V の電源、抵抗 1kΩ を 3 本、テスタ、ブレッドボード。",
        "開放電圧: 負荷を外して、A と GND の間の電圧をテスタで測ります。",
        "短絡電流: テスタを電流のレンジ (200mA) にして、A と GND の間に当てます (テスタが負荷のショートの代わりになります)。R1 が電流を制限するので、10mA ほどで安全です。",
        "測った 2 つの値から RTH = 開放電圧 ÷ 短絡電流 を計算し、負荷 1kΩ をつないだときの A の電圧を予想してから、測って比べます。",
        "安全: 短絡電流は、抵抗が電流を制限している回路だけで測ります。電池や電源の端子に直接、電流のレンジを当ててはいけません。",
    ],
    circuit: include_str!("../../../../../docs/examples/thevenin.schjson"),
    setup: |_| {},
    steps: &[
        Step {
            tab: PanelTab::Analyze,
            title: "DC動作点で、負荷 RL をつないだ A の電圧を見る",
            detail: "「DC動作点を解析」を押します。A は約 3.33V です (A から下は R2 と RL の並列で 500Ω、10V × 500 ÷ (1000 + 500))。右側の OUT2 は、VTH と RTH が仮の値なので、まだ違う値です。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "RL を 1G にして (外して)、開放電圧を測る",
            detail: "RL を選び、値を 1G (外したのと同じ) にして「DC動作点を解析」。A は 5V になります。これが開放電圧で、VTH = 5V です。",
            done: thevenin_open,
        },
        Step {
            tab: PanelTab::Draw,
            title: "RL を 0.001 にして (ショートして)、短絡電流を測る",
            detail: "RL の値を 0.001 (ほぼ 0Ω。ショートと同じ) にして「DC動作点を解析」。RL に 10mA が流れます。これが短絡電流です。RTH = 開放電圧 ÷ 短絡電流 = 5V ÷ 10mA = 500Ω。左の回路を GND の側から見ると、R1 と R2 の並列 (1k と 1k で 500Ω) と同じです。",
            done: thevenin_short,
        },
        Step {
            tab: PanelTab::Draw,
            title: "VTH を 5、RTH を 500、RL を 1k にして、A と OUT2 を比べる",
            detail: "VTH の値を 5、RTH の値を 500 にし、RL を 1k に戻して「DC動作点を解析」。A と OUT2 は、どちらも約 3.33V で同じになります。負荷を変えても (RL と RL2 を同じ値に変えて試してみてください)、2 つはいつも同じです。外から見れば、左の回路は「5V の電池と 500Ω の抵抗」にまとめられます。",
            done: thevenin_matched,
        },
    ],
};

fn thevenin_open(e: &SchEditor) -> bool {
    value_at_least(e, "RL", 1e8) && dc_fresh(e)
}

fn thevenin_short(e: &SchEditor) -> bool {
    value_at_most(e, "RL", 0.01) && dc_fresh(e)
}

fn thevenin_matched(e: &SchEditor) -> bool {
    if !(value_between(e, "RL", 900.0, 1.1e3) && value_between(e, "VTH", 4.8, 5.2) && value_between(e, "RTH", 450.0, 550.0) && dc_fresh(e)) {
        return false;
    }
    let same = matches!((node(e, "A"), node(e, "OUT2")), (Some(a), Some(b)) if (a - b).abs() < 0.05);
    if !same {
        e.lesson_unmet.replace(Some(Unmet::Note("A と OUT2 がまだ違います。RL2 も 1k のままか、VTH と RTH の値を確かめてください".into())));
    }
    same
}
