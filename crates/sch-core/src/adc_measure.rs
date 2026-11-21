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
}

impl Measured {
    /// The unit of the measured value ("" for a part: its own unit).
    pub fn unit(&self) -> &'static str {
        match self {
            Measured::Volts { .. } => "V",
            Measured::Ohms { .. } => "Ω",
            Measured::Amps { .. } => "A",
            Measured::Part { .. } => "",
        }
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
}
