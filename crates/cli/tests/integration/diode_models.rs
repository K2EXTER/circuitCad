//! The bundled diode models (1N4148, 1N4001, 1N4007, 1N5817, 1N5819, SS14, 1N5408, 1N5822, UF4007, BAT54) measured the way their data sheets measure them, next to the data sheets' numbers
//! (docs/model-policy.md, section 5). Runs the commands of `ccad` on small test circuits (`common::ccad`), like `regulator_models.rs`.
//!
//! `diode_models_match_their_data_sheets` fails when a model leaves its data sheet by more than the row's tolerance.
//! `cargo test -p cli --test integration -- --ignored write_diode_model_report` writes `docs/models/diodes.md`
//! and the tables shown next to the parts (`crates/part-lib/model_checks.json`).
//! The models are written by `ccad fit` from the readings of the data sheets in `crates/part-lib/fits/*.fit.json`.

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

/// Forward voltage at `i` amps, driven by a current source.
fn vf(part: &str, i: f64) -> f64 {
    let a = run(&format!("new\nadd {part} as=D1\nadd I {i} as=IF\nnet A IF.+ D1.A\nnet GND IF.- D1.K\ndc probe=V(A)\n"));
    answer(&a, "dc")["probes"]["V(A)"].as_f64().unwrap()
}

/// Reverse current (A) at `vr` volts across the diode.
fn ir(part: &str, vr: f64) -> f64 {
    let a = run(&format!("new\nadd {part} as=D1\nadd V {vr} as=VR\nnet K VR.+ D1.K\nnet GND VR.- D1.A\ndc probe=I(VR)\n"));
    answer(&a, "dc")["probes"]["I(VR)"].as_f64().unwrap().abs()
}

/// Junction capacitance (F) at `vr` volts of reverse bias: the diode behind 100 kohm forms a low-pass; its gain at a frequency
/// near the corner gives C. `guess` places the frequency.
fn cap(part: &str, vr: f64, guess: f64) -> f64 {
    cap_r(part, vr, guess, 100e3)
}

/// [`cap`] through `r` ohms: a diode with a large IS (a big Schottky) drops its reverse bias across 100 kohm.
fn cap_r(part: &str, vr: f64, guess: f64, r: f64) -> f64 {
    let f = 1.0 / (2.0 * std::f64::consts::PI * r * guess);
    let a = run(&format!(
        "new\nadd {part} as=D1\nadd V 1 as=VS\nvalue VS DC {} AC 1\nadd R {r} as=R1\nnet S VS.+ R1.1\nnet X R1.2 D1.A\nnet GND VS.- D1.K\nac {f} {f} ppd=1 probe=V(x)\n",
        -vr
    ));
    let db = answer(&a, "ac")["signals"]["V(X)"]["start"]["db"].as_f64().unwrap();
    let h = 10f64.powf(db / 20.0);
    (1.0 / (h * h) - 1.0).sqrt() / (2.0 * std::f64::consts::PI * f * r)
}

struct Row {
    part: &'static str,
    item: String,
    /// Data sheet min / typ / max, in `unit`.
    ds: [Option<f64>; 3],
    unit: &'static str,
    /// Where in the data sheet, and its conditions.
    at: String,
    /// The model's value in `unit`.
    measure: Box<dyn Fn() -> f64 + Send + Sync>,
    /// Allowed distance from the typ (or the max when only that is given) in `unit`; None = only reported.
    tol: Option<f64>,
}

const VISHAY_4148: &str = "Vishay 1N4148 データシート (Document Number 81857, Rev. 1.6, 2024-11)。25 °C";
const ONSEMI_400X: &str = "onsemi 1N4001〜1N4007 データシート (1N4001/D Rev. 18, 2024-06)。25 °C";
const ONSEMI_581X: &str = "onsemi 1N5817〜1N5819 データシート (1N5817/D Rev. 11, 2021-08)。25 °C";
const VISHAY_5400: &str = "Vishay 1N5400〜1N5408 データシート (Document Number 88516, Revision 19-Dec-2025)。25 °C";
const VISHAY_5820: &str = "Vishay 1N5820〜1N5822 データシート (Document Number 88526, Revision 05-Apr-2019)。25 °C";
const VISHAY_UF400X: &str = "Vishay UF4001〜UF4007 データシート (Document Number 88755, Revision 06-Apr-2020)。25 °C";
const ONSEMI_BAT54: &str = "onsemi BAT54L データシート (BAT54LT1/D Rev. 18, 2024-11)。25 °C";
const VISHAY_SS1X: &str = "Vishay SS12〜SS16 データシート (Document Number 88746, Revision 23-Apr-2020)。25 °C";

fn source(part: &str) -> &'static str {
    match part {
        "1N4148" => VISHAY_4148,
        "1N5817" | "1N5819" => ONSEMI_581X,
        "SS14" => VISHAY_SS1X,
        "1N5408" => VISHAY_5400,
        "1N5822" => VISHAY_5820,
        "UF4007" => VISHAY_UF400X,
        "BAT54" => ONSEMI_BAT54,
        _ => ONSEMI_400X,
    }
}

fn rows() -> Vec<Row> {
    let mut v = vec![];
    // 1N4148: Fig. 1 at 25 C (read from the graph), the table's VF max, CD max, IR max
    for (i, typ, label) in [(0.1e-3, 0.499, "0.1 mA"), (1e-3, 0.610, "1 mA"), (10e-3, 0.730, "10 mA"), (100e-3, 0.877, "100 mA")] {
        v.push(Row {
            part: "1N4148",
            item: format!("順方向電圧 ({label})"),
            ds: [None, Some(typ), if label == "10 mA" { Some(1.0) } else { None }],
            unit: "V",
            at: format!("図 1 の 25 °C (グラフから読んだ){}", if label == "10 mA" { "。表の VF は max 1 V" } else { "" }),
            measure: Box::new(move || vf("1N4148", i)),
            tol: Some(0.015),
        });
    }
    v.push(Row { part: "1N4148", item: "接合容量 (0 V)".into(), ds: [None, None, Some(4.0)], unit: "pF", at: "CD: VR = 0 V、f = 1 MHz (max だけ)".into(), measure: Box::new(|| cap("1N4148", 0.0, 4e-12) * 1e12), tol: Some(0.2) });
    v.push(Row { part: "1N4148", item: "逆電流 (20 V)".into(), ds: [None, None, Some(25.0)], unit: "nA", at: "IR: VR = 20 V (max だけ。図 3 の typ は約 15 nA)".into(), measure: Box::new(|| ir("1N4148", 20.0) * 1e9), tol: None });
    // 1N4001 / 1N4007: Figure 1 at 25 C, the table's VF typ, Figure 3
    for part in ["1N4001", "1N4007"] {
        for (i, typ) in [(0.2, 0.824), (0.5, 0.871), (1.0, 0.916), (2.0, 0.983), (4.0, 1.072)] {
            v.push(Row {
                part,
                item: format!("順方向電圧 ({i} A)"),
                ds: [None, Some(typ), None],
                unit: "V",
                at: "図 1 の 25 °C (グラフから読んだ)".into(),
                measure: Box::new(move || vf(part, i)),
                tol: Some(0.02),
            });
        }
        v.push(Row { part, item: "順方向電圧 (表、1 A)".into(), ds: [None, Some(0.93), Some(1.1)], unit: "V", at: "VF: IF = 1 A、TJ = 25 °C (表。図 1 の 0.916 V とは 1.5% 違う)".into(), measure: Box::new(move || vf(part, 1.0)), tol: Some(0.03) });
        // up to the rated voltage (the 1N4001's 50 V; beyond it the model breaks down)
        let points: &[(f64, f64)] = if part == "1N4001" { &[(1.0, 15.0), (4.0, 9.1), (10.0, 5.92), (30.0, 3.76)] } else { &[(1.0, 15.0), (4.0, 9.1), (10.0, 5.92), (30.0, 3.76), (150.0, 2.35)] };
        for &(vr, typ) in points {
            v.push(Row {
                part,
                item: format!("接合容量 ({vr} V)"),
                ds: [None, Some(typ), None],
                unit: "pF",
                at: "図 3 (容量の代表特性、グラフから読んだ)".into(),
                measure: Box::new(move || cap(part, vr, typ * 1e-12) * 1e12),
                tol: Some(0.15 * typ),
            });
        }
        let rated = if part == "1N4001" { 50.0 } else { 1000.0 };
        v.push(Row { part, item: format!("逆電流 ({rated} V)"), ds: [None, Some(0.05), Some(10.0)], unit: "µA", at: "IR: 定格の直流電圧、TJ = 25 °C".into(), measure: Box::new(move || ir(part, rated) * 1e6), tol: None });
    }
    // 1N5817 / 1N5819: Figure 7 at 25 C (shared by 1N5817 to 1N5819), the table's VF max, IR max (no capacitance in the data
    // sheet). The two models are the same fit; the table's max differ.
    for (part, maxes, rated) in [("1N5817", [0.32, 0.45, 0.75], 20.0), ("1N5819", [0.34, 0.6, 0.9], 40.0)] {
        for (i, typ) in [(0.05, 0.259), (0.1, 0.282), (0.5, 0.359), (1.0, 0.412), (2.5, 0.577), (4.0, 0.695)] {
            v.push(Row {
                part,
                item: format!("順方向電圧 ({i} A)"),
                ds: [None, Some(typ), None],
                unit: "V",
                at: "図 7 の 25 °C (グラフから読んだ。1N5817〜1N5819 で共通)".into(),
                measure: Box::new(move || vf(part, i)),
                tol: Some(0.02),
            });
        }
        for (i, max) in [0.1, 1.0, 3.0].into_iter().zip(maxes) {
            v.push(Row { part, item: format!("順方向電圧 (表、{i} A)"), ds: [None, None, Some(max)], unit: "V", at: "vF: 表の max (パルスで測る)".into(), measure: Box::new(move || vf(part, i)), tol: None });
        }
        v.push(Row { part, item: format!("逆電流 ({rated} V)"), ds: [None, None, Some(1.0)], unit: "mA", at: "IR: 定格の直流電圧、TL = 25 °C (表の max。モデルは IS の分しか流れない)".into(), measure: Box::new(move || ir(part, rated) * 1e3), tol: None });
    }
    // SS14: Fig. 3 at 25 C (the solid curves are SS12 to SS14), the table's VF max, Fig. 5 (solid), IR max
    for (i, typ) in [(0.05, 0.2975), (0.1, 0.3191), (0.5, 0.3787), (1.0, 0.4194)] {
        v.push(Row { part: "SS14", item: format!("順方向電圧 ({i} A)"), ds: [None, Some(typ), None], unit: "V", at: "図 3 の 25 °C、SS12〜SS14 の実線 (グラフから読んだ)".into(), measure: Box::new(move || vf("SS14", i)), tol: Some(0.02) });
    }
    v.push(Row { part: "SS14", item: "順方向電圧 (表、1 A)".into(), ds: [None, None, Some(0.5)], unit: "V", at: "VF: 表の max (パルスで測る)".into(), measure: Box::new(|| vf("SS14", 1.0)), tol: None });
    for (vr, typ) in [(1.0, 184.7), (5.0, 109.8), (10.0, 81.0), (20.0, 56.7)] {
        v.push(Row { part: "SS14", item: format!("接合容量 ({vr} V)"), ds: [None, Some(typ), None], unit: "pF", at: "図 5 (f = 1 MHz)、SS12〜SS14 の実線 (グラフから読んだ)".into(), measure: Box::new(move || cap("SS14", vr, typ * 1e-12) * 1e12), tol: Some(0.15 * typ) });
    }
    // 1N5408, 1N5822, UF4007: their figures at 25 C and the tables' max; BAT54: the table's typ and max
    // (part, forward points, the table's max, (VR, the figure's C, the table's typ C), figure, tolerance of VF, the capacitance
    // measured through this resistance)
    #[allow(clippy::type_complexity)]
    let graphs: [(&'static str, &[(f64, f64)], (f64, f64), (f64, f64, f64), &'static str, f64, f64); 3] = [
        ("1N5408", &[(0.5, 0.7385), (1.0, 0.7767), (3.0, 0.8619), (10.0, 1.0127)], (3.0, 1.2), (4.0, 30.62, 30.0), "図 3", 0.03, 100e3),
        ("1N5822", &[(0.1, 0.2771), (1.0, 0.3934), (3.0, 0.4735), (10.0, 0.6336)], (3.0, 0.525), (5.0, 234.6, 0.0), "図 3", 0.03, 1e3),
        // UF4007: the fit's N is 5 and its 1 A point is 49 mV low (said in the fit's notes)
        ("UF4007", &[(0.05, 0.7619), (0.2, 0.9767), (0.5, 1.1748), (1.0, 1.3991)], (1.0, 1.7), (4.0, 16.49, 17.0), "図 4", 0.05, 1e3),
    ];
    for (part, points, (imax, vmax), (vr, cfig, ctab), fig, vtol, cr) in graphs {
        for &(i, typ) in points {
            v.push(Row { part, item: format!("順方向電圧 ({i} A)"), ds: [None, Some(typ), None], unit: "V", at: format!("{fig} の 25 °C (グラフから読んだ)"), measure: Box::new(move || vf(part, i)), tol: Some(vtol) });
        }
        v.push(Row { part, item: format!("順方向電圧 (表、{imax} A)"), ds: [None, None, Some(vmax)], unit: "V", at: "VF: 表の max (パルスで測る)".into(), measure: Box::new(move || vf(part, imax)), tol: None });
        v.push(Row {
            part,
            item: format!("接合容量 ({vr} V)"),
            ds: [None, Some(cfig), None],
            unit: "pF",
            at: if ctab > 0.0 { format!("容量の図 (グラフから読んだ)。表の typ は {ctab} pF") } else { "容量の図 (グラフから読んだ)".into() },
            measure: Box::new(move || cap_r(part, vr, cfig * 1e-12, cr) * 1e12),
            tol: Some(0.15 * cfig),
        });
    }
    for (i, typ, max) in [(0.1e-3, 0.22, 0.24), (1e-3, 0.29, 0.32), (10e-3, 0.35, 0.4), (30e-3, 0.41, 0.5), (100e-3, 0.52, 0.8)] {
        v.push(Row { part: "BAT54", item: format!("順方向電圧 ({} mA)", i * 1e3), ds: [None, Some(typ), Some(max)], unit: "V", at: "VF: 表の typ と max".into(), measure: Box::new(move || vf("BAT54", i)), tol: Some(0.015) });
    }
    v.push(Row { part: "BAT54", item: "全容量 (1 V)".into(), ds: [None, Some(7.6), Some(10.0)], unit: "pF", at: "CT: VR = 1 V、f = 1 MHz (表。図 4 は 8.1 pF)".into(), measure: Box::new(|| cap("BAT54", 1.0, 7.6e-12) * 1e12), tol: Some(1.2) });
    v.push(Row { part: "SS14", item: "逆電流 (40 V)".into(), ds: [None, None, Some(0.2)], unit: "mA", at: "IR: 定格の直流電圧、TA = 25 °C (表の max。モデルは IS の分しか流れない)".into(), measure: Box::new(|| ir("SS14", 40.0) * 1e3), tol: None });
    v
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
fn diode_models_match_their_data_sheets() {
    let mut bad = vec![];
    let rows = rows();
    let values = checks::par_map(&rows, |r| (r.measure)());
    for (r, v) in rows.iter().zip(values) {
        let target = r.ds[1].or(r.ds[2]);
        if let (Some(t), Some(tol)) = (target, r.tol)
            && ((v - t).abs() > tol || !v.is_finite())
        {
            bad.push(format!("{} {}: model {} {}, data sheet {:?} ({})", r.part, r.item, show(v), r.unit, r.ds, r.at));
        }
    }
    assert!(bad.is_empty(), "models left their data sheets (docs/models/diodes.md):\n{}", bad.join("\n"));
}

#[test]
#[ignore]
fn write_diode_model_report() {
    let (body, tables) = report();
    checks::write(tables, "2026-11-14");
    let text = format!(
        "# 同梱のダイオードのモデルとデータシートの比べ\n\n\
         `crates/cli/tests/integration/diode_models.rs` が作る表(`cargo test -p cli --test integration -- --ignored write_diode_model_report`)。\
         方針は `docs/model-policy.md`、モデルを作るのは `ccad fit`(データシートから読んだ値は `crates/part-lib/fits/` にある)。\n\n\
         - データシートの値は、各表の上に書いたデータシートの表と、描画したグラフから読んだ(2026-11-12。1N5817・1N5819 は 2026-11-14)。グラフの読み取りは 1% ほどの誤差がある。\n\
         - モデルの値は、データシートの条件に近い回路で circuitCad のシミュレータが測ったもの。接合容量は、100 kΩ と組んだローパスの利得から求めた。\n\
         - 「差」はモデル − typ(typ が無い行は max)。合格の幅はテストの各行(順方向電圧は ±15〜20 mV、接合容量は ±15%)。逆電流は比べるだけ(モデルは IS の分しか流れない)。\n{body}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/models/diodes.md");
    std::fs::write(&path, text).unwrap();
    println!("{body}");
}
