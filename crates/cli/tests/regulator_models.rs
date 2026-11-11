//! The bundled ADR4525 (voltage reference) and LP5907-4.5 (LDO) models measured the way their data sheets measure them, next to
//! the data sheets' numbers (docs/model-policy.md, section 5). Runs the real `ccad` binary on small test circuits, like
//! `opamp_models.rs`.
//!
//! `regulator_models_match_their_data_sheets` fails when a model leaves its data sheet by more than the row's tolerance.
//! `cargo test -p cli --test regulator_models -- --ignored write_regulator_model_report` writes `docs/models/regulators.md`
//! and the tables shown next to the parts (`crates/part-lib/model_checks.json`).
//! The models are written by `tools/gen_reference_models.py`.

#[path = "common/checks.rs"]
mod checks;

use serde_json::Value;
use std::process::{Command, Stdio};

fn run(script: &str) -> Vec<Value> {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let lib = std::env::temp_dir().join(format!("ccad-reg-lib-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ccad")).args(["--lib", lib.to_str().unwrap(), "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&lib);
    String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// The answers of every `cmd` command, in order; each must have worked.
fn answers<'a>(a: &'a [Value], cmd: &str) -> Vec<&'a Value> {
    let v: Vec<&Value> = a.iter().filter(|v| v["cmd"] == cmd).collect();
    for x in &v {
        assert_eq!(x["ok"], true, "{cmd} failed: {x}");
    }
    assert!(!v.is_empty(), "no answer for {cmd}: {a:?}");
    v
}

fn probe(v: &Value, name: &str) -> f64 {
    v["probes"][name].as_f64().unwrap_or_else(|| panic!("no probe {name}: {v}"))
}

/// The ADR4525 on its input source VI, with a load current source IL (+ on the output: a positive value sinks into the part,
/// a negative one is drawn from it).
fn adr(vin: f64, il: f64) -> String {
    format!(
        "new\nadd ADR4525 as=U1\nadd V {vin} as=VI\nadd I {il} as=IL\nnet VIN VI.+ U1.VIN\nnet GND VI.- U1.GND IL.-\nnet OUT U1.VOUT IL.+\n\
         add C 1u as=CL\nnet OUT CL.1\nnet GND CL.2\n"
    )
}

/// The LP5907-4.5 on its input source VI (EN tied to the input unless `en` says otherwise), loaded by `rl` ohms, 1 uF on the output.
fn ldo(vin: &str, rl: f64, en: Option<&str>) -> String {
    let en = match en {
        Some(v) => format!("add V {v} as=VE\nnet EN VE.+ U1.EN\nnet GND VE.-\n"),
        None => "net VIN U1.EN\n".into(),
    };
    format!(
        "new\nadd LP5907-4.5 as=U1\nadd V 1 as=VI\nvalue VI {vin}\nadd R {rl} as=RL\nadd C 1u as=CO\n\
         net VIN VI.+ U1.IN\nnet GND VI.- U1.GND RL.2 CO.2\nnet OUT U1.OUT RL.1 CO.1\n{en}"
    )
}

struct Row {
    part: &'static str,
    item: &'static str,
    /// Data sheet min / typ / max, in `unit`.
    ds: [Option<f64>; 3],
    unit: &'static str,
    /// Where in the data sheet, and its conditions.
    at: &'static str,
    /// The model's value in `unit`.
    measure: Box<dyn Fn() -> f64>,
    /// Allowed distance from the typ (or the max when only that is given) in `unit`; None = not checked.
    tol: Option<f64>,
}

const ADR_DS: &str = "ADI ADR4520/4525/4530/4533/4540/4550 データシート Rev. 0、表 4 (ADR4525)。VIN = 3〜15 V、IL = 0 mA、25 °C";
const LDO_DS: &str = "TI LP5907 データシート SNVS798Q (2025-07 改訂)、5.5 節。VIN = 5.5 V、VEN = 1.2 V、IOUT = 1 mA、CIN = COUT = 1 µF";

fn rows() -> Vec<Row> {
    let mut v = adr_ldo_rows();
    v.extend(linear_rows());
    v.extend(tl431_rows());
    v
}

fn adr_ldo_rows() -> Vec<Row> {
    vec![
        Row {
            part: "ADR4525",
            item: "出力電圧",
            ds: [Some(2.4995), Some(2.5), Some(2.5005)],
            unit: "V",
            at: "VOUT。初期誤差 B グレード ±0.02% (500 µV)",
            measure: Box::new(|| probe(answers(&run(&format!("{}dc probe=V(out)\n", adr(5.0, 0.0))), "dc")[0], "V(out)")),
            tol: Some(1e-6),
        },
        Row {
            part: "ADR4525",
            item: "ライン・レギュレーション",
            ds: [None, Some(1.0), Some(10.0)],
            unit: "ppm/V",
            at: "ΔVOUT/ΔVIN (3 V と 15 V で比べた)",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=V(out)\nvalue VI 15\ndc probe=V(out)\n", adr(3.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 12.0 / 2.5 * 1e6
            }),
            tol: Some(0.2),
        },
        Row {
            part: "ADR4525",
            item: "ロード・レギュレーション (吐き出し)",
            ds: [None, Some(30.0), Some(80.0)],
            unit: "ppm/mA",
            at: "ΔVOUT/ΔIL、0 → 10 mA",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=V(out)\nvalue IL -10m\ndc probe=V(out)\n", adr(5.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[0], "V(out)") - probe(d[1], "V(out)")) / 10.0 / 2.5 * 1e6
            }),
            tol: Some(3.0),
        },
        Row {
            part: "ADR4525",
            item: "ロード・レギュレーション (吸い込み)",
            ds: [None, Some(60.0), Some(120.0)],
            unit: "ppm/mA",
            at: "ΔVOUT/ΔIL、0 → −10 mA",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=V(out)\nvalue IL 10m\ndc probe=V(out)\n", adr(5.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 10.0 / 2.5 * 1e6
            }),
            tol: Some(6.0),
        },
        Row {
            part: "ADR4525",
            item: "電源電流",
            ds: [None, Some(700.0), Some(950.0)],
            unit: "µA",
            at: "IQ、無負荷",
            measure: Box::new(|| -probe(answers(&run(&format!("{}dc probe=I(VI)\n", adr(5.0, 0.0))), "dc")[0], "I(VI)") * 1e6),
            tol: Some(70.0),
        },
        Row {
            part: "ADR4525",
            item: "ドロップアウト (入力 − 出力、入力 2.9 V)",
            ds: [None, None, Some(500.0)],
            unit: "mV",
            at: "VDO、無負荷 (データシートは max だけ。モデルは max の値)",
            measure: Box::new(|| (2.9 - probe(answers(&run(&format!("{}dc probe=V(out)\n", adr(2.9, 0.0))), "dc")[0], "V(out)")) * 1e3),
            tol: Some(25.0),
        },
        Row {
            part: "ADR4525",
            item: "リップル除去 (1 kHz)",
            ds: [None, Some(90.0), None],
            unit: "dB",
            at: "RRR、fIN = 1 kHz",
            measure: Box::new(|| {
                let a = run(&format!("{}value VI DC 5 AC 1\nac 1k 1k ppd=1 probe=V(out)\n", adr(5.0, 0.0)));
                -answers(&a, "ac")[0]["signals"]["V(OUT)"]["start"]["db"].as_f64().unwrap()
            }),
            tol: Some(3.0),
        },
        Row {
            part: "ADR4525",
            item: "出力雑音 (1 kHz)",
            ds: [None, Some(41.3), None],
            unit: "nV/√Hz",
            at: "eN、1 kHz",
            measure: Box::new(|| {
                let a = run(&format!("{}noise OUT 100 10k ppd=10\n", adr(5.0, 0.0)));
                let n = answers(&a, "noise")[0]["per_decade"].as_array().unwrap().iter().find(|p| (p["hz"].as_f64().unwrap() - 1e3).abs() < 1.0).unwrap()["out_v_per_rthz"].as_f64().unwrap();
                n * 1e9
            }),
            tol: Some(8.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "出力電圧",
            ds: [Some(4.41), Some(4.5), Some(4.59)],
            unit: "V",
            at: "VOUT(NOM)、±2%",
            measure: Box::new(|| probe(answers(&run(&format!("{}dc probe=V(out)\n", ldo("5.5", 4.5e3, None))), "dc")[0], "V(out)")),
            tol: Some(1e-3),
        },
        Row {
            part: "LP5907-4.5",
            item: "ライン・レギュレーション",
            ds: [None, Some(0.02), None],
            unit: "%/V",
            at: "VIN = 5.0 V と 5.5 V で比べた (4.5 V 品の表の範囲は 5.5〜5.5 V なので、少し下げた)",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=V(out)\nvalue VI 5.5\ndc probe=V(out)\n", ldo("5.0", 4.5e3, None)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 0.5 / 4.5 * 100.0
            }),
            tol: Some(0.002),
        },
        Row {
            part: "LP5907-4.5",
            item: "ロード・レギュレーション",
            ds: [None, Some(0.001), None],
            unit: "%/mA",
            at: "IOUT = 1 → 250 mA",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=V(out)\nvalue RL 18\ndc probe=V(out)\n", ldo("5.5", 4.5e3, None)));
                let d = answers(&a, "dc");
                let (v1, v2) = (probe(d[0], "V(out)"), probe(d[1], "V(out)"));
                (v1 - v2) / (v2 / 18.0 * 1e3 - v1 / 4.5e3 * 1e3) / 4.5 * 100.0
            }),
            tol: Some(0.0001),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (無負荷)",
            ds: [None, Some(12.0), Some(25.0)],
            unit: "µA",
            at: "IQ、VEN = 1.2 V、IOUT = 0 mA (EN に 1.2 V)",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=I(VI)\n", ldo("5.5", 1e12, Some("1.2"))));
                -probe(answers(&a, "dc")[0], "I(VI)") * 1e6
            }),
            tol: Some(1.2),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (250 mA)",
            ds: [None, Some(250.0), Some(425.0)],
            unit: "µA",
            at: "IQ、IOUT = 250 mA (入力の電流 − 負荷の電流)",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=I(VI),V(out)\n", ldo("5.5", 18.0, Some("1.2"))));
                let d = answers(&a, "dc")[0];
                (-probe(d, "I(VI)") - probe(d, "V(out)") / 18.0) * 1e6
            }),
            tol: Some(25.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (停止中)",
            ds: [None, Some(0.2), Some(1.0)],
            unit: "µA",
            at: "VEN = 0.3 V",
            measure: Box::new(|| {
                let a = run(&format!("{}dc probe=I(VI)\n", ldo("5.5", 1e12, Some("0.3"))));
                -probe(answers(&a, "dc")[0], "I(VI)") * 1e6
            }),
            tol: Some(0.05),
        },
        Row {
            part: "LP5907-4.5",
            item: "ドロップアウト (100 mA)",
            ds: [None, Some(50.0), None],
            unit: "mV",
            at: "VDO、IOUT = 100 mA (出力が 100 mV 下がったときの入力 − 出力。入力 4.45 V、負荷 44 Ω で測った)",
            measure: Box::new(|| (4.45 - probe(answers(&run(&format!("{}dc probe=V(out)\n", ldo("4.45", 44.0, None))), "dc")[0], "V(out)")) * 1e3),
            tol: Some(5.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "電流制限",
            ds: [Some(250.0), Some(500.0), None],
            unit: "mA",
            at: "ISC、出力を 100 mV 下げた所で (出力に 4.4 V の電源をつないで、そこへ流れ込む電流)",
            measure: Box::new(|| {
                let s = format!("{}add V 4.4 as=VF\nnet OUT VF.+\nnet GND VF.-\ndc probe=I(VF)\n", ldo("5.5", 1e12, None));
                probe(answers(&run(&s), "dc")[0], "I(VF)").abs() * 1e3
            }),
            tol: Some(25.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (1 kHz)",
            ds: [None, Some(82.0), None],
            unit: "dB",
            at: "f = 1 kHz、IOUT = 20 mA",
            measure: Box::new(|| psrr(1e3)),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (10 kHz)",
            ds: [None, Some(65.0), None],
            unit: "dB",
            at: "f = 10 kHz、IOUT = 20 mA",
            measure: Box::new(|| psrr(1e4)),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (100 kHz)",
            ds: [None, Some(60.0), None],
            unit: "dB",
            at: "f = 100 kHz、IOUT = 20 mA",
            measure: Box::new(|| psrr(1e5)),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "出力雑音",
            ds: [None, Some(10.0), None],
            unit: "µVrms",
            at: "eN、10 Hz〜100 kHz、IOUT = 1 mA",
            measure: Box::new(|| {
                let a = run(&format!("{}noise OUT 10 100k ppd=20\n", ldo("5.5", 4.5e3, None)));
                answers(&a, "noise")[0]["out_rms_v"].as_f64().unwrap() * 1e6
            }),
            tol: Some(1.5),
        },
        Row {
            part: "LP5907-4.5",
            item: "起動時間",
            ds: [None, Some(80.0), Some(150.0)],
            unit: "µs",
            at: "tON、VEN > VIH から VOUT = 95% まで (EN を 10 µs で 0 → 1.2 V)",
            measure: Box::new(|| {
                let s = format!("{}value VE PULSE(0 1.2 10u 1n 1n 1 2)\ntran 300u 0.2u probe=V(out) samples=200 record=probes uic\n", ldo("5.5", 4.5e3, Some("0")));
                let a = run(&s);
                let sm = answers(&a, "tran")[0]["signals"]["V(OUT)"]["samples"].as_array().unwrap().clone();
                let t95 = sm.iter().find(|p| p[1].as_f64().unwrap() >= 0.95 * 4.5).map(|p| p[0].as_f64().unwrap()).unwrap();
                (t95 - 10e-6) * 1e6
            }),
            tol: Some(10.0),
        },
    ]
}

/// The data sheet each part's rows come from.
fn source(part: &str) -> &'static str {
    match part {
        "ADR4525" => ADR_DS,
        "LP5907-4.5" => LDO_DS,
        "LM7805" | "LM7809" | "LM7812" => "onsemi MC7800 シリーズ データシート MC7800/D Rev. 29 の MC78xxC の表 (MC7805C・MC7809C・MC7812C)。25 °C",
        "L78L05" => "TI UA78L データシート SLVS010X (2023-06 改訂) の 6.7 節 (UA78L05、新しいチップ)。VI = 10 V、IO = 40 mA",
        "AMS1117-3.3" | "AMS1117-5.0" => "Advanced Monolithic Systems AMS1117 データシートの ELECTRICAL CHARACTERISTICS。25 °C",
        "MCP1700-3302" => "Microchip MCP1700 データシート DS20001826C の DC CHARACTERISTICS。VIN = VR + 1 V、ILOAD = 100 µA",
        "LM317" => "TI LM317 データシート SLVS044Z (2025-04 改訂) の 6.6 節。VIN − VOUT = 5 V、IOUT = 10 mA。R1 = 240 Ω (OUT−ADJ)、R2 = 720 Ω (ADJ−GND) で出力約 5 V",
        "TL431" => "TI TL431/TL432 データシート SLVS543S (2024-05 改訂) の 6.5 節 (TL431C)。25 °C",
        _ => "",
    }
}

/// A fixed regulator on its input source VI, loaded by a resistor RL that draws `iload` amps at the nominal output `vo`
/// (a resistor, not a current source: past the dropout or the current limit a current source would pull the output away).
fn fixed(part: &'static str, pins: (&str, &str, &str), vo: f64, vin: f64, iload: f64) -> String {
    let (i, g, o) = pins;
    let rl = if iload > 0.0 { vo / iload } else { 1e9 };
    format!("new\nadd {part} as=U1\nadd V {vin} as=VI\nadd R {rl} as=RL\nnet VIN VI.+ U1.{i}\nnet GND VI.- U1.{g} RL.2\nnet OUT U1.{o} RL.1\n")
}

/// The LM317 with R1 = 240 ohm (OUT-ADJ) and R2 = 720 ohm (ADJ-GND), output about 5 V, and a load resistor drawing `iload` more.
fn lm317(vin: f64, iload: f64) -> String {
    lm317_set(vin, iload, 720.0, 5.0)
}

/// The LM317 with R2 = `r2` (output about `vo`).
fn lm317_set(vin: f64, iload: f64, r2: f64, vo: f64) -> String {
    let rl = if iload > 0.0 { vo / iload } else { 1e9 };
    format!(
        "new\nadd LM317 as=U1\nadd V {vin} as=VI\nadd R {rl} as=RL\nadd R 240 as=R1\nadd R {r2} as=R2\n\
         net VIN VI.+ U1.IN\nnet OUT U1.OUT RL.1 R1.1\nnet ADJ U1.ADJ R1.2 R2.1\nnet GND VI.- RL.2 R2.2\n"
    )
}

fn vout(script: String) -> f64 {
    probe(answers(&run(&format!("{script}dc probe=V(out)\n")), "dc")[0], "V(out)")
}

/// Output current into the output held at `at` volts (a source VF).
fn into_held_output(script: String, at: f64) -> f64 {
    let s = format!("{script}add V {at} as=VF\nnet OUT VF.+\nnet GND VF.-\ndc probe=I(VF)\n");
    probe(answers(&run(&s), "dc")[0], "I(VF)").abs()
}

fn ripple_db(script: String, vin: f64, f: f64) -> f64 {
    let a = run(&format!("{script}value VI DC {vin} AC 1\nac {f} {f} ppd=1 probe=V(out)\n"));
    -answers(&a, "ac")[0]["signals"]["V(OUT)"]["start"]["db"].as_f64().unwrap()
}

fn noise_rms(script: String, fl: f64, fh: f64) -> f64 {
    let a = run(&format!("{script}noise OUT {fl} {fh} ppd=20\n"));
    answers(&a, "noise")[0]["out_rms_v"].as_f64().unwrap()
}

/// What a fixed regulator's data sheet gives, typ (the conditions are in the row texts).
struct Fixed {
    part: &'static str,
    pins: (&'static str, &'static str, &'static str),
    vout: f64,
    vin0: f64,
    i0: f64,
    /// line regulation: (input low, input high, load, typ mV, text)
    line: (f64, f64, f64, f64, &'static str),
    /// load regulation: (load low, load high, typ mV, text)
    load: (f64, f64, f64, &'static str),
    /// ground current typ (mA)
    iq_ma: f64,
    /// dropout: (load, typ V)
    dropout: (f64, f64),
    /// ripple rejection: (input, load, f, typ dB)
    ripple: (f64, f64, f64, f64),
    /// current limit: (input, output held at, typ A, text)
    limit: (f64, f64, f64, &'static str),
    /// output noise: (f low, f high, load, typ uV rms, text)
    noise: (f64, f64, f64, f64, &'static str),
    /// the output's min and max at 25 C (part to part)
    vrange: (f64, f64),
}

fn fixed_parts() -> Vec<Fixed> {
    vec![
        Fixed { part: "LM7805", pins: ("IN", "GND", "OUT"), vout: 5.0, vin0: 10.0, i0: 0.5, line: (7.5, 20.0, 1.0, 0.5, "7.5 V ≤ VIN ≤ 20 V、1.0 A"), load: (0.005, 1.0, 1.3, "5 mA ≤ IO ≤ 1.0 A"), iq_ma: 3.2, dropout: (1.0, 2.0), ripple: (13.0, 0.5, 120.0, 83.0), limit: (35.0, 0.0, 0.6, "短絡電流 (VIN = 35 V)"), noise: (10.0, 100e3, 0.5, 50.0, "10 µV/VO × 5 V、10 Hz〜100 kHz"), vrange: (4.8, 5.2) },
        Fixed { part: "LM7809", pins: ("IN", "GND", "OUT"), vout: 9.0, vin0: 15.0, i0: 0.5, line: (11.0, 26.0, 0.5, 6.2, "11 V ≤ VIN ≤ 26 V"), load: (0.005, 1.5, 1.5, "5 mA ≤ IO ≤ 1.5 A"), iq_ma: 3.4, dropout: (1.0, 2.0), ripple: (16.5, 0.5, 120.0, 61.0), limit: (35.0, 0.0, 0.2, "短絡電流 (VIN = 35 V)"), noise: (10.0, 100e3, 0.5, 90.0, "10 µV/VO × 9 V、10 Hz〜100 kHz"), vrange: (8.65, 9.35) },
        Fixed { part: "LM7812", pins: ("IN", "GND", "OUT"), vout: 12.0, vin0: 19.0, i0: 0.5, line: (14.5, 30.0, 0.5, 3.8, "14.5 V ≤ VIN ≤ 30 V"), load: (0.005, 1.5, 8.1, "5 mA ≤ IO ≤ 1.5 A"), iq_ma: 3.4, dropout: (1.0, 2.0), ripple: (20.0, 0.5, 120.0, 60.0), limit: (35.0, 0.0, 0.2, "短絡電流 (VIN = 35 V)"), noise: (10.0, 100e3, 0.5, 120.0, "10 µV/VO × 12 V、10 Hz〜100 kHz"), vrange: (11.5, 12.5) },
        Fixed { part: "L78L05", pins: ("IN", "GND", "OUT"), vout: 5.0, vin0: 10.0, i0: 0.04, line: (7.0, 20.0, 0.04, 32.0, "VI = 7〜20 V"), load: (0.001, 0.04, 8.0, "IO = 1〜40 mA"), iq_ma: 3.53, dropout: (0.04, 1.7), ripple: (13.0, 0.04, 120.0, 55.0), limit: (10.0, 0.0, 0.13, "電流制限 (表に無い。図 6-19 の 25 °C を読んだ)"), noise: (10.0, 100e3, 0.04, 125.0, "10 Hz〜100 kHz"), vrange: (4.95, 5.15) },
        Fixed { part: "AMS1117-3.3", pins: ("IN", "GND", "OUT"), vout: 3.3, vin0: 4.8, i0: 0.0, line: (4.8, 15.3, 0.0, 0.5, "1.5 V ≤ VIN − VOUT ≤ 12 V"), load: (0.0, 0.8, 3.0, "VIN = 4.75 V、0 ≤ IOUT ≤ 0.8 A"), iq_ma: 5.0, dropout: (0.8, 1.1), ripple: (6.3, 1.0, 120.0, 72.0), limit: (4.8, 3.25, 1.1, "電流制限 (VIN − VOUT = 1.5 V)"), noise: (10.0, 10e3, 0.0, 99.0, "0.003 % × 3.3 V、10 Hz〜10 kHz"), vrange: (3.251, 3.349) },
        Fixed { part: "AMS1117-5.0", pins: ("IN", "GND", "OUT"), vout: 5.0, vin0: 6.5, i0: 0.0, line: (6.5, 17.0, 0.0, 0.5, "1.5 V ≤ VIN − VOUT ≤ 12 V"), load: (0.0, 0.8, 5.0, "VIN = 6.5 V、0 ≤ IOUT ≤ 0.8 A"), iq_ma: 5.0, dropout: (0.8, 1.1), ripple: (6.5, 1.0, 120.0, 68.0), limit: (6.5, 4.95, 1.1, "電流制限 (VIN − VOUT = 1.5 V)"), noise: (10.0, 10e3, 0.0, 150.0, "0.003 % × 5 V、10 Hz〜10 kHz"), vrange: (4.925, 5.075) },
        Fixed { part: "MCP1700-3302", pins: ("VIN", "GND", "VOUT"), vout: 3.3, vin0: 4.3, i0: 100e-6, line: (4.3, 6.0, 100e-6, 0.0075 * 3.3 * 1.7 * 1e3, "(VR + 1) V ≤ VIN ≤ 6 V。表は ±0.75 %/V (mV に直した)"), load: (100e-6, 0.25, 33.0, "IL = 0.1〜250 mA、±1.0 % (mV に直した)"), iq_ma: 1.6e-3, dropout: (0.25, 0.178), ripple: (4.3, 0.05, 100.0, 44.0), limit: (4.3, 0.0, 0.408, "短絡電流 (VIN = VR + 1 V)"), noise: (999.0, 1001.0, 0.1, 3.0 * std::f64::consts::SQRT_2, "1 kHz の密度 3 µV/√Hz (IL = 100 mA) を 999〜1001 Hz の rms に直した"), vrange: (3.3 * 0.98, 3.3 * 1.02) },
    ]
}

fn linear_rows() -> Vec<Row> {
    let mut rows = vec![];
    for f in fixed_parts() {
        let Fixed { part, pins, vout: vo, vin0, i0, line, load, iq_ma, dropout, ripple, limit, noise, vrange } = f;
        rows.push(Row { part, item: "出力電圧", ds: [Some(vrange.0), Some(vo), Some(vrange.1)], unit: "V", at: "VO (25 °C)", measure: Box::new(move || vout(fixed(part, pins, vo, vin0, i0))), tol: Some(0.005 * vo) });
        rows.push(Row {
            part,
            item: "ライン・レギュレーション",
            ds: [None, Some(line.3), None],
            unit: "mV",
            at: line.4,
            measure: Box::new(move || (vout(fixed(part, pins, vo, line.1, line.2)) - vout(fixed(part, pins, vo, line.0, line.2))).abs() * 1e3),
            tol: Some(0.2 * line.3),
        });
        rows.push(Row {
            part,
            item: "ロード・レギュレーション",
            ds: [None, Some(load.2), None],
            unit: "mV",
            at: load.3,
            measure: Box::new(move || (vout(fixed(part, pins, vo, vin0, load.0)) - vout(fixed(part, pins, vo, vin0, load.1))) * 1e3),
            tol: Some(0.1 * load.2),
        });
        rows.push(Row {
            part,
            item: "グラウンド電流",
            ds: [None, Some(iq_ma), None],
            unit: "mA",
            at: "IB・IQ (入力の電流 − 負荷の電流)",
            measure: Box::new(move || {
                let a = run(&format!("{}dc probe=I(VI)\n", fixed(part, pins, vo, vin0, i0)));
                (-probe(answers(&a, "dc")[0], "I(VI)") - i0) * 1e3
            }),
            tol: Some(0.05 * iq_ma),
        });
        rows.push(Row {
            part,
            item: "ドロップアウト",
            ds: [None, Some(dropout.1), None],
            unit: "V",
            at: "VI − VO (入力を、公称の出力 + ドロップアウト − 0.3 V に下げた所)",
            measure: Box::new(move || {
                // the load resistor draws the data sheet's current at the output the dropout leaves
                let vin = vo + dropout.1 - 0.3;
                vin - vout(fixed(part, pins, vin - dropout.1, vin, dropout.0))
            }),
            tol: Some(0.05 * dropout.1),
        });
        rows.push(Row {
            part,
            item: "リップル除去",
            ds: [None, Some(ripple.3), None],
            unit: "dB",
            at: "RR (表の条件の周波数。120 Hz か 100 Hz)",
            measure: Box::new(move || ripple_db(fixed(part, pins, vo, ripple.0, ripple.1), ripple.0, ripple.2)),
            tol: Some(3.0),
        });
        rows.push(Row { part, item: "電流制限", ds: [None, Some(limit.2), None], unit: "A", at: limit.3, measure: Box::new(move || into_held_output(fixed(part, pins, vo, limit.0, 0.0), limit.1)), tol: Some(0.1 * limit.2) });
        rows.push(Row {
            part,
            item: "出力雑音",
            ds: [None, Some(noise.3), None],
            unit: "µVrms",
            at: noise.4,
            measure: Box::new(move || noise_rms(fixed(part, pins, vo, vin0, noise.2), noise.0, noise.1) * 1e6),
            tol: Some(0.2 * noise.3),
        });
    }
    // the adjustable one
    let p = "LM317";
    rows.push(Row {
        part: p,
        item: "基準電圧 (OUT − ADJ)",
        ds: [Some(1.2), Some(1.25), Some(1.3)],
        unit: "V",
        at: "VREF、IOUT = 10 mA",
        measure: Box::new(|| {
            let a = run(&format!("{}dc probe=V(out),V(adj)\n", lm317(10.0, 0.005)));
            let d = answers(&a, "dc")[0];
            probe(d, "V(out)") - probe(d, "V(adj)")
        }),
        tol: Some(0.002),
    });
    rows.push(Row {
        part: p,
        item: "ADJ ピンの電流",
        ds: [None, Some(50.0), Some(100.0)],
        unit: "µA",
        at: "IADJ",
        measure: Box::new(|| {
            let a = run(&format!("{}dc probe=V(out),V(adj)\n", lm317(10.0, 0.005)));
            let d = answers(&a, "dc")[0];
            // R2 carries ADJ / 720 to ground, R1 brings VREF / 240 into ADJ: the rest comes out of the pin
            (probe(d, "V(adj)") / 720.0 - (probe(d, "V(out)") - probe(d, "V(adj)")) / 240.0) * 1e6
        }),
        tol: Some(5.0),
    });
    rows.push(Row {
        part: p,
        item: "ライン・レギュレーション",
        ds: [None, Some(0.01), Some(0.04)],
        unit: "%/V",
        at: "3 V ≤ VIN − VOUT ≤ 40 V (入力 8 V と 45 V で比べた)",
        measure: Box::new(|| {
            let (a, b) = (vout(lm317(8.0, 0.005)), vout(lm317(45.0, 0.005)));
            (b - a) / a / 37.0 * 100.0
        }),
        tol: Some(0.002),
    });
    rows.push(Row {
        part: p,
        item: "ロード・レギュレーション",
        ds: [None, Some(0.1), Some(0.5)],
        unit: "%VO",
        at: "10 mA ≤ IOUT ≤ 1.5 A (新しいチップ)",
        measure: Box::new(|| {
            let (a, b) = (vout(lm317(10.0, 0.0)), vout(lm317(10.0, 1.5)));
            (a - b) / a * 100.0
        }),
        tol: Some(0.01),
    });
    rows.push(Row { part: p, item: "ドロップアウト (1 A)", ds: [None, Some(2.0), None], unit: "V", at: "表に無い。図 6-18 (25 °C、IOUT = 1 A) を読んだ", measure: Box::new(|| 6.5 - vout(lm317(6.5, 1.0))), tol: Some(0.1) });
    rows.push(Row { part: p, item: "電流制限 (差 15 V 以下)", ds: [Some(1.5), Some(2.2), None], unit: "A", at: "(VIN − VOUT) ≤ 15 V (出力を 4.9 V に押さえ、入力 15 V)", measure: Box::new(|| into_held_output(lm317(15.0, 0.0), 4.9)), tol: Some(0.1) });
    rows.push(Row { part: p, item: "電流制限 (差 40 V)", ds: [Some(0.15), Some(0.4), None], unit: "A", at: "(VIN − VOUT) = 40 V (出力を 4.9 V に押さえ、入力 44.9 V)", measure: Box::new(|| into_held_output(lm317(44.9, 0.0), 4.9)), tol: Some(0.04) });
    rows.push(Row { part: p, item: "リップル除去", ds: [None, Some(65.0), None], unit: "dB", at: "f = 120 Hz、VOUT = 10 V (R2 = 1680 Ω、入力 15 V)、CADJ = 0 (新しいチップ)", measure: Box::new(|| ripple_db(lm317_set(15.0, 0.0, 1680.0, 10.0), 15.0, 120.0)), tol: Some(3.0) });
    rows.push(Row { part: p, item: "出力雑音", ds: [None, Some(150.0), None], unit: "µVrms", at: "0.003 % × 5 V、10 Hz〜10 kHz", measure: Box::new(|| noise_rms(lm317(10.0, 0.0), 10.0, 10e3) * 1e6), tol: Some(30.0) });
    rows
}

/// The TL431 with its cathode held at `vka` (source VK) and REF driven by VR = `vref`.
fn tl431(vka: f64, vref: f64) -> String {
    format!("new\nadd TL431 as=U1\nadd V {vka} as=VK\nadd V {vref} as=VR\nnet K VK.+ U1.CATHODE\nnet R VR.+ U1.REF\nnet GND VK.- VR.- U1.ANODE\n")
}

/// The REF voltage that makes the cathode carry `ika` at `vka` (from two points on the straight part above the knee).
fn tl431_vref_at(vka: f64, ika: f64) -> f64 {
    let i = |vr: f64| -probe(answers(&run(&format!("{}dc probe=I(VK)\n", tl431(vka, vr))), "dc")[0], "I(VK)");
    // both above the knee (the knee is 2 mV below Vref at VKA = Vref, lower at a higher VKA)
    let (a, b) = (2.4945, 2.4960);
    let (ia, ib) = (i(a), i(b));
    a + (ika - ia) * (b - a) / (ib - ia)
}

fn tl431_rows() -> Vec<Row> {
    let p = "TL431";
    vec![
        Row { part: p, item: "基準電圧", ds: [Some(2.44), Some(2.495), Some(2.55)], unit: "V", at: "VKA = Vref、IKA = 10 mA", measure: Box::new(|| tl431_vref_at(2.495, 10e-3)), tol: Some(0.001) },
        Row {
            part: p,
            item: "REF の入力電流",
            ds: [None, Some(2.0), Some(4.0)],
            unit: "µA",
            at: "IKA = 10 mA",
            measure: Box::new(|| -probe(answers(&run(&format!("{}dc probe=I(VR)\n", tl431(10.0, 2.495))), "dc")[0], "I(VR)") * 1e6),
            tol: Some(0.2),
        },
        Row {
            part: p,
            item: "ΔVref / ΔVKA",
            ds: [None, Some(-1.4), Some(-2.7)],
            unit: "mV/V",
            at: "IKA = 10 mA、ΔVKA = 10 V − Vref",
            measure: Box::new(|| (tl431_vref_at(10.0, 10e-3) - tl431_vref_at(2.495, 10e-3)) / (10.0 - 2.495) * 1e3),
            tol: Some(0.15),
        },
        Row {
            part: p,
            item: "動的インピーダンス",
            ds: [None, Some(0.2), Some(0.5)],
            unit: "Ω",
            at: "VKA = Vref、IKA = 1〜100 mA (REF をカソードにつないで、電流を流し込んだ)",
            measure: Box::new(|| {
                let v = |i: f64| {
                    let s = format!("new\nadd TL431 as=U1\nadd I {i} as=IK\nnet K IK.+ U1.CATHODE U1.REF\nnet GND IK.- U1.ANODE\ndc probe=V(k)\n");
                    probe(answers(&run(&s), "dc")[0], "V(k)")
                };
                (v(0.1) - v(0.001)) / 0.099
            }),
            tol: Some(0.03),
        },
        Row {
            part: p,
            item: "停止中のカソード電流",
            ds: [None, Some(0.1), Some(1.0)],
            unit: "µA",
            at: "VKA = 36 V、Vref = 0",
            measure: Box::new(|| -probe(answers(&run(&format!("{}dc probe=I(VK)\n", tl431(36.0, 0.0))), "dc")[0], "I(VK)") * 1e6),
            tol: Some(0.02),
        },
    ]
}

/// Ripple rejection of the LDO at `f`, 20 mA load (225 ohm).
fn psrr(f: f64) -> f64 {
    let a = run(&format!("{}value VI DC 5.5 AC 1\nac {f} {f} ppd=1 probe=V(out)\n", ldo("5.5", 225.0, None)));
    -answers(&a, "ac")[0]["signals"]["V(OUT)"]["start"]["db"].as_f64().unwrap()
}

fn show(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.4}")
    }
}

/// The report as Markdown, and the rows shown on the screen (one table per part).
fn report() -> (String, Vec<(String, String, Vec<part_lib::model_checks::CheckRow>)>) {
    let mut out = String::new();
    let mut part = "";
    let mut tables: Vec<(String, String, Vec<part_lib::model_checks::CheckRow>)> = vec![];
    for r in rows() {
        if r.part != part {
            part = r.part;
            tables.push((part.to_string(), source(part).to_string(), vec![]));
            let ds = source(part);
            out += &format!("\n## {part}\n\nデータシート: {ds}\n\n| 項目 | データシート (min / typ / max) | 条件・表 | モデル | 差 |\n|---|---|---|---|---|\n");
        }
        let ds = r.ds.iter().map(|x| x.map(show).unwrap_or_else(|| "—".into())).collect::<Vec<_>>().join(" / ");
        let v = (r.measure)();
        let target = r.ds[1].or(r.ds[2]);
        let diff = target.map(|t| if v >= t { format!("+{}", show(v - t)) } else { show(v - t) }).unwrap_or_else(|| "—".into());
        out += &format!("| {} | {ds} {u} | {} | {} {u} | {diff} |\n", r.item, r.at, show(v), u = r.unit);
        let unit = |t: String| if t == "—" { t } else { format!("{t} {}", r.unit) };
        tables.last_mut().unwrap().2.push(part_lib::model_checks::CheckRow {
            item: r.item.into(),
            datasheet: format!("{ds} {}", r.unit),
            target: match (r.ds[1], r.ds[2]) {
                (Some(t), _) => format!("{} {}", show(t), r.unit),
                (None, Some(m)) => format!("max {} {}", show(m), r.unit),
                _ => "—".into(),
            },
            condition: r.at.into(),
            model: unit(show(v)),
            diff: unit(diff),
            tested: r.tol.is_some(),
        });
    }
    (out, tables)
}

#[test]
fn regulator_models_match_their_data_sheets() {
    let mut bad = vec![];
    for r in rows() {
        let v = (r.measure)();
        let target = r.ds[1].or(r.ds[2]);
        if let (Some(t), Some(tol)) = (target, r.tol)
            && ((v - t).abs() > tol || !v.is_finite())
        {
            bad.push(format!("{} {}: model {} {}, data sheet {:?} ({})", r.part, r.item, show(v), r.unit, r.ds, r.at));
        }
    }
    assert!(bad.is_empty(), "models left their data sheets (docs/models/regulators.md):\n{}", bad.join("\n"));
}

#[test]
#[ignore]
fn write_regulator_model_report() {
    let (body, tables) = report();
    checks::write(tables, "2026-11-11");
    let text = format!(
        "# 同梱の電圧基準と LDO のモデルとデータシートの比べ\n\n\
         `crates/cli/tests/regulator_models.rs` が作る表(`cargo test -p cli --test regulator_models -- --ignored write_regulator_model_report`)。\
         方針は `docs/model-policy.md`、モデルを作るのは `tools/gen_reference_models.py`。\n\n\
         - データシートの値は、各表の上に書いたデータシートから写した(2026-11-11)。モデルの値は、データシートの条件に近い回路で circuitCad のシミュレータが測ったもの。\n\
         - 「差」はモデル − typ(typ が無い行は max)。合格の幅はテストの各行に書いてある(おおむね typ の 10%、dB は ±3 dB)。\n\
         - どちらも出力の誤差(ADR4525 の初期誤差 ±0.02%、LP5907 の ±2%)は入れていない(出力はちょうど公称値)。\n{body}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/models/regulators.md");
    std::fs::write(&path, text).unwrap();
    println!("{body}");
}
