//! The guided lesson for the board editor: from a schematic to the files to order. Like the
//! schematic lessons, each step ticks itself off when the board reaches the matching state.

use super::*;

pub const TITLE: &str = "基板編. 回路図から基板をつくる";
const CIRCUIT: &str = include_str!("../../../../docs/examples/regulator_led.schjson");

/// The second lesson: copper that is too thin, from a finished sample project (`docs/examples/thin_power_line.ccproj`).
pub const COPPER_TITLE: &str = "基板編2. 配線の電圧降下";
pub const COPPER_ID: &str = "board_copper";
const COPPER_PROJECT: &str = include_str!("../../../../docs/examples/thin_power_line.ccproj");

pub(super) struct BoardLesson {
    /// 0 = from a schematic to the files to order, 1 = the copper lesson.
    which: usize,
    done: Vec<bool>,
}

struct Step {
    title: &'static str,
    detail: &'static str,
    done: fn(&BoardEditor) -> bool,
}

fn parts_imported(e: &BoardEditor) -> bool {
    !e.board.parts.is_empty()
}

fn parts_on_board(e: &BoardEditor) -> bool {
    parts_imported(e) && e.board.outline.len() >= 3 && e.board.pads.iter().all(|p| inside_polygon(&e.board.outline, p.pos))
}

fn routed(e: &BoardEditor) -> bool {
    parts_on_board(e) && !e.board.tracks.is_empty() && ratsnest(&e.board).is_empty()
}

fn drc_clean(e: &BoardEditor) -> bool {
    routed(e) && e.drc_ran && e.violations.is_empty()
}

fn order_opened(e: &BoardEditor) -> bool {
    drc_clean(e) && e.order.open
}

/// Where the board tools stop: shown as "実物との違い".
const BOARD_LIMITS: &[&str] = &[
    "自動配置は「叩き台」です。つながりの強い部品を近くに並べるだけで、部品の高さ、コネクタを基板の端に置くこと、放熱、ノイズを出す部品と弱い部品の距離は考えません。",
    "自動配線は、2層・45°の格子の上で、設計ルール (線の間隔) を守る配線だけを採用します。電流が大きい線の太さ (線幅は一律です)、電源とグランドの引き方、ノイズ対策、信号の速さ (インピーダンス) は考えません。大電流や高速の回路は、手で引き直してください。",
    "DRC は、設計ルールを守っているかの機械的な検査です。回路が正しく動くか、ノイズや熱が大丈夫かは調べません。それは回路図のシミュレーションと、実際に作っての確認で調べます。",
    "発注先ごとの最小の線幅・穴径などは目安です。発注の前に、必ず発注先のページで最新の条件を確かめてください。",
];

fn copper_applied(e: &BoardEditor) -> bool {
    e.post_layout_fresh
}

fn currents_declared(e: &BoardEditor) -> bool {
    !e.board.net_currents.is_empty()
}

fn copper_wide_enough(e: &BoardEditor) -> bool {
    let conn = drc::connectivity(&e.board);
    let report = pcb_core::current::net_widths(&e.board, &conn, &pcb_core::current::CopperSpec::default());
    !report.is_empty() && report.iter().all(|w| !w.too_thin())
}

const COPPER_STEPS: &[Step] = &[
    Step {
        title: "配線の抵抗を入れて、電圧が落ちるのを見る",
        detail: "左パネルの「高度な配線」→「配線の太さ (電流から)」を開き、「配線の電圧降下」の「シミュレーションに反映」を押します。そのあと上の「回路図」タブで「解析」→「DC動作点を解析」を押すと、負荷 R1 の + 側が 5 V ではなく約 4.5 V になります。細い配線 (0.25 mm、長さ 60 mm) の抵抗で、電圧が落ちたのです",
        done: copper_applied,
    },
    Step {
        title: "DC解析の電流を入れる",
        detail: "基板に戻り、「配線の太さ (電流から)」の「DC解析の電流を入れる」を押します。DC解析で求めた電流 (約 2.3 A) が、VIN と GND の欄に入ります。必要な線幅 (約 1.1 mm) と、今の線幅 (0.25 mm) が赤く出ます",
        done: currents_declared,
    },
    Step {
        title: "足りない配線を太くする",
        detail: "「足りないネットをすべて推奨幅にする」を押します。配線が必要な太さになります。太くしたら、間隔が足りているか、「DRC を実行」でも確かめましょう",
        done: copper_wide_enough,
    },
    Step {
        title: "もう一度反映して、電圧が戻るのを確かめる",
        detail: "もう一度「シミュレーションに反映」を押し、回路図で「DC動作点を解析」を押します。R1 の + 側が約 4.9 V に戻ります。残りの差は、まだ配線にある抵抗です。もっと太く・短くすれば、さらに小さくなります",
        done: copper_applied,
    },
];

const STEPS: &[Step] = &[
    Step {
        title: "回路図から取り込む",
        detail: "左パネルの「回路図から取り込み」を押します。回路図の部品が基板の右側に並び、部品どうしをつなぐべき所に黄色の線 (未配線、ラッツネスト) が出ます。回路図の「つながり」が、そのまま基板の配線の指示になります。",
        done: parts_imported,
    },
    Step {
        title: "部品を並べて、基板の外形を決める",
        detail: "「自動配置 → 部品を並べて外形を決める」を押します。つながりの強い部品が近くなるように並び、外形も部品に合わせて決まります。叩き台なので、気になる部品はドラッグで動かしてみましょう (R キーで回転)。黄色の線が短くなるように置くのがコツです。",
        done: parts_on_board,
    },
    Step {
        title: "配線する",
        detail: "「自動配線 → 全部」を押します。黄色の線が銅の配線 (赤が表面、青が裏面) に変わります。手で引くなら「配線」ツールでパッドからパッドへクリックします。通せなかった線が残ったら、部品を動かしてからやり直しましょう。",
        done: routed,
    },
    Step {
        title: "DRC で配線のルール違反を調べる",
        detail: "「DRC を実行」を押します。線が近すぎる、基板の端にはみ出している、などがないか調べます。違反が出たら、一覧をクリックするとその場所に飛びます。直して、0 件になるまで繰り返します。",
        done: drc_clean,
    },
    Step {
        title: "発注の準備を開く",
        detail: "「発注の準備…」を押します。発注先を選び、チェックと出来上がりの確認 (書き出すガーバーファイルを読み戻した絵) をして、製造ファイルを書き出せます。ここまで来れば、回路図から実際の基板を注文するところまでの流れがつながりました。",
        done: order_opened,
    },
];

impl BoardEditor {
    pub fn start_lesson(&mut self) {
        self.new_file();
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 0, done: vec![false; STEPS.len()] });
        self.status = format!("レッスン「{TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    /// The project (schematic and board) the copper lesson works on.
    pub fn copper_lesson_project() -> &'static str {
        COPPER_PROJECT
    }

    /// Starts the copper lesson on the board that was just opened (the app loads the project first).
    pub fn start_copper_lesson(&mut self) {
        self.board.net_currents.clear();
        self.post_layout_fresh = false;
        self.drc_ran = false;
        self.lesson = Some(BoardLesson { which: 1, done: vec![false; COPPER_STEPS.len()] });
        self.status = format!("レッスン「{COPPER_TITLE}」を始めました。右の案内に沿って操作してみましょう");
    }

    fn lesson_steps(&self) -> &'static [Step] {
        if self.lesson.as_ref().is_some_and(|l| l.which == 1) { COPPER_STEPS } else { STEPS }
    }

    /// The sample circuit this lesson imports.
    pub fn lesson_circuit() -> &'static str {
        CIRCUIT
    }

    /// The progress key of the running board lesson once every step is ticked.
    pub fn finished_board_lesson(&self) -> Option<&'static str> {
        let l = self.lesson.as_ref().filter(|l| l.done.iter().all(|d| *d))?;
        Some(if l.which == 1 { COPPER_ID } else { "board" })
    }

    #[cfg(test)]
    pub fn lesson_progress(&mut self) -> usize {
        self.update_lesson();
        self.lesson.as_ref().map_or(0, |l| l.done.iter().filter(|d| **d).count())
    }

    fn update_lesson(&mut self) {
        let Some(l) = self.lesson.as_ref() else { return };
        let Some(cur) = l.done.iter().position(|d| !d) else { return };
        if (self.lesson_steps()[cur].done)(self)
            && let Some(l) = self.lesson.as_mut() {
                l.done[cur] = true;
            }
    }

    pub fn lesson_window(&mut self, ctx: &egui::Context) {
        self.update_lesson();
        let Some(l) = self.lesson.as_ref() else { return };
        let done = l.done.clone();
        let copper = l.which == 1;
        let steps = self.lesson_steps();
        let cur = done.iter().position(|d| !d);
        let mut close = false;
        egui::Window::new(format!("レッスン: {}", if copper { COPPER_TITLE } else { TITLE })).default_width(360.0).anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(268.0, -56.0)).show(ctx, |ui| {
            ui.label(if copper {
                "電源と負荷が遠く、配線が細い基板です。配線の抵抗で電圧がどれだけ落ちるかを、シミュレーションで見て、電流から必要な太さを決めて直します"
            } else {
                "回路図で描いた回路を、基板のパターンにして、発注用のデータを作るところまでを一通り体験します"
            });
            ui.small(format!("進み具合: {} / {}", done.iter().filter(|d| **d).count(), done.len()));
            ui.separator();
            for (k, step) in steps.iter().enumerate() {
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
                    ui.indent(("board_lesson_detail", k), |ui| {
                        ui.label(step.detail);
                    });
                }
            }
            ui.separator();
            if !copper {
            egui::CollapsingHeader::new("できること・できないこと (ツールの限界)")
                .id_salt(("board_limits", cur.is_none()))
                .default_open(cur.is_none())
                .show(ui, |ui| {
                    ui.small("自動の機能は便利ですが、万能ではありません。次の点は、人が確かめます");
                    for l in BOARD_LIMITS {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("・");
                            ui.label(*l);
                        });
                    }
                });
            }
            if cur.is_none() && copper {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("配線は銅の抵抗を持ち、電流が大きいほど、細く長いほど、電圧が落ちます。流れる電流から必要な太さを決め (IPC-2221)、シミュレーションで確かめる、という順で考えます。この見積もりはDCの電流だけで、スイッチングの瞬間の大きな電流や、配線のインダクタンスは入っていません。余裕を見て、実機でも測りましょう。");
            } else if cur.is_none() {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "レッスン完了!");
                ui.label("基板づくりの流れは「回路図 → 取り込み → 配置 → 配線 → DRC → 発注データ」です。DRC に通らないと発注データは書き出せないので、配線のあとは必ず DRC を実行する習慣をつけましょう。実際に注文するときは、発注先のページで最新の条件を確かめてください。");
            }
            ui.separator();
            if ui.button("閉じる").clicked() {
                close = true;
            }
        });
        if close {
            self.lesson = None;
        }
    }
}
