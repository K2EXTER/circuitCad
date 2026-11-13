//! The left side panel: tool and part palette, the selected part, the ERC and the file / form extras.

use super::*;

/// The analyses of the "解析" tab: (key, title, what it is for).
const ANALYSIS_SECTIONS: &[(&str, &str, &str)] = &[
    ("dc", "DC動作点 (電圧・電流)", "落ち着いたあとの各点の電圧と部品の電流を見ます。まずここから"),
    ("tran", "過渡解析 (時間の変化)", "時間とともに電圧・電流がどう変わるかを、オシロスコープのように見ます"),
    ("ac", "AC解析 (周波数特性)", "周波数ごとの強さ (ゲイン) と位相を見ます。フィルタやアンプの帯域も。入力にする電源の値を「AC 1」にしてください。「安定性の余裕」は、帰還ループを1か所で切って1周したときのゲイン (一巡伝達関数) を測った波形に使います"),
    ("sweep", "DCスイープ (入力→出力)", "入力電圧を変えたときの出力の変化 (伝達特性) を見ます。電圧の信号を選ぶと、反転する (インバーターの) 特性なら VOH / VOL / 切替電圧 / ノイズマージンを計算します"),
    ("param", "パラメータスイープ (値を振る)", "部品の値を何通りか変えて同じ解析をくり返し、波形を重ねて比べます。「Rを変えるとカットオフがどう動くか」を一度に見られます"),
    ("loopgain", "ループゲイン (帰還の安定性)", "帰還のループを1か所切って、1周したときのゲインを測ります。0 dB を横切る所の位相余裕で、電源やアンプが安定か、振動しそうかが分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("polezero", "極と零 (伝達関数)", "AC解析の結果に分数の式を当てはめて、極と零の周波数を求めます。フィルタのカットオフ・共振の鋭さ (Q)・アンプの補償の効き方が、数字で分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("psrr", "電源除去比 (PSRR)", "電源に乗った揺れ (リップル・雑音) が、出力にどれだけ漏れるかを周波数ごとに見ます。選んだ電源だけを揺らすので、回路図の値を「AC 1」に書き換える必要はありません。レギュレータやアンプの電源にコンデンサが要る理由が、数字で分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("noise", "ノイズ解析", "熱雑音・ショット雑音が出力にどれだけ出るかを見ます。周波数範囲・点数は AC解析の欄を共用。入力電源を指定すると入力換算も表示"),
    ("impedance", "インピーダンス (ネットから見た値)", "あるネットから回路の中を見たインピーダンスを、周波数ごとに求めます。電源ラインの出力インピーダンスとバイパスコンデンサの効き (山と谷)、アンプの出力・入力インピーダンスが分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("variation", "温度とばらつき", "温度を変える、部品の値を許容差の範囲でランダムに変えて何度も計算する、などで、ずれる範囲を調べます。縦棒が高いところほど、よく出る値です。「感度」は、どの部品が結果を決めているかを効く順に並べます"),
    ("spec", "スペック (合否の表)", "作りたい回路の目標値 (出力電圧は 4.75〜5.25V、カットオフは 1kHz 前後など) を書いておくと、解析の結果が合格かどうかを ✓/× で表示します。値を変えるたびに判定がやり直されます"),
    ("selfheat", "自己発熱 (部品の温度上昇)", "電流が流れる抵抗は、自分の電力で温まります。温度がどれだけ上がるか、温度係数のある抵抗は値がどれだけ動くかを、熱抵抗 (K/W) から計算します。シャント抵抗や、精度が要る分圧の抵抗の確認に"),
    ("coverage", "モデルの前提 (何がわからないか)", "シミュレーションが計算に入れていない部品 (モデルがない)、汎用の標準値で計算している部品、モデルの注意書きをまとめて見ます。結果を信じてよい範囲が分かります"),
    ("diagnose", "症状から原因を探す (故障診断)", "動かない回路で測った電圧から、どの部品がどう壊れているかを探します。起こりうる故障を全部計算して、測った値に合う順に並べ、次にどこを測ればよいかも教えます。実物の修理に使えます"),
    ("fault", "故障シミュレーション", "部品を壊した状態 (断線・ショート・逆向き・値の間違い・ピンの断線やブリッジ・電源の低下) で解析し、元の回路と比べます。「どこが壊れると危ないか」を、実物を壊さずに学べます"),
];

/// The analyses in groups by what they are for: (heading, when to use them, section keys). Every key of `ANALYSIS_SECTIONS` is in one group.
const ANALYSIS_GROUPS: &[(&str, &str, &[&str])] = &[
    ("基本", "まずここから。電圧・電流 (DC)、時間の変化 (過渡)、周波数の特性 (AC) を見ます", &["dc", "tran", "ac"]),
    ("値を振って調べる", "入力・部品の値・温度・ばらつきを変えて、結果がどれだけ動くかを見ます", &["sweep", "param", "variation"]),
    ("周波数と安定性", "帰還が安定か、電源の揺れや雑音がどれだけ出るか、ネットのインピーダンスを、周波数ごとに見ます", &["loopgain", "polezero", "psrr", "noise", "impedance"]),
    ("確かめる", "目標の値に合っているか、部品が熱くならないか、計算が何を前提にしているかを確かめます", &["spec", "selfheat", "coverage"]),
    ("壊れたとき", "実物が動かないときの原因探しと、部品が壊れたら回路がどうなるかの練習です", &["diagnose", "fault"]),
];

/// The picture before an analysis's title: what the analysis shows (None for the model assumptions, which no picture says well).
fn analysis_icon(key: &str) -> Option<crate::icons::Icon> {
    use crate::icons::Icon;
    Some(match key {
        "dc" => Icon::Meter,
        "tran" => Icon::Wave,
        "ac" => Icon::Bode,
        "sweep" => Icon::Transfer,
        "param" => Icon::Fan,
        "variation" => Icon::Histogram,
        "loopgain" => Icon::Loop,
        "polezero" => Icon::PoleZero,
        "psrr" => Icon::Ripple,
        "noise" => Icon::Noise,
        "impedance" => Icon::Valley,
        "spec" => Icon::Checklist,
        "selfheat" => Icon::Thermo,
        "diagnose" => Icon::Search,
        "fault" => Icon::Broken,
        _ => return None,
    })
}

mod parts;
mod properties;

impl SchEditor {
    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        // the drawing tab's checks stay at the bottom, in sight however long the lists above grow
        if self.panel_tab == PanelTab::Draw {
            egui::Panel::bottom("sch_draw_checks").frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 })).resizable(false).show(ui, |ui| {
                let bar = ui.spacing().scroll.allocated_width() + 10.0;
                ui.set_max_width((ui.available_width() - bar).max(100.0));
                self.panel_erc(ui);
                if ui.button(i18n::tr("操作ガイド (F1)")).on_hover_text(i18n::tr("マウスとキーの操作の一覧を開きます")).clicked() {
                    self.help_request = Some(crate::help::HelpTab::Schematic);
                }
            });
        }
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
                if !self.selected_part_band(ui, false) {
                    self.panel_properties(ui);
                }
                self.panel_parts(ui);
                self.panel_forms(ui);
            }
            PanelTab::Analyze => {
                // a filter over the names and descriptions of the analyses (both languages)
                ui.add(
                    egui::TextEdit::singleline(&mut self.analysis_filter)
                        .hint_text(i18n::tr("解析を探す (例: 安定・雑音・温度)"))
                        .desired_width(f32::INFINITY),
                )
                .on_hover_text(i18n::tr("解析の名前と説明から探します。空にすると全部に戻ります"));
                // the selected part's values, to change one and run again without going back to the drawing tab
                if self.selected_part_band(ui, true) {
                    crate::widgets::group_gap(ui);
                }
                // what the results rest on: a part without a model is not in the calculation at all
                let cov = self.model_coverage();
                if !cov.ignored.is_empty() {
                    let names: Vec<&str> = cov.ignored.iter().map(|p| p.refdes.as_str()).collect();
                    ui.colored_label(crate::theme::WARN, i18n::trf!("⚠ モデルがなく、計算に入っていない部品: {}", names.join("・")))
                        .on_hover_text(i18n::tr("この部品は、回路にないものとして計算されます。動作や電流に関わる部品なら、結果は実物と違います。下の「モデルの前提」で、ほかの注意もまとめて見られます"));
                }
                // during a lesson every section is open, so the lesson's steps can be found; otherwise one at a time
                let lesson = self.lesson.is_some();
                let filter = self.analysis_filter.trim().to_lowercase();
                let matches = |key: &str, title: &str, hint: &str| {
                    filter.is_empty()
                        || [title, hint, key].iter().any(|s| s.to_lowercase().contains(&filter) || i18n::tr_dyn(s).to_lowercase().contains(&filter))
                };
                let mut shown = 0;
                for (heading, when, keys) in ANALYSIS_GROUPS {
                    let sections: Vec<_> =
                        ANALYSIS_SECTIONS.iter().filter(|(k, t, h)| keys.contains(k) && matches(k, t, h)).collect();
                    if sections.is_empty() {
                        continue;
                    }
                    if shown > 0 {
                        ui.add_space(4.0);
                    }
                    crate::icons::help_small(ui, heading, when);
                    for (key, title, hint) in sections {
                        shown += 1;
                        let id = egui::Id::new(("analysis_section", *key, self.sec_gen, lesson));
                        let open = lesson || self.focus_section.unwrap_or("dc") == *key;
                        let was = egui::collapsing_header::CollapsingState::load(ui.ctx(), id).map_or(open, |s| s.is_open());
                        let done = self.analysis_has_result(key);
                        crate::widgets::section_header_marked(ui, id, open, title, hint, Some(analysis_icon(key)), done, |ui| {
                            match *key {
                                "dc" => self.panel_dc(ui),
                                "tran" => self.panel_tran(ui),
                                "ac" => self.panel_ac(ui),
                                "sweep" => self.panel_sweep(ui),
                                "param" => self.panel_param(ui),
                                "noise" => self.panel_noise(ui),
                                "psrr" => self.panel_psrr(ui),
                                "diagnose" => self.panel_troubleshoot(ui),
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
                        // opening one section closes the others (outside a lesson): the open one gets the height
                        let now = egui::collapsing_header::CollapsingState::load(ui.ctx(), id).is_some_and(|s| s.is_open());
                        if !lesson && now && !was {
                            self.focus_section = Some(key);
                            self.sec_gen += 1;
                        }
                    }
                }
                if shown == 0 {
                    ui.weak(i18n::tr("その言葉に合う解析はありません"));
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
        // the findings scroll in a box of their own: the checks stay at the bottom of the panel, and a long list must not cover it all
        let tall = (ui.ctx().content_rect().height() * 0.35).max(120.0);
        egui::ScrollArea::vertical().id_salt("erc_findings").max_height(tall).auto_shrink([false, true]).show(ui, |ui| {
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
        });
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
            // beside the left panel and as tall as the screen allows, so a long tab (the package, the ratings) shows whole
            let tall = (ui.ctx().content_rect().height() - crate::widgets::beside_panel().y - 40.0).clamp(560.0, 900.0);
            egui::Window::new(if form.editing().is_some() { i18n::tr("部品を編集") } else { i18n::tr("部品を作成") }).open(&mut open).default_pos(crate::widgets::beside_panel()).default_size([980.0, tall]).resizable(true).show(ui.ctx(), |ui| {
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
    }
}

#[cfg(test)]
pub(super) fn lib_hover_for_test(d: &part_lib::PartDef) -> String {
    parts::lib_hover(d)
}

impl SchEditor {
    /// The analysis tab with one section open, by its key (`dc`, `variation`, `coverage` ...: `ANALYSIS_SECTIONS`). False for
    /// a key that is not a section (command line `--section`).
    pub fn open_analysis_section(&mut self, key: &str) -> bool {
        match ANALYSIS_SECTIONS.iter().find(|(k, ..)| *k == key) {
            Some((k, ..)) => {
                self.show_analysis_section(k);
                true
            }
            None => false,
        }
    }

    /// Whether the analysis `key` has a result to show (a ✓ on its heading, so the learner sees what has been done).
    fn analysis_has_result(&self, key: &str) -> bool {
        match key {
            "dc" => self.dc.is_some(),
            "tran" => self.tran.is_some(),
            "ac" => self.ac.as_ref().is_some_and(|a| !a.loop_run && !a.psrr_run),
            "loopgain" => self.ac.as_ref().is_some_and(|a| a.loop_run),
            "psrr" => self.ac.as_ref().is_some_and(|a| a.psrr_run),
            "sweep" => self.sweep.is_some(),
            "param" => self.param.is_some(),
            "noise" => self.noise.is_some(),
            "impedance" => self.z.is_some(),
            "polezero" => self.pz.is_some(),
            "variation" => self.mc.is_some() || self.sens.is_some(),
            "selfheat" => self.heat_view.is_some() || self.htran.is_some(),
            "diagnose" => self.diag.result.is_some(),
            "fault" => self.fault_view.is_some() || !self.fault_scan.is_empty(),
            _ => false,
        }
    }

    /// Whether the analysis tab is shown with section `key` asked for (tests of `--section`).
    pub fn analysis_section_open(&self, key: &str) -> bool {
        self.panel_tab == PanelTab::Analyze && self.focus_section == Some(key)
    }

    /// The settings of the selected part as a band that folds by a click (the drawing tab, and the analysis tab with its own
    /// open state). False when no part is selected.
    fn selected_part_band(&mut self, ui: &mut egui::Ui, analyze: bool) -> bool {
        let Some(Sel::Comp(i)) = self.selected else { return false };
        let Some(c) = self.sch.components.get(i) else { return false };
        if !analyze {
            crate::widgets::group_gap(ui);
        }
        let name = if c.kind.is_power_symbol() { String::new() } else { format!(" {}", c.refdes) };
        let title = egui::RichText::new(i18n::trf!("選択中の部品{name}")).strong();
        let open = if analyze { self.props_open_analyze } else { self.props_open };
        let (_, clicked) = crate::widgets::band_header(ui, if analyze { "props_analyze" } else { "props" }, title, open, true, |ui| {
            crate::widgets::well(ui, |ui| self.properties_inner(ui, false));
        });
        if clicked {
            if analyze {
                self.props_open_analyze = !self.props_open_analyze;
            } else {
                self.props_open = !self.props_open;
            }
        }
        true
    }
}

/// What a model is and where it comes from (docs/model-policy.md, section 4): its kind and source, from the model's own
/// `origin` or, for a copy saved before that field, from the bundled table measured on the same model text.
pub(super) fn model_origin(part: &str, m: &sch_core::SimModel) -> (Option<sch_core::ModelOrigin>, Option<&'static part_lib::model_checks::ModelChecks>) {
    let checks = part_lib::model_checks::for_model(part, &m.text);
    // a copy without the field (placed or installed before it existed) takes the bundled part's, when it is the same model
    let origin = m
        .origin
        .clone()
        .or_else(|| part_lib::model_checks::bundled_origin(part, &m.text).cloned())
        .or_else(|| checks.map(|c| sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, c.source.clone())));
    (origin, checks)
}

/// One line on what kind of model it is.
pub(super) fn origin_kind_text(o: Option<&sch_core::ModelOrigin>) -> &'static str {
    use sch_core::OriginKind::*;
    match o.map(|o| o.kind) {
        Some(Equivalent) => i18n::tr("データシートの値から作った等価モデル (メーカーのモデルではありません)"),
        Some(Vendor) => i18n::tr("メーカーのモデル (取り込んだファイルをそのまま使っています)"),
        Some(Generic) => i18n::tr("一般のモデル (理想の素子や代表的な値で動きを表したもの。特定の型番のデータシートとは比べていません)"),
        None => i18n::tr("モデルの出どころ: 記録なし (データシートとはまだ比べていません)"),
    }
}

/// "モデルの出どころ" and, when the model was measured against its data sheet, the table of the figures side by side.
pub(super) fn origin_ui(ui: &mut egui::Ui, part: &str, m: &sch_core::SimModel, id: &str) {
    let (origin, checks) = model_origin(part, m);
    let kind = origin_kind_text(origin.as_ref());
    let line = ui.small(kind);
    if origin.is_none() {
        line.on_hover_text(i18n::tr("このモデルが何から作られたか、データシートとどれだけ合うかは、まだ記録がありません。結果は目安として使い、大事な値はデータシートで確かめてください"));
    }
    if let Some(src) = origin.as_ref().map(|o| o.source.as_str()).filter(|s| !s.is_empty()) {
        ui.small(i18n::trf!("出典: {}", src));
    }
    let Some(c) = checks else { return };
    let header = i18n::trf!("データシートとの比べ ({} 項目)", c.rows.len());
    egui::CollapsingHeader::new(header).id_salt(id).show(ui, |ui| {
        ui.small(i18n::tr("モデルの値は、データシートの条件に近い回路を、このソフトのシミュレータで測ったものです。緑の値は、決めた幅から外れると開発のテストで止まります。値にマウスを置くと、条件と差が出ます"));
        // in a horizontal scroll area: a wide table must not push the side panel wider
        egui::ScrollArea::horizontal().id_salt((id, "scroll")).show(ui, |ui| {
            egui::Grid::new((id, "grid")).num_columns(3).striped(true).spacing([8.0, 2.0]).show(ui, |ui| {
                for h in [i18n::tr("項目"), i18n::tr("データシート"), i18n::tr("モデル")] {
                    ui.small(egui::RichText::new(h).strong());
                }
                ui.end_row();
                for r in &c.rows {
                    // min / typ / max and the conditions on hover
                    let detail = format!("{}  (min / typ / max)\n{}", r.datasheet, r.condition);
                    ui.small(i18n::tr_dyn(&r.item)).on_hover_text(&detail);
                    ui.small(if r.target.is_empty() { &r.datasheet } else { &r.target }).on_hover_text(&detail);
                    // three columns so that the table fits the side panel: the difference and whether a test checks it on hover
                    let check = if r.tested { i18n::tr("決めた幅に入っていることを、テストで確かめています") } else { i18n::tr("比べて表に出すだけで、幅は決めていません") };
                    let model = egui::RichText::new(&r.model);
                    ui.small(if r.tested { model.color(crate::theme::OK) } else { model }).on_hover_text(i18n::trf!("差: {}\n{}", r.diff, check));
                    ui.end_row();
                }
            });
        });
        ui.small(i18n::tr("部品そのもののばらつき (typ と min / max の差) は、モデルの誤差より大きいことがあります。"));
    });
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
