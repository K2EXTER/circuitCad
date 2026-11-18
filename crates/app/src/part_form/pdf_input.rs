//! Filling the fields of the form from numbers clicked in the data sheet window.

use super::datasheet_view::{Picked, Target};
use super::*;

/// Where a value came from: the field, the page (from 0) and the text that was clicked.
pub type Source = (Target, usize, String);

impl PartForm {
    /// Takes what was clicked in the data sheet window, if anything, and puts it in its field.
    pub(super) fn take_pdf_pick(&mut self) {
        if let Some(p) = self.sheet.take_picked() {
            self.apply_pick(p);
        }
    }

    /// Takes the pins the person accepted in the pin table window, if any, and puts them in the pin box.
    pub(super) fn take_pdf_pins(&mut self) {
        use super::pin_import::PinMode;
        let Some(imp) = self.sheet.take_pins() else { return };
        // the table stays the one the pins are compared with from now on
        // a drawing has no columns to read again: nothing to compare with later
        self.pin_ref = (!imp.from_drawing).then_some(super::pdf_check::PinRef { page: imp.page, area: imp.area, num_col: imp.num_col, name_col: imp.name_col });
        self.pin_check = None;
        let read = imp.lines.lines().count();
        self.pick_msg = Some(match imp.mode {
            PinMode::CheckOnly => (true, format!("ピン表は変えずに、p.{} の表 ({read} 本) と比べます。結果は「③ ピン」に出ます", imp.page + 1)),
            mode => {
                self.pins_text = super::pin_import::merged(&self.pins_text, &imp.lines, mode == PinMode::Append);
                let n = self.pins_text.lines().filter(|l| !l.trim().is_empty()).count();
                let how = if mode == PinMode::Append { "あとに足しました" } else { "入れました" };
                (true, format!("ピン表に p.{} の {read} 本のピンを{how} (合計 {n} 本)。向き (left/right) は「③ ピン」で書き足せます", imp.page + 1))
            }
        });
    }

    /// A field's "グラフから読む" by the field's name in the code ("DarlHfe"; `--form-graph-for`, for screenshots).
    pub fn graph_for(&mut self, name: &str) {
        if let Some(d) = super::graph_reader::Dest::by_name(name) {
            self.sheet.start_graph(Some(d));
        }
    }

    /// Takes the points read off a graph that the person sent to the form, and puts them in their field.
    pub(super) fn take_pdf_graph(&mut self) {
        if let Some(out) = self.sheet.take_graph() {
            self.put_graph(&out);
        }
    }

    /// 「表とグラフから全部読む」: the section's values from the electrical characteristics table (as its 「PDF の表から
    /// 読む」), then every curve field read off its graph without a click (`SheetView::read_curve`: the curve read
    /// takes the place of the table's points), then the model made from what is in the fields. A field whose graph
    /// could not be read keeps what it had; each field gets a line of what was read or why not (`--form-graph-all`).
    pub fn read_all_graphs(&mut self, dests: &[super::graph_reader::Dest]) {
        use super::graph_reader::Dest;
        let sheet = &self.sheet;
        match dests.first() {
            Some(Dest::BjtHfe) => self.bjt.fill_from_pdf(&self.name, |s| sheet.spec_values(s)),
            Some(Dest::DarlHfe) => {
                self.darl.fill_from_pdf(|s| sheet.spec_values(s));
                self.darl.put_resistors(sheet.darlington_resistors());
            }
            Some(Dest::OptoLed) => self.opto.fill_from_pdf(|s| sheet.spec_values(s)),
            Some(Dest::PdLight) => self.pd.fill_from_pdf(|s| sheet.spec_values(s)),
            Some(Dest::DiodeForward) => self.dio_fill_from_pdf(),
            Some(Dest::RegDropout) => {
                // a part pasted from a draft has no name in the section: the form's
                let name = self.name.clone();
                self.reg.fill_from_pdf(&name, |s| sheet.spec_values_for(s, &name));
                self.reg_find_variants();
            }
            _ => {}
        }
        let read: Vec<_> = dests.iter().map(|&d| (d, self.sheet.read_curve(d))).collect();
        self.take_read_curves(read);
    }

    /// 「表とグラフから全部読む」 of a section by its name (`--form-graph-all`): bjt, darl, diode, opto, pd, reg.
    pub fn graph_all_for(&mut self, section: &str) {
        use super::graph_reader::Dest;
        let dests: &[Dest] = match section {
            "bjt" => &[Dest::BjtHfe, Dest::BjtVbeOn, Dest::BjtVbeSat, Dest::BjtVceSat, Dest::BjtCobo, Dest::BjtCibo],
            "darl" => &[Dest::DarlHfe, Dest::DarlVbeOn, Dest::DarlVbeSat, Dest::DarlVceSat],
            "diode" => &[Dest::DiodeForward, Dest::DiodeCap],
            "opto" => &[Dest::OptoLed, Dest::OptoCtr],
            "pd" => &[Dest::PdLight, Dest::PdCap],
            "reg" => &[Dest::RegDropout, Dest::RegRipple, Dest::RegNoise],
            _ => return,
        };
        self.read_all_graphs(dests);
        // what the table gave first (the regulator's fields are values, not curves: the graph's are said beside them)
        if section == "reg" {
            for (ok, line) in &self.reg.found {
                eprintln!("{} table: {line}", if *ok { "ok" } else { "--" });
            }
            if !self.reg.variants.is_empty() {
                let list: Vec<String> = self.reg.variants.iter().map(|(v, p)| format!("{} V {p}", super::linear::show(*v))).collect();
                eprintln!("variants: {}", list.join(", "));
            }
        }
        for (_, ok, line) in &self.graph_all {
            eprintln!("{} {line}", if *ok { "ok" } else { "--" });
        }
        // the model made from them, for a script to look at
        let result = match section {
            "bjt" => self.bjt.result.as_ref(),
            "darl" => self.darl.result.as_ref(),
            "opto" => self.opto.result.as_ref(),
            "pd" => self.pd.result.as_ref(),
            "reg" => self.reg.result.as_ref(),
            _ => None,
        };
        match result {
            Some(Ok(f)) => {
                let worst = f.rows.iter().filter(|r| !r.compare_only).map(|r| r.error_pct().abs()).fold(0.0, f64::max);
                eprintln!("model {} (rows {}, worst {worst:.0}%)", f.text.lines().find(|l| l.starts_with(".model")).unwrap_or(""), f.rows.len());
                for r in &f.rows {
                    eprintln!("  {} {}: {} / {} {} ({:+.0}%){}", r.item, r.at, part_lib::fit::num::g(r.datasheet, 4), part_lib::fit::num::g(r.model, 4), r.unit, r.error_pct(), if r.compare_only { " compare" } else { "" });
                }
            }
            Some(Err(e)) => eprintln!("model error: {e}"),
            None => {}
        }
        if let Some(m) = &self.dio_message {
            eprintln!("diode {m:?}");
        }
        for (name, text) in [("hfe", &self.bjt.hfe), ("vbe_on", &self.bjt.vbe_on), ("darl_hfe", &self.darl.hfe), ("dio", &self.dio_points)] {
            if !text.is_empty() {
                eprintln!("{name}: {}", text.lines().collect::<Vec<_>>().join(" | "));
            }
        }
    }

    /// The section's model put into the simulation tab (its 「② シミュレーション」に入れる) and the part saved into `dir`
    /// as the form's save button does (`--form-save`, to place the part in a circuit with `ccad --lib dir`).
    pub fn use_model_and_save(&mut self, section: &str, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let not_made = || "モデルができていません".to_string();
        match section {
            "bjt" => self.bjt_apply()?,
            "diode" => self.dio_apply()?,
            "darl" => {
                let Some(Ok(f)) = self.darl.result.clone() else { return Err(not_made()) };
                self.linear_apply(&f, &[("コレクタ", &["C", "COLLECTOR"]), ("ベース", &["B", "BASE"]), ("エミッタ", &["E", "EMITTER"])])?
            }
            "opto" => {
                let Some(Ok(f)) = self.opto.result.clone() else { return Err(not_made()) };
                self.linear_apply(&f, &[("LED のアノード", &["A", "ANODE"]), ("LED のカソード", &["K", "CATHODE"]), ("エミッタ", &["E", "EMITTER"]), ("コレクタ", &["C", "COLLECTOR"])])?
            }
            "pd" => {
                let Some(Ok(f)) = self.pd.result.clone() else { return Err(not_made()) };
                self.add_light_pin();
                self.linear_apply(&f, super::opto::PD_ROLES)?
            }
            "reg" => {
                let Some(Ok(f)) = self.reg.result.clone() else { return Err(not_made()) };
                self.linear_apply(&f, self.reg_roles())?
            }
            // the part as the form has it
            "" => String::new(),
            _ => return Err(format!("知らない欄: {section}")),
        };
        let def = self.to_def()?;
        part_lib::save(dir, &def).map_err(|e| e.to_string())
    }

    /// The factor that turns a normalized hFE curve ((IC, ratio) points) into hFE, and a sentence of how: the table's
    /// hFE row (the part's own rank when the table has rows per rank) at the current nearest the curve's peak, its typ or
    /// else the geometric mean of its min and max (as the bundled 2N3904), divided by the curve's ratio there.
    pub(super) fn hfe_scale(&self, points: &[(f64, f64)]) -> Result<(f64, String), String> {
        let none = || i18n::tr("表に、図の電流の範囲の hFE の typ も min・max もありません。表の hFE を掛けて入れてください").to_string();
        if points.len() < 2 {
            return Err(none());
        }
        let ratio_at = |ic: f64| -> Option<f64> {
            let w = points.windows(2).find(|w| (w[0].0.min(w[1].0)..=w[0].0.max(w[1].0)).contains(&ic))?;
            let t = (ic.ln() - w[0].0.ln()) / (w[1].0.ln() - w[0].0.ln());
            Some((w[0].1.ln() + t.clamp(0.0, 1.0) * (w[1].1.ln() - w[0].1.ln())).exp())
        };
        let peak = points.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map_or(1e-2, |p| p.0);
        let mut rows = self.sheet.spec_values("hFE");
        if rows.is_empty() {
            rows = self.sheet.spec_values("HFE");
        }
        // rows per rank: only the part's own ("BC546B/547B/548B · (IC = 2.0 mA ...)" for BC546B; the first row of a
        // group carries its names after the conditions, "(IC = 100 mA ...) BC547A/548A", and is another rank's)
        let name = self.name.trim();
        if !name.is_empty() && rows.iter().any(|v| v.conditions.contains(name)) {
            rows.retain(|v| v.conditions.contains(name));
        }
        let best = rows
            .iter()
            .filter_map(|v| {
                let ic = v.condition("IC")?.abs();
                let (h, how) = match (v.typ, v.min, v.max) {
                    (Some(t), _, _) => (t.abs(), i18n::trf!("typ {}", part_lib::fit::num::g(t.abs(), 3))),
                    (None, Some(a), Some(b)) if a.abs() > 0.0 && b.abs() > a.abs() => {
                        let g = (a.abs() * b.abs()).sqrt();
                        (g, i18n::trf!("min {}・max {} の幾何平均 {}", part_lib::fit::num::g(a.abs(), 3), part_lib::fit::num::g(b.abs(), 3), part_lib::fit::num::g(g, 3)))
                    }
                    _ => return None,
                };
                let r = ratio_at(ic)?;
                Some(((ic / peak).ln().abs(), h / r, how, ic, v.page))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let (_, k, how, ic, page) = best.ok_or_else(none)?;
        Ok((k, i18n::trf!("hFE の図が比 (normalized) なので、表 (p.{}) の hFE {how} (IC = {}A) に合わせました。実物の hFE は、ランクと個体で大きく違います", page + 1, part_lib::passive::eng(ic))))
    }

    /// Puts the curves read by 「表とグラフから全部読む」 in their fields, says what was read, and makes the model.
    pub(super) fn take_read_curves(&mut self, read: Vec<(super::graph_reader::Dest, Result<super::graph_reader::GraphOut, String>)>) {
        use super::graph_reader::Dest;
        self.graph_all.clear();
        let Some(&(first, _)) = read.first() else { return };
        for (d, r) in read {
            let field = i18n::tr(d.curve_name());
            // the two capacitances of a transistor have their own names
            let field = match d {
                Dest::BjtCobo => "Cobo",
                Dest::BjtCibo => "Cibo",
                Dest::OptoLed => i18n::tr("LED の順方向"),
                _ => field,
            };
            // a normalized hFE graph: its ratios times the table's hFE
            let mut scaled = None;
            let r = match r {
                Ok(mut out) if out.normalized => match self.hfe_scale(&out.points) {
                    Ok((k, note)) => {
                        out.points.iter_mut().for_each(|p| p.1 *= k);
                        scaled = Some(note);
                        Ok(out)
                    }
                    Err(e) => Err(i18n::trf!("p.{} の図は 1 を基準にした比 (normalized) で、{e}", out.page + 1)),
                },
                r => r,
            };
            match r {
                // a regulator's curve gives a value or two: the line says which, or why it was not used
                Ok(out) if matches!(d, Dest::RegDropout | Dest::RegRipple | Dest::RegNoise) => {
                    let line = self.reg.take_graph(d, &out.points, out.page);
                    let by = if out.by.is_empty() { String::new() } else { format!(" ({})", out.by) };
                    match line {
                        Ok(l) => self.graph_all.push((d, true, format!("{l}{by}"))),
                        Err(e) => self.graph_all.push((d, false, format!("{field}: {e}"))),
                    }
                }
                Ok(out) => {
                    let by = if out.by.is_empty() { String::new() } else { format!(" ({})", out.by) };
                    let how = scaled.as_ref().map_or(String::new(), |n| format!("。{n}"));
                    let at = out.vce.map_or(String::new(), |v| format!(" (VCE = {} V)", number_text(v)));
                    self.graph_all.push((d, true, i18n::trf!("{field}: p.{} の図から {} 点{by}{at}{how}", out.page + 1, out.points.len())));
                    self.put_graph(&out);
                    // the graph's VCE into its field, so the fit takes the curve at the voltage it was measured at
                    if let Some(v) = out.vce {
                        let v = number_text(v);
                        match d {
                            Dest::BjtHfe => self.bjt.hfe_vce = v,
                            Dest::BjtVbeOn => self.bjt.on_vce = v,
                            Dest::DarlHfe => self.darl.hfe_vce = v,
                            Dest::DarlVbeOn => self.darl.on_vce = v,
                            _ => {}
                        }
                    }
                    if scaled.is_some() {
                        self.bjt.hfe_note = scaled;
                    }
                }
                Err(why) => self.graph_all.push((d, false, i18n::trf!("{field}: 読めませんでした ({why})。欄の「グラフから読む」で、曲線をクリックしてください"))),
            }
        }
        if !self.graph_all.iter().any(|l| l.1) {
            return;
        }
        // the model from the fields as they are now (the points read and what was there)
        match first {
            Dest::DiodeForward | Dest::DiodeCap => self.dio_message = Some(self.dio_fit()),
            Dest::BjtHfe | Dest::BjtVbeOn | Dest::BjtVbeSat | Dest::BjtVceSat | Dest::BjtCobo | Dest::BjtCibo => self.bjt_fit(),
            Dest::DarlHfe | Dest::DarlVbeOn | Dest::DarlVbeSat | Dest::DarlVceSat => self.darl_fit(),
            Dest::OptoLed | Dest::OptoCtr => self.opto_make(),
            Dest::PdLight | Dest::PdCap => self.pd_make(),
            Dest::RegDropout | Dest::RegRipple | Dest::RegNoise => self.regulator_make(),
            Dest::Zth => {}
        }
    }

    /// The 「表とグラフから全部読む」 button of a section and the lines of what it read. Nothing while no PDF is open.
    pub(super) fn read_all_button(&mut self, ui: &mut egui::Ui, dests: &[super::graph_reader::Dest]) {
        if !self.sheet.has_doc() {
            return;
        }
        if ui
            .button(i18n::tr("表とグラフから全部読む"))
            .on_hover_text(i18n::tr("「PDF の表から読む」のあと、下の曲線の欄を全部、図の見出しから探した図で読みます (目盛りは図の数字から、曲線はラベルか 1 本だけのものを選びます)。読めた欄を入れて、モデルを作ります。読めなかった欄は、欄の「グラフから読む」で曲線をクリックします。図によって数秒かかります"))
            .clicked()
        {
            self.read_all_graphs(dests);
        }
        for (_, ok, line) in self.graph_all.iter().filter(|l| dests.contains(&l.0)) {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
    }

    /// Puts points read off a graph in their field.
    pub(super) fn put_graph(&mut self, out: &super::graph_reader::GraphOut) {
        use super::graph_reader::{Dest, short};
        match out.dest {
            Dest::DiodeForward => {
                // the fit wants currents 1.5 times apart or more: a traced curve has points close together at its top
                let points = apart(&out.points, 1.5);
                self.dio_points = points.iter().map(|(i, v)| format!("{} {}", short(*i), short(*v))).collect::<Vec<_>>().join("\n");
                self.dio_message = Some(self.dio_fit().map(|m| format!("p.{} のグラフの {} 点を入れました。{m}", out.page + 1, points.len())));
            }
            Dest::DiodeCap => {
                self.dio_caps = out.points.iter().map(|(v, c)| format!("{} {}", short(v.abs()), short(c.abs()))).collect::<Vec<_>>().join("
");
                self.dio_message = Some(Ok(format!("p.{} のグラフの {} 点を「容量の点」に入れました", out.page + 1, out.points.len())));
            }
            Dest::Zth => {
                self.rt_zth_points = out.points.iter().map(|(t, z)| format!("{} {}", short(t.abs()), short(z.abs()))).collect::<Vec<_>>().join("\n");
                self.rt_zth_msg = Some(Ok(format!("p.{} のグラフの {} 点を入れました。「段に当てはめる」を押してください", out.page + 1, out.points.len())));
            }
            Dest::DarlHfe | Dest::DarlVbeOn | Dest::DarlVbeSat | Dest::DarlVceSat => self.darl_take_graph(out.dest, &out.points, out.page),
            Dest::OptoLed | Dest::OptoCtr | Dest::PdLight | Dest::PdCap => self.opto_take_graph(out.dest, &out.points, out.page),
            Dest::RegDropout | Dest::RegRipple | Dest::RegNoise => {
                self.reg.message = Some(match self.reg.take_graph(out.dest, &out.points, out.page) {
                    Ok(l) => (true, i18n::trf!("{l}。「モデルを作る」を押してください")),
                    Err(e) => (false, e),
                });
            }
            d => self.bjt_take_graph(d, &out.points, out.page),
        }
    }

    /// Puts a clicked word in the field it was asked for. A number goes through the unit conversion; what was
    /// assumed or converted is said in `pick_msg`, and a number that cannot be read leaves the field alone.
    pub(super) fn apply_pick(&mut self, p: Picked) {
        let Some(q) = p.target.quantity() else {
            self.name = p.text.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            self.pick_msg = None;
            return;
        };
        let Some(mut v) = datasheet::parse_value(&p.text) else {
            self.pick_msg = Some((false, format!("「{}」は数字として読めませんでした。数字の部分をクリックしてください", p.text)));
            return;
        };
        if v.unit.is_none() {
            v.unit = p.unit_after;
        }
        let c = match datasheet::convert(&v, q) {
            Ok(c) => c,
            Err(e) => {
                self.pick_msg = Some((false, format!("「{}」: {e}", p.text)));
                return;
            }
        };
        let x = c.value;
        match p.target {
            Target::Pitch => self.pitch = x,
            Target::Row => self.row = x,
            Target::PadLen => self.len = x,
            Target::PadWidth => self.width = x,
            Target::Span => self.span = x,
            Target::PadDia => self.pad_d = x,
            Target::Drill => self.drill = x,
            Target::BodyW => self.body_w = x,
            Target::BodyH => self.body_h = x,
            Target::Height => self.height = x,
            Target::MaxV => self.rt_v = number_text(x),
            Target::MaxI => self.rt_i = number_text(x),
            Target::MaxP => self.rt_p = number_text(x),
            Target::ThermalW => self.thermal_w = x,
            Target::ThermalH => self.thermal_h = x,
            Target::Name => {}
        }
        self.sources.retain(|s| s.0 != p.target);
        self.sources.push((p.target, p.page, p.text.clone()));
        let unit = match q {
            datasheet::Quantity::Length => "mm",
            datasheet::Quantity::Voltage => "V",
            datasheet::Quantity::Current => "A",
            datasheet::Quantity::Power => "W",
        };
        let note = c.note.map(|n| format!(" ({n})")).unwrap_or_default();
        self.pick_msg = Some((true, format!("{} に {} {unit} を入れました (p.{} の「{}」){note}", p.target.label(), number_text(x), p.page + 1, p.text)));
    }

    /// A small button that asks for a number to be clicked in the PDF for `target`. It shows the page when the field
    /// was filled that way. Nothing is drawn while no PDF is open.
    pub(super) fn pick_button(&mut self, ui: &mut egui::Ui, target: Target, text: &str) {
        if !self.sheet.has_doc() {
            return;
        }
        let from = self.sources.iter().find(|s| s.0 == target).map(|s| (s.1, s.2.clone()));
        let label = match &from {
            Some((page, _)) => format!("{text} ✓p.{}", page + 1),
            None => text.to_string(),
        };
        let tip = match &from {
            Some((page, word)) => format!("p.{} の「{word}」から入れました。押すと、選び直せます", page + 1),
            None => format!("データシートの窓で、{}の数字をクリックして入れます。インチ・mil は mm に換算します (単位は数字の右にあるものを使います)", target.label()),
        };
        if ui.small_button(i18n::tr_dyn(&label)).on_hover_text(i18n::tr_dyn(&tip)).clicked() {
            self.pick_msg = None;
            self.sheet.start_pick(target);
        }
    }

    /// A row of buttons, one per field, labelled with the field names. Nothing while no PDF is open (the PDF is
    /// opened from the button on the tab row).
    pub(super) fn pick_row(&mut self, ui: &mut egui::Ui, targets: &[Target]) {
        if !self.sheet.has_doc() || targets.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(i18n::tr("PDFから入れる"));
            for t in targets {
                self.pick_button(ui, *t, t.label());
            }
        });
    }

    /// What the last click in the PDF did, or why it could not.
    pub(super) fn pick_message(&self, ui: &mut egui::Ui) {
        if let Some((ok, m)) = &self.pick_msg {
            let color = if *ok { Color32::from_rgb(90, 190, 120) } else { Color32::from_rgb(230, 80, 70) };
            ui.colored_label(color, m);
        }
    }
}

/// Points sorted by their first value with the next at least `ratio` times the one before: the rest are left out,
/// except the last (the largest current, which tells the series resistance) that takes the place of the one before it.
pub(super) fn apart(points: &[(f64, f64)], ratio: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = vec![];
    for &p in points {
        match out.last() {
            Some(l) if p.0 < l.0 * ratio => {}
            _ => out.push(p),
        }
    }
    if let (Some(&last), Some(kept)) = (points.last(), out.last_mut())
        && *kept != last
    {
        *kept = last;
    }
    out
}

/// 1.27 -> "1.27", 0.2 -> "0.2", 30.0 -> "30": the number as a person writes it in a box.
pub(super) fn number_text(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(form: &mut PartForm, target: Target, text: &str, unit_after: Option<&str>) {
        form.apply_pick(Picked { target, text: text.into(), unit_after: unit_after.map(String::from), page: 4 });
    }

    #[test]
    fn a_clicked_number_goes_to_its_field_in_millimetres() {
        let mut f = PartForm::default();
        pick(&mut f, Target::Pitch, "0.65", Some("mm"));
        assert_eq!(f.pitch, 0.65);
        assert_eq!(f.sources, [(Target::Pitch, 4, "0.65".to_string())]);
        assert!(f.pick_msg.as_ref().is_some_and(|(ok, m)| *ok && m.contains("p.5")));
        // inches written after the number
        pick(&mut f, Target::Row, ".300", Some("in"));
        assert_eq!(f.row, 7.62);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("換算"));
        // no unit anywhere: taken as mm and said so
        pick(&mut f, Target::PadLen, "1.5", None);
        assert_eq!(f.len, 1.5);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("単位が書かれていない"));
        // the same field again replaces its source
        pick(&mut f, Target::Pitch, "1.27", None);
        assert_eq!(f.sources.iter().filter(|s| s.0 == Target::Pitch).count(), 1);
        assert_eq!(f.pitch, 1.27);
    }

    #[test]
    fn a_wrong_click_leaves_the_field_alone_and_says_why() {
        let mut f = PartForm::default();
        let before = f.pitch;
        pick(&mut f, Target::Pitch, "SOIC", None);
        assert_eq!(f.pitch, before);
        assert!(f.pick_msg.as_ref().is_some_and(|(ok, m)| !*ok && m.contains("読めません")));
        pick(&mut f, Target::Pitch, "30V", None);
        assert_eq!(f.pitch, before);
        assert!(f.pick_msg.as_ref().unwrap().1.contains("長さ"));
        assert!(f.sources.is_empty());
    }

    #[test]
    fn ratings_take_plain_numbers() {
        let mut f = PartForm::default();
        pick(&mut f, Target::MaxV, "30V", None);
        pick(&mut f, Target::MaxI, "200mA", None);
        pick(&mut f, Target::MaxP, "625", Some("mW"));
        assert_eq!((f.rt_v.as_str(), f.rt_i.as_str(), f.rt_p.as_str()), ("30", "0.2", "0.625"));
        pick(&mut f, Target::MaxV, "5.7A", None);
        assert_eq!(f.rt_v, "30");
    }

    #[test]
    fn a_name_is_trimmed_of_marks() {
        let mut f = PartForm::default();
        pick(&mut f, Target::Name, "LM393B,", None);
        assert_eq!(f.name, "LM393B");
    }

    #[test]
    fn curves_read_together_fill_their_fields_and_say_what_failed() {
        use super::super::graph_reader::{Dest, GraphOut};
        let mut f = PartForm::default();
        f.bjt.cibo = "0.5 8p".into();
        let out = |dest, points: &[(f64, f64)], by: &str| Ok(GraphOut { dest, points: points.to_vec(), page: 4, by: by.into(), normalized: false, vce: None });
        f.take_read_curves(vec![
            (Dest::BjtHfe, out(Dest::BjtHfe, &[(1e-3, 100.0), (1e-2, 150.0), (0.1, 90.0)], "25°C").map(|o| GraphOut { vce: Some(5.0), ..o })),
            (Dest::BjtVbeOn, out(Dest::BjtVbeOn, &[(1e-3, 0.65), (1e-2, 0.72), (0.1, 0.83)], "")),
            (Dest::BjtCibo, Err("図の見出しが見つかりません".into())),
        ]);
        assert!(f.bjt.hfe.contains("150") && f.bjt.vbe_on.contains("0.72"));
        assert_eq!(f.bjt.cibo, "0.5 8p", "a curve not read leaves the field as it was");
        let lines: Vec<&str> = f.graph_all.iter().map(|l| l.2.as_str()).collect();
        assert!(lines[0].contains("p.5") && lines[0].contains("3 点") && lines[0].contains("25°C"), "{lines:?}");
        assert!(!f.graph_all[2].1 && lines[2].starts_with("Cibo") && lines[2].contains("見出し"), "{lines:?}");
        assert!(f.bjt.result.is_some(), "the model is made from the fields");
        // the VCE the hFE graph prints goes into its field (the fit takes the curve at it); VBE(on)'s said none
        assert_eq!((f.bjt.hfe_vce.as_str(), f.bjt.on_vce.as_str()), ("5", "1"));
        assert!(lines[0].contains("VCE = 5 V"), "{lines:?}");
        // nothing read: no model
        let mut f = PartForm::default();
        f.take_read_curves(vec![(Dest::PdLight, Err("x".into()))]);
        assert!(f.pd.result.is_none() && f.graph_all.len() == 1);
        // a normalized hFE curve with no table hFE to scale it by: not put in, and why
        let mut f = PartForm::default();
        f.bjt.hfe = "1m 100".into();
        let ratios = GraphOut { dest: Dest::BjtHfe, points: vec![(1e-3, 0.7), (1e-2, 1.0), (0.1, 0.5)], page: 5, by: String::new(), normalized: true, vce: None };
        f.take_read_curves(vec![(Dest::BjtHfe, Ok(ratios))]);
        assert_eq!(f.bjt.hfe, "1m 100");
        assert!(!f.graph_all[0].1 && f.graph_all[0].2.contains("normalized"), "{:?}", f.graph_all);
    }

    #[test]
    fn traced_points_are_thinned_to_currents_apart() {
        let p = [(1.0, 0.6), (1.2, 0.61), (2.0, 0.65), (60.0, 1.4), (82.0, 1.57)];
        assert_eq!(apart(&p, 1.5), vec![(1.0, 0.6), (2.0, 0.65), (82.0, 1.57)], "the largest takes the place of 60");
        assert_eq!(apart(&[], 1.5), vec![]);
    }

    #[test]
    fn numbers_are_written_as_a_person_would() {
        assert_eq!((number_text(1.27), number_text(30.0), number_text(0.2), number_text(0.625)), ("1.27".into(), "30".into(), "0.2".into(), "0.625".into()));
    }
}
