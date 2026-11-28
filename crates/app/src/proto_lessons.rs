//! The prototype lessons ("試作のレッスン"): a sample opened in the prototype tab and a few steps, each ticked by itself when the tab
//! reaches the state it asks for (`breadboard::LessonState`): the power on, the meter's range and what it reads, the scope on a net, an
//! LED lit, a button pressed, the sketch changed. They teach what the schematic lessons cannot: plugging parts in, the meter and the
//! scope, and an Arduino sketch from blocks to code (the learning path "試作の道"). Like the schematic lessons, a step stays done.

use crate::breadboard::{BoardKind, LessonState, MeterRange, Reading, Tool};
use crate::learn::{spaced, NOTE};
use eframe::egui;
use std::collections::BTreeSet;

pub struct ProtoStep {
    pub title: &'static str,
    pub detail: &'static str,
    /// Whether the step is done now (`Memo`: what earlier frames of the lesson saw, for steps that need a change over time).
    pub done: fn(&LessonState, &mut Memo) -> bool,
}

pub struct ProtoLesson {
    /// The learning path's item id and the progress key (never reuse or rename).
    pub id: &'static str,
    pub title: &'static str,
    pub goal: &'static str,
    /// The longer explanation (folded).
    pub about: &'static str,
    pub safety: &'static str,
    /// The circuit it opens (a sample of docs/examples).
    pub sample: &'static str,
    pub steps: &'static [ProtoStep],
}

/// What a running lesson has seen so far (a blink is an LED seen lit and seen dark; a toggle is the LED's state after each release).
#[derive(Default, Debug)]
pub struct Memo {
    seen: BTreeSet<&'static str>,
    /// The first switch's state in the last frame (to find a release).
    last_switch: Option<bool>,
    /// A release happened and the LED's state after it is not looked at yet.
    released: bool,
}

impl Memo {
    fn see(&mut self, what: &'static str) {
        self.seen.insert(what);
    }

    fn saw(&self, what: &str) -> bool {
        self.seen.contains(what)
    }
}

/// A running prototype lesson.
pub struct ActiveProto {
    pub idx: usize,
    pub done: Vec<bool>,
    pub memo: Memo,
    /// The finish has been recorded.
    pub recorded: bool,
    /// How many frames in a row the step on hand has held its condition.
    steady: usize,
}

/// Frames in a row a step's condition must hold before it is ticked: right after a change (a button let go, a sketch changed) the
/// circuit takes a few frames to follow, and a state seen for one frame only is not the one the step asks for.
pub const STEADY_FRAMES: usize = 6;

impl ActiveProto {
    pub fn new(idx: usize) -> ActiveProto {
        ActiveProto { idx, done: vec![false; LESSONS[idx].steps.len()], memo: Memo::default(), recorded: false, steady: 0 }
    }

    pub fn lesson(&self) -> &'static ProtoLesson {
        &LESSONS[self.idx]
    }

    /// Looks at the tab's state: the step on hand is ticked when its condition holds (the steps before it stay ticked).
    pub fn update(&mut self, s: &LessonState) {
        // what happens over time is noted every frame, whatever the step
        note(s, &mut self.memo);
        let Some(i) = self.done.iter().position(|d| !d) else { return };
        if (self.lesson().steps[i].done)(s, &mut self.memo) {
            self.steady += 1;
            if self.steady >= STEADY_FRAMES {
                self.done[i] = true;
                self.steady = 0;
            }
        } else {
            self.steady = 0;
        }
    }

    pub fn finished(&self) -> bool {
        self.done.iter().all(|d| *d)
    }
}

/// The lesson of a learning-path item.
pub fn find(id: &str) -> Option<usize> {
    LESSONS.iter().position(|l| l.id == id)
}

/// What the lesson window asks the app to do.
pub enum ProtoUi {
    /// Open the lesson's circuit again (the steps start over).
    Restart,
    Close,
}

/// The lesson window (on the right of the prototype tab).
pub fn window(ctx: &egui::Context, a: &ActiveProto) -> Option<ProtoUi> {
    let lesson = a.lesson();
    let mut out = None;
    let mut open = true;
    let screen = ctx.content_rect();
    egui::Window::new(i18n::trf!("試作のレッスン: {}", i18n::tr_dyn(lesson.title)))
        .id(egui::Id::new("proto_lesson"))
        .open(&mut open)
        .default_pos(egui::pos2(screen.right() - 440.0, screen.top() + 80.0))
        .default_width(400.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(screen.height() * 0.75).show(ui, |ui| {
                ui.label(spaced(lesson.goal));
                ui.collapsing(i18n::tr("くわしい説明"), |ui| {
                    ui.label(spaced(lesson.about));
                });
                ui.label(egui::RichText::new(i18n::tr_dyn(lesson.safety)).color(crate::theme::warn()));
                ui.separator();
                let cur = a.done.iter().position(|d| !d);
                for (i, step) in lesson.steps.iter().enumerate() {
                    let (mark, colour) = if a.done[i] {
                        ("✔", crate::theme::ok())
                    } else if Some(i) == cur {
                        ("▶", crate::theme::HIGHLIGHT)
                    } else {
                        ("○", NOTE)
                    };
                    ui.label(egui::RichText::new(format!("{mark} {}", i18n::tr_dyn(step.title))).color(colour).strong());
                    if Some(i) == cur {
                        ui.indent(("proto_step", i), |ui| ui.label(spaced(step.detail)));
                    }
                }
                if a.finished() {
                    ui.separator();
                    ui.colored_label(crate::theme::ok(), i18n::tr("全部の手順ができました。学習パスに記録しました"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(i18n::tr("最初の回路に戻す")).on_hover_text(i18n::tr("レッスンの回路を開き直し、手順を最初からやり直します")).clicked() {
                        out = Some(ProtoUi::Restart);
                    }
                    if ui.button(i18n::tr("閉じる")).on_hover_text(i18n::tr("レッスンを終わります (回路はそのまま残ります)")).clicked() {
                        out = Some(ProtoUi::Close);
                    }
                });
            });
        });
    if !open {
        out = Some(ProtoUi::Close);
    }
    out
}

// ------------------------------------------------------------------ what the steps look at

/// The current through a part in mA while the circuit runs (0 when it is off).
fn ma(s: &LessonState, refdes: &str) -> f64 {
    s.currents.get(refdes).map_or(0.0, |i| i.abs() * 1e3)
}

fn lit(s: &LessonState, led: &str) -> bool {
    s.powered && ma(s, led) > 1.0
}

fn dark(s: &LessonState, led: &str) -> bool {
    s.powered && ma(s, led) < 0.1
}

fn switch_on(s: &LessonState) -> Option<bool> {
    s.switches.first().map(|(_, on)| *on)
}

/// The probes are on these two nets (either way round).
fn probes_on(s: &LessonState, a: &str, b: &str) -> bool {
    let (r, k) = (s.seen.red.as_deref().unwrap_or(""), s.seen.black.as_deref().unwrap_or(""));
    (r.eq_ignore_ascii_case(a) && k.eq_ignore_ascii_case(b)) || (r.eq_ignore_ascii_case(b) && k.eq_ignore_ascii_case(a))
}

/// What happens over time: D1 seen lit and dark (a blink), the knob at both ends, and the LED's state after a button is let go.
fn note(s: &LessonState, m: &mut Memo) {
    if lit(s, "D1") {
        m.see("lit");
    }
    if dark(s, "D1") {
        m.see("dark");
    }
    let on = switch_on(s);
    if m.last_switch == Some(true) && on == Some(false) {
        m.released = true;
    } else if m.released && on == Some(false) && s.powered {
        // the frame after the release: the LED as the sketch left it
        m.released = false;
        if lit(s, "D1") {
            m.see("on_after_release");
        } else if m.saw("on_after_release") {
            m.see("off_after_release");
        }
    }
    m.last_switch = on;
    if let Some((_, v)) = s.knobs.iter().find(|(r, _)| r.starts_with("VR")) {
        if *v < 0.1 {
            m.see("knob_low");
        }
        if *v > 0.9 {
            m.see("knob_high");
        }
    }
}

// ------------------------------------------------------------------ the lessons

pub const LESSONS: &[ProtoLesson] = &[BREADBOARD, METER, BLINK, SCOPE, BUTTON, POT, CODE];

const BREADBOARD: ProtoLesson = ProtoLesson {
    id: "proto_breadboard",
    title: "ブレッドボードのしくみ (差し方と照合)",
    goal: "ブレッドボードの穴のつながり方 (縦の 5 つの穴と、横に長い電源の列) を知り、回路図どおりに差すと動き、1 本の足を違う列に差すと動かなくなることを確かめます",
    about: "ブレッドボードは、はんだを使わずに部品を差して回路を組む板です。中央の溝をはさんで、縦に並んだ 5 つの穴 (a〜e、f〜j) が中でつながっています。上下の端の横に長い列は電源の列で、赤 (+) と青 (−、GND) がそれぞれ端から端までつながっています。部品の 2 本の足は、別々の列 (つながっていない穴) に差します。同じ列に差すと、部品の両端がつながってしまい (ショート)、電流が部品を通りません。この試作のタブは、回路図の部品をどの穴に差すかを自動で並べ、穴にポインタを当てると、つながっている穴が黄色に光ります。回路図と違う差し方は、赤い丸と左の一覧で知らせます。",
    safety: "安全: 実物では、電源を入れたまま部品を差し替えないでください。差し替えるときは、いったん電源を切ります。",
    sample: include_str!("../../../docs/examples/proto_basics.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れて、LED が光るのを見る",
            detail: "左の「電源」を「入」にします。回路図どおりに差してあるので、LED (D1) が光ります。穴にポインタを当てて、縦の 5 つの穴と電源の列がつながっていることも見てみましょう。",
            done: |s, _| lit(s, "D1"),
        },
        ProtoStep {
            title: "LED の足を 1 本、隣の列に差し替える",
            detail: "道具が「選択」のまま、LED (D1) をドラッグして、1 列横にずらします (またはジャンパー線の端をドラッグします)。回路図と違う差し方になり、左に赤い丸と一覧が出ます。LED の道が切れると、LED は消えます。",
            done: |s, m| {
                if !s.matches {
                    m.see("moved");
                }
                !s.matches
            },
        },
        ProtoStep {
            title: "元の列に戻して、照合が通るのを見る",
            detail: "LED を元の列にドラッグして戻します (Ctrl+Z でも戻せます)。左の照合が「回路図どおりにつながっています」に戻れば、この手順はできました。",
            done: |s, m| m.saw("moved") && s.matches,
        },
    ],
};

const METER: ProtoLesson = ProtoLesson {
    id: "proto_meter",
    title: "テスターの使い方 (電圧・電流・抵抗)",
    goal: "テスターのレンジの使い分けを、ブレッドボードの上で練習します。電圧は 2 点の間に当てる (並列)、電流は線を 1 か所外して間に入れる (直列)、抵抗は電源を切って測る、の 3 つです。わざと間違えて、電流のレンジで電源に当てるとヒューズが切れることも確かめます",
    about: "テスター (マルチメーター) は、レンジを切り替えて、電圧・電流・抵抗を測ります。電圧のレンジは、テスターの中がとても大きな抵抗なので、2 点に当てても回路にほとんど影響しません。電流のレンジは、テスターの中がほぼ 0Ω の線なので、測りたい線を 1 か所外して、その間に入れます (直列)。電源の + と − に直接当てると、テスターがショートになって大きな電流が流れ、中のヒューズが切れます (実物では、ヒューズの交換が必要です)。抵抗のレンジは、テスターが自分で小さな電流を流して測るので、電源を切ってから測ります。部品を回路につないだまま測ると、並んだほかの部品も一緒に測ることがあります。この回路は、5V から R1 (1k) と R2 (1k) で分けた MID と、R3 (330Ω) を通る LED です。",
    safety: "安全: 実物のテスターでも、電流のレンジのまま電源に当てるとヒューズが切れます。測り終わったら、レンジを電圧に戻す習慣をつけます。",
    sample: include_str!("../../../docs/examples/proto_basics.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れ、テスターを電圧のレンジにする",
            detail: "「電源」を「入」にし、上の道具の帯で「テスター」を選び、左のレンジを「電圧 V」にします。",
            done: |s, _| s.powered && s.tool == Tool::Meter && s.range == MeterRange::Volts,
        },
        ProtoStep {
            title: "赤を MID、黒を GND に当てて、電圧を読む",
            detail: "R1 と R2 がつながっている列 (MID) の穴をクリックして赤い棒を、GND の青い列の穴をクリックして黒い棒を当てます。約 2.5V が読めます。電圧は、2 点の間 (部品と並列) に当てて測ります。",
            done: |s, _| probes_on(s, "MID", "GND") && matches!(s.seen.reading, Some(Reading::Volts(v)) if (2.3..2.7).contains(&v)),
        },
        ProtoStep {
            title: "わざと間違える: 電流のレンジで、電源の + と GND に当てる",
            detail: "レンジを「電流 A」に切り替え、赤を電源の + の列 (赤い線の列)、黒を GND の列に当てます。テスターの中はほぼ 0Ω なので、電源をショートして「ヒューズが切れました」と出ます。実物でよくある失敗です。",
            done: |s, _| s.range == MeterRange::Amps && s.seen.reading == Some(Reading::Fuse),
        },
        ProtoStep {
            title: "正しく測る: LED の道を 1 か所外し、その間に当てる",
            detail: "道具を「選択」にして R3 をドラッグし、1 列横にずらします。R3 の下の足と LED の上の足が別々の列になり、LED の道が切れます (LED が消え、左に赤い丸が出ます)。道具を「テスター」に戻し、赤を R3 の下の足の列、黒を LED の上の足の列に当てます。テスターが切れた所をつなぎ、LED に流れる電流 (数 mA) が読めます。これが「直列に入れる」です。",
            done: |s, _| s.range == MeterRange::Amps && matches!(s.seen.reading, Some(Reading::Amps(i)) if (0.5e-3..30e-3).contains(&i.abs())),
        },
        ProtoStep {
            title: "電源を切り、抵抗のレンジで R1 の両端を測る",
            detail: "R3 の足を元に戻し、「電源」を「切」にして、レンジを「抵抗 Ω」にします。R1 の両端 (IN と MID の列) に当てると、1kΩ が読めます。回路につないだまま測ると、ふつうは R1 と並んだ道も一緒に測ってしまいます。この回路では、MID から R2・GND・LED・R3 を回って IN に戻る道がありますが、LED はテスターの小さな電流を逆向きには通さないので、R1 だけが読めます。並んだ道がある部品を正しく測るときは、片方の足を外します。",
            done: |s, _| !s.powered && s.range == MeterRange::Ohms && probes_on(s, "IN", "MID") && matches!(s.seen.reading, Some(Reading::Ohms(Some(r))) if (900.0..1100.0).contains(&r)),
        },
    ],
};

const BLINK: ProtoLesson = ProtoLesson {
    id: "proto_blink",
    title: "Arduino で LED を点滅させる (スケッチを動かす)",
    goal: "Arduino Nano のスケッチ (プログラム) が、ブレッドボードの回路と一緒に動くのを見て、スケッチの数字を変えると点滅の速さが変わることを確かめます",
    about: "Arduino のスケッチは、最初に 1 回動く setup() と、そのあとずっとくり返す loop() でできています。この見本は、setup() で D13 (LED_BUILTIN) を出力にし、loop() で HIGH (5V) にして 500 ミリ秒待ち、LOW (0V) にして 500 ミリ秒待つ、をくり返します。D13 には R1 (330Ω) と LED がつながっていて、HIGH のあいだ LED に約 9mA が流れて光ります。delay() の数字 (ミリ秒) を変えると、点滅の速さが変わります。試作のタブでは、電源を入れると、スケッチが回路と一緒に、実際の時間で動きます。",
    safety: "安全: マイコンのピン 1 本に流せる電流は 20mA ほどが目安です (絶対最大定格は 40mA)。LED には、必ず電流を制限する抵抗をつなぎます。",
    sample: include_str!("../../../docs/examples/arduino_blink.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れて、LED が点滅するのを見る",
            detail: "「電源」を「入」にします。LED が 0.5 秒ごとに点いたり消えたりします。左のスケッチの欄で、いま動いている行が分かります。",
            done: |s, m| s.powered && m.saw("lit") && m.saw("dark"),
        },
        ProtoStep {
            title: "delay(500) を 2 か所とも delay(100) にして、速くする",
            detail: "左のスケッチの欄で、delay(500) を 2 か所とも delay(100) に書き換えます。書き換えると、電源を入れたまま新しいスケッチで動き直し、点滅が 5 倍速くなります。",
            done: |s, _| s.powered && s.sketch.as_deref().is_some_and(|t| t.matches("delay(100)").count() >= 2 && !t.contains("delay(500)")),
        },
    ],
};

const SCOPE: ProtoLesson = ProtoLesson {
    id: "proto_scope",
    title: "オシロスコープの使い方 (電圧の時間の変化を見る)",
    goal: "オシロスコープの探り針を当てて、点滅する LED のピンの電圧が 0V と 5V を行き来する四角い波 (方形波) であること、電源の列は平らな線 (直流) であることを見ます",
    about: "テスターの電圧のレンジは、ある瞬間の (またはならした) 1 つの値しか出しません。オシロスコープは、電圧が時間とともにどう変わるかを、横を時間、縦を電圧にしたグラフ (波形) で見せます。点滅する LED のピンは、HIGH (5V) と LOW (0V) を行き来するので、四角い形の波になります。画面の左上が一番高い電圧、左下が一番低い電圧、右下が横の幅 (時間) です。実物のオシロには、横の時間の幅 (時間軸)・縦の電圧の幅・波を止めて見る「トリガ」のつまみがあります。探り針の黒いクリップ (GND) は、回路の GND につなぎます。",
    safety: "安全: 実物のオシロの探り針の GND は、コンセントのアースにつながっています。AC 100V につながった回路や、GND が電源のマイナスでない回路に当てると、ショートすることがあります。",
    sample: include_str!("../../../docs/examples/arduino_blink.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れて、道具「オシロ」を選ぶ",
            detail: "「電源」を「入」にし、上の道具の帯で「オシロ」を選びます。",
            done: |s, _| s.powered && s.tool == Tool::Scope,
        },
        ProtoStep {
            title: "探り針を、LED につながる D13 の列に当てる",
            detail: "Nano の D13 のピンと R1 がつながっている列の穴をクリックします。左に、0V と 5V を行き来する四角い波が出ます。高い所が HIGH (LED が点いている時間)、低い所が LOW です。",
            done: |s, _| matches!(&s.seen.scope, Some((_, lo, hi)) if hi - lo > 3.0),
        },
        ProtoStep {
            title: "探り針を、LED の GND 側の列に当てる",
            detail: "探り針を、LED の − 側 (カソード) の足の列に当て直します。GND につながっているので、電圧は 0V のまま変わらず、平らな線になります。電源の + のように一定の電圧 (直流) も、オシロでは平らな線です。",
            done: |s, _| matches!(&s.seen.scope, Some((_, lo, hi)) if hi - lo < 0.3),
        },
    ],
};

const BUTTON: ProtoLesson = ProtoLesson {
    id: "proto_button",
    title: "押しボタンとブロック (入力・プルアップ・シリアル)",
    goal: "押しボタンを押している間だけ LED が点くスケッチを、ブロックで読み、動かし、シリアルモニタで押した回数を見ます。最後に、ブロックを書き換えて、押していない間に点くように逆にします",
    about: "マイコンのピンを入力にすると、つながっている電圧が HIGH か LOW かを読めます。押しボタンは、押すと 2 本の足がつながる部品で、D2 と GND の間につないであります。押していない間、D2 はどこにもつながらず、電圧が決まりません (浮いている)。そこで INPUT_PULLUP (内部のプルアップ抵抗、約 30kΩ で 5V へ引き上げる) を使い、押していない間は HIGH、押すと GND とつながって LOW になるようにします。ブロックの「もし ピン D2 の入力 = LOW なら」が、押されているかを調べる所です。「シリアルに送る」は、パソコンの画面 (シリアルモニタ) に文字を送ります。ブロックを変えると、右のコードも同じに変わります。",
    safety: "安全: プルアップやプルダウンの抵抗を付けずに入力を浮かせると、手を近づけただけで値が変わることがあります。押しボタンの入力には、必ずどちらかを付けます。",
    sample: include_str!("../../../docs/examples/arduino_blocks.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れ、SW1 を押している間 LED が点くのを見る",
            detail: "「電源」を「入」にし、ブレッドボードの押しボタン SW1 をクリックして押したままにします。押している間だけ LED が点きます。ブロックの、いま動いている所が黄色の縁で光ります。",
            done: |s, _| switch_on(s) == Some(true) && lit(s, "D1"),
        },
        ProtoStep {
            title: "もう 1 回押して、シリアルモニタの回数を見る",
            detail: "SW1 をもう 1 回押します。左のスケッチの欄の下のシリアルモニタに「presses: 2」と出ます。押すたびに、変数 presses が 1 増えます。",
            done: |s, _| s.serial.contains("presses: 2"),
        },
        ProtoStep {
            title: "ブロックを書き換えて、押していない間に点くようにする",
            detail: "ブロックの「もし」の中の「出力: LED (D13) を HIGH」を LOW に、「そうでなければ」の中の LOW を HIGH に変えます (値の ▼ から選びます)。押していない間に LED が点き、押すと消えるようになります。",
            // the sample's own blocks keep the LED dark while the button is up: lit and up means the blocks were turned round
            done: |s, _| s.blocks && switch_on(s) == Some(false) && lit(s, "D1"),
        },
    ],
};

const POT: ProtoLesson = ProtoLesson {
    id: "proto_pot",
    title: "ボリュームを読んで LED の明るさを変える (アナログ入力と PWM)",
    goal: "ボリュームの電圧を analogRead で 0〜1023 の数として読み、シリアルモニタで見て、その数で analogWrite (PWM) が LED の明るさを変えるのを確かめます",
    about: "ボリューム (可変抵抗) の両端を 5V と GND に、真ん中の足を A0 につなぐと、つまみの位置で A0 の電圧が 0〜5V に変わります (分圧)。analogRead(A0) は、その電圧を 0〜1023 の数にします (5V が 1023)。analogWrite(9, 数) は、D9 を速く (490Hz) HIGH と LOW に切り替え、HIGH の割合 (デューティ) を 0〜255 で決めます (PWM)。LED は、目には割合に応じた明るさに見えます。このスケッチは、読んだ数を 4 で割って (0〜255 にして) analogWrite に渡し、数をシリアルモニタに送ります。",
    safety: "安全: ボリュームの真ん中の足を、直接 5V や GND につながないでください。つまみを端まで回すと、抵抗が 0Ω になってショートします。",
    sample: include_str!("../../../docs/examples/arduino_pot.schjson"),
    steps: &[
        ProtoStep {
            title: "電源を入れて、シリアルモニタの数を見る",
            detail: "「電源」を「入」にします。シリアルモニタに、0.2 秒ごとに A0 の読み (いまのつまみの位置で 300 ほど) が出ます。",
            done: |s, _| s.powered && s.serial.lines().filter(|l| !l.trim().is_empty()).count() >= 2,
        },
        ProtoStep {
            title: "つまみを左端と右端まで回して、明るさと数を見る",
            detail: "ブレッドボードのボリューム VR1 のつまみをドラッグして (またはホイールで)、端から端まで回します。数が 0 から 1023 まで変わり、LED の明るさが変わります。",
            done: |_, m| m.saw("knob_low") && m.saw("knob_high"),
        },
    ],
};

const CODE: ProtoLesson = ProtoLesson {
    id: "proto_code",
    title: "仕上げ: コードだけで書く (押すたびに LED を入れ替える)",
    goal: "ブロックをやめて、コード (文字) だけでスケッチを書き換えます。押しボタンを 1 回押すと LED が点いたままになり、もう 1 回押すと消える (トグル) ようにします",
    about: "ブロックで覚えたことは、文字のコードでも同じです。ただ、ブロックから文字に移るときには、書き方 (; や { } や == ) でつまずきやすいので、ここで一度、コードだけで書いてみます。ヒント: 「押された瞬間」を見つけるには、前の読みを覚えておく変数 (int last = HIGH;) を作り、「いまが LOW で、前が HIGH」のときだけ、LED の状態を入れ替えます (int on = 0; を on = !on; で入れ替え、digitalWrite(LED_BUILTIN, on);)。最後に last = いまの読み; で覚え直します。分からなければ、コードの欄の「…」から見本に戻せます。",
    safety: "コードの誤りは、スケッチの欄に赤い印と理由が出ます。行の番号と理由を読んで直します。",
    sample: include_str!("../../../docs/examples/arduino_blocks.schjson"),
    steps: &[
        ProtoStep {
            title: "スケッチの欄を「コード」に切り替える",
            detail: "スケッチの欄の上の「コード | ブロック」で「コード」を選びます。",
            done: |s, _| !s.blocks,
        },
        ProtoStep {
            title: "押すたびに LED が入れ替わるように書き換えて、確かめる",
            detail: "loop() の中を書き換え、電源を入れて SW1 を押して放します。LED が点いたままになり、もう 1 回押して放すと消えれば、できています。",
            done: |s, m| !s.blocks && m.saw("on_after_release") && m.saw("off_after_release"),
        },
    ],
};

/// Every lesson opens on the breadboard (the kind the steps are written for).
pub const BOARD: BoardKind = BoardKind::Breadboard;

impl crate::App {
    /// Opens a prototype lesson: its circuit, the prototype tab on the breadboard with the power off, and the steps from the start.
    pub(crate) fn start_proto_lesson(&mut self, idx: usize) {
        let lesson = &LESSONS[idx];
        if !self.sch.open_sample(lesson.title, lesson.sample) {
            return;
        }
        self.mode = crate::Mode::Proto;
        self.breadboard.kind = BOARD;
        self.breadboard.powered = false;
        self.breadboard.tool = Tool::Move;
        self.proto = Some(ActiveProto::new(idx));
        self.sch.status = format!("試作のレッスン「{}」を始めました。右の案内に沿って操作してみましょう", lesson.title);
    }

    /// Every frame while a prototype lesson runs: tick its steps from the prototype tab, record it when finished, draw its window.
    pub(crate) fn proto_lesson_frame(&mut self, ctx: &egui::Context) {
        if self.proto.is_none() || self.mode != crate::Mode::Proto {
            return;
        }
        let state = self.breadboard.lesson_state(&self.sch);
        let Some(a) = self.proto.as_mut() else { return };
        a.update(&state);
        if a.finished() && !a.recorded {
            a.recorded = true;
            self.progress.mark(a.lesson().id);
        }
        match window(ctx, a) {
            Some(ProtoUi::Restart) => {
                let idx = a.idx;
                self.start_proto_lesson(idx);
            }
            Some(ProtoUi::Close) => self.proto = None,
            None => {}
        }
    }
}
