//! The bundled ADR4525 (voltage reference) and LP5907-4.5 (LDO) models measured the way their data sheets measure them, next to
//! the data sheets' numbers (docs/model-policy.md, section 5). Runs the real `ccad` binary on small test circuits, like
//! `opamp_models.rs`.
//!
//! `regulator_models_match_their_data_sheets` fails when a model leaves its data sheet by more than the row's tolerance.
//! `cargo test -p cli --test regulator_models -- --ignored write_regulator_model_report` writes `docs/models/regulators.md`.
//! The models are written by `tools/gen_reference_models.py`.

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
    measure: fn() -> f64,
    /// Allowed distance from the typ (or the max when only that is given) in `unit`; None = not checked.
    tol: Option<f64>,
}

const ADR_DS: &str = "ADI ADR4520/4525/4530/4533/4540/4550 データシート Rev. 0、表 4 (ADR4525)。VIN = 3〜15 V、IL = 0 mA、25 °C";
const LDO_DS: &str = "TI LP5907 データシート SNVS798Q (2025-07 改訂)、5.5 節。VIN = 5.5 V、VEN = 1.2 V、IOUT = 1 mA、CIN = COUT = 1 µF";

fn rows() -> Vec<Row> {
    vec![
        Row {
            part: "ADR4525",
            item: "出力電圧",
            ds: [None, Some(2.5), None],
            unit: "V",
            at: "VOUT。初期誤差 B グレード ±0.02% (500 µV)",
            measure: || probe(answers(&run(&format!("{}dc probe=V(out)\n", adr(5.0, 0.0))), "dc")[0], "V(out)"),
            tol: Some(1e-6),
        },
        Row {
            part: "ADR4525",
            item: "ライン・レギュレーション",
            ds: [None, Some(1.0), Some(10.0)],
            unit: "ppm/V",
            at: "ΔVOUT/ΔVIN (3 V と 15 V で比べた)",
            measure: || {
                let a = run(&format!("{}dc probe=V(out)\nvalue VI 15\ndc probe=V(out)\n", adr(3.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 12.0 / 2.5 * 1e6
            },
            tol: Some(0.2),
        },
        Row {
            part: "ADR4525",
            item: "ロード・レギュレーション (吐き出し)",
            ds: [None, Some(30.0), Some(80.0)],
            unit: "ppm/mA",
            at: "ΔVOUT/ΔIL、0 → 10 mA",
            measure: || {
                let a = run(&format!("{}dc probe=V(out)\nvalue IL -10m\ndc probe=V(out)\n", adr(5.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[0], "V(out)") - probe(d[1], "V(out)")) / 10.0 / 2.5 * 1e6
            },
            tol: Some(3.0),
        },
        Row {
            part: "ADR4525",
            item: "ロード・レギュレーション (吸い込み)",
            ds: [None, Some(60.0), Some(120.0)],
            unit: "ppm/mA",
            at: "ΔVOUT/ΔIL、0 → −10 mA",
            measure: || {
                let a = run(&format!("{}dc probe=V(out)\nvalue IL 10m\ndc probe=V(out)\n", adr(5.0, 0.0)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 10.0 / 2.5 * 1e6
            },
            tol: Some(6.0),
        },
        Row {
            part: "ADR4525",
            item: "電源電流",
            ds: [None, Some(700.0), Some(950.0)],
            unit: "µA",
            at: "IQ、無負荷",
            measure: || -probe(answers(&run(&format!("{}dc probe=I(VI)\n", adr(5.0, 0.0))), "dc")[0], "I(VI)") * 1e6,
            tol: Some(70.0),
        },
        Row {
            part: "ADR4525",
            item: "ドロップアウト (入力 − 出力、入力 2.9 V)",
            ds: [None, None, Some(500.0)],
            unit: "mV",
            at: "VDO、無負荷 (データシートは max だけ。モデルは max の値)",
            measure: || (2.9 - probe(answers(&run(&format!("{}dc probe=V(out)\n", adr(2.9, 0.0))), "dc")[0], "V(out)")) * 1e3,
            tol: Some(25.0),
        },
        Row {
            part: "ADR4525",
            item: "リップル除去 (1 kHz)",
            ds: [None, Some(90.0), None],
            unit: "dB",
            at: "RRR、fIN = 1 kHz",
            measure: || {
                let a = run(&format!("{}value VI DC 5 AC 1\nac 1k 1k ppd=1 probe=V(out)\n", adr(5.0, 0.0)));
                -answers(&a, "ac")[0]["signals"]["V(OUT)"]["start"]["db"].as_f64().unwrap()
            },
            tol: Some(3.0),
        },
        Row {
            part: "ADR4525",
            item: "出力雑音 (1 kHz)",
            ds: [None, Some(41.3), None],
            unit: "nV/√Hz",
            at: "eN、1 kHz",
            measure: || {
                let a = run(&format!("{}noise OUT 100 10k ppd=10\n", adr(5.0, 0.0)));
                let n = answers(&a, "noise")[0]["per_decade"].as_array().unwrap().iter().find(|p| (p["hz"].as_f64().unwrap() - 1e3).abs() < 1.0).unwrap()["out_v_per_rthz"].as_f64().unwrap();
                n * 1e9
            },
            tol: Some(8.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "出力電圧",
            ds: [Some(4.41), Some(4.5), Some(4.59)],
            unit: "V",
            at: "VOUT(NOM)、±2%",
            measure: || probe(answers(&run(&format!("{}dc probe=V(out)\n", ldo("5.5", 4.5e3, None))), "dc")[0], "V(out)"),
            tol: Some(1e-3),
        },
        Row {
            part: "LP5907-4.5",
            item: "ライン・レギュレーション",
            ds: [None, Some(0.02), None],
            unit: "%/V",
            at: "VIN = 5.0 V と 5.5 V で比べた (4.5 V 品の表の範囲は 5.5〜5.5 V なので、少し下げた)",
            measure: || {
                let a = run(&format!("{}dc probe=V(out)\nvalue VI 5.5\ndc probe=V(out)\n", ldo("5.0", 4.5e3, None)));
                let d = answers(&a, "dc");
                (probe(d[1], "V(out)") - probe(d[0], "V(out)")) / 0.5 / 4.5 * 100.0
            },
            tol: Some(0.002),
        },
        Row {
            part: "LP5907-4.5",
            item: "ロード・レギュレーション",
            ds: [None, Some(0.001), None],
            unit: "%/mA",
            at: "IOUT = 1 → 250 mA",
            measure: || {
                let a = run(&format!("{}dc probe=V(out)\nvalue RL 18\ndc probe=V(out)\n", ldo("5.5", 4.5e3, None)));
                let d = answers(&a, "dc");
                let (v1, v2) = (probe(d[0], "V(out)"), probe(d[1], "V(out)"));
                (v1 - v2) / (v2 / 18.0 * 1e3 - v1 / 4.5e3 * 1e3) / 4.5 * 100.0
            },
            tol: Some(0.0001),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (無負荷)",
            ds: [None, Some(12.0), Some(25.0)],
            unit: "µA",
            at: "IQ、VEN = 1.2 V、IOUT = 0 mA (EN に 1.2 V)",
            measure: || {
                let a = run(&format!("{}dc probe=I(VI)\n", ldo("5.5", 1e12, Some("1.2"))));
                -probe(answers(&a, "dc")[0], "I(VI)") * 1e6
            },
            tol: Some(1.2),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (250 mA)",
            ds: [None, Some(250.0), Some(425.0)],
            unit: "µA",
            at: "IQ、IOUT = 250 mA (入力の電流 − 負荷の電流)",
            measure: || {
                let a = run(&format!("{}dc probe=I(VI),V(out)\n", ldo("5.5", 18.0, Some("1.2"))));
                let d = answers(&a, "dc")[0];
                (-probe(d, "I(VI)") - probe(d, "V(out)") / 18.0) * 1e6
            },
            tol: Some(25.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "電源電流 (停止中)",
            ds: [None, Some(0.2), Some(1.0)],
            unit: "µA",
            at: "VEN = 0.3 V",
            measure: || {
                let a = run(&format!("{}dc probe=I(VI)\n", ldo("5.5", 1e12, Some("0.3"))));
                -probe(answers(&a, "dc")[0], "I(VI)") * 1e6
            },
            tol: Some(0.05),
        },
        Row {
            part: "LP5907-4.5",
            item: "ドロップアウト (100 mA)",
            ds: [None, Some(50.0), None],
            unit: "mV",
            at: "VDO、IOUT = 100 mA (出力が 100 mV 下がったときの入力 − 出力。入力 4.45 V、負荷 44 Ω で測った)",
            measure: || (4.45 - probe(answers(&run(&format!("{}dc probe=V(out)\n", ldo("4.45", 44.0, None))), "dc")[0], "V(out)")) * 1e3,
            tol: Some(5.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "電流制限",
            ds: [Some(250.0), Some(500.0), None],
            unit: "mA",
            at: "ISC、出力を 100 mV 下げた所で (出力に 4.4 V の電源をつないで、そこへ流れ込む電流)",
            measure: || {
                let s = format!("{}add V 4.4 as=VF\nnet OUT VF.+\nnet GND VF.-\ndc probe=I(VF)\n", ldo("5.5", 1e12, None));
                probe(answers(&run(&s), "dc")[0], "I(VF)").abs() * 1e3
            },
            tol: Some(25.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (1 kHz)",
            ds: [None, Some(82.0), None],
            unit: "dB",
            at: "f = 1 kHz、IOUT = 20 mA",
            measure: || psrr(1e3),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (10 kHz)",
            ds: [None, Some(65.0), None],
            unit: "dB",
            at: "f = 10 kHz、IOUT = 20 mA",
            measure: || psrr(1e4),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "PSRR (100 kHz)",
            ds: [None, Some(60.0), None],
            unit: "dB",
            at: "f = 100 kHz、IOUT = 20 mA",
            measure: || psrr(1e5),
            tol: Some(3.0),
        },
        Row {
            part: "LP5907-4.5",
            item: "出力雑音",
            ds: [None, Some(10.0), None],
            unit: "µVrms",
            at: "eN、10 Hz〜100 kHz、IOUT = 1 mA",
            measure: || {
                let a = run(&format!("{}noise OUT 10 100k ppd=20\n", ldo("5.5", 4.5e3, None)));
                answers(&a, "noise")[0]["out_rms_v"].as_f64().unwrap() * 1e6
            },
            tol: Some(1.5),
        },
        Row {
            part: "LP5907-4.5",
            item: "起動時間",
            ds: [None, Some(80.0), Some(150.0)],
            unit: "µs",
            at: "tON、VEN > VIH から VOUT = 95% まで (EN を 10 µs で 0 → 1.2 V)",
            measure: || {
                let s = format!("{}value VE PULSE(0 1.2 10u 1n 1n 1 2)\ntran 300u 0.2u probe=V(out) samples=200 record=probes uic\n", ldo("5.5", 4.5e3, Some("0")));
                let a = run(&s);
                let sm = answers(&a, "tran")[0]["signals"]["V(OUT)"]["samples"].as_array().unwrap().clone();
                let t95 = sm.iter().find(|p| p[1].as_f64().unwrap() >= 0.95 * 4.5).map(|p| p[0].as_f64().unwrap()).unwrap();
                (t95 - 10e-6) * 1e6
            },
            tol: Some(10.0),
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

fn report() -> String {
    let mut out = String::new();
    let mut part = "";
    for r in rows() {
        if r.part != part {
            part = r.part;
            let ds = if part == "ADR4525" { ADR_DS } else { LDO_DS };
            out += &format!("\n## {part}\n\nデータシート: {ds}\n\n| 項目 | データシート (min / typ / max) | 条件・表 | モデル | 差 |\n|---|---|---|---|---|\n");
        }
        let ds = r.ds.iter().map(|x| x.map(show).unwrap_or_else(|| "—".into())).collect::<Vec<_>>().join(" / ");
        let v = (r.measure)();
        let target = r.ds[1].or(r.ds[2]);
        let diff = target.map(|t| if v >= t { format!("+{}", show(v - t)) } else { show(v - t) }).unwrap_or_else(|| "—".into());
        out += &format!("| {} | {ds} {u} | {} | {} {u} | {diff} |\n", r.item, r.at, show(v), u = r.unit);
    }
    out
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
    let body = report();
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
