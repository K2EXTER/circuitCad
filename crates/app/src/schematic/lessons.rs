//! Guided lessons: a sample circuit plus a short list of things to try. Each step is ticked off
//! by itself when the editor reaches the matching state, so the learner is never asked to press
//! a "next" button after doing what was asked.

use super::*;

pub(super) struct ActiveLesson {
    pub(super) idx: usize,
    /// The step whose left-panel page has been shown already.
    pub(super) shown: Option<usize>,
    /// Steps that have been completed (once done, a step stays done).
    pub(super) done: Vec<bool>,
}

struct Step {
    /// Which page of the left panel this step works on.
    tab: PanelTab,
    title: &'static str,
    detail: &'static str,
    done: fn(&SchEditor) -> bool,
}

struct Lesson {
    title: &'static str,
    goal: &'static str,
    summary: &'static str,
    /// Where the simulation differs from the real parts: shown as "実物との違い".
    limits: &'static [&'static str],
    /// The sample circuit (embedded, so lessons work without the docs folder).
    circuit: &'static str,
    setup: fn(&mut SchEditor),
    steps: &'static [Step],
}

// ------------------------------------------------------------------ step conditions

fn dc_fresh(e: &SchEditor) -> bool {
    e.dc.as_ref().is_some_and(|d| d.outcome.is_ok() && d.signature == e.signature())
}

fn dc_ok(e: &SchEditor) -> bool {
    e.dc.as_ref().is_some_and(|d| d.outcome.is_ok())
}

pub(super) fn value_of<'a>(s: &'a Schematic, refdes: &str) -> Option<&'a str> {
    s.components.iter().find(|c| c.refdes == refdes).map(|c| c.value.as_str())
}

fn changed(e: &SchEditor, refdes: &str) -> bool {
    value_of(&e.sch, refdes) != value_of(&e.lesson_base, refdes)
}

fn r2_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R2") && dc_fresh(e)
}

fn r3_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R3") && dc_fresh(e)
}

fn r1_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R1") && dc_fresh(e)
}

fn live_on(e: &SchEditor) -> bool {
    e.live && dc_ok(e)
}

fn mc_done(e: &SchEditor) -> bool {
    e.mc.as_ref().is_some_and(|m| m.outcome.is_ok())
}

fn temp_sweep_done(e: &SchEditor) -> bool {
    e.sweep.as_ref().is_some_and(|s| s.temp && s.outcome.is_ok())
}

fn ac_done(e: &SchEditor) -> bool {
    e.ac.as_ref().is_some_and(|a| a.outcome.is_ok())
}

fn ac_fresh(e: &SchEditor) -> bool {
    e.ac.as_ref().is_some_and(|a| a.outcome.is_ok() && a.signature == e.signature())
}

fn tran_fresh(e: &SchEditor) -> bool {
    e.tran.as_ref().is_some_and(|t| t.outcome.is_ok() && t.signature == e.signature())
}

fn c1_changed_and_ac(e: &SchEditor) -> bool {
    changed(e, "C1") && ac_fresh(e) && e.wave_mode == WaveMode::Ac
}

fn mc_cutoff_done(e: &SchEditor) -> bool {
    e.mc_kind == McKind::Ac && mc_done(e)
}

fn tran_done(e: &SchEditor) -> bool {
    e.tran.as_ref().is_some_and(|t| t.outcome.is_ok())
}

fn c1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "C1") && tran_fresh(e)
}

fn l1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "L1") && tran_fresh(e)
}

fn r3_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "R3") && tran_fresh(e)
}

fn r1_restored_and_tran(e: &SchEditor) -> bool {
    value_of(&e.sch, "R1") == value_of(&e.lesson_base, "R1") && tran_fresh(e)
}

fn r1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "R1") && tran_fresh(e)
}

fn mc_overshoot_done(e: &SchEditor) -> bool {
    e.mc_kind == McKind::Tran && e.mc_metric == TranMetric::Overshoot && mc_done(e)
}

fn sweep_done(e: &SchEditor) -> bool {
    e.sweep.as_ref().is_some_and(|s| !s.temp && s.outcome.is_ok())
}

fn gain_restored_and_ac(e: &SchEditor) -> bool {
    value_of(&e.sch, "R1") == value_of(&e.lesson_base, "R1") && ac_fresh(e) && e.wave_mode == WaveMode::Ac
}

fn currents_shown(e: &SchEditor) -> bool {
    e.show_i && dc_fresh(e)
}

// ------------------------------------------------------------------ the lessons

const LESSONS: &[Lesson] = &[
    Lesson {
        title: "1. 電圧を分ける (分圧回路)",
        goal: "抵抗を2本つなぐと、電源電圧を好きな比率に割れることを確かめます",
        summary: "出力電圧は 電源 × R2 ÷ (R1+R2) です。抵抗の比で決まるので、同じ比なら 1k+1k でも 10k+10k でも同じ電圧です。センサーの信号を測りやすい電圧に変えるなど、あらゆる回路で使われます。",
        limits: &[
            "抵抗は値が変わらない理想の抵抗です。実物には誤差 (1% や 5%) があり、温度でも少し変わります。誤差の影響は、この回路の「ばらつき解析」で調べられます。",
            "電源は内部抵抗のない理想の電圧源です。電池や安い電源には内部抵抗があり、電流を流すと電圧が下がります。",
            "出力には何もつないでいません。次の回路をつなぐと、その入力抵抗が R2 と並列になって、出力電圧が下がります (負荷効果)。",
        ],
        circuit: include_str!("../../../../docs/examples/divider_tolerance.schjson"),
        setup: |e| e.mc_out = "MID".into(),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "DC動作点を解析する",
                detail: "左パネルの「DC動作点を解析」を押します。回路図の各点に電圧が出ます。真ん中の点 (MID) は電源 5V のちょうど半分、2.5V のはずです。配線の色は、低い電圧が青、高い電圧が赤です。",
                done: dc_ok,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R2 を 30k にして、もう一度解析する",
                detail: "R2 をクリックで選び、左の「値」を 30k にします。そのあと「解析」タブで、もう一度「DC動作点を解析」を押します。MID は 5 × 30k ÷ (10k+30k) = 3.75V に変わります。",
                done: r2_changed_and_measured,
            },
            Step {
                tab: PanelTab::Draw,
                title: "「自動で再計算」をオンにして、スライダーを動かす",
                detail: "「自動で再計算」にチェックを入れ、R2 を選んで「スライダー」を左右に動かしてみましょう。動かすたびに MID の電圧が変わります。式どおりに変わっているか確かめてください。",
                done: live_on,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "部品の誤差で電圧がどれだけずれるか見る",
                detail: "「温度とばらつき」の「ばらつき解析を実行」を押します。実際の抵抗は 5% くらいの誤差があるので、MID は 2.5V ぴったりにはなりません。どの抵抗が効いているかも出ます。",
                done: mc_done,
            },
        ],
    },
    Lesson {
        title: "2. LED と電流制限抵抗",
        goal: "LED に抵抗が必要な理由と、温度の影響を確かめます",
        summary: "LED は電圧がわずかに上がるだけで電流が急に増えます。そのため抵抗で電流を決めます (電流 = (電源 − LED の電圧) ÷ 抵抗)。また、ダイオードの電圧は温度が上がると約 2mV/℃ 下がります。",
        limits: &[
            "LED はダイオードの式で、順方向電圧がおよそ 2V になるように決めた簡易モデルです。実物は、色 (赤・青・白) や個体によって 1.8〜3.3V ほど違い、温度でも変わります。",
            "明るさ (光の量) は計算しません。電流が大きいほど明るくなりますが、定格の電流を超えると実物は壊れます (シミュレーションでは壊れません)。",
            "電源と配線の抵抗は 0 として計算しています。",
        ],
        circuit: include_str!("../../../../docs/examples/led.schjson"),
        setup: |_| {},
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "DC動作点を解析して、電流を見る",
                detail: "「DC動作点を解析」を押し、「電流表示」をオンにします。R1 と LED の下に電流が出ます。LED の電圧は 2V くらい、電流は (5−2)÷330 で約 9mA です。",
                done: currents_shown,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 100 にして、電流の変化を見る",
                detail: "R1 を選んで値を 100 にし、もう一度解析します。電流が約 3 倍になります。抵抗を小さくしすぎると LED が壊れる理由です (一般的な LED は 20mA 程度まで)。",
                done: r1_changed_and_measured,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "温度を変えて動作点がどう動くか見る",
                detail: "「温度とばらつき」の「温度スイープを実行」を押します。温度が上がると LED の電圧が下がり、電流がわずかに増えるのがグラフで分かります。",
                done: temp_sweep_done,
            },
        ],
    },
    Lesson {
        title: "3. RC ローパスフィルタ",
        goal: "抵抗とコンデンサで、高い周波数だけを弱める回路の特性を見ます",
        summary: "カットオフ周波数は fc = 1 ÷ (2π × R × C) です。fc で出力は −3dB (約 0.7 倍) になり、それより高い周波数は 1 桁上がるごとに 1/10 になります。ノイズ除去や、音の高音を削るトーンコントロールの基本です。",
        limits: &[
            "抵抗とコンデンサは理想の部品です。実物のコンデンサには、直列の抵抗 (ESR) とインダクタンス (ESL) があり、周波数が高いと特性が変わります。値に「159n ESR=0.5 ESL=5n」のように書くと、モデルに入れて確かめられます。",
            "セラミックコンデンサは、かける電圧が高いと容量が減ります (DC バイアス特性)。この影響は計算しません。",
            "信号源は内部抵抗のない理想の電源です。実物の信号源の出力抵抗は R1 に足し算されるので、カットオフがずれます。",
        ],
        circuit: include_str!("../../../../docs/examples/rc_lowpass.schjson"),
        setup: |_| {},
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "AC解析を実行する",
                detail: "「AC解析を実行」を押します。下にボード線図 (上が大きさ dB、下が位相) が出ます。この回路は R=1k、C=159n なので fc = 1 ÷ (2π×1k×159n) ≒ 1kHz。1kHz のあたりで 0dB から −3dB に下がり始めます。",
                done: ac_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "C1 を 1.59u にして、もう一度解析する",
                detail: "C1 の値を 1.59u (10倍) にして「AC解析を実行」。カットオフが 1/10 の約 100Hz に動きます。コンデンサが大きいほど低い周波数から信号を弱めます。",
                done: c1_changed_and_ac,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "部品の誤差でカットオフがどれだけずれるか見る",
                detail: "C1 を 159n に戻し、「温度とばらつき」で「測るもの」を「カットオフ周波数 (AC)」、「測る点」をフィルタの出力 (R1 と C1 の間) にして「ばらつき解析を実行」。許容差 5%/10% の部品だと、カットオフは 1kHz ぴったりにならず 1 割ほど広がります。フィルタを正確に作るには、許容差の小さい部品が必要です。",
                done: mc_cutoff_done,
            },
        ],
    },
    Lesson {
        title: "4. トランジスタのスイッチ",
        goal: "トランジスタを使った反転回路 (NOT) の動きと、切り替わる電圧を見ます",
        summary: "ベースに十分な電流を流すとトランジスタがオンになり、出力が 0V に引き下げられます。入力が約 0.7V を超えたあたりで切り替わります。デジタル回路の基本動作です。",
        limits: &[
            "2N3904 の電流増幅率 hFE は、代表値の 300 に固定です。実物は 100〜300 など個体差が大きく、電流や温度でも変わります。スイッチを確実に ON にするには、ベース電流を多めに流すのが定石です。",
            "トランジスタの内部の容量と蓄積時間を含みません。そのため、ON/OFF の速さは実物より速く出ます。高速にスイッチする回路の見積もりには使えません。",
            "温度の影響は、「解析」タブの温度スイープで確かめられます (ベース・エミッタ間の電圧が 1℃ あたり約 2mV 下がり、hFE がゆっくり上がる)。ただし、実物の hFE の温度変化の大きさは、部品ごとに違います。",
        ],
        circuit: include_str!("../../../../docs/examples/npn_not.schjson"),
        setup: |e| {
            e.sweep_src = "V2".into();
            e.sweep_start = "0".into();
            e.sweep_stop = "5".into();
            e.sweep_step = "0.05".into();
        },
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で入力と出力の波形を見る",
                detail: "「過渡解析を実行」を押します。入力 (IN) が 0V→5V になると、出力 (OUT) は 5V→0V に反転します。信号の High/Low が逆になる、これが NOT です。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "DCスイープで切り替わる電圧を見る",
                detail: "「DCスイープを実行」(電源 V2 を 0〜5V) を押し、OUT を選びます。入力が約 0.7V を超えると出力が急に下がります。そこがスイッチが切り替わる点です。",
                done: sweep_done,
            },
        ],
    },
    Lesson {
        title: "5. 555 で LED を点滅させる",
        goal: "タイマー IC 555 とコンデンサの充放電で、一定の間隔で点滅させます",
        summary: "コンデンサに抵抗で充電し、ある電圧で放電する、を繰り返すと一定周期の信号ができます。周波数はおよそ 1.44 ÷ ((R1+2×R2)×C1)。コンデンサを小さくすると速く点滅します。",
        limits: &[
            "555 は、コンパレータとラッチを部品で組んだ動作モデルです。しきい値 (電源の 2/3 と 1/3) と出力 (High は電源 −1.7V) は再現しますが、温度や個体によるしきい値のずれは扱いません。",
            "点滅の周期は、部品が理想のときの値です。実物は、コンデンサの誤差や漏れ電流、しきい値のばらつきで 5〜20% ずれることがあります。",
            "出力が切り替わるときに電源へ流れる大きな電流 (スパイク) は扱いません。実物では電源に 0.1µF 程度のコンデンサを付けてこれを抑えます。",
            "モデルは 5V・9V・12V の電源で、周期が同じになることを確かめてあります。実物の NE555 は 4.5V 未満では動作が保証されません。",
        ],
        circuit: include_str!("../../../../docs/examples/ne555_blinker.schjson"),
        setup: |e| e.set_tran_settings("3", "2m"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析を実行する",
                detail: "555 は時間とともに状態が変わる回路なので、DC動作点ではなく過渡解析で見ます。終了 3 秒、刻み 2m のまま「過渡解析を実行」。OUT の波形が High/Low を繰り返します。TH (コンデンサ C1 の電圧) が三角っぽく上下するのも見てみましょう。R1=1k、R2=68k、C1=10u だと約 1Hz です。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "C1 を 1u にして、もう一度解析する",
                detail: "C1 を 1u にして「過渡解析を実行」。周期が 1/10 になり、波形が 10 倍の速さで繰り返します。",
                done: c1_changed_and_tran,
            },
        ],
    },
    Lesson {
        title: "6. 行き過ぎとリンギング (RLC回路)",
        goal: "コイルとコンデンサの回路が、ステップに対して「行き過ぎて揺れる」様子と、抵抗でそれを抑える方法を見ます",
        summary: "コイルとコンデンサは、エネルギーをやり取りして振動します。抵抗が小さいと最終値を行き過ぎて何度も揺れ (リンギング)、大きいと揺れずにゆっくり近づきます。電源回路やデジタル信号の配線でも、行き過ぎ (オーバーシュート) は誤動作や部品破損の原因になります。",
        limits: &[
            "コイルとコンデンサは理想の部品で、損失は抵抗 R1 だけです。実物のコイルには巻線の抵抗 (DCR) があり、電流が大きいと磁気飽和してインダクタンスが下がります。コンデンサにも ESR があります。これらは損失になるので、実物のリンギングは、この波形より早く収まることが多いです。",
            "基板の配線にも、わずかなインダクタンスと容量があり、速い信号ではそれがリンギングの原因になります。この回路には含まれません。",
            "電源は、立ち上がりの形が決まった理想の電源です。",
        ],
        circuit: include_str!("../../../../docs/examples/rlc_pulse.schjson"),
        setup: |e| e.set_tran_settings("5.5m", "5u"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析を実行して、行き過ぎを見る",
                detail: "「過渡解析を実行」を押し、C1 の電圧 (C1 のつなぎ目の信号) を選びます。電源が 0V→5V になった直後、5V を超えて揺れながら落ち着きます。左の「ステップ応答の指標」を開くと、行き過ぎが約 35% と出ます。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 100 にして、もう一度解析する",
                detail: "R1 を 20 から 100 にして「過渡解析を実行」。揺れがなくなり、行き過ぎが 0% になります。抵抗が振動のエネルギーを熱にして逃がす (ダンピング) ためです。代わりに立ち上がりは遅くなります。",
                done: r1_changed_and_tran,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "部品の誤差で行き過ぎがどれだけずれるか見る",
                detail: "R1 を 20 に戻し、「温度とばらつき」で「測るもの」を「ステップ応答」、「指標」を「オーバーシュート」、「測る点」を C1 の電圧にして「ばらつき解析を実行」。部品の誤差で、行き過ぎの大きさが何 % ずれるかが分かります。",
                done: mc_overshoot_done,
            },
        ],
    },
    Lesson {
        title: "7. リチウムイオン電池の充電 (TP4056)",
        goal: "充電ICの内部をのぞかず、「外から見た動き」で、電池の充電の3段階を見ます",
        summary: "リチウムイオン電池は、空に近いときはそっと (プリチャージ)、普通は一定の電流で (定電流)、満タン近くでは電圧を 4.2V に保ちながら電流を減らして (定電圧) 充電します。TP4056 は PROG 端子の抵抗だけで電流を決められます (電流 = 1200 ÷ 抵抗)。実際の電池は数千 F 相当の巨大なコンデンサのようなものなので、ここでは 470µF の小さなコンデンサを電池の代わりにして、時間を縮めています。",
        limits: &[
            "TP4056 は、外から見た動きだけを再現する動作モデルです。プリチャージ・定電流・定電圧の3段階と、状態表示 (CHRG・STDBY) は再現します。",
            "扱わないもの: 温度による電流の制限と TEMP 端子、再充電のしきい値、充電タイマ、入力の逆接続保護。TP4056 は入力と電池の電圧差が熱になる方式で、実物は大きな電流だと熱で電流が自動的に減ることがありますが、それも扱いません。",
            "電池は、小さなコンデンサで代用しています。実物の電池は容量が桁違いに大きく (充電に数時間かかる)、放電特性も複雑です。ここでは時間を縮めて、3段階の形を見るためのものです。",
            "電流の式 (1200 ÷ R) は、データシートの代表値です。実物の電流は、個体や温度でばらつきます。",
        ],
        circuit: include_str!("../../../../docs/examples/li_ion_charger.schjson"),
        setup: |e| e.set_tran_settings("12m", "20u"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、充電の3段階を見る",
                detail: "「過渡解析を実行」を押し、BAT (電池の端子) と CHRG、STDBY の信号を選びます。BAT は 2.6V から、①ゆっくり上がる (プリチャージ、2.9V まで)、②まっすぐ上がる (定電流)、③4.2V で頭打ち (定電圧) の順に変わります。CHRG は充電中 Low (LED が点く)、満タンで High になり、代わりに STDBY が Low になります。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "PROG の抵抗 R1 を 12k にして、もう一度解析する",
                detail: "R1 を 6k から 12k にして「過渡解析を実行」。充電電流が半分になるので、BAT の上がり方が緩やかになり、満タンまでに時間がかかります。電流は左の「電流表示」でも確認できます (DC動作点を解析)。",
                done: r1_changed_and_tran,
            },
            Step {
                tab: PanelTab::Draw,
                title: "電池の初期電圧を 3.6V にして、もう一度解析する",
                detail: "R1 を 6k に戻し、C1 の値の IC=2.6 を IC=3.6 に変えて「過渡解析を実行」。最初から 2.9V を超えているので、プリチャージを飛ばして最初から定電流で充電が始まります。",
                done: c1_changed_and_tran,
            },
        ],
    },
    Lesson {
        title: "8. オペアンプで信号を大きくする (非反転増幅)",
        goal: "オペアンプと2本の抵抗で、小さな信号を決まった倍率に大きくします。倍率の限界も見ます",
        summary: "非反転増幅回路の倍率は 1 + R1 ÷ R2 です (ここでは 1 + 10k ÷ 1k = 11 倍)。倍率は抵抗の比だけで決まり、オペアンプ自体の細かい性能に左右されません。ただし出力は電源電圧の範囲までしか出せず (振れ幅の限界)、周波数が高いと倍率が下がります (GBW = 倍率 × 帯域 がほぼ一定)。倍率を上げるほど、使える帯域は狭くなります。",
        limits: &[
            "LM358 は、開ループ利得 100dB・GBW 約 1MHz・出力の振れ幅 (電源の + 側から約 1.5V 内側まで) を再現する簡易マクロモデルです。",
            "扱わないもの: 入力オフセット電圧、入力バイアス電流、スルーレート、入力できる電圧の範囲、出力電流の限界、容量性の負荷での発振 (位相余裕)、電源電圧による GBW の変化。実物では、直流の精度が必要な回路や、速い信号を大きく増幅する回路で、この差が目立ちます。",
            "「倍率 = 1 + R1 ÷ R2」になるのは、開ループ利得が十分に大きいときだけです。倍率を上げるほど GBW で頭打ちになる様子は、レッスンの手順3で確かめられます。",
        ],
        circuit: include_str!("../../../../docs/examples/opamp_amp.schjson"),
        setup: |e| {
            e.set_tran_settings("3m", "5u");
            e.set_ac_settings("100", "10Meg", "20");
        },
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、入力と出力の波形を見る",
                detail: "「過渡解析を実行」を押し、IN (入力、0.1V の正弦波) と OUT (出力) を選びます。OUT は IN と同じ形で、振れ幅が 11 倍 (約 1.1V) になっています。倍率 = 1 + R1 ÷ R2 です。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 100k にして、もう一度解析する",
                detail: "R1 を 10k から 100k にして「過渡解析を実行」。倍率は 101 倍になり、出力は 10V 近くを目指しますが、電源は ±5V です。LM358 は電源の少し手前までしか出せないので、波形の頭が平らに削られます (クリッピング)。",
                done: r1_changed_and_tran,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 10k に戻して、AC解析で周波数特性を見る",
                detail: "R1 を 10k に戻し、「解析」タブで「AC解析を実行」。倍率 (約 21dB = 11 倍) は低い周波数では平らですが、100kHz 付近から下がります。LM358 の GBW は約 1MHz なので、11 倍なら約 90kHz まで使えます。",
                done: gain_restored_and_ac,
            },
        ],
    },
    Lesson {
        title: "9. 交流を直流にする (トランスと整流)",
        goal: "100V の交流をトランスで下げ、ダイオード4個のブリッジで整流し、コンデンサでならして直流にします。残るゆらぎ (リップル) の大きさを決めるものを見ます",
        summary: "整流は、ダイオードで電流を一方向だけに通して「山」だけを取り出すことです。ブリッジ整流では交流の両方の山を使うので、リップルの周波数は電源の2倍 (50Hz なら 100Hz) になります。そのままでは波打っているので、コンデンサに電気をためて山と山の間を補います。コンデンサが大きいほど、負荷の電流が小さいほど、ゆらぎは小さくなります。ゆらぎ (p-p) の目安は 電流 ÷ (2 × 電源周波数 × 容量) です。",
        limits: &[
            "トランスは「理想変圧器 + 巻線抵抗 + 励磁インダクタンス」のモデルです。巻数比、負荷をかけたときの電圧の下がり方、無負荷電流の概算は再現します。",
            "扱わないもの: 漏れインダクタンス、鉄心の飽和、電源を入れた瞬間の突入電流、温度による変化。実物の突入電流は、コンデンサが空の瞬間にダイオードとヒューズへ大きな電流が流れます。ここでは電源電圧が 0V から滑らかに立ち上がるので、その衝撃は小さく見えます。",
            "コンデンサは理想に近い部品です。実物の電解コンデンサには ESR (内部抵抗) があり、リップル電流で発熱し、寿命が縮みます。ESR があるとリップルはこの波形より大きくなります (コンデンサの値に `ESR=0.1` のように書くと試せます)。",
            "商用電源の電圧は 100V (実効値) の正弦波とし、電圧の変動や波形のひずみは扱いません。商用電源の配線は感電の危険があります。実機は専門家の確認を受け、安全規格に適合した部品を使ってください。",
        ],
        circuit: include_str!("../../../../docs/examples/bridge_rectifier.schjson"),
        setup: |e| e.set_tran_settings("0.3", "100u"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、交流が直流に変わる様子を見る",
                detail: "「過渡解析を実行」を押し、AC1 (トランスの出力、交流) と VDC (整流してならした直流) を選びます。AC1 (トランスの片側) は、GND から見ると 0V〜16V ほどの山が 50Hz で繰り返されます (反対側の AC2 は半周期ずれて山になり、ブリッジが交互に働きます)。VDC は約 15V の直流の上に約 1V の波が乗っています。左の自動測定で、VDC の「振幅(p-p)」がリップルの大きさ、AC1 の周波数が 50Hz と読めます。波形をクリックして測定カーソルを2つ置くと、VDC の山の間隔 (約 10ms) も測れます。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "C1 を 220u にして、もう一度解析する",
                detail: "平滑コンデンサ C1 を 1000u から 220u に変えて「過渡解析を実行」。コンデンサが小さいと山と山の間に電圧が大きく下がり、リップルが約 4 倍 (約 4V) に増えます。目安の式 (電流 ÷ (2 × 周波数 × 容量)) と自動測定の p-p を比べてみましょう。式は少し大きめに出ます (ダイオードが電流を流す時間があるため)。",
                done: c1_changed_and_tran,
            },
            Step {
                tab: PanelTab::Draw,
                title: "C1 を 1000u に戻し、負荷 R1 を 33 にして解析する",
                detail: "負荷を重くする (抵抗を小さくする) と、流れる電流が増えます。C1 を 1000u に戻し、R1 を 100 から 33 にして「過渡解析を実行」。リップルが約 2 倍になり、平均の電圧も約 12V まで下がります。トランスの巻線抵抗とダイオードの電圧降下のためです。余裕があれば「故障シミュレーション」で、D1 を「断線」、比べるものを「過渡 (波形)」、見る点を VDC にして「故障を試す」を押してみましょう。半分の山しか使えず、リップルが約2倍、周波数が 50Hz に変わります。",
                done: r1_changed_and_tran,
            },
        ],
    },
    Lesson {
        title: "10. 電池の内部抵抗と持ち時間 (コイン電池でLEDを点ける)",
        goal: "電池は理想の電圧源ではなく、内部抵抗を持ち、使うほど残量が減って電圧が下がることを、コイン電池 (CR2032) で見ます",
        summary: "電池は「電圧源 + 内部抵抗」でできています。電流を流すと、内部抵抗のぶん端子電圧が下がります (CR2032 は約 15Ω)。また、電池は容量 (CR2032 は約 230mAh) のぶんだけ電流を出すと空になります。持ち時間 ≒ 容量 ÷ 電流。空に近づくと電圧が急に下がります。電流が大きいほど電圧降下も大きく、持ち時間も短くなります。",
        limits: &[
            "電池は、残量を蓄えるコンデンサ・残量で決まる電圧・内部抵抗の簡単なモデルです。新品 (満充電) から始まり、容量・内部抵抗・電圧の下がり方は一般的な品の概算です。",
            "扱わないもの: 温度による変化、大電流で取り出せる容量が減ること、使い込んだ劣化、自己放電。実物のコイン電池は、大きな電流 (十数 mA 以上) を流すと、この波形より早く電圧が落ち、持ち時間も短くなります。",
            "LED も簡単なモデルです (順方向電圧は約 2V)。持ち時間は、LED の明るさの感じ方 (電圧が下がってもまだ光る) までは含みません。",
        ],
        circuit: include_str!("../../../../docs/examples/coin_cell_led.schjson"),
        setup: |e| e.set_tran_settings("108k", "600"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "DC動作点を解析して、電池の電圧を見る",
                detail: "「DC動作点を解析」を押し、電圧表示で VBAT (電池の端子) を見ます。CR2032 は 3V の電池ですが、LED の電流 (約 9mA) が内部抵抗 15Ω を通るので、端子電圧は約 2.86V に下がっています。電流表示で、LED の電流も確かめましょう。",
                done: dc_fresh,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 10 にして、もう一度解析する",
                detail: "R1 を 100 から 10 にして「DC動作点を解析」。LED の電流が約 34mA に増え (LED の定格 20mA 前後を超えています)、内部抵抗の電圧降下が増えて、電池の端子電圧は約 2.48V まで下がります。コイン電池に大きな電流を流すと、電圧が大きく落ちることが分かります。",
                done: r1_changed_and_measured,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "R1 を 100 に戻して、過渡解析で持ち時間を見る",
                detail: "R1 を 100 に戻し、「過渡解析を実行」(終了 108k 秒 = 30 時間、刻み 600 秒)。VBAT は、約 24 時間はほとんど変わらず、そのあと急に下がります。約 9mA で 230mAh を使い切るのが約 25 時間なので、ほぼ計算どおりです。左の「値の決め方」の「電池の持ち時間」でも計算できます。",
                done: r1_restored_and_tran,
            },
        ],
    },
    Lesson {
        title: "11. 降圧コンバータ (12V → 5V をスイッチングで作る)",
        goal: "スイッチを高速に入り切りして電圧を下げる降圧 (バック) コンバータで、デューティ比・コイルの電流・効率を見ます",
        summary: "降圧コンバータは、電圧を「抵抗で捨てる」代わりに、スイッチを 150kHz で入り切りして、コイルとコンデンサでならして下げます。出力電圧 ≒ 入力電圧 × デューティ比 (ONの割合)。12V → 5V なら約 42%。余った電圧を熱にしないので、リニアレギュレータより効率が良く (約 90% 前後)、電圧差が大きいほど有利です。コイルが小さいと、コイルの電流が大きく揺れ、0 まで下がる (不連続) こともあります。スイッチングの高速な波形を追うので、計算に数秒かかります。",
        limits: &[
            "降圧ICは、150kHz の PWM、内部スイッチ (0.15Ω)、ソフトスタートを持つ学習用の動作モデルです。スイッチが実際に入り切りする波形は出ますが、切替の時間と損失は扱いません。",
            "扱わないもの: 過電流・過熱の保護、軽負荷での間欠動作、負荷が急に変わったときの細かい応答 (実物より遅い)。効率は、スイッチの導通損失・コイルの巻線抵抗・ダイオードの損失・制御回路の消費だけで、実物はスイッチ損失などでさらに数 % 低くなります。",
            "出力電圧は、時間が短いと目標 (約 5V) より少し低いところにいます。この回路では、解析の終わり (5ms) で約 4.8〜5V です。",
        ],
        circuit: include_str!("../../../../docs/examples/buck_converter.schjson"),
        setup: |e| e.set_tran_settings("5m", "100n"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、スイッチングと出力の立ち上がりを見る",
                detail: "「過渡解析を実行」(終了 5m、刻み 100n。数秒かかります)。VOUT と SW を選びます。SW は 0V と 12V を 150kHz で切り替え、VOUT は約 3ms かけて (ソフトスタート) 約 5V に近づきます。SW を拡大すると、ONの割合 (デューティ比) が約 40% と読めます (5 ÷ 12)。波形をクリックして測定カーソルを置くと、周期 6.7µs も測れます。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "コイル L1 を 22u にして、もう一度解析する",
                detail: "L1 を 100µ から 22µ にして「過渡解析を実行」。I(L1) (コイルの電流) を選んで拡大すると、揺れが 0.3A → 約 0.7A に増え、谷が 0A まで下がります (電流が途切れる「不連続動作」)。コイルは電流のならし役なので、小さいほど電流が大きく揺れます。出力電圧のリップルも増えます。",
                done: l1_changed_and_tran,
            },
            Step {
                tab: PanelTab::Draw,
                title: "L1 を 100u に戻し、負荷 R3 を 10 にして解析する",
                detail: "L1 を 100µ に戻し、R3 (負荷) を 20Ω から 10Ω にして「過渡解析を実行」。負荷の電流が約 2 倍になっても、VOUT は約 5V のままです (電圧を保つ)。入力側の電流 I(V1) と出力の電力を比べると、効率が出ます: 出力電力 = VOUT² ÷ R3 ≒ 2.4W、入力電力 = 12V × 入力の平均電流 ≒ 2.7W で、効率は約 89% です。リニアレギュレータなら 5 ÷ 12 = 42% 以下で、残りは熱になります。",
                done: r3_changed_and_tran,
            },
        ],
    },
    Lesson {
        title: "12. MOSFET のスイッチ (ゲート電圧とオンの深さ)",
        goal: "電圧で駆動する MOSFET (2N7000) が、ゲート電圧しきい値のあたりでオンになる様子と、電流を流しすぎたときの発熱を見ます",
        summary: "MOSFET は、ゲートの電圧だけでドレインとソースの間をオン・オフするスイッチです (ゲートにはほとんど電流が流れません)。しきい値 (2N7000 は約 2V) を超えるとオンになり始め、ゲート電圧が高いほど「オン抵抗」が下がります。オンの間のドレイン電圧は 電流 × オン抵抗 なので、大きい電流ではゼロにならず、その分が MOSFET の中で熱になります (P = I² × Ron)。「しきい値ぎりぎり」で使うと、オン抵抗が大きくて発熱します。ロジックレベル (5V / 3.3V) で十分オンにできるか、電流は定格内かを、データシートで確かめます。",
        limits: &[
            "2N7000 は、しきい値・利得・容量を持つ簡単なモデルです (レベル1)。オン抵抗の温度変化、ゲート電圧が高いときの飽和の細かい形、ボディダイオードは含みません。",
            "扱わないもの: 高速にスイッチしたときの電荷の出入り (ゲートチャージ)、ゲートを守る静電気対策。実物はゲートが静電気で壊れやすく、ゲート抵抗やプルダウン抵抗が必要です。",
            "発熱の警告は、パッケージごとの目安 (TO-92 は 0.5W など) と比べるだけです。実際の許容損失は温度や基板の放熱で変わるので、データシートで確認してください。",
        ],
        circuit: include_str!("../../../../docs/examples/mosfet_switch.schjson"),
        setup: |e| {
            e.set_tran_settings("1m", "1u");
            e.set_sweep_settings("V2", "0", "5", "0.05");
        },
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で入力と出力の波形を見る",
                detail: "「過渡解析を実行」を押し、IN と OUT を選びます。IN が 0V→5V になると、OUT は 5V から約 0.3V まで下がります (NOT 回路と同じ向きの反転です)。ゼロにならないのは、オン抵抗 (約 3Ω) に 100mA が流れるからです (0.1A × 3Ω ≒ 0.3V)。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "DCスイープで、オンになり始めるゲート電圧を見る",
                detail: "「DCスイープを実行」(電源 V2 を 0〜5V) を押し、OUT を選びます。ゲートが約 2V までは OUT は 5V のまま (オフ)、約 2.1V を超えると下がり始め、5V でほぼ底まで下がります。これがしきい値です。3V だけでは OUT は約 2.7V で、まだ「半分オン」です。3.3V のマイコンで駆動するときは、しきい値の低い (ロジックレベル) 品を選びましょう。",
                done: sweep_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "負荷 R2 を 10 にして、DC動作点を見る",
                detail: "R2 を 47 から 10 にして「DC動作点を解析」。電流が約 365mA に増え、OUT は約 1.3V までしか下がりません (オン抵抗の電圧降下が増えるため)。解析パネルに 🔥 / ⚠ の警告が出ます: R2 は消費電力が形の目安を超え、Q1 (2N7000) も約 0.5W で TO-92 の限界ぎりぎりです。2N7000 の連続電流は 200mA 程度なので、大きい負荷にはオン抵抗の小さい MOSFET (IRLZ44N など) を選びます。",
                done: r2_changed_and_measured,
            },
        ],
    },
    Lesson {
        title: "13. PNP のハイサイドスイッチ (電源側で負荷を入り切りする)",
        goal: "PNP トランジスタで、電源側 (ハイサイド) から負荷を入り切りします。NPN と逆に、入力を Low にするとオンになることと、ベース抵抗の大きさがオンの深さを決めることを見ます",
        summary: "NPN は負荷と GND の間 (ローサイド) に置き、ベースを High にするとオンになります。PNP は電源と負荷の間 (ハイサイド) に置き、ベースをエミッタ (電源) より約 0.7V 低くするとオンになります。つまり入力が Low でオン、High でオフと、向きが逆です。負荷の片方が GND につながったまま入り切りできるのが利点です。オンのときに流せる電流は、ベース電流 × hFE が上限です。ベース抵抗が大きすぎると、ベース電流が足りず、オンになりきれません。R2 (プルアップ) は、入力が浮いているときに確実にオフにするための抵抗です。",
        limits: &[
            "2N3906 は、電流増幅率 hFE=180 などを持つ簡単なモデルです。実物の hFE は個体差が大きく、電流や温度で変わります。スイッチを確実にオンにするには、データシートの小さめの hFE で、ベース電流を 5 倍ほど多めに流します。",
            "扱わないもの: 蓄積時間によるオフの遅れ、温度変化。PNP は NPN より遅く、同じ大きさなら電流も少なめです。",
            "入力は 5V 振幅の信号とし、マイコンの電源が電源 (VCC) より低いときの問題は扱っていません。入力の High が VCC に届かないと、PNP が完全にはオフになりません。そのときは NPN との 2 段にします。",
        ],
        circuit: include_str!("../../../../docs/examples/pnp_highside.schjson"),
        setup: |e| e.set_tran_settings("1m", "1u"),
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、入力が Low のときだけ出力が出るのを見る",
                detail: "「過渡解析を実行」を押し、IN と OUT を選びます。IN が 5V (High) のあいだ OUT は 0V (負荷はオフ)。IN が 0V (Low) になると、OUT が約 4.9V に上がります (負荷に電流が流れる)。NPN と入力の向きが逆です。電源側で入り切りするので、OUT は電源から Vce の分 (0.1V ほど) しか下がりません。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 47k にして、DC動作点を見る",
                detail: "R1 を 4.7k から 47k にして「DC動作点を解析」(DC では IN は 0V の Low)。ベース電流が約 23µA に減り、流せる電流は × hFE (180) で約 4mA が上限になるので、OUT は約 0.9V までしか上がりません。トランジスタが「半分だけオン」で、Vce が大きく (熱くも) なります。スイッチには、オンのときにしっかり飽和する、小さいベース抵抗を選びます。",
                done: r1_changed_and_measured,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 4.7k に戻し、負荷 R2 を 47 にして解析する",
                detail: "R1 を 4.7k に戻し、R2 (負荷) を 220 から 47 にして「DC動作点を解析」。電流が約 100mA に増え、OUT は約 4.9V のままです (ベース電流が十分あるので)。解析パネルに 🔥 / ⚠ が出ます: R3 (1206 の抵抗) は、消費電力が形の目安を超えます。トランジスタの電流が足りていても、負荷側の部品が定格を超えることがあります。",
                done: r3_changed_and_measured,
            },
        ],
    },
    Lesson {
        title: "14. オペアンプの反転増幅 (仮想接地とゲイン)",
        goal: "反転増幅回路で、ゲインが −Rf ÷ Rin で決まること、出力の向きが逆になること、反転入力が仮想的に 0V になること (仮想接地) を見ます",
        summary: "反転増幅は、入力を Rin (R1) 経由で反転入力 (−) に入れ、出力から Rf (R2) で戻す回路です。非反転入力 (+) は GND につなぎます。オペアンプは、反転入力を非反転入力 (0V) と同じ電圧に保とうとして出力を動かします。だから反転入力は常にほぼ 0V (仮想接地) で、Rin を流れる電流 = 入力 ÷ Rin が、そのまま Rf に流れます。出力 = −(Rf ÷ Rin) × 入力。この回路は 10k ÷ 1k = −10 倍 (20dB、位相は 180°)。非反転増幅との違いは、ゲインを 1 より小さくでき (Rf < Rin)、入力インピーダンスが Rin で決まる点です。",
        limits: &[
            "LM358 は、利得帯域幅積 (約 1MHz)・出力の振れ幅を含む動作モデルです。電源は理想の ±5V です。",
            "扱わないもの: 入力のバイアス電流・オフセット電圧の影響 (大きな抵抗を使うと、出力に直流のずれが出ます)、スルーレート、温度ドリフト。実機は電源ピンの近くに 0.1µF を置きます。",
            "ゲインを大きくしすぎる (Rin を小さくする) と、出力が電源に届いて頭打ちになり、仮想接地も保てなくなります (反転入力が 0V からずれます)。",
        ],
        circuit: include_str!("../../../../docs/examples/opamp_inverting.schjson"),
        setup: |e| {
            e.set_ac_settings("10", "1Meg", "10");
            e.set_tran_settings("3m", "5u");
        },
        steps: &[
            Step {
                tab: PanelTab::Analyze,
                title: "過渡解析で、反転と仮想接地を見る",
                detail: "「過渡解析を実行」を押し、IN と OUT と N (反転入力) を選びます。入力は振幅 0.1V の 1kHz 正弦波。OUT は逆向きで振幅 1V (−10 倍) です。N は、入力が ±0.1V に振れても、ほぼ 0V のまま (±1mV くらい) です。これが仮想接地です。",
                done: tran_done,
            },
            Step {
                tab: PanelTab::Analyze,
                title: "AC解析で、ゲインと位相を見る",
                detail: "「AC解析を実行」を押します。低い周波数でゲインは約 20dB (10 倍)、位相は 180° (反転) です。非反転増幅 (レッスン8) では位相は 0° でした。帯域は 1MHz ÷ (1 + 10) ≒ 90kHz あたりから下がり始めます。",
                done: ac_done,
            },
            Step {
                tab: PanelTab::Draw,
                title: "R1 を 100 にして、頭打ちを見る",
                detail: "R1 (Rin) を 1k から 100 にしてゲインを −100 倍にし、「過渡解析を実行」。出力は ±10V になるはずですが、電源の範囲に届き、マイナス側は約 −5V、プラス側は約 +3.5V で頭打ちになります (クリップ)。N を見ると、波形が削れている間は 0V から約 0.1V ずれます。オペアンプが出力を動かせなくなって、仮想接地が保てないからです。",
                done: r1_changed_and_tran,
            },
        ],
    },
];

/// The "実物との違い" notes of lesson `idx`.
#[cfg(test)]
pub(super) fn lesson_limits(idx: usize) -> &'static [&'static str] {
    LESSONS.get(idx).map_or(&[], |l| l.limits)
}

pub fn lesson_titles() -> Vec<&'static str> {
    LESSONS.iter().map(|l| l.title).collect()
}

impl SchEditor {
    /// Replace the sheet by a sample circuit (used by the board lesson). False if it cannot be read.
    pub fn load_sample(&mut self, json: &str) -> bool {
        let Ok(sch) = serde_json::from_str::<Schematic>(json) else { return false };
        self.new_file();
        self.sch = sch;
        self.reset_history();
        self.fit_pending = true;
        true
    }

    /// Load a lesson's circuit and start tracking its steps.
    pub fn start_lesson(&mut self, idx: usize) {
        let Some(lesson) = LESSONS.get(idx) else { return };
        let Ok(sch) = serde_json::from_str::<Schematic>(lesson.circuit) else {
            self.status = "レッスンの回路を読み込めませんでした".into();
            return;
        };
        self.new_file();
        self.sch = sch;
        self.reset_history();
        self.lesson_base = self.sch.clone();
        self.fit_pending = true;
        (lesson.setup)(self);
        self.lesson = Some(ActiveLesson { idx, shown: None, done: vec![false; lesson.steps.len()] });
        self.status = format!("レッスン「{}」を始めました。右の案内に沿って操作してみましょう", lesson.title);
    }

    /// Tick off the step the learner is on, once its condition holds.
    pub(super) fn update_lesson(&mut self) {
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let Some(cur) = active.done.iter().position(|d| !d) else { return };
        if (lesson.steps[cur].done)(self)
            && let Some(a) = self.lesson.as_mut() {
                a.done[cur] = true;
            }
        // open the page of the left panel that the step on hand works on
        let Some(active) = self.lesson.as_mut() else { return };
        if let Some(next) = active.done.iter().position(|d| !d)
            && active.shown != Some(next) {
                active.shown = Some(next);
                self.panel_tab = lesson.steps[next].tab;
            }
    }

    /// Floating guide window for the running lesson.
    pub fn lesson_window(&mut self, ctx: &egui::Context) {
        self.update_lesson();
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let cur = active.done.iter().position(|d| !d);
        let done = active.done.clone();
        let idx = active.idx;
        let mut close = false;
        let mut restart = false;
        let mut next = None;
        egui::Window::new(format!("レッスン: {}", lesson.title)).default_width(360.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(lesson.goal);
            ui.small(format!("進み具合: {} / {}", done.iter().filter(|d| **d).count(), done.len()));
            ui.separator();
            for (k, step) in lesson.steps.iter().enumerate() {
                let (mark, color) = if done[k] {
                    ("✔", Color32::from_rgb(60, 160, 90))
                } else if Some(k) == cur {
                    ("▶", Color32::from_rgb(220, 150, 40))
                } else {
                    ("○", Color32::from_gray(140))
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(step.title).strong().color(if Some(k) == cur { color } else { ui.visuals().text_color() }));
                });
                if Some(k) == cur {
                    ui.indent(("lesson_detail", k), |ui| {
                        ui.label(step.detail);
                    });
                }
            }
            ui.separator();
            // opened by itself once the lesson is finished (the id changes, so egui opens it again)
            egui::CollapsingHeader::new("実物との違い (このモデルの限界)")
                .id_salt(("limits", idx, cur.is_none()))
                .default_open(cur.is_none())
                .show(ui, |ui| {
                    ui.small("シミュレーションは、部品を単純なモデルにして計算します。実物の回路と、次の点が違います");
                    for l in lesson.limits {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("・");
                            ui.label(*l);
                        });
                    }
                });
            if cur.is_none() {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label(lesson.summary);
                if idx + 1 < LESSONS.len() && ui.button(format!("次のレッスンへ: {}", LESSONS[idx + 1].title)).clicked() {
                    next = Some(idx + 1);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("最初の回路に戻す").on_hover_text("回路を、このレッスンの最初の状態にします").clicked() {
                    restart = true;
                }
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        });
        if let Some(n) = next {
            self.start_lesson(n);
        } else if restart {
            self.start_lesson(idx);
        } else if close {
            self.lesson = None;
        }
    }
}
