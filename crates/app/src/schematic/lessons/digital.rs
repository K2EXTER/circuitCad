//! Lessons that run a digital circuit by hand (the live logic mode, `schematic/live_logic.rs`): the switches are clicked, the
//! nets light High / Low, the truth table fills, the clock is stepped.

use super::*;

/// The live logic mode is on and `f` holds for it.
fn live(e: &SchEditor, f: impl Fn(&super::super::live_logic::LiveLogic) -> bool) -> bool {
    e.live_logic.as_ref().is_some_and(f)
}

fn truth_complete(e: &SchEditor) -> bool {
    live(e, |l| l.truth.as_ref().is_some_and(|t| t.complete()))
}

fn stepped_events(e: &SchEditor) -> bool {
    live(e, |l| l.slow && l.events_stepped >= 3)
}

/// Both buttons released has shown Q at 1 and at 0: the latch remembers.
fn latch_remembered(e: &SchEditor) -> bool {
    live(e, |l| l.truth.as_ref().is_some_and(|t| t.rows.get(&0b11).is_some_and(|r| r.first() == Some(&None))))
}

fn latch_both_pressed(e: &SchEditor) -> bool {
    live(e, |l| l.truth.as_ref().is_some_and(|t| t.rows.contains_key(&0)))
}

fn clocked_by_hand(e: &SchEditor) -> bool {
    live(e, |l| l.clock_edges >= 6)
}

fn clock_running_fast(e: &SchEditor) -> bool {
    live(e, |l| l.running && l.hz >= 4.0 && l.clock_edges >= 20)
}

const LIVE_LIMITS: &[&str] = &[
    "動かしている回路は、過渡解析と同じシミュレータで、画面の時刻まで続きを解いています (別の簡単な計算ではありません)。ゲートは、しきい値が電源の半分、出力が約 60Ω の抵抗、遅れが 1 つの代表値の簡単なモデルです。",
    "LED は、光っているか消えているかだけを見せます (明るさは表しません)。電流や電圧を数字で見るには、編集に戻って DC 動作点や過渡解析を使います。",
    "スイッチは、跳ね (チャタリング) のない理想のスイッチです。実物の押しボタンは、押した瞬間に数 ms のあいだ接点が何度も跳ねます。",
];

pub(super) const HALF_ADDER: Lesson = Lesson {
    id: "half_adder",
    title: "87. 半加算器を動かす (スイッチで 0 と 1、真理値表を埋める)",
    goal: "2 つのスライドスイッチで 1 ビットの数 A と B を作り、XOR と AND のゲートで足し算 (半加算器) ができることを、スイッチを切り替えながら確かめます。4 通りを全部試して真理値表を埋め、ゆっくり再生で、信号がゲートを伝わる遅れ (伝搬遅延) を目で見ます",
    summary: "デジタル回路は、電圧の High (1) と Low (0) で数を表します。1 ビットの数 A と B を足すと、答えは 0、1、2 (2 進数で 10) の 3 通りで、2 ビットが要ります。下の桁 S (Sum、和) は「A と B のどちらか 1 つだけが 1 のとき 1」で、これは XOR (排他的論理和) です。上の桁 C (Carry、桁上がり) は「A と B が両方 1 のとき 1」で、これは AND です。この 2 つのゲートの組を半加算器と呼びます (下の桁からの桁上がりも足すのが全加算器で、全加算器を並べると何ビットでも足せます)。入力と出力の関係を全部書いた表が真理値表で、入力が 2 つなら 2 × 2 = 4 行です。この回路は、スライドスイッチ SW1・SW2 の COM が、A 側 (GND、0) か B 側 (電源、1) のどちらかにつながり、A と B を作ります。プルダウンの抵抗が要らない作り方です。74HC86 (XOR) の出力が S、74HC08 (AND) の出力が C で、それぞれ 330Ω を通して LED を光らせます。動かしている間、配線は High が赤、Low が青、どちらとも言えない電圧が黄色 (不定、X) になり、ゲートのピンの横に 0 と 1 が出ます。実物のゲートは、入力が変わってから出力が変わるまでに 10ns ほどかかります (伝搬遅延)。ゆっくり再生は、この遅れを 1 秒ほどに引き延ばします。",
    limits: LIVE_LIMITS,
    real: &[
        "用意するもの: 74HC86、74HC08、スライドスイッチ 2 個 (または、ジャンパ線を GND と 5V につなぎ替える)、LED 2 個と 330Ω 2 本、5V の電源 (USB)、ブレッドボード。IC の電源ピンの近くに 0.1µF を入れます。",
        "組み方: 回路図のとおりに組み、使わないゲートの入力 (4・5・9・10・12・13 番) は GND につなぎます (浮かせると、勝手に切り替わることがあります)。",
        "確かめ方: スイッチを 4 通りに切り替え、LED の光り方を、このソフトで埋めた真理値表と比べます。",
        "安全: 5V 以下で行います。LED には必ず抵抗を入れます。",
    ],
    circuit: include_str!("../../../../../docs/examples/half_adder.schjson"),
    setup: |e| e.start_live_logic(),
    steps: &[
        Step {
            tab: PanelTab::Draw,
            title: "スイッチを切り替えて、4 通りを全部試す",
            detail: "回路図のスライドスイッチ SW1 (A) と SW2 (B) をクリックすると、0 と 1 が切り替わります。押すたびに、右下の真理値表の行が 1 つずつ埋まります。A = 1・B = 1 のとき、S は 0 で C が 1 (1 + 1 = 2 進数の 10) になるのを確かめ、4 行を全部埋めます。",
            done: truth_complete,
        },
        Step {
            tab: PanelTab::Draw,
            title: "ゆっくり再生にして、信号が伝わる順を 1 イベントずつ見る",
            detail: "上の帯の「ゆっくり」に印を付け、▶ を ⏸ にして止めます。SW1 をクリックしてから、「1 イベント」を何度か押します。最初にスイッチの線 (A) が変わり、次に約 10ns 遅れて XOR の出力 S と AND の出力 C が変わります。ゲートの数だけ遅れが積み重なるのが、速い回路を作るときの限界になります。",
            done: stepped_events,
        },
    ],
};

pub(super) const SR_LATCH: Lesson = Lesson {
    id: "sr_latch",
    title: "88. SR ラッチを動かす (2 つの NAND で 1 ビットを覚える)",
    goal: "NAND ゲート 2 つを、お互いの出力を入力に返してつなぐと、押しボタンを離したあとも、最後に押した方を覚えていること (ラッチ) を確かめます。同じ入力でも出力が前の状態で決まるので、真理値表に「記憶」の行が出ます",
    summary: "これまでのゲートは、出力が今の入力だけで決まりました (組み合わせ回路)。出力を入力に返すと、回路は自分の状態を持ち続けられます (順序回路)。SR ラッチは、その一番簡単な形で、NAND ゲート 2 つでできます。Q = NOT(S_N AND QB)、QB = NOT(R_N AND Q) です。入力 S_N と R_N は、ふだん 10kΩ のプルアップで High で、押しボタンを押すと GND につながって Low になります (Low で働く負論理なので、名前に _N を付けています)。S_N を Low にすると Q が 1 (セット)、R_N を Low にすると Q が 0 (リセット) になります。両方を High に戻しても (ボタンを離しても)、Q は変わらず、最後の操作を覚えています。これが 1 ビットの記憶で、SRAM やフリップフロップの元になっています。両方を同時に押すと Q も QB も 1 になり、「Q と QB は反対」という約束が崩れます。この状態から両方同時に離すと、どちらに落ちるかは、ゲートの遅れの小さな差で決まります (実物では予測できません)。そのため、両方を同時に Low にするのは禁止の入力です。74HC00 の残りの 2 つの NAND は、入力を 2 本ともつないでインバータ (NOT) にして、LED を光らせる役 (LED_Q = Q、LED_QB = QB) にしています。ラッチの線に LED の電流を流さないためです。",
    limits: LIVE_LIMITS,
    real: &[
        "用意するもの: 74HC00、タクトスイッチ 2 個、10kΩ 2 本 (プルアップ)、LED 2 個と 330Ω 2 本、5V の電源 (USB)、ブレッドボード。IC の電源ピンの近くに 0.1µF を入れます。",
        "組み方: 回路図のとおりに組みます。74HC00 の 4 つの NAND を全部使います (2 つがラッチ、2 つが LED を光らせるインバータ)。",
        "確かめ方: SET (SW1) を押して離すと Q の LED が点いたまま、RESET (SW2) を押して離すと消えたままになるのを見ます。電源を入れ直した直後に、どちらの LED が点くかも見ます (決まっていません)。",
        "安全: 5V 以下で行います。LED には必ず抵抗を入れます。",
    ],
    circuit: include_str!("../../../../../docs/examples/sr_latch.schjson"),
    setup: |e| e.start_live_logic(),
    steps: &[
        Step {
            tab: PanelTab::Draw,
            title: "セットとリセットを押して、離しても覚えているのを見る",
            detail: "押しボタン SW1 (S_N) をクリックして押している間、S_N が Low (青) になり、Q の LED (D1) が点きます。離しても点いたままです。次に SW2 (R_N) を押すと消え、離しても消えたままです。真理値表の S_N = 1・R_N = 1 の行は、LED_Q が 1 のときも 0 のときもあったので「記憶」になります。",
            done: latch_remembered,
        },
        Step {
            tab: PanelTab::Draw,
            title: "両方を同時に押す (禁止の入力)",
            detail: "押しボタンは、Shift を押しながらクリックすると、押したままになります (もう一度クリックすると離れます)。SW1 を Shift + クリックで押したままにして、SW2 もクリックして押します。D1 と D2 の LED が両方点き、「Q と QB は反対」の約束が崩れます。真理値表に S_N = 0・R_N = 0 の行が出ます。最後に SW1 をクリックして離します。",
            done: latch_both_pressed,
        },
    ],
};

pub(super) const COUNTER_LIVE: Lesson = Lesson {
    id: "counter_live",
    title: "89. カウンタを動かす (クロックで 1 つずつ数える)",
    goal: "4 ビットのカウンタ 74HC161 にクロックを 1 回ずつ入れて、4 つの LED が 2 進数で 0 から 15 まで数えるのを見ます。クロックの速さを変えて動かし、ロジックアナライザのように流れる波形で、各ビットの周期が倍ずつ長くなることを確かめます",
    summary: "カウンタは、クロックの立ち上がり (Low から High に変わる瞬間) のたびに、持っている数を 1 つ増やす回路です。中はフリップフロップ (クロックの立ち上がりでだけ入力を取り込んで覚える記憶の素子) 4 個と、次の数を作るゲートでできています。74HC161 の出力 QA〜QD (この回路のネット Q0〜Q3) は、Q0 が一番下の桁 (1)、Q3 が一番上の桁 (8) の 2 進数で、0000 → 0001 → 0010 → … → 1111 (15) と進み、次は 0000 に戻ります。Q0 はクロック 2 回で 1 周期、Q1 は 4 回、Q2 は 8 回、Q3 は 16 回で、周期が倍ずつ長くなります (周波数が半分ずつになる: 分周)。下のロジックアナライザでは、Q0〜Q3 が 1 本のバス Q にまとまり、値が 16 進数 (0〜F) で出ます。この回路の VCK は、ふつうの解析では 1Hz のパルスの電源ですが、動かしている間は、上の帯のつまみと「1 クロック」のボタンがクロックになります。CLR・LOAD・ENP・ENT は電源につないで、いつも数える設定にしています。",
    limits: LIVE_LIMITS,
    real: &[
        "用意するもの: 74HC161、LED 4 個と 330Ω 4 本、5V の電源 (USB)、ブレッドボード。クロックには、555 の発振回路 (1Hz くらい) か、マイコンの出力を使います。IC の電源ピンの近くに 0.1µF を入れます。",
        "注意: クロックを押しボタンで作ると、接点が跳ねて (チャタリング)、1 回押しただけで何回も数えます。押しボタンで試すなら、RC とシュミットトリガ (74HC14) でチャタリングを取ります (レッスン「チャタリングを取る」)。",
        "確かめ方: クロックを 1 回ずつ入れ、LED の並びを 2 進数で読みます。16 回で 0 に戻るのを確かめます。",
        "安全: 5V 以下で行います。LED には必ず抵抗を入れます。",
    ],
    circuit: include_str!("../../../../../docs/examples/live_counter.schjson"),
    setup: |e| e.start_live_logic(),
    steps: &[
        Step {
            tab: PanelTab::Draw,
            title: "「1 クロック」を押して、1 つずつ数えるのを見る",
            detail: "上の帯の「1 クロック」を押すと、クロックが 1 回上がって下がり、LED (Q0〜Q3) が 2 進数で 1 つ増えます。3 回押して、0001 → 0010 → 0011 と進むのを確かめます。下の波形では、バス Q の値が 1、2、3 と増えます。",
            done: clocked_by_hand,
        },
        Step {
            tab: PanelTab::Draw,
            title: "クロックを速くして動かし、分周を見る",
            detail: "▶ で動かし、クロックのつまみを 4Hz 以上にします。下のロジックアナライザで、Q0 は 2 クロック、Q1 は 4 クロックで 1 周期と、周期が倍ずつ長くなるのを見ます (バス Q の 3 角の印を押すと、ビットごとに開きます)。",
            done: clock_running_fast,
        },
        Step {
            tab: PanelTab::Draw,
            title: "ゆっくり再生で、クロックから出力までの遅れを見る",
            detail: "⏸ で止め、「ゆっくり」に印を付けて、「1 クロック」を押してから「1 イベント」を何度か押します。クロックが上がってから、約 20ns 遅れて Q0〜Q3 が変わります (クロックから出力までの遅れ)。桁上がりのあるとき (0111 → 1000 など) も、4 つのビットはほぼ同時に変わります。74HC161 は全部のフリップフロップに同じクロックが入る同期カウンタだからです。",
            done: stepped_events,
        },
    ],
};
