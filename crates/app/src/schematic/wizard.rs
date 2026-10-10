//! "値の決め方" window: calculators that explain how to choose a part value, with cautions and
//! typical failure patterns, for beginners.

use super::*;
use sim_core::{boost_design, mosfet_switch, opamp_gain, buck_design, battery_life, zener_regulator, divider, heat_design, led_resistor, rc_filter, regulator, smoothing_cap, transistor_switch, CalcOut};

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
        title: "熱 (温度と放熱板)",
        about: "(入力 − 出力) × 電流 が熱になるレギュレータで、部品の温度と、放熱板が要るかを見積もります",
        inputs: &[
            ("入力電圧 (V)", "12", "レギュレータにつなぐ電源の電圧"),
            ("出力電圧 (V)", "5", "レギュレータの出力"),
            ("出力電流 (mA)", "500", "流す電流の最大値"),
            ("熱抵抗 θja (℃/W)", "50", "放熱板なしで、部品の中から空気までの熱の逃げにくさ。データシートで確認 (TO-220 は約 50)"),
            ("熱抵抗 θjc (℃/W)", "5", "部品の中からケースまで。放熱板をつけるときの計算に使う (TO-220 は約 5)"),
            ("周囲温度 (℃)", "25", "部品のまわりの空気の温度。箱の中では 40〜50 ℃ で考える"),
            ("最大接合温度 (℃)", "125", "部品の中の温度の上限。データシートで確認 (78xx は 125 ℃)"),
        ],
        run: |v| heat_design(v[0], v[1], v[2], v[3], v[4], v[5], v[6]),
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
    Calc {
        title: "降圧コンバータのコイルとコンデンサ",
        about: "スイッチングで電圧を下げる降圧 (バック) コンバータの、デューティ比・コイル・出力コンデンサを、許せるリップルから決めます",
        inputs: &[
            ("入力電圧 (V)", "12", "コンバータにつなぐ電源の電圧"),
            ("出力電圧 (V)", "5", "作りたい電圧 (入力より低い値)"),
            ("出力電流 (mA)", "500", "つなぐ回路が使う電流の最大値"),
            ("スイッチング周波数 (kHz)", "150", "降圧ICのスイッチングの周波数。ICのデータシートの値 (多くは 100〜500kHz)"),
            ("コイル電流の揺れ (出力電流の %)", "40", "コイルの電流がどれだけ上下するか。20〜40 % が定番"),
            ("出力電圧のリップル (mV)", "30", "出力に残る細かい波の大きさ (p-p)。マイコン用なら 30〜50mV くらい"),
        ],
        run: |v| buck_design(v[0], v[1], v[2], v[3], v[4], v[5]),
    },
    Calc {
        title: "昇圧コンバータのコイルとコンデンサ",
        about: "スイッチングで電圧を上げる昇圧 (ブースト) コンバータの、デューティ比・コイル・出力コンデンサを決めます。電池 1〜2 本から 5V を作るときなどに使います (効率は 85 % と仮定します)",
        inputs: &[
            ("入力電圧 (V)", "5", "コンバータにつなぐ電源の電圧。電池なら、電圧が下がりきったときの値で見積もると安全です"),
            ("出力電圧 (V)", "12", "作りたい電圧 (入力より高い値)"),
            ("出力電流 (mA)", "200", "つなぐ回路が使う電流の最大値"),
            ("スイッチング周波数 (kHz)", "200", "昇圧ICのスイッチングの周波数。ICのデータシートの値"),
            ("コイル電流の揺れ (入力電流の %)", "40", "コイルの電流がどれだけ上下するか。20〜40 % が定番"),
            ("出力電圧のリップル (mV)", "50", "出力に残る細かい波の大きさ (p-p)"),
        ],
        run: |v| boost_design(v[0], v[1], v[2], v[3], v[4], v[5]),
    },
    Calc {
        title: "MOSFET のスイッチ (発熱とゲート抵抗)",
        about: "N チャネル MOSFET で負荷をオン/オフするときの、オンのときの電圧降下・発熱・ゲート抵抗とプルダウン抵抗の目安を調べます",
        inputs: &[
            ("ゲートの駆動電圧 (V)", "5", "マイコンの出力 (High) の電圧。3.3V や 5V が多いです"),
            ("しきい値電圧 Vgs(th) (V)", "2", "MOSFET がオンになり始めるゲート電圧。データシートで確認 (2N7000 は約 2V)"),
            ("負荷の電流 (mA)", "100", "ドレインに流したい電流"),
            ("オン抵抗 Rds(on) (mΩ)", "5000", "データシートの値 (ゲート電圧が近い条件のもの)。2N7000 は約 5Ω = 5000mΩ、IRLZ44N は約 22mΩ"),
            ("負荷の電源電圧 (V)", "5", "負荷につなぐ電源の電圧"),
        ],
        run: |v| mosfet_switch(v[0], v[1], v[2], v[3], v[4]),
    },
    Calc {
        title: "オペアンプの増幅 (抵抗の値)",
        about: "オペアンプで信号を大きくするときの、帰還抵抗と R2 を決め、使える帯域と出力が電源で頭打ちにならないかを確かめます",
        inputs: &[
            ("欲しいゲイン (倍)", "11", "出力 ÷ 入力。反転増幅でも正の数で入れます (出力の向きが逆になります)"),
            ("帰還抵抗 Rf (Ω)", "10k", "出力から戻す抵抗。1k〜1M くらいで、先に決めます (10k がよく使われます)"),
            ("反転増幅にする (0 = 非反転, 1 = 反転)", "0", "非反転は入力が + 側、反転は入力が − 側 (出力の向きが逆になる)"),
            ("利得帯域幅積 (kHz)", "1000", "オペアンプのデータシートの GBW。LM358 は約 1MHz = 1000kHz"),
            ("電源電圧 ± (V)", "5", "オペアンプの電源 (両電源の片側の値)"),
            ("入力の振幅 (mV)", "100", "入力信号の振幅 (ピーク)"),
        ],
        run: |v| opamp_gain(v[0], v[1], v[2], v[3], v[4], v[5]),
    },
    Calc {
        title: "電池の持ち時間",
        about: "電池の容量と負荷の電流から、持ち時間の目安と、内部抵抗による電圧の下がりを調べます。コイン電池に大きな電流を流してよいかの判断にも使えます",
        inputs: &[
            ("電池の容量 (mAh)", "230", "電池の表示容量。CR2032 は約 230、単3アルカリは約 2500、18650 は約 2600"),
            ("負荷の平均電流 (mA)", "9", "つなぐ回路が流す電流の平均。点滅するものは、平均を見積もります"),
            ("電池の内部抵抗 (Ω)", "15", "CR2032 は約 15、アルカリは約 0.15、リチウムイオンは約 0.06"),
            ("電池の電圧 (V)", "3", "新しいときの電圧"),
            ("使える割合 (%)", "70", "表示容量のうち、実際に使える割合。電流が大きいほど減ります。70% くらいで見積もると安全です"),
        ],
        run: |v| battery_life(v[0], v[1], v[2], v[3], v[4]),
    },
    Calc {
        title: "ツェナーダイオードで電圧を一定にする",
        about: "ツェナーダイオードと抵抗で、決まった電圧 (5.1V など) を取り出します。小さな電流の基準電圧に向いています",
        inputs: &[
            ("入力電圧 (V)", "12", "ツェナーと抵抗をつなぐ電源の電圧"),
            ("ツェナー電圧 (V)", "5.1", "ツェナーダイオードの電圧 (1N4733A なら 5.1)"),
            ("負荷の電流 (最大, mA)", "20", "取り出す電流の最大値"),
            ("ツェナーに残す電流 (mA)", "5", "ツェナーが働くのに最低限必要な電流。データシートの試験電流 (5〜10mA くらい)"),
        ],
        run: |v| zener_regulator(v[0], v[1], v[2], v[3]),
    },
    Calc {
        title: "基板の配線の太さ (電流から)",
        about: "基板の銅の配線に流せる電流から、必要な線幅を決めます。電源やモーターの線を信号線と同じ細さにすると、熱を持って焼けることがあります",
        inputs: &[
            ("流す電流 (A)", "1", "その配線に流れる最大の電流。つなぐ部品のデータシートの最大電流や、電源の容量から見積もります"),
            ("許す温度上昇 (℃)", "10", "配線が周囲より何度まで熱くなってよいか。10℃ が一般的で、余裕を見るなら 5〜10℃"),
            ("銅箔の厚さ (µm)", "35", "ふつうの2層基板は 35µm (1oz)。厚銅は 70µm (2oz)。発注先の仕様で確認します"),
        ],
        run: |v| trace_width(v[0], v[1], v[2]),
    },
];

/// Track width for a current (IPC-2221), with cautions and failure patterns, as a calculator result.
fn trace_width(amps: f64, rise: f64, um: f64) -> Result<CalcOut, String> {
    use pcb_core::current::{current_for_width, round_up_width, width_for_current, CopperSpec};
    for (name, v) in [("電流", amps), ("温度上昇", rise), ("銅箔の厚さ", um)] {
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("{name}は 0 より大きい数にしてください"));
        }
    }
    let ext = CopperSpec { thickness_um: um, rise_c: rise, external: true };
    let int = CopperSpec { external: false, ..ext };
    let (w_ext, w_int) = (width_for_current(amps, &ext), width_for_current(amps, &int));
    let rec = round_up_width(w_ext);
    let mut o = CalcOut::default();
    o.rows = vec![
        ("計算上の線幅 (表面・裏面)".into(), format!("{w_ext:.3} mm")),
        ("使う線幅 (0.05mm 刻みで切り上げ)".into(), format!("{rec:.2} mm")),
        ("内層に配線する場合".into(), format!("{w_int:.2} mm (内層は熱が逃げにくい)")),
        ("その幅で流せる電流".into(), format!("{:.2} A", current_for_width(rec, &ext))),
    ];
    o.steps = vec![
        "IPC-2221 の式: I = k × ΔT^0.44 × A^0.725 (A: 配線の断面積 [mil²]、ΔT: 温度上昇、k: 表面 0.048・内層 0.024)".into(),
        format!("断面積 A = (I ÷ (k × ΔT^0.44))^(1/0.725)。幅 = A ÷ 銅箔の厚さ ({um} µm = {:.2} mil) = {w_ext:.3} mm", um / 25.4),
    ];
    if rec > 2.0 {
        o.cautions.push("2 mm を超える太さです。配線でなく、ゾーン (銅を面で塗る) や基板の両面に同じ配線を重ねることを検討しましょう".into());
    }
    o.cautions.push("線幅は配線のいちばん細い所で決まります。パッドにつながる部分や、ビア・コネクタの手前で細くなっていないか確認します".into());
    o.cautions.push("ビア1個に流せる電流は小さい (1A 前後)。大電流の線を層替えするときは、ビアを複数並べます".into());
    o.cautions.push("短時間だけ流れる電流 (モーターの起動など) と、ずっと流れる電流を分けて考えます。この式は、ずっと流れる電流が目安です".into());
    o.cautions.push("線幅を太くすると、隣の配線との間隔が減ります。基板エディタの「配線の太さ」で自動で太くしたあと、DRC で間隔を確かめましょう".into());
    o.faults = vec![
        "細すぎる配線: 熱を持って基板が変色し、最悪は銅が焼き切れて (ヒューズのように) 断線します".into(),
        "ビアが足りない/細い: 大電流の途中で熱を持ちます。電源の層替えにビアを複数使います".into(),
        "パッドへの接続部だけ細い: そこが一番熱くなる所 (ボトルネック) になります".into(),
        "銅箔が計画より薄い (安い発注の既定は 35µm): 流せる電流が減ります。仕様を確認しましょう".into(),
    ];
    Ok(o)
}

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
        egui::Window::new("値の決め方").open(&mut open).default_size([480.0, 600.0]).vscroll(true).show(ctx, |ui| {
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
