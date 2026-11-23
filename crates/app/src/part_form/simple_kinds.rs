//! "かんたん作成" for three two-terminal kinds whose model follows from a few table values (Q1): an NTC thermistor (R25 and
//! B: `fit::NtcSpec`), a crystal or ceramic resonator (frequency, ESR, C0, C1, CL: `fit::CrystalSpec`) and a TVS diode
//! (VRWM, VBR at IT, VC at IPP: `fit::TvsSpec`), and a relay with one coil and one contact (coil voltage and resistance,
//! operate and release voltages and times, contact resistance: `fit::RelaySpec`). The boxes hold the fields' text; `spec()` reads them, `part_lib::fit`
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
        let mut table = vec![];
        for l in self.table.lines().filter(|l| !l.trim().is_empty()) {
            let f: Vec<&str> = l.split_whitespace().collect();
            let [t, r] = f[..] else { return Err(format!("抵抗-温度表の行「{l}」は「温度 抵抗」の 2 つの数で書きます (例: 100 680)")) };
            table.push([num("温度", t, false)?, num("抵抗", r, false)?]);
        }
        // no B: the one that fits the table
        let (b, b_range) = if self.b.trim().is_empty() {
            fit::ntc_b_from_table(r25, &table).ok_or("B 定数を入れるか、抵抗-温度表に 25 ℃ 以外の点を入れてください (表から B を求めます)")?
        } else {
            (num("B 定数", &self.b, false)?, self.b_range.trim().to_string())
        };
        let description = format!("NTC サーミスタ {}Ω (25 ℃)・B 定数 {} K。温度が上がると抵抗が下がる", part_lib::passive::eng(r25), b.round());
        Ok(fit::NtcSpec { common: common(part, source, description), r25, b, b_range, table, dissipation: opt("熱放散定数", &self.dissipation, false)? })
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
/// The relay's boxes.
#[derive(Clone, Default)]
pub struct RelayInput {
    pub coil_v: String,
    pub coil_r: String,
    /// Volts, or a percentage of the rated voltage ("75%").
    pub operate_v: String,
    pub release_v: String,
    pub contact_r: String,
    pub operate_t: String,
    pub release_t: String,
    pub coil_l: String,
    pub make_only: bool,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

impl RelayInput {
    /// Songle's SRD-05VDC-SL-C, the usual relay of hobby modules, as its table writes it: 5 V, 70 Ω, pull-in 75 % max,
    /// drop-out 10 % min, operate 10 ms max, release 5 ms max, contact 100 mΩ max.
    pub fn example() -> RelayInput {
        RelayInput { coil_v: "5".into(), coil_r: "70".into(), operate_v: "75%".into(), release_v: "10%".into(), contact_r: "100m".into(), operate_t: "10m".into(), release_t: "5m".into(), ..Default::default() }
    }

    /// A voltage box: volts, or "75%" of the rated voltage.
    fn volts(label: &str, t: &str, rated: f64) -> Result<f64, String> {
        match t.trim().strip_suffix('%') {
            Some(p) => p.trim().parse::<f64>().map(|p| p / 100.0 * rated).map_err(|_| format!("{label}「{}」を数で入れてください (例: 3.75 か 75%)", t.trim())),
            None => num(label, t, false),
        }
    }

    pub fn spec(&self, part: &str, source: &str) -> Result<fit::RelaySpec, String> {
        let coil_v = num("コイルの定格電圧", &self.coil_v, false)?;
        let description = format!("リレー (コイル {} V、{})", self.coil_v.trim(), if self.make_only { "メーク接点 1 つ" } else { "切り替え接点 1 つ" });
        Ok(fit::RelaySpec {
            common: common(part, source, description),
            coil_v,
            coil_r: num("コイルの抵抗", &self.coil_r, false)?,
            operate_v: Self::volts("動作電圧", &self.operate_v, coil_v)?,
            release_v: Self::volts("復帰電圧", &self.release_v, coil_v)?,
            contact_r: opt("接点の抵抗", &self.contact_r, false)?,
            operate_t: opt("動作時間", &self.operate_t, false)?,
            release_t: opt("復帰時間", &self.release_t, false)?,
            coil_l: opt("コイルのインダクタンス", &self.coil_l, false)?,
            make_only: self.make_only,
        })
    }
}

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
/// A relay's coil ends and contacts (IEC numbers too: A1・A2, 11 / 14 / 12).
pub(super) const RELAY_ROLES: &[(&str, &[&str])] = &[
    ("コイル 1", &["A1", "COIL+", "COIL1", "COIL", "L1", "+"]),
    ("コイル 2", &["A2", "COIL-", "COIL2", "L2", "-"]),
    ("COM", &["COM", "C", "11"]),
    ("NO", &["NO", "14"]),
    ("NC", &["NC", "12"]),
];

/// Which of the three boxes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Simple {
    Ntc,
    Crystal,
    Tvs,
    Relay,
}

impl PartForm {
    /// Makes the model of a box from its fields.
    pub(super) fn simple_make(&mut self, k: Simple) {
        let (name, source) = (self.linear_name(match k { Simple::Ntc => "NTC", Simple::Crystal => "XTAL", Simple::Tvs => "TVS", Simple::Relay => "RELAY" }), self.linear_source());
        let r = match k {
            Simple::Ntc => self.ntc.spec(&name, &source).map(fit::Spec::Ntc),
            Simple::Crystal => self.xtal.spec(&name, &source).map(fit::Spec::Crystal),
            Simple::Tvs => self.tvs.spec(&name, &source).map(fit::Spec::Tvs),
            Simple::Relay => self.relay.spec(&name, &source).map(fit::Spec::Relay),
        }
        .and_then(|s| fit::fit(&s));
        let (result, message) = match k {
            Simple::Ntc => (&mut self.ntc.result, &mut self.ntc.message),
            Simple::Crystal => (&mut self.xtal.result, &mut self.xtal.message),
            Simple::Tvs => (&mut self.tvs.result, &mut self.tvs.message),
            Simple::Relay => (&mut self.relay.result, &mut self.relay.message),
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
                Simple::Relay => "SRD-05VDC-SL-C",
            }
            .into();
        }
        match k {
            Simple::Ntc => self.ntc = NtcInput::example(),
            Simple::Crystal => self.xtal = CrystalInput::example(),
            Simple::Tvs => self.tvs = TvsInput::example(),
            Simple::Relay => self.relay = RelayInput::example(),
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
            // a make contact has no NC
            Simple::Relay => (&self.relay.result, if self.relay.make_only { &RELAY_ROLES[..4] } else { RELAY_ROLES }),
        };
        let Some(Ok(f)) = result.clone() else { return Err("モデルができていません".into()) };
        self.linear_apply(&f, roles)
    }

    fn simple_buttons(&mut self, ui: &mut egui::Ui, k: Simple, id: &str) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作ってずれを見る")).on_hover_text(i18n::tr("入れた値からモデルを作り、表の値とモデルの値を並べます")).clicked() {
                self.simple_make(k);
            }
            let made = matches!(match k { Simple::Ntc => &self.ntc.result, Simple::Crystal => &self.xtal.result, Simple::Tvs => &self.tvs.result, Simple::Relay => &self.relay.result }, Some(Ok(_)));
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
                    Simple::Relay => self.relay.message = msg,
                }
            }
        });
        let (result, message) = match k {
            Simple::Ntc => (&self.ntc.result, &self.ntc.message),
            Simple::Crystal => (&self.xtal.result, &self.xtal.message),
            Simple::Tvs => (&self.tvs.result, &self.tvs.message),
            Simple::Relay => (&self.relay.result, &self.relay.message),
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
            Self::simple_row(ui, "B 定数 [K]・その範囲", "表の B 定数と、どの 2 つの温度で決めたか (B25/50・B25/85)。空にすると、下の抵抗-温度表に合う B を求めます (使う温度の範囲の点だけを入れると、その範囲で合います)", vec![(&mut n.b, "3950", 80.0), (&mut n.b_range, "B25/50", 80.0)]);
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
            ui.colored_label(if *ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(line).small());
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

    /// Puts the pins of a relay (coil A1・A2, COM, NO, NC) into 「③ ピン」, its pads a row of holes (an IC's two rows want an even
    /// count; the relay's own pad places are drawn in 「④ 部品の形」).
    pub fn relay_pins(&mut self) {
        self.fp_preset.clear();
        self.pkg = PkgKind::Row;
        self.pins_text = if self.relay.make_only { "1 A1 left\n2 A2 left\n3 COM right\n4 NO right" } else { "1 A1 left\n2 A2 left\n3 COM right\n4 NO right\n5 NC right" }.into();
        self.prefix = "K".into();
    }

    pub(super) fn relay_ui(&mut self, ui: &mut egui::Ui) {
        crate::icons::help_small(ui, "コイルの値と接点の値から", "データシートのコイルの表 (定格電圧・コイル抵抗・動作電圧 (Pick-up / Must operate)・復帰電圧 (Drop-out / Must release)) と、接点の表 (接触抵抗・動作時間・復帰時間) を入れると、コイルの電圧で接点が切り替わるモデルを作ります (同梱の TQ2-L2-5V と同じ作り)。動作電圧と復帰電圧の間では前の状態のまま (ヒステリシス) です");
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("SRD-05VDC-SL-C の例を入れる")).on_hover_text(i18n::tr("工作のリレーモジュールによく載っている 5 V のリレーの表の値を入れます")).clicked() {
                self.simple_example_and_make(Simple::Relay);
            }
            ui.checkbox(&mut self.relay.make_only, i18n::tr("メーク接点だけ (1a)")).on_hover_text(i18n::tr("NC の無い、COM と NO だけのリレー (1 Form A)。チェックしないと切り替え接点 (1c: COM・NO・NC)"));
            if ui.button(i18n::tr("ピンを A1・A2・COM・NO・NC にする")).on_hover_text(i18n::tr("「③ ピン」をリレーのピン名にし、部品の形を 1 列の穴にします (番号とパッドの位置は、データシートの図に合わせて「③ ピン」「④ 部品の形」で直してください)")).clicked() {
                self.relay_pins();
            }
        });
        let r = &mut self.relay;
        egui::Grid::new("relay_inputs").num_columns(2).show(ui, |ui| {
            Self::simple_row(ui, "コイル: 定格電圧 [V]・抵抗 [Ω]", "コイルの表の定格電圧 (Nominal voltage) とコイル抵抗 (Coil resistance)", vec![(&mut r.coil_v, "5", 60.0), (&mut r.coil_r, "70", 60.0)]);
            Self::simple_row(ui, "動作電圧・復帰電圧 [V か %]", "これを超えると接点が動く電圧 (Pick-up、表の最大) と、これより下がると戻る電圧 (Drop-out、表の最小)。75% のように定格の割合でも書けます", vec![(&mut r.operate_v, "75%", 60.0), (&mut r.release_v, "10%", 60.0)]);
            Self::simple_row(ui, "接点の抵抗 [Ω]", "接触抵抗 (Contact resistance、初期の最大)。空なら 100m", vec![(&mut r.contact_r, "100m", 60.0)]);
            Self::simple_row(ui, "動作時間・復帰時間 [s]", "Operate time と Release time (表の最大)。空なら 0 (すぐ動く)", vec![(&mut r.operate_t, "10m", 60.0), (&mut r.release_t, "5m", 60.0)]);
            Self::simple_row(ui, "コイルのインダクタンス [H]", "表にあれば (省略可)。入れると電流がゆっくり立ち上がり、切ったときに逆起電力が出ます", vec![(&mut r.coil_l, "", 60.0)]);
        });
        self.simple_buttons(ui, Simple::Relay, "relay_rows");
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
        for (k, name, pins) in [(Simple::Ntc, "NTC10K", "1 1\n2 2"), (Simple::Crystal, "XTAL12", "1 X1\n2 GND\n3 X2\n4 GND"), (Simple::Tvs, "TVS5", "1 K\n2 A"), (Simple::Relay, "RLY5", "1 A1\n2 A2\n3 COM\n4 NO\n5 NC")] {
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
                Simple::Relay => vec!["1", "2", "3", "4", "5"],
            };
            assert_eq!(sim.pins, want, "{k:?}");
        }
    }

    /// No B but a table: B is the one that fits it, its range said.
    #[test]
    fn an_ntc_without_b_takes_it_from_its_table() {
        let n = NtcInput { r25: "10k".into(), b: String::new(), table: "0 32.6k\n50 3.59k".into(), ..Default::default() };
        let s = n.spec("NTC", "test").unwrap();
        assert!((s.b - 3950.0).abs() < 60.0 && s.b_range.starts_with("B0/50"), "{} {}", s.b, s.b_range);
        assert!(NtcInput { table: String::new(), ..n }.spec("NTC", "test").unwrap_err().contains("抵抗-温度表"));
    }

    /// SRD-05VDC-SL-C's model: 71 mA at 5 V; 3 V (below the 3.75 V operate voltage) does not move it, 5 V does 10 ms later,
    /// 2 V (above the 0.5 V release voltage) keeps it, 0 V lets it go 5 ms later; the NC contact the other way round.
    #[test]
    fn a_relay_operates_and_releases_with_hysteresis_and_delay() {
        let mut f = PartForm { name: "SRD5".into(), ..Default::default() };
        f.simple_example_and_make(Simple::Relay);
        let Some(Ok(m)) = f.relay.result.clone() else { panic!("{:?}", f.relay.result) };
        assert!((m.rows[0].model - 71.43).abs() < 0.01, "{:?}", m.rows);
        let mut c = sim_core::Circuit::new();
        let n: Vec<usize> = ["coil", "com", "no", "nc"].iter().map(|x| c.node(x)).collect();
        sim_core::add_subckt_instance(&mut c, "K1", "SRD5", &m.text, &[n[0], 0, n[1], n[2], n[3]]).unwrap();
        // the coil: 3 V from 1 ms, 5 V from 10 ms, 2 V from 40 ms, 0 V from 60 ms
        let edge = 1e-5;
        let pwl = vec![(0.0, 0.0), (1e-3, 0.0), (1e-3 + edge, 3.0), (10e-3, 3.0), (10e-3 + edge, 5.0), (40e-3, 5.0), (40e-3 + edge, 2.0), (60e-3, 2.0), (60e-3 + edge, 0.0)];
        c.add(sim_core::Element::VSource { name: "Vcoil".into(), p: n[0], n: 0, wave: sim_core::Waveform::Pwl(pwl), ac: None });
        c.add(sim_core::Element::vdc("Vcom", n[1], 0, 1.0));
        c.add(sim_core::Element::Resistor { name: "Rno".into(), a: n[2], b: 0, r: 1e3 });
        c.add(sim_core::Element::Resistor { name: "Rnc".into(), a: n[3], b: 0, r: 1e3 });
        let t = sim_core::transient(&c, &sim_core::TranSettings { tstop: 80e-3, tstep: 1e-4, uic: false }).unwrap();
        let at = |node: &str, when: f64| {
            let k = t.time.iter().position(|x| *x >= when).unwrap();
            t.signal(&format!("V({node})")).unwrap().values[k]
        };
        for (when, on) in [(5e-3, false), (15e-3, false), (25e-3, true), (50e-3, true), (62e-3, true), (70e-3, false)] {
            let (closed, open) = if on { ("no", "nc") } else { ("nc", "no") };
            assert!(at(closed, when) > 0.99 && at(open, when) < 1e-3, "{} ms: {closed} {} V, {open} {} V", when * 1e3, at(closed, when), at(open, when));
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
