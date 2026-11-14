//! The "修理の道" lessons: a built board of the lesson's circuit has one hidden fault. The learner measures it with the virtual
//! meter (`repair_practice.rs`), compares the readings with the healthy circuit's DC operating point, and names the cause. The
//! fault differs from run to run (the seed is the clock).

use super::conditions::*;
use super::*;

fn fresh_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64 / 1000)
}

fn measured_symptom(e: &SchEditor) -> bool {
    e.repair.as_ref().is_some_and(|r| r.readings.iter().any(|(n, _)| *n == r.symptom))
}

fn solved(e: &SchEditor) -> bool {
    e.repair.as_ref().is_some_and(|r| r.solved)
}

const LIMITS: &[&str] = &[
    "故障は 1 つだけで、DC (落ち着いたあと) の電圧だけで見分けます。実物では、2 つ同時に壊れていることや、温めると症状が変わる接触不良もあります。",
    "テスターの読みは、壊れた回路のシミュレーションの値に、小さなばらつき (±0.3% + 2mV) を足したものです。実物のテスターは、当てる場所 (ピンの根元か、配線の途中か) や、テスター自身の誤差でも読みが変わります。",
    "壊れ方は、断線・ショート・値の桁違い・ピンの浮き (はんだ付けの不良) を、部品の壊れやすさの目安で選んでいます。実物では、半分だけ切れかかった抵抗や、熱で一時的に動かなくなる IC のような、中途半端な壊れ方もあります。",
];

const REAL: &[&str] = &["実物を測るときは、黒いリード (COM) を回路の GND に当てたまま、赤いリードで 1 点ずつ当てます。電源を入れた状態で測るので、ピンどうしをリードでショートさせないよう注意します。"];

/// The steps every repair lesson shares; only the point where the trouble shows differs.
const fn steps(measure: &'static str) -> [Step; 3] {
    [
        Step { tab: PanelTab::Draw, title: "症状の出ている点を、テスターで測る", detail: measure, done: measured_symptom },
        Step {
            tab: PanelTab::Analyze,
            title: "正しい回路の電圧を見て、比べる",
            detail: "「DC動作点を解析」を押すと、回路図 (正しい設計) の各点の電圧が出ます。テスターで測った基板の値と比べて、食い違いが始まる所を探します。修理の基本は「正しいはずの値」と「測った値」の比較です。",
            done: dc_fresh,
        },
        Step {
            tab: PanelTab::Draw,
            title: "原因を見つけて答える",
            detail: "電源から出力へ、食い違う点の前後を測って、場所を絞ります。見当がついたら、レッスンの窓の「原因」で部品と壊れ方を選び、「これが原因」を押します。外れても続けられます。迷ったら「ヒント」で、次に測るとよい点が分かります。",
            done: solved,
        },
    ]
}

const DIVIDER_STEPS: [Step; 3] = steps("レッスンの窓の「テスターを当てる」がオンのまま、回路図の MID の配線をクリックします。正しい分圧なら 2.5V のはずです。");
const ZENER_STEPS: [Step; 3] = steps("回路図の VOUT の配線をクリックして測ります。正しく動いていれば 5.1V 前後のはずです。");
const LED_STEPS: [Step; 3] = steps("LED が点きません。回路図の COL (トランジスタのコレクタ) の配線をクリックして測ります。トランジスタがオンなら、ほぼ 0V のはずです。");
const REGULATOR_STEPS: [Step; 3] = steps("回路図の VOUT の配線をクリックして測ります。正しく動いていれば 5V 前後のはずです。");

pub(super) const REPAIR_DIVIDER: Lesson = Lesson {
    id: "repair_divider",
    title: "修理 1. テスターの当て方 (分圧回路)",
    goal: "作った分圧回路の出力がおかしい。テスターで測って、どの部品がどう壊れているかを当てます",
    summary: "修理は、見当をつけて部品を替えることではなく、測って場所を絞ることです。テスターの黒いリードを GND に当てたまま、赤いリードで 1 点ずつ電圧を測ります。\n\n回路図は正しい設計のままです。壊れているのは、それを作った「基板」のほうで、どこが壊れているかは見えません。テスターで測った値と、回路図をシミュレーションした「正しい値」を比べて、食い違いが始まる所を探します。\n\n分圧回路は部品が 3 つだけです。出力 MID が 0V なら「上の抵抗が切れている」か「電源が来ていない」、電源の電圧と同じなら「下の抵抗が切れている」。どちらかを見分けるには、もう 1 点 (VIN) を測ります。",
    limits: LIMITS,
    real: REAL,
    circuit: include_str!("../../../../../docs/examples/divider_tolerance.schjson"),
    setup: |e| e.start_repair("MID", fresh_seed()),
    steps: &DIVIDER_STEPS,
};

pub(super) const REPAIR_ZENER: Lesson = Lesson {
    id: "repair_zener",
    title: "修理 2. ツェナーの電源",
    goal: "ツェナーで 5.1V を作るはずの電源が、正しい電圧を出さない。測って原因を当てます",
    summary: "電源の修理は、電源の入口 (VIN) から出口 (VOUT) へ、順に測るのが基本です。入口が正しくて出口がおかしければ、その間の部品 (直列の抵抗 R1、ツェナー D1、負荷 R2) のどれかです。\n\n出力が 0V に近いなら、電流が届いていない (R1 が切れている) か、出力が GND に落ちている (ツェナーのショート)。出力が 5.1V より高いなら、ツェナーが働いていない (切れている) ことを疑います。\n\nツェナーのショートと R1 の断線は、どちらも出力が 0V になり、VOUT だけでは見分けられません。R1 の両端を測ると分かります。",
    limits: LIMITS,
    real: REAL,
    circuit: include_str!("../../../../../docs/examples/zener_regulator.schjson"),
    setup: |e| e.start_repair("VOUT", fresh_seed()),
    steps: &ZENER_STEPS,
};

pub(super) const REPAIR_LED: Lesson = Lesson {
    id: "repair_led",
    title: "修理 3. トランジスタで点ける LED",
    goal: "トランジスタでオンにしているはずの LED が点かない。測って原因を当てます",
    summary: "トランジスタのスイッチは、ベース (BASE) に電流を流すと、コレクタ (COL) から電流を引き込み、LED が点きます。オンのとき、ベースは約 0.7V、コレクタはほぼ 0V です。\n\nLED が点かないとき、まずコレクタを測ります。5V 近くなら、トランジスタがオンになっていない (ベースに電流が来ていない、トランジスタが壊れている)。ほぼ 0V なのに点かないなら、LED や、LED の上の抵抗の側を疑います。\n\nトランジスタは、ピンのはんだ付けの不良 (ピンが浮いている) もよく起きます。ピンが浮いたときの電圧も考えてみましょう。",
    limits: LIMITS,
    real: REAL,
    circuit: include_str!("../../../../../docs/examples/repair_led_switch.schjson"),
    setup: |e| e.start_repair("COL", fresh_seed()),
    steps: &LED_STEPS,
};

pub(super) const REPAIR_REGULATOR: Lesson = Lesson {
    id: "repair_regulator",
    title: "修理 4. 帰還のある電源 (直列レギュレータ)",
    goal: "オペアンプとトランジスタで 5V を作る電源が、正しい電圧を出さない。帰還のループの中から原因を絞ります",
    summary: "帰還のある回路は、1 か所が壊れると、ループ全体の電圧がそろって狂います。どこを測っても値がおかしく見えるので、「どこから狂い始めるか」ではなく、「ループのどこで約束が破れているか」を見ます。\n\nこの電源の約束は 3 つです。基準電圧 VREF は TL431 の電圧 (約 2.5V)、帰還 FB は VREF と等しい (オペアンプが等しくしようとする)、出力 VOUT は FB を分圧の比で戻した値。VREF が正しいのに FB が違うなら、オペアンプかトランジスタ (DRIVE) の側。VREF が違えば、基準電圧の側です。\n\nオペアンプやトランジスタのピンの浮き (はんだ付けの不良) も候補に入ります。",
    limits: LIMITS,
    real: REAL,
    circuit: include_str!("../../../../../docs/examples/series_regulator.schjson"),
    setup: |e| e.start_repair("VOUT", fresh_seed()),
    steps: &REGULATOR_STEPS,
};
