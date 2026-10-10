//! The analysis part of the side panel: DC, transient, AC, sweep, noise, temperature / tolerance and the results list.

use super::*;

impl SchEditor {
    pub(super) fn panel_dc(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("DC動作点を解析").on_hover_text("時間が経って落ち着いたときの各点の電圧と電流を求めます。回路図の各ピンの横に電圧が出るので、「思った電圧になっているか」の確認に一番よく使います").clicked() {
                self.run_dc();
            }
            ui.checkbox(&mut self.show_v, "電圧表示").on_hover_text("DC解析の結果を回路図に重ねて表示します。配線は電圧が低いほど青、高いほど赤に色分けされます");
            ui.checkbox(&mut self.show_i, "電流表示").on_hover_text("各部品を流れる電流を数値と矢印 (電流の向き) で表示します。「どこに電流が流れているか」の理解に役立ちます");
        });
        ui.checkbox(&mut self.live, "自動で再計算").on_hover_text(
            "回路を変えるたびに、開いている解析 (DC / 過渡 / AC) をやり直します。
部品の「スライダー」と組み合わせると、値を動かしながら結果がどう変わるかを実験できます。
重い回路では動きがもたつくのでオフにしてください",
        );
        ui.horizontal(|ui| {
            ui.label("回路の温度");
            ui.add(egui::TextEdit::singleline(&mut self.temp_c).desired_width(36.0))
                .on_hover_text("DC・過渡・AC・DCスイープで使う温度 (摂氏、標準は 27)。
温度が上がるとダイオードやトランジスタの順方向電圧が約 2 mV/℃ ずつ下がります。
抵抗とコンデンサは温度で変わらないものとして扱います");
            ui.label("℃");
        });
    }

    pub(super) fn panel_tran(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstop).desired_width(56.0)).on_hover_text("どこまで時間を進めるか。例: 10m = 10ミリ秒。見たい波形が数周期入る長さにします");
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstep).desired_width(56.0)).on_hover_text("計算の最大の時間刻み。波形が角ばって見えるときは小さくします (終了時間の 1/1000 くらいが目安)");
        });
        ui.horizontal(|ui| {
            if ui.button("過渡解析を実行").on_hover_text("時間とともに電圧・電流がどう変わるかを計算し、オシロスコープのように波形を表示します。充放電や発振、パルスの応答を見るのに使います").clicked() {
                self.run_tran();
            }
            ui.checkbox(&mut self.tran_uic, "ゼロ状態から").on_hover_text("コンデンサの電圧などを 0 の状態から始めます。電源を入れた直後の立ち上がりを見たいときに使います");
        });
    }

    pub(super) fn panel_ac(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("開始");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstart).desired_width(48.0)).on_hover_text("測り始める周波数 (Hz)。例: 10");
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstop).desired_width(48.0)).on_hover_text("測り終わる周波数 (Hz)。例: 1Meg = 1MHz");
            ui.label("点/dec");
            ui.add(egui::TextEdit::singleline(&mut self.ac_ppd).desired_width(30.0)).on_hover_text("周波数が10倍になる間に何点測るか。多いほど滑らかですが、20 もあれば十分です");
        });
        if ui.button("AC解析を実行").on_hover_text("入力の周波数を変えたとき、出力の大きさと位相がどう変わるか (周波数特性) を求めます。フィルタやアンプの帯域を調べるボード線図が出ます").clicked() {
            self.run_ac();
        }
        ui.small("入力にする電源の値を「AC 1」にしてください");
    }

    pub(super) fn panel_sweep(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("電源");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_src).desired_width(36.0).hint_text("V2"))
                .on_hover_text("電圧を変える電源の番号 (例 V2)。インバーターなら入力にあたる電源です。値を少しずつ変えながら、そのつど動作点を求めます");
            ui.label("開始");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_start).desired_width(34.0)).on_hover_text("スイープの開始電圧 (V)");
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_stop).desired_width(34.0)).on_hover_text("スイープの終了電圧 (V)。ロジックなら電源電圧まで");
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_step).desired_width(40.0)).on_hover_text("電圧を変える間隔 (V)。切り替わりをなめらかに見るには、電源電圧の 1/100 くらい");
        });
        if ui.button("DCスイープを実行").on_hover_text("入力電圧を変えながら出力を測り、伝達特性のグラフを描きます。インバーターなら VOH / VOL / 切替電圧 / ノイズマージンも自動で計算します").clicked() {
            self.run_sweep();
        }
    }

    pub(super) fn panel_noise(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("出力");
            ui.add(egui::TextEdit::singleline(&mut self.noise_out).desired_width(56.0).hint_text("OUT / A,B"))
                .on_hover_text("ノイズを測るノードの名前。「A,B」と書くと A と B の電位差を測ります。\n名前が分からないときは右の「回路図で選ぶ」を使うと、クリックするだけで入力できます");
            ui.label("入力電源");
            ui.add(egui::TextEdit::singleline(&mut self.noise_in).desired_width(36.0).hint_text("V1"))
                .on_hover_text("回路の入力にあたる電源の番号 (例 V1)。入れると、ノイズを入力側に換算した値も出ます。\n「このアンプは入力で何 µV のノイズがあるのと同じか」が分かります。空欄でも構いません");
            ui.label("℃");
            ui.add(egui::TextEdit::singleline(&mut self.noise_temp).desired_width(30.0)).on_hover_text("回路の温度 (摂氏)。熱雑音は絶対温度に比例するので、温度を上げると増えます");
        });
        ui.horizontal(|ui| {
            let label = if self.noise_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.noise_pick, label).on_hover_text("押してから回路図のピンや配線をクリックすると、そのノードが出力になり、すぐ解析します。\nShift+クリックで基準ノード (差動出力)").clicked() {
                self.noise_pick = !self.noise_pick;
                self.pick_hover = None;
                if self.noise_pick {
                    self.status = "回路図の測りたいノードをクリックしてください".into();
                }
            }
            if ui.button("ノイズ解析を実行").on_hover_text("抵抗の熱雑音や、ダイオード・トランジスタのショット雑音が出力にどれだけ現れるかを周波数ごとに計算します。").clicked() {
                self.run_noise();
            }
        });
        ui.small("周波数範囲・点数は AC解析の欄を共用。入力電源を指定すると入力換算も表示");
    }

    pub(super) fn panel_variation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("温度");
            ui.add(egui::TextEdit::singleline(&mut self.temp_from).desired_width(34.0)).on_hover_text("温度スイープの開始温度 (℃)");
            ui.label("〜");
            ui.add(egui::TextEdit::singleline(&mut self.temp_to).desired_width(34.0)).on_hover_text("終了温度 (℃)。屋外機器なら -20〜60、車載なら -40〜105 くらいを見ます");
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.temp_step).desired_width(30.0)).on_hover_text("温度の間隔 (℃)");
        });
        if ui.button("温度スイープを実行").on_hover_text("温度を変えながら動作点を求め、各点の電圧がどう動くかをグラフにします。
LED の電流や、トランジスタ回路のバイアスが温度でずれる様子が分かります").clicked() {
            self.run_temp_sweep();
        }
        ui.horizontal(|ui| {
            ui.label("測る点");
            let shown = if self.mc_out.is_empty() { "選ぶ…".to_string() } else { self.mc_out.clone() };
            egui::ComboBox::from_id_salt("mc_node").selected_text(shown).show_ui(ui, |ui| {
                for n in self.node_names() {
                    ui.selectable_value(&mut self.mc_out, n.clone(), n);
                }
            }).response.on_hover_text("電圧のずれを調べるノード (回路図のラベル名か自動で付いた名前)");
            ui.label("回数");
            ui.add(egui::TextEdit::singleline(&mut self.mc_runs).desired_width(34.0)).on_hover_text("ランダムな回路を何個作って計算するか。多いほど分布が滑らかです (300 もあれば十分)");
        });
        ui.horizontal(|ui| {
            ui.label("測るもの");
            ui.selectable_value(&mut self.mc_kind, McKind::Dc, "電圧 (DC)").on_hover_text("選んだ点の直流電圧が、部品の誤差でどれだけずれるか");
            ui.selectable_value(&mut self.mc_kind, McKind::Ac, "カットオフ (AC)")
                .on_hover_text("フィルタが -3dB になる周波数が、部品の誤差でどれだけずれるか。入力電源の値を「AC 1」にし、周波数の範囲は AC解析の欄を使います");
            ui.selectable_value(&mut self.mc_kind, McKind::Tran, "ステップ応答")
                .on_hover_text("電源が 0V→5V に切り替わったときの応答の速さや行き過ぎが、部品の誤差でどれだけずれるか。入力電源は PULSE(0 5 0 1u 1u 1 2) のようにし、時間は過渡解析の欄を使います。落ち着くまで十分長い終了時間が必要です");
        });
        if self.mc_kind == McKind::Tran {
            ui.horizontal(|ui| {
                ui.label("指標");
                egui::ComboBox::from_id_salt("mc_metric")
                    .selected_text(match self.mc_metric {
                        TranMetric::RiseTime => "立ち上がり時間",
                        TranMetric::Overshoot => "オーバーシュート",
                        TranMetric::SettlingTime => "整定時間",
                        TranMetric::FinalValue => "最終値",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mc_metric, TranMetric::RiseTime, "立ち上がり時間").on_hover_text("出力が最終値の 10% から 90% まで変わるのにかかる時間。小さいほど速い");
                        ui.selectable_value(&mut self.mc_metric, TranMetric::Overshoot, "オーバーシュート").on_hover_text("最終値を行き過ぎる量 (ステップの大きさに対する %)");
                        ui.selectable_value(&mut self.mc_metric, TranMetric::SettlingTime, "整定時間").on_hover_text("10% に達してから、最終値の ±2% に収まるまでの時間");
                        ui.selectable_value(&mut self.mc_metric, TranMetric::FinalValue, "最終値").on_hover_text("落ち着いたあとの電圧");
                    });
            });
        }
        ui.horizontal(|ui| {
            ui.label("許容差 %  R");
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_r).desired_width(26.0)).on_hover_text("抵抗の誤差。炭素皮膜抵抗は 5%、金属皮膜は 1% が一般的です");
            ui.label("C");
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_c).desired_width(26.0)).on_hover_text("コンデンサの誤差。セラミック (X7R) は 10〜20%、電解は 20% ほどです");
            ui.label("L");
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_l).desired_width(26.0)).on_hover_text("コイルの誤差。10〜20% が一般的です");
        });
        if ui.button("ばらつき解析を実行").on_hover_text("すべての部品の値を許容差の範囲でランダムに変えて何度も動作点を計算し、選んだ点の電圧がどれだけばらつくかを見ます。
「同じ回路を作っても、実物ごとに電圧が違う」理由と、どの部品を高精度にすべきかが分かります").clicked() {
            self.run_mc();
        }
    }

    pub(super) fn panel_results(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let stale = self.dc.as_ref().is_some_and(|d| d.signature != self.signature());
        if let Some(dc) = &self.dc {
            if stale {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "回路が変更されました。再度解析してください");
            }
            if let Some(f) = &dc.fault {
                ui.colored_label(Color32::from_rgb(230, 90, 70), format!("⚠ これは「{f}」と仮定した結果です (実際の回路ではありません)"));
            }
            match &dc.outcome {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                    }
                }
                Ok(res) => {
                    for w in &res.warnings {
                        ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                    }
                    egui::CollapsingHeader::new("解析結果").default_open(true).show(ui, |ui| {
                        egui::Grid::new("dc_nodes").striped(true).show(ui, |ui| {
                            for (n, v) in res.voltages.iter().filter(|(n, _)| !n.contains('.')) {
                                ui.label(n);
                                ui.label(eng(*v, "V"));
                                ui.end_row();
                            }
                        });
                        ui.separator();
                        egui::Grid::new("dc_elems").striped(true).show(ui, |ui| {
                            ui.strong("素子");
                            ui.strong("電流");
                            ui.strong("電圧");
                            ui.strong("電力");
                            ui.end_row();
                            for e in res.elements.iter().filter(|e| !e.name.contains('.')) {
                                ui.label(&e.name);
                                ui.label(eng(e.current, "A"));
                                ui.label(eng(e.voltage, "V"));
                                ui.label(eng(e.power, "W"));
                                ui.end_row();
                            }
                        });
                        ui.small("電流は素子の第1端子→第2端子の向き。電源は電力を供給すると負");
                    });
                }
            }
        }
        ui.collapsing("ネットリスト", |ui| {
            for n in extract(&self.sch) {
                let pins: Vec<String> = n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                ui.label(format!("{}: {}", n.name, pins.join(", ")));
            }
        });
        if ui.button("SPICEネットリストを書き出し…").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("SPICE", &["cir"]).set_file_name("circuit.cir").save_file() {
                self.status = match std::fs::write(&p, to_spice(&self.sch, "circuitCad")) {
                    Ok(()) => format!("書き出しました: {}", p.display()),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
    }
}
