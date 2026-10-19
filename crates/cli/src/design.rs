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
    ("regulator", "around a 78xx linear regulator (capacitors, dissipation)", &["vin_V", "vout_V", "current_mA"], |v| sim_core::regulator(v[0], v[1], v[2])),
    (
        "heat",
        "temperature of a linear regulator and the heat sink it needs",
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
            json!({"hz": r.freq[i], "db": db[i], "phase_deg": ph[i]})
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
        Ok(obj(json!({"circuit_ok": gate.is_none(), "points": r.freq.len(), "signals": sigs, "available": names, "hint": hint, "warnings": r.warnings.iter().cloned().chain(gate).collect::<Vec<_>>()})))
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
