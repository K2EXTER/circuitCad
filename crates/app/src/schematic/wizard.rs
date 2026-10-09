//! "値の決め方" window: calculators that explain how to choose a part value, with cautions and
//! typical failure patterns, for beginners.

use super::*;
use sim_core::{divider, led_resistor, rc_filter, regulator, smoothing_cap, transistor_switch, CalcOut};

/// One calculator: its title, what it is for, the inputs (label, default, hover help) and the function.
pub(super) struct Calc {
    pub title: &'static str,
    pub about: &'static str,
    pub inputs: &'static [(&'static str, &'static str, &'static str)],
    pub run: fn(&[f64]) -> Result<CalcOut, String>,
}

pub(super) const CALCS: &[Calc] = &[
    Calc {
        title: "LEDの電流制限抵抗",
        about: "LED を光らせるとき、直列に入れる抵抗の値を決めます。LED は電流を自分では制限しないので、抵抗が必須です",
        inputs: &[
            ("電源電圧 (V)", "5", "LED と抵抗を直列につなぐ電源の電圧"),
            ("LEDの順方向電圧 (V)", "2", "LED が光るときにかかる電圧。赤 約2V・緑 約2.2V・青/白 約3V。データシートで確認"),
            ("狙う電流 (mA)", "10", "LED に流したい電流。5〜15 mA で十分明るく、定格 (20 mA 前後) より控えめにするのが長持ちのコツ"),
        ],
        run: |v| led_resistor(v[0], v[1], v[2]),
    },
    Calc {
        title: "分圧 (抵抗2本で電圧を下げる)",
        about: "電源電圧から、決まった低い電圧を取り出す R1 (上側) と R2 (下側) を決めます。センサの信号を下げたいときなどに使います",
        inputs: &[
            ("入力電圧 (V)", "5", "分圧する前の電圧"),
            ("欲しい出力電圧 (V)", "3.3", "取り出したい電圧。入力より低い値"),
            ("R2 (Ω)", "10k", "下側の抵抗は先に決めます。1k〜100k がよく使われます (小さいほど負荷に強く、大きいほど省電力)"),
        ],
        run: |v| divider(v[0], v[1], v[2]),
    },
    Calc {
        title: "RCフィルタの抵抗",
        about: "ノイズを除く/ゆっくり変化させるための RC フィルタで、欲しいカットオフ周波数から抵抗を決めます",
        inputs: &[
            ("カットオフ周波数 (Hz)", "1k", "これより高い周波数が弱まり始める点 (ローパスの場合)"),
            ("コンデンサ (F)", "100n", "手持ちの標準値から先に決めます。100n (0.1µF) がよく使われます"),
        ],
        run: |v| rc_filter(v[0], v[1]),
    },
    Calc {
        title: "3端子レギュレータ (78xx) まわり",
        about: "電圧を一定にする 78xx 系のレギュレータに必要なコンデンサと、発熱を確かめます",
        inputs: &[
            ("入力電圧 (V)", "12", "レギュレータにつなぐ電源の電圧"),
            ("出力電圧 (V)", "5", "レギュレータの出力 (7805 なら 5)"),
            ("出力電流 (mA)", "100", "つなぐ回路が使う電流の最大値"),
        ],
        run: |v| regulator(v[0], v[1], v[2]),
    },
    Calc {
        title: "トランジスタのスイッチ (ベース抵抗)",
        about: "NPN トランジスタで LED やリレーなどをオン/オフするときの、ベース抵抗を決めます",
        inputs: &[
            ("入力の電圧 (V)", "5", "ベース抵抗につなぐ信号の High の電圧 (マイコンの出力電圧など)"),
            ("負荷の電流 (mA)", "50", "コレクタ側に流したい電流"),
            ("hFE (電流増幅率)", "100", "トランジスタの増幅率。データシートの最小値に近い値を使うと安全"),
        ],
        run: |v| transistor_switch(v[0], v[1], v[2]),
    },
    Calc {
        title: "整流の平滑コンデンサ",
        about: "トランス + ブリッジ整流のあとに入れる平滑コンデンサの容量と耐圧を、許容できるリップルから決めます",
        inputs: &[
            ("トランスの出力 (V, 実効値)", "12", "トランスの2次側の電圧 (実効値)。12V と書いてあるトランスなら 12"),
            ("負荷電流 (mA)", "150", "つなぐ回路が使う電流の最大値"),
            ("電源周波数 (Hz)", "50", "東日本 50 Hz・西日本 60 Hz"),
            ("許容リップル (V, p-p)", "1.5", "直流に残る波の大きさ。後ろにレギュレータを入れるなら 1〜2 V、そのままマイコンなどにつなぐなら小さく"),
        ],
        run: |v| smoothing_cap(v[0], v[1], v[2], v[3]),
    },
];

impl SchEditor {
    /// Opens the window on calculator `k`.
    pub fn open_wizard(&mut self, k: usize) {
        self.wiz_open = true;
        self.wiz_kind = k.min(CALCS.len() - 1);
        self.wiz_in = CALCS[self.wiz_kind].inputs.iter().map(|i| i.1.to_string()).collect();
    }

    /// The window's result for the current inputs.
    pub(super) fn wizard_result(&self) -> Result<CalcOut, String> {
        let calc = &CALCS[self.wiz_kind.min(CALCS.len() - 1)];
        let mut v = vec![];
        for (k, (label, _, _)) in calc.inputs.iter().enumerate() {
            let t = self.wiz_in.get(k).map_or("", |s| s.as_str());
            v.push(parse_value(t).ok_or_else(|| format!("「{label}」を数値で入力してください (例: 5, 3.3, 10k, 100n)"))?);
        }
        (calc.run)(&v)
    }

    pub fn wizard_window(&mut self, ctx: &egui::Context) {
        if !self.wiz_open {
            return;
        }
        let mut open = true;
        let selected_value = match self.selected {
            Some(Sel::Comp(i)) if self.sch.components.get(i).is_some_and(|c| matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor)) => Some(i),
            _ => None,
        };
        let mut apply = None;
        egui::Window::new("値の決め方").open(&mut open).default_width(460.0).vscroll(true).show(ctx, |ui| {
            ui.small("部品の値は、勘ではなく計算で決められます。入力を変えると、結果・計算のしかた・注意点・壊れ方がすぐ下に出ます");
            ui.horizontal_wrapped(|ui| {
                for (k, c) in CALCS.iter().enumerate() {
                    if ui.selectable_label(self.wiz_kind == k, c.title).clicked() && self.wiz_kind != k {
                        self.open_wizard(k);
                    }
                }
            });
            ui.separator();
            let calc = &CALCS[self.wiz_kind];
            ui.label(calc.about);
            if self.wiz_in.len() != calc.inputs.len() {
                self.wiz_in = calc.inputs.iter().map(|i| i.1.to_string()).collect();
            }
            egui::Grid::new("wiz_inputs").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (k, (label, _, tip)) in calc.inputs.iter().enumerate() {
                    ui.label(*label).on_hover_text(*tip);
                    ui.add(egui::TextEdit::singleline(&mut self.wiz_in[k]).desired_width(90.0)).on_hover_text(*tip);
                    ui.end_row();
                }
            });
            ui.separator();
            match self.wizard_result() {
                Err(msg) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), msg);
                }
                Ok(out) => {
                    egui::Grid::new("wiz_rows").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                        for (k, v) in &out.rows {
                            ui.label(k);
                            ui.strong(v);
                            ui.end_row();
                        }
                    });
                    if let Some(p) = &out.primary {
                        ui.horizontal(|ui| {
                            let b = ui.add_enabled(selected_value.is_some(), egui::Button::new(format!("選択中の部品の値を {p} にする")));
                            if b.on_hover_text("回路図で抵抗・コンデンサ・コイルを選ぶと押せます。選んだ部品の値を、この結果で置き換えます (Ctrl+Z で戻せます)").clicked() {
                                apply = selected_value.map(|i| (i, p.clone()));
                            }
                        });
                    }
                    egui::CollapsingHeader::new("計算のしかた").default_open(true).show(ui, |ui| {
                        for s in &out.steps {
                            ui.label(format!("・{s}"));
                        }
                    });
                    egui::CollapsingHeader::new("注意点").default_open(true).show(ui, |ui| {
                        for s in &out.cautions {
                            ui.colored_label(Color32::from_rgb(210, 150, 40), format!("⚠ {s}"));
                        }
                    });
                    egui::CollapsingHeader::new("こうなると壊れる/おかしくなる (故障パターン)").default_open(false).show(ui, |ui| {
                        for s in &out.faults {
                            ui.label(format!("・{s}"));
                        }
                        ui.small("回路図で実際に抵抗を外したり値を変えたりして、DC解析の結果がどう変わるか試してみましょう");
                    });
                }
            }
        });
        if let Some((i, v)) = apply
            && let Some(c) = self.sch.components.get_mut(i)
        {
            c.value = v;
        }
        if !open {
            self.wiz_open = false;
        }
    }
}
