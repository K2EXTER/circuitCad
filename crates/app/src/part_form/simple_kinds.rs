//! "かんたん作成" for three two-terminal kinds whose model follows from a few table values (Q1): an NTC thermistor (R25 and
//! B: `fit::NtcSpec`), a crystal or ceramic resonator (frequency, ESR, C0, C1, CL: `fit::CrystalSpec`) and a TVS diode
//! (VRWM, VBR at IT, VC at IPP: `fit::TvsSpec`). The boxes hold the fields' text; `spec()` reads them, `part_lib::fit`
//! makes the model, and 「② シミュレーション」に入れる puts it in as a `.subckt` with the pins found by name.

use super::*;
use part_lib::fit::{self, Common, Fitted};

/// A number of a field: SPICE's suffixes (10k, 3p, 800u); for a frequency also "12M" / "12MHz" as mega (nobody means millihertz).
fn num(label: &str, t: &str, freq: bool) -> Result<f64, String> {
    let s = t.trim().trim_end_matches(['Ω', 'Ω']).trim();
    let s = if freq { s.trim_end_matches("Hz").trim_end_matches("hz") } else { s };
    let v = if freq && (s.ends_with('M') || s.ends_with("MEG") || s.ends_with("meg")) {
        s.trim_end_matches('M').trim_end_matches("MEG").trim_end_matches("meg").parse::<f64>().ok().map(|x| x * 1e6)
    } else {
        sim_core::parse_value(s.trim_end_matches(['V', 'v', 'A', 'F', 'K']))
    };
    v.filter(|v| v.is_finite()).ok_or_else(|| format!("{label}「{}」を数で入れてください (例: 10k、3p、12M)", t.trim()))
}

fn opt(label: &str, t: &str, freq: bool) -> Result<Option<f64>, String> {
    if t.trim().is_empty() { Ok(None) } else { num(label, t, freq).map(Some) }
}

fn common(name: &str, source: &str, description: String) -> Common {
    Common { part: name.into(), name: name.replace(['-', '.', ' '], "_"), source: source.into(), url: String::new(), description, read: String::new(), remarks: vec![] }
}

/// The NTC thermistor's boxes.
#[derive(Clone, Default)]
pub struct NtcInput {
    pub r25: String,
    pub b: String,
    pub b_range: String,
    /// "°C Ω" lines of the R-T table (optional).
    pub table: String,
    pub dissipation: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl NtcInput {
    pub fn example() -> NtcInput {
        NtcInput { r25: "10k".into(), b: "3950".into(), b_range: "B25/50".into(), table: "0 32.6k\n25 10k\n50 3.59k\n100 680".into(), dissipation: "2".into(), ..Default::default() }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::NtcSpec, String> {
        let r25 = num("25 °C の抵抗", &self.r25, false)?;
        let b = num("B 定数", &self.b, false)?;
        let mut table = vec![];
        for l in self.table.lines().filter(|l| !l.trim().is_empty()) {
            let f: Vec<&str> = l.split_whitespace().collect();
            let [t, r] = f[..] else { return Err(format!("抵抗-温度表の行「{l}」は「温度 抵抗」の 2 つの数で書きます (例: 100 680)")) };
            table.push([num("温度", t, false)?, num("抵抗", r, false)?]);
        }
        let description = format!("NTC サーミスタ {}Ω (25 ℃)・B 定数 {} K。温度が上がると抵抗が下がる", part_lib::passive::eng(r25), self.b.trim());
        Ok(fit::NtcSpec { common: common(part, source, description), r25, b, b_range: self.b_range.trim().into(), table, dissipation: opt("熱放散定数", &self.dissipation, false)? })
    }
}

/// The crystal's boxes.
#[derive(Clone, Default)]
pub struct CrystalInput {
    pub f: String,
    pub esr: String,
    pub c0: String,
    pub c1: String,
    pub cl: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl CrystalInput {
    /// The bundled XTAL_12MHz_3225's values.
    pub fn example() -> CrystalInput {
        CrystalInput { f: "12M".into(), esr: "40".into(), c0: "3p".into(), c1: "10f".into(), cl: String::new(), ..Default::default() }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::CrystalSpec, String> {
        let f = num("周波数", &self.f, true)?;
        let description = format!("{} の水晶振動子 (等価回路: 直列の R-L-C と並列容量 C0)", show_hz(f));
        Ok(fit::CrystalSpec { common: common(part, source, description), f, esr: num("ESR", &self.esr, false)?, c0: num("並列容量 C0", &self.c0, false)?, c1: opt("等価直列容量 C1", &self.c1, false)?, cl: opt("負荷容量 CL", &self.cl, false)? })
    }
}

fn show_hz(f: f64) -> String {
    if f >= 1e6 { format!("{}MHz", f / 1e6) } else { format!("{}kHz", f / 1e3) }
}

/// The TVS diode's boxes.
#[derive(Clone, Default)]
pub struct TvsInput {
    pub vrwm: String,
    pub ir: String,
    pub vbr: String,
    pub it: String,
    pub vc: String,
    pub ipp: String,
    pub cj: String,
    pub bidirectional: bool,
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl TvsInput {
    /// SMBJ5.0A as its maker's table writes it (Littelfuse).
    pub fn example() -> TvsInput {
        TvsInput { vrwm: "5".into(), ir: "800u".into(), vbr: "6.4".into(), it: "10m".into(), vc: "9.2".into(), ipp: "65.2".into(), cj: String::new(), bidirectional: false, ..Default::default() }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::TvsSpec, String> {
        if self.vc.trim().is_empty() || self.ipp.trim().is_empty() {
            return Err("クランプ電圧 VC とそのピーク電流 IPP が要ります。表に無いもの (ESD 保護ダイオードに多い) は、「クランプ電圧 対 ピークパルス電流」の図から 1 点読んで入れてください".into());
        }
        let vrwm = num("VRWM", &self.vrwm, false)?;
        let description = format!("TVS ダイオード ({}、VRWM {} V)。サージ・静電気を降伏でクランプする", if self.bidirectional { "双方向" } else { "単方向" }, self.vrwm.trim());
        Ok(fit::TvsSpec {
            common: common(part, source, description),
            vrwm,
            ir: num("IR", &self.ir, false)?,
            vbr: num("VBR", &self.vbr, false)?,
            it: num("IT", &self.it, false)?,
            vc: num("VC", &self.vc, false)?,
            ipp: num("IPP", &self.ipp, false)?,
            cj: opt("容量", &self.cj, false)?,
            bidirectional: self.bidirectional,
        })
    }

    /// The electrical characteristics table's typ (else min, for VBR; max for IR, VC) of each symbol, with its page and conditions.
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<datasheet::spec_table::SpecValue>) {
        self.found.clear();
        let si = |v: &datasheet::spec_table::SpecValue, x: f64| v.unit.as_deref().and_then(datasheet::spec_table::prefix).map_or(x, |k| x * k);
        let take = |syms: &[&str], pick: &dyn Fn(&datasheet::spec_table::SpecValue) -> Option<f64>| syms.iter().flat_map(|s| values(s)).find_map(|v| pick(&v).map(|x| (si(&v, x), v)));
        let typ_first = |v: &datasheet::spec_table::SpecValue| v.typ.or(v.min).or(v.max);
        let max_first = |v: &datasheet::spec_table::SpecValue| v.max.or(v.typ);
        let min_first = |v: &datasheet::spec_table::SpecValue| v.min.or(v.typ);
        let fields: [(&str, &[&str], &dyn Fn(&datasheet::spec_table::SpecValue) -> Option<f64>, usize); 6] = [
            ("VRWM", &["VRWM", "VRM", "VWM"], &max_first, 0),
            ("IR", &["IR", "IRM"], &max_first, 1),
            ("VBR", &["VBR", "V(BR)", "VBR(min)"], &min_first, 2),
            ("VC", &["VC", "VCL", "VCLAMP"], &max_first, 3),
            ("IPP", &["IPP", "IPPM"], &max_first, 4),
            ("容量", &["Cd", "CJ", "Cj", "C"], &typ_first, 5),
        ];
        for (label, syms, pick, k) in fields {
            match take(syms, pick) {
                Some((x, v)) => {
                    let text = part_lib::passive::eng(x);
                    *[&mut self.vrwm, &mut self.ir, &mut self.vbr, &mut self.vc, &mut self.ipp, &mut self.cj][k] = text.clone();
                    self.found.push((true, format!("{label} {text}: p.{}「{} {}」", v.page + 1, v.symbol, v.conditions.trim())));
                    // the test current is VBR's condition ("IT = 1 mA"), the peak current VC's when its own row is missing
                    if k == 2 && let Some(it) = condition_current(&v.conditions) {
                        self.it = part_lib::passive::eng(it);
                    }
                    if k == 3 && self.ipp.trim().is_empty() && let Some(i) = condition_current(&v.conditions) {
                        self.ipp = part_lib::passive::eng(i);
                    }
                }
                None => self.found.push((false, format!("{label} は表に見つかりませんでした"))),
            }
        }
        self.result = None;
    }
}

/// The current of a conditions text ("IR = 1 mA", "IPP = 5 A; tp = 8/20 µs"): the first number with an ampere unit.
fn condition_current(text: &str) -> Option<f64> {
    let words: Vec<&str> = text.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '=').filter(|w| !w.is_empty()).collect();
    for (k, w) in words.iter().enumerate() {
        let joined = match words.get(k + 1) {
            Some(u) if matches!(*u, "A" | "mA" | "µA" | "uA") => format!("{w}{u}"),
            _ => w.to_string(),
        };
        for (suffix, scale) in [("mA", 1e-3), ("µA", 1e-6), ("uA", 1e-6), ("A", 1.0)] {
            if let Some(n) = joined.strip_suffix(suffix).and_then(|n| n.parse::<f64>().ok()) {
                return Some(n * scale);
            }
        }
    }
    None
}

/// Pins by role, by name: a thermistor's two ends, a crystal's (X1/X2 of a four-pad one), a TVS's anode and cathode.
pub(super) const NTC_ROLES: &[(&str, &[&str])] = &[("端子 1", &["1", "A", "T1"]), ("端子 2", &["2", "B", "T2"])];
pub(super) const XTAL_ROLES: &[(&str, &[&str])] = &[("端子 1", &["X1", "XIN", "1", "A"]), ("端子 2", &["X2", "XOUT", "3", "2", "B"])];
pub(super) const TVS_ROLES: &[(&str, &[&str])] = &[("アノード", &["A", "ANODE", "1"]), ("カソード", &["K", "CATHODE", "2"])];

/// Which of the three boxes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Simple {
    Ntc,
    Crystal,
    Tvs,
}

impl PartForm {
    /// Makes the model of a box from its fields.
    pub(super) fn simple_make(&mut self, k: Simple) {
        let (name, source) = (self.linear_name(match k { Simple::Ntc => "NTC", Simple::Crystal => "XTAL", Simple::Tvs => "TVS" }), self.linear_source());
        let r = match k {
            Simple::Ntc => self.ntc.spec(&name, &source).map(fit::Spec::Ntc),
            Simple::Crystal => self.xtal.spec(&name, &source).map(fit::Spec::Crystal),
            Simple::Tvs => self.tvs.spec(&name, &source).map(fit::Spec::Tvs),
        }
        .and_then(|s| fit::fit(&s));
        let (result, message) = match k {
            Simple::Ntc => (&mut self.ntc.result, &mut self.ntc.message),
            Simple::Crystal => (&mut self.xtal.result, &mut self.xtal.message),
            Simple::Tvs => (&mut self.tvs.result, &mut self.tvs.message),
        };
        *result = Some(r);
        *message = None;
    }

    /// The example of a box put in and made (`--form-ntc-example` and the like, for screenshots and tests).
    pub fn simple_example_and_make(&mut self, k: Simple) {
        if self.name.trim().is_empty() {
            self.name = match k {
                Simple::Ntc => "NTC_10k_B3950_2",
                Simple::Crystal => "XTAL_12MHz_2",
                Simple::Tvs => "SMBJ5.0A",
            }
            .into();
        }
        match k {
            Simple::Ntc => self.ntc = NtcInput::example(),
            Simple::Crystal => self.xtal = CrystalInput::example(),
            Simple::Tvs => self.tvs = TvsInput::example(),
        }
        self.simple_make(k);
    }

    /// The TVS box read from the open PDF's table and made (`--form-tvs-pdf`); what was found goes to stderr.
    pub fn tvs_from_pdf_and_make(&mut self) {
        let sheet = &self.sheet;
        self.tvs.fill_from_pdf(|s| sheet.spec_values(s));
        for (ok, l) in &self.tvs.found {
            eprintln!("tvs {}: {l}", if *ok { "found" } else { "missing" });
        }
        self.simple_make(Simple::Tvs);
        if let Some(Ok(f)) = &self.tvs.result {
            for r in &f.rows {
                eprintln!("tvs row {} {}: table {} model {}", r.item, r.at, r.datasheet, r.model);
            }
        }
    }

    /// `simple_apply` for the command line.
    pub fn simple_apply_pub(&mut self, k: Simple) -> Result<String, String> {
        self.simple_apply(k)
    }

    /// Puts the made model into 「② シミュレーション」.
    pub(super) fn simple_apply(&mut self, k: Simple) -> Result<String, String> {
        let (result, roles) = match k {
            Simple::Ntc => (&self.ntc.result, NTC_ROLES),
            Simple::Crystal => (&self.xtal.result, XTAL_ROLES),
            Simple::Tvs => (&self.tvs.result, TVS_ROLES),
        };
        let Some(Ok(f)) = result.clone() else { return Err("モデルができていません".into()) };
        self.linear_apply(&f, roles)
    }

    fn simple_buttons(&mut self, ui: &mut egui::Ui, k: Simple, id: &str) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた値からモデルを作り、表の値とモデルの値を並べます")).clicked() {
                self.simple_make(k);
            }
            let made = matches!(match k { Simple::Ntc => &self.ntc.result, Simple::Crystal => &self.xtal.result, Simple::Tvs => &self.tvs.result }, Some(Ok(_)));
            if made && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します")).clicked() {
                let m = self.simple_apply(k);
                let msg = Some(match m {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
                match k {
                    Simple::Ntc => self.ntc.message = msg,
                    Simple::Crystal => self.xtal.message = msg,
                    Simple::Tvs => self.tvs.message = msg,
                }
            }
        });
        let (result, message) = match k {
            Simple::Ntc => (&self.ntc.result, &self.ntc.message),
            Simple::Crystal => (&self.xtal.result, &self.xtal.message),
            Simple::Tvs => (&self.tvs.result, &self.tvs.message),
        };
        if let Some(Ok(f)) = result {
            super::mosfet::fit_table(ui, id, f);
        }
        Self::result_box(ui, result, message);
    }

    /// One row of fields: a label with its explanation, then (box, hint, width) pairs.
    fn simple_row(ui: &mut egui::Ui, label: &str, tip: &str, fields: Vec<(&mut String, &str, f32)>) {
        ui.label(i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(tip));
        ui.horizontal(|ui| {
            for (f, hint, w) in fields {
                ui.add(egui::TextEdit::singleline(f).hint_text(hint).desired_width(w));
            }
        });
        ui.end_row();
    }

    pub(super) fn ntc_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "25 ℃ の抵抗と B 定数から", "データシートの表の「25 ℃ の抵抗 (R25)」と「B 定数 (B25/50 や B25/85)」を入れると、シミュレーションの温度で抵抗が変わるモデルを作ります (同梱の NTC_10k_B3950 と同じ形)。抵抗-温度表があれば何点か入れると、B の式とのずれが分かります");
        if ui.button(i18n::tr("10kΩ・B3950 の例を入れる")).on_hover_text(i18n::tr("同梱の NTC_10k_B3950 と同じ値を入れます")).clicked() {
            self.simple_example_and_make(Simple::Ntc);
        }
        let n = &mut self.ntc;
        egui::Grid::new("ntc_inputs").num_columns(2).show(ui, |ui| {
            Self::simple_row(ui, "25 ℃ の抵抗 [Ω]", "R25。10k、4.7k のように", vec![(&mut n.r25, "10k", 80.0)]);
            Self::simple_row(ui, "B 定数 [K]・その範囲", "表の B 定数と、どの 2 つの温度で決めたか (B25/50・B25/85)", vec![(&mut n.b, "3950", 80.0), (&mut n.b_range, "B25/50", 80.0)]);
            Self::simple_row(ui, "熱放散定数 [mW/K]", "自己発熱の目安 (省略可。モデルには入らず、注記に書きます)", vec![(&mut n.dissipation, "2", 80.0)]);
        });
        ui.label(egui::RichText::new(i18n::tr("抵抗-温度表 (省略可、1 行に「温度 抵抗」)")).small());
        ui.add(egui::TextEdit::multiline(&mut n.table).desired_rows(3).desired_width(200.0).hint_text("100 680"));
        self.simple_buttons(ui, Simple::Ntc, "ntc_rows");
    }

    pub(super) fn crystal_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "周波数・ESR・並列容量から", "データシートの表の公称周波数・等価直列抵抗 (ESR)・並列容量 (C0、シャント容量) を入れると、直列の R-L-C と C0 の等価回路を作ります (同梱の水晶と同じ形)。等価直列容量 C1 (モーショナル容量) が表にあれば入れ、無ければふつうの値を使います。負荷容量 CL を入れると、公称周波数は CL を付けたときの周波数として直列共振を少し下げます");
        if ui.button(i18n::tr("12 MHz の例を入れる")).on_hover_text(i18n::tr("同梱の XTAL_12MHz_3225 と同じ値を入れます")).clicked() {
            self.simple_example_and_make(Simple::Crystal);
        }
        let x = &mut self.xtal;
        egui::Grid::new("xtal_inputs").num_columns(2).show(ui, |ui| {
            Self::simple_row(ui, "公称周波数 [Hz]", "12M (= 12 MHz)、32.768k のように", vec![(&mut x.f, "12M", 80.0)]);
            Self::simple_row(ui, "ESR [Ω]", "等価直列抵抗 (表の最大値)", vec![(&mut x.esr, "40", 80.0)]);
            Self::simple_row(ui, "並列容量 C0 [F]", "シャント容量。3p のように", vec![(&mut x.c0, "3p", 80.0)]);
            Self::simple_row(ui, "等価直列容量 C1 [F]", "モーショナル容量。表に無ければ空のまま", vec![(&mut x.c1, "5f", 80.0)]);
            Self::simple_row(ui, "負荷容量 CL [F]", "公称周波数の条件の負荷容量 (省略可)", vec![(&mut x.cl, "18p", 80.0)]);
        });
        self.simple_buttons(ui, Simple::Crystal, "xtal_rows");
    }

    pub(super) fn tvs_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "表の VRWM・VBR・VC から", "データシートの電気的特性の表の、逆方向の動作電圧 (VRWM) とそこでの漏れ電流 (IR)、降伏電圧 (VBR) とその試験電流 (IT)、ピークパルス電流 (IPP) でのクランプ電圧 (VC) を入れると、降伏とクランプを合わせたモデルを作ります。双方向のものは 2 つを向かい合わせにします");
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc() && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの表から VRWM・IR・VBR (と IT)・VC (と IPP)・容量を探して欄に入れます。入れた値は表と見比べてください")).clicked() {
                let sheet = &self.sheet;
                self.tvs.fill_from_pdf(|s| sheet.spec_values(s));
            }
            if ui.button(i18n::tr("SMBJ5.0A の例を入れる")).on_hover_text(i18n::tr("SMBJ5.0A (5 V の単方向 TVS) の表の値を入れます")).clicked() {
                self.simple_example_and_make(Simple::Tvs);
            }
            ui.checkbox(&mut self.tvs.bidirectional, i18n::tr("双方向")).on_hover_text(i18n::tr("型番の末尾が CA のように、どちら向きの電圧もクランプするもの"));
        });
        for (ok, line) in &self.tvs.found {
            ui.colored_label(if *ok { crate::theme::OK } else { crate::theme::WARN }, egui::RichText::new(line).small());
        }
        let t = &mut self.tvs;
        egui::Grid::new("tvs_inputs").num_columns(2).show(ui, |ui| {
            Self::simple_row(ui, "VRWM [V]・IR [A]", "逆方向の動作電圧 (この電圧まではほぼ流れない) と、そこでの漏れ電流 (表の最大値)", vec![(&mut t.vrwm, "5", 60.0), (&mut t.ir, "800u", 60.0)]);
            Self::simple_row(ui, "VBR [V]・IT [A]", "降伏電圧 (表の最小値か typ) と、その試験電流", vec![(&mut t.vbr, "6.4", 60.0), (&mut t.it, "10m", 60.0)]);
            Self::simple_row(ui, "VC [V]・IPP [A]", "ピークパルス電流でのクランプ電圧 (表の最大値) と、その電流", vec![(&mut t.vc, "9.2", 60.0), (&mut t.ipp, "65.2", 60.0)]);
            Self::simple_row(ui, "容量 [F]", "0 V での容量 (省略可)", vec![(&mut t.cj, "3n", 60.0)]);
        });
        self.simple_buttons(ui, Simple::Tvs, "tvs_rows");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_with_their_units() {
        assert_eq!(num("f", "12M", true), Ok(12e6));
        assert_eq!(num("f", "32.768kHz", true), Ok(32768.0));
        assert_eq!(num("c", "3p", false), Ok(3e-12));
        assert_eq!(num("r", "10kΩ", false), Ok(10e3));
        assert!(num("r", "abc", false).unwrap_err().contains("abc"));
        assert_eq!(condition_current("IR = 1 mA"), Some(1e-3));
        assert_eq!(condition_current("IPP = 5 A; tp = 8/20 µs"), Some(5.0));
    }

    /// The examples make the bundled models' parts (NTC, crystal) and the TVS; each goes into 「② シミュレーション」 by its pins'
    /// names and the part saves.
    #[test]
    fn the_examples_make_parts() {
        for (k, name, pins) in [(Simple::Ntc, "NTC10K", "1 1\n2 2"), (Simple::Crystal, "XTAL12", "1 X1\n2 GND\n3 X2\n4 GND"), (Simple::Tvs, "TVS5", "1 K\n2 A")] {
            let mut f = PartForm { name: name.into(), pins_text: pins.into(), ..Default::default() };
            f.simple_example_and_make(k);
            f.simple_apply(k).unwrap_or_else(|e| panic!("{k:?}: {e}"));
            let def = f.to_def().unwrap_or_else(|e| panic!("{k:?}: {e}"));
            let sim = def.sim.unwrap();
            assert!(sim.text.starts_with(".subckt"), "{k:?}");
            // the pins in the model's order: a crystal's X1 and X2, a TVS's anode then cathode
            let want: Vec<&str> = match k {
                Simple::Ntc => vec!["1", "2"],
                Simple::Crystal => vec!["1", "3"],
                Simple::Tvs => vec!["2", "1"],
            };
            assert_eq!(sim.pins, want, "{k:?}");
        }
    }

    /// A TVS table as Nexperia writes one (PESD-like): the stand-off, leakage, breakdown with its test current, clamping with its
    /// pulse current, capacitance.
    #[test]
    fn a_tvs_table_fills_the_boxes() {
        use datasheet::spec_table::SpecValue;
        let v = |symbol: &str, cond: &str, min: Option<f64>, typ: Option<f64>, max: Option<f64>, unit: &str| SpecValue { page: 3, symbol: symbol.into(), conditions: cond.into(), min, typ, max, unit: Some(unit.into()), rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 } };
        let table = [
            v("VRWM", "", None, None, Some(5.0), "V"),
            v("IR", "VRWM = 5 V", None, Some(5.0), Some(100.0), "nA"),
            v("VBR", "IR = 1 mA", Some(5.5), Some(6.8), Some(9.5), "V"),
            v("VCL", "IPP = 3 A; tp = 8/20 µs", None, None, Some(14.0), "V"),
            v("Cd", "f = 1 MHz; VR = 0 V", None, Some(10.0), Some(14.0), "pF"),
        ];
        let mut t = TvsInput::default();
        t.fill_from_pdf(|s| table.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!((t.vrwm.as_str(), t.ir.as_str(), t.vbr.as_str(), t.it.as_str(), t.vc.as_str(), t.ipp.as_str(), t.cj.as_str()), ("5", "100n", "5.5", "1m", "14", "3", "10p"), "{:?}", t.found);
        assert!(t.found.iter().any(|(ok, l)| !ok && l.starts_with("IPP")));
        let s = t.spec("PESD5V0", "test").unwrap();
        assert!(fit::fit(&fit::Spec::Tvs(s)).is_ok());
    }
}
