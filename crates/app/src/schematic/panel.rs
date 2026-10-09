//! The left side panel: tools, part palette, properties and analysis buttons.

use super::*;

impl SchEditor {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        // the panel is taller than a small window: let it scroll instead of hiding the bottom controls
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.side_panel_body(ui));
    }

    pub(super) fn side_panel_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("回路図");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "選択").on_hover_text("部品や配線をクリックで選択、部品はドラッグで移動できます。選ぶと値(抵抗値など)を変えられます");
            ui.selectable_value(&mut self.tool, Tool::Wire, "配線").on_hover_text("端点をクリックして線を引きます。クリックするたびに折れ線が続き、ダブルクリックか右クリックで終わります");
            ui.selectable_value(&mut self.tool, Tool::Label, "ラベル").on_hover_text("同じ名前のラベルを付けた場所は、線でつながなくても同じ配線 (ネット) になります。図がすっきりします");
        });
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text);
        }
        ui.separator();
        ui.label("部品 (クリックして配置 / Rキーで回転)");
        for k in Kind::PALETTE {
            if ui.selectable_label(self.tool == Tool::Place && self.place_lib.is_none() && self.place == *k, k.label()).on_hover_text(kind_tip(*k)).clicked() {
                self.tool = Tool::Place;
                self.place_lib = None;
                self.place = *k;
                self.pending = None;
            }
        }
        ui.separator();
        let mut chosen = None;
        egui::CollapsingHeader::new(format!("ライブラリ部品 ({})", self.library.len())).default_open(true).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索"));
            let f = self.lib_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                // grouped by category, then by name
                let cat = |d: &PartDef| if d.category.is_empty() { "その他".to_string() } else { d.category.clone() };
                let mut order: Vec<usize> = (0..self.library.len()).collect();
                order.sort_by_key(|&i| (cat(&self.library[i]), self.library[i].name.to_lowercase()));
                let mut last = String::new();
                for i in order {
                    let d = &self.library[i];
                    let c = cat(d);
                    if !f.is_empty() && ![&d.name, &d.description, &c].iter().any(|t| t.to_lowercase().contains(&f)) {
                        continue;
                    }
                    if c != last {
                        ui.label(egui::RichText::new(&c).small().weak());
                        last = c;
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let selected = self.tool == Tool::Place && self.place_lib == Some(i);
                    if ui.selectable_label(selected, format!("  {}{mark}", d.name)).on_hover_text(&d.description).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            ui.small("◎ = シミュレーションモデルあり");
            ui.horizontal(|ui| {
                if ui.button("新規作成…").clicked() {
                    self.form = Some(PartForm::default());
                }
                if ui.button("JSONから追加…").clicked() {
                    self.import_part_json();
                }
                if ui.button("再読み込み").clicked() {
                    self.reload_library();
                }
            });
            ui.small(format!("保存先: {}", self.lib_dir.display()));
            for e in &self.lib_errors {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
        });
        if let Some(i) = chosen {
            self.tool = Tool::Place;
            self.place_lib = Some(i);
            self.pending = None;
        }
        ui.separator();
        if let Some(Sel::Comp(i)) = self.selected
            && i < self.sch.components.len() {
                let c = &mut self.sch.components[i];
                ui.label("選択中の部品");
                if !c.kind.is_power_symbol() {
                    ui.horizontal(|ui| {
                        ui.label("番号");
                        ui.text_edit_singleline(&mut c.refdes);
                    });
                }
                if c.kind != Kind::Ground {
                    ui.horizontal(|ui| {
                        ui.label("値");
                        ui.text_edit_singleline(&mut c.value);
                    });
                }
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) {
                    let (first, rest) = match c.value.trim().split_once(char::is_whitespace) {
                        Some((f, r)) => (f.to_string(), format!(" {}", r.trim())),
                        None => (c.value.trim().to_string(), String::new()),
                    };
                    if let Some(cur) = parse_value(&first).filter(|v| *v > 0.0) {
                        // re-centre on the typed value unless the slider itself wrote it
                        if self.slider.as_ref().is_none_or(|(k, _, t)| *k != i || *t != c.value) {
                            self.slider = Some((i, cur, c.value.clone()));
                        }
                        let base = self.slider.as_ref().map_or(cur, |s| s.1);
                        let mut x = (cur / base).log10().clamp(-2.0, 2.0);
                        let changed = ui
                            .horizontal(|ui| {
                                ui.label("スライダー");
                                ui.add(egui::Slider::new(&mut x, -2.0..=2.0).show_value(false)).on_hover_text(
                                    "ドラッグして値を動かします (中央 = 今の値、左右に 1/100 〜 100 倍)。
下の「自動で再計算」をオンにすると、動かしながら波形や電圧が変わるのを見られます",
                                ).changed()
                            })
                            .inner;
                        if changed {
                            let v = format_value(base * 10f64.powf(x));
                            c.value = format!("{v}{rest}");
                            self.slider = Some((i, base, c.value.clone()));
                        }
                    }
                }
                if c.kind == Kind::VSource {
                    // read the form again when the text changed or another source was selected
                    let stale = self.src_form.as_ref().is_none_or(|(k, t, _)| *k != i || *t != c.value);
                    if stale {
                        self.src_form = Some((i, c.value.clone(), SourceForm::parse(&c.value)));
                    }
                    if let Some((_, text, form)) = self.src_form.as_mut() {
                        ui.separator();
                        if crate::source_form::show(ui, form) {
                            match form.to_text() {
                                Ok(t) => {
                                    c.value = t.clone();
                                    *text = t;
                                }
                                Err(msg) => {
                                    ui.colored_label(Color32::from_rgb(230, 60, 60), msg);
                                }
                            }
                        } else if let Err(msg) = form.to_text() {
                            ui.colored_label(Color32::from_rgb(230, 60, 60), msg);
                        }
                    }
                    ui.collapsing("値の書き方 (テキストで直接入力する場合)", |ui| {
                        ui.small("例: 5 / SIN(0 1 1k) / PULSE(0 5 100u 1u 1u 400u 1m) / PWL(0 0 1m 5) / 5 AC 1");
                        ui.small("ノイズ: NOISE(実効値 最高周波数 [シード]) 例: 5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k) (+ の前後は空白)");
                    });
                }
                if c.kind == Kind::Capacitor {
                    ui.small("例: 100n / 100n ESR=20m ESL=5n (等価直列抵抗・インダクタンス)");
                }
                if c.kind == Kind::Inductor {
                    ui.small("例: 10u / 10u DCR=0.1 (巻線抵抗)");
                }
                if let Some(sym) = &c.custom {
                    ui.label(format!("フットプリント: {}", c.footprint));
                    match &sym.sim {
                        Some(m) => ui.label(format!("モデル: {}", m.name)),
                        None => ui.colored_label(Color32::from_rgb(220, 160, 40), "シミュレーションモデルなし"),
                    };
                } else if !c.kind.is_power_symbol() {
                    let current = c.footprint_name();
                    egui::ComboBox::from_label("フットプリント").selected_text(current).show_ui(ui, |ui| {
                        for n in pcb_core::footprints::names() {
                            ui.selectable_value(&mut c.footprint, n.clone(), n);
                        }
                    });
                }
                if ui.button("回転 (R)").clicked() {
                    self.rotate_req = Some(i);
                }
            }
        if self.selected.is_some() && ui.button("削除 (Delete)").clicked() {
            self.delete_selected();
        }
        ui.separator();
        if ui.button("ERC (電気ルールチェック)").on_hover_text("配線ミスの点検。ピンや線の端がつながっていない、電源が短絡している、部品番号がダブっている、などを探して丸で示します (赤紫 = エラー、橙 = 注意)。結果の行をクリックすると、その場所へ移動して直し方を表示します。シミュレーションや基板に進む前にやっておくと安心です").clicked() {
            self.problems = erc(&self.sch);
            self.erc_focus = None;
            self.erc_ran = true;
            let errors = self.problems.iter().filter(|p| p.severity == Severity::Error).count();
            self.status = format!("ERC: エラー {} 件、注意 {} 件", errors, self.problems.len() - errors);
        }
        if self.erc_ran && self.problems.is_empty() {
            ui.colored_label(Color32::from_rgb(60, 160, 90), "✔ 問題は見つかりませんでした");
        }
        let mut jump = None;
        for (k, p) in self.problems.iter().enumerate() {
            let (mark, color) = match p.severity {
                Severity::Error => ("✖", Color32::from_rgb(220, 40, 60)),
                Severity::Warning => ("▲", Color32::from_rgb(210, 150, 20)),
            };
            let label = egui::RichText::new(format!("{mark} {}", p.message)).color(color);
            let tip = if p.at.is_some() { "クリックで、その場所へ移動して説明を表示します" } else { "クリックで説明を表示します" };
            if ui.link(label).on_hover_text(tip).clicked() {
                self.erc_focus = Some(k);
                jump = p.at;
            }
            if self.erc_focus == Some(k) {
                ui.indent(("erc_hint", k), |ui| {
                    ui.small(&p.hint);
                });
            }
        }
        if let Some(at) = jump {
            self.pan = Vec2::new(at.0 as f32, at.1 as f32);
        }
        ui.separator();
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
        ui.separator();
        ui.label("過渡解析");
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
        ui.separator();
        ui.label("AC解析 (周波数特性)");
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
        ui.separator();
        ui.label("DCスイープ (入力→出力の特性)");
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
        ui.separator();
        ui.label("ノイズ解析 (熱雑音・ショット雑音)");
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
        ui.separator();
        ui.label("実験: 温度とばらつき");
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
        ui.separator();
        let stale = self.dc.as_ref().is_some_and(|d| d.signature != self.signature());
        if let Some(dc) = &self.dc {
            if stale {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "回路が変更されました。再度解析してください");
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
        ui.separator();
        if let Some(mut form) = self.form.take() {
            let mut open = true;
            let mut saved = None;
            egui::Window::new("部品を作成").open(&mut open).default_width(560.0).vscroll(true).show(ui.ctx(), |ui| {
                saved = form.ui(ui);
            });
            match saved {
                Some(def) => match part_lib::save(&self.lib_dir, &def) {
                    Ok(p) => {
                        self.reload_library();
                        self.status = format!("ライブラリに保存しました: {}", p.display());
                    }
                    Err(e) => {
                        self.status = format!("保存失敗: {e}");
                        self.form = Some(form);
                    }
                },
                None if open => self.form = Some(form),
                None => {}
            }
        }
        ui.small("左クリック: 配置 / 配線 (斜めは自動でL字)\n右クリック・ダブルクリック: 配線を終了\n選択ツール: 左ドラッグ = 完全に囲んだものを選択 / 右ドラッグ = 触れたものも選択 (Shiftで追加)\n中ドラッグ: 移動 (配線中などは右ドラッグも可) / ホイール: 拡大縮小");
    }
}
