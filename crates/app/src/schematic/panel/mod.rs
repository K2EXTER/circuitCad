//! The left side panel: tool and part palette, the selected part, the ERC and the file / form extras.

use super::*;

/// The analyses of the "解析" tab: (key, title, open at first, what it is for).
const ANALYSIS_SECTIONS: &[(&str, &str, bool, &str)] = &[
    ("dc", "DC動作点 (電圧・電流)", true, "落ち着いたあとの各点の電圧と部品の電流を見ます。まずここから"),
    ("tran", "過渡解析 (時間の変化)", true, "時間とともに電圧・電流がどう変わるかを、オシロスコープのように見ます"),
    ("ac", "AC解析 (周波数特性)", true, "周波数ごとの強さ (ゲイン) と位相を見ます。フィルタやアンプの帯域も。入力にする電源の値を「AC 1」にしてください。「安定性の余裕」は、帰還ループを1か所で切って1周したときのゲイン (一巡伝達関数) を測った波形に使います"),
    ("sweep", "DCスイープ (入力→出力)", false, "入力電圧を変えたときの出力の変化 (伝達特性) を見ます。電圧の信号を選ぶと、反転する (インバーターの) 特性なら VOH / VOL / 切替電圧 / ノイズマージンを計算します"),
    ("param", "パラメータスイープ (値を振る)", false, "部品の値を何通りか変えて同じ解析をくり返し、波形を重ねて比べます。「Rを変えるとカットオフがどう動くか」を一度に見られます"),
    ("loopgain", "ループゲイン (帰還の安定性)", false, "帰還のループを1か所切って、1周したときのゲインを測ります。0 dB を横切る所の位相余裕で、電源やアンプが安定か、振動しそうかが分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("polezero", "極と零 (伝達関数)", false, "AC解析の結果に分数の式を当てはめて、極と零の周波数を求めます。フィルタのカットオフ・共振の鋭さ (Q)・アンプの補償の効き方が、数字で分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("psrr", "電源除去比 (PSRR)", false, "電源に乗った揺れ (リップル・雑音) が、出力にどれだけ漏れるかを周波数ごとに見ます。選んだ電源だけを揺らすので、回路図の値を「AC 1」に書き換える必要はありません。レギュレータやアンプの電源にコンデンサが要る理由が、数字で分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("noise", "ノイズ解析", false, "熱雑音・ショット雑音が出力にどれだけ出るかを見ます。周波数範囲・点数は AC解析の欄を共用。入力電源を指定すると入力換算も表示"),
    ("impedance", "インピーダンス (ネットから見た値)", false, "あるネットから回路の中を見たインピーダンスを、周波数ごとに求めます。電源ラインの出力インピーダンスとバイパスコンデンサの効き (山と谷)、アンプの出力・入力インピーダンスが分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("variation", "温度とばらつき", false, "温度を変える、部品の値を許容差の範囲でランダムに変えて何度も計算する、などで、ずれる範囲を調べます。縦棒が高いところほど、よく出る値です。「感度」は、どの部品が結果を決めているかを効く順に並べます"),
    ("spec", "スペック (合否の表)", false, "作りたい回路の目標値 (出力電圧は 4.75〜5.25V、カットオフは 1kHz 前後など) を書いておくと、解析の結果が合格かどうかを ✓/× で表示します。値を変えるたびに判定がやり直されます"),
    ("selfheat", "自己発熱 (部品の温度上昇)", false, "電流が流れる抵抗は、自分の電力で温まります。温度がどれだけ上がるか、温度係数のある抵抗は値がどれだけ動くかを、熱抵抗 (K/W) から計算します。シャント抵抗や、精度が要る分圧の抵抗の確認に"),
    ("coverage", "モデルの前提 (何がわからないか)", false, "シミュレーションが計算に入れていない部品 (モデルがない)、汎用の標準値で計算している部品、モデルの注意書きをまとめて見ます。結果を信じてよい範囲が分かります"),
    ("fault", "故障シミュレーション", false, "部品を壊した状態 (断線・ショート・逆向き・値の間違い・ピンの断線やブリッジ・電源の低下) で解析し、元の回路と比べます。「どこが壊れると危ないか」を、実物を壊さずに学べます"),
];

mod parts;
mod properties;

impl SchEditor {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        // the panel is taller than a small window: let it scroll instead of hiding the bottom controls
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // leave room on the right so this bar does not sit on top of the inner part-list bar
            let bar = ui.spacing().scroll.allocated_width() + 10.0;
            ui.set_max_width((ui.available_width() - bar).max(100.0));
            self.side_panel_body(ui)
        });
    }

    pub(super) fn side_panel_body(&mut self, ui: &mut egui::Ui) {
        self.poll_fault_scan(ui.ctx());
        crate::widgets::tab_bar(ui, &mut self.panel_tab, &[
            (PanelTab::Draw, "作図", "部品を置く、配線する、部品の値を変える、配線ミスを点検する (ERC)"),
            (PanelTab::Analyze, "解析", "シミュレーションを実行して、電圧・波形・周波数特性・ノイズ・温度やばらつきの影響を見る"),
        ]);
        match self.panel_tab {
            PanelTab::Draw => {
                self.panel_tools(ui);
                // the selected part's settings sit right under quick add, above the long part lists
                ui.spacing_mut().item_spacing.y = 5.0;
                if let Some(Sel::Comp(i)) = self.selected
                    && let Some(c) = self.sch.components.get(i)
                {
                    // the settings of the selected part: a band like the part lists below, folded by a click
                    crate::widgets::group_gap(ui);
                    let name = if c.kind.is_power_symbol() { String::new() } else { format!(" {}", c.refdes) };
                    let title = egui::RichText::new(i18n::trf!("選択中の部品{name}")).strong();
                    let (_, clicked) = crate::widgets::band_header(ui, "props", title, self.props_open, true, |ui| {
                        crate::widgets::well(ui, |ui| self.properties_inner(ui, false));
                    });
                    if clicked {
                        self.props_open = !self.props_open;
                    }
                } else {
                    self.panel_properties(ui);
                }
                self.panel_parts(ui);
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
                    let b = crate::widgets::primary_button_wide(ui, !in_lesson, label);
                    if b.on_hover_text(if in_lesson { i18n::tr("レッスン中は、手順に出てくる欄を探せるように、すべて開いています") } else { tip }).clicked() {
                        self.sec_gen += 1;
                        self.sec_mode = if all_open { 2 } else { 1 };
                    }
                });
                // what the results rest on: a part without a model is not in the calculation at all
                let cov = self.model_coverage();
                if !cov.ignored.is_empty() {
                    let names: Vec<&str> = cov.ignored.iter().map(|p| p.refdes.as_str()).collect();
                    ui.colored_label(crate::theme::WARN, i18n::trf!("⚠ モデルがなく、計算に入っていない部品: {}", names.join("・")))
                        .on_hover_text(i18n::tr("この部品は、回路にないものとして計算されます。動作や電流に関わる部品なら、結果は実物と違います。下の「モデルの前提」で、ほかの注意もまとめて見られます"));
                }
                let lesson = self.lesson.is_some();
                let (open_all, close_all) = (self.sec_mode == 1 || lesson, self.sec_mode == 2 && !lesson);
                for (key, title, basic, hint) in ANALYSIS_SECTIONS {
                    let id = egui::Id::new(("analysis_section", *key, self.sec_gen, lesson));
                    crate::widgets::section_header(ui, id, open_all || (*basic && !close_all), title, hint, |ui| {
                        match *key {
                            "dc" => self.panel_dc(ui),
                            "tran" => self.panel_tran(ui),
                            "ac" => self.panel_ac(ui),
                            "sweep" => self.panel_sweep(ui),
                            "param" => self.panel_param(ui),
                            "noise" => self.panel_noise(ui),
                            "psrr" => self.panel_psrr(ui),
                            "loopgain" => self.panel_loop(ui),
                            "impedance" => self.panel_z(ui),
                            "polezero" => self.panel_pz(ui),
                            "variation" => self.panel_variation(ui),
                            "spec" => self.panel_spec(ui),
                            "coverage" => self.panel_coverage(ui),
                            "selfheat" => {
                                self.panel_selfheat(ui);
                                self.panel_heat_tran(ui);
                            }
                            _ => self.panel_fault(ui),
                        }
                    });
                }
                self.panel_results(ui);
            }
        }
        self.result_windows(ui.ctx());
    }

    /// Tools, undo/redo and quick add: the top of the draw tab.
    pub(super) fn panel_tools(&mut self, ui: &mut egui::Ui) {
        // the tools, undo and quick add are one group: close together
        ui.spacing_mut().item_spacing.y = crate::theme::SPACE_S;
        // the four tools are one segmented bar: joined, equal width, one chosen (the buttons below it act; these switch the mode)
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let tools: [(crate::icons::Icon, &str, Tool, &str); 4] = [
                (crate::icons::Icon::Select, "選択", Tool::Select, "部品や配線をクリックで選択、部品はドラッグで移動できます。選ぶと値(抵抗値など)を変えられます"),
                (crate::icons::Icon::Wire, i18n::tr_ctx("回路図", "配線"), Tool::Wire, "端点をクリックして線を引きます。クリックするたびに折れ線が続き、ダブルクリックで終わります。右クリックで配線を確定し、配線していないときの右クリックで選択ツールに戻ります"),
                (crate::icons::Icon::Bus, "バス", Tool::Bus, "太い青の線でバス(信号の束)を描きます。バスの線は何もつなぎません(見た目と点検のためのものです)。名前は、ラベルツールで D[0..7] と書いて、バスの線の上に置きます。つなぐ各ピンには、ラベルツールで D[0..7] と書いたまま、ピンを順にクリックすると、D0 D1 D2 … と順に付きます"),
                (crate::icons::Icon::Label, "ラベル", Tool::Label, "同じ名前のラベルを付けた場所は、線でつながなくても同じ配線 (ネット) になります。図がすっきりします"),
            ];
            let width = ui.available_width() / tools.len() as f32;
            for (n, (icon, label, tool, tip)) in tools.into_iter().enumerate() {
                let r = if n == 0 { 4 } else { 0 };
                let e = if n == 3 { 4 } else { 0 };
                let corners = egui::CornerRadius { nw: r, sw: r, ne: e, se: e };
                if crate::icons::segment_button(ui, icon, label, "", self.tool == tool, true, tip, width, corners).clicked() {
                    self.tool = tool;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // an edit not yet written to the history (mid-drag, just made) can be undone too
            let can_undo = !self.undo.is_empty() || self.sch != self.committed;
            let half = egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 };
            let other = egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 };
            let width = ui.available_width() / 2.0;
            if crate::icons::segment_button(ui, crate::icons::Icon::Undo, "元に戻す", "Ctrl+Z", false, can_undo, "直前の操作を取り消します (Ctrl+Z)", width, half).clicked() {
                self.undo();
            }
            if crate::icons::segment_button(ui, crate::icons::Icon::Redo, "やり直し", "Ctrl+Y", false, !self.redo.is_empty(), "取り消した操作をやり直します (Ctrl+Y)", width, other).clicked() {
                self.redo();
            }
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "クイック追加 (A)"), "クイック追加 (A)").on_hover_text(i18n::tr("部品の名前を打って探し、↑↓と Enter で置く部品にします。回路図の上で A キーを押しても開きます。お気に入りと最近使った部品が先頭に出ます")).clicked() {
            self.open_quick_add();
        }
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text).on_hover_text(i18n::tr("ネットの名前 (例: VCC、OUT)。同じ名前のラベルを付けた所は、線でつながなくても同じ配線になります。D[0..7] のように書くと、バスの名前です: バスの線の上をクリックするとバスの名前が付き、ほかの所をクリックすると D0 D1 D2 … と順に付きます"));
            ui.checkbox(&mut self.label_step, i18n::tr("番号を自動で進める")).on_hover_text(i18n::tr("オンにすると、名前の終わりの数字を、ラベルを付けるたびに 1 つ進めます (D0 → D1 → D2 …)"));
            self.port_tool_ui(ui);
        }
    }

    pub(super) fn panel_erc(&mut self, ui: &mut egui::Ui) {
        crate::widgets::group_gap(ui);
        ui.spacing_mut().item_spacing.y = crate::theme::SPACE_S;
        if self.lit(crate::widgets::primary_button_wide(ui, true, "ERC (電気ルールチェック)"), "ERC (電気ルールチェック)").on_hover_text(i18n::tr("配線ミスの点検。ピンや線の端がつながっていない、電源が短絡している、部品番号がダブっている、などを探して丸で示します (赤紫 = エラー、橙 = 注意)。結果の行をクリックすると、その場所へ移動して直し方を表示します。シミュレーションや基板に進む前にやっておくと安心です")).clicked() {
            self.problems = erc(&self.sch);
            self.problems.extend(self.conversion_problems());
            self.erc_focus = None;
            self.erc_ran = true;
            let errors = self.problems.iter().filter(|p| p.severity == Severity::Error).count();
            self.status = format!("ERC: エラー {} 件、注意 {} 件", errors, self.problems.len() - errors);
        }
        // the other two checks of the design: its requirements table, and areas left out of the ERC while drawing
        let skip_enabled = self.selection_box_exists();
        let bar = crate::widgets::segment_row(ui, &[("設計の要件の表…", true), ("選択を描きかけにする", skip_enabled)]);
        if bar[0].clone().on_hover_text(i18n::tr("「設計の要件」の表を開きます。ネットの役割・近くに置く部品の組・離しておくものを書く所です (「編集」メニューの「設計の要件 (表)」と同じ)")).clicked() {
            self.open_requirements();
        }
        if bar[1].clone().on_hover_text(i18n::tr("選んだ部品・配線を囲む範囲を「描きかけ」にして、その中のERCの指摘を出さなくします。作りながら出るエラーの山を減らすための機能です。範囲は灰色の枠で表示され、完成したら下の×で外します。保存もされます")).clicked() {
            self.skip_selection();
        }
        self.erc_skip_ui(ui);
        if self.erc_ran && self.problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 問題は見つかりませんでした"));
        }
        let mut jump = None;
        let mut jump_sheet = 0;
        let mut make_nc: Option<(String, String)> = None;
        for (k, p) in self.problems.iter().enumerate() {
            let (mark, color) = match p.severity {
                Severity::Error => ("✖", crate::theme::ERROR),
                Severity::Warning => ("▲", crate::theme::WARN),
            };
            let sheet = if self.sch.sheet_count() > 1 && p.at.is_some() { format!("[{}] ", self.sch.sheet_name(p.sheet)) } else { String::new() };
            let label = egui::RichText::new(format!("{mark} {sheet}{}", i18n::tr_dyn(&p.message))).color(color);
            let tip = if p.at.is_some() { "クリックで、その場所へ移動して説明を表示します" } else { "クリックで説明を表示します" };
            if ui.link(label).on_hover_text(tip).clicked() {
                self.erc_focus = Some(k);
                jump = p.at;
                jump_sheet = p.sheet;
            }
            if self.erc_focus == Some(k) {
                ui.indent(("erc_hint", k), |ui| {
                    ui.label(i18n::tr_dyn(&p.hint));
                    if let Some((refdes, pin)) = &p.pin
                        && ui.small_button(i18n::tr("使わないピン (NC) にする")).on_hover_text(i18n::tr("このピンは、つながないのが正しいときに押します (使わない出力、データシートが開放と言うピンなど)。印の × が付いて、ERC はこのピンを聞かなくなります。つなぎ忘れなら、押さずに配線しましょう")).clicked()
                    {
                        make_nc = Some((refdes.clone(), pin.clone()));
                    }
                });
            }
        }
        if let Some(at) = jump {
            self.show_sheet(jump_sheet);
            self.pan = Vec2::new(at.0 as f32, at.1 as f32);
        }
        if let Some((refdes, pin)) = make_nc {
            self.sch.set_no_connect(&refdes, &pin, true);
            self.problems = erc(&self.sch);
            self.problems.extend(self.conversion_problems());
            self.erc_focus = None;
            self.status = format!("{refdes} ピン{pin} を「使わない (NC)」にしました。印を外すには、部品を選んで「使わないピン」のチェックを外します");
        }
    }

    pub(super) fn panel_forms(&mut self, ui: &mut egui::Ui) {
        crate::widgets::group_gap(ui);
        if let Some(mut form) = self.form.take() {
            let mut open = true;
            let mut saved = None;
            egui::Window::new(if form.editing().is_some() { i18n::tr("部品を編集") } else { i18n::tr("部品を作成") }).open(&mut open).default_size([980.0, 660.0]).resizable(true).show(ui.ctx(), |ui| {
                saved = form.ui(ui);
            });
            // the file buttons of the form: the library belongs to the editor
            match form.request.take() {
                Some(crate::part_form::FormRequest::ImportJson) => {
                    let before = self.status.clone();
                    self.import_part_json();
                    if self.status != before {
                        form.pick_msg = Some((self.status.starts_with("ライブラリに追加しました"), self.status.clone()));
                    }
                }
                Some(crate::part_form::FormRequest::ImportKicad) => self.open_kicad_import(),
                None => {}
            }
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
        self.kicad_window(ui.ctx());
        if ui.button(i18n::tr("操作ガイド (F1)")).on_hover_text(i18n::tr("マウスとキーの操作の一覧を開きます")).clicked() {
            self.help_request = Some(crate::help::HelpTab::Schematic);
        }
    }
}

#[cfg(test)]
pub(super) fn lib_hover_for_test(d: &part_lib::PartDef) -> String {
    parts::lib_hover(d)
}

/// "このモデルの限界": a collapsed list of what the model leaves out, so that a result is not
/// mistaken for the real part's behaviour.
pub(super) fn limits_ui(ui: &mut egui::Ui, limits: &[String], id: &str) {
    if limits.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(i18n::tr("このモデルの限界 (実物との違い)")).id_salt(id).show(ui, |ui| {
        for l in limits {
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr("・"));
                ui.small(l);
            });
        }
    });
}
