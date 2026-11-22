//! High / Low / undefined, as a CMOS input reads a voltage (the live logic mode and the logic analyser).

/// A level read with the CMOS thresholds: below VIL, above VIH, or between them (undefined).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
    Mid,
}

/// The input thresholds of a CMOS logic family (74HC): 30 % and 70 % of the supply.
pub fn thresholds(vcc: f64) -> (f64, f64) {
    (0.3 * vcc, 0.7 * vcc)
}

pub fn level(v: f64, (vil, vih): (f64, f64)) -> Level {
    if v <= vil {
        Level::Low
    } else if v >= vih {
        Level::High
    } else {
        Level::Mid
    }
}
