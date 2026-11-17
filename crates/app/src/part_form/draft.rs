//! "データシートから下書き": one button that reads the whole data sheet and fills what it can find into the form at once: the part
//! number, the kind of part, the package (a built-in footprint, or a dual row with lands from the dimension table), the pins (the
//! pin table found under its heading), the body's size and height, and for a MOSFET, a transistor, a diode, an op amp, an
//! optocoupler, a photodiode or a Darlington the values of its electrical characteristics table (into that kind's box). Everything is listed with the page it came from and a box to leave it out, and goes in only
//! when the person presses the button: these are guesses from a document's layout, to be checked.

use super::*;
use datasheet::draft::{DimTable, FoundPins, PackageName};
use datasheet::absmax::Rating;
use datasheet::land::LandPattern;

/// What was found, and which of it goes in.
pub struct FormDraft {
    pub name: Option<String>,
    pub kind: Option<(PartType, &'static str)>,
    /// The package names, the likeliest first, and the one chosen.
    pub packages: Vec<PackageName>,
    pub package: usize,
    /// The pins from the pin table and from the pin drawing (whichever were found), and the one used.
    pub pin_sources: Vec<FoundPins>,
    pub pin_pick: usize,
    /// The table and the drawing were both found and gave the same pins.
    pub pins_agree: bool,
    pub dims: Option<DimTable>,
    /// The recommended land patterns of the data sheet (one per package it draws).
    pub lands: Vec<LandPattern>,
    /// The rows of the absolute maximum ratings table.
    pub ratings: Vec<Rating>,
    /// A P type (PNP, P-channel) by what the sheet says, for the symbol.
    pub p_type: Option<bool>,
    /// Use: name, kind, package, pins, body, model, ratings.
    pub use_: [bool; 7],
    pub open: bool,
}

/// The form's kind of part for a category the data sheet suggests.
fn kind_of(category: &str) -> Option<PartType> {
    Some(match category {
        "MOSFET" | "トランジスタ" | "ダーリントン" => PartType::Transistor,
        "ダイオード" | "ツェナーダイオード" | "フォトダイオード" => PartType::Diode,
        "コンパレータ" | "オペアンプ" | "フォトカプラ" | "レギュレータ" | "アナログスイッチ" | "ロジックIC" | "マイコン" => PartType::Ic,
        _ => return None,
    })
}

/// The part list's category for a kind the data sheet suggests (the bundled PC817 and BPW34 are sensors).
fn list_category(kind: &'static str) -> &'static str {
    match kind {
        "フォトカプラ" | "フォトダイオード" => "センサ",
        "ダーリントン" => "トランジスタ",
        k => k,
    }
}

/// Whether the form has a box that the electrical characteristics table can fill, for this kind.
fn has_model(kind: Option<(PartType, &'static str)>) -> bool {
    kind.is_some_and(|(t, cat)| matches!(t, PartType::Transistor | PartType::Diode) || matches!(cat, "オペアンプ" | "フォトカプラ" | "レギュレータ"))
}

/// A built-in footprint of the same name as a package ("SOT-23", "TO-92").
fn builtin_footprint(p: &PackageName) -> Option<String> {
    let names = pcb_core::footprints::names();
    let base = p.family.to_uppercase();
    names.into_iter().find(|n| n.eq_ignore_ascii_case(&p.name) || n.eq_ignore_ascii_case(&base))
}

/// Package families whose leads stand in two rows (the form's "デュアル").
fn is_dual(family: &str) -> bool {
    matches!(family, "soic" | "so" | "sop" | "tssop" | "msop" | "vssop" | "ssop" | "dip" | "pdip")
}

impl FormDraft {
    /// How many pins the part has: what the pins found say, else the package's name.
    fn pin_count(&self) -> Option<u32> {
        self.pin_sources.get(self.pin_pick).map(|p| p.pins.len() as u32).or(self.packages.get(self.package).and_then(|p| p.pins))
    }

    /// The most voltage, current and power and the highest temperature from the absolute maximum ratings, by the symbols of the kind
    /// of part (a MOSFET's VDS and ID, a transistor's VCEO and IC, a diode's VRRM and IF(AV), an IC's supply voltage).
    pub(super) fn max_ratings(&self) -> [Option<&Rating>; 4] {
        use datasheet::absmax::pick;
        let cat = self.kind.map(|k| k.1).unwrap_or("");
        let r = &self.ratings;
        let (v, i): (&[&str], &[&str]) = match cat {
            "MOSFET" => (&["VDS", "VDSS"], &["ID"]),
            "トランジスタ" | "ダーリントン" => (&["VCEO", "VCE"], &["IC"]),
            // an optocoupler as the bundled PC817: the output's voltage, the LED's current, the collector's power
            "フォトカプラ" => (&["VCEO", "VCE"], &["IF"]),
            "フォトダイオード" => (&["VR"], &[]),
            "ダイオード" => (&["VRRM", "VRWM", "VR"], &["IF(AV)", "IFAV", "IO", "IF"]),
            "ツェナーダイオード" => (&[], &["IZM", "IZ"]),
            _ => (&["VCC", "VDD", "VIN", "VI", "VS", "V+"], &["IO", "IOUT"]),
        };
        let p: &[&str] = if cat == "フォトカプラ" { &["PC", "Ptot", "PD"] } else { &["PD", "Ptot", "PV"] };
        [pick(r, v), pick(r, i), pick(r, p), pick(r, &["TJ", "Tj"])]
    }

    /// The recommended land pattern of the chosen package (its family and pin count), when the data sheet draws it in two rows.
    pub(super) fn land(&self) -> Option<&LandPattern> {
        let p = self.packages.get(self.package)?;
        let n = self.pin_count();
        self.lands.iter().find(|l| l.dual() && l.family == Some(p.family) && n.is_none_or(|n| n == l.pins))
    }
}

/// The body's width across the rows, its length along them, its height, the lead span tip to tip, a foot's length and a lead's width,
/// from a dimension table (the letters differ between makers: the span is the largest of E / H / HE, the body the smallest of E / E1).
pub(super) struct Sizes {
    pub body_w: Option<f64>,
    pub body_l: Option<f64>,
    pub height: Option<f64>,
    pub pitch: Option<f64>,
    pub span: Option<(f64, f64)>,
    pub foot: Option<(f64, f64)>,
    pub lead_w: Option<(f64, f64)>,
}

pub(super) fn sizes(t: &DimTable) -> Sizes {
    let typ = |s: &str| t.get(s).and_then(|d| d.typical());
    let across: Vec<(&str, f64)> = ["E", "E1", "H", "HE"].iter().filter_map(|s| typ(s).map(|v| (*s, v))).collect();
    let span_sym = across.iter().filter(|(s, _)| *s != "E1").max_by(|a, b| a.1.total_cmp(&b.1)).map(|a| a.0);
    let body_w = across.iter().filter(|(s, _)| matches!(*s, "E" | "E1")).map(|a| a.1).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v))));
    let span = span_sym.and_then(|s| t.get(s)?.range()).filter(|r| body_w.is_none_or(|b| r.1 > b + 0.2));
    Sizes {
        body_w,
        body_l: typ("D"),
        height: t.get("A").and_then(|d| d.max.or(d.typical())),
        pitch: typ("e"),
        span,
        foot: t.get("L").and_then(|d| d.range()),
        lead_w: t.get("b").and_then(|d| d.range()),
    }
}

impl PartForm {
    /// Reads the open data sheet for a draft (`None` without one).
    pub(super) fn make_draft(&self) -> Option<FormDraft> {
        let ds = self.sheet.datasheet()?;
        // a scanned PDF: the candidates from the words the OCR reads now
        let fresh = if ds.ocr_on() { self.sheet.fresh_candidates() } else { None };
        let c = fresh.as_ref().or(self.sheet.candidates());
        let name = c.and_then(|c| c.names.first().cloned());
        let kind = c.and_then(|c| c.category).and_then(|cat| kind_of(cat).map(|k| (k, cat)));
        // the pins: from the table and from the drawing. When they differ the drawing goes first: it is of one package, where a
        // table's rows of several numbers ("2, 3, 6, 7") and several packages' columns are read wrong more easily. A drawing with
        // fewer pins than the table is not all of it (LP5907's drawing gave 2 of its 5 pins): the table then goes first
        let mut pin_sources: Vec<FoundPins> = ds.find_pin_table().into_iter().collect();
        let mut pins_agree = false;
        if let Some(d) = ds.find_pin_drawing() {
            match pin_sources.first() {
                Some(t) if t.pins == d.pins => pins_agree = true,
                Some(t) if d.pins.len() < t.pins.len() => pin_sources.push(d),
                Some(_) => pin_sources.insert(0, d),
                None => pin_sources.push(d),
            }
        }
        let mut packages = ds.package_names();
        packages.truncate(6);
        // with the pins counted, a name that says that many pins (or says none) first
        let package = pin_sources.first().and_then(|p| packages.iter().position(|k| k.pins == Some(p.pins.len() as u32))).unwrap_or(0);
        let dims = ds.dimension_table();
        let lands = ds.land_patterns();
        // a table with a column per part number ("TIP120, TIP121, TIP122") gives the named part's column
        let ratings = ds.absolute_maximum_of(name.as_deref());
        let model = has_model(kind);
        let p_type = ds.p_type();
        let mut d = FormDraft { use_: [name.is_some(), kind.is_some(), !packages.is_empty(), !pin_sources.is_empty(), dims.is_some(), model, false], name, kind, packages, package, pin_sources, pin_pick: 0, pins_agree, dims, lands, ratings, p_type, open: true };
        d.use_[6] = d.max_ratings().iter().any(Option::is_some);
        Some(d)
    }

    /// Puts the chosen parts of the draft into the form; returns what was done, line by line.
    pub(super) fn apply_draft(&mut self, d: &FormDraft) -> Vec<String> {
        let mut done = vec![];
        // the kind first: choosing it puts its example pins and package in, which the rest then replaces
        if d.use_[1]
            && let Some((k, cat)) = d.kind
        {
            self.apply_type(k);
            self.category = list_category(cat).to_string();
            // the symbol's polarity by what the sheet says (2N3906: "PNP Silicon")
            let p = d.p_type == Some(true);
            match cat {
                "MOSFET" => self.glyph = if p { "pmos" } else { "nmos" }.into(),
                "トランジスタ" | "ダーリントン" if p => self.glyph = "pnp".into(),
                "フォトダイオード" => self.glyph = "photodiode".into(),
                _ => {}
            }
            done.push(format!("種類: {cat}{}", if p && matches!(cat, "MOSFET" | "トランジスタ" | "ダーリントン") { " (P 形)" } else { "" }));
        }
        if d.use_[0]
            && let Some(n) = &d.name
        {
            self.name = n.clone();
            done.push(format!("部品名: {n}"));
        }
        if d.use_[3]
            && let Some(p) = d.pin_sources.get(d.pin_pick)
        {
            self.pins_text = super::pin_import::lines_with_kinds(&p.pins, &p.io);
            // a table can be read again to compare with later; a drawing cannot
            self.pin_ref = (!p.from_drawing).then_some(super::pdf_check::PinRef { page: p.page, area: p.area, num_col: p.num_col, name_col: p.name_col });
            self.pin_check = None;
            done.push(format!("ピン: p.{} の{}から {} 本", p.page + 1, if p.from_drawing { "ピン配置の図" } else { "表" }, p.pins.len()));
        }
        let s = d.dims.as_ref().map(sizes);
        if d.use_[2]
            && let Some(p) = d.packages.get(d.package)
        {
            if let Some(fp) = builtin_footprint(p) {
                self.fp_preset = fp.clone();
                done.push(format!("パッケージ: {} (組み込みの {fp})", p.name));
            } else if is_dual(p.family) {
                self.fp_preset.clear();
                self.pkg = PkgKind::Dual;
                self.tht = matches!(p.family, "dip" | "pdip");
                let mut how = format!("パッケージ: {} (デュアル)", p.name);
                if let Some(l) = d.land() {
                    // the maker's own land pattern goes before any calculation
                    if let Some(e) = l.pitch {
                        self.pitch = e;
                    }
                    if let Some(r) = l.row {
                        self.row = r;
                    }
                    (self.len, self.width) = (l.pad_l, l.pad_w);
                    how += &format!("、パッドは推奨ランドパターン (p.{}) の {:.2} × {:.2} mm・列の間 {:.2} mm・ピッチ {} mm", l.page + 1, l.pad_l, l.pad_w, self.row, l.pitch.map_or("—".into(), |e| format!("{e:.2}")));
                } else if let Some(s) = &s {
                    if let Some(e) = s.pitch {
                        self.pitch = e;
                        how += &format!("、ピッチ {e:.2} mm");
                    }
                    if !self.tht
                        && let (Some(span), Some(foot), Some(w)) = (s.span, s.foot, s.lead_w)
                        && let Some(g) = pcb_core::land::gull_wing(span, foot, w)
                    {
                        (self.row, self.len, self.width) = (g.row, g.pad_l, g.pad_w);
                        how += &format!("、パッドは寸法表から IPC の式で {:.2} × {:.2} mm・列の間 {:.2} mm (推奨ランドパターンがあれば見比べてください)", g.pad_l, g.pad_w, g.row);
                    }
                }
                done.push(how);
            } else {
                done.push(format!("パッケージ: {} は組み込みの形が無いので、「④ 部品の形」で寸法を入れてください", p.name));
            }
        }
        if d.use_[4]
            && let Some(s) = &s
        {
            if let (Some(w), Some(l)) = (s.body_w, s.body_l) {
                (self.body_on, self.body_w, self.body_h, self.body_x, self.body_y) = (true, w, l, 0.0, 0.0);
                self.body_fit = None;
                done.push(format!("本体: {w:.2} × {l:.2} mm"));
            }
            if let Some(h) = s.height {
                self.height = h;
                done.push(format!("高さ: {h:.2} mm (寸法表の A の最大)"));
            }
        }
        if d.use_[6] {
            let m = d.max_ratings();
            let mut put = vec![];
            for (k, (field, unit)) in [(&mut self.rt_v, "V"), (&mut self.rt_i, "A"), (&mut self.rt_p, "W")].into_iter().enumerate() {
                if let Some(r) = m[k] {
                    *field = passive::eng(r.value);
                    put.push(format!("{} {}{unit}", r.symbol, passive::eng(r.value)));
                }
            }
            if let Some(r) = m[3] {
                self.rt_t = format!("{}", r.value);
                put.push(format!("{} {} ℃", r.symbol, r.value));
            }
            if let Some(p) = m.iter().flatten().next() {
                done.push(format!("定格: p.{} の絶対最大定格から {}", p.page + 1, put.join("・")));
            }
            // the thermal resistances with the ratings (θJA, θjc, the derating's start)
            self.fill_heat_from_pdf();
            let heat: Vec<String> = [("θja", &self.rt_ja), ("θjc", &self.rt_jc)].iter().filter(|(_, v)| !v.is_empty()).map(|(n, v)| format!("{n} {v} K/W")).collect();
            if !heat.is_empty() {
                done.push(format!("熱: {}", heat.join("・")));
            }
        }
        if d.use_[5] {
            let sheet = &self.sheet;
            let values = |s: &str| sheet.spec_values(s);
            // the kinds with a box of their own; the box opens to show what went in
            let boxed = match d.kind.map(|k| k.1) {
                Some("オペアンプ") => {
                    self.opamp.fill_from_pdf(values);
                    Some(("opamp_box", "オペアンプ"))
                }
                Some("レギュレータ") => {
                    let name = self.name.clone();
                    self.reg.fill_from_pdf(&name, values);
                    Some(("reg_box", "レギュレータ"))
                }
                Some("フォトカプラ") => {
                    self.opto.fill_from_pdf(values);
                    Some(("opto_box", "フォトカプラ"))
                }
                Some("フォトダイオード") => {
                    self.pd.fill_from_pdf(values);
                    Some(("", "フォトダイオード"))
                }
                Some("ダーリントン") => {
                    self.darl.fill_from_pdf(values);
                    self.darl.put_resistors(self.sheet.darlington_resistors());
                    Some(("darl_box", "ダーリントン"))
                }
                _ => None,
            };
            if let Some((id, what)) = boxed {
                self.model_box = (!id.is_empty()).then_some(id);
                done.push(format!("{what}の値: 電気的特性の表から (「かんたん作成」の欄に。曲線は図から足して、「② シミュレーション」で確かめてください)"));
            }
            let sheet = &self.sheet;
            match d.kind.map(|k| k.0).filter(|_| boxed.is_none()) {
                Some(PartType::Transistor) if self.glyph == "nmos" || self.glyph == "pmos" => {
                    self.mos.fill_from_pdf(|s| sheet.spec_values(s));
                    done.push("MOSFET の値: 電気的特性の表から (「② シミュレーション」で確かめてモデルを作ってください)".into());
                }
                Some(PartType::Transistor) => {
                    let name = self.name.clone();
                    self.bjt.fill_from_pdf(&name, |s| sheet.spec_values(s));
                    done.push("トランジスタの値: 電気的特性の表から (「② シミュレーション」で確かめてモデルを作ってください)".into());
                }
                Some(PartType::Diode) => {
                    self.dio_fill_from_pdf();
                    done.push("ダイオードの値: 電気的特性の表から (「② シミュレーション」で確かめてください)".into());
                }
                _ => {}
            }
        }
        done
    }

    /// For the command line: the draft made and all of it put in (`--form-draft`, for screenshots and checks).
    pub fn draft_all(&mut self) {
        if let Some(d) = self.make_draft() {
            let done = self.apply_draft(&d);
            // what went in, for scripts (`--form-draft`)
            for line in &done {
                eprintln!("draft: {line}");
            }
            self.pick_msg = Some((true, done.join(" / ")));
            self.draft = Some(FormDraft { open: true, ..d });
        }
    }

    /// The button beside the data sheet's.
    pub(super) fn draft_button(&mut self, ui: &mut egui::Ui) {
        if ui.add_enabled(self.sheet.has_doc(), egui::Button::new(i18n::tr("データシートから下書き"))).on_hover_text(i18n::tr("開いたデータシートを丸ごと読んで、部品名・種類・パッケージ・ピン・本体の大きさ・(トランジスタ・ダイオードなら) 特性の値を探し、見つかったものを一覧にします。確かめてから、まとめてフォームに入れます")).on_disabled_hover_text(i18n::tr("先に「データシート (PDF) を開く…」でデータシートを開いてください")).clicked() {
            self.draft = self.make_draft();
        }
    }

    /// The window that lists the draft.
    pub(super) fn draft_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.draft.take() else { return };
        if !d.open {
            return;
        }
        let mut apply = false;
        let mut go: Option<usize> = None;
        let mut open = true;
        egui::Window::new(i18n::tr("データシートから下書き")).open(&mut open).default_width(560.0).show(ctx, |ui| {
            ui.small(i18n::tr("データシートの文字の並びから見つけたものです。違うものは外し、ページを開いて確かめてください"));
            egui::Grid::new("draft_grid").num_columns(4).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
                let page_button = |ui: &mut egui::Ui, page: Option<usize>, go: &mut Option<usize>| {
                    match page {
                        Some(p) => {
                            if ui.small_button(format!("p.{}", p + 1)).on_hover_text(i18n::tr("このページをデータシートの窓で開きます")).clicked() {
                                *go = Some(p);
                            }
                        }
                        None => {
                            ui.label("");
                        }
                    }
                };
                let none = || egui::RichText::new(i18n::tr("見つかりません")).weak();
                // name
                ui.add_enabled(d.name.is_some(), egui::Checkbox::new(&mut d.use_[0], i18n::tr("部品名")));
                match &mut d.name {
                    Some(n) => {
                        ui.text_edit_singleline(n);
                    }
                    None => {
                        ui.label(none());
                    }
                }
                ui.label("");
                ui.end_row();
                // kind
                ui.add_enabled(d.kind.is_some(), egui::Checkbox::new(&mut d.use_[1], i18n::tr("種類")));
                match d.kind {
                    Some((_, cat)) => ui.label(cat),
                    None => ui.label(none()),
                };
                ui.label("");
                ui.end_row();
                // package
                ui.add_enabled(!d.packages.is_empty(), egui::Checkbox::new(&mut d.use_[2], i18n::tr("パッケージ")));
                if d.packages.is_empty() {
                    ui.label(none());
                    ui.label("");
                } else {
                    let shown = d.packages[d.package].name.clone();
                    egui::ComboBox::from_id_salt("draft_pkg").selected_text(&shown).show_ui(ui, |ui| {
                        for (k, p) in d.packages.iter().enumerate() {
                            ui.selectable_value(&mut d.package, k, i18n::trf!("{} ({} 回)", p.name, p.count));
                        }
                    }).response.on_hover_text(i18n::tr("データシートに書かれている回数の多い順です。いくつかのパッケージがあるデータシートでは、使うものを選んでください"));
                    page_button(ui, Some(d.packages[d.package].page), &mut go);
                }
                ui.end_row();
                // its land pattern
                if let Some(l) = d.land() {
                    ui.label("");
                    ui.small(i18n::trf!("推奨ランドパターン: パッド {:.2} × {:.2} mm、列の間 {}、ピッチ {} mm", l.pad_l, l.pad_w, l.row.map_or("—".into(), |r| format!("{r:.2}")), l.pitch.map_or("—".into(), |e| format!("{e:.2}"))))
                        .on_hover_text(i18n::tr("データシートの推奨ランドパターンの図の「14X (1.55)」のような書き方から読みました (14 個あるのがパッド、ピン数 − 2 個あるのがピッチ、1 つだけの大きな数が列の中心の間)。パッケージを入れると、この寸法でパッドを作ります"));
                    page_button(ui, Some(l.page), &mut go);
                    ui.end_row();
                } else if d.packages.get(d.package).is_some_and(|p| is_dual(p.family)) {
                    ui.label("");
                    ui.small(egui::RichText::new(i18n::tr("推奨ランドパターンは読めませんでした (寸法表があれば、IPC の式でパッドを計算します)")).weak());
                    ui.label("");
                    ui.end_row();
                }
                // pins
                ui.add_enabled(!d.pin_sources.is_empty(), egui::Checkbox::new(&mut d.use_[3], i18n::tr("ピン")));
                if d.pin_sources.is_empty() {
                    ui.label(none()).on_hover_text(i18n::tr("ピンの表も、番号と名前の書かれたピン配置の図も見つかりませんでした。データシートの窓の「ピン表を囲む」で、表か図を囲んで読めます"));
                    ui.label("");
                } else {
                    ui.vertical(|ui| {
                        if d.pin_sources.len() > 1 {
                            ui.horizontal(|ui| {
                                for (k, p) in d.pin_sources.iter().enumerate() {
                                    ui.selectable_value(&mut d.pin_pick, k, if p.from_drawing { i18n::tr("図から") } else { i18n::tr("表から") });
                                }
                                ui.colored_label(crate::theme::WARN, i18n::tr("表と図で違います")).on_hover_text(i18n::tr("ピンの表とピン配置の図で、読めたピンが違いました。両方のページを開いて、正しい方を選んでください (表は、いくつものパッケージの列や「2, 3, 6, 7」のような行を読み違えることがあります)"));
                            });
                        }
                        let p = &d.pin_sources[d.pin_pick.min(d.pin_sources.len() - 1)];
                        let list: Vec<String> = p.pins.iter().take(10).map(|(n, m)| format!("{n} {m}")).collect();
                        let more = if p.pins.len() > 10 { i18n::trf!(" ほか {} 本", p.pins.len() - 10) } else { String::new() };
                        ui.label(format!("{}{more}", list.join(", "))).on_hover_text(if p.from_drawing { i18n::tr("ピン配置の図の、番号のすぐ横 (か上下) の名前を読みました。図のとおりか、ページで確かめてください") } else { i18n::tr("「ピン配置」の見出しの下の表から読みました。番号と名前の組が表のとおりか、ページで確かめてください") });
                        if d.pins_agree {
                            ui.small(egui::RichText::new(i18n::tr("✔ 表とピン配置の図が一致しています")).color(crate::theme::OK));
                        }
                        if d.pin_sources.len() == 1 && p.from_drawing {
                            ui.small(i18n::tr("ピン配置の図から読みました (表は見つかりませんでした)"));
                        }
                    });
                    let p = &d.pin_sources[d.pin_pick.min(d.pin_sources.len() - 1)];
                    page_button(ui, Some(p.page), &mut go);
                }
                ui.end_row();
                // body
                ui.add_enabled(d.dims.is_some(), egui::Checkbox::new(&mut d.use_[4], i18n::tr("本体の寸法")));
                match &d.dims {
                    Some(t) => {
                        let s = sizes(t);
                        let f = |v: Option<f64>| v.map_or("—".to_string(), |v| format!("{v:.2}"));
                        ui.label(i18n::trf!("幅 {} × 長さ {} × 高さ {} mm、ピッチ {}{}", f(s.body_w), f(s.body_l), f(s.height), f(s.pitch), if t.from_inch { " (インチの表から)" } else { "" }))
                            .on_hover_text(t.rows.iter().map(|(k, v)| format!("{k}: {}", v.typical().map_or("—".into(), |x| format!("{x:.3}")))).collect::<Vec<_>>().join("  "));
                        page_button(ui, Some(t.page), &mut go);
                    }
                    None => {
                        ui.label(none()).on_hover_text(i18n::tr("寸法の表 (DIM・MIN・NOM・MAX) が文字で見つかりませんでした。図に寸法が書かれているデータシートは、「④ 部品の形」で入れてください"));
                        ui.label("");
                    }
                }
                ui.end_row();
                // model
                let model_kind = has_model(d.kind);
                ui.add_enabled(model_kind, egui::Checkbox::new(&mut d.use_[5], i18n::tr("特性の値")));
                ui.label(if model_kind { egui::RichText::new(i18n::tr("電気的特性の表から読みます (入れたあと「② シミュレーション」で確かめます)")) } else { egui::RichText::new(i18n::tr("トランジスタ・MOSFET・ダイオード・オペアンプ・フォトカプラ・フォトダイオード・ダーリントンのとき")).weak() });
                ui.label("");
                ui.end_row();
                // ratings
                let m: Vec<Option<Rating>> = d.max_ratings().iter().map(|r| r.cloned()).collect();
                let any = m.iter().any(Option::is_some);
                ui.add_enabled(any, egui::Checkbox::new(&mut d.use_[6], i18n::tr("定格")));
                if any {
                    let units = ["V", "A", "W", "℃"];
                    let parts: Vec<String> = m.iter().zip(units).filter_map(|(r, u)| r.as_ref().map(|r| format!("{} {}{}", r.symbol, if u == "℃" { format!("{} ", r.value) } else { passive::eng(r.value) }, u))).collect();
                    ui.label(parts.join("、")).on_hover_text(i18n::tr("「絶対最大定格」の表から、部品の種類に合う記号 (MOSFET なら VDS・ID、トランジスタなら VCEO・IC、ダイオードなら VRRM・IF(AV)、IC なら電源電圧) の最大の値を読みました。「⑤ 定格」の最大電圧・電流・電力・最高温度に入ります"));
                    page_button(ui, m.iter().flatten().next().map(|r| r.page), &mut go);
                } else {
                    ui.label(none()).on_hover_text(i18n::tr("「絶対最大定格」の表が文字で見つからないか、部品の種類の記号がありませんでした"));
                    ui.label("");
                }
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                if crate::widgets::primary_button(ui, true, "チェックしたものを入れる").on_hover_text(i18n::tr("チェックした項目をフォームに入れます。種類を入れると、その種類の例でフォームがいったん初期化されます")).clicked() {
                    apply = true;
                }
                if ui.button(i18n::tr("閉じる")).clicked() {
                    d.open = false;
                }
            });
        });
        if let Some(p) = go {
            self.sheet.go_to_page(p);
        }
        if apply {
            let done = self.apply_draft(&d);
            self.pick_msg = Some((true, if done.is_empty() { "入れるものがありませんでした".into() } else { format!("データシートから入れました: {}", done.join(" / ")) }));
            d.open = false;
        }
        if open && d.open {
            self.draft = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datasheet::draft::Dim;

    /// (symbol, min, nom, max).
    type Row<'a> = (&'a str, Option<f64>, Option<f64>, Option<f64>);

    fn table(rows: &[Row]) -> DimTable {
        DimTable { page: 0, from_inch: false, rows: rows.iter().map(|(s, a, n, b)| (s.to_string(), Dim { min: *a, nom: *n, max: *b })).collect() }
    }

    /// The letters of the dimension table, as JEDEC writes them (E the span, E1 the body) and as onsemi does (E the body, H the span).
    #[test]
    fn sizes_from_both_ways_of_lettering() {
        let jedec = table(&[("A", Some(1.35), None, Some(1.75)), ("b", Some(0.31), None, Some(0.51)), ("D", Some(4.8), Some(4.9), Some(5.0)), ("E", Some(5.8), Some(6.0), Some(6.2)), ("E1", Some(3.8), Some(3.9), Some(4.0)), ("e", None, Some(1.27), None), ("L", Some(0.4), None, Some(1.27))]);
        let s = sizes(&jedec);
        assert_eq!((s.body_w, s.body_l, s.height, s.pitch), (Some(3.9), Some(4.9), Some(1.75), Some(1.27)));
        assert_eq!(s.span, Some((5.8, 6.2)));
        let onsemi = table(&[("A", None, None, Some(1.75)), ("D", None, Some(4.9), None), ("E", Some(3.8), None, Some(4.0)), ("H", Some(5.8), None, Some(6.2)), ("e", None, Some(1.27), None)]);
        let s = sizes(&onsemi);
        assert_eq!(s.body_w, Some(3.9));
        assert_eq!(s.span, Some((5.8, 6.2)));
    }

    #[test]
    fn categories_and_packages_map_to_the_form() {
        assert_eq!(kind_of("MOSFET"), Some(PartType::Transistor));
        assert_eq!(kind_of("オペアンプ"), Some(PartType::Ic));
        assert_eq!(kind_of("?"), None);
        // the light parts and the Darlington: their own boxes, the bundled parts' list categories
        assert_eq!((kind_of("フォトカプラ"), list_category("フォトカプラ")), (Some(PartType::Ic), "センサ"));
        assert_eq!((kind_of("フォトダイオード"), list_category("フォトダイオード")), (Some(PartType::Diode), "センサ"));
        assert_eq!((kind_of("ダーリントン"), list_category("ダーリントン")), (Some(PartType::Transistor), "トランジスタ"));
        assert!(has_model(Some((PartType::Ic, "オペアンプ"))) && !has_model(Some((PartType::Ic, "ロジックIC"))));
        let p = |name: &str, family: &'static str| PackageName { name: name.into(), family, pins: None, count: 1, page: 0 };
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23")).as_deref(), Some("SOT-23"));
        assert_eq!(builtin_footprint(&p("TO-92", "to-92")).as_deref(), Some("TO-92"));
        assert!(builtin_footprint(&p("SOIC-8", "soic")).is_none());
        assert!(is_dual("tssop") && !is_dual("sot-223"));
    }
}
