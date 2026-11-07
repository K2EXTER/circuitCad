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
    ("noise", "ノイズ解析", false, "熱雑音・ショット雑音が出力にどれだけ出るかを見ます。周波数範囲・点数は AC解析の欄を共用。入力電源を指定すると入力換算も表示"),
    ("impedance", "インピーダンス (ネットから見た値)", false, "あるネットから回路の中を見たインピーダンスを、周波数ごとに求めます。電源ラインの出力インピーダンスとバイパスコンデンサの効き (山と谷)、アンプの出力・入力インピーダンスが分かります。周波数範囲・点数は AC解析の欄を共用"),
    ("variation", "温度とばらつき", false, "温度を変える、部品の値を許容差の範囲でランダムに変えて何度も計算する、などで、ずれる範囲を調べます。縦棒が高いところほど、よく出る値です"),
    ("spec", "スペック (合否の表)", false, "作りたい回路の目標値 (出力電圧は 4.75〜5.25V、カットオフは 1kHz 前後など) を書いておくと、解析の結果が合格かどうかを ✓/× で表示します。値を変えるたびに判定がやり直されます"),
    ("selfheat", "自己発熱 (部品の温度上昇)", false, "電流が流れる抵抗は、自分の電力で温まります。温度がどれだけ上がるか、温度係数のある抵抗は値がどれだけ動くかを、熱抵抗 (K/W) から計算します。シャント抵抗や、精度が要る分圧の抵抗の確認に"),
    ("coverage", "モデルの前提 (何がわからないか)", false, "シミュレーションが計算に入れていない部品 (モデルがない)、汎用の標準値で計算している部品、モデルの注意書きをまとめて見ます。結果を信じてよい範囲が分かります"),
    ("fault", "故障シミュレーション", false, "部品を壊した状態 (断線・ショート・逆向き・値の間違い・ピンの断線やブリッジ・電源の低下) で解析し、元の回路と比べます。「どこが壊れると危ないか」を、実物を壊さずに学べます"),
];

/// Which family of the tree a library part belongs to under the basic part `k` (None = not under this one).
fn part_family(k: Kind, d: &PartDef) -> Option<&'static str> {
    match k {
        Kind::Capacitor => d.capacitor_family(),
        _ => match (k, d.symbol().glyph.as_str()) {
            (Kind::Diode, "diode") => Some("整流・スイッチング"),
            (Kind::Diode, "schottky") => Some("ショットキー"),
            (Kind::Diode, "zener") => Some("ツェナー"),
            (Kind::Npn, "npn") => Some("NPN"),
            (Kind::Npn, "pnp") => Some("PNP"),
            (Kind::Npn, "nmos") => Some("MOSFET Nch"),
            (Kind::Npn, "pmos") => Some("MOSFET Pch"),
            _ => None,
        },
    }
}

/// One line about each family, shown under its name in the tree.
fn family_tip(family: &str) -> &'static str {
    match family {
        "整流・スイッチング" => "電流を一方向に流します。1N400x は電源の整流、1N4148 は小信号の高速スイッチ用です",
        "ショットキー" => "順方向の電圧降下が小さく (約0.3〜0.5V) 高速。電源の整流や逆流防止に向きます。逆方向の漏れ電流は大きめです",
        "ツェナー" => "逆向きにつなぐと、決まった電圧 (ツェナー電圧) で電流が流れ、電圧を一定に保ちます。基準電圧や簡単な保護に使います",
        "NPN" => "ベース電流で、コレクタからエミッタへ流れる電流を制御します。負荷を GND 側で切り替える (ローサイド) のに使います",
        "PNP" => "NPN と電流の向きが逆。電源側で負荷を切り替える (ハイサイド) のに使います",
        "MOSFET Nch" => "ゲート電圧で制御します。ゲートに電流がほとんど要らず、大電流のスイッチに向きます (GND 側で切り替える)",
        "MOSFET Pch" => "N チャネルと極性が逆。電源側で負荷を切り替える (ハイサイド) のに使います",
        _ => cap_tip(family),
    }
}

fn cap_tip(family: &str) -> &'static str {
    match family {
        "電解" => "容量が大きく、電源の平滑に使います。極性があり、逆につなぐと壊れます。ESR が大きめです",
        "タンタル" => "小さくて容量が大きく、ESR は電解より小さめ。極性があり、耐圧に余裕を見ないと壊れます",
        "フィルム" => "極性なし。ESR・ESL が小さく、温度や電圧で容量が変わりにくいので、フィルタやタイミングに向きます",
        _ => "極性なし。小さくて高い周波数に強い、チップ部品の定番です。電圧がかかると容量が減ります",
    }
}

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
                    let title = egui::RichText::new(format!("選択中の部品{name}")).strong();
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
                    if b.on_hover_text(if in_lesson { "レッスン中は、手順に出てくる欄を探せるように、すべて開いています" } else { tip }).clicked() {
                        self.sec_gen += 1;
                        self.sec_mode = if all_open { 2 } else { 1 };
                    }
                });
                // what the results rest on: a part without a model is not in the calculation at all
                let cov = self.model_coverage();
                if !cov.ignored.is_empty() {
                    let names: Vec<&str> = cov.ignored.iter().map(|p| p.refdes.as_str()).collect();
                    ui.colored_label(crate::theme::WARN, format!("⚠ モデルがなく、計算に入っていない部品: {}", names.join("・")))
                        .on_hover_text("この部品は、回路にないものとして計算されます。動作や電流に関わる部品なら、結果は実物と違います。下の「モデルの前提」で、ほかの注意もまとめて見られます");
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
                (crate::icons::Icon::Wire, "配線", Tool::Wire, "端点をクリックして線を引きます。クリックするたびに折れ線が続き、ダブルクリックで終わります。右クリックで配線を確定し、配線していないときの右クリックで選択ツールに戻ります"),
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
        if self.lit(crate::widgets::primary_button_wide(ui, true, "クイック追加 (A)"), "クイック追加 (A)").on_hover_text("部品の名前を打って探し、↑↓と Enter で置く部品にします。回路図の上で A キーを押しても開きます。お気に入りと最近使った部品が先頭に出ます").clicked() {
            self.open_quick_add();
        }
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text).on_hover_text("ネットの名前 (例: VCC、OUT)。同じ名前のラベルを付けた所は、線でつながなくても同じ配線になります。D[0..7] のように書くと、バスの名前です: バスの線の上をクリックするとバスの名前が付き、ほかの所をクリックすると D0 D1 D2 … と順に付きます");
            ui.checkbox(&mut self.label_step, "番号を自動で進める").on_hover_text("オンにすると、名前の終わりの数字を、ラベルを付けるたびに 1 つ進めます (D0 → D1 → D2 …)");
            self.port_tool_ui(ui);
        }
    }

    /// The part lists and the part-making buttons (below the tools and the selected part's settings).
    pub(super) fn panel_parts(&mut self, ui: &mut egui::Ui) {
        crate::widgets::group_gap(ui);
        ui.spacing_mut().item_spacing.y = 5.0;
        let mut chosen = None;
        // library parts hang under the basic part of their kind (capacitor, diode, transistor), grouped by family
        type Family<'a> = (Kind, Vec<(&'a str, Vec<usize>)>);
        let families: Vec<Family> = [Kind::Capacitor, Kind::Diode, Kind::Npn]
            .into_iter()
            .map(|k| {
                let names: &[&str] = match k {
                    Kind::Capacitor => &["積層セラミック", "電解", "タンタル", "フィルム"],
                    Kind::Diode => &["整流・スイッチング", "ショットキー", "ツェナー"],
                    _ => &["NPN", "PNP", "MOSFET Nch", "MOSFET Pch"],
                };
                let groups = names
                    .iter()
                    .map(|f| (*f, (0..self.library.len()).filter(|&i| part_family(k, &self.library[i]) == Some(*f)).collect::<Vec<_>>()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect::<Vec<_>>();
                (k, groups)
            })
            .filter(|(_, g)| !g.is_empty())
            .collect();
        let basic_title = if self.basic_open { egui::RichText::new("基本の部品").strong() } else { egui::RichText::new("基本の部品") };
        let (_, basic_clicked) = crate::widgets::band_header(ui, "basic", basic_title, self.basic_open, true, |ui| {
            crate::widgets::well(ui, |ui| {
            ui.small("クリックして配置 / Rキーで回転");
            for k in Kind::PALETTE {
                let selected = self.tool == Tool::Place && self.place_lib.is_none() && self.place == *k;
                let mut click = false;
                if let Some((_, cap_kinds)) = families.iter().find(|(fk, _)| fk == k) {
                    // a folder, not a button: it opens to the plain part and the kinds from the library
                    let title = match k {
                        Kind::Npn => "トランジスタ".to_string(),
                        _ => k.label(),
                    };
                    let id = ui.make_persistent_id(("part_tree", k.label()));
                    let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                    let mut toggled = false;
                    state.show_header(ui, |ui| {
                        let (rect, pic) = ui.allocate_exact_size(egui::vec2(34.0, 22.0), egui::Sense::click());
                        super::symbols::paint_kind_preview(ui.painter(), rect, *k, ui.visuals().text_color());
                        toggled = (pic | ui.add(egui::Label::new(title).sense(egui::Sense::click()))).clicked();
                    }).body(|ui| {
                        click = kind_row(ui, selected, *k, &format!("{} (基本)", k.label())).on_hover_text(kind_tip(*k)).clicked();
                        for (family, parts) in cap_kinds.iter() {
                            egui::CollapsingHeader::new(*family)
                                .id_salt(("part_family", k.label(), *family))
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.small(family_tip(family));
                                    for &i in parts {
                                        let sel = self.tool == Tool::Place && self.place_lib == Some(i);
                                        if ui.selectable_label(sel, &self.library[i].name).on_hover_text(lib_hover(&self.library[i])).clicked() {
                                            chosen = Some(i);
                                        }
                                    }
                                });
                        }
                    });
                    if toggled {
                        let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                        st.toggle(ui);
                        st.store(ui.ctx());
                    }
                } else {
                    click = kind_row(ui, selected, *k, &k.label()).on_hover_text(kind_tip(*k)).clicked();
                }
                if click {
                    self.tool = Tool::Place;
                    self.place_lib = None;
                    self.place = *k;
                    self.pending = None;
                }
            }
            });
        });
        // the two lists take turns: opening one closes the other, so the open list has the height of the panel
        if basic_clicked {
            self.basic_open = !self.basic_open;
            if self.basic_open {
                self.lib_open = false;
            }
        }
        let library_title = crate::widgets::count_title(ui, "ライブラリ部品", self.library.len());
        let (_, library_clicked) = crate::widgets::band_header(ui, "library", library_title, self.lib_open, true, |ui| {
            // everything inside the library group sits in one sunken well joined to the band; the indent line is not needed there
            crate::widgets::well(ui, |ui| {
            ui.visuals_mut().indent_has_left_vline = false;
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索 (名前・説明・ピン名・パッケージ)").desired_width(f32::INFINITY).margin(egui::Margin::symmetric(6, 7)))
                .on_hover_text("部品名のほか、説明、ピンの名前、パッケージ名 (例: SOT-23、TO-92) でも探せます");
            let cat_of = |d: &PartDef| if d.category.is_empty() { "その他".to_string() } else { d.category.clone() };
            let mut cats: Vec<String> = self.library.iter().map(cat_of).collect();
            cats.sort();
            cats.dedup();
            ui.horizontal(|ui| {
                // the same width and height as the search field above: it takes what the "◎のみ" box leaves
                ui.spacing_mut().button_padding.y = 5.0;
                let combo_width = ui.available_width() - 76.0;
                egui::ComboBox::from_id_salt("lib_cat")
                    .width(combo_width)
                    .selected_text(if self.lib_cat.is_empty() { "すべてのカテゴリ".to_string() } else { self.lib_cat.clone() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.lib_cat, String::new(), "すべてのカテゴリ");
                        for c in &cats {
                            ui.selectable_value(&mut self.lib_cat, c.clone(), c);
                        }
                    }).response.on_hover_text("ライブラリ部品の分類。選んだ分類だけを一覧に出します");
                ui.checkbox(&mut self.lib_sim_only, "◎のみ").on_hover_text("シミュレーションモデルのある部品だけを表示します");
            });
            let f = self.lib_filter.to_lowercase();
            let filtering = !f.is_empty() || !self.lib_cat.is_empty() || self.lib_sim_only;
            // which parts pass the search / category / simulation filters
            let shown: Vec<usize> = (0..self.library.len())
                .filter(|&i| {
                    let d = &self.library[i];
                    if (!self.lib_cat.is_empty() && self.lib_cat != cat_of(d)) || (self.lib_sim_only && d.sim.is_none()) {
                        return false;
                    }
                    if f.is_empty() {
                        return true;
                    }
                    let pins = d.pins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" ");
                    [d.name.as_str(), d.description.as_str(), cat_of(d).as_str(), pins.as_str(), d.footprint_name().as_str()].iter().any(|t| t.to_lowercase().contains(&f))
                })
                .collect();
            // the list fills what is left of the window (the part card and "部品を作る" stay below it)
            let list_height = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 215.0).clamp(160.0, 900.0);
            let by_name = |n: &String| self.library.iter().position(|d| &d.name == n);
            let favs: Vec<usize> = self.lib_prefs.fav.iter().filter_map(by_name).collect();
            let recents: Vec<usize> = self.lib_prefs.recent.iter().filter_map(by_name).filter(|i| !favs.contains(i)).collect();
            let mut toggle_fav: Option<String> = None;
            let mut edit_part: Option<PartDef> = None;
            let mut delete_part: Option<String> = None;
            let mut row = |ui: &mut egui::Ui, i: usize, selected: bool, prefs: &library::LibPrefs| {
                let d = &self.library[i];
                ui.horizontal(|ui| {
                    let fav = prefs.is_fav(&d.name);
                    if ui.add(egui::Button::new(egui::RichText::new(if fav { "★" } else { "☆" }).color(if fav { Color32::from_rgb(240, 190, 40) } else { Color32::GRAY })).frame(false).small())
                        .on_hover_text(if fav { "お気に入りから外します" } else { "お気に入りに入れます。一覧の先頭に出ます" })
                        .clicked()
                    {
                        toggle_fav = Some(d.name.clone());
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let (rect, pic) = ui.allocate_exact_size(egui::vec2(34.0, 24.0), egui::Sense::click());
                    let color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                    super::symbols::paint_lines_preview(ui.painter(), rect, &super::symbol_lines(&d.symbol()), color);
                    if (pic | ui.selectable_label(selected, format!("{}{mark}", d.name))).on_hover_text(lib_hover(d)).clicked() {
                        chosen = Some(i);
                    }
                });
            };
            egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, true]).show(ui, |ui| {
                let sel = |i: usize| self.tool == Tool::Place && self.place_lib == Some(i);
                if !filtering {
                    for (title, ids, tip) in [("★ お気に入り", &favs, "☆ を押した部品"), ("最近使った部品", &recents, "最近置いた部品 (新しい順)")] {
                        if !ids.is_empty() {
                            ui.label(egui::RichText::new(title).small().strong()).on_hover_text(tip);
                            for &i in ids {
                                row(ui, i, sel(i), &self.lib_prefs);
                            }
                            ui.add_space(4.0);
                        }
                    }
                }
                // grouped by category (folded unless searching), then by name
                let mut order = shown.clone();
                order.sort_by_key(|&i| (cat_of(&self.library[i]), self.library[i].name.to_lowercase()));
                let mut groups: Vec<(String, Vec<usize>)> = vec![];
                for i in order {
                    let c = cat_of(&self.library[i]);
                    match groups.last_mut() {
                        Some((g, v)) if *g == c => v.push(i),
                        _ => groups.push((c, vec![i])),
                    }
                }
                if groups.is_empty() {
                    ui.small("条件に合う部品がありません。検索の文字やカテゴリを変えてみてください");
                }
                for (c, ids) in groups {
                    let holds_selected = ids.iter().any(|&i| sel(i));
                    let mut h = egui::CollapsingHeader::new(crate::widgets::count_title(ui, &c, ids.len())).id_salt(("libcat", &c));
                    if filtering || holds_selected {
                        h = h.open(Some(true));
                    }
                    h.show(ui, |ui| {
                        for &i in &ids {
                            row(ui, i, sel(i), &self.lib_prefs);
                        }
                    });
                }
            });
            // a line under the scrolling part: a row cut by the edge reads as scrolled, not as overlapping the notes below
            ui.add_space(2.0);
            ui.separator();
            if let Some(n) = toggle_fav {
                self.lib_prefs.toggle_fav(&n);
                self.lib_prefs.save(&self.lib_dir);
            }
            if let Some(d) = self.place_lib.and_then(|k| self.library.get(k)) {
                ui.group(|ui| {
                    ui.strong(&d.name);
                    ui.small(format!("パッケージ: {}", d.footprint_name()));
                    if ui.small_button("編集…").on_hover_text("この部品を部品作成の窓で開いて、ピン・部品の形・モデル・定格などを直します。保存すると書き換わります (名前を変えると別の部品として追加)").clicked() {
                        edit_part = Some(d.clone());
                    }
                    if self.lib_delete.as_deref() == Some(d.name.as_str()) {
                        ui.horizontal(|ui| {
                            ui.colored_label(crate::theme::ERROR, "本当に削除しますか?");
                            if ui.small_button("削除する").on_hover_text("この部品のファイルをライブラリのフォルダから消します。元に戻せません。回路図や基板に置いた部品は、そのまま残ります").clicked() {
                                delete_part = Some(d.name.clone());
                            }
                            if ui.small_button("やめる").clicked() {
                                self.lib_delete = None;
                            }
                        });
                    } else if ui.small_button("削除…").on_hover_text("この部品を、ライブラリから消します (押したあとに、もう一度確かめます)。同梱の部品も消せて、消したものは、次に起動しても戻りません。回路図や基板に置いた部品は、そのまま残ります").clicked() {
                        self.lib_delete = Some(d.name.clone());
                    }
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
            if let Some(n) = delete_part {
                self.lib_delete = None;
                self.delete_library_part(&n);
            }
            if let Some(d) = edit_part {
                self.status = format!("{} を編集します。保存すると書き換わります", d.name);
                self.form = Some(PartForm::from_def(&d));
            }
            ui.small("◎ = シミュレーションモデルあり");
            ui.horizontal(|ui| {
                if self.lit(ui.button("再読み込み"), "再読み込み").on_hover_text("ライブラリのフォルダを読み直します。ファイルを直接直したときに押します").clicked() {
                    self.reload_library();
                }
            });
            ui.small("保存先: ライブラリのフォルダ").on_hover_text(self.lib_dir.display().to_string());
            for e in &self.lib_errors {
                ui.colored_label(crate::theme::ERROR, e);
            }
            });
        });
        if library_clicked {
            self.lib_open = !self.lib_open;
            if self.lib_open {
                self.basic_open = false;
            }
        }
        // below the (closed by default) library list, outside it, so a newcomer can find them without opening anything
        // make or reuse a part: one bar (the parts library's own buttons, then the snippet pair that works on the selection)
        let has = self.has_selection();
        let bar = crate::widgets::segment_row(ui, &[("部品を作る…", true), ("選択を保存…", has), ("スニペット…", true)]);
        let make = self.lit(bar[0].clone(), "＋ 部品を作る…").on_hover_text("ライブラリにない部品を、データシートを見ながら自分で作って追加します (記号・部品の形・シミュレーションモデル・定格)。JSON や KiCad のファイルからの追加も、開いた窓の「① 基本」からできます");
        if make.clicked() {
            self.form = Some(PartForm::default());
        }
        if bar[1].clone().on_hover_text("選んだ部品・線・ラベルをファイル (スニペット) に保存します。電源回路やLED点灯回路など、よく使う部分を別の回路でも使えます").clicked() {
            self.save_snippet_dialog();
        }
        if bar[2].clone().on_hover_text("保存したスニペットを選んで、いまの回路の空いた所に置きます。部品番号は新しくなります。置いたあと、ドラッグで動かせます").clicked() {
            self.load_snippet_dialog();
        }
        if let Some(i) = chosen {
            self.choose_library_part(i);
        }
    }

    pub(super) fn panel_properties(&mut self, ui: &mut egui::Ui) {
        self.properties_inner(ui, true);
    }

    /// The settings of the selected part; `heading` adds the "選択中の部品" line (the panel band has its own title).
    fn properties_inner(&mut self, ui: &mut egui::Ui, heading: bool) {
        self.label_net_ui(ui);
        self.port_editor_ui(ui);
        let mut open_sheet: Option<usize> = None;
        if let Some(Sel::Comp(i)) = self.selected
            && i < self.sch.components.len() {
                let fresh = self.dc.as_ref().is_some_and(|d| d.signature == self.signature());
                let sub_rows: Vec<(String, String)> = self.sch.components[i].sub.as_ref().map(|s| self.sch.value_rows(s.sheet)).unwrap_or_default();
                let c = &mut self.sch.components[i];
                if heading {
                    ui.label(egui::RichText::new("選択中の部品").strong());
                }
                // the two things everyone edits first, aligned in one table
                let (has_refdes, has_value) = (!c.kind.is_power_symbol(), c.kind != Kind::Ground);
                if has_refdes || has_value {
                    egui::Grid::new(("part_main", i)).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
                        if has_refdes {
                            ui.label("番号");
                            ui.add(egui::TextEdit::singleline(&mut c.refdes).desired_width(f32::INFINITY)).on_hover_text("部品番号 (R1、C2、U1 など)。同じ番号が2つあると、どちらかが解析やネットで区別できません");
                            ui.end_row();
                        }
                        if has_value {
                            ui.label("値");
                            ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(f32::INFINITY)).on_hover_text(value_hint(c.kind));
                            ui.end_row();
                        }
                    });
                }
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor)
                    && ui.small_button("値の決め方…").on_hover_text("この部品の値を計算で決める窓を開きます (LEDの抵抗・分圧・フィルタなど)").clicked()
                {
                    self.wiz_open = true;
                }
                if let Kind::Header(n) = c.kind {
                    egui::CollapsingHeader::new("ピンの名前 (信号名)").id_salt(("pin_names", i)).default_open(true).show(ui, |ui| {
                        ui.small("ピンごとの名前を、記号のそばと基板のシルクに書きます (配線やネットは変わりません)").on_hover_text("コネクタや端子台の各ピンが何の信号かを書いておくと、配線の間違いが減ります。空にすると、そのピンには名前を書きません");
                        c.pin_names.resize(n as usize, String::new());
                        for k in 0..n as usize {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}", k + 1));
                                ui.add(egui::TextEdit::singleline(&mut c.pin_names[k]).desired_width(110.0)).on_hover_text("このピンの名前 (F+、SA、GND など)");
                            });
                        }
                        if c.pin_names.iter().all(|s| s.is_empty()) {
                            c.pin_names.clear();
                        }
                    });
                }
                // where to buy this very part: reaches the shopping list and the BOM of the board
                if !c.kind.is_power_symbol() && c.sub.is_none() && !c.sim_only && c.kind != Kind::Ground {
                    egui::CollapsingHeader::new("買える店の型番").id_salt(("shops", i)).default_open(!c.shops.is_empty()).show(ui, |ui| {
                        ui.small("店のページで確かめた型番だけ入れます。購入リストと部品表に出ます (空欄なら、検索リンクになります)").on_hover_text("抵抗やコンデンサのように、ライブラリ部品ではない部品に、買う型番を付けられます。ライブラリ部品が型番を持っているときは、ここに書いた型番が優先されます。間違った型番より、空欄のほうがよいです");
                        for (id, label, hint) in [("akizuki", "秋月", "秋月電子通商の商品コード (例: 100941)"), ("digikey", "DigiKey", "DigiKey で検索できる型番 (例: RC0805FR-071KL)"), ("lcsc", "LCSC", "LCSC の部品番号 (C + 数字。例: C25804)")] {
                            let mut v = c.shops.get(id).cloned().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(label);
                                ui.add(egui::TextEdit::singleline(&mut v).desired_width(f32::INFINITY)).on_hover_text(hint);
                            });
                            if v.is_empty() {
                                c.shops.remove(id);
                            } else {
                                c.shops.insert(id.to_string(), v);
                            }
                        }
                    });
                }
                // the pins this part does not use: marked, ERC stops asking about them
                if !c.kind.is_power_symbol() && c.sub.is_none() && (c.custom.is_some() || c.kind.pins().len() >= 3) {
                    let pins: Vec<(String, String)> = match &c.custom {
                        Some(s) => s.pins.iter().map(|p| (p.number.clone(), p.name.clone())).collect(),
                        None => c.kind.pins().into_iter().map(|(n, _)| (n.clone(), n)).collect(),
                    };
                    let marked = self.sch.no_connects.iter().filter(|(r, _)| *r == c.refdes).count();
                    egui::CollapsingHeader::new(format!("使わないピン (NC) ({marked})")).id_salt(("nc_pins", i)).default_open(marked > 0).show(ui, |ui| {
                        ui.small("つながないピンに印を付けます。回路図に × が出て、ERC は「未接続」と言わなくなります").on_hover_text("データシートが「開放のまま」と言うピンや、使わない出力などに付けます。つなぎ忘れとの区別が付くので、本当のミスだけが ERC に残ります。あとから線やラベルでつなぐと、ERC が印との食い違いを知らせます");
                        for (num, name) in &pins {
                            let mut on = self.sch.no_connects.iter().any(|(r, p)| *r == c.refdes && p == num);
                            let text = if name.is_empty() || name == num { num.clone() } else { format!("{num}  {name}") };
                            if ui.checkbox(&mut on, text).on_hover_text("チェックを付けると、このピンは使わない (つながない) ピンになります").changed() {
                                if on {
                                    self.sch.no_connects.push((c.refdes.clone(), num.clone()));
                                } else {
                                    self.sch.no_connects.retain(|(r, p)| !(*r == c.refdes && p == num));
                                }
                            }
                        }
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
                        // one decade each way (1/10 .. 10x): wide enough to explore, fine enough at full panel width
                        let span = 1.0;
                        let mut x = (cur / base).log10().clamp(-span, span);
                        // the slider takes the full width; the values at its two ends sit under it, so the range is readable
                        ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
                        let changed = ui
                            .add(egui::Slider::new(&mut x, -span..=span).show_value(false))
                            .on_hover_text(
                                "ドラッグして値を動かします (中央 = 今の値)。動かせる範囲は今の値の 1/10 〜 10 倍で、両端の値を下に出しています。これより大きく変えたいときは、値の欄に直接打ちます。
下の「自動で再計算」をオンにすると、動かしながら波形や電圧が変わるのを見られます",
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.small(format!("{}{}", format_value(base * 10f64.powf(-span)), rest));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.small(format!("{}{}", format_value(base * 10f64.powf(span)), rest));
                            });
                        });
                        if changed {
                            let v = format_value(base * 10f64.powf(x));
                            c.value = format!("{v}{rest}");
                            self.slider = Some((i, base, c.value.clone()));
                        }
                        ui.checkbox(&mut self.live, "自動で再計算").on_hover_text("オンにすると、値を動かすたびに、開いている解析 (DC / 過渡 / AC) をやり直します。重い回路ではオフにしてください");
                        if self.live {
                            ui.checkbox(&mut self.live_ghost, "変える前の線を残す").on_hover_text("オンにすると、値を動かし始める前の波形・特性を、うすい線で残します。動かした後のこい線と重ねて、どう変わったかを比べられます。波形の「いまの波形を残す」で残した線は、これで置き換わります");
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
                                    ui.colored_label(crate::theme::ERROR, msg);
                                }
                            }
                        } else if let Err(msg) = form.to_text() {
                            ui.colored_label(crate::theme::ERROR, msg);
                        }
                    }
                }
                // the board and model side of the part: rarely changed, so folded
                if !c.kind.is_power_symbol() && c.sub.is_none() {
                    egui::CollapsingHeader::new("基板とモデル").id_salt(("part_board", i)).default_open(false).show(ui, |ui| {
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
                                    ui.colored_label(crate::theme::WARN, "シミュレーションモデルなし");
                                }
                            }
                        } else {
                            let current = c.footprint_name();
                            egui::ComboBox::from_label("フットプリント").selected_text(current).show_ui(ui, |ui| {
                                for n in pcb_core::footprints::names() {
                                    ui.selectable_value(&mut c.footprint, n.clone(), n);
                                }
                            }).response.on_hover_text("基板に載せる部品の形 (パッドの並び)。実物の部品の寸法に合わせて選びます");
                        }
                        ui.checkbox(&mut c.sim_only, "シミュレーション専用 (基板に載せない)")
                            .on_hover_text("試験用の電源や、測る相手の代わりに置いた抵抗など、実物の基板には要らない部品に付けます。付けると、基板の取り込み・部品表・購入リストに出ません (シミュレーションには使われます)");
                    });
                }
                if let Some(sub) = c.sub.as_mut() {
                    if ui.button("シートの中を開く").on_hover_text("この記号が表すシートを開きます (記号のダブルクリックでも開きます)。直すと、同じシートを使う全部の記号に効きます").clicked() {
                        open_sheet = Some(sub.sheet);
                    }
                    if !sub_rows.is_empty() {
                        let title = format!("このコピーだけの値 ({})", sub.values.len());
                        egui::CollapsingHeader::new(title).id_salt(("sub_values", i)).default_open(!sub.values.is_empty()).show(ui, |ui| {
                            ui.small("書いた部品だけ別の値 (空欄=シートのまま)").on_hover_text("同じシートを何回も使うとき、チャンネルごとに抵抗値などを変えたい部品だけ、ここに書きます。シートの中の部品は、そのままです。コピーの部品は R1_CH1 のように別の番号になり、シミュレーションにも基板にも、ここの値が使われます");
                            egui::Grid::new(("sub_values_grid", i)).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                                for (refdes, base) in &sub_rows {
                                    let mut text = sub.values.get(refdes).cloned().unwrap_or_default();
                                    ui.label(refdes);
                                    ui.small(base).on_hover_text("シートの値");
                                    if ui.add(egui::TextEdit::singleline(&mut text).hint_text(base.as_str()).desired_width(f32::INFINITY)).on_hover_text("このコピーでの値。空欄にすると、シートの値に戻ります").changed() {
                                        if text.trim().is_empty() {
                                            sub.values.remove(refdes);
                                        } else {
                                            sub.values.insert(refdes.clone(), text);
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                } else {
                    advice_ui(ui, &*c, self.dc.as_ref(), fresh, &self.library);
                }
            }
        if let Some(k) = open_sheet {
            self.enter_sheet(k);
        }
        // one row of actions on the selection: rotate, the clipboard, delete
        if self.has_selection() || !self.clipboard.is_empty() {
            let has = self.has_selection();
            ui.add_space(crate::theme::SPACE_S);
            ui.spacing_mut().item_spacing.y = crate::theme::SPACE_S;
            // two joined bars, like the buttons under the part lists: change the part, then the clipboard
            let rotatable = matches!(self.selected, Some(Sel::Comp(_)));
            let edit = crate::widgets::segment_row(ui, &[("回転 (R)", rotatable), ("複製", has), ("削除", has)]);
            if let Some(Sel::Comp(i)) = self.selected
                && self.lit(edit[0].clone(), "回転 (R)").on_hover_text("部品を 90° 回します (R キーでも同じです)").clicked()
            {
                self.rotate_req = Some(i);
            }
            if edit[1].clone().on_hover_text("コピーしてすぐ貼り付けます (Ctrl+D)").clicked() {
                self.duplicate_selection();
            }
            if self.lit(edit[2].clone(), "削除 (Delete)").on_hover_text("選んだものを消します (Delete キーでも同じです)").clicked() {
                self.delete_selected();
            }
            let has_clip = !self.clipboard.is_empty();
            let clip = crate::widgets::segment_row(ui, &[("コピー", has), ("切り取り", has), ("貼り付け", has_clip)]);
            if clip[0].clone().on_hover_text("選んだ部品・線・ラベルをコピーします (Ctrl+C)。部品のピンについているネットラベルも一緒にコピーされます").clicked() {
                self.copy_selection();
            }
            if clip[1].clone().on_hover_text("コピーして、元のものを消します (Ctrl+X)").clicked() {
                self.cut_selection();
            }
            if clip[2].clone().on_hover_text("コピーしたものを、近くの空いた所に置きます (Ctrl+V)。部品番号は新しくなります。貼ったあと、ドラッグで動かせます").clicked() {
                self.paste_clipboard();
            }
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
        crate::widgets::group_gap(ui);
        ui.spacing_mut().item_spacing.y = crate::theme::SPACE_S;
        if self.lit(crate::widgets::primary_button_wide(ui, true, "ERC (電気ルールチェック)"), "ERC (電気ルールチェック)").on_hover_text("配線ミスの点検。ピンや線の端がつながっていない、電源が短絡している、部品番号がダブっている、などを探して丸で示します (赤紫 = エラー、橙 = 注意)。結果の行をクリックすると、その場所へ移動して直し方を表示します。シミュレーションや基板に進む前にやっておくと安心です").clicked() {
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
        if bar[0].clone().on_hover_text("「設計の要件」の表を開きます。ネットの役割・近くに置く部品の組・離しておくものを書く所です (「編集」メニューの「設計の要件 (表)」と同じ)").clicked() {
            self.open_requirements();
        }
        if bar[1].clone().on_hover_text("選んだ部品・配線を囲む範囲を「描きかけ」にして、その中のERCの指摘を出さなくします。作りながら出るエラーの山を減らすための機能です。範囲は灰色の枠で表示され、完成したら下の×で外します。保存もされます").clicked() {
            self.skip_selection();
        }
        self.erc_skip_ui(ui);
        if self.erc_ran && self.problems.is_empty() {
            ui.colored_label(crate::theme::OK, "✔ 問題は見つかりませんでした");
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
            let label = egui::RichText::new(format!("{mark} {sheet}{}", p.message)).color(color);
            let tip = if p.at.is_some() { "クリックで、その場所へ移動して説明を表示します" } else { "クリックで説明を表示します" };
            if ui.link(label).on_hover_text(tip).clicked() {
                self.erc_focus = Some(k);
                jump = p.at;
                jump_sheet = p.sheet;
            }
            if self.erc_focus == Some(k) {
                ui.indent(("erc_hint", k), |ui| {
                    ui.label(&p.hint);
                    if let Some((refdes, pin)) = &p.pin
                        && ui.small_button("使わないピン (NC) にする").on_hover_text("このピンは、つながないのが正しいときに押します (使わない出力、データシートが開放と言うピンなど)。印の × が付いて、ERC はこのピンを聞かなくなります。つなぎ忘れなら、押さずに配線しましょう").clicked()
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
            egui::Window::new(if form.editing().is_some() { "部品を編集" } else { "部品を作成" }).open(&mut open).default_size([980.0, 660.0]).resizable(true).show(ui.ctx(), |ui| {
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
        if ui.button("操作ガイド (F1)").on_hover_text("マウスとキーの操作の一覧を開きます").clicked() {
            self.help_request = Some(crate::help::HelpTab::Schematic);
        }
    }
}

/// Tooltip of a library part: its description, then what its model leaves out.
fn lib_hover(d: &part_lib::PartDef) -> String {
    let mut t = d.description.clone();
    if let Some(r) = d.ratings.as_ref().filter(|r| !r.is_empty()) {
        t.push_str("

");
        t.push_str(&r.summary());
        t.push_str("
(データシートの代表的な値。使う前に、必ずデータシートで確かめてください)");
    }
    if d.land_unverified {
        t.push_str("\n\nランドパターン: データシートの図面と照らし合わせていない概算です。基板にする前に、パッドの位置・大きさ・ピン番号を確かめてください (発注の準備のチェックも注意します)。");
    }
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
100u IC=3.2 … 過渡解析を始めるときの電圧 (電池の代わりに使えます)
22u BIAS=60 … 直流の電圧がかかると、セラミックコンデンサの容量は下がります。その下がる割合 (%) です (例: 16V 品の 22u は、5V で約 60% 下がります)",
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

/// Common wattages of resistors, smallest first.
const RESISTOR_WATTS: &[f64] = &[0.0625, 0.1, 0.125, 0.25, 0.5, 1.0, 2.0, 5.0];

fn watt_label(w: f64) -> String {
    match w {
        w if (w - 0.0625).abs() < 1e-9 => "1/16W".into(),
        w if (w - 0.125).abs() < 1e-9 => "1/8W".into(),
        w if (w - 0.25).abs() < 1e-9 => "1/4W".into(),
        w if (w - 0.5).abs() < 1e-9 => "1/2W".into(),
        w => format!("{}W", sch_core_trim(w)),
    }
}

fn sch_core_trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// "部品選びのヒント" for the selected part: its data sheet figures, what this circuit asks of it (from the
/// DC result), the rating it would need with a margin of 2, whether it has that, and library parts that do.
fn advice_ui(ui: &mut egui::Ui, c: &Component, dc: Option<&DcView>, fresh: bool, library: &[PartDef]) {
    let lib_def = c.custom.as_ref().and_then(|s| library.iter().find(|d| d.name == s.name));
    let ratings = c.custom.as_ref().and_then(|s| s.ratings.as_ref()).or_else(|| lib_def.and_then(|d| d.ratings.as_ref()));
    if c.kind != Kind::Resistor && c.custom.is_none() {
        return;
    }
    egui::CollapsingHeader::new("部品選びのヒント (この回路で必要な定格)").id_salt(("advice", &c.refdes)).default_open(false).show(ui, |ui| {
        if let Some(r) = ratings.filter(|r| !r.is_empty()) {
            crate::widgets::section_heading(ui, "データシートの情報");
            for l in r.summary().lines() {
                ui.label(format!("・{l}"));
            }
        } else if c.custom.is_some() {
            ui.label("この部品には、定格 (データシートの情報) が入っていません。「＋ 部品を作る」の部品フォームの「データシートの情報」で入れられます");
        }
        let Some(view) = dc else {
            ui.label("DC動作点を解析すると、この回路でこの部品にかかる電流・電力と、必要な定格が出ます");
            return;
        };
        let Ok(res) = &view.outcome else {
            ui.label("DC動作点が解けていないので、必要な定格を出せません");
            return;
        };
        if !fresh {
            ui.colored_label(crate::theme::WARN, "回路を変えたので、いまは前の解析結果です。DC動作点をもう一度解析してください");
        }
        let st = sim_core::stress(res, &c.refdes);
        let (Some(i), Some(p)) = (st.i, st.p) else {
            ui.label("この部品は、解析結果に個別の値がありません (IC の内部モデルなど)");
            return;
        };
        ui.separator();
        crate::widgets::section_heading(ui, "この回路でかかる値");
        ui.label(format!("・電流 {}  電力 {}", eng(i, "A"), eng(p, "W")));
        if matches!(c.custom.as_ref().and_then(|s| s.sim.as_ref()).map(|m| m.element.to_ascii_uppercase()).as_deref(), Some("Q" | "M" | "J")) {
            ui.label(format!("・部品の両端の電圧 (Vce / Vds) {}", eng(st.v.unwrap_or(0.0), "V")));
        }
        ui.label(format!("・回路の最大電圧 {}", eng(st.v_circuit, "V")));
        let req = sim_core::requirement(&st, 2.0);
        crate::widgets::section_heading(ui, "余裕を 2 倍とるなら").on_hover_text("部品は、定格ぎりぎりで使うと、熱や劣化で壊れやすくなります。電圧・電流・電力は、最低でも 2 倍の余裕を見るのが、初学者の目安です");
        if c.kind == Kind::Resistor {
            let need = req.p_min;
            let pick = RESISTOR_WATTS.iter().copied().find(|w| *w >= need);
            ui.label(format!("・抵抗の定格電力 {} 以上 → {}", eng(need, "W"), pick.map_or("5W を超えます。電流を減らすか、抵抗を直列・並列に分けてください".to_string(), |w| format!("{} の抵抗が使えます", watt_label(w)))));
            return;
        }
        ui.label(format!("・最大電圧 {} 以上 / 最大電流 {} 以上 / 最大電力 {} 以上", eng(req.v_min, "V"), eng(req.i_min, "A"), eng(req.p_min, "W")));
        match ratings.filter(|r| !r.is_empty()) {
            Some(r) => {
                let ok = sim_core::meets(&req, r, true);
                let (text, color) = if ok { ("✔ この部品は、2 倍の余裕を満たしています", crate::theme::OK) } else { ("✖ この部品は、2 倍の余裕を満たしていません (いずれかの最大定格が足りない、または不明です)", crate::theme::ERROR) };
                ui.colored_label(color, text);
            }
            None => {
                ui.label("この部品の定格が分からないので、判定できません。データシートで、上の条件を満たすか確かめてください");
            }
        }
        // library parts of the same kind that would do
        if let Some(def) = lib_def {
            let cat = if def.category.is_empty() { None } else { Some(def.category.as_str()) };
            let mut ok: Vec<&PartDef> = library.iter().filter(|d| d.name != def.name && cat.is_some_and(|c| d.category == c) && d.ratings.as_ref().is_some_and(|r| sim_core::meets(&req, r, true))).collect();
            ok.sort_by(|a, b| a.name.cmp(&b.name));
            if !ok.is_empty() {
                crate::widgets::section_heading(ui, "同じ種類で、条件を満たすライブラリ部品");
                ui.horizontal_wrapped(|ui| {
                    for d in ok.iter().take(8) {
                        ui.label(&d.name).on_hover_text(d.ratings.as_ref().map(|r| r.summary()).unwrap_or_default());
                    }
                });
            }
        }
    });
}

/// One row of the basic parts list: a small picture of the symbol, then the name.
fn kind_row(ui: &mut egui::Ui, selected: bool, kind: Kind, text: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().icon_width + ui.spacing().item_spacing.x);
        let (rect, pic) = ui.allocate_exact_size(egui::vec2(34.0, 22.0), egui::Sense::click());
        let color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
        super::symbols::paint_kind_preview(ui.painter(), rect, kind, color);
        pic | ui.selectable_label(selected, text)
    })
    .inner
}
