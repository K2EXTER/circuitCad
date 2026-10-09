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
                detail: "「実験: 温度とばらつき」の「ばらつき解析を実行」を押します。実際の抵抗は 5% くらいの誤差があるので、MID は 2.5V ぴったりにはなりません。どの抵抗が効いているかも出ます。",
                done: mc_done,
            },
        ],
    },
    Lesson {
        title: "2. LED と電流制限抵抗",
        goal: "LED に抵抗が必要な理由と、温度の影響を確かめます",
        summary: "LED は電圧がわずかに上がるだけで電流が急に増えます。そのため抵抗で電流を決めます (電流 = (電源 − LED の電圧) ÷ 抵抗)。また、ダイオードの電圧は温度が上がると約 2mV/℃ 下がります。",
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
                detail: "「実験: 温度とばらつき」の「温度スイープを実行」を押します。温度が上がると LED の電圧が下がり、電流がわずかに増えるのがグラフで分かります。",
                done: temp_sweep_done,
            },
        ],
    },
    Lesson {
        title: "3. RC ローパスフィルタ",
        goal: "抵抗とコンデンサで、高い周波数だけを弱める回路の特性を見ます",
        summary: "カットオフ周波数は fc = 1 ÷ (2π × R × C) です。fc で出力は −3dB (約 0.7 倍) になり、それより高い周波数は 1 桁上がるごとに 1/10 になります。ノイズ除去や、音の高音を削るトーンコントロールの基本です。",
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
                detail: "C1 を 159n に戻し、「実験: 温度とばらつき」で「測るもの」を「カットオフ周波数 (AC)」、「測る点」をフィルタの出力 (R1 と C1 の間) にして「ばらつき解析を実行」。許容差 5%/10% の部品だと、カットオフは 1kHz ぴったりにならず 1 割ほど広がります。フィルタを正確に作るには、許容差の小さい部品が必要です。",
                done: mc_cutoff_done,
            },
        ],
    },
    Lesson {
        title: "4. トランジスタのスイッチ",
        goal: "トランジスタを使った反転回路 (NOT) の動きと、切り替わる電圧を見ます",
        summary: "ベースに十分な電流を流すとトランジスタがオンになり、出力が 0V に引き下げられます。入力が約 0.7V を超えたあたりで切り替わります。デジタル回路の基本動作です。",
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
                detail: "R1 を 20 に戻し、「実験: 温度とばらつき」で「測るもの」を「ステップ応答」、「指標」を「オーバーシュート」、「測る点」を C1 の電圧にして「ばらつき解析を実行」。部品の誤差で、行き過ぎの大きさが何 % ずれるかが分かります。",
                done: mc_overshoot_done,
            },
        ],
    },
    Lesson {
        title: "7. リチウムイオン電池の充電 (TP4056)",
        goal: "充電ICの内部をのぞかず、「外から見た動き」で、電池の充電の3段階を見ます",
        summary: "リチウムイオン電池は、空に近いときはそっと (プリチャージ)、普通は一定の電流で (定電流)、満タン近くでは電圧を 4.2V に保ちながら電流を減らして (定電圧) 充電します。TP4056 は PROG 端子の抵抗だけで電流を決められます (電流 = 1200 ÷ 抵抗)。実際の電池は数千 F 相当の巨大なコンデンサのようなものなので、ここでは 470µF の小さなコンデンサを電池の代わりにして、時間を縮めています。",
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
];

pub fn lesson_titles() -> Vec<&'static str> {
    LESSONS.iter().map(|l| l.title).collect()
}

impl SchEditor {
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
            ui.small("操作は左パネルで行います。部品の値の変更は「作図」タブ、解析の実行は「解析」タブです (手順に合わせて自動で切り替わります)");
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
