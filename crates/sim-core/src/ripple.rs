//! Ripple current and life of electrolytic capacitors in a transient run: the rms of the current that flows
//! through the part (the AC part of it) against the data sheet's rated ripple current, and the life the
//! data sheet's endurance promises at the temperature the part really runs at.

use crate::ratings::RatingLevel;
use crate::tran::TranResult;
use sch_core::Schematic;

/// How much the inside of a capacitor is warmer than the air when it carries its rated ripple current (℃). The
/// data sheets give 5 to 10 degrees for small parts; the larger value is used so that the estimate errs short.
/// The rise follows the square of the current (the heat is I²·ESR).
pub const RIPPLE_HEATING_AT_RATED_C: f64 = 10.0;

/// The life is not quoted beyond this (hours, 15 years): other things end a capacitor first.
pub const MAX_LIFE_HOURS: f64 = 15.0 * 8760.0;

/// What one capacitor with data sheet ripple / life figures went through in the run.
#[derive(Clone, Debug, PartialEq)]
pub struct RippleReport {
    pub refdes: String,
    /// rms of the AC part of the current (A), over the second half of the run.
    pub i_rms: f64,
    /// The ripple frequency (Hz) worked out from the zero crossings; 0 when the current does not cross its mean twice.
    pub freq: f64,
    /// The rated ripple current at this frequency (A), when the part gives one.
    pub allowed: Option<f64>,
    pub level: Option<RatingLevel>,
    /// The temperature inside the part (℃): the air plus the heating of the ripple current.
    pub core_temp_c: f64,
    /// The life at that temperature (hours), when the part gives its endurance.
    pub life_hours: Option<f64>,
}

impl RippleReport {
    /// The finding in one sentence of Japanese.
    pub fn message(&self) -> String {
        let mut t = format!("{} のリップル電流 {}", self.refdes, fmt_a(self.i_rms));
        if self.freq > 0.0 {
            t.push_str(&format!(" ({}Hz)", fmt_hz(self.freq)));
        }
        if let Some(a) = self.allowed {
            t.push_str(&format!(" は、定格 {} の {:.0}% です", fmt_a(a), self.i_rms / a * 100.0));
            t.push_str(match self.level {
                Some(RatingLevel::Over) => " (超えています)",
                Some(RatingLevel::Caution) => " (余裕が少ないです)",
                None => "",
            });
        }
        t
    }

    /// What to do about it; empty when nothing is wrong.
    pub fn hint(&self) -> &'static str {
        match self.level {
            Some(_) => "リップル電流は、コンデンサの中で熱になって寿命を縮めます。容量の大きい部品・ESR の小さい部品・定格リップル電流の大きい部品を選ぶか、同じ部品を並列にして電流を分けます。",
            None => "",
        }
    }

    /// The life estimate in words (hours and years), when there is one.
    pub fn life_text(&self) -> Option<String> {
        let h = self.life_hours?;
        let capped = h >= MAX_LIFE_HOURS - 1.0;
        let years = h / 8760.0;
        Some(format!(
            "{} の推定寿命: {}約 {:.0} 時間 (約 {:.1} 年)。内部の温度 {:.0}℃ で見積もりました",
            self.refdes,
            if capped { "15 年以上 (上限) " } else { "" },
            h,
            years,
            self.core_temp_c
        ))
    }
}

fn fmt_a(a: f64) -> String {
    if a.abs() < 1.0 { format!("{:.0} mA", a * 1e3) } else { format!("{a:.2} A") }
}

fn fmt_hz(f: f64) -> String {
    if f >= 1e6 {
        format!("{:.1}M", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.1}k", f / 1e3)
    } else {
        format!("{f:.0}")
    }
}

/// The current through a library capacitor: the capacitor inside its model is in series with the rest, so any of
/// the series elements carries the whole current.
fn current_of<'a>(tran: &'a TranResult, refdes: &str) -> Option<&'a [f64]> {
    [format!("I({refdes}.C1)"), format!("I({refdes}.Resr)"), format!("I({refdes})")].iter().find_map(|n| tran.signal(n)).map(|s| s.values.as_slice())
}

/// The mean, the rms of the rest and the number of upward crossings of the mean, over the samples from `t0` on.
/// The samples are not evenly spaced (the step follows the circuit), so every segment counts by its length.
fn ac_part(time: &[f64], i: &[f64], t0: f64) -> Option<(f64, usize, f64)> {
    let start = time.iter().position(|t| *t >= t0)?;
    let (t, v) = (&time[start..], &i[start..]);
    if t.len() < 3 {
        return None;
    }
    let span = t[t.len() - 1] - t[0];
    if span <= 0.0 {
        return None;
    }
    let mean = t.windows(2).zip(v.windows(2)).map(|(tt, vv)| (tt[1] - tt[0]) * (vv[0] + vv[1]) / 2.0).sum::<f64>() / span;
    // a straight line between samples: the integral of (v - mean)² over a segment is (a² + ab + b²) / 3 · dt
    let sq: f64 = t
        .windows(2)
        .zip(v.windows(2))
        .map(|(tt, vv)| {
            let (a, b) = (vv[0] - mean, vv[1] - mean);
            (tt[1] - tt[0]) * (a * a + a * b + b * b) / 3.0
        })
        .sum();
    let crossings = v.windows(2).filter(|w| w[0] - mean < 0.0 && w[1] - mean >= 0.0).count();
    Some(((sq / span).sqrt(), crossings, span))
}

/// Reports for every capacitor whose part carries ripple or life figures, from the second half of the run (the
/// start-up is over by then). `ambient_c` is the circuit temperature of the run. Empty when there is none.
pub fn ripple_reports(sch: &Schematic, tran: &TranResult, ambient_c: f64) -> Vec<RippleReport> {
    let mut out = vec![];
    let (Some(t_first), Some(t_last)) = (tran.time.first(), tran.time.last()) else { return out };
    let t0 = t_first + (t_last - t_first) / 2.0;
    for c in &sch.components {
        let Some(rt) = c.custom.as_ref().and_then(|s| s.ratings.as_ref()) else { continue };
        if rt.ripple.is_none() && rt.life.is_none() {
            continue;
        }
        let Some(current) = current_of(tran, &c.refdes) else { continue };
        let Some((i_rms, crossings, span)) = ac_part(&tran.time, current, t0) else { continue };
        let freq = if crossings >= 2 { crossings as f64 / span } else { 0.0 };
        let allowed = rt.ripple.as_ref().map(|r| r.allowed_at(if freq > 0.0 { freq } else { r.freq_hz }));
        let level = allowed.and_then(|a| {
            if i_rms > a {
                Some(RatingLevel::Over)
            } else if i_rms > 0.8 * a {
                Some(RatingLevel::Caution)
            } else {
                None
            }
        });
        let core = ambient_c + allowed.map_or(0.0, |a| RIPPLE_HEATING_AT_RATED_C * (i_rms / a).powi(2));
        let life_hours = rt.life.as_ref().map(|l| l.at(core).min(MAX_LIFE_HOURS));
        out.push(RippleReport { refdes: c.refdes.clone(), i_rms, freq, allowed, level, core_temp_c: core, life_hours });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tran::Signal;
    use sch_core::{CustomSymbol, LifeRating, PartRatings, PinSide, RippleRating, SymPin};

    fn sheet(ripple: Option<RippleRating>, life: Option<LifeRating>) -> Schematic {
        let mut s = Schematic::default();
        let pin = |n: &str, side| SymPin { number: n.into(), name: n.into(), side, sim_only: false };
        let sym = CustomSymbol {
            name: "CE_TEST".into(),
            prefix: "C".into(),
            pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)],
            sim: None,
            ratings: Some(PartRatings { ripple, life, ..Default::default() }),
            glyph: "electrolytic".into(),
        };
        s.add_custom(sym, "100u", "", (10, 10), 0);
        s
    }

    /// A sine of `amp` amps at `f` Hz on top of `dc`: twenty cycles, sampled unevenly (the run's steps are not equal).
    fn run(amp: f64, dc: f64, f: f64) -> TranResult {
        let mut time = vec![];
        let mut i = vec![];
        let (mut t, mut k, base) = (0.0, 0, 1.0 / (f * 50.0));
        while t < 20.0 / f {
            time.push(t);
            i.push(dc + amp * (2.0 * std::f64::consts::PI * f * t).sin());
            t += if k % 3 == 0 { 0.6 * base } else { base };
            k += 1;
        }
        TranResult { time, signals: vec![Signal { name: "I(C1.C1)".into(), unit: "A", values: i }], warnings: vec![], retries: 0, rejected: 0 }
    }

    #[test]
    fn the_rms_of_the_ac_part_and_its_frequency_are_measured() {
        let tran = run(0.5, 0.3, 100e3);
        let s = sheet(Some(RippleRating { i_rms: 1.0, freq_hz: 100e3, coeffs: vec![] }), None);
        let r = &ripple_reports(&s, &tran, 25.0)[0];
        // a sine of 0.5 A peak is 0.354 A rms, and the DC part does not count
        assert!((r.i_rms - 0.5 / 2f64.sqrt()).abs() < 0.01, "{}", r.i_rms);
        assert!((r.freq - 100e3).abs() < 3e3, "{}", r.freq);
        assert_eq!(r.level, None);
        assert!(r.message().contains("35") && r.hint().is_empty());
    }

    #[test]
    fn the_table_lowers_the_allowed_current_at_low_frequency_and_over_means_over() {
        // 120 Hz ripple: a table that gives 0.5 at 120 Hz and 1.0 at 100 kHz halves what the part may carry
        let rating = RippleRating { i_rms: 1.0, freq_hz: 100e3, coeffs: vec![(120.0, 0.5), (1e3, 0.77), (10e3, 0.94), (100e3, 1.0)] };
        assert!((rating.allowed_at(100e3) - 1.0).abs() < 1e-9 && (rating.allowed_at(120.0) - 0.5).abs() < 1e-9);
        let mid = rating.allowed_at(3e3);
        assert!(mid > 0.77 && mid < 0.94, "{mid}");
        assert!((rating.allowed_at(10.0) - 0.5).abs() < 1e-9, "held at the table's end");
        let tran = run(0.9, 0.0, 120.0);
        let s = sheet(Some(rating), None);
        let r = &ripple_reports(&s, &tran, 25.0)[0];
        // 0.9 A peak = 0.64 A rms against 0.5 A allowed
        assert_eq!(r.level, Some(RatingLevel::Over), "{r:?}");
        assert!(r.message().contains("超えて") && !r.hint().is_empty());
    }

    #[test]
    fn heat_from_the_ripple_shortens_the_life() {
        let life = LifeRating { hours: 6000.0, temp_c: 105.0 };
        assert!((life.at(105.0) - 6000.0).abs() < 1e-9);
        assert!((life.at(95.0) - 12000.0).abs() < 1e-9, "10 degrees cooler: twice as long");
        assert!((life.at(115.0) - 3000.0).abs() < 1e-9);
        let tran = run(0.5 * 2f64.sqrt(), 0.0, 100e3); // 0.5 A rms
        let s = sheet(Some(RippleRating { i_rms: 0.5, freq_hz: 100e3, coeffs: vec![] }), Some(life.clone()));
        let r = &ripple_reports(&s, &tran, 25.0)[0];
        // at its rated ripple the part warms by the rated rise; at 25 degrees air it lasts 2^((105-35)/10) = 128 times 6000 h, capped
        assert!((r.core_temp_c - (25.0 + RIPPLE_HEATING_AT_RATED_C)).abs() < 0.5, "{}", r.core_temp_c);
        assert_eq!(r.life_hours, Some(MAX_LIFE_HOURS));
        assert!(r.life_text().unwrap().contains("上限"));
        // hot air and heavy ripple: 85 degrees ambient, 1.5 times the rated current
        let hot = ripple_reports(&s, &run(0.75 * 2f64.sqrt(), 0.0, 100e3), 85.0);
        assert!(hot[0].life_hours.unwrap() < 6000.0 * 1.2 && hot[0].level == Some(RatingLevel::Over));
        // a part without figures is not reported
        assert!(ripple_reports(&sheet(None, None), &tran, 25.0).is_empty());
    }
}
