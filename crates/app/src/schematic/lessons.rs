//! Guided lessons: a sample circuit plus a short list of things to try. Each step is ticked off
//! by itself when the editor reaches the matching state, so the learner is never asked to press
//! a "next" button after doing what was asked.

use super::*;

pub(super) struct ActiveLesson {
    pub(super) idx: usize,
    /// Steps that have been completed (once done, a step stays done).
    pub(super) done: Vec<bool>,
}

struct Step {
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

fn c1_changed_and_ac(e: &SchEditor) -> bool {
    changed(e, "C1") && ac_done(e) && e.wave_mode == WaveMode::Ac
}

fn tran_done(e: &SchEditor) -> bool {
    e.tran.as_ref().is_some_and(|t| t.outcome.is_ok())
}

fn c1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "C1") && tran_done(e)
}

fn sweep_done(e: &SchEditor) -> bool {
    e.sweep.as_ref().is_some_and(|s| !s.temp && s.outcome.is_ok())
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
                title: "DC動作点を解析する",
                detail: "左のパネルの「DC動作点を解析」を押します。回路図の各点に電圧が出ます。真ん中の点 (MID) は電源 5V のちょうど半分、2.5V のはずです。配線の色は、低い電圧が青、高い電圧が赤です。",
                done: dc_ok,
            },
            Step {
                title: "R2 を 30k にして、もう一度解析する",
                detail: "R2 をクリックで選び、左の「値」を 30k にして、もう一度「DC動作点を解析」。MID は 5 × 30k ÷ (10k+30k) = 3.75V に変わります。",
                done: r2_changed_and_measured,
            },
            Step {
                title: "「自動で再計算」をオンにして、スライダーを動かす",
                detail: "「自動で再計算」にチェックを入れ、R2 を選んで「スライダー」を左右に動かしてみましょう。動かすたびに MID の電圧が変わります。式どおりに変わっているか確かめてください。",
                done: live_on,
            },
            Step {
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
                title: "DC動作点を解析して、電流を見る",
                detail: "「DC動作点を解析」を押し、「電流表示」をオンにします。R1 と LED の下に電流が出ます。LED の電圧は 2V くらい、電流は (5−2)÷330 で約 9mA です。",
                done: currents_shown,
            },
            Step {
                title: "R1 を 100 にして、電流の変化を見る",
                detail: "R1 を選んで値を 100 にし、もう一度解析します。電流が約 3 倍になります。抵抗を小さくしすぎると LED が壊れる理由です (一般的な LED は 20mA 程度まで)。",
                done: r1_changed_and_measured,
            },
            Step {
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
                title: "AC解析を実行する",
                detail: "「AC解析を実行」を押します。下にボード線図 (上が大きさ dB、下が位相) が出ます。この回路は R=1k、C=159n なので fc = 1 ÷ (2π×1k×159n) ≒ 1kHz。1kHz のあたりで 0dB から −3dB に下がり始めます。",
                done: ac_done,
            },
            Step {
                title: "C1 を 1.59u にして、もう一度解析する",
                detail: "C1 の値を 1.59u (10倍) にして「AC解析を実行」。カットオフが 1/10 の約 100Hz に動きます。コンデンサが大きいほど低い周波数から信号を弱めます。",
                done: c1_changed_and_ac,
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
                title: "過渡解析で入力と出力の波形を見る",
                detail: "「過渡解析を実行」を押します。入力 (IN) が 0V→5V になると、出力 (OUT) は 5V→0V に反転します。信号の High/Low が逆になる、これが NOT です。",
                done: tran_done,
            },
            Step {
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
                title: "過渡解析を実行する",
                detail: "555 は時間とともに状態が変わる回路なので、DC動作点ではなく過渡解析で見ます。終了 3 秒、刻み 2m のまま「過渡解析を実行」。OUT の波形が High/Low を繰り返します。TH (コンデンサ C1 の電圧) が三角っぽく上下するのも見てみましょう。R1=1k、R2=68k、C1=10u だと約 1Hz です。",
                done: tran_done,
            },
            Step {
                title: "C1 を 1u にして、もう一度解析する",
                detail: "C1 を 1u にして「過渡解析を実行」。周期が 1/10 になり、波形が 10 倍の速さで繰り返します。",
                done: c1_changed_and_tran,
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
        self.lesson = Some(ActiveLesson { idx, done: vec![false; lesson.steps.len()] });
        self.status = format!("レッスン「{}」を始めました。右の案内に沿って操作してみましょう", lesson.title);
    }

    /// Tick off the step the learner is on, once its condition holds.
    pub(super) fn update_lesson(&mut self) {
        let Some(active) = self.lesson.as_ref() else { return };
        let Some(lesson) = LESSONS.get(active.idx) else { return };
        let Some(cur) = active.done.iter().position(|d| !d) else { return };
        if (lesson.steps[cur].done)(self) {
            if let Some(a) = self.lesson.as_mut() {
                a.done[cur] = true;
            }
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
