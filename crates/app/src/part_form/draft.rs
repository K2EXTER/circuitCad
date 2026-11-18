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
    /// The pins are the usual order of the package (a MOSFET or transistor whose sheet numbers no lead), not read from the
    /// sheet: the package's name, the parts that order was seen on, and the kind ("MOSFET", "トランジスタ"). Not ticked until the person checks the drawing.
    pub pins_usual: Option<(String, &'static str, &'static str)>,
    pub dims: Option<DimTable>,
    /// The recommended land patterns of the data sheet (one per package it draws).
    pub lands: Vec<LandPattern>,
    /// The rows of the absolute maximum ratings table, and of the recommended operating conditions (for a current the first lacks).
    pub ratings: Vec<Rating>,
    pub recommended: Vec<Rating>,
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
pub(super) fn builtin_footprint(p: &PackageName, pins: Option<&[String]>) -> Option<String> {
    let names = pcb_core::footprints::names();
    let base = p.family.to_uppercase();
    let count = pins.map(<[String]>::len).or(p.pins.map(|n| n as usize));
    // the name as written, its family, and either with the pin count ("SOT-23" of a 5-pin part is the SOT-23-5); a diode's
    // outline by its "D_" name ("SOD-123" is the D_SOD-123)
    let mut wanted = vec![p.name.clone(), base.clone(), format!("D_{base}")];
    if let Some(n) = count {
        wanted.extend([format!("{}-{n}", p.name), format!("{base}-{n}")]);
    }
    // its pads named as the part's pins are numbered (not the 3-pin SOT-23 for a 5-pin LDO, nor the lettered B/E/C one
    // for pins 1 to 3), else as many pads as the package's name says
    let fits = |n: &str| {
        let Some(f) = pcb_core::footprints::get(n) else { return false };
        let mut pads: Vec<&str> = f.pads.iter().map(|p| p.pin.as_str()).collect();
        pads.sort_unstable();
        pads.dedup();
        match (pins, count) {
            (Some(nums), _) => {
                let mut want: Vec<&str> = nums.iter().map(String::as_str).collect();
                want.sort_unstable();
                want.dedup();
                pads == want
            }
            (None, Some(k)) => pads.len() == k,
            (None, None) => true,
        }
    };
    if let Some(n) = wanted.iter().find_map(|w| names.iter().find(|n| n.eq_ignore_ascii_case(w) && fits(n)).cloned()) {
        return Some(n);
    }
    // the pin count unknown: the one outline of that name with a count ("TO-220" → "TO-220-3"; not the SOT-23-5 or -6)
    let up = format!("{}-", p.name.to_uppercase());
    let mut with_count = names.iter().filter(|n| count.is_none() && n.to_uppercase().starts_with(&up));
    match (with_count.next(), with_count.next()) {
        (Some(n), None) => Some(n.clone()),
        _ => None,
    }
}

/// Which of the pins found are of the package `k` (by the packages over their column: "SOT-89/TO-92"), when a table has a column per
/// package.
pub(super) fn pins_of_package(sources: &[FoundPins], k: &PackageName) -> Option<usize> {
    sources.iter().position(|s| {
        s.package.as_deref().is_some_and(|names| names.split('/').any(|n| datasheet::draft::package_of(n).is_some_and(|(f, _)| f == k.family)))
    })
}

/// A tab printed as a pin of its own with the name of pin 2 ("TAB OUTPUT" beside "2 OUTPUT", TI's LM317 in SOT-223):
/// its number. The built-in SOT-223 makes the tab pad 2.
fn tab_as_pin_2(pins: &[(String, String)]) -> Option<String> {
    let two = pins.iter().find(|p| p.0 == "2")?;
    pins.iter().find(|p| (p.0.eq_ignore_ascii_case("TAB") || p.0 == "4") && p.1.eq_ignore_ascii_case(&two.1)).map(|p| p.0.clone())
}

/// Package families whose leads stand in two rows (the form's "デュアル"): the small outlines, and the 6- and 8-lead SOT and SC
/// outlines (three or four leads a side).
fn is_dual(family: &str) -> bool {
    matches!(family, "soic" | "so" | "sop" | "tssop" | "msop" | "vssop" | "ssop" | "dip" | "pdip" | "sot-363" | "sot-563" | "sot-583" | "sc-88" | "sc-74")
}

/// The pin of an exposed pad in a pin table: a row named as one ("EP", "PAD", "THERMAL PAD", "EXPOSED PAD", "PowerPAD").
fn exposed_pad_pin(pins: &str) -> Option<String> {
    pins.lines().find_map(|l| {
        let mut t = l.split_whitespace();
        let (num, name) = (t.next()?, t.next()?.to_ascii_uppercase());
        matches!(name.as_str(), "EP" | "PAD" | "EPAD" | "THERMAL" | "THERMALPAD" | "EXPOSED" | "EXPOSEDPAD" | "POWERPAD" | "SLUG").then(|| num.to_string())
    })
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
            // an adjustable regulator floats: its limit is the input-output differential (LM317)
            _ => (&["VCC", "VDD", "VIN", "VI", "VS", "V+", "VI-VO"], &["IO", "IOUT"]),
        };
        let p: &[&str] = if cat == "フォトカプラ" { &["PC", "Ptot", "PD"] } else { &["PD", "Ptot", "PV"] };
        // a current the absolute maximum ratings do not give (they say "internally limited"): the recommended operating maximum
        [pick(r, v), pick(r, i).or_else(|| pick(&self.recommended, i)), pick(r, p), pick(r, &["TJ", "Tj"])]
    }

    /// The recommended land pattern of the chosen package (its family and pin count), when the data sheet draws it in two rows or
    /// on four sides. An exposed pad's pin may be among the pins counted (16 pins and the pad as pin 17).
    pub(super) fn land(&self) -> Option<&LandPattern> {
        let p = self.packages.get(self.package)?;
        let n = self.pin_count();
        let pins_fit = |l: &LandPattern| n.is_none_or(|n| n == l.pins || (l.ep.is_some() && n == l.pins + 1));
        self.lands.iter().find(|l| (l.dual() || l.four_sided()) && l.family == Some(p.family) && pins_fit(l))
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
    /// The body across the rows (E) as (min, max): a no-lead package's terminals end at its edge.
    pub e: Option<(f64, f64)>,
    /// A no-lead package's exposed pad: E2 across the rows, D2 along them.
    pub ep: Option<(f64, f64)>,
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
        e: t.get("E").and_then(|d| d.range()),
        ep: typ("E2").zip(typ("D2")),
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
        // (a table of a column per package gives the pins of each)
        let mut pin_sources: Vec<FoundPins> = ds.find_pin_tables();
        let mut pins_agree = false;
        if let Some(d) = ds.find_pin_drawing() {
            match pin_sources.first() {
                // (any package's column: the drawing is of one package)
                Some(_) if pin_sources.iter().any(|t| t.pins == d.pins) => pins_agree = true,
                Some(t) if d.pins.len() < t.pins.len() => pin_sources.push(d),
                Some(_) => pin_sources.insert(0, d),
                None => pin_sources.push(d),
            }
        }
        // no table and no drawing: a list of the pins in lines ("Pin 1. Input", MC7800; the case outline's style, onsemi)
        if pin_sources.is_empty()
            && let Some(l) = ds.find_pin_list()
        {
            pin_sources.push(l);
        }
        let mut packages = ds.package_names();
        packages.truncate(6);
        // a MOSFET or transistor whose sheet numbers no lead (G, D, S or E, B, C only in its drawings, 候補 Q13・Q14): a sentence
        // that numbers them, the letters of a front view's drawing, or else the package's usual order, offered but not ticked
        let mut pins_usual = None;
        let leaded = kind.map(|k| k.1).filter(|k| matches!(*k, "MOSFET" | "トランジスタ" | "ダーリントン"));
        if pin_sources.is_empty()
            && let Some(what) = leaded
        {
            let family = packages.first().map(|p| p.family);
            if let Some(p) = ds.lead_pins(family) {
                let how = match p.from {
                    datasheet::lead_pins::LeadPinsFrom::Text => "文",
                    _ => "図の文字の並び",
                };
                pin_sources.push(FoundPins { page: p.page, area: p.area, num_col: 0, name_col: 0, pins: p.pins, io: vec![], from_drawing: true, package: Some(how.into()) });
            } else if let Some((pins, seen)) = family.and_then(|f| datasheet::lead_pins::usual(f, what != "MOSFET")) {
                let page = packages.first().map_or(0, |p| p.page);
                pin_sources.push(FoundPins { page, area: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 }, num_col: 0, name_col: 0, pins, io: vec![], from_drawing: true, package: None });
                pins_usual = Some((packages[0].name.clone(), seen, what));
            }
        }
        // with the pins counted, a name that says that many pins (or says none) first
        // (but not one with no built-in outline over the most written one that has an outline for the pins: MC7800's TO-220, not its DPAK-3)
        let numbers: Option<Vec<String>> = pin_sources.first().map(|p| p.pins.iter().map(|q| q.0.clone()).collect());
        let fits = |k: &PackageName| builtin_footprint(k, numbers.as_deref()).is_some();
        let package = match pin_sources.first().and_then(|p| packages.iter().position(|k| k.pins == Some(p.pins.len() as u32))) {
            Some(i) if !fits(&packages[i]) && packages.first().is_some_and(fits) => 0,
            Some(i) => i,
            None => 0,
        };
        // the pins of that package, in a table of a column per package
        let pin_pick = packages.get(package).and_then(|k| pins_of_package(&pin_sources, k)).unwrap_or(0);
        let dims = ds.dimension_table();
        let lands = ds.land_patterns();
        // a table with a column per part number ("TIP120, TIP121, TIP122") gives the named part's column
        let ratings = ds.absolute_maximum_of(name.as_deref());
        let recommended = ds.recommended_operating_of(name.as_deref());
        let model = has_model(kind);
        let p_type = ds.p_type();
        let use_pins = !pin_sources.is_empty() && pins_usual.is_none();
        let mut d = FormDraft { use_: [name.is_some(), kind.is_some(), !packages.is_empty(), use_pins, dims.is_some(), model, false], name, kind, packages, package, pin_sources, pin_pick, pins_agree, pins_usual, dims, lands, ratings, recommended, p_type, open: true };
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
            let from = match (&p.package, p.from_drawing) {
                (Some(s), true) if s.starts_with("STYLE") => format!("外形図の {s} の一覧"),
                (Some(s), true) => s.clone(),
                (_, true) => "ピン配置の図か一覧".to_string(),
                (_, false) => "表".to_string(),
            };
            match &d.pins_usual {
                Some((pkg, _, what)) => done.push(format!("ピン: {pkg} の{what}のよくある並び ({})。データシートの図で確かめてください", super::pin_import::lines_with_kinds(&p.pins, &[]).replace('\n', "・"))),
                None => done.push(format!("ピン: p.{} の{from}から {} 本", p.page + 1, p.pins.len())),
            }
        }
        let s = d.dims.as_ref().map(sizes);
        if d.use_[2]
            && let Some(p) = d.packages.get(d.package)
        {
            // the pins put in (a tab with pin 2's name is pin 2's pad), or else the form's: their numbers
            let pins: Option<Vec<(String, String)>> = d.use_[3].then(|| d.pin_sources.get(d.pin_pick).map(|p| p.pins.clone())).flatten();
            let tab = pins.as_deref().and_then(tab_as_pin_2);
            let numbers: Vec<String> = match &pins {
                Some(v) => v.iter().map(|p| p.0.clone()).filter(|n| Some(n) != tab.as_ref()).collect(),
                None => self.pins_text.lines().filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect(),
            };
            // the draft's own pins must fit; the form's (a template's) may be the person's to change, so the name alone will do
            let fp = builtin_footprint(p, Some(&numbers)).or_else(|| if pins.is_none() { builtin_footprint(p, None) } else { None });
            if let Some(fp) = fp {
                self.fp_preset = fp.clone();
                let mut how = format!("パッケージ: {} (組み込みの {fp})", p.name);
                // the tab's own line goes when the outline has no pad of its number: its pad is pin 2's (the SOT-223's tab, a
                // TO-220's tab on the middle lead)
                if let Some(t) = &tab
                    && pcb_core::footprints::get(&fp).is_some_and(|f| !f.pads.iter().any(|p| p.pin == *t))
                {
                    self.pins_text = self.pins_text.lines().filter(|l| l.split_whitespace().next() != Some(t.as_str())).collect::<Vec<_>>().join("\n");
                    how += &format!("。{t} は 2 番と同じピン (タブ) なので、2 番のパッドにまとめました");
                }
                done.push(how);
            } else if let Some(sides) = datasheet::draft::no_lead_sides(p.family) {
                done.push(self.apply_no_lead(p, sides, d.land(), s.as_ref()));
            } else if let Some(l) = d.land().filter(|l| l.pins == 2)
                && let Some(how) = self.apply_two_pads(p, l)
            {
                done.push(how);
            } else if datasheet::draft::two_terminal(p.family)
                && let Some((span, foot, w)) = s.as_ref().and_then(|s| Some((s.span.or(s.e)?, s.foot?, s.lead_w?)))
                && let Some(g) = pcb_core::land::gull_wing(span, foot, w)
            {
                self.fp_preset.clear();
                (self.pkg, self.pitch, self.len, self.width) = (PkgKind::Chip2, g.row, g.pad_l, g.pad_w);
                done.push(format!("パッケージ: {} (2 端子)、パッドは寸法表から IPC の式で {:.2} × {:.2} mm・中心の間 {:.2} mm (推奨のパッドの図があれば見比べてください)", p.name, g.pad_l, g.pad_w, g.row));
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
                    let from = if d.recommended.iter().any(|x| std::ptr::eq(x, r)) { format!(" (p.{} の推奨動作条件の最大)", r.page + 1) } else { String::new() };
                    put.push(format!("{} {}{unit}{from}", r.symbol, passive::eng(r.value)));
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
                    // (the columns of the name's grade: MC7805C's, not MC7805B's beside them)
                    self.reg.fill_from_pdf(&name, |s| sheet.spec_values_for(s, &name));
                    self.reg_find_variants();
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

    /// A QFN (pads on four sides: the form's "クアッド") or a SON / DFN (two rows: "デュアル") with its exposed pad: the pads from
    /// the maker's land pattern when it draws one, else from the dimension table by IPC's sums for flat terminals (body E, terminal
    /// L and b, pitch e), the exposed pad from the drawing or from E2 × D2. The pad's pin is the pin table's EP / PAD row, else the
    /// drawing's number in it, else the next number, and a row for it is added. Returns the line for the list of what was done.
    /// The form's package from a maker's drawing of two pads (Nexperia's soldering footprint of a SOD323 or SOD882): the two-pad
    /// chip with the drawing's own sizes, before any calculation from the dimension table. `None` when the drawing does not say
    /// how far the pads' centres are apart.
    pub(super) fn apply_two_pads(&mut self, p: &PackageName, l: &LandPattern) -> Option<String> {
        let centres = l.row?;
        self.fp_preset.clear();
        (self.pkg, self.pitch, self.len, self.width) = (PkgKind::Chip2, centres, l.pad_l, l.pad_w);
        Some(format!("パッケージ: {} (2 端子)、パッドは推奨のパッドの図 (p.{}) の {:.2} × {:.2} mm・中心の間 {:.2} mm", p.name, l.page + 1, l.pad_l, l.pad_w, centres))
    }

    pub(super) fn apply_no_lead(&mut self, p: &PackageName, sides: u32, land: Option<&LandPattern>, s: Option<&Sizes>) -> String {
        self.fp_preset.clear();
        self.tht = false;
        self.pkg = if sides == 4 { PkgKind::Quad } else { PkgKind::Dual };
        let shape = if sides == 4 { "クアッド" } else { "デュアル" };
        let mut how = format!("パッケージ: {} ({shape}、足の無い形)", p.name);
        let mut ep = None;
        if let Some(l) = land {
            if let Some(e) = l.pitch {
                self.pitch = e;
            }
            if let Some(r) = l.row {
                if sides == 4 {
                    self.span = r;
                } else {
                    self.row = r;
                }
            }
            (self.len, self.width) = (l.pad_l, l.pad_w);
            ep = l.ep.map(|e| (e, l.ep_pin.clone()));
            how += &format!("、パッドは推奨ランドパターン (p.{}) の {:.2} × {:.2} mm・向かいのパッドの中心の間 {:.2} mm・ピッチ {} mm", l.page + 1, l.pad_l, l.pad_w, l.row.unwrap_or(0.0), l.pitch.map_or("—".into(), |e| format!("{e:.2}")));
        } else if let Some(s) = s {
            if let Some(e) = s.pitch {
                self.pitch = e;
            }
            if let (Some(span), Some(foot), Some(w)) = (s.e, s.foot, s.lead_w)
                && let Some(mut g) = pcb_core::land::no_lead(span, foot, w)
            {
                // the pads' inner ends kept 0.2 mm off the exposed pad (IPC's heel side can reach it)
                if let Some((across, _)) = s.ep {
                    let outer = g.row / 2.0 + g.pad_l / 2.0;
                    let least = across / 2.0 + 0.2;
                    if g.row / 2.0 - g.pad_l / 2.0 < least {
                        g.pad_l = (outer - least).max(0.2);
                        g.row = 2.0 * (outer - g.pad_l / 2.0);
                    }
                }
                if sides == 4 {
                    self.span = g.row;
                } else {
                    self.row = g.row;
                }
                (self.len, self.width) = (g.pad_l, g.pad_w);
                how += &format!("、パッドは寸法表から IPC の式で {:.2} × {:.2} mm・向かいのパッドの中心の間 {:.2} mm・ピッチ {} mm (推奨ランドパターンがあれば見比べてください)", g.pad_l, g.pad_w, g.row, s.pitch.map_or("—".into(), |e| format!("{e:.2}")));
            }
            ep = s.ep.map(|e| (e, None));
        }
        match ep {
            Some(((w, h), drawn)) => {
                let pin = exposed_pad_pin(&self.pins_text).or(drawn).unwrap_or_else(|| {
                    let top = self.pins_text.lines().filter_map(|l| l.split_whitespace().next()?.parse::<u32>().ok()).max().unwrap_or(0);
                    (top + 1).to_string()
                });
                if !self.pins_text.lines().any(|l| l.split_whitespace().next() == Some(pin.as_str())) {
                    if !self.pins_text.is_empty() && !self.pins_text.ends_with('\n') {
                        self.pins_text.push('\n');
                    }
                    self.pins_text += &format!("{pin} EP");
                }
                (self.thermal_on, self.thermal_pin, self.thermal_w, self.thermal_h) = (true, pin.clone(), w, h);
                how += &format!("。放熱パッド {w:.2} × {h:.2} mm (ピン {pin}。GND などにつなぐなら、ピン表の名前を直してください)");
            }
            None => {
                self.thermal_on = false;
                how += "。放熱パッドの寸法は見つかりませんでした (寸法表の D2・E2 か推奨ランドパターンを見て「④ 部品の形」で入れてください)";
            }
        }
        how
    }

    /// For the command line: the draft made and all of it put in (`--form-draft`, for screenshots and checks).
    pub fn draft_all(&mut self, package: Option<&str>) {
        if let Some(mut d) = self.make_draft() {
            if let Some(k) = package.and_then(|f| d.packages.iter().position(|p| p.family.eq_ignore_ascii_case(f))) {
                d.package = k;
                if let Some(j) = pins_of_package(&d.pin_sources, &d.packages[k]) {
                    d.pin_pick = j;
                }
            }
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
                    let before = d.package;
                    egui::ComboBox::from_id_salt("draft_pkg").selected_text(&shown).show_ui(ui, |ui| {
                        for (k, p) in d.packages.iter().enumerate() {
                            ui.selectable_value(&mut d.package, k, i18n::trf!("{} ({} 回)", p.name, p.count));
                        }
                    }).response.on_hover_text(i18n::tr("データシートに書かれている回数の多い順です。いくつかのパッケージがあるデータシートでは、使うものを選んでください"));
                    // another package: its own pins, when the table has a column per package
                    if d.package != before
                        && let Some(k) = pins_of_package(&d.pin_sources, &d.packages[d.package])
                    {
                        d.pin_pick = k;
                    }
                    page_button(ui, Some(d.packages[d.package].page), &mut go);
                }
                ui.end_row();
                // its land pattern
                if let Some(l) = d.land() {
                    ui.label("");
                    let ep = l.ep.map_or(String::new(), |(w, h)| i18n::trf!("、放熱パッド {:.2} × {:.2} mm", w, h));
                    if l.pins == 2 {
                        // two pads: no rows and no pitch, the pads' centres
                        ui.small(i18n::trf!("推奨のパッドの図: パッド {:.2} × {:.2} mm、中心の間 {} mm", l.pad_l, l.pad_w, l.row.map_or("—".into(), |r| format!("{r:.2}"))))
                            .on_hover_text(i18n::tr("データシートのはんだ付けの図 (Reflow soldering footprint) から読みました。「(2×)」の付いた寸法のうち小さい方から 2 番目をパッド (はんだペースト < パッド < レジスト の順)、枠で囲んだ数をパッドの中心の間としています。パッドの大きさは、2 つのパッドを結ぶ向き × それと直角の向きです。図を開いて見比べてください"));
                    } else {
                        ui.small(i18n::trf!("推奨ランドパターン: パッド {:.2} × {:.2} mm、列の間 {}、ピッチ {} mm{}", l.pad_l, l.pad_w, l.row.map_or("—".into(), |r| format!("{r:.2}")), l.pitch.map_or("—".into(), |e| format!("{e:.2}")), ep))
                            .on_hover_text(i18n::tr("データシートの推奨ランドパターンの図の「14X (1.55)」のような書き方から読みました (14 個あるのがパッド、ピン数 − 2 個あるのがピッチ、1 つだけの大きな数が列の中心の間)。パッケージを入れると、この寸法でパッドを作ります"));
                    }
                    page_button(ui, Some(l.page), &mut go);
                    ui.end_row();
                } else if d.packages.get(d.package).is_some_and(|p| is_dual(p.family) || datasheet::draft::no_lead_sides(p.family).is_some() || datasheet::draft::two_terminal(p.family)) {
                    ui.label("");
                    ui.small(egui::RichText::new(i18n::tr("推奨ランドパターンは読めませんでした (寸法表があれば、IPC の式でパッドを計算します。QFN・SON は D2・E2 が放熱パッド)")).weak());
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
                                    let label = match (&p.package, p.from_drawing) {
                                        (_, true) => i18n::tr("図から").to_string(),
                                        (Some(pkg), false) => pkg.clone(),
                                        (None, false) => i18n::tr("表から").to_string(),
                                    };
                                    ui.selectable_value(&mut d.pin_pick, k, label);
                                }
                                if d.pin_sources.iter().any(|p| p.package.is_some()) {
                                    ui.small(i18n::tr("パッケージごとの列")).on_hover_text(i18n::tr("ピンの表にパッケージごとの番号の列がありました。使うパッケージの列を選んでください (上のパッケージを選ぶと、その列に合わせます)"));
                                } else {
                                    ui.colored_label(crate::theme::WARN, i18n::tr("表と図で違います")).on_hover_text(i18n::tr("ピンの表とピン配置の図で、読めたピンが違いました。両方のページを開いて、正しい方を選んでください (表は、いくつものパッケージの列や「2, 3, 6, 7」のような行を読み違えることがあります)"));
                                }
                            });
                        }
                        let p = &d.pin_sources[d.pin_pick.min(d.pin_sources.len() - 1)];
                        let list: Vec<String> = p.pins.iter().take(10).map(|(n, m)| format!("{n} {m}")).collect();
                        let more = if p.pins.len() > 10 { i18n::trf!(" ほか {} 本", p.pins.len() - 10) } else { String::new() };
                        ui.label(format!("{}{more}", list.join(", "))).on_hover_text(if p.from_drawing { i18n::tr("ピン配置の図の、番号のすぐ横 (か上下) の名前を読みました。図のとおりか、ページで確かめてください") } else { i18n::tr("「ピン配置」の見出しの下の表から読みました。番号と名前の組が表のとおりか、ページで確かめてください") });
                        if d.pins_agree {
                            ui.small(egui::RichText::new(i18n::tr("✔ 表とピン配置の図が一致しています")).color(crate::theme::OK));
                        }
                        if let Some((pkg, seen, what)) = &d.pins_usual {
                            ui.colored_label(crate::theme::WARN, i18n::trf!("データシートにピンの番号がありません。{pkg} の{}のよくある並びです", i18n::tr(what)))
                                .on_hover_text(i18n::trf!("この PDF は端子 (G・D・S や E・B・C) を記号と外形の絵にしか書いていません。{pkg} ではこの並びが多い ({seen} など) ものの、メーカーによって違います。外形図の絵と見比べて、合っていれば「ピン」にチェックを入れてください。違えば「③ ピン」で直します"));
                        } else if d.pin_sources.len() == 1 && p.from_drawing {
                            match p.package.as_deref() {
                                // a case outline's style (onsemi: "STYLE 1" named on the first page)
                                Some(s) if s.starts_with("STYLE") => ui.small(i18n::trf!("外形図の {s} のピンの一覧から読みました (1 ページ目にその型と書かれています)")),
                                Some("文") => ui.small(i18n::tr("「Pin 1 = Gate」のような文から読みました")),
                                Some(_) => ui.small(i18n::tr("外形の絵の端子の文字 (G・D・S や E・B・C) の並び (正面から見て左から 1・2・3) から読みました。図で確かめてください")),
                                None => ui.small(i18n::tr("ピン配置の図か、ピンの一覧から読みました (表は見つかりませんでした)")),
                            };
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

    /// A QFN from its dimension table (no land drawing): クアッド, the pads by IPC's no-lead sums kept off the exposed pad, the
    /// exposed pad E2 × D2 as a new pin after the last; a SON from TI's land drawing: デュアル, the drawing's pad and its number.
    #[test]
    fn no_lead_packages_get_their_exposed_pad() {
        let qfn = table(&[("A", Some(0.8), Some(0.85), Some(0.9)), ("b", Some(0.18), Some(0.25), Some(0.30)), ("D", Some(2.9), Some(3.0), Some(3.1)), ("D2", Some(1.6), Some(1.7), Some(1.8)), ("E", Some(2.9), Some(3.0), Some(3.1)), ("E2", Some(1.6), Some(1.7), Some(1.8)), ("e", None, Some(0.5), None), ("L", Some(0.3), Some(0.4), Some(0.5))]);
        let s = sizes(&qfn);
        let mut f = PartForm { pins_text: (1..=16).map(|i| format!("{i} P{i}")).collect::<Vec<_>>().join("\n"), ..Default::default() };
        let p = PackageName { name: "QFN-16".into(), family: "qfn", pins: Some(16), count: 1, page: 0 };
        let how = f.apply_no_lead(&p, 4, None, Some(&s));
        assert!(f.pkg == PkgKind::Quad && f.fp_preset.is_empty(), "{how}");
        assert_eq!(f.pitch, 0.5);
        // the inner ends 0.2 mm off the 1.7 mm pad, the outer ends where IPC puts them
        assert!((f.span / 2.0 - f.len / 2.0 - (0.85 + 0.2)).abs() < 1e-9, "{how}");
        assert!((f.span / 2.0 + f.len / 2.0 - 3.93 / 2.0).abs() < 0.03, "{how}");
        assert!((f.thermal_on, f.thermal_pin.as_str(), f.thermal_w, f.thermal_h) == (true, "17", 1.7, 1.7), "{how}");
        assert!(f.pins_text.ends_with("16 P16\n17 EP"));
        let def = f.to_def().unwrap();
        let fp = def.package.build("QFN").unwrap();
        assert_eq!(fp.pads.len(), 17);

        // TI's WSON-8 land, a pin table that names the pad already
        let land = LandPattern { page: 48, family: Some("wson"), pins: 8, pad_l: 0.5, pad_w: 0.25, pitch: Some(0.5), row: Some(1.9), quad: false, ep: Some((0.9, 1.6)), ep_pin: Some("9".into()) };
        let mut f = PartForm { pins_text: "1 OUT1\n2 IN1-\n3 IN1+\n4 GND\n5 IN2+\n6 IN2-\n7 OUT2\n8 VCC\n9 PAD".into(), ..Default::default() };
        let p = PackageName { name: "WSON".into(), family: "wson", pins: None, count: 1, page: 0 };
        let how = f.apply_no_lead(&p, 2, Some(&land), None);
        assert!(f.pkg == PkgKind::Dual && (f.pitch, f.row, f.len, f.width) == (0.5, 1.9, 0.5, 0.25), "{how}");
        assert!((f.thermal_pin.as_str(), f.thermal_w, f.thermal_h) == ("9", 0.9, 1.6) && f.pins_text.lines().count() == 9, "{how}");
        assert!(how.contains("推奨ランドパターン (p.49)"), "{how}");
        // no exposed pad found: none made, and said so
        let mut f = PartForm::default();
        let how = f.apply_no_lead(&p, 2, Some(&LandPattern { ep: None, ep_pin: None, ..land }), None);
        assert!(!f.thermal_on && how.contains("D2・E2"), "{how}");
        assert_eq!(exposed_pad_pin("1 A\n17 EP"), Some("17".into()));
        assert_eq!(exposed_pad_pin("1 A\n2 B"), None);
    }

    /// Nexperia's soldering footprint of a SOD882 goes into the form as the two-pad chip: the pads 0.4 mm along the line through
    /// them and 0.7 mm across, their centres 0.7 mm apart.
    #[test]
    fn a_two_pad_footprint_drawing_fills_the_form() {
        let land = LandPattern { page: 8, family: Some("sod-882"), pins: 2, pad_l: 0.4, pad_w: 0.7, pitch: None, row: Some(0.7), quad: false, ep: None, ep_pin: None };
        let p = PackageName { name: "SOD-882".into(), family: "sod-882", pins: None, count: 1, page: 0 };
        let mut f = PartForm { fp_preset: "D_SOD-123".into(), pins_text: "1 K
2 A".into(), ..Default::default() };
        let how = f.apply_two_pads(&p, &land).unwrap();
        assert!(f.pkg == PkgKind::Chip2 && f.fp_preset.is_empty() && (f.pitch, f.len, f.width) == (0.7, 0.4, 0.7), "{how}");
        assert!(how.contains("推奨のパッドの図 (p.9)"), "{how}");
        let fp = f.to_def().unwrap().package.build("SOD882").unwrap();
        assert_eq!(fp.pads.len(), 2);
        assert!((fp.pads[1].pos.x - fp.pads[0].pos.x - 0.7).abs() < 1e-9, "{:?}", fp.pads);
        // a drawing without the centres' distance fills nothing
        let mut f = PartForm::default();
        assert!(f.apply_two_pads(&p, &LandPattern { row: None, ..land }).is_none() && f.pkg != PkgKind::Chip2);
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
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23"), None).as_deref(), Some("SOT-23"));
        assert_eq!(builtin_footprint(&p("TO-92", "to-92"), None).as_deref(), Some("TO-92"));
        assert!(builtin_footprint(&p("SOIC-8", "soic"), None).is_none());
        // the pins' numbers pick the outline: a 5-pin part's "SOT-23" is the SOT-23-5, pins 1 to 3 the numbered SOT-23
        let nums = |n: usize| (1..=n).map(|i| i.to_string()).collect::<Vec<_>>();
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23"), Some(&nums(5))).as_deref(), Some("SOT-23-5"));
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23"), Some(&nums(3))).as_deref(), Some("SOT-23-3"));
        let bec: Vec<String> = ["B", "E", "C"].iter().map(|s| s.to_string()).collect();
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23"), Some(&bec)).as_deref(), Some("SOT-23"));
        assert!(builtin_footprint(&p("SOT-23", "sot-23"), Some(&nums(8))).is_none());
        assert_eq!(builtin_footprint(&p("SOT-223", "sot-223"), Some(&nums(3))).as_deref(), Some("SOT-223"));
        assert_eq!(builtin_footprint(&p("TO-220", "to-220"), Some(&nums(3))).as_deref(), Some("TO-220-3"));
        assert_eq!(builtin_footprint(&p("TO-220", "to-220"), None).as_deref(), Some("TO-220-3"), "the one TO-220");
        assert_eq!(builtin_footprint(&p("SOT-23", "sot-23"), None).as_deref(), Some("SOT-23"));
        // LM317's tab printed with the output's name
        let lm317: Vec<(String, String)> = [("1", "ADJUST"), ("2", "OUTPUT"), ("3", "INPUT"), ("TAB", "OUTPUT")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(tab_as_pin_2(&lm317).as_deref(), Some("TAB"));
        assert_eq!(tab_as_pin_2(&lm317[..3]), None);
        // a table of a column per package: the pins of the package chosen
        let src = |pkg: Option<&str>, n: usize| FoundPins { page: 0, area: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }, num_col: 0, name_col: 1, pins: (1..=n).map(|i| (i.to_string(), format!("P{i}"))).collect(), io: vec![], from_drawing: false, package: pkg.map(str::to_string) };
        let sources = vec![src(Some("DFN-6"), 7), src(Some("SOT-23"), 3), src(Some("SOT-89/TO-92"), 3)];
        assert_eq!(pins_of_package(&sources, &p("TO-92", "to-92")), Some(2));
        assert_eq!(pins_of_package(&sources, &p("SOT-23", "sot-23")), Some(1));
        assert_eq!(pins_of_package(&[src(None, 3)], &p("SOT-23", "sot-23")), None);
        assert!(is_dual("tssop") && is_dual("sot-363") && !is_dual("sot-223"));
        // a diode's outline by its D_ name
        assert_eq!(builtin_footprint(&p("SOD-123", "sod-123"), None).as_deref(), Some("D_SOD-123"));
    }
}
