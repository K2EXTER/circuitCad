//! Inrush current (T3): what flows the moment the supply is switched on, while the capacitors charge from nothing. Each supply (a
//! voltage source whose − is ground, with a DC value) gets the resistance of the real source and its wiring in series and rises from
//! 0 V in `T_RISE`, as a switch closing would; a transient run from all-zero state then gives each supply's peak current, its I²t
//! (what heats a fuse and the contacts), how long the surge lasts, and the peak current of every part on the way, against its
//! rated current. A supply that does not come back to its DC current afterwards has blown a fuse (or latched something).

use crate::circuit::{Circuit, Element};
use crate::waveform::Waveform;
use sch_core::{extract, Kind, Schematic};

/// How long the switch takes to close: the supply rises from 0 V over this time.
pub const T_RISE: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
pub struct InrushSettings {
    /// The resistance of the source and its wiring (Ω): an AC adapter and its cable about 0.1–0.5 Ω, a lab supply's leads 0.05 Ω.
    pub r_source: f64,
    /// How long to watch (s).
    pub tstop: f64,
}

impl Default for InrushSettings {
    fn default() -> Self {
        Self { r_source: 0.1, tstop: 20e-3 }
    }
}

/// One part's current during the surge.
#[derive(Clone, Debug, PartialEq)]
pub struct PartPeak {
    pub part: String,
    /// The largest current through any of its pins (A).
    pub peak: f64,
    /// Its rated current (A), from its data.
    pub rated: Option<f64>,
}

impl PartPeak {
    pub fn over(&self) -> bool {
        self.rated.is_some_and(|r| self.peak > r)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inrush {
    pub source: String,
    pub volts: f64,
    /// Largest current out of the supply (A) and when (s).
    pub peak: f64,
    pub t_peak: f64,
    /// ∫ i² dt over the run (A²s).
    pub i2t: f64,
    /// The supply's current once everything has settled (the DC operating point, A).
    pub steady: f64,
    /// The last time the current was above twice the steady current and a tenth of the peak (s): how long the surge lasts.
    pub surge_s: f64,
    /// The current at the end of the run (A).
    pub end: f64,
    /// Parts that carry at least 5 % of the peak, largest first.
    pub parts: Vec<PartPeak>,
    /// The supply current over time (s, A), for the plot.
    pub wave: (Vec<f64>, Vec<f64>),
}

impl Inrush {
    /// The current did not come back to its DC value: a fuse blew, or something latched.
    pub fn did_not_recover(&self) -> bool {
        self.steady > 1e-3 && self.end < 0.5 * self.steady
    }

    /// The peak is far above the steady current (and large in itself): worth limiting.
    pub fn large(&self) -> bool {
        self.peak > 1.0 && self.peak > 10.0 * self.steady.max(1e-3)
    }

    /// The series resistance (Ω, an NTC's cold resistance) that would bring the peak down to `target` amps.
    pub fn limit_ohms(&self, target: f64, r_source: f64) -> f64 {
        (self.volts / target.max(1e-3) - r_source).max(0.0)
    }

    /// The current to aim the limited peak at: the smallest rating on the way, else ten times the steady current (at least 1 A).
    pub fn target(&self) -> f64 {
        self.parts.iter().filter_map(|p| p.rated).fold(f64::INFINITY, f64::min).min((10.0 * self.steady).max(1.0))
    }
}

/// The supplies of the sheet: (refdes, DC volts) of the voltage sources whose − is ground.
fn supplies(sch: &Schematic, c: &Circuit) -> Vec<(String, f64)> {
    let nets = extract(sch);
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone());
    sch.components
        .iter()
        .filter(|x| x.kind == Kind::VSource)
        .filter(|x| net_of(&x.refdes, "-").as_deref() == Some("GND") && net_of(&x.refdes, "+").is_some_and(|n| n != "GND"))
        .filter_map(|x| {
            c.elements.iter().find_map(|e| match e {
                Element::VSource { name, wave: Waveform::Dc(v), .. } if *name == x.refdes && *v != 0.0 => Some((name.clone(), *v)),
                _ => None,
            })
        })
        .collect()
}

/// The circuit with each supply behind `r_source` and rising from 0 V.
fn switched_on(c: &Circuit, list: &[(String, f64)], r_source: f64) -> Circuit {
    let mut on = c.clone();
    for (name, v) in list {
        let Some(k) = on.elements.iter().position(|e| matches!(e, Element::VSource { name: n, .. } if n == name)) else { continue };
        let Element::VSource { p, .. } = on.elements[k].clone() else { continue };
        let inner = on.node(&format!("{name}.src"));
        if let Element::VSource { p: pp, wave, .. } = &mut on.elements[k] {
            *pp = inner;
            *wave = Waveform::Pwl(vec![(0.0, 0.0), (T_RISE, *v)]);
        }
        on.add(Element::Resistor { name: format!("{name}.Rsrc"), a: inner, b: p, r: r_source.max(1e-4) });
    }
    on
}

/// The supplies' inrush at power-on.
pub fn inrush(sch: &Schematic, c: &Circuit, s: &InrushSettings) -> Result<Vec<Inrush>, String> {
    let flat = sch.flat();
    let list = supplies(&flat, c);
    if list.is_empty() {
        return Err("電源 (− が GND で、値が直流の電圧源) がありません。突入電流は、電源を入れた瞬間に電源から流れる電流です".into());
    }
    if !(s.tstop > 0.0) || !(s.r_source >= 0.0) {
        return Err("見る時間は 0 より大きく、電源の抵抗は 0 以上にしてください".into());
    }
    let dc = crate::dc_op(c).map_err(|e| format!("DC動作点を解けませんでした: {e}"))?;
    let on = switched_on(c, &list, s.r_source);
    let tr = crate::transient(&on, &crate::TranSettings { tstop: s.tstop, tstep: crate::auto_tstep(s.tstop), uic: true }).map_err(|e| format!("電源投入の過渡解析ができませんでした: {e}"))?;
    let n = tr.time.len();
    // per part, the largest pin current over time: what each element puts into each node, summed per part (as `netcurrent` does)
    let mut per_part: std::collections::BTreeMap<String, std::collections::BTreeMap<usize, Vec<f64>>> = Default::default();
    for e in &on.elements {
        let Some(sig) = tr.signal(&format!("I({})", e.name())) else { continue };
        let part = e.name().split('.').next().unwrap_or(e.name()).to_string();
        let into = per_part.entry(part).or_default();
        for k in 0..n {
            crate::netcurrent::terminals_of(e, sig.values[k], |node, i| into.entry(node).or_insert_with(|| vec![0.0; n])[k] += i);
        }
    }
    let peak_of = |part: &str| -> f64 { per_part.get(part).map_or(0.0, |m| m.iter().filter(|(node, _)| **node != 0).flat_map(|(_, v)| v.iter()).fold(0.0f64, |a, x| a.max(x.abs()))) };
    let rated = |part: &str| sch.components.iter().find(|x| x.refdes == part).and_then(|x| x.custom.as_ref()).and_then(|y| y.ratings.as_ref()).and_then(|r| r.i_max).filter(|i| *i > 0.0);

    let mut out = vec![];
    for (name, volts) in &list {
        let Some(sig) = tr.signal(&format!("I({name})")) else { continue };
        // the current out of + into the circuit: a source's current runs from + through it to −, so it is the negative of I(V)
        let i: Vec<f64> = sig.values.iter().map(|x| -x).collect();
        let (mut peak, mut t_peak) = (0.0f64, 0.0);
        for (x, t) in i.iter().zip(&tr.time) {
            if x.abs() > peak {
                (peak, t_peak) = (x.abs(), *t);
            }
        }
        let i2t: f64 = (1..n).map(|k| 0.5 * (i[k] * i[k] + i[k - 1] * i[k - 1]) * (tr.time[k] - tr.time[k - 1])).sum();
        let steady = dc.element(name).map_or(0.0, |r| r.current.abs());
        let level = (2.0 * steady).max(0.1 * peak);
        let surge_s = (0..n).rev().find(|&k| i[k].abs() > level).map_or(0.0, |k| tr.time[k]);
        let end = i.last().map_or(0.0, |x| x.abs());
        let mut parts: Vec<PartPeak> = per_part
            .keys()
            .filter(|p| *p != name && sch.components.iter().any(|x| x.refdes == **p))
            .map(|p| PartPeak { part: p.clone(), peak: peak_of(p), rated: rated(p) })
            .filter(|p| p.peak >= 0.05 * peak)
            .collect();
        parts.sort_by(|a, b| b.peak.total_cmp(&a.peak));
        out.push(Inrush { source: name.clone(), volts: *volts, peak, t_peak, i2t, steady, surge_s, end, parts, wave: (tr.time.clone(), i) });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::Wire;

    /// 12 V into 470 µF (and a 100 Ω load) through a part rated 1 A.
    fn sheet() -> Schematic {
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 20), 0);
        s.components[v].value = "12".into();
        let c = s.add(Kind::Capacitor, (30, 20), 1);
        s.components[c].value = "470u".into();
        let r = s.add(Kind::Resistor, (40, 20), 1);
        s.components[r].value = "100".into();
        let g = s.add(Kind::Ground, (20, 40), 0);
        let link = |s: &mut Schematic, a: (usize, usize), b: (usize, usize)| {
            let pa = s.components[a.0].pins()[a.1].1;
            let pb = s.components[b.0].pins()[b.1].1;
            s.wires.push(Wire { a: pa, b: (pb.0, pa.1) });
            s.wires.push(Wire { a: (pb.0, pa.1), b: pb });
        };
        link(&mut s, (v, 0), (c, 0));
        link(&mut s, (c, 0), (r, 0));
        for (x, p) in [(v, 1), (c, 1), (r, 1)] {
            link(&mut s, (x, p), (g, 0));
        }
        s
    }

    fn run(s: &Schematic, r_source: f64) -> Inrush {
        let c = crate::from_schematic(s).unwrap();
        inrush(s, &c, &InrushSettings { r_source, tstop: 0.2 }).unwrap().remove(0)
    }

    #[test]
    fn a_big_capacitor_draws_v_over_r_and_half_c_v_squared_over_r() {
        let x = run(&sheet(), 0.1);
        // the peak is 12 V over 0.1 Ω (the capacitor starts at zero); the charge takes I²t = C V² / 2R = 0.338 A²s, the load adds a little
        assert!((x.peak - 120.0).abs() < 6.0, "{}", x.peak);
        assert!(x.t_peak < 1e-4, "{}", x.t_peak);
        assert!((x.i2t - 0.338).abs() < 0.03, "{}", x.i2t);
        assert!((x.steady - 0.12).abs() < 1e-3, "{}", x.steady);
        assert!(x.large() && !x.did_not_recover());
        // τ = 0.1 Ω × 470 µF = 47 µs: over within a fraction of a millisecond
        assert!(x.surge_s < 1e-3, "{}", x.surge_s);
        assert_eq!(x.parts[0].part, "C1");
        // the resistance that keeps the peak at 1.2 A (ten times the steady current): 10 Ω − the source's 0.1 Ω
        assert!((x.target() - 1.2).abs() < 0.01 && (x.limit_ohms(x.target(), 0.1) - 9.9).abs() < 0.05);
        // a softer source (1 Ω): a tenth of the peak and of the I²t
        let soft = run(&sheet(), 1.0);
        assert!((soft.peak - 12.0).abs() < 0.6 && (soft.i2t - 0.034).abs() < 0.005, "{} {}", soft.peak, soft.i2t);
    }
}
