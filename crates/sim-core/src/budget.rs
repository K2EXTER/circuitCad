//! Power budget (N2): every supply rail of the sheet at the DC operating point — what feeds it (a source or a regulator), what it
//! gives and to whom, the regulator's input current, heat and efficiency, and how much more it could still give ("あと何 mA") within
//! its rated current, its temperature limit and the rail that feeds it. The rails make a tree: battery → 5 V → 3.3 V.
//!
//! A rail is a net (not ground) that a voltage source's + terminal or a regulator's output pin drives. A regulator is a library part
//! with an input pin (`IN` / `VIN` / `VI`) and an output pin (`OUT` / `VOUT` / `VO`, or a pin of kind `PowerOut`). What each part
//! takes from a net is the signed sum over its elements (`netcurrent::part_currents`), so a load that is a whole IC model counts once.

use crate::circuit::Circuit;
use crate::dc::DcResult;
use sch_core::{extract, Component, Kind, PinKind, Schematic};

/// What draws from a rail, and how much (A).
#[derive(Clone, Debug, PartialEq)]
pub struct RailLoad {
    pub part: String,
    pub amps: f64,
    /// The load is the regulator of another rail (its input).
    pub feeds_rail: bool,
}

/// What stops a rail from giving more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The source's rated current (`PartRatings::i_max`).
    Rating,
    /// The regulator's temperature limit at its θ (a linear regulator turns (in − out) × I into heat).
    Heat,
    /// The rail that feeds this one has less to spare.
    Upstream,
}

impl Limit {
    pub fn label(self) -> &'static str {
        match self {
            Limit::Rating => "定格の電流",
            Limit::Heat => "発熱 (温度の上限)",
            Limit::Upstream => "上の電源の余裕",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rail {
    pub net: String,
    pub volts: f64,
    /// The part that drives it (a source or a regulator).
    pub source: String,
    pub regulator: bool,
    /// What the source gives into the rail (A).
    pub amps: f64,
    /// Largest first.
    pub loads: Vec<RailLoad>,
    /// The source's rated current (A), when its data gives one.
    pub rated: Option<f64>,
    /// The rail that feeds this one's regulator (index into the list), if that is a rail of the sheet.
    pub parent: Option<usize>,
    /// How deep in the tree (0 = fed by a source).
    pub depth: usize,
    /// A regulator's input: voltage (V) and current (A).
    pub input: Option<(f64, f64)>,
    /// What the regulator turns into heat (W): the sum of pin voltage × current into it.
    pub heat_w: Option<f64>,
    /// Output power over input power.
    pub efficiency: Option<f64>,
    /// The regulator's temperature (°C) at the given surroundings, from its θ (data sheet θJA or the package's guess).
    pub temp_c: Option<f64>,
    /// Its temperature limit (°C).
    pub t_max: Option<f64>,
    /// How much more current (A) the rail could give, and what stops it; None when nothing about the source says.
    pub headroom: Option<(f64, Limit)>,
}

impl Rail {
    /// The rail is near or over what it may give (less than a fifth of the most it could give to spare, or too hot).
    pub fn tight(&self) -> bool {
        let short = self.headroom.is_some_and(|(h, _)| h < 0.2 * (self.amps + h.max(0.0)).max(1e-3));
        let hot = matches!((self.temp_c, self.t_max), (Some(t), Some(m)) if t > m - 20.0);
        short || hot
    }
}

fn pin_names(c: &Component) -> Vec<(String, String, Option<PinKind>)> {
    c.custom.as_ref().map_or(vec![], |s| s.pins.iter().map(|p| (p.number.clone(), p.name.to_ascii_uppercase(), p.kind)).collect())
}

/// A regulator's (input pin, output pin) numbers.
pub fn regulator_pins(c: &Component) -> Option<(String, String)> {
    if c.kind != Kind::Custom {
        return None;
    }
    let pins = pin_names(c);
    let input = pins.iter().find(|(_, n, _)| matches!(n.as_str(), "IN" | "VIN" | "VI"))?;
    let output = pins.iter().find(|(_, n, k)| matches!(n.as_str(), "OUT" | "VOUT" | "VO") || *k == Some(PinKind::PowerOut))?;
    Some((input.0.clone(), output.0.clone()))
}

/// The supply rails of the sheet at the operating point `dc` of `circuit` (built from `sch`), parents before their children.
/// `ambient` is the surroundings' temperature (°C) for the regulators' heat.
pub fn power_budget(sch: &Schematic, circuit: &Circuit, dc: &DcResult, ambient: f64) -> Vec<Rail> {
    let flat = sch.flat();
    let s = &*flat;
    let nets = extract(s);
    let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone());
    let parts = crate::netcurrent::part_currents(circuit, dc);
    let node = |net: &str| circuit.names.iter().position(|n| n == net || (net == "GND" && n == "0"));
    // what part `r` gives into the net (A; negative = it takes)
    let into = |r: &str, net: &str| -> f64 { node(net).and_then(|k| parts.get(r).map(|v| v[k])).unwrap_or(0.0) };
    let volts = |net: &str| dc.voltage(net).unwrap_or(0.0);

    // the rails and their sources
    struct Found<'a> {
        net: String,
        comp: &'a Component,
        input: Option<String>,
    }
    let mut found: Vec<Found> = vec![];
    for c in &s.components {
        if c.sim_only && c.kind != Kind::VSource {
            continue;
        }
        let (out, input) = match c.kind {
            Kind::VSource => {
                let pins = c.pins();
                let (Some(p), Some(n)) = (net_of(&c.refdes, &pins[0].0), net_of(&c.refdes, &pins[1].0)) else { continue };
                if n != "GND" || p == "GND" {
                    continue;
                }
                (p, None)
            }
            Kind::Custom => {
                let Some((i, o)) = regulator_pins(c) else { continue };
                let Some(out) = net_of(&c.refdes, &o) else { continue };
                (out, net_of(&c.refdes, &i))
            }
            _ => continue,
        };
        if out == "GND" || found.iter().any(|f| f.net == out) || into(&c.refdes, &out) <= 1e-9 && c.kind != Kind::VSource {
            continue;
        }
        found.push(Found { net: out, comp: c, input });
    }

    let mut rails: Vec<Rail> = found
        .iter()
        .map(|f| {
            let c = f.comp;
            let amps = into(&c.refdes, &f.net).max(0.0);
            let mut loads: Vec<RailLoad> = parts
                .iter()
                .filter(|(r, _)| r.as_str() != c.refdes)
                .filter_map(|(r, _)| {
                    let a = -into(r, &f.net);
                    (a > 1e-9).then(|| RailLoad { part: r.clone(), amps: a, feeds_rail: found.iter().any(|g| g.comp.refdes == *r && g.input.as_deref() == Some(&f.net)) })
                })
                .collect();
            loads.sort_by(|a, b| b.amps.total_cmp(&a.amps));
            let rated = c.custom.as_ref().and_then(|s| s.ratings.as_ref()).and_then(|r| r.i_max).filter(|x| *x > 0.0);
            let regulator = c.kind == Kind::Custom;
            let (mut input, mut heat_w, mut efficiency, mut temp_c, mut t_max) = (None, None, None, None, None);
            if regulator {
                let pins = c.pins();
                let heat: f64 = pins.iter().filter_map(|(p, _)| net_of(&c.refdes, p)).map(|n| volts(&n) * -into(&c.refdes, &n)).sum::<f64>().max(0.0);
                heat_w = Some(heat);
                if let Some(i) = &f.input {
                    let (vin, iin) = (volts(i), -into(&c.refdes, i));
                    input = Some((vin, iin));
                    let pin = vin * iin;
                    efficiency = (pin > 1e-12).then(|| (volts(&f.net) * amps / pin).clamp(0.0, 1.0));
                }
                temp_c = Some(ambient + heat * crate::own_theta(c));
                t_max = Some(crate::temperature_limit(c, 1).t_max);
            }
            Rail { net: f.net.clone(), volts: volts(&f.net), source: c.refdes.clone(), regulator, amps, loads, rated, parent: None, depth: 0, input, heat_w, efficiency, temp_c, t_max, headroom: None }
        })
        .collect();

    // the tree: a regulator's input on another rail
    for (k, f) in found.iter().enumerate() {
        rails[k].parent = f.input.as_ref().and_then(|i| rails.iter().position(|r| r.net == *i)).filter(|&p| p != k);
    }
    // parents first (depth by walking up; a loop of rails, which a real circuit cannot have, stops at the length of the list)
    for k in 0..rails.len() {
        let (mut d, mut at) = (0, rails[k].parent);
        while let Some(p) = at {
            d += 1;
            at = if d > rails.len() { None } else { rails[p].parent };
        }
        rails[k].depth = d;
    }
    let mut order: Vec<usize> = vec![];
    fn visit(k: usize, rails: &[Rail], order: &mut Vec<usize>) {
        if order.contains(&k) {
            return;
        }
        order.push(k);
        let mut kids: Vec<usize> = (0..rails.len()).filter(|&c| rails[c].parent == Some(k) && c != k).collect();
        kids.sort_by(|a, b| rails[*b].volts.total_cmp(&rails[*a].volts));
        for c in kids {
            visit(c, rails, order);
        }
    }
    let mut roots: Vec<usize> = (0..rails.len()).filter(|&k| rails[k].parent.is_none()).collect();
    roots.sort_by(|a, b| rails[*b].volts.total_cmp(&rails[*a].volts));
    for r in roots {
        visit(r, &rails, &mut order);
    }
    for k in 0..rails.len() {
        visit(k, &rails, &mut order);
    }
    let old_to_new: Vec<usize> = (0..rails.len()).map(|k| order.iter().position(|&o| o == k).unwrap()).collect();
    let mut sorted: Vec<Rail> = order.iter().map(|&k| rails[k].clone()).collect();
    for r in &mut sorted {
        r.parent = r.parent.map(|p| old_to_new[p]);
    }

    // headroom, parents first: the rating, the heat a linear regulator makes with more current, and the parent's spare current (a linear
    // regulator draws its output current from its input)
    for k in 0..sorted.len() {
        let r = &sorted[k];
        let mut best: Option<(f64, Limit)> = None;
        let mut take = |v: f64, why: Limit| {
            if best.is_none_or(|(b, _)| v < b) {
                best = Some((v, why));
            }
        };
        if let Some(i) = r.rated {
            take(i - r.amps, Limit::Rating);
        }
        if let (Some((vin, _)), Some(heat), Some(t), Some(m)) = (r.input, r.heat_w, r.temp_c, r.t_max) {
            let drop = vin - r.volts;
            if drop > 1e-3 {
                let theta = if heat > 1e-9 { (t - ambient) / heat } else { 0.0 };
                if theta > 0.0 {
                    take(((m - ambient) / theta - heat) / drop, Limit::Heat);
                }
            }
        }
        if let Some((h, _)) = r.parent.and_then(|p| sorted[p].headroom) {
            take(h, Limit::Upstream);
        }
        sorted[k].headroom = best;
    }
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::Wire;

    /// A battery of 9 V, the L78L05 (rated 100 mA; θ 200 K/W and 125 °C here) and a load resistor on its output.
    fn sheet(load_ohms: &str) -> Schematic {
        let def: part_lib::PartDef = serde_json::from_str(part_lib::EXAMPLES.iter().find(|(n, _)| *n == "L78L05_TO-92").unwrap().1).unwrap();
        let mut reg = def.symbol();
        let r = reg.ratings.get_or_insert_with(Default::default);
        (r.theta_ja, r.t_max) = (Some(200.0), Some(125.0));
        let mut s = Schematic::default();
        let v = s.add(Kind::VSource, (0, 20), 0);
        s.components[v].value = "9".into();
        let u = s.add_custom(reg, "L78L05", "TO-92", (20, 20), 0);
        let r = s.add(Kind::Resistor, (40, 20), 1);
        s.components[r].value = load_ohms.into();
        let g = s.add(Kind::Ground, (20, 40), 0);
        let link = |s: &mut Schematic, a: (usize, usize), b: (usize, usize)| {
            let pa = s.components[a.0].pins()[a.1].1;
            let pb = s.components[b.0].pins()[b.1].1;
            s.wires.push(Wire { a: pa, b: (pb.0, pa.1) });
            s.wires.push(Wire { a: (pb.0, pa.1), b: pb });
        };
        // the L78L05's pins by number: 3 IN, 1 OUT, 2 GND
        let at = |s: &Schematic, n: &str| s.components[u].pins().iter().position(|(m, _)| m == n).unwrap();
        let (pin_in, pin_out, pin_gnd) = (at(&s, "3"), at(&s, "1"), at(&s, "2"));
        link(&mut s, (v, 0), (u, pin_in));
        link(&mut s, (u, pin_out), (r, 0));
        for (c, p) in [(v, 1), (u, pin_gnd), (r, 1)] {
            link(&mut s, (c, p), (g, 0));
        }
        s
    }

    fn budget(s: &Schematic) -> Vec<Rail> {
        let c = crate::from_schematic(s).unwrap();
        let dc = crate::dc_op(&c).unwrap();
        power_budget(s, &c, &dc, 25.0)
    }

    #[test]
    fn a_battery_feeds_a_regulator_and_the_regulator_its_load() {
        let rails = budget(&sheet("100"));
        assert_eq!(rails.len(), 2, "{rails:?}");
        let (top, out) = (&rails[0], &rails[1]);
        assert_eq!((top.source.as_str(), out.source.as_str(), out.parent, out.depth), ("V1", "U1", Some(0), 1));
        assert!((out.volts - 5.0).abs() < 0.1, "{}", out.volts);
        let i = out.volts / 100.0;
        assert!((out.amps - i).abs() < 1e-3, "{}", out.amps);
        assert_eq!(out.loads[0].part, "R1");
        // the battery gives the load's current and the regulator's own few mA, all of it to the regulator
        assert!(top.amps > i + 1e-3 && top.amps < i + 6e-3, "{}", top.amps);
        assert!(top.loads[0].feeds_rail && top.loads[0].part == "U1");
        let (vin, iin) = out.input.unwrap();
        assert!((vin - 9.0).abs() < 0.01 && (iin - top.amps).abs() < 1e-6);
        // heat: (9 − 5) V × 50 mA + 9 V × the quiescent current; efficiency = 0.25 W over what goes in
        let heat = out.heat_w.unwrap();
        assert!((heat - (vin * iin - out.volts * out.amps)).abs() < 1e-3, "{heat}");
        assert!(heat > 0.2 && heat < 0.26, "{heat}");
        assert!((out.efficiency.unwrap() - out.volts * out.amps / (vin * iin)).abs() < 1e-6);
        // 25 °C + heat × 200 K/W; the heat allows (100/200 − heat)/4 ≈ 65 mA more, the rating 50 mA more: the rating stops it
        assert!((out.temp_c.unwrap() - (25.0 + heat * 200.0)).abs() < 1e-6);
        let (h, why) = out.headroom.unwrap();
        assert!((h - (0.1 - out.amps)).abs() < 1e-6 && why == Limit::Rating, "{h} {why:?}");
        assert!(!out.tight());
        assert!(top.headroom.is_none(), "an ideal source says nothing of its limit");
    }

    #[test]
    fn a_heavy_load_is_tight() {
        // about 80 mA of the 100 mA: under a fifth left
        let rails = budget(&sheet("62"));
        let out = &rails[1];
        let (h, why) = out.headroom.unwrap();
        assert!(h < 0.021 && why == Limit::Rating, "{h} {why:?}");
        assert!(out.tight());
        // with a hotter part (θ 600 K/W) the heat stops it first
        let mut s = sheet("100");
        let u = s.components.iter_mut().find(|c| c.refdes == "U1").unwrap();
        u.theta = 600.0;
        let (_, why) = budget(&s)[1].headroom.unwrap();
        assert_eq!(why, Limit::Heat);
    }
}
