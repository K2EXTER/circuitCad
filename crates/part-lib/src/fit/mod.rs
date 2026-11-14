//! Equivalent models from data sheet readings (M10): a `.fit.json` file holds what was read off one part's data sheet
//! (table values and points read on its graphs); [`fit`] turns it into the `.model` card, the notes shown with the model
//! (`sim.limits`) and a table of how far the model lands from each reading. [`write_part`] puts the model into the
//! part's `.part.json`. The bundled diodes, transistors and MOSFETs are made this way (their readings are in
//! `crates/part-lib/fits/`); `ccad fit FILE` runs it from the command line. Details: `docs/code-notes/part-lib.md`.

mod bjt;
mod darlington;
mod diode;
mod edit;
mod mos;
pub mod num;

pub use bjt::BjtSpec;
pub use darlington::DarlingtonSpec;
pub use diode::DiodeSpec;
pub use edit::write_part;
pub use mos::{Caps, MosSpec};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What was read off one part's data sheet. `kind` picks the model: "diode", "bjt" or "mos".
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Spec {
    Diode(diode::DiodeSpec),
    Bjt(bjt::BjtSpec),
    Mos(mos::MosSpec),
    Darlington(darlington::DarlingtonSpec),
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
        }
    }

    pub fn load(path: &Path) -> Result<Spec, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e} (kind は diode / bjt / mos / darlington。形は crates/part-lib/fits/ の例を見てください)", path.display()))
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
}

/// Fits the model to the readings.
pub fn fit(spec: &Spec) -> Result<Fitted, String> {
    let c = spec.common();
    let (text, limits, rows, notes) = match spec {
        Spec::Diode(s) => diode::fit(s)?,
        Spec::Bjt(s) => bjt::fit(s)?,
        Spec::Mos(s) => mos::fit(s)?,
        Spec::Darlington(s) => darlington::fit(s)?,
    };
    Ok(Fitted { part: c.part.clone(), source: c.source.clone(), url: c.url.clone(), description: c.description.clone(), text, limits, rows, notes })
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
