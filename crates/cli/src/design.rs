//! Design helpers: `calc` (value calculators), `ac` (frequency response) and `spec` (pass / fail against a target).

use crate::session::{obj, opt, options, Session};
use crate::Res;
use serde_json::{json, Value};
use sim_core::{CalcOut, SpecInputs, SpecKind};

type Run = fn(&[f64]) -> Result<CalcOut, String>;

/// key, what it designs, input names (units in the name), calculation. Same calculators as the app's
/// "値の決め方" window (`schematic/wizard.rs` CALCS); the board trace width one lives only in the app.
const CALCS: &[(&str, &str, &[&str], Run)] = &[
    ("led", "series resistor for an LED", &["supply_V", "led_forward_V", "current_mA"], |v| sim_core::led_resistor(v[0], v[1], v[2])),
    ("divider", "two-resistor voltage divider (R1 on top, R2 below)", &["vin_V", "vout_V", "r2_ohm"], |v| sim_core::divider(v[0], v[1], v[2])),
    ("rc", "resistor of an RC filter for a cut-off frequency", &["cutoff_Hz", "capacitor_F"], |v| sim_core::rc_filter(v[0], v[1])),
    ("regulator", "around a 78xx linear regulator: capacitors, power lost as heat and efficiency (use 'heat' for temperature and heat sink)", &["vin_V", "vout_V", "current_mA"], |v| sim_core::regulator(v[0], v[1], v[2])),
    (
        "heat",
        "junction temperature of a linear regulator and the heat sink it needs (needs the thermal data sheet numbers)",
        &["vin_V", "vout_V", "current_mA", "theta_ja_C_per_W", "theta_jc_C_per_W", "ambient_C", "tj_max_C"],
        |v| sim_core::heat_design(v[0], v[1], v[2], v[3], v[4], v[5], v[6]),
    ),
    ("fuse", "choosing a fuse", &["normal_mA", "supply_V", "fault_resistance_ohm", "part_max_mA"], |v| sim_core::fuse_design(v[0], v[1], v[2], v[3])),
    ("npn_switch", "base resistor of a transistor switch", &["input_V", "load_mA", "hfe"], |v| sim_core::transistor_switch(v[0], v[1], v[2])),
    ("smoothing", "smoothing capacitor after a rectifier", &["transformer_Vrms", "load_mA", "mains_Hz", "ripple_Vpp"], |v| sim_core::smoothing_cap(v[0], v[1], v[2], v[3])),
    (
        "buck",
        "inductor and output capacitor of a buck converter",
        &["vin_V", "vout_V", "iout_mA", "fsw_kHz", "inductor_ripple_pct", "output_ripple_mV"],
        |v| sim_core::buck_design(v[0], v[1], v[2], v[3], v[4], v[5]),
    ),
    (
        "boost",
        "inductor and output capacitor of a boost converter",
        &["vin_V", "vout_V", "iout_mA", "fsw_kHz", "inductor_ripple_pct", "output_ripple_mV"],
        |v| sim_core::boost_design(v[0], v[1], v[2], v[3], v[4], v[5]),
    ),
    ("mosfet", "MOSFET switch (dissipation, gate resistor)", &["gate_drive_V", "vth_V", "load_mA", "rds_on_mohm", "load_supply_V"], |v| sim_core::mosfet_switch(v[0], v[1], v[2], v[3], v[4])),
    (
        "opamp",
        "feedback resistors of an op-amp amplifier",
        &["gain", "rf_ohm", "inverting_0_or_1", "gbw_kHz", "supply_V", "input_peak_mV"],
        |v| sim_core::opamp_gain(v[0], v[1], v[2], v[3], v[4], v[5]),
    ),
    ("battery", "how long a battery lasts", &["capacity_mAh", "load_mA", "internal_ohm", "voltage_V", "usable_pct"], |v| sim_core::battery_life(v[0], v[1], v[2], v[3], v[4])),
    ("zener", "series resistor of a zener regulator", &["vin_V", "vz_V", "load_mA", "iz_min_mA"], |v| sim_core::zener_regulator(v[0], v[1], v[2], v[3])),
];

fn round6(x: f64) -> f64 {
    format!("{x:.6e}").parse().unwrap_or(x)
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

impl Session {
    /// `calc` lists the calculators; `calc KEY NUMBER...` runs one (the same ones as the app's value wizard).
    pub fn calc(&self, args: &[String]) -> Res {
        let Some(key) = args.first() else {
            let list: Vec<Value> = CALCS.iter().map(|(k, what, inputs, _)| json!({"calc": k, "designs": what, "inputs": inputs})).collect();
            return Ok(obj(json!({"calculators": list, "usage": "calc KEY NUMBER ...   numbers in the order of 'inputs'; 10k, 4.7u, 100n are fine"})));
        };
        let Some((k, what, inputs, run)) = CALCS.iter().find(|c| c.0 == lower(key)) else {
            let keys: Vec<&str> = CALCS.iter().map(|c| c.0).collect();
            return Err(format!("unknown calculator '{key}' (one of: {})", keys.join(", ")));
        };
        if args.len() - 1 != inputs.len() {
            return Err(format!("calc {k} needs {} numbers: {}", inputs.len(), inputs.join(" ")));
        }
        let mut v = vec![];
        for (name, w) in inputs.iter().zip(&args[1..]) {
            // current_mA, gbw_kHz, rds_on_mohm ...: the number is already in the prefixed unit, so '10m' would silently mean 0.01 mA
            let unit = name.rsplit('_').next().unwrap_or("");
            if unit.len() > 1 && (unit.starts_with('m') || unit.starts_with('k')) && w.chars().last().is_some_and(|c| c.is_ascii_alphabetic()) {
                return Err(format!("{name} is a plain number already in {unit} (write {} for 10 {unit}), not a value with an SI suffix: '{w}' was refused", "10"));
            }
            v.push(sim_core::parse_value(w).ok_or_else(|| format!("{name}: '{w}' is not a number (write 5, 3.3, 10k, 100n ...)"))?);
        }
        let out = run(&v)?;
        let rows: Vec<Value> = out.rows.iter().map(|(a, b)| json!({"label": a, "value": b})).collect();
        Ok(obj(json!({"calc": k, "designs": what, "value": out.primary, "rows": rows, "steps": out.steps, "cautions": out.cautions, "faults": out.faults})))
    }

    /// `ac F0 F1 [ppd=10] [probe=V(out),...]`: frequency response; keeps the result for `spec`.
    pub fn ac(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 2 {
            return Err("ac FSTART FSTOP [ppd=10] [probe=V(out),...]   (needs a source with an AC magnitude: 'value V1 \"SIN(0 1 1k) AC 1\"' or 'value V1 \"0 AC 1\"')".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive frequency (write 10, 1k, 100k ...)"));
        let (f0, f1) = (num(&pos[0])?, num(&pos[1])?);
        let ppd: usize = opt(&kv, "ppd").and_then(|s| s.parse().ok()).unwrap_or(10);
        let probes: Vec<String> = opt(&kv, "probe").map(|p| p.split(',').map(lower).collect()).unwrap_or_default();
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let r = sim_core::ac_analysis(&c, &sim_core::AcSettings { fstart: f0, fstop: f1, sweep: sim_core::AcSweep::Decade(ppd) }).map_err(|e| e.to_string())?;
        let at = |f: f64, db: &[f64], ph: &[f64]| {
            let i = r.freq.iter().enumerate().min_by(|a, b| (a.1 / f).ln().abs().total_cmp(&(b.1 / f).ln().abs())).map(|(i, _)| i).unwrap_or(0);
            json!({"hz": round6(r.freq[i]), "db": db[i], "phase_deg": ph[i]})
        };
        let mut sigs = serde_json::Map::new();
        for s in r.signals.iter().filter(|s| !probes.is_empty() && probes.iter().any(|p| lower(&s.name) == *p)) {
            let (db, ph, mag) = (s.db(), s.phase_deg(), s.mag());
            // a point per decade shows the shape without the whole sweep
            let mut decades = vec![];
            let mut f = f0;
            while f <= f1 * 1.0001 {
                decades.push(at(f, &db, &ph));
                f *= 10.0;
            }
            let mut o = json!({"unit": s.unit, "start": at(f0, &db, &ph), "per_decade": decades, "peak_db": db.iter().cloned().fold(f64::MIN, f64::max)});
            if let Some(f3) = sim_core::minus_3db(&r.freq, &mag) {
                o["minus_3db_hz"] = json!(f3);
            }
            if let Some(m) = sim_core::stability_margins(&r.freq, &db, &ph) {
                o["phase_margin_deg"] = json!(m.phase_margin);
            }
            sigs.insert(s.name.clone(), o);
        }
        let names: Vec<&str> = r.signals.iter().map(|s| s.name.as_str()).filter(|n| n.starts_with("V(")).take(30).collect();
        let hint = if probes.is_empty() { "add probe=V(out) to see a signal; these are the voltage signals" } else { "" };
        self.last_ac = Some(r.clone());
        Ok(obj(json!({"circuit_ok": gate.is_none(), "models": self.model_gaps(), "points": r.freq.len(), "signals": sigs, "available": names, "hint": hint, "warnings": r.warnings.iter().cloned().chain(gate).chain(self.skin_warning(f1)).collect::<Vec<_>>()})))
    }

    /// `thd probe=V(out) [f=1k] [t0=] [t1=] [harmonics=10]`: harmonic distortion of a signal of the last `tran` run.
    pub fn thd(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let usage = "thd probe=V(out) [f=1k] [t0=SEC] [t1=SEC] [harmonics=10]   (after 'tran' with a sine source; the base frequency is found by itself when f= is left out; the window is cut to whole periods and starts after the first fifth of the run unless t0= says)";
        let probe = opt(&kv, "probe").map(str::to_string).or_else(|| pos.first().cloned()).ok_or_else(|| usage.to_string())?;
        let r = self.last_tran.as_ref().ok_or("thd: run 'tran' first (the distortion is read from its result; a sine source and a few dozen periods)")?;
        let sg = r.signals.iter().find(|s| s.name.eq_ignore_ascii_case(probe.trim())).ok_or_else(|| {
            let names: Vec<&str> = r.signals.iter().map(|s| s.name.as_str()).filter(|n| n.starts_with("V(")).take(30).collect();
            format!("no signal {probe} in the last tran run (voltage signals: {})", names.join(", "))
        })?;
        let end = r.time.last().copied().unwrap_or(0.0);
        let num = |key: &str, default: f64| -> Result<f64, String> { match opt(&kv, key) { Some(s) => sim_core::parse_value(s).ok_or_else(|| format!("{key}={s} is not a number")), None => Ok(default) } };
        let (t0, t1) = (num("t0", end * 0.2)?, num("t1", end)?);
        let f0 = match opt(&kv, "f") { Some(s) => Some(sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("f={s} is not a positive frequency"))?), None => None };
        let n: usize = opt(&kv, "harmonics").and_then(|s| s.parse().ok()).unwrap_or(10);
        let t = sim_core::thd(&r.time, &sg.values, f0, t0, t1, n)?;
        let rows: Vec<Value> = t.harmonics.iter().map(|h| json!({"order": h.order, "hz": round6(h.freq), "amplitude": h.amp, "db_below_fundamental": if t.fundamental() > 0.0 && h.amp > 0.0 { Some(20.0 * (h.amp / t.fundamental()).log10()) } else { None }})).collect();
        let mut warnings: Vec<String> = vec![];
        if t.points_per_period < 40.0 {
            warnings.push(format!("only {:.0} waveform points per period: the straight lines between them add distortion of their own. Use a smaller step in 'tran' (a few hundred points per period)", t.points_per_period));
        }
        Ok(obj(json!({"probe": sg.name, "unit": sg.unit, "fundamental_hz": t.f0, "periods_used": t.periods, "window": [t.t0, t.t1], "dc": t.dc, "thd_percent": t.thd * 100.0, "thd_db": if t.thd > 0.0 { Some(20.0 * t.thd.log10()) } else { None }, "thd_plus_noise_percent": t.thd_n * 100.0, "harmonics": rows, "warnings": warnings})))
    }

    /// `worst NET [measure=dc|cutoff|rise|overshoot|settle|final] [tol_r=1] [tol_c=10] [tol_l=10] [tol_v=0] [f=10:1Meg] [tran=10m:10u] [top=8]`:
    /// the smallest and the largest value one result can take when every part sits at an end of its tolerance.
    pub fn worst(&mut self, args: &[String]) -> Res {
        use sim_core::{McMeasure, TranMetric};
        let (pos, kv) = options(args);
        let usage = "worst NET [measure=dc|cutoff|rise|overshoot|settle|final] [tol_r=1] [tol_c=10] [tol_l=10] [tol_v=0] [f=10:1Meg] [tran=10m:10u] [top=8]   (tolerances in percent; NET is the net measured)";
        let net = pos.first().ok_or_else(|| usage.to_string())?.trim().to_string();
        let tol = |key: &str, default: f64| -> Result<f64, String> {
            match opt(&kv, key) {
                Some(s) => s.parse::<f64>().ok().filter(|v| (0.0..100.0).contains(v)).map(|v| v / 100.0).ok_or_else(|| format!("{key}={s}: a percentage from 0 to 100 (5 = ±5 %)")),
                None => Ok(default / 100.0),
            }
        };
        let (tol_r, tol_c, tol_l, tol_v) = (tol("tol_r", 1.0)?, tol("tol_c", 10.0)?, tol("tol_l", 10.0)?, tol("tol_v", 0.0)?);
        let pair = |key: &str, default: (f64, f64)| -> Result<(f64, f64), String> {
            match opt(&kv, key) {
                Some(s) => {
                    let (a, b) = s.split_once(':').ok_or_else(|| format!("{key}={s}: write two values with a colon (like 10:1Meg)"))?;
                    Ok((sim_core::parse_value(a).ok_or_else(|| format!("{key}: '{a}' is not a number"))?, sim_core::parse_value(b).ok_or_else(|| format!("{key}: '{b}' is not a number"))?))
                }
                None => Ok(default),
            }
        };
        let how = opt(&kv, "measure").unwrap_or("dc").to_ascii_lowercase();
        let measure = match how.as_str() {
            "dc" => McMeasure::DcVoltage,
            "cutoff" => {
                let (f0, f1) = pair("f", (10.0, 1e6))?;
                McMeasure::Cutoff { fstart: f0, fstop: f1, points_per_decade: 50 }
            }
            "rise" | "overshoot" | "settle" | "final" => {
                let (tstop, tstep) = pair("tran", (10e-3, 10e-6))?;
                let metric = match how.as_str() {
                    "rise" => TranMetric::RiseTime,
                    "overshoot" => TranMetric::Overshoot,
                    "settle" => TranMetric::SettlingTime,
                    _ => TranMetric::FinalValue,
                };
                McMeasure::Tran { tstop, tstep, uic: false, metric }
            }
            other => return Err(format!("measure={other}: use dc, cutoff, rise, overshoot, settle or final")),
        };
        let top: usize = opt(&kv, "top").and_then(|s| s.parse().ok()).unwrap_or(8).clamp(1, 100);
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let w = sim_core::worst_case(&c, &sim_core::WcSettings { tol_r, tol_c, tol_l, tol_v, output: net, measure }).map_err(|e| e.to_string())?;
        let corner = |v: &[(String, bool)]| -> Vec<String> { v.iter().map(|(n, h)| format!("{n}{}", if *h { "+" } else { "-" })).collect() };
        let parts: Vec<Value> = w.parts.iter().take(top).map(|p| json!({"part": p.name, "tolerance_percent": round6(p.tol * 100.0), "at_low_end": p.low, "at_high_end": p.high})).collect();
        let pct = |v: f64| if w.nominal.abs() > 1e-30 { Some(round6((v - w.nominal) / w.nominal.abs() * 100.0)) } else { None };
        Ok(obj(json!({
            "circuit_ok": gate.is_none(), "net": w.output, "unit": w.unit, "nominal": w.nominal,
            "min": w.min, "max": w.max, "min_percent_from_nominal": pct(w.min), "max_percent_from_nominal": pct(w.max),
            "sum_of_parts": {"min": w.min_sum, "max": w.max_sum}, "likely_at_worst": {"min": w.min_rss, "max": w.max_rss},
            "min_corner": corner(&w.min_corner), "max_corner": corner(&w.max_corner),
            "parts": parts, "unsolved_parts": w.unsolved,
            "meaning": "min/max are solved circuits with every part at one end of its tolerance (+ = high end, - = low end); likely_at_worst treats the parts as independent",
            "warnings": gate.into_iter().collect::<Vec<_>>()
        })))
    }

    /// `zout NET F0 F1 [ppd=10] [ref=NET] [remove=RL,V1]`: the impedance seen from a net (every source off, a 1 A test current).
    pub fn zout(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 3 {
            return Err("zout NET FSTART FSTOP [ppd=10] [ref=NET] [remove=R1,V1]   (impedance seen from NET, against ground or ref=; sources are switched off. remove= takes parts out first: the load for the output impedance of a stage, the source and its series resistor for an input impedance)".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive frequency (write 10, 1k, 100k ...)"));
        let (f0, f1) = (num(&pos[1])?, num(&pos[2])?);
        let ppd: usize = opt(&kv, "ppd").and_then(|s| s.parse().ok()).unwrap_or(10);
        let remove: Vec<String> = opt(&kv, "remove").map(|p| p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let r = sim_core::impedance_analysis(&c, &pos[0], opt(&kv, "ref").as_deref(), &remove, &sim_core::AcSettings { fstart: f0, fstop: f1, sweep: sim_core::AcSweep::Decade(ppd) }).map_err(|e| e.to_string())?;
        let (mag, ph) = (r.mag(), r.phase_deg());
        let at = |f: f64| {
            let i = r.freq.iter().enumerate().min_by(|a, b| (a.1 / f).ln().abs().total_cmp(&(b.1 / f).ln().abs())).map(|(i, _)| i).unwrap_or(0);
            json!({"hz": round6(r.freq[i]), "ohm": mag[i], "phase_deg": ph[i], "looks_like": sim_core::z_character(ph[i])})
        };
        let mut decades = vec![];
        let mut f = f0;
        while f <= f1 * 1.0001 {
            decades.push(at(f));
            f *= 10.0;
        }
        let extrema: Vec<Value> = sim_core::z_extrema(&r.freq, &mag, 3.0).iter().map(|e| json!({"hz": round6(e.freq), "ohm": e.ohm, "kind": if e.peak { "peak (parallel resonance)" } else { "dip (series resonance)" }})).collect();
        let (lo, hi) = (mag.iter().cloned().fold(f64::MAX, f64::min), mag.iter().cloned().fold(0.0, f64::max));
        Ok(obj(json!({"circuit_ok": gate.is_none(), "net": pos[0], "points": r.freq.len(), "per_decade": decades, "min_ohm": lo, "max_ohm": hi, "peaks_and_dips": extrema, "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
    }

    /// `noise OUT[,REF] F0 F1 [ppd=10] [input=V1] [temp=27] [top=8]`: thermal and shot noise at an output node.
    pub fn noise(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.len() < 3 {
            return Err("noise OUT[,REF] FSTART FSTOP [ppd=10] [input=V1] [temp=27] [top=8]   (OUT: a net name, or 'A,B' for the difference of two nets; input= refers the noise to a V or I source)".into());
        }
        let num = |s: &str| sim_core::parse_value(s).filter(|v| *v > 0.0).ok_or_else(|| format!("'{s}' is not a positive frequency (write 10, 1k, 100k ...)"));
        let (f0, f1) = (num(&pos[1])?, num(&pos[2])?);
        let ppd: usize = opt(&kv, "ppd").and_then(|s| s.parse().ok()).unwrap_or(10);
        let top: usize = opt(&kv, "top").and_then(|s| s.parse().ok()).unwrap_or(8);
        let temp_c = opt(&kv, "temp").map(|s| sim_core::parse_value(s).ok_or_else(|| format!("temp={s} is not a number"))).transpose()?.unwrap_or(27.0);
        let gate = self.erc_gate()?;
        let c = self.circuit()?;
        let node = |s: &str| -> Result<String, String> {
            c.names.iter().find(|n| n.eq_ignore_ascii_case(s.trim())).cloned().ok_or_else(|| {
                let some: Vec<&str> = c.names.iter().map(String::as_str).filter(|n| !n.contains('.') && *n != "0").take(30).collect();
                format!("no net '{s}' in the circuit (nets: {})", some.join(", "))
            })
        };
        let (out, out_ref) = match pos[0].split_once(',') {
            Some((a, b)) => (node(a)?, Some(node(b)?)),
            None => (node(&pos[0])?, None),
        };
        let input = match opt(&kv, "input") {
            Some(s) => Some(c.elements.iter().map(|e| e.name()).find(|n| n.eq_ignore_ascii_case(s)).map(str::to_string).ok_or_else(|| format!("no source '{s}' for input= (a V or I source name, for example V1)"))?),
            None => None,
        };
        let st = sim_core::NoiseSettings { sweep: sim_core::AcSettings { fstart: f0, fstop: f1, sweep: sim_core::AcSweep::Decade(ppd) }, output: out.clone(), output_ref: out_ref, input, temp_c };
        let r = sim_core::noise_analysis(&c, &st).map_err(|e| e.to_string())?;
        let near = |f: f64| r.freq.iter().enumerate().min_by(|a, b| (a.1 / f).ln().abs().total_cmp(&(b.1 / f).ln().abs())).map(|(i, _)| i).unwrap_or(0);
        let (od, id) = (r.out_density(), r.in_density());
        let mut decades = vec![];
        let mut f = f0;
        while f <= f1 * 1.0001 {
            let i = near(f);
            let mut row = json!({"hz": round6(r.freq[i]), "out_v_per_rthz": od[i]});
            if let Some(d) = &id {
                row["in_per_rthz"] = json!(d[i]);
            }
            decades.push(row);
            f *= 10.0;
        }
        let total2 = r.out_rms * r.out_rms;
        let contribs: Vec<Value> = r.contributions.iter().take(top).map(|k| json!({"name": k.name, "out_rms_v": k.rms, "share_percent": if total2 > 0.0 { 100.0 * k.rms * k.rms / total2 } else { 0.0 }})).collect();
        let mut o = json!({"circuit_ok": gate.is_none(), "output": pos[0], "points": r.freq.len(), "out_rms_v": r.out_rms, "per_decade": decades, "top_contributors": contribs, "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()});
        if let Some(x) = r.in_rms {
            o["in_rms"] = json!(x);
        }
        Ok(obj(o))
    }

    /// `spec KIND TARGET LO HI [at=Hz]`: a pass / fail check of the last `dc` / `tran` / `ac` answer.
    pub fn spec(&self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        if pos.is_empty() {
            let kinds: Vec<Value> = SpecKind::ALL.iter().map(|k| json!({"kind": k.key(), "measures": k.label(), "unit": k.unit(), "needs": k.needs()})).collect();
            return Ok(obj(json!({"kinds": kinds, "usage": "spec KIND TARGET LO HI [at=Hz]   LO or HI may be '-' (no limit). Run the analysis first: dc, tran or ac"})));
        }
        if pos.len() != 4 {
            return Err("spec KIND TARGET LO HI [at=Hz]   (LO / HI: a number, or '-' for no limit; 'spec' alone lists the kinds)".into());
        }
        let kind = SpecKind::from_key(&lower(&pos[0])).ok_or_else(|| format!("unknown kind '{}' (run 'spec' to list them)", pos[0]))?;
        let bound = |s: &str| -> Result<Option<f64>, String> {
            if s == "-" { Ok(None) } else { sim_core::parse_value(s).map(Some).ok_or_else(|| format!("'{s}' is not a number (write 4.9, 10m, or '-')")) }
        };
        let (lo, hi) = (bound(&pos[2])?, bound(&pos[3])?);
        let at = opt(&kv, "at").map(|s| sim_core::parse_value(s).ok_or_else(|| format!("at={s} is not a frequency"))).transpose()?.unwrap_or(0.0);
        // names are matched without regard to case, as everywhere else in ccad
        let target = self.canonical_target(kind, &pos[1]);
        let spec = sim_core::Spec { kind, target: target.clone(), at, lo, hi };
        let inputs = SpecInputs { dc: self.last_dc.as_ref(), tran: self.last_tran.as_ref(), ac: self.last_ac.as_ref() };
        let e = sim_core::evaluate_spec(&spec, &inputs);
        Ok(obj(json!({"kind": kind.key(), "target": target, "lo": lo, "hi": hi, "value": e.value, "unit": kind.unit(), "pass": e.pass, "note": e.note})))
    }

    /// The target as the analysis result spells it (the user may have written another case).
    fn canonical_target(&self, kind: SpecKind, t: &str) -> String {
        let pick = |names: Vec<&String>| names.into_iter().find(|n| n.eq_ignore_ascii_case(t)).cloned().unwrap_or_else(|| t.to_string());
        match kind {
            SpecKind::DcVoltage => self.last_dc.as_ref().map_or(t.to_string(), |d| pick(d.voltages.iter().map(|(n, _)| n).collect())),
            SpecKind::DcCurrent => self.last_dc.as_ref().map_or(t.to_string(), |d| pick(d.elements.iter().map(|e| &e.name).collect())),
            SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => self.last_tran.as_ref().map_or(t.to_string(), |r| pick(r.signals.iter().map(|s| &s.name).collect())),
            _ => self.last_ac.as_ref().map_or(t.to_string(), |r| pick(r.signals.iter().map(|s| &s.name).collect())),
        }
    }
}

impl Session {
    /// With the board's copper in the circuit, a sweep above 1 MHz goes beyond what the wiring model knows: its resistance is the DC value.
    pub(crate) fn skin_warning(&self, fmax: f64) -> Option<String> {
        (self.post_layout && self.board.is_some() && fmax > 1e6).then(|| {
            "post-layout wiring: the copper resistance is the DC value. Above about 300 kHz the skin effect raises it (x1.4 at 10 MHz, x3.5-4 at 100 MHz for 35 um copper, FastHenry2) and above about 10 MHz the inductance of close traces is up to 10 % lower, so results above 1 MHz are optimistic for the resistance".to_string()
        })
    }
}
