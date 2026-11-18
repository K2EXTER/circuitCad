//! The bundled comparator models (LM393, LM339) measured the way their data sheets measure them, next to the data sheets'
//! numbers (docs/model-policy.md, section 5). Runs the commands of `ccad` on small test circuits (`common::ccad`).
//!
//! `comparator_models_match_their_data_sheets` fails when a model leaves its data sheet by more than the row's tolerance.
//! `cargo test -p cli --test integration -- --ignored write_comparator_model_report` writes `docs/models/comparators.md`
//! and the tables shown next to the parts (`crates/part-lib/model_checks.json`).
//! The models are written by `ccad fit` from the typ values in `crates/part-lib/fits/LM393_SOIC-8.fit.json` and
//! `LM339_SOIC-14.fit.json` (`part_lib::fit::comparator`).

use super::checks;

use serde_json::Value;
use super::common::{ccad, Flags};

fn run(script: &str) -> Vec<Value> {
    ccad(script, Flags::default()).0
}

fn answer<'a>(a: &'a [Value], cmd: &str) -> &'a Value {
    let v = a.iter().find(|v| v["cmd"] == cmd).unwrap_or_else(|| panic!("no answer for {cmd}: {a:?}"));
    assert_eq!(v["ok"], true, "{cmd} failed: {v}");
    v
}

/// The comparator on a 5 V supply, IN- held at `reference`.
fn powered(part: &str, reference: f64) -> String {
    format!("new\nadd {part} as=U1\nadd V 5 as=VCC\nadd V {reference} as=VR\nnet VDD VCC.+ U1.VCC\nnet REF VR.+ U1.IN-\nnet GND VCC.- VR.- U1.GND\n")
}

/// The input offset the model carries (typ): the response times are measured with the overdrive from the real threshold.
const VOS: f64 = 2e-3;

/// The response time (µs) from IN+ stepping `from` → `to` (IN- at `reference`) to the output crossing 1.4 V, pulled up to
/// 5 V through 5.1 kΩ with 15 pF (TI's test circuit).
fn response(part: &str, from: f64, to: f64, reference: f64) -> f64 {
    let falling = to < from;
    let a = run(&format!(
        "{}add V 0 as=VI\nvalue VI PULSE({from} {to} 1u 1n 1n 20u 40u)\nadd R 5.1k as=RP\nadd C 15p as=CL\nnet VDD RP.1\nnet IN VI.+ U1.IN+\nnet OUT U1.OUT RP.2 CL.1\nnet GND VI.- CL.2\ntran 6u 2n probe=V(out) samples=3000 record=probes\n",
        powered(part, reference)
    ));
    let s: Vec<(f64, f64)> = answer(&a, "tran")["signals"]["V(OUT)"]["samples"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    s.windows(2)
        .find(|w| w[0].0 >= 1e-6 && if falling { w[0].1 > 1.4 && w[1].1 <= 1.4 } else { w[0].1 < 1.4 && w[1].1 >= 1.4 })
        .map(|w| (w[0].0 + (1.4 - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0) - 1e-6) * 1e6)
        .unwrap_or_else(|| panic!("{part}: the output never crossed 1.4 V"))
}

/// The output's low voltage (V) with `i` amps pushed into it (IN+ below IN-).
fn vol(part: &str, i: f64) -> f64 {
    let a = run(&format!("{}add V 0.4 as=VI\nadd I {i} as=IO\nnet IN VI.+ U1.IN+\nnet OUT IO.+ U1.OUT\nnet GND VI.- IO.-\ndc probe=V(out)\n", powered(part, 0.5)));
    answer(&a, "dc")["probes"]["V(out)"].as_f64().unwrap()
}

/// The current (mA) the output sinks held at `v` volts (IN+ below IN-).
fn iol(part: &str, v: f64) -> f64 {
    let a = run(&format!("{}add V 0.4 as=VI\nadd V {v} as=VO\nnet IN VI.+ U1.IN+\nnet OUT VO.+ U1.OUT\nnet GND VI.- VO.-\ndc probe=I(VO)\n", powered(part, 0.5)));
    answer(&a, "dc")["probes"]["I(VO)"].as_f64().unwrap().abs() * 1e3
}

/// IN+ (minus IN- at 0.5 V) where the output, pulled up through 5.1 kΩ, crosses 2.5 V (mV): the input offset.
fn threshold(part: &str) -> f64 {
    let a = run(&format!("{}add V 0.5 as=VI\nadd R 5.1k as=RP\nnet VDD RP.1\nnet IN VI.+ U1.IN+\nnet OUT U1.OUT RP.2\nnet GND VI.-\nsweep VI 0.49 0.51 steps=401 probe=V(out)\n", powered(part, 0.5)));
    let rows: Vec<(f64, f64)> = answer(&a, "sweep")["rows"].as_array().unwrap().iter().map(|r| (r["value"].as_f64().unwrap(), r["V(out)"].as_f64().unwrap())).collect();
    let w = rows.windows(2).find(|w| w[0].1 < 2.5 && w[1].1 >= 2.5).expect("no crossing");
    (w[0].0 + (2.5 - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0) - 0.5) * 1e3
}

/// The current (nA) out of IN+ held at 0.5 V, and the supply current (µA) with the output open.
fn input_and_supply(part: &str) -> (f64, f64) {
    let a = run(&format!("{}add V 0.4 as=VI\nnet IN VI.+ U1.IN+\nnet GND VI.-\ndc probe=I(VI),I(VCC)\n", powered(part, 0.5)));
    let p = &answer(&a, "dc")["probes"];
    (p["I(VI)"].as_f64().unwrap().abs() * 1e9, p["I(VCC)"].as_f64().unwrap().abs() * 1e6)
}

struct Row {
    part: &'static str,
    item: String,
    /// The data sheet's min, typ, max
    ds: [Option<f64>; 3],
    unit: &'static str,
    at: String,
    measure: Box<dyn Fn() -> f64 + Sync>,
    /// Allowed distance from the typ (or the max when there is no typ); None: shown, not checked
    tol: Option<f64>,
}

fn source(part: &str) -> &'static str {
    match part {
        "LM393" => "TI LMx93x・LM2903x データシート (JAJSIA8AH、2025-04 改訂) の 5.8 節 (LM393) と 5.11 節。VCC = 5 V、25 °C",
        _ => "TI LM139/LM239/LM339/LM2901 データシート (SLCS006Z、2025-05 改訂) の 6.10 節 (LM339) と 6.14 節。VCC = 5 V、25 °C",
    }
}

fn rows() -> Vec<Row> {
    let mut v = vec![];
    // (part, VOL typ at 4 mA, IOL typ at 1.5 V, ICC typ / max of one comparator in µA)
    let parts: [(&'static str, f64, Option<f64>, (f64, f64)); 2] = [("LM393", 0.13, None, (225.0, 500.0)), ("LM339", 0.15, Some(16.0), (200.0, 500.0))];
    for (part, vol_typ, iol_typ, (iq, iq_max)) in parts {
        let r = 0.5;
        let th = r + VOS;
        let timing = "5.1 kΩ で 5 V に引き上げ、15 pF、出力が 1.4 V を越えるまで";
        for (item, from, to, rf, typ) in [
            ("応答時間 (出力 High → Low)", th + 0.095, th - 0.005, r, 1.3),
            ("応答時間 (出力 Low → High)", th - 0.095, th + 0.005, r, 1.3),
            ("応答時間 TTL (出力 High → Low)", 3.0, 0.0, 1.4, 0.3),
            ("応答時間 TTL (出力 Low → High)", 0.0, 3.0, 1.4, 0.3),
        ] {
            let what = if typ > 1.0 { "100 mV の入力のステップ、5 mV のオーバードライブ (切り替わり点から)" } else { "TTL の入力 (0 ↔ 3 V、基準 1.4 V)" };
            v.push(Row { part, item: item.into(), ds: [None, Some(typ), None], unit: "µs", at: format!("{what}。{timing}。表は向きを分けない"), measure: Box::new(move || response(part, from, to, rf)), tol: Some(0.2 * typ) });
        }
        v.push(Row { part, item: "Low の出力電圧 (4 mA)".into(), ds: [None, Some(vol_typ), Some(0.4)], unit: "V", at: "VOL: IOL = 4 mA (表)".into(), measure: Box::new(move || vol(part, 4e-3)), tol: Some(0.01) });
        v.push(Row { part, item: "Low の出力電流 (1.5 V)".into(), ds: [Some(6.0), iol_typ, None], unit: "mA", at: format!("IOL: VOL = 1.5 V (表{})", if iol_typ.is_none() { "は min だけ。モデルは LM339 の typ 16 mA" } else { "" }), measure: Box::new(move || iol(part, 1.5)), tol: iol_typ.map(|t| 0.1 * t) });
        v.push(Row { part, item: "入力オフセット".into(), ds: [None, Some(2.0), Some(5.0)], unit: "mV", at: "VIO: 出力が 2.5 V を越える IN+ − IN− (表)".into(), measure: Box::new(move || threshold(part)), tol: Some(0.2) });
        v.push(Row { part, item: "入力バイアス電流".into(), ds: [None, Some(25.0), Some(250.0)], unit: "nA", at: "IIB: ピンから出る向き (表)".into(), measure: Box::new(move || input_and_supply(part).0), tol: Some(1.0) });
        v.push(Row { part, item: "電源電流 (1 回路)".into(), ds: [None, Some(iq), Some(iq_max)], unit: "µA", at: "ICC: 無負荷、表の全部の回路の値を回路の数で割った".into(), measure: Box::new(move || input_and_supply(part).1), tol: Some(0.05 * iq) });
    }
    v
}

fn show(v: f64) -> String {
    if v.abs() >= 100.0 { format!("{v:.0}") } else if v.abs() >= 10.0 { format!("{v:.1}") } else { format!("{v:.3}") }
}

fn report() -> (String, Vec<(String, String, Vec<part_lib::model_checks::CheckRow>)>) {
    let mut out = String::new();
    let mut part = "";
    let mut tables: Vec<(String, String, Vec<part_lib::model_checks::CheckRow>)> = vec![];
    let rows = rows();
    let values = checks::par_map(&rows, |r| (r.measure)());
    for (r, v) in rows.iter().zip(values) {
        if r.part != part {
            part = r.part;
            tables.push((part.to_string(), source(part).to_string(), vec![]));
            out += &format!("\n## {part}\n\nデータシート: {}\n\n| 項目 | データシート (min / typ / max) | 条件・図や表 | モデル | 差 |\n|---|---|---|---|---|\n", source(part));
        }
        let ds = r.ds.iter().map(|x| x.map(show).unwrap_or_else(|| "—".into())).collect::<Vec<_>>().join(" / ");
        let target = r.ds[1].or(r.ds[2]);
        let diff = target.map(|t| if v >= t { format!("+{}", show(v - t)) } else { show(v - t) }).unwrap_or_else(|| "—".into());
        out += &format!("| {} | {ds} {u} | {} | {} {u} | {diff} |\n", r.item, r.at, show(v), u = r.unit);
        let unit = |t: String| if t == "—" { t } else { format!("{t} {}", r.unit) };
        tables.last_mut().unwrap().2.push(part_lib::model_checks::CheckRow {
            item: r.item.clone(),
            datasheet: format!("{ds} {}", r.unit),
            target: match (r.ds[1], r.ds[2]) {
                (Some(t), _) => format!("{} {}", show(t), r.unit),
                (None, Some(m)) => format!("max {} {}", show(m), r.unit),
                _ => "—".into(),
            },
            condition: r.at.clone(),
            model: unit(show(v)),
            diff: unit(diff),
            tested: r.tol.is_some(),
        });
    }
    (out, tables)
}

#[test]
fn comparator_models_match_their_data_sheets() {
    let rows = rows();
    let values = checks::par_map(&rows, |r| (r.measure)());
    let bad: Vec<String> = rows
        .iter()
        .zip(values)
        .filter_map(|(r, v)| {
            let tol = r.tol?;
            let target = r.ds[1].or(r.ds[2])?;
            ((v - target).abs() > tol).then(|| format!("{} {}: model {v} {}, data sheet {:?} ({})", r.part, r.item, r.unit, r.ds, r.at))
        })
        .collect();
    assert!(bad.is_empty(), "models left their data sheets (docs/models/comparators.md):\n{}", bad.join("\n"));
}

#[test]
#[ignore]
fn write_comparator_model_report() {
    let (body, tables) = report();
    checks::write(tables, "2026-11-18");
    let text = format!(
        "# 同梱のコンパレータのモデルとデータシートの比べ\n\n\
         `crates/cli/tests/integration/comparator_models.rs` が作る表(`cargo test -p cli --test integration -- --ignored write_comparator_model_report`)。\
         方針は `docs/model-policy.md`、モデルを作るのは `ccad fit`(`kind: comparator`、データシートの typ の値は `crates/part-lib/fits/` にある)。\n\n\
         - モデルの値は、データシートの条件に近い回路で circuitCad のシミュレータが測ったもの (VCC = 5 V)。\n\
         - 応答時間は、表は向きを分けないので、出力が下がる向きと上がる向きの両方を並べた。オーバードライブは、モデルのオフセット 2 mV を足した本当の切り替わり点から測る。\n\
         - 「差」はモデル − typ(typ が無い行は max)。合格の幅はテストの各行(応答時間は ±20 %)。\n{body}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/models/comparators.md");
    std::fs::write(&path, text).unwrap();
    println!("{body}");
}
