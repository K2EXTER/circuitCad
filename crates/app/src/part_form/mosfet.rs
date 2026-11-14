//! "かんたん作成" for a MOSFET (M10, step 3): the threshold, the on resistances, the capacitances and gFS from the
//! data sheet's table (typed, or offered from the open PDF by `datasheet::spec_table`), fitted with the same
//! level 1 + RD model as the bundled MOSFETs (`part_lib::fit`), with a table of how far the model lands from each
//! number and why a large gap is the model's form, not a typing mistake.

use super::*;
use part_lib::fit::{self, Fitted};

/// The AO3400A's typical values (Alpha & Omega Rev. 3.1, the bundled part's readings): the example button.
const EXAMPLE: (&str, &str, &str, &str, &str, &str) = ("1.05", "250u", "10 5.7 18m\n4.5 5 19m\n2.5 3 24m", "630p", "50p", "33 5 5.7");

/// What the form holds for the MOSFET section, as typed.
#[derive(Default)]
pub struct MosInput {
    pub vth: String,
    pub vth_id: String,
    /// "VGS ID RDS(on)" lines.
    pub rds: String,
    pub ciss: String,
    pub crss: String,
    /// "gFS VDS ID" (optional).
    pub gfs: String,
    /// Where the numbers taken from the PDF came from (one line each), or what was not found.
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

fn num(label: &str, t: &str) -> Result<f64, String> {
    let l = t.trim().to_ascii_lowercase();
    let l = l.trim_end_matches(['v', 'a', 'f', 's', 'Ω']).trim_end_matches("ohm").trim();
    sim_core::parse_value(l).map(f64::abs).filter(|v| *v > 0.0).ok_or_else(|| format!("{label}「{}」を、0 より大きい数で入れてください (例: 1.05、250u、18m)", t.trim()))
}

/// A number as typed into a box: 0.018 -> "18m", 1.05 -> "1.05".
fn short(v: f64) -> String {
    if v.abs() < 0.1 { part_lib::passive::eng(v) } else { part_lib::fit::num::g(v, 4) }
}

impl MosInput {
    /// The readings as the fit takes them.
    pub fn spec(&self, name: &str, source: &str, pch: bool) -> Result<fit::Spec, String> {
        let mut rds = vec![];
        for (n, line) in self.rds.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            let [vgs, id, r] = t.as_slice() else {
                return Err(format!("オン抵抗の {}行目: 「VGS ID RDS(on)」の 3 つの数で書いてください (例: 4.5 5 19m)", n + 1));
            };
            rds.push([num("VGS", vgs)?, num("ID", id)?, num("RDS(on)", r)?]);
        }
        let gfs = if self.gfs.trim().is_empty() {
            None
        } else {
            let t: Vec<&str> = self.gfs.split_whitespace().collect();
            let [g, vds, id] = t.as_slice() else { return Err("gFS は「gFS VDS ID」の 3 つの数で書いてください (例: 33 5 5.7)。空でも作れます".into()) };
            Some([num("gFS", g)?, num("VDS", vds)?, num("ID", id)?])
        };
        let json = serde_json::json!({
            "kind": "mos",
            "part": name,
            "name": part_lib::diode::model_name(name),
            "pch": pch,
            "source": source,
            "vth": [num("VGS(th)", &self.vth)?, num("VGS(th) の ID", &self.vth_id)?],
            "rds": rds,
            "caps": {"ciss": num("Ciss", &self.ciss)?, "crss": num("Crss", &self.crss)?},
            "gfs": gfs,
        });
        serde_json::from_value(json).map_err(|e| e.to_string())
    }

    /// Fills the boxes from the open PDF's table: the typ of each symbol and its conditions. What is not found (no
    /// table, no typ) is said and left as it was.
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<datasheet::spec_table::SpecValue>) {
        use datasheet::spec_table::SpecValue;
        self.found.clear();
        let typ = |v: &SpecValue| v.typ_si().map(f64::abs);
        let at = |v: &SpecValue| format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions);
        let first = |symbols: &[&str]| symbols.iter().map(|s| values(s)).find(|v| !v.is_empty()).unwrap_or_default();
        // threshold
        let vth = first(&["VGS(th)", "Vth", "VGS(TO)"]);
        match vth.iter().find(|v| typ(v).is_some()) {
            Some(v) => {
                self.vth = short(typ(v).unwrap_or_default());
                if let Some(i) = v.condition("ID") {
                    self.vth_id = short(i);
                }
                self.found.push((true, format!("VGS(th) {} V: {}", self.vth, at(v))));
            }
            None => self.found.push((false, if vth.is_empty() { "VGS(th) が表に見つかりません".into() } else { format!("VGS(th) に typ がありません ({})。min と max の間を自分で入れてください", at(&vth[0])) })),
        }
        // on resistances: every line with a gate voltage and a current (the hot lines, TJ = 125 °C, are left out)
        let rds = first(&["RDS(on)", "RDS(ON)", "rDS(on)"]);
        let lines: Vec<String> = rds
            .iter()
            .filter(|v| !v.conditions.contains("125") && !v.conditions.to_ascii_uppercase().contains("TJ"))
            .filter_map(|v| Some(format!("{} {} {}", short(v.condition("VGS")?), short(v.condition("ID")?), short(typ(v)?))))
            .collect();
        if lines.is_empty() {
            self.found.push((false, if rds.is_empty() { "RDS(on) が表に見つかりません".into() } else { "RDS(on) の行に typ・VGS・ID がそろっていません。自分で入れてください".into() }));
        } else {
            self.rds = lines.join("\n");
            self.found.push((true, format!("RDS(on) {} 行: p.{}", lines.len(), rds[0].page + 1)));
        }
        for (sym, field) in [("Ciss", 0), ("Crss", 1)] {
            let v = first(&[sym]);
            match v.iter().find(|v| typ(v).is_some()) {
                Some(v) => {
                    let s = short(typ(v).unwrap_or_default());
                    self.found.push((true, format!("{sym} {s}F: {}", at(v))));
                    if field == 0 { self.ciss = s } else { self.crss = s }
                }
                None => self.found.push((false, format!("{sym} の typ が表に見つかりません"))),
            }
        }
        let g = first(&["gFS", "gfs", "Yfs"]);
        if let Some(v) = g.iter().find(|v| typ(v).is_some())
            && let (Some(vds), Some(id)) = (v.condition("VDS"), v.condition("ID"))
        {
            self.gfs = format!("{} {} {}", short(typ(v).unwrap_or_default()), short(vds), short(id));
            self.found.push((true, format!("gFS: {}", at(v))));
        }
    }
}

impl PartForm {
    /// "PDF の表から読む" and "モデルを作ってずれを見る" pressed (`--form-mos-pdf`, for screenshots).
    pub fn mos_from_pdf_and_fit(&mut self) {
        let sheet = &self.sheet;
        self.mos.fill_from_pdf(|s| sheet.spec_values(s));
        self.mos_fit();
    }

    fn mos_pch(&self) -> bool {
        self.glyph == "pmos"
    }

    fn mos_fit(&mut self) {
        let source = if self.datasheet.trim().is_empty() { "データシート".to_string() } else { self.datasheet.trim().to_string() };
        let name = if self.name.trim().is_empty() { "MOSFET".to_string() } else { self.name.trim().to_string() };
        self.mos.result = Some(self.mos.spec(&name, &source, self.mos_pch()).and_then(|s| fit::fit(&s)));
        self.mos.message = None;
    }

    /// Puts the fitted model into the simulation tab.
    fn mos_apply(&mut self) -> Result<String, String> {
        let Some(Ok(f)) = &self.mos.result else { return Err("先に「モデルを作ってずれを見る」を押してください".into()) };
        let pins = self.parse_pins()?;
        let find = |n: &str| pins.iter().find(|p| p.name.eq_ignore_ascii_case(n)).map(|p| p.number.clone());
        let (Some(d), Some(g), Some(s)) = (find("D"), find("G"), find("S")) else {
            return Err("ピン表に、D・G・S の名前のピンが要ります (「③ ピン」で直してください)".into());
        };
        self.sim = SimChoice::Model;
        self.sim_element = "M".into();
        self.sim_name = f.text.split_whitespace().nth(1).unwrap_or("M").to_string();
        self.sim_text = f.text.clone();
        self.sim_pins = format!("{d} {g} {s}");
        self.sim_limits = f.limits.join("\n");
        self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, f.source.clone()), self.sim_text.clone()));
        Ok("モデルを「② シミュレーション」に入れました。そちらの「試し読み」で確かめられます".into())
    }

    pub(super) fn mosfet_ui(&mut self, ui: &mut egui::Ui) {
        crate::widgets::section_heading(ui, "かんたん作成: MOSFET をデータシートの表からつくる");
        crate::icons::help_small(ui, "表の typ の値からモデルを作ります", "データシートの「電気的特性」の表の typ の値 (しきい値・オン抵抗・容量) から、同梱の MOSFET と同じ作り方 (レベル1 + 直列抵抗) でモデルを作り、表の値とのずれを並べます");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("mos_howto").show(ui, |ui| {
            for t in [
                "VGS(th) (しきい値): ドレインに小さい電流 (表の ID、たとえば 250µA) が流れ始めるゲート電圧。モデルの VTO はここから決まります。",
                "RDS(on) (オン抵抗): ゲート電圧ごとの抵抗。2〜3 行あると、チャネルの強さ KP と、配線やチップの抵抗 RD を分けて決められます。",
                "Ciss・Crss (容量): ゲートを動かすのに要る電荷の目安。CGS = Ciss − Crss、CGD = Crss にします (電圧による変化は扱いません)。",
                "gFS は比べるだけです。オン抵抗に合わせた 2乗則のモデルでは、ここまでは合わないことがよくあります。",
                "使うのは typ (標準) の列です。min・max は個体差の範囲で、回路は max でも動くように設計します。P チャネルは、マイナスの付いた値も大きさで入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、VGS(th)・RDS(on)・Ciss・Crss・gFS の typ と、その測定条件を探して欄に入れます。入れた値は表と見比べて確かめてください")).clicked()
            {
                let sheet = &self.sheet;
                self.mos.fill_from_pdf(|s| sheet.spec_values(s));
                self.mos.result = None;
            }
            if ui.button(i18n::tr("AO3400A の例を入れる")).on_hover_text(i18n::tr("同梱の AO3400A を作ったときの表の typ の値を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                let (a, b, c, d, e, f) = EXAMPLE;
                (self.mos.vth, self.mos.vth_id, self.mos.rds, self.mos.ciss, self.mos.crss, self.mos.gfs) = (a.into(), b.into(), c.into(), d.into(), e.into(), f.into());
                self.mos.found.clear();
                self.mos_fit();
            }
        });
        for (ok, line) in &self.mos.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        egui::Grid::new("mos_inputs").num_columns(2).show(ui, |ui| {
            ui.label(i18n::tr("VGS(th) [V]、その ID [A]")).on_hover_text(i18n::tr("しきい値の typ と、表の測定条件の ID"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.mos.vth).desired_width(70.0).hint_text("1.05"));
                ui.add(egui::TextEdit::singleline(&mut self.mos.vth_id).desired_width(70.0).hint_text("250u"));
            });
            ui.end_row();
            ui.label(i18n::tr("RDS(on): VGS ID 抵抗")).on_hover_text(i18n::tr("1 行に「ゲート電圧 [V] ドレイン電流 [A] オン抵抗 [Ω]」。m は 1000 分の 1 (18m = 18 mΩ)"));
            ui.add(egui::TextEdit::multiline(&mut self.mos.rds).desired_rows(3).desired_width(200.0).font(egui::TextStyle::Monospace).hint_text("10 5.7 18m\n4.5 5 19m"));
            ui.end_row();
            ui.label(i18n::tr("Ciss・Crss [F]")).on_hover_text(i18n::tr("入力容量と帰還容量の typ。p は 1 兆分の 1 (630p = 630 pF)"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.mos.ciss).desired_width(70.0).hint_text("630p"));
                ui.add(egui::TextEdit::singleline(&mut self.mos.crss).desired_width(70.0).hint_text("50p"));
            });
            ui.end_row();
            ui.label(i18n::tr("gFS: gFS VDS ID (空でも可)")).on_hover_text(i18n::tr("順方向伝達コンダクタンスと、その測定条件。比べるだけで、モデルは合わせません"));
            ui.add(egui::TextEdit::singleline(&mut self.mos.gfs).desired_width(200.0).hint_text("33 5 5.7"));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた値に合わせてモデルを作り、表の値とモデルの値を並べます")).clicked() {
                self.mos_fit();
            }
            if matches!(self.mos.result, Some(Ok(_))) && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと、その注記 (何から作り、何を扱わないか) を、シミュレーションのタブに入れます")).clicked() {
                self.mos.message = Some(match self.mos_apply() {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        match &self.mos.result {
            Some(Ok(f)) => {
                ui.monospace(&f.text);
                fit_table(ui, "mos_rows", f);
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::ERROR, e);
            }
            None => {}
        }
        if let Some((ok, m)) = &self.mos.message {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::ERROR }, m);
        }
    }
}

/// The readings next to the model's values, a gap over 5 % in orange, and the notes on why (the model's form).
pub(super) fn fit_table(ui: &mut egui::Ui, id: &str, f: &Fitted) {
    egui::Grid::new(id).num_columns(5).striped(true).show(ui, |ui| {
        for h in ["項目", "条件", "データシート", "モデル", "ずれ"] {
            ui.label(egui::RichText::new(i18n::tr(h)).strong());
        }
        ui.end_row();
        for r in &f.rows {
            let e = r.error_pct();
            let color = if r.compare_only { ui.visuals().weak_text_color() } else if e.abs() > 5.0 { crate::theme::WARN } else { ui.visuals().text_color() };
            ui.label(&r.item);
            ui.label(egui::RichText::new(&r.at).small());
            ui.label(format!("{}{}", part_lib::passive::eng(r.datasheet), r.unit));
            ui.label(format!("{}{}", part_lib::passive::eng(r.model), r.unit));
            let gap = ui.colored_label(color, format!("{e:+.1}%{}", if r.compare_only { i18n::tr(" (比べるだけ)") } else { "" }));
            if r.compare_only {
                gap.on_hover_text(i18n::tr("モデルを合わせていない値です。モデルの形 (2乗則) では合わないことが多く、参考に並べています"));
            }
            ui.end_row();
        }
    });
    if !f.notes.is_empty() {
        ui.label(egui::RichText::new(i18n::tr("ずれが大きい所 (モデルの形の限界。読み違いでなければ、そのままで使えます)")).strong());
        for n in &f.notes {
            ui.colored_label(crate::theme::WARN, egui::RichText::new(i18n::trf!("・{n}")).small());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datasheet::spec_table::SpecValue;

    #[test]
    fn the_example_fits_like_the_bundled_ao3400a() {
        let (a, b, c, d, e, g) = EXAMPLE;
        let m = MosInput { vth: a.into(), vth_id: b.into(), rds: c.into(), ciss: d.into(), crss: e.into(), gfs: g.into(), ..Default::default() };
        let f = fit::fit(&m.spec("AO3400A", "s", false).unwrap()).unwrap();
        assert_eq!(f.text, ".model AO3400A NMOS(VTO=1.048 KP=95.94 RD=0.01652 CGS=5.8e-10 CGD=5e-11)");
        assert!(f.rows.iter().any(|r| r.item == "gFS" && r.compare_only));
    }

    #[test]
    fn a_bad_line_says_which_and_how() {
        let m = MosInput { vth: "1".into(), vth_id: "250u".into(), rds: "10 5.7".into(), ciss: "1n".into(), crss: "1p".into(), ..Default::default() };
        assert!(m.spec("X", "s", false).unwrap_err().contains("1行目"));
        let m = MosInput { rds: "10 1 0.1".into(), vth: String::new(), ..m };
        assert!(m.spec("X", "s", false).unwrap_err().contains("VGS(th)"), "an empty threshold is named");
    }

    #[test]
    fn the_pdf_table_fills_the_boxes_and_says_what_is_missing() {
        let v = |symbol: &str, conditions: &str, typ: f64, unit: &str| SpecValue { page: 1, symbol: symbol.into(), conditions: conditions.into(), min: None, typ: Some(typ), max: None, unit: Some(unit.into()), rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 } };
        let table = [
            v("VGS(th)", "VDS=VGS ID=-250µA", -0.9, "V"),
            v("RDS(ON)", "VGS=-10V, ID=-4.0A", 41.0, "mΩ"),
            v("RDS(ON)", "TJ=125°C", 60.0, "mΩ"),
            v("RDS(ON)", "VGS=-4.5V, ID=-3.5A", 47.0, "mΩ"),
            v("Ciss", "VGS=0V, VDS=-15V, f=1MHz", 645.0, "pF"),
        ];
        let mut m = MosInput::default();
        m.fill_from_pdf(|s| table.iter().filter(|x| x.symbol.eq_ignore_ascii_case(s)).cloned().collect());
        assert_eq!((m.vth.as_str(), m.vth_id.as_str()), ("0.9", "250u"));
        assert_eq!(m.rds, "10 4 41m\n4.5 3.5 47m");
        assert_eq!(m.ciss, "645p");
        assert!(m.found.iter().any(|(ok, l)| !ok && l.contains("Crss")), "{:?}", m.found);
    }
}
