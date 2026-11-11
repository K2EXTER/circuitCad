//! The bundled op-amp models measured the way their data sheets measure them, next to the data sheet's numbers
//! (`docs/model-policy.md`, section 5). Runs the real `ccad` binary on small test circuits.
//!
//! `opamp_models_match_their_data_sheets` fails when a model leaves its data sheet by more than `tolerance`.
//! `cargo test -p cli --test opamp_models -- --ignored write_opamp_model_report` writes `docs/models/opamps.md`
//! and the tables shown next to the parts (`crates/part-lib/model_checks.json`).
//!
//! The data sheet numbers are typed in from the data sheets named in `SOURCE` (read 2026-11-11), never from memory.

#[path = "common/checks.rs"]
mod checks;

use serde_json::Value;
use std::process::{Command, Stdio};

fn run(script: &str) -> Vec<Value> {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let lib = std::env::temp_dir().join(format!("ccad-opamp-lib-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ccad"))
        .args(["--lib", lib.to_str().unwrap(), "--keep-going", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&lib);
    String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn answer<'a>(a: &'a [Value], cmd: &str) -> &'a Value {
    let v = a.iter().rev().find(|v| v["cmd"] == cmd).unwrap_or_else(|| panic!("no answer for {cmd}: {a:?}"));
    assert_eq!(v["ok"], true, "{cmd} failed: {v}");
    v
}

/// How the data sheet powers the part.
#[derive(Clone, Copy)]
struct Supply {
    /// Total supply voltage (V+ − V−).
    vs: f64,
    /// Split (±vs/2 around ground) or single (V− at ground, the reference at vs/2).
    split: bool,
}

struct Part {
    name: &'static str,
    /// Pin names: positive supply, negative supply.
    vplus: &'static str,
    vminus: &'static str,
    /// The data sheet the rows come from.
    source: &'static str,
    rows: &'static [Row],
}

#[derive(Clone, Copy)]
enum Item {
    /// Open-loop gain at DC (dB), output loaded by `rl` to the reference.
    Aol { rl: f64 },
    /// Unity-gain frequency of the open-loop gain (Hz).
    Gbw,
    /// Phase margin of the open loop at unity gain (degrees).
    PhaseMargin,
    /// Slew rate of a voltage follower, rising edge of a `step` volt step, `rl` load (V/µs).
    Slew { step: f64, rl: f64 },
    /// Input offset voltage of a follower at the reference (V).
    Offset,
    /// Input bias current of a follower at the reference (A).
    Bias,
    /// Quiescent current of one amplifier, follower at the reference, no load (A).
    Supply,
    /// Output swing: distance from the positive rail with the input overdriven by 1 V, `rl` to the reference (V).
    SwingHigh { rl: f64 },
    /// Distance from the negative rail, `rl` to the negative rail (`to_rail`) or to the reference (V).
    SwingLow { rl: f64, to_rail: bool },
    /// Output current into a short to the reference, input overdriven (A).
    Short,
    /// Input voltage noise density at 1 kHz, follower (V/√Hz).
    Noise1k,
}

struct Row {
    item: Item,
    supply: Supply,
    /// Data sheet min / typ / max (None = not given). Same unit as the measurement.
    min: Option<f64>,
    typ: Option<f64>,
    max: Option<f64>,
    /// Where in the data sheet, and its conditions.
    at: &'static str,
}

const fn s(vs: f64, split: bool) -> Supply {
    Supply { vs, split }
}

/// LM358 (the original part, not the B version): TI SLOS068AB (revised Oct 2024), 5.7.
const LM358: &[Row] = &[
    Row { item: Item::Aol { rl: 2e3 }, supply: s(15.0, false), min: Some(87.96), typ: Some(100.0), max: None, at: "5.7 AOL: VS = 15 V, VO = 1〜11 V, RL ≥ 2 kΩ。25 / 100 V/mV" },
    Row { item: Item::Gbw, supply: s(5.0, false), min: None, typ: Some(0.7e6), max: None, at: "5.7 GBW(VS = 5 V)" },
    Row { item: Item::Slew { step: 2.0, rl: 1e6 }, supply: s(5.0, false), min: None, typ: Some(0.3), max: None, at: "5.7 SR: G = +1(VS = 5 V)" },
    Row { item: Item::Offset, supply: s(5.0, false), min: None, typ: Some(3e-3), max: Some(7e-3), at: "5.7 VOS: VS = 5〜30 V(絶対値)" },
    Row { item: Item::Bias, supply: s(5.0, false), min: None, typ: Some(20e-9), max: Some(250e-9), at: "5.7 IB(ピンから出る向き、絶対値)" },
    Row { item: Item::Supply, supply: s(5.0, false), min: None, typ: Some(350e-6), max: Some(600e-6), at: "5.7 IQ: 1 回路あたり、VO = 2.5 V、無負荷" },
    Row { item: Item::SwingHigh { rl: 2e3 }, supply: s(5.0, false), min: None, typ: None, max: Some(1.5), at: "5.7 VO 正側: VS = 5 V、RL ≥ 2 kΩ(電源から 1.5 V)" },
    Row { item: Item::SwingLow { rl: 10e3, to_rail: true }, supply: s(5.0, false), min: None, typ: Some(5e-3), max: Some(20e-3), at: "5.7 VO 負側: VS = 5 V、RL ≤ 10 kΩ を V− へ" },
    Row { item: Item::Short, supply: s(10.0, false), min: None, typ: Some(40e-3), max: Some(60e-3), at: "5.7 ISC: VS = 10 V、VO = VS/2" },
    Row { item: Item::Noise1k, supply: s(5.0, false), min: None, typ: Some(40e-9), max: None, at: "5.7 en: f = 1 kHz" },
];

/// LM324 (the original part): TI SLOS066AE (revised Sep 2025), 6.7 and 6.10.
const LM324: &[Row] = &[
    Row { item: Item::Aol { rl: 2e3 }, supply: s(15.0, false), min: Some(87.96), typ: Some(100.0), max: None, at: "6.7 AVD: VCC = 15 V、VO = 1〜11 V、RL ≥ 2 kΩ。25 / 100 V/mV" },
    Row { item: Item::Gbw, supply: s(30.0, true), min: None, typ: Some(1.2e6), max: None, at: "6.10 B1: ±15 V、RL = 1 MΩ、CL = 20 pF" },
    Row { item: Item::Slew { step: 10.0, rl: 1e6 }, supply: s(30.0, true), min: None, typ: Some(0.5), max: None, at: "6.10 SR: ±15 V、VI = ±10 V" },
    Row { item: Item::Offset, supply: s(5.0, false), min: None, typ: Some(3e-3), max: Some(7e-3), at: "6.7 VIO(絶対値)" },
    Row { item: Item::Bias, supply: s(5.0, false), min: None, typ: Some(20e-9), max: Some(250e-9), at: "6.7 IIB(絶対値)" },
    Row { item: Item::Supply, supply: s(5.0, false), min: None, typ: Some(0.7e-3 / 4.0), max: Some(1.2e-3 / 4.0), at: "6.7 ICC: 4 回路で 0.7 / 1.2 mA を 4 で割った値" },
    Row { item: Item::SwingHigh { rl: 2e3 }, supply: s(5.0, false), min: None, typ: None, max: Some(1.5), at: "6.7 VOH: RL = 2 kΩ(VCC − 1.5 V)" },
    Row { item: Item::SwingLow { rl: 10e3, to_rail: true }, supply: s(5.0, false), min: None, typ: Some(5e-3), max: Some(20e-3), at: "6.7 VOL: RL ≤ 10 kΩ" },
    Row { item: Item::Short, supply: s(10.0, true), min: None, typ: Some(40e-3), max: Some(60e-3), at: "6.7 IOS: ±5 V、VO = 0 V" },
    Row { item: Item::Noise1k, supply: s(30.0, true), min: None, typ: Some(35e-9), max: None, at: "6.10 Vn: f = 1 kHz" },
];

/// TL072 (C grade, SOIC): TI SLOS080W (revised Jul 2025), 5.8 and 5.9.
const TL072: &[Row] = &[
    Row { item: Item::Aol { rl: 2e3 }, supply: s(30.0, true), min: Some(87.96), typ: Some(106.02), max: None, at: "5.8 AOL: ±15 V(TL07xC)。25 / 200 V/mV" },
    Row { item: Item::Gbw, supply: s(30.0, true), min: None, typ: Some(5.25e6), max: None, at: "5.8 GBW: NS・PS パッケージと M 以外(SOIC の D はこちら)。NS・PS は 3 MHz" },
    Row { item: Item::PhaseMargin, supply: s(30.0, true), min: None, typ: Some(56.0), max: None, at: "5.9 位相余裕: G = +1、RL = 10 kΩ、CL = 20 pF" },
    Row { item: Item::Slew { step: 10.0, rl: 2e3 }, supply: s(30.0, true), min: Some(8.0), typ: Some(20.0), max: None, at: "5.9 SR: VI = 10 V、RL = 2 kΩ、CL = 100 pF" },
    Row { item: Item::Offset, supply: s(30.0, true), min: None, typ: Some(3e-3), max: Some(10e-3), at: "5.8 VOS: TL07xC(絶対値)" },
    Row { item: Item::Bias, supply: s(30.0, true), min: None, typ: Some(65e-12), max: Some(200e-12), at: "5.8 IB" },
    Row { item: Item::Supply, supply: s(30.0, true), min: None, typ: Some(1.4e-3), max: Some(2.5e-3), at: "5.8 IQ: 1 回路あたり、VO = 0 V、無負荷" },
    Row { item: Item::SwingHigh { rl: 10e3 }, supply: s(30.0, true), min: None, typ: Some(1.5), max: Some(3.0), at: "5.8 VOM: RL = 10 kΩ で ±13.5 V typ、±12 V min(電源からの距離に直した)" },
    Row { item: Item::Noise1k, supply: s(30.0, true), min: None, typ: Some(37e-9), max: None, at: "5.9 eN: f = 1 kHz(NS・PS と M 以外)" },
];

/// NE5532: TI SLOS075K (revised Dec 2025), 5.5 and 5.6.
const NE5532: &[Row] = &[
    Row { item: Item::Aol { rl: 2e3 }, supply: s(30.0, true), min: Some(87.96), typ: Some(100.0), max: None, at: "5.5 AVD: RL ≥ 2 kΩ、VO = ±10 V。25 / 100 V/mV" },
    Row { item: Item::Gbw, supply: s(30.0, true), min: None, typ: Some(12e6), max: None, at: "5.5 B1(単位利得の帯域)" },
    Row { item: Item::Slew { step: 10.0, rl: 2e3 }, supply: s(30.0, true), min: None, typ: Some(5.0), max: None, at: "5.6 SR: 単位利得" },
    Row { item: Item::Offset, supply: s(30.0, true), min: None, typ: Some(0.5e-3), max: Some(4e-3), at: "5.5 VIO(絶対値)" },
    Row { item: Item::Bias, supply: s(30.0, true), min: None, typ: Some(200e-9), max: Some(800e-9), at: "5.5 IIB" },
    Row { item: Item::Supply, supply: s(30.0, true), min: None, typ: Some(3e-3), max: Some(8e-3), at: "5.5 ICC: 2 回路で 6 / 16 mA を 2 で割った値" },
    Row { item: Item::Short, supply: s(30.0, true), min: None, typ: Some(38e-3), max: None, at: "5.5 IOS" },
    Row { item: Item::Noise1k, supply: s(30.0, true), min: None, typ: Some(5e-9), max: None, at: "5.6 Vn: f = 1 kHz" },
];

/// MCP6001 / MCP6002: Microchip DS20001733L, DC and AC electrical specifications.
const MCP600X: &[Row] = &[
    Row { item: Item::Aol { rl: 10e3 }, supply: s(5.5, false), min: Some(88.0), typ: Some(112.0), max: None, at: "DC: AOL、VOUT = 0.3 V〜VDD − 0.3 V" },
    Row { item: Item::Gbw, supply: s(5.5, false), min: None, typ: Some(1.0e6), max: None, at: "AC: GBWP" },
    Row { item: Item::PhaseMargin, supply: s(5.5, false), min: None, typ: Some(90.0), max: None, at: "AC: PM、G = +1" },
    Row { item: Item::Slew { step: 2.0, rl: 10e3 }, supply: s(5.5, false), min: None, typ: Some(0.6), max: None, at: "AC: SR" },
    Row { item: Item::Offset, supply: s(5.5, false), min: None, typ: None, max: Some(4.5e-3), at: "DC: VOS ±4.5 mV(VCM = VSS)" },
    Row { item: Item::Bias, supply: s(5.5, false), min: None, typ: Some(1e-12), max: None, at: "DC: IB ±1 pA" },
    Row { item: Item::Supply, supply: s(5.5, false), min: Some(50e-6), typ: Some(100e-6), max: Some(170e-6), at: "DC: IQ、VDD = 5.5 V" },
    Row { item: Item::SwingHigh { rl: 10e3 }, supply: s(5.5, false), min: None, typ: None, max: Some(25e-3), at: "DC: VOH、VDD = 5.5 V、入力 0.5 V 超過(VDD − 25 mV)" },
    Row { item: Item::SwingLow { rl: 10e3, to_rail: false }, supply: s(5.5, false), min: None, typ: None, max: Some(25e-3), at: "DC: VOL(VSS + 25 mV)" },
    Row { item: Item::Short, supply: s(5.5, false), min: None, typ: Some(23e-3), max: None, at: "DC: ISC、VDD = 5.5 V" },
    Row { item: Item::Noise1k, supply: s(5.5, false), min: None, typ: Some(28e-9), max: None, at: "AC: eni、f = 1 kHz" },
];

const PARTS: &[Part] = &[
    Part { name: "LM358", vplus: "V+", vminus: "V-", source: "TI SLOS068AB(2024-10 改訂)", rows: LM358 },
    Part { name: "LM324", vplus: "V+", vminus: "V-", source: "TI SLOS066AE(2025-09 改訂)", rows: LM324 },
    Part { name: "TL072", vplus: "V+", vminus: "V-", source: "TI SLOS080W(2025-07 改訂)", rows: TL072 },
    Part { name: "NE5532", vplus: "V+", vminus: "V-", source: "TI SLOS075K(2025-12 改訂)", rows: NE5532 },
    Part { name: "MCP6001", vplus: "VDD", vminus: "VSS", source: "Microchip DS20001733L", rows: MCP600X },
    Part { name: "MCP6002", vplus: "VDD", vminus: "VSS", source: "Microchip DS20001733L", rows: MCP600X },
];

/// The op amp with its supplies, as a script whose nets are VCC, VEE and REF (the mid point every input source and load
/// is tied to). A split supply has REF at ground; a single supply has VEE at ground and REF at vs/2. Returns the script
/// and the voltage of REF.
fn powered(p: &Part, sup: Supply) -> (String, f64) {
    let h = sup.vs / 2.0;
    let mut sc = format!("new
add {} as=U1
", p.name);
    if sup.split {
        sc += &format!("add V {h} as=VP
add V {h} as=VN
net VCC VP.+ U1.{vp}
net GND VP.- VN.+
net VEE VN.- U1.{vm}
", vp = p.vplus, vm = p.vminus);
        (sc, 0.0)
    } else {
        sc += &format!("add V {} as=VP
add V {h} as=VM
net VCC VP.+ U1.{vp}
net GND VP.- VM.- U1.{vm}
net REF VM.+
", sup.vs, vp = p.vplus, vm = p.vminus);
        (sc, h)
    }
}

/// The circuit's own nets, with REF meaning ground for a split supply and VEE meaning ground for a single one.
fn tie(sup: Supply, script: String) -> String {
    if sup.split {
        script.replace("net REF ", "net GND ")
    } else {
        script.replace("net VEE ", "net GND ")
    }
}

/// Unity-gain frequency from the decade points of an open-loop answer, assuming −20 dB/decade between them.
fn unity_hz(sig: &Value) -> Option<f64> {
    let pts: Vec<(f64, f64)> = sig["per_decade"].as_array()?.iter().map(|p| (p["hz"].as_f64().unwrap(), p["db"].as_f64().unwrap())).collect();
    pts.windows(2).find(|w| w[0].1 >= 0.0 && w[1].1 < 0.0).map(|w| {
        let (f0, d0) = w[0];
        let (f1, d1) = w[1];
        let t = d0 / (d0 - d1);
        10f64.powf(f0.log10() + t * (f1.log10() - f0.log10()))
    })
}

/// One measurement of the model, in the row's unit. Input sources and loads are tied to REF and given relative to it.
fn measure(p: &Part, row: &Row) -> Option<f64> {
    let sup = row.supply;
    let (base, refv) = powered(p, sup);
    let go = |body: String| run(&tie(sup, format!("{base}{body}")));
    match row.item {
        Item::Aol { .. } | Item::Gbw | Item::PhaseMargin => {
            let rl = if let Item::Aol { rl } = row.item { rl } else { 1e6 };
            // the output is fed back through a huge coil (closed at DC, open for AC) and the inverting input is held
            // by a huge capacitor: the AC answer is the open-loop gain around the right operating point
            let a = go(format!(
                "add V 1 as=VI
value VI DC 0 AC 1
add L 1e9 as=LF
add C 1 as=CF
add R {rl} as=RL
                 net IN VI.+ U1.IN+
net REF VI.- CF.2 RL.2
net INN U1.IN- LF.1 CF.1
net OUT U1.OUT LF.2 RL.1
                 ac 0.01 1000Meg ppd=10 probe=V(out)
"
            ));
            let sig = &answer(&a, "ac")["signals"]["V(OUT)"];
            match row.item {
                Item::Aol { .. } => sig["start"]["db"].as_f64(),
                Item::Gbw => {
                    // the decade points only give a first guess (they assume −20 dB/decade); then single-frequency
                    // runs close in on 0 dB (secant on log f)
                    let gain_at = |f: f64| -> Option<f64> {
                        let a = go(format!(
                            "add V 1 as=VI
value VI DC 0 AC 1
add L 1e9 as=LF
add C 1 as=CF
add R {rl} as=RL
                             net IN VI.+ U1.IN+
net REF VI.- CF.2 RL.2
net INN U1.IN- LF.1 CF.1
net OUT U1.OUT LF.2 RL.1
                             ac {f} {f} ppd=1 probe=V(out)
"
                        ));
                        answer(&a, "ac")["signals"]["V(OUT)"]["start"]["db"].as_f64()
                    };
                    let mut f0 = unity_hz(sig)?;
                    let mut d0 = gain_at(f0)?;
                    let mut f1 = f0 * 10f64.powf(d0 / 20.0);
                    for _ in 0..12 {
                        let d1 = gain_at(f1)?;
                        if d1.abs() < 1e-3 || (d1 - d0).abs() < 1e-12 {
                            break;
                        }
                        let next = 10f64.powf(f1.log10() - d1 * (f1.log10() - f0.log10()) / (d1 - d0));
                        (f0, d0, f1) = (f1, d1, next);
                    }
                    Some(f1)
                }
                _ => sig["phase_margin_deg"].as_f64(),
            }
        }
        Item::Slew { step, rl } => {
            let edge = step / (row.typ.unwrap_or(1.0) * 1e6);
            let tstop = 12.0 * edge.max(1e-7);
            let a = go(format!(
                "add V 1 as=VI
value VI PULSE({lo} {hi} {d} 1n 1n {w} {per})
add R {rl} as=RL
                 net IN VI.+ U1.IN+
net REF VI.- RL.2
net OUT U1.OUT U1.IN- RL.1
                 tran {tstop} {ts} probe=V(out) samples=200 record=probes
",
                lo = -step / 2.0,
                hi = step / 2.0,
                d = tstop * 0.1,
                w = tstop,
                per = tstop * 3.0,
                ts = tstop / 2000.0
            ));
            let samples: Vec<(f64, f64)> =
                answer(&a, "tran")["signals"]["V(OUT)"]["samples"].as_array()?.iter().map(|s| (s[0].as_f64().unwrap(), s[1].as_f64().unwrap())).collect();
            // the slope between 20 % and 80 % of the step, in V/µs
            let (v20, v80) = (refv - step / 2.0 + 0.2 * step, refv - step / 2.0 + 0.8 * step);
            let cross = |level: f64| {
                samples.windows(2).find(|w| w[0].1 < level && w[1].1 >= level).map(|w| w[0].0 + (level - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0))
            };
            let (t20, t80) = (cross(v20)?, cross(v80)?);
            Some((v80 - v20) / (t80 - t20) / 1e6)
        }
        Item::Offset | Item::Bias | Item::Supply => {
            let a = go("add V 0 as=VI
net IN VI.+ U1.IN+
net REF VI.-
net OUT U1.OUT U1.IN-
dc probe=V(out),V(in),I(VI),I(VP)
".into());
            let pr = &answer(&a, "dc")["probes"];
            let v = |k: &str| pr[k].as_f64();
            match row.item {
                Item::Offset => Some((v("V(out)")? - v("V(in)")?).abs()),
                Item::Bias => Some(v("I(VI)")?.abs()),
                _ => Some(v("I(VP)")?.abs()),
            }
        }
        Item::SwingHigh { rl } | Item::SwingLow { rl, .. } => {
            let high = matches!(row.item, Item::SwingHigh { .. });
            let to_rail = matches!(row.item, Item::SwingLow { to_rail: true, .. });
            let load = if to_rail { "net VEE RL.2
" } else { "net REF RL.2
" };
            let drive = if high { 1.0 } else { -1.0 };
            let a = go(format!(
                "add V {drive} as=VI
add R {rl} as=RL
net REF VI.- U1.IN-
net IN VI.+ U1.IN+
net OUT U1.OUT RL.1
{load}                 dc probe=V(out)
"
            ));
            let pr = &answer(&a, "dc")["probes"];
            let out = pr["V(out)"].as_f64()?;
            // the supplies are ideal sources: the rails are where they are set
            let (vcc, vee) = if sup.split { (sup.vs / 2.0, -sup.vs / 2.0) } else { (sup.vs, 0.0) };
            Some(if high { vcc - out } else { out - vee })
        }
        Item::Short => {
            let a = go("add V 1 as=VI
add R 1 as=RS
net REF VI.- U1.IN- RS.2
net IN VI.+ U1.IN+
net OUT U1.OUT RS.1
dc probe=I(RS)
".into());
            Some(answer(&a, "dc")["probes"]["I(RS)"].as_f64()?.abs())
        }
        Item::Noise1k => {
            let a = go("add V 0 as=VI
net IN VI.+ U1.IN+
net REF VI.-
net OUT U1.OUT U1.IN-
noise OUT 100 10k ppd=10 input=VI
".into());
            answer(&a, "noise")["per_decade"].as_array()?.iter().find(|p| (p["hz"].as_f64().unwrap() - 1000.0).abs() < 1.0)?["in_per_rthz"].as_f64()
        }
    }
}

fn item_name(i: &Item) -> &'static str {
    match i {
        Item::Aol { .. } => "開ループ利得",
        Item::Gbw => "GBW(単位利得の周波数)",
        Item::PhaseMargin => "位相余裕",
        Item::Slew { .. } => "スルーレート",
        Item::Offset => "入力オフセット電圧",
        Item::Bias => "入力バイアス電流",
        Item::Supply => "電源電流(1 回路)",
        Item::SwingHigh { .. } => "出力の振れ幅(正の電源から)",
        Item::SwingLow { .. } => "出力の振れ幅(負の電源から)",
        Item::Short => "出力の短絡電流",
        Item::Noise1k => "入力換算の雑音(1 kHz)",
    }
}

/// A value with its unit, short.
fn show(i: &Item, v: f64) -> String {
    let eng = |v: f64, unit: &str| {
        let a = v.abs();
        let (k, p) = if a == 0.0 {
            (1.0, "")
        } else if a >= 1e6 {
            (1e-6, "M")
        } else if a >= 1e3 {
            (1e-3, "k")
        } else if a >= 1.0 {
            (1.0, "")
        } else if a >= 1e-3 {
            (1e3, "m")
        } else if a >= 1e-6 {
            (1e6, "µ")
        } else if a >= 1e-9 {
            (1e9, "n")
        } else if a >= 1e-12 {
            (1e12, "p")
        } else {
            (1e15, "f")
        };
        format!("{:.3} {p}{unit}", v * k).replace(".000 ", " ")
    };
    match i {
        Item::Aol { .. } => format!("{v:.1} dB"),
        Item::Gbw => eng(v, "Hz"),
        Item::PhaseMargin => format!("{v:.0}°"),
        Item::Slew { .. } => format!("{v:.2} V/µs"),
        Item::Offset | Item::SwingHigh { .. } | Item::SwingLow { .. } => eng(v, "V"),
        Item::Bias | Item::Supply | Item::Short => eng(v, "A"),
        Item::Noise1k => eng(v, "V/√Hz"),
    }
}

/// The report: one table per part, as Markdown and as the rows shown on the screen.
fn report() -> (String, Vec<(String, String, Vec<part_lib::model_checks::CheckRow>)>) {
    let mut out = String::new();
    let mut tables = vec![];
    for p in PARTS {
        let mut rows = vec![];
        out += &format!("\n## {}\n\nデータシート: {}\n\n| 項目 | データシート (min / typ / max) | 条件・表 | モデル | 差 (typ と比べて) |\n|---|---|---|---|---|\n", p.name, p.source);
        for row in p.rows {
            let ds = [row.min, row.typ, row.max].iter().map(|x| x.map(|v| show(&row.item, v)).unwrap_or_else(|| "—".into())).collect::<Vec<_>>().join(" / ");
            let m = measure(p, row);
            let model = m.map(|v| show(&row.item, v)).unwrap_or_else(|| "測れない".into());
            let diff = match (m, row.typ.or(row.max)) {
                (Some(v), Some(t)) if matches!(row.item, Item::Aol { .. } | Item::PhaseMargin) => format!("{:+.1}", v - t),
                (Some(v), Some(t)) if t != 0.0 => {
                    let r = v / t;
                    if r < 0.01 {
                        // the model has no such effect: what is measured is only a side effect (the finite gain)
                        "モデルに無い(ほぼ 0)".to_string()
                    } else if r > 100.0 {
                        format!("{:.0e} 倍", r)
                    } else {
                        format!("{:+.0}%", (r - 1.0) * 100.0).replace("-0%", "0%").replace("+0%", "0%")
                    }
                }
                _ => "—".into(),
            };
            let diff_md = if row.typ.is_none() && row.max.is_some() { format!("{diff} (max と)") } else { diff.clone() };
            out += &format!("| {} | {ds} | {} | {model} | {diff_md} |\n", item_name(&row.item), row.at);
            let target = match (row.typ, row.max) {
                (Some(t), _) => show(&row.item, t),
                (None, Some(m)) => format!("max {}", show(&row.item, m)),
                _ => "—".into(),
            };
            rows.push(part_lib::model_checks::CheckRow { item: item_name(&row.item).into(), datasheet: ds, target, condition: row.at.into(), model, diff, tested: tolerance(row).is_some() });
        }
        tables.push((p.name.to_string(), p.source.to_string(), rows));
    }
    (out, tables)
}

/// How far the model may be from the data sheet (docs/model-policy.md, section 5): None = not checked.
fn tolerance(row: &Row) -> Option<Check> {
    match row.item {
        Item::Aol { .. } => row.typ.map(|t| Check::Abs(t, 1.0)),
        Item::PhaseMargin => row.typ.map(|t| Check::Abs(t, 5.0)),
        Item::Gbw => row.typ.map(|t| Check::Rel(t, 0.10)),
        Item::Slew { .. } | Item::Noise1k => row.typ.map(|t| Check::Rel(t, 0.20)),
        // below 10 pA the simulator's own node conductances (1 pS) are of the same size
        Item::Bias => row.typ.filter(|t| *t >= 10e-12).map(|t| Check::Rel(t, 0.10)),
        Item::Offset | Item::Supply | Item::Short => row.typ.map(|t| Check::Rel(t, 0.10)),
        // a swing is good when it is no worse than the data sheet's typ (or max when only that is given)
        Item::SwingHigh { .. } | Item::SwingLow { .. } => row.typ.or(row.max).map(Check::AtMost),
    }
}

enum Check {
    /// Within ± the second number (dB, degrees).
    Abs(f64, f64),
    /// Within ± the share.
    Rel(f64, f64),
    /// No more than (5 % allowed for rounding).
    AtMost(f64),
}

#[test]
fn opamp_models_match_their_data_sheets() {
    let mut bad = vec![];
    for p in PARTS {
        for row in p.rows {
            let Some(v) = measure(p, row).filter(|v| v.is_finite()) else {
                bad.push(format!("{} {}: no measurement", p.name, item_name(&row.item)));
                continue;
            };
            let ok = match tolerance(row) {
                None => true,
                Some(Check::Abs(t, d)) => (v - t).abs() <= d,
                Some(Check::Rel(t, r)) => (v / t - 1.0).abs() <= r,
                Some(Check::AtMost(t)) => v <= t * 1.05,
            };
            if !ok {
                bad.push(format!(
                    "{} {}: model {}, data sheet {:?} / {:?} / {:?} ({})",
                    p.name,
                    item_name(&row.item),
                    show(&row.item, v),
                    row.min,
                    row.typ,
                    row.max,
                    row.at
                ));
            }
        }
    }
    assert!(bad.is_empty(), "op-amp models left their data sheets (docs/models/opamps.md):\n{}", bad.join("\n"));
}

#[test]
#[ignore]
fn write_opamp_model_report() {
    let (body, tables) = report();
    checks::write(tables, "2026-11-11");
    let text = format!(
        "# 同梱のオペアンプのモデルとデータシートの比べ\n\n\
         `crates/cli/tests/opamp_models.rs` が作る表(`cargo test -p cli --test opamp_models -- --ignored write_opamp_model_report`)。\
         方針と表の読み方は `docs/model-policy.md` の 5 節。\n\n\
         - データシートの値は、各表の上に書いたデータシートから写した(2026-11-11)。モデルの値は、データシートの条件に近い回路で circuitCad のシミュレータが測ったもの。\n\
         - 「差」は typ と比べた差(typ が無い行は max と)。開ループ利得と位相余裕は dB・度の差。\n\
         - モデルは 1 回路だけ。電源電流は 1 回路あたりに直したデータシートの値と比べる。\n\
         - モデルはデータシートの typ に合わせて作った(2026-11-11)。typ が無い項目(MCP600x のオフセット)は 0。\n\
         - 合格の幅(`opamp_models_match_their_data_sheets`): 開ループ利得 ±1 dB、位相余裕 ±5°、GBW・オフセット・バイアス電流・電源電流・短絡電流 ±10%、スルーレートと雑音 ±20%、出力の振れ幅は typ(無ければ max)以下。\n\
         - pA 以下の電流は、シミュレータが各ノードに置く小さなコンダクタンス(1 pS)の分が混ざるので、正確でない(MCP600x のバイアス電流。10 pA 未満は判定しない)。\n\
         - 入力オフセット電圧の行のモデルの値には、有限の利得による小さな差(出力 ÷ 開ループ利得)も入っている。\n{body}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/models/opamps.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    println!("{body}");
}
