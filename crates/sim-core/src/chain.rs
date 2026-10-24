//! Chain failure: break one part, run the transient analysis, find the part whose peak current or
//! mean power goes past its rating, let that part fail the way such parts usually do (resistors and
//! LEDs open, semiconductors and capacitors short), and repeat. The list of steps is the story a
//! learner needs: "the coil's layers short, the current climbs, Q1 burns, D1 sees the full supply...".

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;
use crate::fault::{main_pins, with_fault, Fault};
use crate::ratings::package_power_limit;
use crate::tran::{transient, TranResult, TranSettings};
use crate::value::format_value;
use sch_core::{Component, Kind, Schematic};

/// What a coil without a rated power is assumed to shed (W).
const COIL_W: f64 = 1.0;

/// The most parts that fail one after another before the analysis stops.
pub const MAX_CHAIN: usize = 5;

#[derive(Clone, Debug)]
pub struct ChainStep {
    pub part: String,
    /// What happens to the part ("断線", "ショート", "巻線のレアショート (20% の巻数)").
    pub what: String,
    /// Why it happens, in words (the stress that exceeded the rating); the first step has the fault itself.
    pub why: String,
    pub fault: Fault,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChainEnd {
    /// No part is past its rating any more: the chain stops here.
    Settled,
    /// The broken circuit cannot be simulated: runaway current or no stable state.
    Unsolvable,
    /// `MAX_CHAIN` parts failed and it was still going.
    Cut,
}

#[derive(Clone, Debug)]
pub struct ChainReport {
    pub steps: Vec<ChainStep>,
    pub end: ChainEnd,
    /// Peak current of the first DC supply: (name, healthy, at the end). A fuse would have to stop this.
    pub supply: Option<(String, f64, f64)>,
    /// For a layer short: what circulates in the shorted turns.
    pub loop_heat: Option<LoopHeat>,
}

/// The shorted turns of a coil are a one-turn-like secondary closed on itself: a large circulating
/// current in a short piece of the winding.
#[derive(Clone, Debug)]
pub struct LoopHeat {
    pub coil: String,
    /// RMS current in the coil (the rest of the winding).
    pub i_coil: f64,
    /// RMS current circulating in the shorted turns: `(1 - x) / x` times the coil's.
    pub i_loop: f64,
    /// Heat in the shorted turns (`x` of the winding resistance), when the coil has a `DCR`.
    pub p_loop: Option<f64>,
    /// Heat in the whole healthy winding, for comparison.
    pub p_healthy: Option<f64>,
}

/// The shorted turns burn out when they dissipate more than this (W): they are a short piece of thin wire.
const LOOP_BURN_W: f64 = 0.25;

/// A part past its rating in a transient run.
struct Over {
    refdes: String,
    /// Peak or mean figure over the rating (> 1 means past it): which part goes first.
    ratio: f64,
    why: String,
}

fn window_of(set: &TranSettings) -> (f64, f64) {
    (set.tstop * 0.5, set.tstop)
}

fn node_wave<'a>(c: &Circuit, tr: &'a TranResult, node: usize) -> Option<&'a [f64]> {
    if node == 0 {
        return None;
    }
    tr.signal(&format!("V({})", c.names[node])).map(|s| s.values.as_slice())
}

/// Time average of `f(i)` over the window, by the trapezoid rule on the uneven time steps.
fn time_mean(t: &[f64], w: (f64, f64), f: impl Fn(usize) -> f64) -> Option<f64> {
    let (mut sum, mut span) = (0.0, 0.0);
    for k in 1..t.len() {
        if t[k - 1] < w.0 || t[k] > w.1 {
            continue;
        }
        let dt = t[k] - t[k - 1];
        sum += 0.5 * (f(k - 1) + f(k)) * dt;
        span += dt;
    }
    (span > 0.0).then(|| sum / span)
}

fn in_window(t: &[f64], w: (f64, f64)) -> impl Iterator<Item = usize> + '_ {
    (0..t.len()).filter(move |&k| t[k] >= w.0 && t[k] <= w.1)
}

fn watts(p: f64) -> String {
    if p >= 1.0 { format!("{p:.1} W") } else { format!("{:.0} mW", p * 1e3) }
}

fn amps(i: f64) -> String {
    format!("{}A", format_value((i * 1e3).round() / 1e3))
}

/// Parts of `sch` that are past their rating in `tr` (peak current, peak reverse voltage, mean power).
fn overs(sch: &Schematic, c: &Circuit, tr: &TranResult, w: (f64, f64)) -> Vec<Over> {
    let mut out = vec![];
    for comp in sch.components.iter().filter(|x| !x.kind.is_power_symbol()) {
        let r = comp.refdes.as_str();
        let Some(el) = c.elements.iter().find(|e| e.name() == r) else { continue };
        let Some(cur) = tr.signal(&format!("I({r})")) else { continue };
        let ratings = comp.custom.as_ref().and_then(|s| s.ratings.as_ref());
        let peak = in_window(&tr.time, w).map(|k| cur.values[k].abs()).fold(0.0, f64::max);
        if let Some(imax) = ratings.and_then(|x| x.i_max)
            && peak > imax {
                out.push(Over { refdes: r.into(), ratio: peak / imax, why: format!("{r} のピーク電流 {} が最大定格 {} を超えた", amps(peak), amps(imax)) });
                continue;
            }
        if comp.kind == Kind::Led && peak > 0.03 {
            out.push(Over { refdes: r.into(), ratio: peak / 0.03, why: format!("{r} (LED) のピーク電流 {} が上限の目安 30 mA を超えた", amps(peak)) });
            continue;
        }
        let Some((a, b)) = main_pins(el) else { continue };
        let (va, vb) = (node_wave(c, tr, a), node_wave(c, tr, b));
        let volt = |k: usize| va.map_or(0.0, |v| v[k]) - vb.map_or(0.0, |v| v[k]);
        if let (Element::Diode { .. }, Some(vmax)) = (el, ratings.and_then(|x| x.v_max)) {
            let rev = in_window(&tr.time, w).map(|k| -volt(k)).fold(0.0, f64::max);
            if rev > vmax {
                out.push(Over { refdes: r.into(), ratio: rev / vmax, why: format!("{r} にかかる逆電圧 {rev:.0} V が耐圧 {vmax:.0} V を超えた") });
                continue;
            }
        }
        if comp.kind == Kind::Inductor {
            // heat in the winding resistance; a coil without one has no loss to count
            let Some(Element::Resistor { r: dcr, .. }) = c.elements.iter().find(|e| e.name() == format!("{r}.DCR")) else { continue };
            let limit = ratings.and_then(|x| x.p_max).or_else(|| package_power_limit(&comp.footprint_name())).unwrap_or(COIL_W);
            let Some(p) = time_mean(&tr.time, w, |k| cur.values[k] * cur.values[k] * dcr) else { continue };
            if p > limit {
                out.push(Over { refdes: r.into(), ratio: p / limit, why: format!("{r} (コイル) の巻線の発熱 {} が目安 {} を超えた", watts(p), watts(limit)) });
            }
            continue;
        }
        if !matches!(comp.kind, Kind::Resistor | Kind::Npn | Kind::Diode | Kind::Custom) {
            continue;
        }
        let limit = ratings.and_then(|x| x.p_max).or_else(|| package_power_limit(&comp.footprint_name()));
        let (Some(limit), Some(p)) = (limit, time_mean(&tr.time, w, |k| cur.values[k] * volt(k))) else { continue };
        if p > limit {
            out.push(Over { refdes: r.into(), ratio: p / limit, why: format!("{r} の平均消費電力 {} が定格・目安 {} を超えた", watts(p), watts(limit)) });
        }
    }
    out
}

/// How a part that was pushed past its rating usually dies.
fn failure_of(comp: &Component, el: &Element) -> Option<Fault> {
    match el {
        Element::Resistor { .. } => Some(Fault::Open),
        Element::Diode { .. } if comp.kind == Kind::Led => Some(Fault::Open),
        Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. } | Element::Capacitor { .. } => Some(Fault::Short),
        Element::Inductor { .. } => Some(Fault::Open),
        _ => None,
    }
}

fn supply_peak(c: &Circuit, tr: &TranResult, w: (f64, f64)) -> Option<(String, f64)> {
    let name = c.elements.iter().find_map(|e| match e {
        Element::VSource { name, wave: crate::waveform::Waveform::Dc(v), .. } if *v != 0.0 && !name.contains('.') => Some(name.clone()),
        _ => None,
    })?;
    let cur = tr.signal(&format!("I({name})"))?;
    Some((name, in_window(&tr.time, w).map(|k| cur.values[k].abs()).fold(0.0, f64::max)))
}

/// Breaks `element` of `c` (made from `sch`) as `fault` and follows the failures that come after.
pub fn chain_failure(sch: &Schematic, c: &Circuit, element: &str, fault: Fault, set: &TranSettings) -> Result<ChainReport, SimError> {
    let sch = &*sch.flat();
    let w = window_of(set);
    let mut cur = with_fault(c, element, fault).ok_or_else(|| SimError::Invalid(format!("{element} には「{}」の故障を設定できません", fault.label())))?;
    let healthy = transient(c, set)?;
    // what was already past its rating before anything broke is not the fault's doing
    let mut failed: Vec<String> = overs(sch, c, &healthy, w).into_iter().map(|o| o.refdes).collect();
    failed.push(element.to_string());
    let supply0 = supply_peak(c, &healthy, w);
    let mut rep = ChainReport {
        steps: vec![ChainStep { part: element.into(), what: fault.label(), why: "最初に壊した部品".into(), fault }],
        end: ChainEnd::Settled,
        supply: None,
        loop_heat: None,
    };
    let mut burn: Option<String> = None;
    loop {
        let tr = match transient(&cur, set) {
            Ok(t) => t,
            Err(_) => {
                rep.end = ChainEnd::Unsolvable;
                break;
            }
        };
        if let (Fault::TurnShort(x), None) = (fault, &rep.loop_heat) {
            let rms = |tr: &TranResult| {
                let s = tr.signal(&format!("I({element})"))?;
                time_mean(&tr.time, w, |k| s.values[k] * s.values[k]).map(f64::sqrt)
            };
            if let (Some(i_coil), Some(i_h)) = (rms(&tr), rms(&healthy)) {
                let dcr = c.elements.iter().find_map(|e| match e {
                    Element::Resistor { name, r, .. } if *name == format!("{element}.DCR") => Some(*r),
                    _ => None,
                });
                let i_loop = i_coil * (1.0 - x) / x;
                let heat = LoopHeat { coil: element.into(), i_coil, i_loop, p_loop: dcr.map(|r| i_loop * i_loop * x * r), p_healthy: dcr.map(|r| i_h * i_h * r) };
                // electrical overloads happen at once; the shorted turns take seconds to cook, so they go last
                if heat.p_loop.is_some_and(|p| p > LOOP_BURN_W) {
                    burn = Some(format!("ショートした巻線の発熱 {} が目安 {} を超えた", watts(heat.p_loop.unwrap_or(0.0)), watts(LOOP_BURN_W)));
                }
                rep.loop_heat = Some(heat);
            }
        }
        // the highest the supply current gets on the way: it is what a fuse has to cut, even if the chain later ends in an open coil
        if let Some(((n, h), (_, f))) = supply0.as_ref().zip(supply_peak(&cur, &tr, w)) {
            let top = rep.supply.as_ref().map_or(f, |(_, _, prev)| prev.max(f));
            rep.supply = Some((n.clone(), *h, top));
        }
        let mut over = overs(sch, &cur, &tr, w);
        over.retain(|o| !failed.contains(&o.refdes));
        over.sort_by(|a, b| b.ratio.total_cmp(&a.ratio));
        // the worst offender goes first; a part with no way to fail (an IC) is skipped
        let next = over.into_iter().find_map(|o| {
            let comp = sch.components.iter().find(|x| x.refdes == o.refdes)?;
            let el = cur.elements.iter().find(|e| e.name() == o.refdes)?;
            let f = failure_of(comp, el)?;
            Some((o, f))
        });
        let Some((o, f)) = next else {
            // nothing else is past its rating: the shorted turns finally burn through
            if let Some(why) = burn.take()
                && let Some(broken) = with_fault(&cur, element, Fault::Open) {
                    rep.steps.push(ChainStep { part: element.into(), what: "焼損して断線".into(), why, fault: Fault::Open });
                    cur = broken;
                    continue;
                }
            break;
        };
        if rep.steps.len() > MAX_CHAIN {
            rep.end = ChainEnd::Cut;
            break;
        }
        let Some(broken) = with_fault(&cur, &o.refdes, f) else { break };
        failed.push(o.refdes.clone());
        rep.steps.push(ChainStep { part: o.refdes, what: if f == Fault::Open { "過熱・過電流で断線".into() } else { "破損してショート".into() }, why: o.why, fault: f });
        cur = broken;
    }
    Ok(rep)
}

impl ChainReport {
    /// The story in plain Japanese: each step, how it ends, and what would have stopped it.
    pub fn summary(&self) -> Vec<String> {
        let first = &self.steps[0];
        let mut out = vec![format!("{} が「{}」になると、次の順に壊れていきます:", first.part, first.what)];
        if let Some(h) = &self.loop_heat {
            let mut l = format!("  ・{} のショートした巻線には、コイル電流 {} の {:.1} 倍の {} が回ります(ショートした輪は変圧器の二次側と同じ働きをします)", h.coil, amps(h.i_coil), h.i_loop / h.i_coil.max(1e-12), amps(h.i_loop));
            if let (Some(p), Some(p0)) = (h.p_loop, h.p_healthy) {
                l += &format!("。その部分の発熱は {} で、正常なコイル全体の {} と比べて、細い線の一部に熱が集まります", watts(p), watts(p0));
            }
            out.push(l);
        }
        for (k, s) in self.steps.iter().enumerate().skip(1) {
            out.push(format!("  {k}. {} → {}: {}", s.part, s.what, s.why));
        }
        out.push(match self.end {
            ChainEnd::Settled if self.steps.len() == 1 => "連鎖は起きません: ほかの部品は定格内に収まります(動作は変わるので、波形で確かめましょう)".into(),
            ChainEnd::Settled => format!("最初の故障を含めて {} 個壊れたところで、定格を超える部品がなくなり、連鎖が止まります", self.steps.len()),
            ChainEnd::Unsolvable => "このあと回路は計算できなくなります。実機では大電流が流れ続け、電源や配線まで傷むおそれがあります".into(),
            ChainEnd::Cut => format!("{MAX_CHAIN} 個壊れてもまだ続きます。ここで打ち切りました"),
        });
        if let Some((n, h, f)) = &self.supply
            && *f > 1.5 * h && *f > 0.05 {
                out.push(format!("電源 {n} のピーク電流は {} → {} になります。電源の直後にヒューズを入れると、連鎖が広がる前に切れて止められます", amps(*h), amps(*f)));
            }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buck() -> (Schematic, Circuit) {
        let s: Schematic = serde_json::from_str(include_str!("../../../docs/examples/buck_converter.schjson")).unwrap();
        let c = crate::from_schematic(&s).unwrap();
        (s, c)
    }

    fn coil() -> (Schematic, Circuit) {
        let s: Schematic = serde_json::from_str(include_str!("../../../docs/examples/coil_layer_short.schjson")).unwrap();
        let c = crate::from_schematic(&s).unwrap();
        (s, c)
    }

    fn parts(r: &ChainReport) -> Vec<(&str, Fault)> {
        r.steps.iter().map(|s| (s.part.as_str(), s.fault)).collect()
    }

    #[test]
    fn a_layer_short_changes_inductance_and_winding_resistance() {
        let (_, c) = coil();
        let b = with_fault(&c, "L1", Fault::TurnShort(0.5)).unwrap();
        let l = |c: &Circuit| c.elements.iter().find_map(|e| match e { Element::Inductor { name, l, .. } if name == "L1" => Some(*l), _ => None }).unwrap();
        let dcr = |c: &Circuit| c.elements.iter().find_map(|e| match e { Element::Resistor { name, r, .. } if name == "L1.DCR" => Some(*r), _ => None }).unwrap();
        assert!((l(&b) / l(&c) - 0.25).abs() < 1e-9 && (dcr(&b) / dcr(&c) - 0.5).abs() < 1e-9);
        // a resistor has no turns to short
        assert!(with_fault(&c, "R1", Fault::TurnShort(0.5)).is_none());
    }

    /// Half the turns shorted: the coil takes 1.4x the current, the driver's 200 mA limit goes first,
    /// then the shorted turns cook. Only 10% shorted: the turns burn without touching the driver.
    #[test]
    fn the_driver_dies_before_the_shorted_turns_burn_and_a_small_short_burns_alone() {
        let (s, c) = coil();
        let set = TranSettings { tstop: 10e-3, tstep: 5e-6, uic: false };
        let big = chain_failure(&s, &c, "L1", Fault::TurnShort(0.5), &set).unwrap();
        assert_eq!(parts(&big), vec![("L1", Fault::TurnShort(0.5)), ("Q1", Fault::Short), ("L1", Fault::Open)], "{:?}", big.summary());
        assert_eq!(big.end, ChainEnd::Settled);
        let text = big.summary().join("
");
        assert!(text.contains("Q1") && text.contains("最大定格") && text.contains("焼損"), "{text}");
        let small = chain_failure(&s, &c, "L1", Fault::TurnShort(0.1), &set).unwrap();
        assert_eq!(parts(&small), vec![("L1", Fault::TurnShort(0.1)), ("L1", Fault::Open)], "{:?}", small.summary());
        let h = small.loop_heat.unwrap();
        assert!((h.i_loop / h.i_coil - 9.0).abs() < 1e-6, "{h:?}");
    }

    #[test]
    fn a_healthy_stress_or_a_regulated_circuit_does_not_start_a_chain() {
        let (s, c) = buck();
        let set = TranSettings { tstop: 2e-3, tstep: 100e-9, uic: false };
        let r = chain_failure(&s, &c, "L1", Fault::TurnShort(0.2), &set).unwrap();
        assert_eq!(parts(&r).len(), 1, "{:?}", r.summary());
        assert!(r.summary().iter().any(|l| l.contains("連鎖は起きません")));
    }
}
