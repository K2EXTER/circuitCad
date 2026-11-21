//! What an ADC on the schematic measures, one way per row (candidate L1): the thing measured (a voltage, a resistance or a current
//! put between two nets, or the value of a part already drawn: a sensor, a bridge's arm), the conditions it is measured under (a
//! latching relay set, a switch thrown, a part's value replaced: a meter's range, a multiplexer's channel), and the ADC's inputs,
//! gain and data rate. Kept with the schematic; the simulator works out each row's reading, its noise in the measured unit and the
//! digits that stay still (`sim_core::adcmeasure`).

use serde::{Deserialize, Serialize};

/// What is measured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measured {
    /// A voltage source between the two nets.
    Volts { plus: String, minus: String },
    /// A resistor between the two nets.
    Ohms { plus: String, minus: String },
    /// A current source driving the current into `plus` and out of `minus` (through the circuit between them).
    Amps { plus: String, minus: String },
    /// The value of a part already on the schematic (a resistor, a voltage or current source): a sensor, a bridge's arm.
    Part { part: String },
    /// A temperature sensor on the schematic (two pins), in °C: an NTC thermistor (`b` > 0: `r0` at 25 °C and its B constant) or
    /// a platinum or other resistance thermometer (`alpha` > 0: `r0` at 0 °C, linear). The part is put in as the resistor it is
    /// at the temperature.
    Temp {
        part: String,
        r0: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        b: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        alpha: f64,
    },
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Measured {
    /// The unit of the measured value ("" for a part: its own unit).
    pub fn unit(&self) -> &'static str {
        match self {
            Measured::Volts { .. } => "V",
            Measured::Ohms { .. } => "Ω",
            Measured::Amps { .. } => "A",
            Measured::Part { .. } => "",
            Measured::Temp { .. } => "°C",
        }
    }

    /// The part on the schematic that is measured (a part's value, a temperature sensor).
    pub fn part(&self) -> Option<&str> {
        match self {
            Measured::Part { part } | Measured::Temp { part, .. } => Some(part),
            _ => None,
        }
    }
}

/// The resistance of a temperature sensor at `t` °C: an NTC (`b` > 0, `r0` at 25 °C) or a linear one (`alpha`, `r0` at 0 °C).
pub fn sensor_ohms(r0: f64, b: f64, alpha: f64, t: f64) -> f64 {
    if b > 0.0 {
        r0 * (b * (1.0 / (t + 273.15) - 1.0 / 298.15)).exp()
    } else {
        r0 * (1.0 + alpha * t)
    }
}

/// One way of measuring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdcMeasure {
    pub name: String,
    pub measured: Measured,
    /// The value tried (V, ohm, A, or the part's unit): a full scale, or a typical value.
    pub at: f64,
    /// The conditions, each a part's reference (a latching relay: set; a switch: pushed, or thrown to its B side) or `REF=VALUE`
    /// (a part's value replaced).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    /// The ADC (reference) and the pins (numbers) of its input.
    pub adc: String,
    pub pos: String,
    pub neg: String,
    /// A ratio reading: the second input (pins) the first is divided by (a resistance against a reference resistor, a bridge
    /// against its excitation). Empty: a plain reading.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_pos: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_neg: String,
    pub gain: f64,
    pub rate: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub low_latency: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chop: bool,
    /// For the error estimate, the parts that carry no figure of their own: a resistor's tolerance (%) and temperature
    /// coefficient (ppm/K). And the temperature span the board sees beyond the one it was calibrated at (K).
    #[serde(default = "default_tol")]
    pub tol_pct: f64,
    #[serde(default = "default_tc")]
    pub tc_ppm: f64,
    #[serde(default = "default_dtemp")]
    pub dtemp: f64,
}

pub const DEFAULT_TOL_PCT: f64 = 1.0;
pub const DEFAULT_TC_PPM: f64 = 100.0;
pub const DEFAULT_DTEMP: f64 = 10.0;

fn default_tol() -> f64 {
    DEFAULT_TOL_PCT
}

fn default_tc() -> f64 {
    DEFAULT_TC_PPM
}

fn default_dtemp() -> f64 {
    DEFAULT_DTEMP
}

impl AdcMeasure {
    /// Whether it is a ratio reading.
    pub fn ratio(&self) -> bool {
        !self.ref_pos.is_empty() && !self.ref_neg.is_empty()
    }
}

/// One condition as written: a part's reference alone (`K1`, `SW2`) or a value for a part (`R5=10k`).
pub fn condition(text: &str) -> (String, Option<String>) {
    match text.split_once('=') {
        Some((r, v)) => (r.trim().to_string(), Some(v.trim().to_string())),
        None => (text.trim().to_string(), None),
    }
}

/// The conditions typed in one box, split at commas and spaces (a value with spaces is not one: `R5=10k`, `V3=1.2`).
pub fn parse_conditions(text: &str) -> Vec<String> {
    text.split([',', '、', ' ']).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_are_typed_in_one_box() {
        assert_eq!(parse_conditions("K1, SW2 R5=10k、V3=1.2"), ["K1", "SW2", "R5=10k", "V3=1.2"]);
        assert_eq!(condition("R5=10k"), ("R5".into(), Some("10k".into())));
        assert_eq!(condition("K1"), ("K1".into(), None));
    }

    #[test]
    fn a_sensor_follows_its_law() {
        assert!((sensor_ohms(10e3, 3950.0, 0.0, 25.0) - 10e3).abs() < 1e-6);
        assert!((sensor_ohms(10e3, 3950.0, 0.0, 100.0) - 680.0).abs() < 20.0, "the NTC's own description: about 680 ohm at 100 C");
        assert!((sensor_ohms(100.0, 0.0, 0.00385, 100.0) - 138.5).abs() < 1e-9, "Pt100");
    }
}
