//! Measuring an op amp's or a linear regulator's model the way the bundled ones are compared with their data sheets
//! (`crates/cli/tests/integration/opamp_models.rs`, `regulator_models.rs`), for a model just made in the part form (M3):
//! the same test circuits, built directly on a `Circuit` instead of through `ccad` scripts.
//!
//! Each function returns the measured figures by name; a figure the model cannot give (no unity-gain crossing, no
//! operating point) is left out rather than failing the rest.

use crate::*;

/// One figure measured on a model, with the circuit it was measured in.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    /// What it is ("aol", "gbw", "sr", "vos", "ib", "iq", "hu", "hl", "isc", "en", "pm"; "vout", "line", "load", "iq",
    /// "dropout", "ilim", "rr").
    pub key: &'static str,
    pub value: f64,
    /// The test conditions, in words.
    pub condition: String,
}

fn vdc(c: &mut Circuit, name: &str, p: usize, n: usize, v: f64) {
    c.add(Element::vdc(name, p, n, v));
}

fn res(c: &mut Circuit, name: &str, a: usize, b: usize, r: f64) {
    c.add(Element::Resistor { name: name.into(), a, b, r });
}

/// An op amp (`.subckt NAME inp inn out vp vn`) powered from ±`vs`/2 (ground in the middle).
fn opamp_circuit(name: &str, text: &str, vs: f64) -> Result<(Circuit, [usize; 3]), String> {
    let mut c = Circuit::new();
    let (inp, inn, out, vp, vn) = (c.node("inp"), c.node("inn"), c.node("out"), c.node("vp"), c.node("vn"));
    add_subckt_instance(&mut c, "U1", name, text, &[inp, inn, out, vp, vn])?;
    vdc(&mut c, "VP", vp, 0, vs / 2.0);
    vdc(&mut c, "VN", 0, vn, vs / 2.0);
    Ok((c, [inp, inn, out]))
}

/// The open-loop gain at `f` (dB) and its phase (degrees): the output fed back through a huge coil (closed at DC) and the
/// inverting input held by a huge capacitor, so the operating point is the follower's and the AC answer is open loop.
fn open_loop_at(name: &str, text: &str, vs: f64, rl: f64, f: f64) -> Option<(f64, f64)> {
    let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs).ok()?;
    c.add(Element::VSource { name: "VI".into(), p: inp, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
    c.add(Element::Inductor { name: "LF".into(), a: out, b: inn, l: 1e12 });
    c.add(Element::Capacitor { name: "CF".into(), a: inn, b: 0, c: 1000.0 });
    res(&mut c, "RL", out, 0, rl);
    let r = ac_analysis(&c, &AcSettings { fstart: f, fstop: f, sweep: AcSweep::Decade(1) }).ok()?;
    let s = r.signal("V(out)")?;
    let (re, im) = (s.re[0], s.im[0]);
    Some((20.0 * re.hypot(im).log10(), im.atan2(re).to_degrees()))
}

/// The op amp's figures on a ±`vs`/2 supply: offset, bias current and supply current (as a follower at 0 V), the open-loop
/// gain (at 0.01 Hz, 2 kΩ load), the unity-gain frequency and phase margin (1 MΩ), the slew rate (a 2 V step as a follower,
/// 10 kΩ), how far the output gets from each rail (10 kΩ to ground, 0.1 V of overdrive), the short-circuit current and the
/// input noise at 1 kHz.
pub fn opamp(name: &str, text: &str, vs: f64) -> Result<Vec<Measured>, String> {
    let mut out_rows = vec![];
    let sup = format!("電源 ±{} V", crate::value::format_value(vs / 2.0));
    // follower at 0 V: VOS, IB, IQ
    {
        let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs)?;
        vdc(&mut c, "VI", inp, 0, 0.0);
        res(&mut c, "RF", out, inn, 1e-3);
        let r = dc_op(&c).map_err(|e| e.to_string())?;
        let vout = r.voltage("out").unwrap_or(0.0);
        out_rows.push(Measured { key: "vos", value: vout.abs(), condition: format!("{sup}、ボルテージフォロワで入力 0 V の出力 (絶対値)") });
        if let Some(e) = r.element("VI") {
            out_rows.push(Measured { key: "ib", value: e.current.abs(), condition: format!("{sup}、IN+ に流れる電流 (絶対値)") });
        }
        if let Some(e) = r.element("VP") {
            out_rows.push(Measured { key: "iq", value: e.current.abs(), condition: format!("{sup}、無負荷のボルテージフォロワ") });
        }
    }
    if let Some((db, _)) = open_loop_at(name, text, vs, 2e3, 0.01) {
        out_rows.push(Measured { key: "aol", value: db, condition: format!("{sup}、0.01 Hz、負荷 2 kΩ") });
    }
    // unity-gain frequency: decade steps to bracket 0 dB, then bisection on log f
    let gain = |f: f64| open_loop_at(name, text, vs, 1e6, f);
    let mut lo = 1.0;
    let mut hi = None;
    for k in 0..10 {
        let f = 10f64.powi(k);
        match gain(f) {
            Some((db, _)) if db < 0.0 => {
                hi = Some(f);
                break;
            }
            Some(_) => lo = f,
            None => break,
        }
    }
    if let Some(mut hi) = hi {
        for _ in 0..40 {
            let mid = (lo * hi).sqrt();
            match gain(mid) {
                Some((db, _)) if db < 0.0 => hi = mid,
                Some(_) => lo = mid,
                None => break,
            }
        }
        let f = (lo * hi).sqrt();
        out_rows.push(Measured { key: "gbw", value: f, condition: format!("{sup}、開ループの利得が 0 dB になる周波数") });
        if let Some((_, ph)) = gain(f) {
            let pm = if ph <= 0.0 { 180.0 + ph } else { ph - 180.0 };
            out_rows.push(Measured { key: "pm", value: pm, condition: format!("{sup}、0 dB の周波数での位相") });
        }
    }
    // slew rate: a 2 V step as a follower, the steepest slope of the output
    if let Some(&Measured { value: gbw, .. }) = out_rows.iter().find(|m| m.key == "gbw") {
        let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs)?;
        let edge = 1.0 / gbw;
        let tstop = 400.0 * edge;
        c.add(Element::VSource { name: "VI".into(), p: inp, n: 0, wave: Waveform::Pulse { v1: -1.0, v2: 1.0, td: tstop * 0.1, tr: 1e-9, tf: 1e-9, pw: tstop, per: 2.0 * tstop }, ac: None });
        res(&mut c, "RF", out, inn, 1e-3);
        res(&mut c, "RL", out, 0, 1e4);
        if let Ok(t) = transient(&c, &TranSettings { tstop, tstep: tstop / 4000.0, uic: false })
            && let Some(s) = t.signal("V(out)")
        {
            // the slope between 20 % and 80 % of the step (as the bundled models' comparison measures it)
            let cross = |level: f64| (1..t.time.len()).find(|&k| s.values[k - 1] < level && s.values[k] >= level).map(|k| {
                let (t0, t1, v0, v1) = (t.time[k - 1], t.time[k], s.values[k - 1], s.values[k]);
                t0 + (level - v0) / (v1 - v0) * (t1 - t0)
            });
            if let (Some(t20), Some(t80)) = (cross(-0.6), cross(0.6))
                && t80 > t20
            {
                out_rows.push(Measured { key: "sr", value: 1.2 / (t80 - t20), condition: format!("{sup}、ボルテージフォロワに 2 V の段の 20〜80 %、負荷 10 kΩ") });
            }
        }
    }
    // the output's distance from each rail, and the short-circuit current
    for (key, over) in [("hu", 0.1), ("hl", -0.1)] {
        let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs)?;
        vdc(&mut c, "VI", inp, 0, over);
        vdc(&mut c, "VR", inn, 0, 0.0);
        res(&mut c, "RL", out, 0, 1e4);
        if let Ok(r) = dc_op(&c) {
            let v = r.voltage("out").unwrap_or(0.0);
            let d = if over > 0.0 { vs / 2.0 - v } else { v + vs / 2.0 };
            out_rows.push(Measured { key, value: d, condition: format!("{sup}、負荷 10 kΩ、{}の電源からの距離", if over > 0.0 { "正" } else { "負" }) });
        }
    }
    {
        let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs)?;
        vdc(&mut c, "VI", inp, 0, 0.1);
        vdc(&mut c, "VR", inn, 0, 0.0);
        vdc(&mut c, "VS", out, 0, 0.0);
        if let Ok(r) = dc_op(&c)
            && let Some(e) = r.element("VS")
        {
            out_rows.push(Measured { key: "isc", value: e.current.abs(), condition: format!("{sup}、出力を 0 V に短絡 (吐き出し)") });
        }
    }
    // input noise at 1 kHz: the follower's output noise
    {
        let (mut c, [inp, inn, out]) = opamp_circuit(name, text, vs)?;
        vdc(&mut c, "VI", inp, 0, 0.0);
        res(&mut c, "RF", out, inn, 1e-3);
        let st = NoiseSettings { sweep: AcSettings { fstart: 1e3, fstop: 1e3, sweep: AcSweep::Decade(1) }, output: "out".into(), output_ref: None, input: None, temp_c: 26.85 };
        if let Ok(r) = noise_analysis(&c, &st)
            && let Some(p) = r.out_psd.first()
        {
            out_rows.push(Measured { key: "en", value: p.sqrt(), condition: format!("{sup}、ボルテージフォロワの出力の雑音密度 (1 kHz)") });
        }
    }
    Ok(out_rows)
}

/// A regulator (`.subckt NAME in gnd out`, or `in adj out` when `adjustable`): the output (or the reference OUT − ADJ with ADJ
/// grounded) at `vin0` and `light` A of load, the line regulation from `vin0` to `vin0` + 5 V (V/V), the load regulation up to
/// `heavy` A (Ω), the ground current, the dropout at `heavy` A (the input held `expected_dropout` − 0.3 V above the output), the
/// short-circuit current at the input `vin0`, and the ripple rejection at `rr_f` Hz.
pub struct RegulatorTest {
    pub adjustable: bool,
    pub vin0: f64,
    pub light: f64,
    pub heavy: f64,
    pub expected_dropout: f64,
    pub rr_f: f64,
}

fn regulator_circuit(name: &str, text: &str, t: &RegulatorTest, vin: f64, load: Option<f64>) -> Result<Circuit, String> {
    let mut c = Circuit::new();
    let (inp, mid, out) = (c.node("in"), c.node("mid"), c.node("out"));
    add_subckt_instance(&mut c, "U1", name, text, &[inp, mid, out])?;
    c.add(Element::VSource { name: "VIN".into(), p: inp, n: 0, wave: Waveform::Dc(vin), ac: Some((1.0, 0.0)) });
    // the ground (or ADJ) pin through a 0 V source, to read its current
    vdc(&mut c, "VG", mid, 0, 0.0);
    if let Some(i) = load {
        c.add(Element::ISource { name: "IL".into(), from: out, to: 0, wave: Waveform::Dc(i), ac: None });
    }
    let _ = t;
    Ok(c)
}

pub fn regulator(name: &str, text: &str, t: &RegulatorTest) -> Result<Vec<Measured>, String> {
    let mut rows = vec![];
    let vout_at = |vin: f64, load: f64| -> Option<f64> {
        let c = regulator_circuit(name, text, t, vin, Some(load)).ok()?;
        dc_op(&c).ok()?.voltage("out")
    };
    let what = if t.adjustable { "基準電圧 (OUT − ADJ、ADJ を GND に)" } else { "出力電圧" };
    let v0 = vout_at(t.vin0, t.light).ok_or("動作点が求まりません")?;
    rows.push(Measured { key: "vout", value: v0, condition: format!("{what}、入力 {} V、負荷 {} A", crate::value::format_value(t.vin0), crate::value::format_value(t.light)) });
    if let Some(v1) = vout_at(t.vin0 + 5.0, t.light) {
        rows.push(Measured { key: "line", value: (v1 - v0) / 5.0, condition: format!("入力 {} → {} V の出力の変化 ÷ 5 V", crate::value::format_value(t.vin0), crate::value::format_value(t.vin0 + 5.0)) });
    }
    if let Some(v1) = vout_at(t.vin0, t.heavy) {
        rows.push(Measured { key: "load", value: (v0 - v1) / (t.heavy - t.light), condition: format!("負荷 {} → {} A の出力の下がり ÷ 電流の変化", crate::value::format_value(t.light), crate::value::format_value(t.heavy)) });
    }
    if let Ok(c) = regulator_circuit(name, text, t, t.vin0, Some(t.light))
        && let Ok(r) = dc_op(&c)
        && let Some(e) = r.element("VG")
    {
        rows.push(Measured { key: "iq", value: e.current.abs(), condition: format!("{} ピンの電流、負荷 {} A", if t.adjustable { "ADJ" } else { "GND" }, crate::value::format_value(t.light)) });
    }
    {
        let vin = v0 + t.expected_dropout - 0.3;
        if let Some(v) = vout_at(vin, t.heavy) {
            rows.push(Measured { key: "dropout", value: vin - v, condition: format!("入力を 出力 + ドロップアウト − 0.3 V ({} V) に下げ、負荷 {} A", crate::value::format_value(vin), crate::value::format_value(t.heavy)) });
        }
    }
    if let Ok(mut c) = regulator_circuit(name, text, t, t.vin0, None) {
        let out = c.node("out");
        vdc(&mut c, "VS", out, 0, 0.0);
        if let Ok(r) = dc_op(&c)
            && let Some(e) = r.element("VS")
        {
            rows.push(Measured { key: "ilim", value: e.current.abs(), condition: format!("出力を短絡、入力 {} V", crate::value::format_value(t.vin0)) });
        }
    }
    if let Ok(c) = regulator_circuit(name, text, t, t.vin0, Some(t.light))
        && let Ok(r) = ac_analysis(&c, &AcSettings { fstart: t.rr_f, fstop: t.rr_f, sweep: AcSweep::Decade(1) })
        && let Some(s) = r.signal("V(out)")
    {
        let g = s.re[0].hypot(s.im[0]);
        if g > 0.0 {
            rows.push(Measured { key: "rr", value: -20.0 * g.log10(), condition: format!("{} Hz、入力から出力へ", crate::value::format_value(t.rr_f)) });
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> (String, String) {
        let def: part_lib::PartDef = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|p| p.name == name).unwrap();
        let sim = def.sim.unwrap();
        (sim.name, sim.text)
    }

    /// The bundled LM358 and LM7805 measure here as the CLI's comparison tables have them (part-lib/model_checks.json):
    /// LM358 AOL 99.8 dB, GBW 700 kHz, SR 0.30 V/µs, VOS 3.0 mV, IB 18.5 nA, IQ 350 µA, 1.43 V from the top rail, en 40 nV/√Hz;
    /// LM7805 5.000 V, line 0.5 mV over 12.5 V, ground current 3.13 mA, 83 dB at 120 Hz, 0.6 A into a short at 35 V.
    #[test]
    fn the_bundled_op_amp_and_regulator_measure_as_their_tables() {
        let (name, text) = model("LM358");
        let m = opamp(&name, &text, 5.0).unwrap();
        let get = |k: &str| m.iter().find(|x| x.key == k).unwrap_or_else(|| panic!("no {k}: {m:?}")).value;
        let mut bad = vec![];
        for (k, want, tol) in [("aol", 99.8, 0.02), ("gbw", 700e3, 0.03), ("sr", 0.30e6, 0.05), ("vos", 3.0e-3, 0.03), ("ib", 18.5e-9, 0.1), ("iq", 350e-6, 0.03), ("hu", 1.43, 0.05), ("en", 40e-9, 0.05)] {
            let v = get(k);
            if (v / want - 1.0).abs() > tol {
                bad.push(format!("LM358 {k}: {v:e}, want {want:e}"));
            }
        }
        let (name, text) = model("LM7805");
        let t = RegulatorTest { adjustable: false, vin0: 10.0, light: 5e-3, heavy: 1.0, expected_dropout: 2.0, rr_f: 120.0 };
        let m = regulator(&name, &text, &t).unwrap();
        let get = |k: &str| m.iter().find(|x| x.key == k).unwrap_or_else(|| panic!("no {k}: {m:?}")).value;
        for (k, want, tol) in [("vout", 5.0, 0.002), ("line", 0.5e-3 / 12.5, 0.05), ("iq", 3.13e-3, 0.05), ("rr", 83.0, 0.01)] {
            let v = get(k);
            if (v / want - 1.0).abs() > tol {
                bad.push(format!("LM7805 {k}: {v:e}, want {want:e}"));
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }
}
