//! The left side panel: tool and part palette, the selected part, the ERC and the file / form extras.

use super::*;

/// The analyses of the "解析" tab: (key, title, open at first, what it is for).
const ANALYSIS_SECTIONS: &[(&str, &str, bool, &str)] = &[
    ("dc", "DC動作点 (電圧・電流)", true, "落ち着いたあとの各点の電圧と部品の電流を見ます。まずここから"),
    ("tran", "過渡解析 (時間の変化)", true, "時間とともに電圧・電流がどう変わるかを、オシロスコープのように見ます"),
    ("ac", "AC解析 (周波数特性)", true, "周波数ごとの強さ (ゲイン) と位相を見ます。フィルタやアンプの帯域、安定性の余裕も"),
    ("sweep", "DCスイープ (入力→出力)", false, "入力電圧を変えたときの出力の変化 (伝達特性) を見ます"),
    ("param", "パラメータスイープ (値を振る)", false, "部品の値を何通りか変えて、波形を重ねて比べます"),
    ("noise", "ノイズ解析", false, "熱雑音・ショット雑音が出力にどれだけ出るかを見ます"),
    ("variation", "温度とばらつき", false, "温度を変える・部品の誤差でずれる範囲を調べます"),
    ("fault", "故障シミュレーション", false, "部品を壊した (断線・ショート・値の間違い) ときに何が起きるかを見ます"),
];

impl SchEditor {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        // the panel is taller than a small window: let it scroll instead of hiding the bottom controls
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.side_panel_body(ui));
    }

    pub(super) fn side_panel_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("回路図");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.panel_tab, PanelTab::Draw, "作図")
                .on_hover_text("部品を置く、配線する、部品の値を変える、配線ミスを点検する (ERC)");
            ui.selectable_value(&mut self.panel_tab, PanelTab::Analyze, "解析")
                .on_hover_text("シミュレーションを実行して、電圧・波形・周波数特性・ノイズ・温度やばらつきの影響を見る");
        });
        ui.separator();
        match self.panel_tab {
            PanelTab::Draw => {
                self.panel_parts(ui);
                self.panel_properties(ui);
                self.panel_erc(ui);
                self.panel_forms(ui);
            }
            PanelTab::Analyze => {
                ui.horizontal(|ui| {
                    // one button that flips: open everything, then close everything, then open again
                    let all_open = self.sec_mode == 1;
                    let (label, tip) = if all_open { ("すべて閉じる", "すべての解析の設定を閉じて、見出しだけにします") } else { ("すべて開く", "すべての解析の設定を開きます。もう一度押すと、すべて閉じます") };
                    // during a lesson every section is open, so the lesson's steps can be found
                    let in_lesson = self.lesson.is_some();
                    let b = ui.add_enabled(!in_lesson, egui::Button::new(label).small());
                    if b.on_hover_text(if in_lesson { "レッスン中は、手順に出てくる欄を探せるように、すべて開いています" } else { tip }).clicked() {
                        self.sec_gen += 1;
                        self.sec_mode = if all_open { 2 } else { 1 };
                    }
                });
                let lesson = self.lesson.is_some();
                let (open_all, close_all) = (self.sec_mode == 1 || lesson, self.sec_mode == 2 && !lesson);
                for (key, title, basic, hint) in ANALYSIS_SECTIONS {
                    let id = egui::Id::new(("analysis_section", *key, self.sec_gen, lesson));
                    egui::CollapsingHeader::new(egui::RichText::new(*title).strong()).id_salt(id).default_open(open_all || (*basic && !close_all)).show(ui, |ui| {
                        match *key {
                            "dc" => self.panel_dc(ui),
                            "tran" => self.panel_tran(ui),
                            "ac" => self.panel_ac(ui),
                            "sweep" => self.panel_sweep(ui),
                            "param" => self.panel_param(ui),
                            "noise" => self.panel_noise(ui),
                            "variation" => self.panel_variation(ui),
                            _ => self.panel_fault(ui),
                        }
                    }).header_response.on_hover_text(*hint);
                }
                self.panel_results(ui);
            }
        }
    }

    pub(super) fn panel_parts(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "選択").on_hover_text("部品や配線をクリックで選択、部品はドラッグで移動できます。選ぶと値(抵抗値など)を変えられます");
            ui.selectable_value(&mut self.tool, Tool::Wire, "配線").on_hover_text("端点をクリックして線を引きます。クリックするたびに折れ線が続き、ダブルクリックで終わります。右クリックで配線を確定し、配線していないときの右クリックで選択ツールに戻ります");
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
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索 (名前・説明・ピン名・パッケージ)"))
                .on_hover_text("部品名のほか、説明、ピンの名前、パッケージ名 (例: SOT-23、TO-92) でも探せます");
            let cat_of = |d: &PartDef| if d.category.is_empty() { "その他".to_string() } else { d.category.clone() };
            let mut cats: Vec<String> = self.library.iter().map(cat_of).collect();
            cats.sort();
            cats.dedup();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("lib_cat")
                    .selected_text(if self.lib_cat.is_empty() { "すべてのカテゴリ".to_string() } else { self.lib_cat.clone() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.lib_cat, String::new(), "すべてのカテゴリ");
                        for c in &cats {
                            ui.selectable_value(&mut self.lib_cat, c.clone(), c);
                        }
                    });
                ui.checkbox(&mut self.lib_sim_only, "◎のみ").on_hover_text("シミュレーションモデルのある部品だけを表示します");
            });
            let f = self.lib_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
                // grouped by category, then by name
                let mut order: Vec<usize> = (0..self.library.len()).collect();
                order.sort_by_key(|&i| (cat_of(&self.library[i]), self.library[i].name.to_lowercase()));
                let mut last = String::new();
                for i in order {
                    let d = &self.library[i];
                    let c = cat_of(d);
                    if (!self.lib_cat.is_empty() && self.lib_cat != c) || (self.lib_sim_only && d.sim.is_none()) {
                        continue;
                    }
                    if !f.is_empty() {
                        let pins = d.pins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" ");
                        if ![d.name.as_str(), d.description.as_str(), c.as_str(), pins.as_str(), d.footprint_name().as_str()].iter().any(|t| t.to_lowercase().contains(&f)) {
                            continue;
                        }
                    }
                    if c != last {
                        ui.label(egui::RichText::new(&c).small().weak());
                        last = c;
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let selected = self.tool == Tool::Place && self.place_lib == Some(i);
                    if ui.selectable_label(selected, format!("  {}{mark}", d.name)).on_hover_text(lib_hover(d)).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            if let Some(d) = self.place_lib.and_then(|k| self.library.get(k)) {
                ui.group(|ui| {
                    ui.strong(&d.name);
                    ui.small(format!("パッケージ: {}", d.footprint_name()));
                    ui.small(&d.description);
                    if let Some(m) = &d.sim {
                        limits_ui(ui, &m.limits, "lib_limits");
                    }
                    egui::CollapsingHeader::new("ピン表").id_salt("lib_pins").show(ui, |ui| {
                        egui::Grid::new("lib_pin_grid").num_columns(2).show(ui, |ui| {
                            for p in &d.pins {
                                ui.small(&p.number);
                                ui.small(&p.name);
                                ui.end_row();
                            }
                        });
                    });
                    ui.small(if d.sim.is_some() { "シミュレーションモデルあり ◎" } else { "シミュレーションモデルなし (基板設計用)" });
                });
            }
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
    }

    pub(super) fn panel_properties(&mut self, ui: &mut egui::Ui) {
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
                        ui.text_edit_singleline(&mut c.value).on_hover_text(value_hint(c.kind));
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
                        let span = self.slider_span;
                        let mut x = (cur / base).log10().clamp(-span, span);
                        let changed = ui
                            .horizontal(|ui| {
                                ui.label("スライダー");
                                ui.add(egui::Slider::new(&mut x, -span..=span).show_value(false)).on_hover_text(
                                    "ドラッグして値を動かします (中央 = 今の値)。動かせる幅は右の「幅」で変えられます。
下の「自動で再計算」をオンにすると、動かしながら波形や電圧が変わるのを見られます",
                                ).changed()
                            })
                            .inner;
                        ui.horizontal(|ui| {
                            ui.label("幅");
                            for (label, v, tip) in [
                                ("細かい", 0.3, "今の値の 1/2 〜 2 倍。少しずつ調整できます。収束しにくい回路ではこれを使います"),
                                ("ふつう", 1.0, "今の値の 1/10 〜 10 倍"),
                                ("広い", 2.0, "今の値の 1/100 〜 100 倍。大きく動かせますが、極端な値では計算が収束しないことがあります"),
                            ] {
                                if ui.selectable_label((self.slider_span - v).abs() < 1e-9, label).on_hover_text(tip).clicked() {
                                    self.slider_span = v;
                                    // centre on the value now shown, so the narrower range starts from where the part is
                                    self.slider = Some((i, cur, c.value.clone()));
                                }
                            }
                        });
                        if changed {
                            let v = format_value(base * 10f64.powf(x));
                            c.value = format!("{v}{rest}");
                            self.slider = Some((i, base, c.value.clone()));
                        }
                        ui.checkbox(&mut self.live, "自動で再計算").on_hover_text("オンにすると、値を動かすたびに、開いている解析 (DC / 過渡 / AC) をやり直します。重い回路ではオフにしてください");
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
                }
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor)
                    && ui.small_button("値の決め方…").on_hover_text("この部品の値を計算で決める窓を開きます (LEDの抵抗・分圧・フィルタなど)").clicked()
                {
                    self.wiz_open = true;
                }
                if let Some(sym) = &c.custom {
                    ui.label(format!("フットプリント: {}", c.footprint));
                    match &sym.sim {
                        Some(m) => {
                            ui.label(format!("モデル: {}", m.name));
                            // a circuit saved before the notes existed carries the model without them:
                            // take the notes of the same library model (same name, same text)
                            let from_library = self.library.iter().find(|d| d.name == sym.name && d.sim.as_ref().is_some_and(|x| x.text == m.text)).and_then(|d| d.sim.as_ref()).map(|x| &x.limits);
                            limits_ui(ui, if m.limits.is_empty() { from_library.map_or(&[][..], |l| l.as_slice()) } else { &m.limits }, "placed_limits");
                        }
                        None => {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), "シミュレーションモデルなし");
                        }
                    }
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
    }

    /// The settings window opened by right-clicking a part: the same editor as the left panel's
    /// "選択中の部品" section, next to the part. Closes when another thing gets selected.
    pub fn part_popup_window(&mut self, ctx: &egui::Context) {
        let Some((i, pos, serial)) = self.part_popup else { return };
        if self.selected != Some(Sel::Comp(i)) || i >= self.sch.components.len() {
            self.part_popup = None;
            return;
        }
        let c = &self.sch.components[i];
        let title = if c.kind.is_power_symbol() { "部品の設定".to_string() } else { format!("部品の設定: {}", c.refdes) };
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new(("part_popup", serial)))
            .open(&mut open)
            .default_pos(pos + egui::vec2(12.0, 12.0))
            .default_width(260.0)
            .show(ctx, |ui| {
                self.panel_properties(ui);
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) || self.selected != Some(Sel::Comp(i)) {
            self.part_popup = None;
        }
    }

    pub(super) fn panel_erc(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if ui.button("ERC (電気ルールチェック)").on_hover_text("配線ミスの点検。ピンや線の端がつながっていない、電源が短絡している、部品番号がダブっている、などを探して丸で示します (赤紫 = エラー、橙 = 注意)。結果の行をクリックすると、その場所へ移動して直し方を表示します。シミュレーションや基板に進む前にやっておくと安心です").clicked() {
            self.problems = erc(&self.sch);
            self.problems.extend(self.conversion_problems());
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
    }

    pub(super) fn panel_forms(&mut self, ui: &mut egui::Ui) {
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
        if ui.small_button("操作ガイド (F1)").on_hover_text("マウスとキーの操作の一覧を開きます").clicked() {
            self.help_request = Some(crate::help::HelpTab::Schematic);
        }
    }
}

/// Tooltip of a library part: its description, then what its model leaves out.
fn lib_hover(d: &part_lib::PartDef) -> String {
    let mut t = d.description.clone();
    if let Some(m) = &d.sim
        && !m.limits.is_empty() {
            t.push_str("\n\nこのモデルの限界 (実物との違い):");
            for l in &m.limits {
                t.push_str("\n・");
                t.push_str(l);
            }
        }
    t
}

#[cfg(test)]
pub(super) fn lib_hover_for_test(d: &part_lib::PartDef) -> String {
    lib_hover(d)
}

/// "このモデルの限界": a collapsed list of what the model leaves out, so that a result is not
/// mistaken for the real part's behaviour.
pub(super) fn limits_ui(ui: &mut egui::Ui, limits: &[String], id: &str) {
    if limits.is_empty() {
        return;
    }
    egui::CollapsingHeader::new("このモデルの限界 (実物との違い)").id_salt(id).show(ui, |ui| {
        for l in limits {
            ui.horizontal_wrapped(|ui| {
                ui.label("・");
                ui.small(l);
            });
        }
    });
}

/// What can be typed in the value field of a part of this kind (shown when the pointer rests on the field).
fn value_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Capacitor => "容量。例: 100n / 4.7u
100n ESR=20m ESL=5n … 等価直列抵抗・インダクタンスも入れられます
100u IC=3.2 … 過渡解析を始めるときの電圧 (電池の代わりに使えます)",
        Kind::Inductor => "インダクタンス。例: 10u / 1m
10u DCR=0.1 … 巻線抵抗も入れられます",
        Kind::Resistor => "抵抗値。例: 100 / 4.7k / 1Meg (k = 1000、Meg = 100万、m = 1/1000、u = 1/100万、n、p)",
        Kind::VSource => "電源の値。例:
5 … 直流 5V
SIN(0 1 1k) … 正弦波 (中心 振幅 周波数)
PULSE(0 5 100u 1u 1u 400u 1m) … 方形波 (低 高 遅延 立上り 立下り 幅 周期)
PWL(0 0 1m 5) … 折れ線 (時間 電圧 の組)
5 AC 1 … AC解析の入力にする
NOISE(0.05 1Meg) … ノイズ (実効値 最高周波数)。5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k) のように + でつなげます (+ の前後は空白)",
        _ => "部品の値。単位つきの数字は 10k, 100n, 4.7u のように書けます",
    }
}
