//! Equivalent models from data sheet readings (M10): a `.fit.json` file holds what was read off one part's data sheet
//! (table values and points read on its graphs); [`fit`] turns it into the `.model` card, the notes shown with the model
//! (`sim.limits`) and a table of how far the model lands from each reading. [`write_part`] puts the model into the
//! part's `.part.json`. The bundled diodes, transistors and MOSFETs are made this way (their readings are in
//! `crates/part-lib/fits/`); `ccad fit FILE` runs it from the command line. Op amps, linear regulators and shunt
//! references are not fitted but built from the data sheet's typ values with a template (`opamp`, `regulator`; M3), and so
//! are series references (`reference`) and SET-pin LDOs (`setldo`, P6).
//! Details: `docs/code-notes/part-lib.md`.

mod bjt;
pub mod buck;
pub mod comparator;
mod darlington;
mod diode;
mod edit;
mod mos;
pub mod num;
pub mod opamp;
mod opto;
pub mod reference;
pub mod regulator;
pub mod setldo;
pub mod simple;

pub use bjt::BjtSpec;
pub use buck::BuckSpec;
pub use comparator::ComparatorSpec;
pub use darlington::DarlingtonSpec;
pub use diode::DiodeSpec;
pub use edit::write_part;
pub use mos::{Caps, MosSpec};
pub use opamp::OpampSpec;
pub use opto::{OptocouplerSpec, PhotodiodeSpec};
pub use reference::ReferenceSpec;
pub use regulator::{RegulatorSpec, ShuntSpec};
pub use setldo::SetLdoSpec;
pub use simple::{CrystalSpec, NtcSpec, RelaySpec, TvsSpec, ntc_b_from_table};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What was read off one part's data sheet. `kind` picks the model: "diode", "bjt", "mos", "darlington", "photodiode",
/// "optocoupler", "opamp", "comparator", "regulator", "shunt", "reference", "setldo", "buck", "ntc", "crystal", "tvs" or "relay".
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Spec {
    Diode(diode::DiodeSpec),
    Bjt(bjt::BjtSpec),
    Mos(mos::MosSpec),
    Darlington(darlington::DarlingtonSpec),
    Photodiode(opto::PhotodiodeSpec),
    Optocoupler(opto::OptocouplerSpec),
    Opamp(opamp::OpampSpec),
    Comparator(comparator::ComparatorSpec),
    Regulator(regulator::RegulatorSpec),
    Shunt(regulator::ShuntSpec),
    Reference(reference::ReferenceSpec),
    SetLdo(setldo::SetLdoSpec),
    Buck(buck::BuckSpec),
    Ntc(simple::NtcSpec),
    Crystal(simple::CrystalSpec),
    Tvs(simple::TvsSpec),
    Relay(simple::RelaySpec),
}

/// The simulator's temperature at which the models hold their data sheets' typ values (the data sheets say 25 °C; the
/// simulator's nominal is 27 °C, and a model's temperature terms are 0 there, so the nominal results stay as they were).
pub const T_MODEL: f64 = 27.0;

/// An output or reference voltage against the temperature, read off the data sheet's graph (M8): points (°C, relative
/// deviation: 0.001 = +0.1 %), joined by straight lines and continued past the ends by the end segments' slopes. A data
/// sheet's coefficient (onsemi's MC7805: −0.3 mV/°C) is two points.
#[derive(Clone, Debug, Deserialize)]
pub struct TempCurve {
    pub points: Vec<[f64; 2]>,
    /// Where it was read (the figure, the table's row), for the notes.
    pub from: String,
}

impl TempCurve {
    /// The relative deviation at `t` (°C).
    pub fn at(&self, t: f64) -> f64 {
        let p = &self.points;
        let k = (1..p.len() - 1).find(|&k| t < p[k][0]).unwrap_or(p.len() - 1);
        let ([t0, y0], [t1, y1]) = (p[k - 1], p[k]);
        y0 + (y1 - y0) / (t1 - t0) * (t - t0)
    }

    /// The same as an expression in `temp`, less its value at `T_MODEL` (0 there): the first segment carried on below its
    /// start, the last above its end, the ones between clamped to their span.
    pub fn expr(&self) -> String {
        let p = &self.points;
        let n = p.len();
        let slope = |k: usize| (p[k + 1][1] - p[k][1]) / (p[k + 1][0] - p[k][0]);
        let mut terms = vec![];
        for k in 0..n - 1 {
            let (a, b) = (num::g(p[k][0], 6), num::g(p[k + 1][0], 6));
            let x = match (k == 0, k == n - 2) {
                (true, true) => "temp".to_string(),
                (true, false) => format!("min(temp,{b})"),
                (false, true) => format!("max(temp,{a})"),
                (false, false) => format!("limit(temp,{a},{b})"),
            };
            terms.push(format!("{}*({x}-{a})", num::g(slope(k), 6)));
        }
        format!("({}+{})", terms.join("+"), num::g(p[0][1] - self.at(T_MODEL), 6))
    }

    pub fn check(&self) -> Result<(), String> {
        if self.points.len() < 2 || self.points.windows(2).any(|w| w[1][0] <= w[0][0]) {
            return Err("temp.points: 2 つ以上の点を、温度の低い順に".into());
        }
        Ok(())
    }

    /// The deviation over the points' span, for the notes ("−40〜125 ℃ で −0.28〜+0.13 %").
    pub fn span(&self) -> String {
        let (lo, hi) = self.points.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p[1]), h.max(p[1])));
        let (t0, t1) = (self.points[0][0], self.points[self.points.len() - 1][0]);
        let base = self.at(25.0);
        format!("{}〜{} ℃ で 25 ℃ から {:+.2}〜{:+.2} %", num::g(t0, 6), num::g(t1, 6), (lo - base) * 100.0, (hi - base) * 100.0)
    }
}

/// The fields every kind has.
#[derive(Clone, Debug, Deserialize)]
pub struct Common {
    /// The part file's name without `.part.json` (`1N4148`, `2N3904_TO-92`).
    pub part: String,
    /// The model's name in the `.model` card.
    pub name: String,
    /// The data sheet: maker, document number, revision, date.
    pub source: String,
    #[serde(default)]
    pub url: String,
    /// The part's description (shown in the library).
    #[serde(default)]
    pub description: String,
    /// When the readings were made (kept for the record).
    #[serde(default)]
    pub read: String,
    /// How each reading was made (which figure, what was assumed). Kept for the record; not used by the fit.
    #[serde(default)]
    pub remarks: Vec<String>,
}

impl Spec {
    pub fn common(&self) -> &Common {
        match self {
            Spec::Diode(s) => &s.common,
            Spec::Bjt(s) => &s.common,
            Spec::Mos(s) => &s.common,
            Spec::Darlington(s) => &s.common,
            Spec::Photodiode(s) => &s.common,
            Spec::Optocoupler(s) => &s.common,
            Spec::Opamp(s) => &s.common,
            Spec::Comparator(s) => &s.common,
            Spec::Regulator(s) => &s.common,
            Spec::Shunt(s) => &s.common,
            Spec::Reference(s) => &s.common,
            Spec::SetLdo(s) => &s.common,
            Spec::Buck(s) => &s.common,
            Spec::Ntc(s) => &s.common,
            Spec::Crystal(s) => &s.common,
            Spec::Tvs(s) => &s.common,
            Spec::Relay(s) => &s.common,
        }
    }

    pub fn load(path: &Path) -> Result<Spec, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e} (kind は diode / bjt / mos / darlington / photodiode / optocoupler / opamp / comparator / regulator / shunt / reference / setldo / ntc / crystal / tvs / relay。形は crates/part-lib/fits/ の例を見てください)", path.display()))
    }
}

/// One reading next to the model's value at the same condition.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    /// What is compared ("VF", "hFE", "RDS(on)", ...).
    pub item: String,
    /// The condition ("10 mA", "VGS = 4.5 V, ID = 0.22 A").
    pub at: String,
    pub datasheet: f64,
    pub model: f64,
    pub unit: String,
    /// Shown for comparison only; the fit does not aim at it (a value the model's form cannot follow).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compare_only: bool,
}

impl Row {
    /// (model − data sheet) / data sheet in percent.
    pub fn error_pct(&self) -> f64 {
        (self.model / self.datasheet - 1.0) * 100.0
    }
}

/// The result of [`fit`].
#[derive(Clone, Debug, Serialize)]
pub struct Fitted {
    pub part: String,
    pub source: String,
    pub url: String,
    pub description: String,
    /// The `.model` card.
    pub text: String,
    /// What the model is made from and what it leaves out (`sim.limits`).
    pub limits: Vec<String>,
    pub rows: Vec<Row>,
    /// Things to look at (a reading the model's form cannot follow).
    pub notes: Vec<String>,
    /// Values of the part's `sim.params` the model sets (written only where the part has them: a regulator's `vout`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(String, f64)>,
}

/// Fits the model to the readings.
/// The values of a `.model` card in `text` (the first, or the one named `name`): `IS=1e-14` → ("IS", 1e-14).
pub(crate) fn card_params(text: &str, name: Option<&str>) -> Option<std::collections::BTreeMap<String, f64>> {
    let card = text.lines().map(str::trim).filter(|l| l.starts_with(".model")).find(|l| name.is_none_or(|n| l.split_whitespace().nth(1) == Some(n)))?;
    let inner = card.split_once('(')?.1.trim_end_matches(')');
    Some(inner.split_whitespace().filter_map(|kv| kv.split_once('=')).filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?))).collect())
}

/// The fit's own objective (the sum it makes as small as it can: how far the model is from the readings) for a model as written, for
/// the kinds whose fit is a search that may end in a slightly different place on another system (the transistors: `None` for the rest).
pub fn objective(spec: &Spec, text: &str) -> Option<f64> {
    match spec {
        Spec::Bjt(b) => bjt::objective(b, text),
        Spec::Darlington(d) => darlington::objective(d, text),
        _ => None,
    }
}

pub fn fit(spec: &Spec) -> Result<Fitted, String> {
    let c = spec.common();
    let (text, limits, rows, notes) = match spec {
        Spec::Diode(s) => diode::fit(s)?,
        Spec::Bjt(s) => bjt::fit(s)?,
        Spec::Mos(s) => mos::fit(s)?,
        Spec::Darlington(s) => darlington::fit(s)?,
        Spec::Photodiode(s) => opto::fit_photodiode(s)?,
        Spec::Optocoupler(s) => opto::fit_optocoupler(s)?,
        Spec::Opamp(s) => opamp::fit(s)?,
        Spec::Comparator(s) => comparator::fit(s)?,
        Spec::Regulator(s) => regulator::fit_regulator(s)?,
        Spec::Shunt(s) => regulator::fit_shunt(s)?,
        Spec::Reference(s) => reference::fit_reference(s)?,
        Spec::SetLdo(s) => setldo::fit_setldo(s)?,
        Spec::Buck(s) => buck::fit_buck(s)?,
        Spec::Ntc(s) => simple::fit_ntc(s)?,
        Spec::Crystal(s) => simple::fit_crystal(s)?,
        Spec::Tvs(s) => simple::fit_tvs(s)?,
        Spec::Relay(s) => simple::fit_relay(s)?,
    };
    let (description, params) = match spec {
        Spec::Opamp(s) => (opamp::description(s), vec![("gbw".into(), s.gbw), ("gain_dc".into(), 10f64.powf(s.aol_db / 20.0).round())]),
        Spec::Comparator(s) => (comparator::description(s), vec![]),
        Spec::Regulator(s) => (c.description.clone(), regulator::regulator_params(s)),
        _ => (c.description.clone(), vec![]),
    };
    Ok(Fitted { part: c.part.clone(), source: c.source.clone(), url: c.url.clone(), description, text, limits, rows, notes, params })
}

/// The output of each kind's fit: the card, the limits, the rows and the notes.
type Out = (String, Vec<String>, Vec<Row>, Vec<String>);

fn row(item: &str, at: String, datasheet: f64, model: f64, unit: &str) -> Row {
    Row { item: item.into(), at, datasheet, model, unit: unit.into(), compare_only: false }
}

/// A current for a condition: "10 mA", "0.22 A", "250 µA".
fn amps(i: f64) -> String {
    let a = i.abs();
    if a >= 1.0 {
        format!("{} A", num::g(i, 4))
    } else if a >= 1e-3 {
        format!("{} mA", num::g(i * 1e3, 4))
    } else {
        format!("{} µA", num::g(i * 1e6, 4))
    }
}

/// Junction capacitance `C(V) = CJ0 / (1 + V/VJ)^M` fitted to (reverse voltage, capacitance) points: a grid over VJ
/// (0.1 .. 1.2 V) and M (`m_from` .. 0.70 in hundredths), CJ0 by least squares in log. One point: VJ 0.7, M 0.5 assumed.
fn fit_cap(points: &[[f64; 2]], m_from: u32) -> (f64, f64, f64) {
    if let [[v, c]] = points {
        return (c * (1.0 + v / 0.7).powf(0.5), 0.7, 0.5);
    }
    let mut best: Option<(f64, f64, f64, f64)> = None;
    for vj10 in 1..13 {
        let vj = vj10 as f64 / 10.0;
        for m100 in m_from..71 {
            let mm = m100 as f64 / 100.0;
            let logs: Vec<f64> = points.iter().map(|[v, c]| c.ln() + mm * (1.0 + v / vj).ln()).collect();
            let lc0 = logs.iter().sum::<f64>() / logs.len() as f64;
            let err: f64 = logs.iter().map(|x| (x - lc0).powi(2)).sum();
            if best.is_none_or(|b| err < b.0) {
                best = Some((err, lc0.exp(), vj, mm));
            }
        }
    }
    let b = best.unwrap_or((0.0, 0.0, 0.7, 0.5));
    (b.1, b.2, b.3)
}

fn cap_at(cj0: f64, vj: f64, m: f64, v: f64) -> f64 {
    cj0 / (1.0 + v / vj).powf(m)
}

const LAST_WORD: &str = "最終の確認は、メーカーの公式モデルで行ってください (公開されていれば「部品の取り込み」で読めます)。";

#[cfg(test)]
mod tests;
