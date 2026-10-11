//! Fault simulation: break one part (open, short, wrong value) and compare the DC operating
//! point with the healthy circuit, so a learner sees what a failure does to the voltages.

use crate::circuit::{Circuit, Element};
use crate::waveform::Waveform;
use crate::ac::{ac_analysis, AcSettings};
use crate::dc::{dc_op, DcResult, SimError};
use crate::montecarlo::minus_3db;
use crate::measure::{wave_measure, WaveMeasure};
use crate::spectrum::stats;
use crate::tran::{transient, TranSettings};
use crate::value::format_value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    /// Disconnected: the part is removed.
    Open,
    /// The part's main terminals joined by a tiny resistance.
    Short,
    /// Value multiplied by this factor (R, C, L; a DC source's voltage).
    Scale(f64),
    /// Connected the wrong way round (diode, transistor: the two main terminals swapped).
    Reverse,
    /// A coil whose insulation broke between layers: this fraction of its turns is shorted. The
    /// rest has `(1 - x)^2` of the inductance (the shorted turns cancel the flux) and `1 - x` of the
    /// winding resistance, and the shorted turns carry `(1 - x) / x` times the coil current.
    TurnShort(f64),
}

impl Fault {
    pub fn label(self) -> String {
        match self {
            Fault::Open => "断線 (オープン)".into(),
            Fault::Short => "ショート".into(),
            Fault::Scale(k) if k > 1.0 => format!("値が {} 倍", format_value(k)),
            Fault::Scale(k) => format!("値が 1/{}", format_value(1.0 / k)),
            Fault::Reverse => "逆向きにつなぐ".into(),
            Fault::TurnShort(x) => format!("巻線のレアショート ({:.0}% の巻数)", x * 100.0),
        }
    }
}

/// Fractions of the turns offered for a layer short.
pub const TURN_SHORTS: [f64; 3] = [0.1, 0.2, 0.5];

/// Resistance that stands in for a short.
const SHORT_OHMS: f64 = 1e-3;

fn is_top_level(name: &str) -> bool {
    !name.contains('.')
}

/// Names of the parts that can be broken (not the parts inside a library IC model).
pub fn breakable(c: &Circuit) -> Vec<String> {
    c.elements
        .iter()
        .filter(|e| is_top_level(e.name()) && !applicable(e).is_empty())
        .map(|e| e.name().to_string())
        .collect()
}

/// Faults that make sense for this kind of element.
pub fn applicable(e: &Element) -> Vec<Fault> {
    match e {
        Element::Inductor { .. } => {
            let mut v = vec![Fault::Open, Fault::Short, Fault::Scale(10.0), Fault::Scale(0.1)];
            v.extend(TURN_SHORTS.map(Fault::TurnShort));
            v
        }
        Element::Resistor { .. } | Element::Capacitor { .. } => {
            vec![Fault::Open, Fault::Short, Fault::Scale(10.0), Fault::Scale(0.1)]
        }
        Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. } => vec![Fault::Open, Fault::Short, Fault::Reverse],
        // a supply with a fixed DC value can die or sag / surge (signal sources have no single "voltage")
        Element::VSource { wave: Waveform::Dc(v), .. } if *v != 0.0 => vec![Fault::Open, Fault::Scale(0.5), Fault::Scale(1.5)],
        _ => vec![],
    }
}

/// The main two terminals of an element (what a short joins).
pub(crate) fn main_pins(e: &Element) -> Option<(usize, usize)> {
    match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::Inductor { a, b, .. } => Some((*a, *b)),
        Element::Diode { a, k, .. } => Some((*a, *k)),
        Element::Bjt { c, e, .. } => Some((*c, *e)),
        Element::Mosfet { d, s, .. } => Some((*d, *s)),
        _ => None,
    }
}

/// `c` with `element` broken as `fault`. `None` when the fault does not apply to that element.
pub fn with_fault(c: &Circuit, element: &str, fault: Fault) -> Option<Circuit> {
    let idx = c.elements.iter().position(|e| e.name() == element)?;
    if !applicable(&c.elements[idx]).contains(&fault) {
        return None;
    }
    let mut out = c.clone();
    match fault {
        // a dead supply is a source that gives 0 V (removing it would leave its loop open and unsolvable)
        Fault::Open if matches!(c.elements[idx], Element::VSource { .. }) => {
            if let Element::VSource { wave, .. } = &mut out.elements[idx] {
                *wave = Waveform::Dc(0.0);
            }
        }
        Fault::Open => {
            out.elements.remove(idx);
        }
        Fault::Reverse => match &mut out.elements[idx] {
            Element::Diode { a, k, .. } => std::mem::swap(a, k),
            Element::Bjt { c, e, .. } => std::mem::swap(c, e),
            Element::Mosfet { d, s, .. } => std::mem::swap(d, s),
            _ => return None,
        },
        Fault::Short => {
            let (a, b) = main_pins(&c.elements[idx])?;
            out.elements[idx] = Element::Resistor { name: element.into(), a, b, r: SHORT_OHMS };
        }
        Fault::TurnShort(x) => {
            let keep = 1.0 - x;
            if let Element::Inductor { l, .. } = &mut out.elements[idx] {
                *l *= keep * keep;
            }
            // the winding resistance (`<name>.DCR`) shrinks with the turns that are left
            let dcr = format!("{element}.DCR");
            if let Some(Element::Resistor { r, .. }) = out.elements.iter_mut().find(|e| e.name() == dcr) {
                *r *= keep;
            }
        }
        Fault::Scale(k) => match &mut out.elements[idx] {
            Element::Resistor { r, .. } => *r *= k,
            Element::Capacitor { c, .. } => *c *= k,
            Element::Inductor { l, .. } => *l *= k,
            Element::VSource { wave: Waveform::Dc(v), .. } => *v *= k,
            _ => return None,
        },
    }
    Some(out)
}

/// A circuit with something broken, and how to call that in words.
#[derive(Clone, Debug)]
pub struct Broken {
    pub circuit: Circuit,
    /// The part (or "U1") that was broken.
    pub element: String,
    /// What was done to it, e.g. "断線 (オープン)" or "ピン 2 (OUT) を断線".
    pub label: String,
}

/// Words for `fault` on `element`, which depend on what the element is (a supply "dies", it is not "opened").
pub fn describe_fault(c: &Circuit, element: &str, fault: Fault) -> String {
    match (c.elements.iter().find(|e| e.name() == element), fault) {
        (Some(Element::VSource { .. }), Fault::Open) => "電源が切れる (0V)".into(),
        (Some(Element::VSource { .. }), Fault::Scale(k)) => format!("電圧が {k} 倍"),
        _ => fault.label(),
    }
}

/// `element` of `c` broken as `fault`; `None` when the fault does not apply to it.
pub fn break_part(c: &Circuit, element: &str, fault: Fault) -> Option<Broken> {
    let circuit = with_fault(c, element, fault)?;
    Some(Broken { circuit, element: element.into(), label: describe_fault(c, element, fault) })
}

/// Breaks one pin of one part of the schematic (disconnected, grounded or bridged to another net).
/// Returns the healthy circuit and the broken one, both at `temp_c`. `label` says what was done,
/// for the reports ("ピン 2 (OUT) を断線").
pub fn break_pin(s: &sch_core::Schematic, temp_c: f64, spec: &crate::PinFaultSpec, label: &str) -> Result<(Circuit, Broken), SimError> {
    let join = |e: Vec<crate::ConvertError>| SimError::Invalid(e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "));
    let healthy = crate::from_schematic(s).map_err(join)?.at_temperature(temp_c);
    if !s.components.iter().any(|c| c.refdes == spec.refdes && c.pins().iter().any(|(n, _)| *n == spec.pin)) {
        return Err(SimError::Invalid(format!("{} にピン {} がありません", spec.refdes, spec.pin)));
    }
    let circuit = crate::from_schematic_with_pin_fault(s, spec).map_err(join)?.at_temperature(temp_c);
    Ok((healthy, Broken { circuit, element: spec.refdes.clone(), label: label.into() }))
}

fn not_applicable(element: &str, fault: Fault) -> SimError {
    SimError::Invalid(format!("{element} には「{}」の故障を設定できません", fault.label()))
}

#[derive(Clone, Debug)]
pub struct FaultReport {
    pub element: String,
    pub label: String,
    /// The fault when it is a whole-part fault (pin faults have none).
    pub fault: Option<Fault>,
    pub baseline: DcResult,
    /// `Err` when the broken circuit has no DC solution.
    pub faulted: Result<DcResult, SimError>,
    /// (node, healthy V, broken V), biggest change first; only nodes that moved noticeably.
    pub node_changes: Vec<(String, f64, f64)>,
    /// (part, healthy value, broken value, quantity) for parts whose current or power changed a lot.
    pub part_changes: Vec<PartChange>,
}

#[derive(Clone, Debug)]
pub struct PartChange {
    pub name: String,
    pub current: (f64, f64),
    pub power: (f64, f64),
}

/// Breaks `element` as `fault` and compares the DC operating points.
pub fn simulate_fault(c: &Circuit, element: &str, fault: Fault) -> Result<FaultReport, SimError> {
    let b = break_part(c, element, fault).ok_or_else(|| not_applicable(element, fault))?;
    let mut r = simulate_broken(c, &b)?;
    r.fault = Some(fault);
    Ok(r)
}

/// Compares the DC operating points of `c` and of the broken circuit.
pub fn simulate_broken(c: &Circuit, b: &Broken) -> Result<FaultReport, SimError> {
    let (element, broken) = (b.element.as_str(), &b.circuit);
    let baseline = dc_op(c)?;
    let faulted = dc_op(broken);
    let mut node_changes = vec![];
    let mut part_changes = vec![];
    if let Ok(f) = &faulted {
        for (n, v0) in baseline.voltages.iter().filter(|(n, _)| is_top_level(n)) {
            let v1 = f.voltage(n).unwrap_or(0.0);
            if (v1 - v0).abs() > 0.01 + 0.02 * v0.abs() {
                node_changes.push((n.clone(), *v0, v1));
            }
        }
        node_changes.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        for e0 in baseline.elements.iter().filter(|e| is_top_level(&e.name)) {
            if e0.name == element {
                continue;
            }
            let Some(e1) = f.element(&e0.name) else { continue };
            let di = (e1.current - e0.current).abs();
            let big = di > 1e-6 && di > 0.3 * e0.current.abs().max(1e-6);
            let hot = e1.power > 0.25 && e1.power > 2.0 * e0.power.max(0.0);
            if big || hot {
                part_changes.push(PartChange { name: e0.name.clone(), current: (e0.current, e1.current), power: (e0.power, e1.power) });
            }
        }
    }
    Ok(FaultReport { element: element.into(), label: b.label.clone(), fault: None, baseline, faulted, node_changes, part_changes })
}

fn volts(v: f64) -> String {
    format!("{}V", format_value((v * 1e4).round() / 1e4))
}

fn amps(i: f64) -> String {
    if i.abs() < 1e-9 { "0A".into() } else { format!("{}A", format_value(i)) }
}

impl FaultReport {
    /// What happened, in plain Japanese, most important first.
    pub fn summary(&self) -> Vec<String> {
        let mut out = vec![];
        let head = format!("{} を「{}」にしたとき", self.element, self.label);
        match &self.faulted {
            Err(e) => {
                out.push(format!("{head}、回路は DC では計算できませんでした ({e})。実機でも動作が定まらない、または大電流が流れる状態です"));
                return out;
            }
            Ok(_) => {}
        }
        if self.node_changes.is_empty() && self.part_changes.is_empty() {
            out.push(format!("{head}、DC の電圧と電流はほとんど変わりませんでした。"));
            match (self.fault, self.baseline.element(&self.element)) {
                (Some(Fault::Open), Some(e)) if e.current.abs() < 1e-9 => out.push("この部品には DC では電流が流れていないためです (コンデンサは DC では元から断線扱い)。過渡解析・AC解析なら違いが見えます".into()),
                (Some(Fault::TurnShort(_)), _) => out.push("コイルは DC では元からショート扱いなので、レアショートで変わるのはインダクタンス(電流の立ち上がり)です。過渡解析か「連鎖故障を追う」で見えます".into()),
                (Some(Fault::Short), _) if self.element.starts_with('L') => out.push("コイルは DC では元からショート扱いです。過渡解析・AC解析なら違いが見えます".into()),
                _ => {}
            }
            return out;
        }
        out.push(format!("{head}、次のように変わりました:"));
        for (n, a, b) in self.node_changes.iter().take(6) {
            out.push(format!("ノード {n}: {} → {} ({:+.2} V)", volts(*a), volts(*b), b - a));
        }
        for p in self.part_changes.iter().take(6) {
            let mut l = format!("{}: 電流 {} → {}", p.name, amps(p.current.0), amps(p.current.1));
            if p.power.1 > 0.25 && p.power.1 > 2.0 * p.power.0.max(0.0) {
                l += &format!("、消費電力 {:.0} mW → {:.0} mW", p.power.0.max(0.0) * 1e3, p.power.1 * 1e3);
                l += if p.power.1 > 1.0 { " (1W 超: 焼損・発煙のおそれ)" } else { " (一般的な 1/4 W 抵抗の定格を超えます)" };
            } else if p.current.1.abs() < 1e-9 && p.current.0.abs() > 1e-6 {
                l += " (電流が止まります)";
            }
            out.push(l);
        }
        out
    }
}

/// How dangerous a fault is for the parts (and the person at the bench).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Danger {
    /// Nothing runs past its rating that did not already.
    Safe,
    /// A part loses its margin (hot, close to its maximum).
    Caution,
    /// A part is past its rating, or the broken circuit has no stable state (large current).
    Danger,
}

/// The rating checks of the broken circuit, minus what was already flagged when healthy.
#[derive(Clone, Debug)]
pub struct FaultSafety {
    pub level: Danger,
    /// New or worse findings, worst first.
    pub issues: Vec<crate::RatingIssue>,
    /// The broken circuit could not be solved at DC.
    pub unsolvable: bool,
}

/// Rates `r` against the part ratings of `sch` (the schematic `r` was made from).
pub fn assess_safety(sch: &sch_core::Schematic, r: &FaultReport) -> FaultSafety {
    use crate::RatingLevel;
    let Ok(f) = &r.faulted else {
        return FaultSafety { level: Danger::Danger, issues: vec![], unsolvable: true };
    };
    let before = crate::check_ratings(sch, &r.baseline);
    let worst_before = |refdes: &str| before.iter().filter(|i| i.refdes == refdes).map(|i| i.level).max();
    let mut issues: Vec<crate::RatingIssue> = crate::check_ratings(sch, f).into_iter().filter(|i| worst_before(&i.refdes).is_none_or(|l| i.level > l)).collect();
    issues.sort_by(|a, b| b.level.cmp(&a.level));
    let level = match issues.first().map(|i| i.level) {
        Some(RatingLevel::Over) => Danger::Danger,
        Some(RatingLevel::Caution) => Danger::Caution,
        None => Danger::Safe,
    };
    FaultSafety { level, issues, unsolvable: false }
}

impl FaultSafety {
    /// Plain Japanese: the verdict first, then the parts at risk and what to do about it.
    pub fn summary(&self) -> Vec<String> {
        let mut out = vec![];
        if self.unsolvable {
            out.push("安全性: 危険。DC では状態が定まりません。実機では大電流が流れたり、部品が壊れたりするおそれがあります".into());
            out.push("対策: 電源にヒューズや電流制限をつけ、故障しても電流が上がりきらないようにします".into());
            return out;
        }
        out.push(match self.level {
            Danger::Safe => "安全性: 定格を超える部品は新しく出ませんでした (壊れても部品は無事な範囲です)".into(),
            Danger::Caution => "安全性: 注意。余裕がなくなる部品があります".into(),
            Danger::Danger => "安全性: 危険。定格を超える部品があり、焼損・発煙のおそれがあります".into(),
        });
        for i in self.issues.iter().take(4) {
            out.push(format!("・{}", i.message));
        }
        if let Some(i) = self.issues.first() {
            out.push(format!("対策: {}", i.hint));
        }
        if self.level == Danger::Danger {
            out.push("電源側にヒューズを入れると、故障のときに電流を切って被害を止められます (レッスン「保護回路」)".into());
        }
        out
    }
}

/// A fault compared in the time domain: the same node, healthy and broken.
#[derive(Clone, Debug)]
pub struct TranFaultReport {
    pub element: String,
    pub label: String,
    pub node: String,
    pub healthy_time: Vec<f64>,
    pub healthy: Vec<f64>,
    /// `Err` when the broken circuit cannot be simulated.
    pub faulted: Result<(Vec<f64>, Vec<f64>), SimError>,
    /// Window used for the figures below (the second half of the run, after start-up).
    pub window: (f64, f64),
    pub healthy_measure: Option<WaveMeasure>,
    pub faulted_measure: Option<WaveMeasure>,
    pub healthy_mean: Option<f64>,
    pub faulted_mean: Option<f64>,
}

/// Runs the transient analysis on the healthy circuit and with `element` broken as `fault`,
/// and measures `node` in both.
pub fn simulate_fault_tran(c: &Circuit, element: &str, fault: Fault, s: &TranSettings, node: &str) -> Result<TranFaultReport, SimError> {
    let b = break_part(c, element, fault).ok_or_else(|| not_applicable(element, fault))?;
    simulate_broken_tran(c, &b, s, node)
}

/// The transient comparison for an already broken circuit.
pub fn simulate_broken_tran(c: &Circuit, b: &Broken, s: &TranSettings, node: &str) -> Result<TranFaultReport, SimError> {
    let (element, broken) = (b.element.as_str(), &b.circuit);
    let signal = format!("V({node})");
    let pick = |r: &crate::TranResult| r.signal(&signal).map(|g| (r.time.clone(), g.values.clone()));
    let h = transient(c, s)?;
    let (healthy_time, healthy) = pick(&h).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))?;
    let faulted = transient(broken, s).and_then(|r| pick(&r).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません"))));
    let window = (s.tstop * 0.5, s.tstop);
    let measure = |t: &[f64], v: &[f64]| (wave_measure(t, v, window.0, window.1), stats(t, v, window.0, window.1).map(|x| x.mean));
    let (healthy_measure, healthy_mean) = measure(&healthy_time, &healthy);
    let (faulted_measure, faulted_mean) = match &faulted {
        Ok((t, v)) => measure(t, v),
        Err(_) => (None, None),
    };
    Ok(TranFaultReport { element: element.into(), label: b.label.clone(), node: node.into(), healthy_time, healthy, faulted, window, healthy_measure, faulted_measure, healthy_mean, faulted_mean })
}

impl TranFaultReport {
    /// What changed at the node, in plain Japanese.
    pub fn summary(&self) -> Vec<String> {
        let head = format!("{} を「{}」にしたときの V({})", self.element, self.label, self.node);
        if let Err(e) = &self.faulted {
            return vec![format!("{head}: 過渡解析が計算できませんでした ({e})。大電流が流れるか、動作が定まらない状態です")];
        }
        let (Some(h), Some(f)) = (self.healthy_measure, self.faulted_measure) else {
            return vec![format!("{head}: 範囲内の波形が短くて測れませんでした。終了時間を長くしてください")];
        };
        let mut out = vec![format!("{head} (終了時間の後半での値)")];
        let mut changed = false;
        if let (Some(a), Some(b)) = (self.healthy_mean, self.faulted_mean) {
            let tol = 0.02 * a.abs().max(b.abs()) + 0.01;
            if (a - b).abs() > tol {
                out.push(format!("平均電圧: {} → {}", format_value((a * 1e3).round() / 1e3), format_value((b * 1e3).round() / 1e3)));
                changed = true;
            }
        }
        if (f.pp - h.pp).abs() > 0.2 * h.pp.max(f.pp) + 1e-3 {
            let ratio = if h.pp > 1e-9 { format!(" ({:.1} 倍)", f.pp / h.pp) } else { String::new() };
            out.push(format!("振幅 (p-p): {}V → {}V{ratio}", format_value((h.pp * 1e3).round() / 1e3), format_value((f.pp * 1e3).round() / 1e3)));
            changed = true;
        }
        match (h.freq, f.freq) {
            (Some(a), Some(b)) if (a - b).abs() > 0.1 * a => {
                out.push(format!("周波数: {}Hz → {}Hz", format_value((a * 10.0).round() / 10.0), format_value((b * 10.0).round() / 10.0)));
                changed = true;
            }
            (Some(a), None) => {
                out.push(format!("周期的な波形 ({}Hz) が見られなくなりました", format_value((a * 10.0).round() / 10.0)));
                changed = true;
            }
            (None, Some(b)) => {
                out.push(format!("周期的な波形 ({}Hz) が現れました", format_value((b * 10.0).round() / 10.0)));
                changed = true;
            }
            _ => {}
        }
        if let (Some(a), Some(b)) = (h.duty, f.duty)
            && (a - b).abs() > 0.1
        {
            out.push(format!("デューティ比: {:.0} % → {:.0} %", a * 100.0, b * 100.0));
            changed = true;
        }
        if !changed {
            out.push("この点の波形はほとんど変わりませんでした。別の点を見るか、解析の範囲を変えてみましょう".into());
        }
        out
    }
}

/// A fault compared in the frequency domain: the gain at a node, healthy and broken.
#[derive(Clone, Debug)]
pub struct AcFaultReport {
    pub element: String,
    pub label: String,
    pub node: String,
    pub freq: Vec<f64>,
    pub healthy_db: Vec<f64>,
    /// `Err` when the broken circuit cannot be solved.
    pub faulted_db: Result<Vec<f64>, SimError>,
}

/// Runs the AC analysis on the healthy circuit and with `element` broken as `fault`, and returns the
/// gain (dB) at `node` for both.
pub fn simulate_fault_ac(c: &Circuit, element: &str, fault: Fault, s: &AcSettings, node: &str) -> Result<AcFaultReport, SimError> {
    let b = break_part(c, element, fault).ok_or_else(|| not_applicable(element, fault))?;
    simulate_broken_ac(c, &b, s, node)
}

/// The frequency-response comparison for an already broken circuit.
pub fn simulate_broken_ac(c: &Circuit, b: &Broken, s: &AcSettings, node: &str) -> Result<AcFaultReport, SimError> {
    let (element, broken) = (b.element.as_str(), &b.circuit);
    let signal = format!("V({node})");
    let h = ac_analysis(c, s)?;
    let hs = h.signal(&signal).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))?;
    let faulted_db = ac_analysis(broken, s).and_then(|r| r.signal(&signal).map(|g| g.db()).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません"))));
    Ok(AcFaultReport { element: element.into(), label: b.label.clone(), node: node.into(), freq: h.freq.clone(), healthy_db: hs.db(), faulted_db })
}

fn peak(freq: &[f64], db: &[f64]) -> Option<(f64, f64)> {
    (0..db.len()).max_by(|&a, &b| db[a].total_cmp(&db[b])).map(|i| (freq[i], db[i]))
}

fn hz(f: f64) -> String {
    format!("{}Hz", format_value((f * 100.0).round() / 100.0))
}

impl AcFaultReport {
    /// What changed in the frequency response, in plain Japanese.
    pub fn summary(&self) -> Vec<String> {
        let head = format!("{} を「{}」にしたときの V({}) の周波数特性", self.element, self.label, self.node);
        let f_db = match &self.faulted_db {
            Err(e) => return vec![format!("{head}: AC解析が計算できませんでした ({e})")],
            Ok(d) => d,
        };
        let mut out = vec![head];
        let mut changed = false;
        let (fr, hd) = (&self.freq, &self.healthy_db);
        if let (Some((hf, hp)), Some((ff, fp))) = (peak(fr, hd), peak(fr, f_db)) {
            if (hp - fp).abs() > 1.0 {
                out.push(format!("最大ゲイン: {hp:.1} dB ({}) → {fp:.1} dB ({})", hz(hf), hz(ff)));
                changed = true;
            } else if (hf / ff).ln().abs() > 0.3 && hp > -60.0 {
                out.push(format!("ゲインが最大になる周波数: {} → {}", hz(hf), hz(ff)));
                changed = true;
            }
        }
        let mag = |db: &[f64]| db.iter().map(|d| 10f64.powf(d / 20.0)).collect::<Vec<_>>();
        match (minus_3db(fr, &mag(hd)), minus_3db(fr, &mag(f_db))) {
            (Some(a), Some(b)) if (a / b).ln().abs() > 0.1 => {
                out.push(format!("-3dB点 (カットオフ): {} → {}", hz(a), hz(b)));
                changed = true;
            }
            (Some(a), None) => {
                out.push(format!("-3dB点 ({}) が範囲内になくなりました (ゲインが下がらない、または全域で低い)", hz(a)));
                changed = true;
            }
            (None, Some(b)) => {
                out.push(format!("-3dB点が {} に現れました", hz(b)));
                changed = true;
            }
            _ => {}
        }
        if let (Some(a), Some(b)) = (hd.first(), f_db.first())
            && (a - b).abs() > 1.0
        {
            out.push(format!("低い周波数 ({}) のゲイン: {a:.1} dB → {b:.1} dB", hz(fr[0])));
            changed = true;
        }
        if let (Some(a), Some(b)) = (hd.last(), f_db.last())
            && (a - b).abs() > 1.0
        {
            out.push(format!("高い周波数 ({}) のゲイン: {a:.1} dB → {b:.1} dB", hz(*fr.last().unwrap())));
            changed = true;
        }
        if !changed {
            out.push("周波数特性はほとんど変わりませんでした。別の点を見るか、周波数の範囲を変えてみましょう".into());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 V - R1(1k) - node MID - R2(1k) - GND, with an LED-free divider.
    fn divider() -> Circuit {
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("IN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: 1e3 });
        c.add(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: 1e3 });
        c
    }

    fn mid(r: &FaultReport) -> f64 {
        r.faulted.as_ref().unwrap().voltage("MID").unwrap()
    }

    #[test]
    fn open_and_short_move_the_divider_to_the_rails() {
        let c = divider();
        let open_r1 = simulate_fault(&c, "R1", Fault::Open).unwrap();
        assert!(mid(&open_r1).abs() < 1e-3, "{}", mid(&open_r1));
        let open_r2 = simulate_fault(&c, "R2", Fault::Open).unwrap();
        assert!((mid(&open_r2) - 5.0).abs() < 1e-3);
        let short_r1 = simulate_fault(&c, "R1", Fault::Short).unwrap();
        assert!((mid(&short_r1) - 5.0).abs() < 0.01);
        let x10 = simulate_fault(&c, "R1", Fault::Scale(10.0)).unwrap();
        assert!((mid(&x10) - 5.0 / 11.0).abs() < 1e-3);
    }

    #[test]
    fn summary_names_the_changes() {
        let r = simulate_fault(&divider(), "R2", Fault::Open).unwrap();
        let s = r.summary().join("\n");
        assert!(s.contains("R2") && s.contains("ノード MID") && s.contains("2.5V → 5V"), "{s}");
    }

    #[test]
    fn faults_that_do_not_apply_are_refused() {
        let c = divider();
        assert!(simulate_fault(&c, "V1", Fault::Short).is_err());
        assert!(with_fault(&c, "R1", Fault::Scale(3.0)).is_none());
        assert_eq!(breakable(&c), vec!["V1".to_string(), "R1".to_string(), "R2".to_string()]);
    }

    #[test]
    fn a_capacitor_open_does_nothing_at_dc_and_says_why() {
        let mut c = divider();
        let mid_n = c.node("MID");
        c.add(Element::Capacitor { name: "C1".into(), a: mid_n, b: 0, c: 1e-6 });
        let r = simulate_fault(&c, "C1", Fault::Open).unwrap();
        assert!(r.node_changes.is_empty());
        assert!(r.summary().iter().any(|l| l.contains("DC では電流が流れていない")), "{:?}", r.summary());
    }

    #[test]
    fn an_overloaded_part_is_flagged_as_hot() {
        // 12 V across R1 = 100 and R2 = 10k; shorting R2's neighbour pushes 1.4 W through R1
        let mut c = Circuit::new();
        let (vin, mid_n) = (c.node("IN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 12.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid_n, r: 100.0 });
        c.add(Element::Resistor { name: "R2".into(), a: mid_n, b: 0, r: 10e3 });
        c.add(Element::Resistor { name: "R3".into(), a: mid_n, b: 0, r: 100.0 });
        let r = simulate_fault(&c, "R2", Fault::Short).unwrap();
        let s = r.summary().join("\n");
        assert!(s.contains("R1") && s.contains("1W 超"), "{s}");
    }

    #[test]
    fn safety_rates_a_shorted_led_resistor_as_danger_and_a_healthy_one_as_safe() {
        let s: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/led.schjson")).unwrap();
        let c = crate::from_schematic(&s).unwrap();
        // no limiter: the LED takes everything the supply gives
        let short = simulate_fault(&c, "R1", Fault::Short).unwrap();
        let sf = assess_safety(&s, &short);
        assert_eq!(sf.level, Danger::Danger, "{sf:?}");
        assert!(sf.summary().iter().any(|l| l.contains("D1")), "{:?}", sf.summary());
        // a bigger resistor only dims the LED
        let dim = assess_safety(&s, &simulate_fault(&c, "R1", Fault::Scale(10.0)).unwrap());
        assert_eq!(dim.level, Danger::Safe, "{dim:?}");
        // findings that were there before the fault are not blamed on it
        let mut hot = s.clone();
        hot.components.iter_mut().filter(|x| x.refdes == "R1").for_each(|x| x.value = "10".into());
        let hc = crate::from_schematic(&hot).unwrap();
        let again = assess_safety(&hot, &simulate_fault(&hc, "R1", Fault::Scale(0.1)).unwrap());
        assert!(again.issues.iter().all(|i| i.level > crate::RatingLevel::Caution) || again.issues.is_empty(), "{again:?}");
    }

    #[test]
    fn transient_fault_compares_the_waveform_at_a_node() {
        use crate::waveform::parse_source;
        // 1 kHz sine through R1 into C1 (corner at 1 kHz): opening C1 lets the full amplitude through
        let mut c = Circuit::new();
        let (vin, out) = (c.node("IN"), c.node("OUT"));
        let wave = parse_source("SIN(0 2 1k)").unwrap();
        c.add(Element::VSource { name: "V1".into(), p: vin, n: 0, wave, ac: None });
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: out, r: 1e3 });
        c.add(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 159e-9 });
        let set = TranSettings { tstop: 20e-3, tstep: 10e-6, uic: false };
        let r = simulate_fault_tran(&c, "C1", Fault::Open, &set, "OUT").unwrap();
        let (h, f) = (r.healthy_measure.unwrap(), r.faulted_measure.unwrap());
        assert!(f.pp > 1.3 * h.pp, "{} vs {}", f.pp, h.pp);
        assert!(r.summary().iter().any(|l| l.contains("振幅 (p-p)")), "{:?}", r.summary());
        // a short at C1 pins the node to 0 V: the periodic wave disappears
        let s = simulate_fault_tran(&c, "C1", Fault::Short, &set, "OUT").unwrap();
        assert!(s.summary().iter().any(|l| l.contains("見られなくなりました") || l.contains("振幅")), "{:?}", s.summary());
        assert!(simulate_fault_tran(&c, "C1", Fault::Open, &set, "NOPE").is_err());
    }

    #[test]
    fn ac_fault_moves_the_cutoff_and_the_summary_says_so() {
        use crate::ac::AcSweep;
        // R1 = 1k, C1 = 159 nF: cutoff 1 kHz. R1 x10 moves it to 100 Hz; C1 open removes it.
        let mut c = Circuit::new();
        let (vin, out) = (c.node("IN"), c.node("OUT"));
        c.add(Element::VSource { name: "V1".into(), p: vin, n: 0, wave: crate::waveform::parse_source_full("AC 1").unwrap().wave, ac: Some((1.0, 0.0)) });
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: out, r: 1e3 });
        c.add(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 159e-9 });
        let set = AcSettings { fstart: 10.0, fstop: 100e3, sweep: AcSweep::Decade(20) };
        let r = simulate_fault_ac(&c, "R1", Fault::Scale(10.0), &set, "OUT").unwrap();
        let text = r.summary().join("
");
        assert!(text.contains("-3dB点") && text.contains("Hz → "), "{text}");
        let open = simulate_fault_ac(&c, "C1", Fault::Open, &set, "OUT").unwrap().summary().join("
");
        assert!(open.contains("-3dB点") && open.contains("なくなりました"), "{open}");
        assert!(simulate_fault_ac(&c, "C1", Fault::Open, &set, "NOPE").is_err());
        // a part that does not matter leaves the response alone
        assert!(simulate_fault_ac(&c, "R1", Fault::Scale(1.0), &set, "OUT").is_err());
    }

    #[test]
    fn reversed_diode_and_a_dying_supply() {
        use crate::circuit::DiodeModel;
        // 5 V - R1 - D1 - GND: the diode conducts; reversed it blocks and the node floats up to 5 V
        let mut c = Circuit::new();
        let (vin, mid) = (c.node("IN"), c.node("MID"));
        c.add(Element::vdc("V1", vin, 0, 5.0));
        c.add(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: 1e3 });
        c.add(Element::Diode { name: "D1".into(), a: mid, k: 0, model: DiodeModel::default() });
        let healthy = dc_op(&c).unwrap().voltage("MID").unwrap();
        assert!((0.5..0.9).contains(&healthy), "{healthy}");
        let rev = simulate_fault(&c, "D1", Fault::Reverse).unwrap();
        assert!((rev.faulted.as_ref().unwrap().voltage("MID").unwrap() - 5.0).abs() < 0.01);
        assert!(rev.summary().join("
").contains("逆向きにつなぐ"), "{:?}", rev.summary());
        // the supply: half voltage halves everything, "dead" gives 0 V
        let half = simulate_fault(&c, "V1", Fault::Scale(0.5)).unwrap();
        assert!((half.faulted.as_ref().unwrap().voltage("IN").unwrap() - 2.5).abs() < 1e-6);
        assert!(half.summary().join("
").contains("電圧が 0.5 倍"), "{:?}", half.summary());
        let dead = simulate_fault(&c, "V1", Fault::Open).unwrap();
        assert!(dead.faulted.as_ref().unwrap().voltage("IN").unwrap().abs() < 1e-9);
        assert!(dead.summary().join("
").contains("電源が切れる"));
        // a transistor can be fitted backwards too
        assert!(applicable(&Element::Bjt { name: "Q1".into(), c: 1, b: 2, e: 0, model: crate::BjtModel::default() }).contains(&Fault::Reverse));
    }
}
