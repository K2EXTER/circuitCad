//! The analysis part of the side panel: DC, transient, AC, sweep, noise, temperature / tolerance and the results list.

use super::*;

impl SchEditor {
    pub(super) fn panel_dc(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_v, i18n::tr("電圧表示")).on_hover_text(i18n::tr("DC解析の結果を回路図に重ねて表示します。配線は電圧が低いほど青、高いほど赤に色分けされます"));
            ui.checkbox(&mut self.show_i, i18n::tr("電流表示")).on_hover_text(i18n::tr("各部品を流れる電流を数値と矢印 (電流の向き) で表示します。「どこに電流が流れているか」の理解に役立ちます。数値の符号は向きの約束です: 素子の第1端子→第2端子の向きに流れるとプラス、逆向きならマイナス (例: -9.2 mA は、9.2 mA が逆向きに流れている)。電源は、電力を供給しているとマイナスになります。1 nA 未満 (直流でのコンデンサなど、ほぼ 0 のもの) は表示しません"));
        });
        ui.checkbox(&mut self.live, i18n::tr("自動で再計算")).on_hover_text(
            i18n::tr("回路を変えるたびに、開いている解析 (DC / 過渡 / AC) をやり直します。
部品の「スライダー」と組み合わせると、値を動かしながら結果がどう変わるかを実験できます。
重い回路では動きがもたつくのでオフにしてください"),
        );
        ui.horizontal(|ui| {
            ui.label(i18n::tr("回路の温度"));
            ui.add(egui::TextEdit::singleline(&mut self.temp_c).desired_width(36.0))
                .on_hover_text(i18n::tr("DC・過渡・AC・DCスイープで使う温度 (摂氏、標準は 27)。
温度が上がるとダイオードやトランジスタの順方向電圧が約 2 mV/℃ ずつ下がります。
抵抗・コンデンサ・コイルは、ライブラリ部品の温度係数 (TC1・TC2) を入れたものだけ、温度で値が変わります (基本部品は変わりません)"));
            ui.label("℃");
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "DC動作点を解析"), "DC動作点を解析").on_hover_text(i18n::tr("時間が経って落ち着いたときの各点の電圧と電流を求めます。回路図の各ピンの横に電圧が出るので、「思った電圧になっているか」の確認に一番よく使います")).clicked() {
            self.run_dc();
        }
        if !self.wiring.is_empty() {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(90, 190, 110), i18n::trf!("基板の配線抵抗を反映中 ({} ピン、結合 {} 組)", self.wiring.len(), self.couplings.len()))
                    .on_hover_text(i18n::tr("基板の銅箔 (配線・ビア・ベタ) の抵抗とインダクタンスを、電源の入口から各ピンの手前に直列に入れて解析しています。インダクタンスはACや速い変化で効きます。近くを並んで走る別のネットの配線とは、磁気の結合 (相互インダクタンス) も入れてあり、行きと帰りが近いほどループのインダクタンスが小さくなります。配線を変えたら、基板の「配線の電圧降下」から反映し直してください。ネットの電圧は入口側の値で、部品のピンの電圧は少し低くなります"));
                if ui.small_button(i18n::tr("解除")).on_hover_text(i18n::tr("配線の抵抗を入れない理想の回路に戻します")).clicked() {
                    self.wiring.clear();
                    self.couplings.clear();
                    self.heat_copper.clear();
                }
            });
        }
    }

    pub(super) fn panel_tran(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("終了"));
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstop).desired_width(56.0)).on_hover_text(i18n::tr("どこまで時間を進めるか。例: 10m = 10ミリ秒。見たい波形が数周期入る長さにします"));
            ui.label(i18n::tr("刻み"));
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstep).hint_text(i18n::tr("自動")).desired_width(56.0)).on_hover_text(i18n::tr("計算の最大の時間刻み。空欄なら自動で、終了時間の 1/1000 になります (波形の変化が速い所では、自動でもっと細かくします)。波形が角ばって見えるときは、小さい値を入れます"));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.tran_uic, i18n::tr("ゼロ状態から")).on_hover_text(i18n::tr("コンデンサの電圧などを 0 の状態から始めます。電源を入れた直後の立ち上がりを見たいときに使います"));
        });

        if self.lit(crate::widgets::primary_button_wide(ui, true, "過渡解析を実行"), "過渡解析を実行").on_hover_text(i18n::tr("時間とともに電圧・電流がどう変わるかを計算し、オシロスコープのように波形を表示します。充放電や発振、パルスの応答を見るのに使います")).clicked() {
            self.run_tran();
        }
    }

    pub(super) fn panel_ac(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("開始"));
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstart).desired_width(48.0)).on_hover_text(i18n::tr("測り始める周波数 (Hz)。例: 10"));
            ui.label(i18n::tr("終了"));
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstop).desired_width(48.0)).on_hover_text(i18n::tr("測り終わる周波数 (Hz)。例: 1Meg = 1MHz"));
            ui.label(i18n::tr("点/dec"));
            ui.add(egui::TextEdit::singleline(&mut self.ac_ppd).desired_width(30.0)).on_hover_text(i18n::tr("周波数が10倍になる間に何点測るか。多いほど滑らかですが、20 もあれば十分です"));
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "AC解析を実行"), "AC解析を実行").on_hover_text(i18n::tr("入力の周波数を変えたとき、出力の大きさと位相がどう変わるか (周波数特性) を求めます。フィルタやアンプの帯域を調べるボード線図が出ます")).clicked() {
            self.run_ac();
        }
    }

    pub(super) fn panel_sweep(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("電源"));
            ui.add(egui::TextEdit::singleline(&mut self.sweep_src).desired_width(36.0).hint_text("V2"))
                .on_hover_text(i18n::tr("電圧を変える電源の番号 (例 V2)。インバーターなら入力にあたる電源です。値を少しずつ変えながら、そのつど動作点を求めます"));
            ui.label(i18n::tr("開始"));
            ui.add(egui::TextEdit::singleline(&mut self.sweep_start).desired_width(34.0)).on_hover_text(i18n::tr("スイープの開始電圧 (V)"));
            ui.label(i18n::tr("終了"));
            ui.add(egui::TextEdit::singleline(&mut self.sweep_stop).desired_width(34.0)).on_hover_text(i18n::tr("スイープの終了電圧 (V)。ロジックなら電源電圧まで"));
            ui.label(i18n::tr("刻み"));
            ui.add(egui::TextEdit::singleline(&mut self.sweep_step).desired_width(40.0)).on_hover_text(i18n::tr("電圧を変える間隔 (V)。切り替わりをなめらかに見るには、電源電圧の 1/100 くらい"));
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "DCスイープを実行"), "DCスイープを実行").on_hover_text(i18n::tr("入力電圧を変えながら出力を測り、伝達特性のグラフを描きます。インバーターなら VOH / VOL / 切替電圧 / ノイズマージンも自動で計算します")).clicked() {
            self.run_sweep();
        }
    }

    pub(super) fn panel_noise(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(i18n::tr("出力"));
            ui.add(egui::TextEdit::singleline(&mut self.noise_out).desired_width(56.0).hint_text("OUT / A,B"))
                .on_hover_text(i18n::tr("ノイズを測るノードの名前。「A,B」と書くと A と B の電位差を測ります。\n名前が分からないときは右の「回路図で選ぶ」を使うと、クリックするだけで入力できます"));
            ui.label(i18n::tr("入力電源"));
            ui.add(egui::TextEdit::singleline(&mut self.noise_in).desired_width(36.0).hint_text("V1"))
                .on_hover_text(i18n::tr("回路の入力にあたる電源の番号 (例 V1)。入れると、ノイズを入力側に換算した値も出ます。\n「このアンプは入力で何 µV のノイズがあるのと同じか」が分かります。空欄でも構いません"));
            ui.label("℃");
            ui.add(egui::TextEdit::singleline(&mut self.noise_temp).desired_width(30.0)).on_hover_text(i18n::tr("回路の温度 (摂氏)。熱雑音は絶対温度に比例するので、温度を上げると増えます"));
        });
        ui.horizontal(|ui| {
            let label = if self.noise_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.noise_pick, label).on_hover_text(i18n::tr("押してから回路図のピンや配線をクリックすると、そのノードが出力になり、すぐ解析します。\nShift+クリックで基準ノード (差動出力)")).clicked() {
                self.noise_pick = !self.noise_pick;
                self.pick_probe = false;
                self.pick_hover = None;
                if self.noise_pick {
                    self.status = "回路図の測りたいノードをクリックしてください".into();
                }
            }
            if self.lit(ui.button(i18n::tr("ノイズ解析を実行")), "ノイズ解析を実行").on_hover_text(i18n::tr("抵抗の熱雑音や、ダイオード・トランジスタのショット雑音が出力にどれだけ現れるかを周波数ごとに計算します。")).clicked() {
                self.run_noise();
            }
        });
    }

    pub(super) fn panel_variation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("温度"));
            ui.add(egui::TextEdit::singleline(&mut self.temp_from).desired_width(34.0)).on_hover_text(i18n::tr("温度スイープの開始温度 (℃)"));
            ui.label("〜");
            ui.add(egui::TextEdit::singleline(&mut self.temp_to).desired_width(34.0)).on_hover_text(i18n::tr("終了温度 (℃)。屋外機器なら -20〜60、車載なら -40〜105 くらいを見ます"));
            ui.label(i18n::tr("刻み"));
            ui.add(egui::TextEdit::singleline(&mut self.temp_step).desired_width(30.0)).on_hover_text(i18n::tr("温度の間隔 (℃)"));
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "温度スイープを実行"), "温度スイープを実行").on_hover_text(i18n::tr("温度を変えながら動作点を求め、各点の電圧がどう動くかをグラフにします。
LED の電流や、トランジスタ回路のバイアスが温度でずれる様子が分かります")).clicked() {
            self.run_temp_sweep();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("測る点"));
            let shown = if self.mc_out.is_empty() { "選ぶ…".to_string() } else { self.mc_out.clone() };
            egui::ComboBox::from_id_salt("mc_node").selected_text(shown).show_ui(ui, |ui| {
                for n in self.node_names() {
                    ui.selectable_value(&mut self.mc_out, n.clone(), n);
                }
            }).response.on_hover_text(i18n::tr("電圧のずれを調べるノード (回路図のラベル名か自動で付いた名前)"));
            ui.label(i18n::tr("回数"));
            ui.add(egui::TextEdit::singleline(&mut self.mc_runs).desired_width(34.0)).on_hover_text(i18n::tr("ランダムな回路を何個作って計算するか。多いほど分布が滑らかです (300 もあれば十分)"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("測るもの"));
            ui.selectable_value(&mut self.mc_kind, McKind::Dc, i18n::tr("電圧 (DC)")).on_hover_text(i18n::tr("選んだ点の直流電圧が、部品の誤差でどれだけずれるか"));
            ui.selectable_value(&mut self.mc_kind, McKind::Ac, i18n::tr("カットオフ (AC)"))
                .on_hover_text(i18n::tr("フィルタが -3dB になる周波数が、部品の誤差でどれだけずれるか。入力電源の値を「AC 1」にし、周波数の範囲は AC解析の欄を使います"));
            ui.selectable_value(&mut self.mc_kind, McKind::Tran, i18n::tr("ステップ応答"))
                .on_hover_text(i18n::tr("電源が 0V→5V に切り替わったときの応答の速さや行き過ぎが、部品の誤差でどれだけずれるか。入力電源は PULSE(0 5 0 1u 1u 1 2) のようにし、時間は過渡解析の欄を使います。落ち着くまで十分長い終了時間が必要です"));
        });
        if self.mc_kind == McKind::Tran {
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr("指標"));
                egui::ComboBox::from_id_salt("mc_metric")
                    .selected_text(match self.mc_metric {
                        TranMetric::RiseTime => i18n::tr("立ち上がり時間"),
                        TranMetric::Overshoot => i18n::tr("オーバーシュート"),
                        TranMetric::SettlingTime => i18n::tr("整定時間"),
                        TranMetric::FinalValue => i18n::tr("最終値"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mc_metric, TranMetric::RiseTime, i18n::tr("立ち上がり時間")).on_hover_text(i18n::tr("出力が最終値の 10% から 90% まで変わるのにかかる時間。小さいほど速い"));
                        ui.selectable_value(&mut self.mc_metric, TranMetric::Overshoot, i18n::tr("オーバーシュート")).on_hover_text(i18n::tr("最終値を行き過ぎる量 (ステップの大きさに対する %)"));
                        ui.selectable_value(&mut self.mc_metric, TranMetric::SettlingTime, i18n::tr("整定時間")).on_hover_text(i18n::tr("10% に達してから、最終値の ±2% に収まるまでの時間"));
                        ui.selectable_value(&mut self.mc_metric, TranMetric::FinalValue, i18n::tr("最終値")).on_hover_text(i18n::tr("落ち着いたあとの電圧"));
                    });
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("許容差 %  R"));
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_r).desired_width(26.0)).on_hover_text(i18n::tr("抵抗の誤差。炭素皮膜抵抗は 5%、金属皮膜は 1% が一般的です。許容差の欄は、回路図と一緒に保存されます"));
            ui.label("C");
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_c).desired_width(26.0)).on_hover_text(i18n::tr("コンデンサの誤差。セラミック (X7R) は 10〜20%、電解は 20% ほどです"));
            ui.label("L");
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_l).desired_width(26.0)).on_hover_text(i18n::tr("コイルの誤差。10〜20% が一般的です"));
            ui.label(i18n::tr("電源"));
            ui.add(egui::TextEdit::singleline(&mut self.mc_tol_v).desired_width(26.0)).on_hover_text(i18n::tr("直流電源の電圧の誤差。最悪ケースだけに使います (ランダムのばらつきは電源をそのままにします)。0 なら電源は変えません。例: 5V 電源が ±5% なら 5"));
        });
        ui.checkbox(&mut self.mc_spread, i18n::tr("データシートの min / max も入れる")).on_hover_text(i18n::tr(
            "オペアンプのオフセット、レギュレータや基準電圧 IC の出力電圧のように、同じ型番でも 1 個ずつ違う値を、データシートの min〜max の範囲で動かします (モデルに書いてある部品だけ)。ランダムのばらつきでは範囲の中で均等に動かすので、typ の近くに多い実物より、少し広めに出ます。外すと、モデルの値 (typ) のままにします",
        ));
        if self.lit(crate::widgets::primary_button_wide(ui, true, "ばらつき解析を実行"), "ばらつき解析を実行").on_hover_text(i18n::tr("すべての部品の値を許容差の範囲でランダムに変えて何度も動作点を計算し、選んだ点の電圧がどれだけばらつくかを見ます。
「同じ回路を作っても、実物ごとに電圧が違う」理由と、どの部品を高精度にすべきかが分かります")).clicked() {
            self.run_mc();
        }
        if self.lit(ui.button(i18n::tr("感度を計算 (効く部品の順)")), "感度を計算 (効く部品の順)").on_hover_text(i18n::tr("部品を 1 つずつ 1% 大きく・小さくして、上で選んだ点の結果がどれだけ動くかを、効く順に並べます。許容差は使いません。どの部品を精度のいいものにすべきか、どれは安い部品でいいかが分かります")).clicked() {
            self.run_sens();
        }
    }

    pub(super) fn panel_results(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let stale = self.dc.as_ref().is_some_and(|d| d.signature != self.signature());
        let mut go_tran = false;
        if let Some(dc) = &self.dc {
            if stale {
                ui.colored_label(crate::theme::WARN, i18n::tr("回路が変更されました。再度解析してください"));
            }
            if let Some(f) = &dc.fault {
                ui.colored_label(crate::theme::ERROR, i18n::trf!("⚠ これは「{f}」と仮定した結果です (実際の回路ではありません)"));
            }
            match &dc.outcome {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(crate::theme::ERROR, m);
                    }
                    // an oscillator or latch has no single operating point: its waveform is the answer
                    if msgs.iter().any(|m| m.contains("過渡解析")) && self.lit(ui.button(i18n::tr("過渡解析を実行")), "過渡解析を実行").on_hover_text(i18n::tr("DCの1点ではなく、時間を追った波形を計算します。発振回路や2つの状態を持つ回路は、ここで動きが見えます (終了時間と刻みは「過渡解析」の設定が使われます)")).clicked() {
                        go_tran = true;
                    }
                }
                Ok(res) => {
                    crate::widgets::warning_lines(ui, "dc_warn", &res.warnings, |_| false);
                    for r in &dc.ratings {
                        let (mark, col) = if r.level == sim_core::RatingLevel::Over {
                            ("⚠ 定格超え", crate::theme::ERROR)
                        } else {
                            ("⚠ 余裕が少ない", crate::theme::WARN)
                        };
                        ui.colored_label(col, format!("{mark}: {}", i18n::tr_dyn(&r.message))).on_hover_text(i18n::tr_dyn(&r.hint));
                    }
                    // the long lists are in a window (search, sort, click to jump): the panel only says how much there is
                    let nodes = res.voltages.iter().filter(|(n, _)| !n.contains('.')).count();
                    let elems = res.elements.iter().filter(|e| !e.name.contains('.')).count();
                    ui.label(i18n::trf!("ノード {nodes} 個・素子 {elems} 個"));
                    if let Some(hot) = res.elements.iter().filter(|e| !e.name.contains('.')).max_by(|a, b| a.power.abs().total_cmp(&b.power.abs())) {
                        ui.small(i18n::trf!("電力が最大: {} ({})", hot.name, eng(hot.power.abs(), "W")))
                            .on_hover_text(i18n::tr("いちばん熱くなる素子です。表を「電力が大きい順」に並べると、続きが分かります"));
                    }
                }
            }
        }
        if go_tran {
            self.run_tran();
        }
        // the lists and the export are one bar: two windows (search, sort, click to jump) and the file for other simulators
        let has_result = self.dc.as_ref().is_some_and(|d| d.outcome.is_ok());
        let bar = crate::widgets::segment_row(ui, &[("結果の表…", has_result), ("ネットリスト…", true), ("SPICE書き出し…", true)]);
        bar[0].clone().on_hover_text(if has_result { i18n::tr("ノードの電圧と、素子ごとの電流・電圧・電力を、検索と並べ替えのできる窓で開きます。行を押すと、回路図でその部品を選びます") } else { i18n::tr("先に「DC動作点を解析」を押すと、結果の表が開けます") });
        bar[1].clone().on_hover_text(i18n::tr("どのピンとどのピンがつながっているか (ネット) の一覧を、検索のできる窓で開きます。回路図の配線ミスや、基板に渡るつながりの確認に使います"));
        self.lit(bar[2].clone(), "SPICEネットリストを書き出し…").on_hover_text(i18n::tr("回路を SPICE のネットリスト (.cir) として書き出します。LTspice や ngspice など、ほかのシミュレータで同じ回路を動かしたいときに使います"));
        if bar[0].clicked() {
            self.tables.results = true;
        }
        if bar[1].clicked() {
            self.tables.netlist = true;
        }
        if bar[2].clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("SPICE", &["cir"]).set_file_name("circuit.cir").save_file() {
                self.status = match std::fs::write(&p, to_spice(&self.sch, "circuitCad")) {
                    Ok(()) => format!("書き出しました: {}", p.display()),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
    }
}
