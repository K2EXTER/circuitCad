//! The part lists of the left panel: basic parts, the library tree and the part-making buttons.

use super::*;

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
    /// The part lists and the part-making buttons (below the tools and the selected part's settings).
    pub(crate) fn panel_parts(&mut self, ui: &mut egui::Ui) {
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
        let basic_title = if self.basic_open { egui::RichText::new(i18n::tr("基本の部品")).strong() } else { egui::RichText::new(i18n::tr("基本の部品")) };
        let (_, basic_clicked) = crate::widgets::band_header(ui, "basic", basic_title, self.basic_open, true, |ui| {
            crate::widgets::well(ui, |ui| {
            ui.small(i18n::tr("クリックして配置 / Rキーで回転"));
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
                        toggled = (pic | ui.add(egui::Label::new(i18n::tr_dyn(&title)).sense(egui::Sense::click()))).clicked();
                    }).body(|ui| {
                        click = kind_row(ui, selected, *k, &i18n::trf!("{} (基本)", k.label())).on_hover_text(i18n::tr_dyn(kind_tip(*k))).clicked();
                        for (family, parts) in cap_kinds.iter() {
                            egui::CollapsingHeader::new(i18n::tr_dyn(family))
                                .id_salt(("part_family", k.label(), *family))
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.small(i18n::tr_dyn(family_tip(family)));
                                    for &i in parts {
                                        let sel = self.tool == Tool::Place && self.place_lib == Some(i);
                                        if ui.selectable_label(sel, &self.library[i].name).on_hover_text(i18n::tr_dyn(&lib_hover(&self.library[i]))).clicked() {
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
                    click = kind_row(ui, selected, *k, &k.label()).on_hover_text(i18n::tr_dyn(kind_tip(*k))).clicked();
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
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text(i18n::tr("検索 (名前・説明・ピン名・パッケージ)")).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(6, 7)))
                .on_hover_text(i18n::tr("部品名のほか、説明、ピンの名前、パッケージ名 (例: SOT-23、TO-92) でも探せます"));
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
                        ui.selectable_value(&mut self.lib_cat, String::new(), i18n::tr("すべてのカテゴリ"));
                        for c in &cats {
                            ui.selectable_value(&mut self.lib_cat, c.clone(), c);
                        }
                    }).response.on_hover_text(i18n::tr("ライブラリ部品の分類。選んだ分類だけを一覧に出します"));
                ui.checkbox(&mut self.lib_sim_only, i18n::tr("◎のみ")).on_hover_text(i18n::tr("シミュレーションモデルのある部品だけを表示します"));
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
                        .on_hover_text(if fav { i18n::tr("お気に入りから外します") } else { i18n::tr("お気に入りに入れます。一覧の先頭に出ます") })
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
                    ui.small(i18n::tr("条件に合う部品がありません。検索の文字やカテゴリを変えてみてください"));
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
                    ui.small(i18n::trf!("パッケージ: {}", d.footprint_name()));
                    if ui.small_button(i18n::tr("編集…")).on_hover_text(i18n::tr("この部品を部品作成の窓で開いて、ピン・部品の形・モデル・定格などを直します。保存すると書き換わります (名前を変えると別の部品として追加)")).clicked() {
                        edit_part = Some(d.clone());
                    }
                    if self.lib_delete.as_deref() == Some(d.name.as_str()) {
                        ui.horizontal(|ui| {
                            ui.colored_label(crate::theme::ERROR, i18n::tr("本当に削除しますか?"));
                            if ui.small_button(i18n::tr("削除する")).on_hover_text(i18n::tr("この部品のファイルをライブラリのフォルダから消します。元に戻せません。回路図や基板に置いた部品は、そのまま残ります")).clicked() {
                                delete_part = Some(d.name.clone());
                            }
                            if ui.small_button(i18n::tr("やめる")).clicked() {
                                self.lib_delete = None;
                            }
                        });
                    } else if ui.small_button(i18n::tr("削除…")).on_hover_text(i18n::tr("この部品を、ライブラリから消します (押したあとに、もう一度確かめます)。同梱の部品も消せて、消したものは、次に起動しても戻りません。回路図や基板に置いた部品は、そのまま残ります")).clicked() {
                        self.lib_delete = Some(d.name.clone());
                    }
                    ui.small(&d.description);
                    if let Some(m) = &d.sim {
                        limits_ui(ui, &m.limits, "lib_limits");
                    }
                    egui::CollapsingHeader::new(i18n::tr("ピン表")).id_salt("lib_pins").show(ui, |ui| {
                        egui::Grid::new("lib_pin_grid").num_columns(2).show(ui, |ui| {
                            for p in &d.pins {
                                ui.small(&p.number);
                                ui.small(&p.name);
                                ui.end_row();
                            }
                        });
                    });
                    ui.small(if d.sim.is_some() { i18n::tr("シミュレーションモデルあり ◎") } else { i18n::tr("シミュレーションモデルなし (基板設計用)") });
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
            ui.small(i18n::tr("◎ = シミュレーションモデルあり"));
            ui.horizontal(|ui| {
                if self.lit(ui.button(i18n::tr("再読み込み")), "再読み込み").on_hover_text(i18n::tr("ライブラリのフォルダを読み直します。ファイルを直接直したときに押します")).clicked() {
                    self.reload_library();
                }
            });
            ui.small(i18n::tr("保存先: ライブラリのフォルダ")).on_hover_text(self.lib_dir.display().to_string());
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
        let make = self.lit(bar[0].clone(), "＋ 部品を作る…").on_hover_text(i18n::tr("ライブラリにない部品を、データシートを見ながら自分で作って追加します (記号・部品の形・シミュレーションモデル・定格)。JSON や KiCad のファイルからの追加も、開いた窓の「① 基本」からできます"));
        if make.clicked() {
            self.form = Some(PartForm::default());
        }
        if bar[1].clone().on_hover_text(i18n::tr("選んだ部品・線・ラベルをファイル (スニペット) に保存します。電源回路やLED点灯回路など、よく使う部分を別の回路でも使えます")).clicked() {
            self.save_snippet_dialog();
        }
        if bar[2].clone().on_hover_text(i18n::tr("保存したスニペットを選んで、いまの回路の空いた所に置きます。部品番号は新しくなります。置いたあと、ドラッグで動かせます")).clicked() {
            self.load_snippet_dialog();
        }
        if let Some(i) = chosen {
            self.choose_library_part(i);
        }
    }
}

/// Tooltip of a library part: its description, then what its model leaves out.
pub(super) fn lib_hover(d: &part_lib::PartDef) -> String {
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

/// One row of the basic parts list: a small picture of the symbol, then the name.
fn kind_row(ui: &mut egui::Ui, selected: bool, kind: Kind, text: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().icon_width + ui.spacing().item_spacing.x);
        let (rect, pic) = ui.allocate_exact_size(egui::vec2(34.0, 22.0), egui::Sense::click());
        let color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
        super::symbols::paint_kind_preview(ui.painter(), rect, kind, color);
        pic | ui.selectable_label(selected, i18n::tr_dyn(text))
    })
    .inner
}
