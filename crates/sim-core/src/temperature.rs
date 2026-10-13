//! Circuit temperature. The solver itself runs at the nominal 27 °C (kT/q = 25.865 mV); a different
//! temperature is applied by rewriting the semiconductor models, so every analysis (DC, transient,
//! AC) picks it up without knowing about temperature:
//!
//! * diodes / BJTs: the saturation current grows steeply with temperature (the reason a silicon
//!   junction drops about 2 mV less per °C), `n` is scaled so that `n·VT` equals `n·kT/q` at the
//!   new temperature, and the current gain `bf` rises slowly;
//! * MOSFETs: the mobility falls (`kp ∝ T^-1.5`) and the threshold shrinks by about 2 mV per °C.
//!
//! Resistors, capacitors and inductors are treated as ideal (no temperature coefficient).

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, SimError};
use crate::dcsweep::{DcSweepResult, DcSweepSignal};

/// Temperature the models are specified at.
pub const T_NOM_C: f64 = 27.0;
const EG: f64 = 1.11; // silicon band gap, eV
const XTI: f64 = 3.0; // saturation-current temperature exponent
const XTB: f64 = 1.5; // current-gain temperature exponent
const K_OVER_Q: f64 = 8.617333e-5; // V/K

impl Circuit {
    /// A copy of the circuit whose semiconductors behave as at `temp_c` degrees Celsius.
    pub fn at_temperature(&self, temp_c: f64) -> Circuit {
        let mut c = self.clone();
        if (temp_c - T_NOM_C).abs() < 1e-9 {
            return c;
        }
        let (t, tn) = (temp_c + 273.15, T_NOM_C + 273.15);
        let ratio = t / tn;
        let vt = K_OVER_Q * t;
        // saturation current of a junction with emission coefficient n
        let is_at = |is: f64, n: f64, xti: f64| is * ratio.powf(xti / n) * ((EG / (n * vt)) * (ratio - 1.0)).exp();
        for e in &mut c.elements {
            match e {
                Element::Diode { model, .. } => {
                    model.is = is_at(model.is, model.n, XTI);
                    model.n *= ratio;
                }
                Element::Bjt { model, .. } => {
                    model.is = is_at(model.is, model.nf, XTI);
                    model.bf *= ratio.powf(XTB);
                    model.nf *= ratio;
                    model.nr *= ratio;
                }
                Element::Mosfet { model, .. } => {
                    model.kp *= ratio.powf(-1.5);
                    let shrink = 0.002 * (temp_c - T_NOM_C);
                    model.vto = model.vto.signum() * (model.vto.abs() - shrink).max(0.05);
                }
                Element::BSource { expr, .. } => *expr = expr.at_temperature(temp_c),
                _ => {}
            }
        }
        c
    }
}

/// DC operating point at every temperature from `t0` to `t1` (inclusive, `step` apart). The
/// result has the same shape as a DC sweep, with the temperature (°C) as the x axis.
pub fn temp_sweep(circuit: &Circuit, t0: f64, t1: f64, step: f64) -> Result<DcSweepResult, SimError> {
    if !(t0.is_finite() && t1.is_finite() && step.is_finite()) || step <= 0.0 || t1 < t0 {
        return Err(SimError::Invalid("温度の開始・終了・刻みを確認してください (開始 ≦ 終了、刻み > 0)".into()));
    }
    let n = ((t1 - t0) / step + 1e-9).floor() as usize + 1;
    if n > 2_000 {
        return Err(SimError::Invalid("点数が多すぎます。刻みを大きくしてください".into()));
    }
    let mut signals: Vec<DcSweepSignal> = vec![];
    let mut warnings = vec![];
    let mut xs = vec![];
    for k in 0..n {
        let temp = t0 + step * k as f64;
        let r = dc_op(&circuit.at_temperature(temp)).map_err(|e| match e {
            SimError::NoConvergence => SimError::Invalid(format!("{temp} ℃ で収束しませんでした")),
            other => other,
        })?;
        if k == 0 {
            warnings = r.warnings.clone();
            for (name, _) in r.voltages.iter().filter(|(name, _)| name != "GND") {
                signals.push(DcSweepSignal { name: format!("V({name})"), unit: "V", values: vec![] });
            }
            for el in &r.elements {
                signals.push(DcSweepSignal { name: format!("I({})", el.name), unit: "A", values: vec![] });
            }
        }
        for sg in signals.iter_mut() {
            let v = if let Some(name) = sg.name.strip_prefix("V(").and_then(|x| x.strip_suffix(')')) {
                r.voltage(name)
            } else {
                sg.name.strip_prefix("I(").and_then(|x| x.strip_suffix(')')).and_then(|name| r.element(name)).map(|el| el.current)
            };
            sg.values.push(v.unwrap_or(f64::NAN));
        }
        xs.push(temp);
    }
    Ok(DcSweepResult { source: "温度".into(), x: xs, signals, warnings })
}
