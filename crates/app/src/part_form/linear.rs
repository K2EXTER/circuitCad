//! "かんたん作成" for an op amp and a linear regulator (M3): the data sheet's typ values go into the same templates
//! the bundled LM358 … MCP6002 and 78xx … LM317 are made with (`part_lib::fit::opamp`, `regulator`), and the model
//! goes to the simulation tab with its notes (what it is made from and what it leaves out). An op amp's values can
//! be offered from the open PDF's electrical characteristics table (`datasheet::spec_table`); a regulator's table
//! usually names its rows in words, so its numbers are typed (or clicked in the PDF).

use super::*;
use part_lib::fit::{self, Fitted};

/// The boxes of the op-amp section, as typed (SI prefixes: 700k, 20n, 2Meg).
#[derive(Clone, Default)]
pub struct OpampInput {
    pub aol: String,
    pub gbw: String,
    /// V/µs.
    pub sr: String,
    pub vos: String,
    pub vos_max: String,
    pub ib: String,
    pub ib_max: String,
    pub iq: String,
    pub iq_min: String,
    pub iq_max: String,
    pub rin: String,
    pub hu: String,
    pub hl: String,
    pub isc: String,
    pub en: String,
    pub ro: String,
    pub pm: String,
    pub found: Vec<(bool, String)>,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
}

/// The boxes of the regulator section.
#[derive(Clone, Default)]
pub struct RegulatorInput {
    pub adjustable: bool,
    pub vout: String,
    /// "min max" (optional).
    pub vrange: String,
    pub vin0: String,
    /// "ΔVout ΔVin" or "0.01%/V".
    pub line: String,
    /// "ΔVout ΔIout" or "0.1% ΔIout".
    pub load: String,
    pub vdo0: String,
    pub rdo: String,
    pub iq: String,
    pub iadj: String,
    pub iadj_max: String,
    pub min_load: String,
    /// "difference current" lines (1 or 2).
    pub ilim: String,
    pub rr: String,
    pub rr_f: String,
    pub rr_vout: String,
    /// "rms f_low f_high" or a density.
    pub noise: String,
    pub result: Option<Result<Fitted, String>>,
    pub message: Option<(bool, String)>,
    /// What "PDF の表から読む" put in or could not find, one line each.
    pub found: Vec<(bool, String)>,
    /// The output voltages the data sheet's part numbers tell, each with its first part number (`reg_fill::variants`).
    pub variants: Vec<(f64, String)>,
    /// Every part number with a voltage in its name (voltage, part number), and the package each is ordered in (part number,
    /// package): for making every version at once (`reg_fill::versions`).
    pub parts: Vec<(f64, String)>,
    pub packages: Vec<(String, String)>,
    /// The grades the tables print over their columns (onsemi's "MC7805B", "MC7805C", "MC7805AB": each its own version) and
    /// the temperature range each part number is ordered for (part number, (low, high) °C).
    pub grades: Vec<String>,
    pub temps: Vec<(String, (f64, f64))>,
}

/// A number as typed, with an SI prefix and an optional unit after it ("700kHz", "-20nA", "2Meg").
fn value(label: &str, t: &str) -> Result<f64, String> {
    let s = t.trim().trim_end_matches("/√Hz").trim_end_matches("/rtHz");
    sim_core::parse_value(s).filter(|v| v.is_finite()).ok_or_else(|| {
        if t.trim().is_empty() { format!("{label}が空です。データシートの typ を入れてください") } else { format!("{label}「{}」が数として読めません (例: 700k、20n、2Meg、0.3)", t.trim()) }
    })
}

/// An optional number: empty is None.
fn opt(label: &str, t: &str) -> Result<Option<f64>, String> {
    if t.trim().is_empty() { Ok(None) } else { value(label, t).map(Some) }
}

/// The numbers of a line ("0.5m 12.5").
fn numbers(label: &str, t: &str) -> Result<Vec<f64>, String> {
    t.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(|s| value(label, s)).collect()
}

/// A number for a box: SI prefix, 6 digits ("5.25Meg", "1.4m", "106.02").
pub(super) fn show(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let (k, pre) = [(1e6, "Meg"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p")].into_iter().find(|(k, _)| v.abs() >= *k * 0.9999).unwrap_or((1e-12, "p"));
    format!("{}{pre}", fit::num::g(v / k, 6))
}

/// Open-loop gain in dB from what the table prints: dB, or V/mV, V/µV, V/V.
fn gain_db(text: &str) -> Result<f64, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let (num, scale) = if let Some(n) = lower.strip_suffix("v/mv") {
        (n, Some(1e3))
    } else if let Some(n) = lower.strip_suffix("v/µv").or_else(|| lower.strip_suffix("v/uv")) {
        (n, Some(1e6))
    } else if let Some(n) = lower.strip_suffix("v/v") {
        (n, Some(1.0))
    } else {
        (lower.trim_end_matches("db"), None)
    };
    let v: f64 = num.trim().parse().map_err(|_| format!("開ループ利得「{t}」が読めません。dB の数 (100) か、表の書き方のまま (200 V/mV) で入れてください"))?;
    Ok(match scale {
        Some(k) => 20.0 * (v * k).log10(),
        None => v,
    })
}

impl OpampInput {
    pub fn spec(&self, part: &str, source: &str) -> Result<fit::OpampSpec, String> {
        let s = fit::OpampSpec {
            common: common(part, source),
            cite: source.into(),
            about: String::new(),
            aol_db: gain_db(&self.aol)?,
            gbw: value("GBW", &self.gbw)?,
            sr: value("スルーレート", &self.sr)? * 1e6,
            vos: value("オフセット (typ)", &self.vos)?,
            vos_max: value("オフセット (max)", &self.vos_max)?.abs(),
            ib: value("バイアス電流 (typ)", &self.ib)?,
            ib_max: opt("バイアス電流 (max)", &self.ib_max)?.map(f64::abs),
            iq: value("電源電流 (typ)", &self.iq)?,
            iq_min: opt("電源電流 (min)", &self.iq_min)?,
            iq_max: value("電源電流 (max)", &self.iq_max)?,
            rin: if self.rin.trim().is_empty() { "1e12".into() } else { self.rin.trim().to_string() },
            hu: value("出力の振れ幅 (上)", &self.hu)?,
            hl: value("出力の振れ幅 (下)", &self.hl)?,
            isc: value("短絡電流", &self.isc)?,
            en: value("雑音", &self.en)?,
            ro: if self.ro.trim().is_empty() { 50.0 } else { value("出力抵抗", &self.ro)? },
            pm: opt("位相余裕", &self.pm)?,
        };
        if value("入力抵抗", &s.rin).is_err() {
            return Err(format!("入力抵抗「{}」が読めません (例: 2Meg、1e12)", s.rin));
        }
        fit::opamp::check(&s)?;
        Ok(s)
    }

    /// The boxes filled from a spec (the example buttons).
    fn from_spec(s: &fit::OpampSpec) -> OpampInput {
        let o = |v: Option<f64>| v.map(show).unwrap_or_default();
        OpampInput {
            aol: fit::num::g(s.aol_db, 6),
            gbw: show(s.gbw),
            sr: fit::num::g(s.sr / 1e6, 6),
            vos: show(s.vos),
            vos_max: show(s.vos_max),
            ib: show(s.ib),
            ib_max: o(s.ib_max),
            iq: show(s.iq),
            iq_min: o(s.iq_min),
            iq_max: show(s.iq_max),
            rin: s.rin.clone(),
            hu: show(s.hu),
            hl: show(s.hl),
            isc: show(s.isc),
            en: show(s.en),
            ro: show(s.ro),
            pm: s.pm.map(|v| fit::num::g(v, 6)).unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Fills the boxes from the open PDF's table: the first row with a typ of each symbol that is not at a hot or cold
    /// temperature. A unit the table does not print is taken as the usual one (said in the line).
    pub fn fill_from_pdf(&mut self, values: impl Fn(&str) -> Vec<datasheet::spec_table::SpecValue>) {
        use datasheet::spec_table::SpecValue;
        self.found.clear();
        // rows at a temperature other than 25 °C ("TA = 125°C", "−40°C +125°C") are the hot and cold limits
        let room = |v: &SpecValue| temperatures(&v.conditions).iter().all(|t| (t - 25.0).abs() < 0.5);
        let rows = |symbols: &[&str]| -> Vec<SpecValue> { symbols.iter().flat_map(|s| values(s)).filter(room).collect() };
        let at = |v: &SpecValue| format!("p.{}「{} {}」", v.page + 1, v.symbol, v.conditions.trim());
        // the value in SI: from the printed unit, or (no unit read) the usual unit of that row
        let si = |v: &SpecValue, x: f64, usual: f64| -> (f64, bool) {
            match v.unit.as_deref().and_then(datasheet::spec_table::prefix) {
                Some(k) => (x * k, false),
                None => (x * usual, true),
            }
        };
        let mut put = |name: &str, symbols: &[&str], usual: f64, usual_unit: &str, field: &mut String, max_field: Option<&mut String>| {
            let r = rows(symbols);
            match r.iter().find(|v| v.typ.is_some()) {
                Some(v) => {
                    let (x, guessed) = si(v, v.typ.unwrap_or_default(), usual);
                    *field = show(x);
                    let mut line = format!("{name} {}: {}", field, at(v));
                    if guessed {
                        line += &format!(" (単位が読めないので {usual_unit} とした)");
                    }
                    if let (Some(m), Some(max)) = (max_field, v.max) {
                        *m = show(si(v, max, usual).0.abs());
                        line += &format!("、max {m}");
                    }
                    self.found.push((true, line));
                }
                None => self.found.push((false, format!("{name} の typ が表に見つかりません。自分で入れてください"))),
            }
        };
        put("オフセット", &["VOS", "VIO", "VOFFSET"], 1e-3, "mV", &mut self.vos, Some(&mut self.vos_max));
        put("バイアス電流", &["IB", "IIB"], 1e-9, "nA", &mut self.ib, Some(&mut self.ib_max));
        put("電源電流", &["ISY", "IQ", "ICC", "IDD", "ISUPPLY"], 1e-3, "mA", &mut self.iq, Some(&mut self.iq_max));
        put("GBW", &["GBW", "GBP", "GBWP", "B1", "UGBW"], 1e6, "MHz", &mut self.gbw, None);
        put("短絡電流", &["ISC", "ISC+", "ISC-", "ISC−"], 1e-3, "mA", &mut self.isc, None);
        // the slew rate in V/µs (the box's unit), the noise at 1 kHz in V/√Hz, the phase margin in degrees
        let sr = rows(&["SR", "SR+", "SR−", "SR-"]);
        match sr.iter().find(|v| v.typ.is_some()) {
            Some(v) => {
                self.sr = fit::num::g(v.typ.unwrap_or_default(), 6);
                self.found.push((true, format!("スルーレート {} V/µs: {}", self.sr, at(v))));
            }
            None => self.found.push((false, "スルーレートの typ が表に見つかりません".into())),
        }
        let en = rows(&["en", "eN", "Vn", "EN"]);
        let near_1k = |v: &&SpecValue| v.conditions.contains("1 kHz") || v.conditions.contains("1kHz");
        match en.iter().filter(|v| v.typ.is_some()).find(near_1k).or_else(|| en.iter().find(|v| v.typ.is_some() && !v.unit.as_deref().unwrap_or("").contains("p-p") && v.unit.as_deref() != Some("nV"))) {
            Some(v) => {
                let x = v.typ.unwrap_or_default();
                let unit = v.unit.as_deref().unwrap_or("");
                self.en = show(x * if unit.starts_with("µV") || unit.starts_with("uV") { 1e-6 } else { 1e-9 });
                self.found.push((true, format!("雑音 {}V/√Hz: {}{}", self.en, at(v), if unit.is_empty() || unit == "?" { " (単位が読めないので nV/√Hz とした)" } else { "" })));
            }
            None => self.found.push((false, "1 kHz の雑音 (nV/√Hz) が表に見つかりません".into())),
        }
        let pm = rows(&["ΦM", "φM", "PM", "θM", "ΦM"]);
        if let Some(v) = pm.iter().find(|v| v.typ.is_some()) {
            self.pm = fit::num::g(v.typ.unwrap_or_default(), 6);
            self.found.push((true, format!("位相余裕 {}°: {}", self.pm, at(v))));
        }
        let aol = rows(&["AVO", "AVOL", "AOL", "AVD", "AV", "Avol"]);
        match aol.iter().find(|v| v.typ.is_some() && v.unit.is_some()) {
            Some(v) => {
                let text = format!("{} {}", v.typ.unwrap_or_default(), v.unit.as_deref().unwrap_or(""));
                match gain_db(&text) {
                    Ok(db) => {
                        self.aol = fit::num::g(db, 4);
                        self.found.push((true, format!("開ループ利得 {} dB ({text}): {}", self.aol, at(v))));
                    }
                    Err(_) => self.found.push((false, format!("開ループ利得の単位「{}」が読めません ({})", v.unit.as_deref().unwrap_or(""), at(v)))),
                }
            }
            None => self.found.push((false, "開ループ利得の typ が表に見つかりません (dB か V/mV)".into())),
        }
        self.found.push((false, "出力の振れ幅 (電源からどこまで) は表の書き方がまちまちなので、自分で入れてください".into()));
        // values from several pages: a data sheet with one table per supply voltage gives each value from the first
        // table that has it, which may not be the same supply
        let mut pages: Vec<usize> = self
            .found
            .iter()
            .filter(|(ok, _)| *ok)
            .filter_map(|(_, l)| l.split_once(": p.")?.1.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
            .collect();
        pages.sort_unstable();
        pages.dedup();
        if let (Some(first), Some(last)) = (pages.first(), pages.last())
            && pages.len() > 1
        {
            self.found.push((false, format!("値が p.{first}〜p.{last} の別々の表から来ています。電源電圧ごとに表が分かれているデータシートでは、使う電源電圧の表の値にそろえてください")));
        }
    }
}

/// The temperatures written in a table row's conditions ("TA = 125°C", "−40°C +125°C", "25 °C").
pub(super) fn temperatures(conditions: &str) -> Vec<f64> {
    let t = conditions.replace('\u{2212}', "-").replace(" °C", "°C");
    t.match_indices("°C")
        .filter_map(|(i, _)| {
            let head = &t[..i];
            let start = head.char_indices().rev().take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '-' | '+')).last().map_or(i, |(j, _)| j);
            head[start..].parse::<f64>().ok()
        })
        .collect()
}

fn common(part: &str, source: &str) -> fit::Common {
    serde_json::from_value(serde_json::json!({"part": part, "name": part_lib::diode::model_name(part), "source": source})).expect("common")
}

/// Two-number fields: "a b", or "x%" style for the regulation lines.
fn regulation(label: &str, t: &str, vout: f64, per_volt: bool) -> Result<f64, String> {
    let t = t.trim();
    let how = if per_volt { "「0.5m 12.5」(出力の変化 [V] と 入力の変化 [V]) か「0.01%/V」" } else { "「1.3m 1」(出力の変化 [V] と 電流の変化 [A]) か「0.1% 1.5」" };
    if per_volt && let Some(p) = t.strip_suffix("%/V").or_else(|| t.strip_suffix("%/v")) {
        return p.trim().parse::<f64>().map(|p| p / 100.0 * vout).map_err(|_| format!("{label}は {how} で入れてください"));
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    match parts.as_slice() {
        [a, b] if a.ends_with('%') => {
            let p: f64 = a.trim_end_matches('%').parse().map_err(|_| format!("{label}は {how} で入れてください"))?;
            Ok(p / 100.0 * vout / value(label, b)?)
        }
        [a, b] => Ok(value(label, a)?.abs() / value(label, b)?.abs()),
        _ => Err(format!("{label}は {how} で入れてください")),
    }
}

impl RegulatorInput {
    pub fn spec(&self, part: &str, source: &str) -> Result<fit::RegulatorSpec, String> {
        let vout = value(if self.adjustable { "基準電圧" } else { "出力電圧" }, &self.vout)?;
        let vrange = match numbers("出力電圧の min・max", &self.vrange)?.as_slice() {
            [] => None,
            [lo, hi] => Some([*lo, *hi]),
            _ => return Err("出力電圧の min・max は「4.8 5.2」の 2 つの数で入れてください (空でも作れます)".into()),
        };
        let ilim = match numbers("電流制限", &self.ilim.replace('\n', " "))?.as_slice() {
            [d, i] => [fit::regulator::LimitAt { diff: *d, amps: *i }, fit::regulator::LimitAt { diff: d + 1.0, amps: *i }],
            [d1, i1, d2, i2] => [fit::regulator::LimitAt { diff: *d1, amps: *i1 }, fit::regulator::LimitAt { diff: *d2, amps: *i2 }],
            _ => return Err("電流制限は 1 行に「入出力の差 [V] 電流 [A]」、1 行か 2 行で入れてください (例: 10 2.2)".into()),
        };
        let noise = match numbers("雑音", &self.noise)?.as_slice() {
            [d] => fit::regulator::Noise::Density { density: *d },
            [rms, fl, fh] => fit::regulator::Noise::Rms { rms: *rms, f_low: *fl, f_high: *fh },
            _ => return Err("雑音は「50u 10 100k」(rms [V] と 帯域の下・上 [Hz]) か、密度 1 つ (3u = 3 µV/√Hz) で入れてください".into()),
        };
        let json = serde_json::json!({"part": part, "name": part_lib::diode::model_name(part), "source": source});
        let s = fit::RegulatorSpec {
            common: serde_json::from_value(json).map_err(|e| e.to_string())?,
            cite: source.into(),
            from_figures: String::new(),
            adjustable: self.adjustable,
            vout,
            vrange,
            vin0: opt("試験の入力電圧", &self.vin0)?,
            line: regulation("ライン・レギュレーション", &self.line, vout, true)?,
            rout: regulation("ロード・レギュレーション", &self.load, vout, false)?,
            vdo0: value("ドロップアウト", &self.vdo0)?,
            rdo: opt("ドロップアウトの電流による増え", &self.rdo)?.unwrap_or(0.0),
            iq: if self.adjustable { 0.0 } else { value("グラウンド電流", &self.iq)? },
            iadj: if self.adjustable { value("ADJ の電流 (typ)", &self.iadj)? } else { 0.0 },
            iadj_max: if self.adjustable { value("ADJ の電流 (max)", &self.iadj_max)? } else { 0.0 },
            min_load: if self.adjustable { opt("最小負荷電流", &self.min_load)? } else { None },
            ilim,
            rr: value("リップル除去", self.rr.trim().trim_end_matches("dB"))?,
            rr_f: value("リップル除去の周波数", &self.rr_f)?,
            rr_vout: if self.adjustable { opt("リップル除去を測った出力電圧", &self.rr_vout)? } else { None },
            noise,
        };
        fit::regulator::check_regulator(&s)?;
        Ok(s)
    }

    /// The LM7805 (fixed) and LM317 (adjustable) as the bundled ones were made (onsemi MC7800/D Rev. 29, TI SLVS044Z).
    fn example(adjustable: bool) -> RegulatorInput {
        let t = |s: &str| s.to_string();
        if adjustable {
            RegulatorInput {
                adjustable,
                vout: t("1.25"),
                vrange: t("1.2 1.3"),
                vin0: String::new(),
                line: t("0.01%/V"),
                load: t("0.1% 1.49"),
                vdo0: t("1.6"),
                rdo: t("0.4"),
                iadj: t("50u"),
                iadj_max: t("100u"),
                min_load: t("3.5m"),
                ilim: t("15 2.2\n40 0.4"),
                rr: t("65"),
                rr_f: t("120"),
                rr_vout: t("10"),
                noise: t("37.5u 10 10k"),
                ..Default::default()
            }
        } else {
            RegulatorInput {
                vout: t("5"),
                vrange: t("4.8 5.2"),
                vin0: t("10"),
                line: t("0.5m 12.5"),
                load: t("1.3m 0.995"),
                vdo0: t("1.6"),
                rdo: t("0.4"),
                iq: t("3.2m"),
                ilim: t("10 2.2\n35 0.6"),
                rr: t("83"),
                rr_f: t("120"),
                noise: t("50u 10 100k"),
                ..Default::default()
            }
        }
    }
}

/// The bundled LM358's readings (`crates/part-lib/fits/LM358_SOIC-8.fit.json`): the op-amp example.
fn lm358() -> fit::OpampSpec {
    match serde_json::from_str::<fit::Spec>(include_str!("../../../part-lib/fits/LM358_SOIC-8.fit.json")) {
        Ok(fit::Spec::Opamp(s)) => s,
        _ => unreachable!("the bundled LM358 readings are an op amp"),
    }
}

impl PartForm {
    pub(super) fn linear_source(&self) -> String {
        if self.datasheet.trim().is_empty() { "データシート".to_string() } else { self.datasheet.trim().to_string() }
    }

    pub(super) fn linear_name(&self, fallback: &str) -> String {
        if self.name.trim().is_empty() { fallback.to_string() } else { self.name.trim().to_string() }
    }

    fn opamp_make(&mut self) {
        let (name, source) = (self.linear_name("OPAMP"), self.linear_source());
        self.opamp.result = Some(self.opamp.spec(&name, &source).and_then(|s| fit::fit(&fit::Spec::Opamp(s))));
        self.opamp.message = None;
    }

    /// The regulator's pins by role, found by name (IN・GND or ADJ・OUT).
    pub(super) fn reg_roles(&self) -> &'static [(&'static str, &'static [&'static str])] {
        if self.reg.adjustable {
            &[("入力", &["IN", "VIN", "INPUT"]), ("ADJ", &["ADJ", "ADJUST"]), ("出力", &["OUT", "VOUT", "OUTPUT"])]
        } else {
            &[("入力", &["IN", "VIN", "INPUT", "VI"]), ("GND", &["GND", "COM", "COMMON", "GROUND"]), ("出力", &["OUT", "VOUT", "OUTPUT", "VO"])]
        }
    }

    pub(super) fn regulator_make(&mut self) {
        let (name, source) = (self.linear_name("REG"), self.linear_source());
        self.reg.result = Some(self.reg.spec(&name, &source).and_then(|s| fit::fit(&fit::Spec::Regulator(s))));
        self.reg.message = None;
    }

    /// The op-amp example (LM358) put in the boxes and made (`--form-opamp-example`, for screenshots).
    pub fn opamp_example_and_make(&mut self) {
        self.opamp = OpampInput::from_spec(&lm358());
        self.opamp_make();
    }

    /// The op-amp section read from the PDF's table and made (`--form-opamp-pdf`, for screenshots).
    pub fn opamp_from_pdf_and_make(&mut self) {
        let sheet = &self.sheet;
        self.opamp.fill_from_pdf(|s| sheet.spec_values(s));
        self.opamp_make();
    }

    /// The regulator example (fixed LM7805 or adjustable LM317) made (`--form-regulator-example [adj]`).
    pub fn regulator_example_and_make(&mut self, adjustable: bool) {
        self.reg = RegulatorInput::example(adjustable);
        self.regulator_make();
    }

    /// Puts a template's model into the simulation tab, its pins found by name.
    pub(super) fn linear_apply(&mut self, f: &Fitted, roles: &[(&str, &[&str])]) -> Result<String, String> {
        let pins = self.parse_pins()?;
        let mut order = vec![];
        for (role, names) in roles {
            let p = pins.iter().find(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n))).ok_or_else(|| {
                format!("ピン表に「{role}」のピン ({}) が要ります。「③ ピン」でピンの名前を直してください", names.join("・"))
            })?;
            order.push(p.number.clone());
        }
        self.sim = SimChoice::Subckt;
        self.sim_name = f.text.split_whitespace().nth(1).unwrap_or("X").to_string();
        self.sim_text = f.text.clone();
        self.sim_pins = order.join(" ");
        self.sim_limits = f.limits.join("\n");
        self.sim_origin = Some((sch_core::ModelOrigin::new(sch_core::OriginKind::Equivalent, f.source.clone()), self.sim_text.clone()));
        if self.description.trim().is_empty() && !f.description.is_empty() {
            self.description = f.description.clone();
        }
        Ok("モデルを「② シミュレーション」に入れました。そちらの「試し読み」で確かめられます".into())
    }

    pub(super) fn result_box(ui: &mut egui::Ui, result: &Option<Result<Fitted, String>>, message: &Option<(bool, String)>) {
        match result {
            Some(Ok(f)) => {
                egui::CollapsingHeader::new(i18n::tr("できたモデル (SPICE の文)")).id_salt(("lin_text", f.part.as_str())).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.monospace(&f.text);
                    });
                });
                ui.label(egui::RichText::new(i18n::tr("モデルの注記 (何から作り、何を扱わないか)")).strong());
                for l in &f.limits {
                    ui.label(egui::RichText::new(i18n::trf!("・{l}")).small());
                }
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::error(), e);
            }
            None => {}
        }
        if let Some((ok, m)) = message {
            ui.colored_label(if *ok { crate::theme::ok() } else { crate::theme::error() }, m);
        }
    }

    pub(super) fn opamp_ui(&mut self, ui: &mut egui::Ui) {
        crate::widgets::section_heading(ui, "かんたん作成: オペアンプをデータシートの表からつくる");
        crate::icons::help_small(ui, "表の typ の値からモデルを作ります", "データシートの「電気的特性」の表の typ の値を、同梱のオペアンプ (LM358・TL072 など) と同じひな形に入れてモデルを作ります。データシートとどれだけ合うかは、同梱の部品で確かめてあります (docs/models/opamps.md)");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("opamp_howto").show(ui, |ui| {
            for t in [
                "開ループ利得 (AVO・AOL): 帰還をかけないときの増幅の大きさ。表が V/mV なら「200 V/mV」のまま入れると dB に直します。",
                "GBW (利得帯域幅積): 増幅が 1 倍に落ちる周波数。10 倍のアンプなら GBW の 1/10 くらいまで使えます。",
                "スルーレート (SR): 出力が 1 µs に動ける電圧。大きな振幅の速い信号は、これで形がくずれます。",
                "オフセット・バイアス電流: 入力に乗る小さな誤差。typ のほかに max を入れると、ばらつきの解析でその幅まで動かします。",
                "出力の振れ幅: 出力が電源からどこまで近づけるか (V)。レール・ツー・レールなら 10m くらい、LM358 の上側は 1.4 V。",
                "電源電流・雑音・短絡電流は、表の「1 回路あたり」の値で。2 回路入りの部品は、表が 2 回路分なら半分にします。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、オフセット・バイアス電流・電源電流・GBW・スルーレート・雑音・位相余裕・開ループ利得の typ (と max) を探して欄に入れます。25 °C の行を使います。入れた値は表と見比べて確かめてください")).clicked()
            {
                let sheet = &self.sheet;
                self.opamp.fill_from_pdf(|s| sheet.spec_values(s));
                self.opamp.result = None;
            }
            if ui.button(i18n::tr("LM358 の例を入れる")).on_hover_text(i18n::tr("同梱の LM358 を作ったときの値 (TI のデータシートの typ) を入れます。自分のデータシートの値に書き換えて使います")).clicked() {
                self.opamp_example_and_make();
            }
        });
        for (ok, line) in &self.opamp.found {
            ui.colored_label(if *ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(line).small());
        }
        let o = &mut self.opamp;
        egui::Grid::new("opamp_inputs").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, label: &str, tip: &str, fields: Vec<(&mut String, &str)>| {
                ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                ui.horizontal(|ui| {
                    for (f, hint) in fields {
                        ui.add(egui::TextEdit::singleline(f).desired_width(70.0).hint_text(hint));
                    }
                });
                ui.end_row();
            };
            row(ui, "開ループ利得 [dB]", "dB の数、または表の書き方のまま (200 V/mV)", vec![(&mut o.aol, "100")]);
            row(ui, "GBW [Hz]", "利得帯域幅積。k は 1000 倍、Meg は 100 万倍 (700k = 700 kHz)", vec![(&mut o.gbw, "700k")]);
            row(ui, "スルーレート [V/µs]", "出力が 1 µs に動ける電圧", vec![(&mut o.sr, "0.3")]);
            row(ui, "オフセット typ・max [V]", "入力オフセット電圧。m は 1000 分の 1 (3m = 3 mV)。typ が無ければ 0", vec![(&mut o.vos, "3m"), (&mut o.vos_max, "7m")]);
            row(ui, "バイアス電流 typ・max [A]", "入力に流れ込む電流。ピンから出ていく (PNP 入力の LM358 など) ならマイナス。max は空でも可", vec![(&mut o.ib, "-20n"), (&mut o.ib_max, "250n")]);
            row(ui, "電源電流 typ・min・max [A]", "1 回路あたりの電源電流。min は空でも可", vec![(&mut o.iq, "350u"), (&mut o.iq_min, ""), (&mut o.iq_max, "600u")]);
            row(ui, "出力の振れ幅 上・下 [V]", "出力が正の電源から・負の電源から、どこまで近づけるか", vec![(&mut o.hu, "1.4"), (&mut o.hl, "5m")]);
            row(ui, "短絡電流 [A]", "出力を短絡したときに流れる電流", vec![(&mut o.isc, "40m")]);
            row(ui, "雑音 (1 kHz) [V/√Hz]", "入力換算の電圧雑音密度。n は 10 億分の 1 (40n = 40 nV/√Hz)", vec![(&mut o.en, "40n")]);
            row(ui, "入力抵抗・出力抵抗 [Ω]", "空なら 1e12 と 50 Ω。表に無いことが多い値です", vec![(&mut o.rin, "2Meg"), (&mut o.ro, "50")]);
            row(ui, "位相余裕 [°]", "表にあれば。入れると、その位相余裕になる 2 つ目の極を足します (空でも可)", vec![(&mut o.pm, "")]);
        });
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作る")).on_hover_text(i18n::tr("入れた値をひな形に入れてモデルを作ります。足りない値・おかしな値は、何を直すかを出します")).clicked() {
                self.opamp_make();
            }
            if let Some(Ok(f)) = self.opamp.result.clone()
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (IN+・IN-・OUT・V+・V-)")).clicked()
            {
                let roles: &[(&str, &[&str])] = &[("+入力", &["IN+", "+IN", "INP", "NONINV"]), ("−入力", &["IN-", "-IN", "INN", "INV"]), ("出力", &["OUT", "VOUT", "OUTPUT"]), ("正の電源", &["V+", "VCC", "VDD", "VS+", "+VS"]), ("負の電源", &["V-", "VEE", "VSS", "VS-", "-VS", "GND"])];
                self.opamp.message = Some(match self.linear_apply(&f, roles) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        Self::result_box(ui, &self.opamp.result, &self.opamp.message);
    }

    pub(super) fn regulator_ui(&mut self, ui: &mut egui::Ui) {
        crate::widgets::section_heading(ui, "かんたん作成: 3 端子レギュレータをデータシートの表からつくる");
        crate::icons::help_small(ui, "表の typ の値からモデルを作ります", "データシートの表の typ の値を、同梱のレギュレータ (78xx・AMS1117・LM317 など) と同じひな形に入れてモデルを作ります。表の行の名前は会社ごとにまちまちなので、値はデータシートを見ながら入れます (PDF の数字はクリックで拾えます)");
        egui::CollapsingHeader::new(i18n::tr("考え方: どの値を使う? (はじめての人はここから)")).id_salt("reg_howto").show(ui, |ui| {
            for t in [
                "ライン・レギュレーション: 入力電圧を変えたときの出力の変化。表が「入力 7〜25 V で 0.5 mV」なら「0.5m 18」、「0.01 %/V」ならそのまま。",
                "ロード・レギュレーション: 出力電流を変えたときの出力の下がり。「5 mA〜1 A で 1.3 mV」なら「1.3m 0.995」、「0.1 %」なら「0.1% 1.5」(電流の変化と)。",
                "ドロップアウト: 入力が出力よりこれだけ高くないと、出力が下がり始める電圧。電流で増えるなら、その傾き [Ω] も (1 A で 0.4 V 増えるなら 0.4)。",
                "電流制限: 「入出力の差 [V] 電流 [A]」を 1〜2 行。差が大きいと制限が小さくなる部品 (78xx) は 2 行にします。",
                "可変のもの (LM317 など) は「可変 (ADJ ピン)」に入れ、出力電圧の欄に基準電圧 (1.25 V) を入れます。",
            ] {
                ui.label(egui::RichText::new(i18n::trf!("・{t}")).small());
            }
        });
        ui.horizontal_wrapped(|ui| {
            if self.sheet.has_doc()
                && ui.button(i18n::tr("PDF の表から読む")).on_hover_text(i18n::tr("開いているデータシートの「電気的特性」の表から、出力電圧 (可変なら基準電圧)・ライン / ロード・レギュレーション (条件の範囲と)・ドロップアウト・グラウンド電流 (可変なら ADJ の電流と最小負荷)・電流制限・リップル除去・雑音の typ を探して欄に入れます。25 °C の行を使い、部品名の電圧 (7805 なら 5 V) のページを先に見ます。入れた値は表と見比べて確かめてください")).clicked()
            {
                let (sheet, name) = (&self.sheet, self.name.clone());
                self.reg.fill_from_pdf(&name, |s| sheet.spec_values_for(s, &name));
                self.reg.result = None;
                self.reg_find_variants();
            }
            if ui.button(i18n::tr("LM7805 の例を入れる")).on_hover_text(i18n::tr("同梱の LM7805 を作ったときの値 (onsemi のデータシートの typ) を入れます")).clicked() {
                self.regulator_example_and_make(false);
            }
            if ui.button(i18n::tr("LM317 の例を入れる")).on_hover_text(i18n::tr("同梱の LM317 (可変) を作ったときの値 (TI のデータシートの typ) を入れます")).clicked() {
                self.regulator_example_and_make(true);
            }
            ui.checkbox(&mut self.reg.adjustable, i18n::tr("可変 (ADJ ピン)")).on_hover_text(i18n::tr("LM317 のように、抵抗 2 本で出力電圧を決める部品。ピンは IN・ADJ・OUT"));
        });
        for (ok, line) in &self.reg.found {
            ui.colored_label(if *ok { crate::theme::ok() } else { crate::theme::warn() }, egui::RichText::new(line).small());
        }
        // one data sheet for several output voltages (LP5907, MC7800, AMS1117): the person picks one
        if !self.reg.adjustable && !self.reg.variants.is_empty() {
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(i18n::tr("電圧違い (注文情報):")).small())
                    .on_hover_text(i18n::tr("データシートの型番から読んだ出力電圧です。選ぶと、その型番を部品名にして表を読み直します (電圧ごとにページの分かれた表は、そのページの値を使います)"));
                for (v, part) in &self.reg.variants {
                    let on = super::linear::show(*v) == self.reg.vout.trim();
                    if ui.selectable_label(on, format!("{} V", super::linear::show(*v))).on_hover_text(i18n::trf!("{part} (出力 {} V)", super::linear::show(*v))).clicked() {
                        pick = Some((*v, part.clone()));
                    }
                }
            });
            if let Some((v, part)) = pick {
                self.reg_choose_variant(v, &part);
            }
            let n = super::reg_fill::versions(&self.reg.variants, &self.reg.parts, &self.reg.packages, &self.reg.grades).len();
            if ui.button(i18n::trf!("電圧違いをまとめて作る ({n} 通り)")).on_hover_text(i18n::tr("注文情報の型番の電圧ごと (パッケージ違いがあればそのパッケージごと) に、表をその型番で読み直してモデルを作り、ピンと「④ 部品の形」はこのフォームのまま (パッケージの分かる型番は組み込みの形) で、部品を全部ライブラリに保存します。組み込みの形が無いパッケージは作りません。先にピン表を確かめてください")).clicked() {
                self.reg_make_all();
            }
        }
        {
            use super::graph_reader::Dest;
            self.read_all_button(ui, &[Dest::RegDropout, Dest::RegRipple, Dest::RegNoise]);
            // one graph by a click, for a field the read-all could not read
            if self.sheet.has_doc() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(i18n::tr("グラフから読む:")).small());
                    for (d, label, tip) in [
                        (Dest::RegDropout, "ドロップアウト", "「Dropout Voltage vs Load Current」の図で曲線をクリックします。点に直線を当てて、ドロップアウトと電流による増えを入れます"),
                        (Dest::RegRipple, "リップル除去", "「Ripple Rejection vs Frequency」の図で曲線をクリックします。欄の周波数 (空なら 120 Hz) の値を、表に無いときに入れます"),
                        (Dest::RegNoise, "雑音", "「Noise Spectral Density」の図で曲線をクリックします。1 kHz の密度を、表に無いときに入れます"),
                    ] {
                        if ui.small_button(i18n::tr(label)).on_hover_text(i18n::tr(tip)).clicked() {
                            self.sheet.start_graph(Some(d));
                        }
                    }
                });
            }
        }
        let r = &mut self.reg;
        let adj = r.adjustable;
        egui::Grid::new("reg_inputs").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, label: &str, tip: &str, fields: Vec<(&mut String, &str, f32)>| {
                ui.label(i18n::tr(label)).on_hover_text(i18n::tr(tip));
                ui.horizontal(|ui| {
                    for (f, hint, w) in fields {
                        ui.add(egui::TextEdit::singleline(f).desired_width(w).hint_text(hint));
                    }
                });
                ui.end_row();
            };
            if adj {
                row(ui, "基準電圧 typ [V]・min max", "OUT と ADJ の間の電圧 (1.25 V)。min・max は部品ごとの違い (空でも可)", vec![(&mut r.vout, "1.25", 60.0), (&mut r.vrange, "1.2 1.3", 90.0)]);
            } else {
                row(ui, "出力電圧 typ [V]・min max", "min・max は部品ごとの違い (空でも可)", vec![(&mut r.vout, "5", 60.0), (&mut r.vrange, "4.8 5.2", 90.0)]);
                row(ui, "表の測定の入力電圧 [V]", "表の値を測ったときの入力電圧 (空なら 6.25 V)", vec![(&mut r.vin0, "10", 60.0)]);
            }
            row(ui, "ライン・レギュレーション", "「出力の変化 入力の変化」(0.5m 12.5) か「0.01%/V」", vec![(&mut r.line, "0.5m 12.5", 160.0)]);
            row(ui, "ロード・レギュレーション", "「出力の変化 電流の変化」(1.3m 1) か「0.1% 1.5」", vec![(&mut r.load, "1.3m 1", 160.0)]);
            row(ui, "ドロップアウト [V]・電流による増え [Ω]", "小さい電流でのドロップアウトと、1 A あたりの増え (空なら 0)", vec![(&mut r.vdo0, "1.6", 60.0), (&mut r.rdo, "0.4", 60.0)]);
            if adj {
                row(ui, "ADJ の電流 typ・max [A]・最小負荷 [A]", "ADJ ピンから出る電流と、出力に要る最小の電流", vec![(&mut r.iadj, "50u", 60.0), (&mut r.iadj_max, "100u", 60.0), (&mut r.min_load, "3.5m", 60.0)]);
            } else {
                row(ui, "グラウンド電流 [A]", "GND ピンに流れる電流 (自己消費)", vec![(&mut r.iq, "3.2m", 60.0)]);
            }
            ui.label(i18n::tr("電流制限: 差 [V] 電流 [A]")).on_hover_text(i18n::tr("1 行に「入出力の差 電流」。1 行か 2 行 (差で変わる部品)"));
            ui.add(egui::TextEdit::multiline(&mut r.ilim).desired_rows(2).desired_width(160.0).font(egui::TextStyle::Monospace).hint_text("10 2.2\n35 0.6"));
            ui.end_row();
            row(ui, "リップル除去 [dB]・周波数 [Hz]", "入力のリップルが出力でどれだけ小さくなるか", vec![(&mut r.rr, "83", 60.0), (&mut r.rr_f, "120", 60.0)]);
            if adj {
                row(ui, "リップル除去を測った出力電圧 [V]", "表の条件の出力電圧 (LM317 は 10 V)", vec![(&mut r.rr_vout, "10", 60.0)]);
            }
            row(ui, "出力雑音", "「rms [V] 帯域の下 上 [Hz]」(50u 10 100k) か、密度 1 つ (3u = 3 µV/√Hz)", vec![(&mut r.noise, "50u 10 100k", 160.0)]);
        });
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("モデルを作る")).on_hover_text(i18n::tr("入れた値をひな形に入れてモデルを作ります")).clicked() {
                self.regulator_make();
            }
            if let Some(Ok(f)) = self.reg.result.clone()
                && ui.button(i18n::tr("「② シミュレーション」に入れる")).on_hover_text(i18n::tr("作ったモデルと注記を、シミュレーションのタブに入れます。ピンは名前で探します (IN・GND か ADJ・OUT)")).clicked()
            {
                self.reg.message = Some(match self.linear_apply(&f, self.reg_roles()) {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        Self::result_box(ui, &self.reg.result, &self.reg.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(part: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../part-lib/examples").join(format!("{part}.part.json"));
        let d: part_lib::PartDef = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        d.sim.unwrap().text
    }

    fn text(f: Result<Fitted, String>) -> String {
        f.unwrap().text
    }

    #[test]
    fn the_examples_give_the_bundled_models() {
        // the boxes as the example button fills them (numbers with SI prefixes) give the bundled model again
        let o = OpampInput::from_spec(&lm358());
        let s = o.spec("LM358", "s").unwrap();
        assert_eq!(text(fit::fit(&fit::Spec::Opamp(s))), bundled("LM358_SOIC-8"));
        for (adj, part) in [(false, "LM7805_TO-220"), (true, "LM317_TO-220")] {
            let name = part.split('_').next().unwrap();
            let s = RegulatorInput::example(adj).spec(name, "s").unwrap();
            let f = fit::fit(&fit::Spec::Regulator(s)).unwrap();
            assert_eq!(f.text, bundled(part), "{part}");
        }
    }

    #[test]
    fn temperatures_in_the_conditions_are_read() {
        assert_eq!(temperatures("−40°C +125°C"), vec![-40.0, 125.0]);
        assert_eq!(temperatures("TA = 25 °C, VS = 5 V"), vec![25.0]);
        assert!(temperatures("RL = 10 kΩ").is_empty());
    }

    #[test]
    fn the_regulation_lines_take_the_data_sheets_ways_of_writing() {
        assert!((regulation("x", "0.5m 12.5", 5.0, true).unwrap() - 4e-5).abs() < 1e-15);
        assert!((regulation("x", "0.01%/V", 1.25, true).unwrap() - 1.25e-4).abs() < 1e-15);
        assert!((regulation("x", "0.1% 1.5", 5.0, false).unwrap() - 5e-3 / 1.5).abs() < 1e-15);
        assert!(regulation("x", "12.5", 5.0, true).unwrap_err().contains("0.01%/V"));
    }

    #[test]
    fn missing_or_wrong_values_say_which_and_how() {
        let mut o = OpampInput::from_spec(&lm358());
        o.gbw.clear();
        assert!(o.spec("X", "s").unwrap_err().contains("GBW"));
        let mut o = OpampInput::from_spec(&lm358());
        o.aol = "200 V/mV".into();
        assert!((o.spec("X", "s").unwrap().aol_db - 106.0206).abs() < 1e-3, "V/mV is turned into dB");
        o.iq_max = "1u".into();
        assert!(o.spec("X", "s").unwrap_err().contains("max"));
        let mut r = RegulatorInput::example(false);
        r.ilim = "10".into();
        assert!(r.spec("X", "s").unwrap_err().contains("電流制限"));
    }

    #[test]
    fn the_pdf_table_fills_the_op_amp_boxes() {
        use datasheet::spec_table::SpecValue;
        let v = |symbol: &str, conditions: &str, typ: f64, max: Option<f64>, unit: Option<&str>| SpecValue {
            page: 7,
            symbol: symbol.into(),
            conditions: conditions.into(),
            min: None,
            typ: Some(typ),
            max,
            unit: unit.map(str::to_string),
            rect: datasheet::Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 },
        };
        // as the ADA4522's table reads: some units are not read (SR, ΦM, the noise at 1 kHz)
        let table = [
            v("VOS", "−40°C +125°C", 7.2, None, Some("µV")),
            v("VOS", "ADA4522-1, ADA4522-2", 4.0, Some(22.0), Some("µV")),
            v("IB", "", 80.0, Some(300.0), Some("pA")),
            v("GBP", "", 2.7, None, Some("MHz")),
            v("SR+", "", 1.4, None, None),
            v("eN", "Voltage Noise Density AV = 100, f = 1 kHz", 5.8, None, None),
            v("ISY", "", 840.0, Some(910.0), Some("µA")),
            v("ISC+", "TA = 125°C", 15.0, None, Some("mA")),
            v("ISC+", "", 33.0, None, Some("mA")),
            v("ΦM", "", 64.0, None, None),
            v("AVO", "RL = 10 kΩ", 150.0, None, Some("dB")),
        ];
        let mut o = OpampInput::default();
        o.fill_from_pdf(|s| table.iter().filter(|x| x.symbol == s).cloned().collect());
        assert_eq!((o.vos.as_str(), o.vos_max.as_str()), ("4u", "22u"), "the 25 °C row, not the hot one");
        assert_eq!((o.ib.as_str(), o.ib_max.as_str()), ("80p", "300p"));
        assert_eq!(o.gbw, "2.7Meg");
        assert_eq!(o.sr, "1.4");
        assert_eq!(o.en, "5.8n");
        assert_eq!((o.iq.as_str(), o.iq_max.as_str()), ("840u", "910u"));
        assert_eq!(o.isc, "33m");
        assert_eq!(o.pm, "64");
        assert_eq!(o.aol, "150");
        assert!(o.found.iter().any(|(ok, l)| !ok && l.contains("振れ幅")));
        assert!(!o.found.iter().any(|(_, l)| l.contains("別々の表")), "all from p.8: no word about pages");
        let mut moved = table.to_vec();
        moved[4].page = 5;
        o.fill_from_pdf(|s| moved.iter().filter(|x| x.symbol == s).cloned().collect());
        assert!(o.found.iter().any(|(ok, l)| !ok && l.contains("p.6〜p.8")), "{:?}", o.found);
    }
}
