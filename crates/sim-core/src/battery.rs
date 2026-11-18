//! How long a battery lasts in the circuit (candidate B2). The library's battery parts (`AA_ALKALINE`, `CR2032`, `LIION_18650` ...)
//! carry their capacity (`capacity_mah`) and internal resistance (`rint`) in the model's params, and their model keeps the state of
//! charge on an internal node (`soc`, 1 = full; the element `Bv` carries the battery's current). From a transient run this finds the
//! average and the peak current the battery gives over a stretch of time (with the sleep and the bursts of an intermittent circuit in
//! it), and from DC solutions at less and less charge, the charge at which the circuit stops: an output falling under its lower limit
//! (a regulator's dropout, a brown-out) or, without a limit, the battery's own end. Hours = usable charge / average current.

use crate::dc::dc_op;
use crate::{Circuit, TranResult};
use sch_core::{extract, Schematic};

/// A battery part of the schematic.
#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub refdes: String,
    /// The part's name (`CR2032`).
    pub part: String,
    pub capacity_mah: f64,
    pub rint: f64,
    /// The nets of its + and - pins.
    pub plus: String,
    pub minus: String,
}

/// The battery parts of the schematic: parts whose model has `capacity_mah` and keeps its charge on a node `soc`.
pub fn batteries(s: &Schematic) -> Vec<Battery> {
    let nets = extract(s);
    let net_of = |refdes: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
    s.components
        .iter()
        .filter_map(|c| {
            let custom = c.custom.as_ref()?;
            let sim = custom.sim.as_ref()?;
            let cap = *sim.params.get("capacity_mah")?;
            if !(cap > 0.0) || !sim.text.contains("V(soc)") {
                return None;
            }
            // the model's first pin is +, the second - (`.subckt NAME p n`)
            let (p, n) = (sim.pins.first()?.clone(), sim.pins.get(1)?.clone());
            Some(Battery { refdes: c.refdes.clone(), part: custom.name.clone(), capacity_mah: cap, rint: sim.params.get("rint").copied().unwrap_or(0.0), plus: net_of(&c.refdes, &p), minus: net_of(&c.refdes, &n) })
        })
        .collect()
}

/// The schematic with battery `refdes` starting at state of charge `soc` (0..1) instead of full: the model holds the charge node at
/// its starting value while `time < 1u` (`Bpull ... 1000*(V(soc)-1)`), which is also what the DC solution uses.
pub fn with_charge(s: &Schematic, refdes: &str, soc: f64) -> Schematic {
    let mut out = s.clone();
    if let Some(sim) = out.components.iter_mut().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_mut()).and_then(|c| c.sim.as_mut()) {
        let key = "1000*(V(soc)-";
        if let Some(at) = sim.text.find(key) {
            let start = at + key.len();
            let end = sim.text[start..].find(')').map_or(start, |k| start + k);
            sim.text.replace_range(start..end, &format!("{soc:.6}"));
        }
    }
    out
}

/// What the circuit needs to keep working: the voltage of `node` stays at or above `v_min`. An empty node: no condition (the battery
/// is used down to its own end).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryCondition {
    pub node: String,
    pub v_min: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryLife {
    pub battery: Battery,
    /// The stretch of the transient run the current was averaged over (s).
    pub from: f64,
    pub to: f64,
    /// Average and highest current the battery gives over that stretch (A; giving out = positive).
    pub i_avg: f64,
    pub i_peak: f64,
    /// The charge left when the circuit stops (0..1), and the battery's terminal voltage at that point under the average load.
    pub stop_soc: f64,
    pub stop_volts: Option<f64>,
    /// The battery's terminal voltage full, under the average load.
    pub full_volts: Option<f64>,
    /// The condition decided the end (false: the battery's own end came first, or there was no condition).
    pub by_condition: bool,
    pub hours: f64,
    /// What to keep in mind with this answer.
    pub notes: Vec<String>,
}

fn node_v(r: &crate::DcResult, net: &str) -> Option<f64> {
    if net.eq_ignore_ascii_case("GND") || net == "0" || net.is_empty() {
        return Some(0.0);
    }
    r.voltage(net)
}

/// The trapezoid mean and the highest value of `y(t)` over `from..to`.
fn mean_and_peak(t: &[f64], y: &[f64], from: f64, to: f64) -> Option<(f64, f64)> {
    let (mut area, mut span, mut peak) = (0.0, 0.0, f64::MIN);
    for k in 1..t.len().min(y.len()) {
        let (a, b) = (t[k - 1].max(from), t[k].min(to));
        if b <= a {
            continue;
        }
        let at = |x: f64| y[k - 1] + (y[k] - y[k - 1]) * (x - t[k - 1]) / (t[k] - t[k - 1]).max(1e-30);
        area += (at(a) + at(b)) / 2.0 * (b - a);
        span += b - a;
        peak = peak.max(at(a)).max(at(b));
    }
    (span > 0.0).then(|| (area / span, peak))
}

/// How long each battery of `s` lasts. `tr`: a transient run of `s`; `from..to`: the stretch to average (`to <= from` = the second
/// half of the run, after the start-up); `build`: the circuit of a schematic, as the analyses build it (temperature, wiring).
pub fn battery_life(s: &Schematic, tr: &TranResult, from: f64, to: f64, cond: &BatteryCondition, build: &dyn Fn(&Schematic) -> Result<Circuit, String>) -> Result<Vec<BatteryLife>, String> {
    let list = batteries(s);
    if list.is_empty() {
        return Err("回路図に電池の部品がありません。ライブラリの「電池」(単3アルカリ・CR2032・18650 など) を電源に使ってください".into());
    }
    let end = *tr.time.last().ok_or("過渡解析の結果が空です")?;
    let (from, to) = if to > from { (from.max(0.0), to.min(end)) } else { (end / 2.0, end) };
    if to <= from {
        return Err(format!("平均をとる区間が、過渡解析の時間 (0〜{}s) の外です", crate::format_value(end)));
    }
    let mut out = vec![];
    for b in list {
        let sig = tr.signal(&format!("I({}.Bv)", b.refdes)).ok_or_else(|| format!("{} の電流が過渡解析の結果にありません (電池のモデルが古い形です)", b.refdes))?;
        // the element's current flows into its + node: giving out is negative
        let giving: Vec<f64> = sig.values.iter().map(|i| -i).collect();
        let (i_avg, i_peak) = mean_and_peak(&tr.time, &giving, from, to).ok_or("平均をとる区間に過渡解析の点がありません")?;
        if i_avg <= 0.0 {
            return Err(format!("{} から電流が出ていません (平均 {}A)。電池が充電されているか、何もつながっていません", b.refdes, crate::format_value(i_avg)));
        }
        let solve = |soc: f64| -> Result<crate::DcResult, String> { dc_op(&build(&with_charge(s, &b.refdes, soc))?).map_err(|e| e.to_string()) };
        let terminal = |r: &crate::DcResult| Some(node_v(r, &b.plus)? - node_v(r, &b.minus)?);
        let works = |soc: f64| -> Result<bool, String> {
            let r = solve(soc)?;
            Ok(cond.node.trim().is_empty() || node_v(&r, cond.node.trim()).ok_or_else(|| format!("ネット「{}」がありません", cond.node))? >= cond.v_min)
        };
        let full = solve(1.0)?;
        let mut notes = vec![];
        let (stop_soc, by_condition) = if !works(1.0)? {
            return Err(format!("満充電の {} でも、{} が {}V に届きません (いまは {}V)。条件の電圧か、回路を見直してください", b.refdes, cond.node, crate::format_value(cond.v_min), node_v(&full, cond.node.trim()).map_or("?".into(), crate::format_value)));
        } else if works(0.0)? {
            (0.0, false)
        } else {
            // the circuit works at `hi` and not at `lo`
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..18 {
                let mid = (lo + hi) / 2.0;
                if works(mid)? { hi = mid } else { lo = mid }
            }
            (hi, true)
        };
        let stop = solve(stop_soc)?;
        let hours = b.capacity_mah * 1e-3 * (1.0 - stop_soc) / i_avg;
        if by_condition {
            notes.push(format!("電池の残量が {:.0} % になると、{} が {}V を下回って止まります。そこから先の容量は使えません", stop_soc * 100.0, cond.node.trim(), crate::format_value(cond.v_min)));
        }
        let pulse_drop = (i_peak - i_avg) * b.rint;
        if by_condition && pulse_drop > 0.02 {
            notes.push(format!("いちばん大きい電流 ({}A) が流れる瞬間は、内部抵抗で電池の電圧がさらに約 {}V 下がるので、これより少し早く止まることがあります", crate::format_value(i_peak), crate::format_value(pulse_drop)));
        }
        if let Some(v) = terminal(&full)
            && i_peak * b.rint > 0.1 * v
        {
            notes.push(format!("いちばん大きい電流で、内部抵抗の電圧降下が電池の電圧の {:.0} % になります。内部抵抗の大きい電池 (コイン電池など) には向かない電流です", i_peak * b.rint / v * 100.0));
        }
        notes.push("容量は小さい電流での値で計算しています。大きい電流・低温・古い電池では、取り出せる量が減ります (目安は 50〜80 %)。自己放電も入っていません".into());
        out.push(BatteryLife { full_volts: terminal(&full), stop_volts: terminal(&stop), battery: b, from, to, i_avg, i_peak, stop_soc, by_condition, hours, notes });
    }
    Ok(out)
}

/// Hours in words: "35 時間" or "120 日 (2880 時間)".
pub fn hours_text(h: f64) -> String {
    if h >= 48.0 {
        let days = h / 24.0;
        if days >= 730.0 { format!("約 {:.1} 年 ({:.0} 日)", days / 365.0, days) } else { format!("約 {days:.0} 日 ({h:.0} 時間)") }
    } else if h >= 1.0 {
        format!("{h:.1} 時間")
    } else {
        format!("{:.0} 分", h * 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mean_takes_the_time_steps_into_account() {
        // 1 A for 1 s then 0 A for 3 s (unequal steps): a quarter on average, the peak 1
        let t = [0.0, 1.0, 1.0 + 1e-9, 2.0, 4.0];
        let y = [1.0, 1.0, 0.0, 0.0, 0.0];
        let (m, p) = mean_and_peak(&t, &y, 0.0, 4.0).unwrap();
        assert!((m - 0.25).abs() < 1e-6 && p == 1.0, "{m} {p}");
        let (m, _) = mean_and_peak(&t, &y, 0.5, 1.0).unwrap();
        assert!((m - 1.0).abs() < 1e-9);
        assert!(mean_and_peak(&t, &y, 5.0, 6.0).is_none());
    }

    fn sample() -> Schematic {
        serde_json::from_str(include_str!("../../../docs/examples/battery_life.schjson")).unwrap()
    }

    fn run_with(s: &Schematic, v_min: f64) -> Result<BatteryLife, String> {
        let build = |x: &Schematic| crate::from_schematic(x).map_err(|e| format!("{e:?}"));
        let tr = crate::transient(&build(s).unwrap(), &crate::TranSettings { tstop: 0.3, tstep: 50e-6, uic: false }).unwrap();
        // two whole periods of the load (a stretch that is not a whole number of periods gives another average)
        let mut r = battery_life(s, &tr, 0.1, 0.3, &BatteryCondition { node: "3V3".into(), v_min }, &build)?;
        assert_eq!(r.len(), 1);
        Ok(r.remove(0))
    }

    fn run(s: &Schematic) -> BatteryLife {
        run_with(s, 3.2).unwrap()
    }

    /// The sample: 1 mA asleep and 100 mA for 5 ms in every 100 ms is about 5.95 mA on average; the 2600 mAh cell, with the regulator
    /// needing a little more than 3.3 V, lasts some 17 days.
    #[test]
    fn the_sample_lasts_as_its_average_current_and_the_regulator_say() {
        let s = sample();
        let b = batteries(&s);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].plus.as_str(), b[0].minus.as_str(), b[0].capacity_mah), ("VBAT", "GND", 2600.0));
        let l = run(&s);
        assert!((l.i_avg - 5.95e-3).abs() < 0.4e-3, "average {}", l.i_avg);
        assert!(l.i_peak > 0.095 && l.i_peak < 0.12, "peak {}", l.i_peak);
        assert!(l.by_condition && l.stop_soc > 0.0 && l.stop_soc < 0.3, "stops near the end: {}", l.stop_soc);
        let v = l.stop_volts.unwrap();
        // at the sleeping 1 mA the MCP1700 needs hardly more than its output
        assert!(v > 3.19 && v < 3.4, "the cell's voltage when the output drops: {v}");
        let expect = 2600e-3 * (1.0 - l.stop_soc) / l.i_avg;
        assert!((l.hours - expect).abs() < 1e-6 && l.hours > 300.0 && l.hours < 450.0, "{}", l.hours);

        // the same with the AMS1117 (dropout 1.1 V): it stops with much more charge left
        let mut ams = s.clone();
        let def: part_lib::PartDef = serde_json::from_str(part_lib::EXAMPLES.iter().find(|(n, _)| *n == "AMS1117-3.3_SOT-223").unwrap().1).unwrap();
        let u = ams.components.iter_mut().find(|c| c.refdes == "U1").unwrap();
        u.custom = Some(def.symbol());
        u.value = def.default_value();
        // a full cell (4.2 V) less 1.1 V is under 3.2 V: it never makes the rail
        assert!(run_with(&ams, 3.2).unwrap_err().contains("届きません"));
        // with 3.0 V enough, it stops with much more charge left than the MCP1700
        let worse = run_with(&ams, 3.0).unwrap();
        assert!(worse.stop_soc > l.stop_soc + 0.3, "{} vs {}", worse.stop_soc, l.stop_soc);
        assert!(worse.hours < l.hours * 0.7, "{} vs {}", worse.hours, l.hours);
    }

    #[test]
    fn with_charge_moves_the_starting_charge_only() {
        let s = sample();
        let half = with_charge(&s, "BT1", 0.5);
        let text = |x: &Schematic| x.components.iter().find(|c| c.refdes == "BT1").unwrap().custom.as_ref().unwrap().sim.as_ref().unwrap().text.clone();
        assert!(text(&half).contains("1000*(V(soc)-0.500000)") && !text(&s).contains("0.500000"));
        // without a condition the battery is used to its own end
        let build = |x: &Schematic| crate::from_schematic(x).map_err(|e| format!("{e:?}"));
        let tr = crate::transient(&build(&s).unwrap(), &crate::TranSettings { tstop: 0.2, tstep: 50e-6, uic: false }).unwrap();
        let r = battery_life(&s, &tr, 0.0, 0.0, &BatteryCondition::default(), &build).unwrap();
        assert_eq!(r[0].stop_soc, 0.0);
        assert!(!r[0].by_condition);
        assert!(battery_life(&Schematic::default(), &tr, 0.0, 0.0, &BatteryCondition::default(), &build).is_err());
    }

    #[test]
    fn hours_read_well() {
        assert_eq!(hours_text(0.5), "30 分");
        assert_eq!(hours_text(12.0), "12.0 時間");
        assert_eq!(hours_text(240.0), "約 10 日 (240 時間)");
        assert!(hours_text(24.0 * 800.0).starts_with("約 2.2 年"));
    }
}
