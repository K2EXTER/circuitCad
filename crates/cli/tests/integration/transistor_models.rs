//! The bundled transistor models (2N3904, 2N3906, 2N7000, AO3400A, AO3401A) measured the way their data sheets measure them, next to the data
//! sheets' numbers (docs/model-policy.md, section 5). Runs the commands of `ccad` on small test circuits (`common::ccad`).
//!
//! `transistor_models_match_their_data_sheets` fails when a model leaves its data sheet by more than the row's tolerance.
//! `cargo test -p cli --test integration -- --ignored write_transistor_model_report` writes `docs/models/transistors.md`
//! and the tables shown next to the parts (`crates/part-lib/model_checks.json`).
//! The models are written by `tools/gen_transistor_models.py` (the readings of the data sheets are there).

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

fn probes(script: &str, names: &[&str]) -> Vec<f64> {
    let a = run(&format!("{script}dc probe={}\n", names.join(",")));
    let d = answer(&a, "dc");
    names.iter().map(|n| d["probes"][*n].as_f64().unwrap_or_else(|| panic!("no probe {n}: {d}"))).collect()
}

/// A capacitance (F) seen at net X: a source with `bias` volts DC and 1 V AC drives X through 100 kohm; the gain at a frequency
/// near the corner gives C. `circuit` connects the part to X (and to GND / S for the rest).
fn cap_at(circuit: &str, bias: f64, guess: f64) -> f64 {
    let r = 100e3;
    let f = 1.0 / (2.0 * std::f64::consts::PI * r * guess);
    let a = run(&format!("new\n{circuit}add V 1 as=VS\nvalue VS DC {bias} AC 1\nadd R {r} as=RX\nnet S VS.+ RX.1\nnet X RX.2\nnet GND VS.-\nac {f} {f} ppd=1 probe=V(x)\n"));
    let db = answer(&a, "ac")["signals"]["V(X)"]["start"]["db"].as_f64().unwrap();
    let h = 10f64.powf(db / 20.0);
    (1.0 / (h * h) - 1.0).sqrt() / (2.0 * std::f64::consts::PI * f * r)
}

// ------------------------------------------------------------------ bipolar transistors
/// A bipolar transistor and its data sheet's numbers (the readings of the graphs are those of `tools/gen_transistor_models.py`).
/// A PNP is measured with every source turned round; its numbers are sizes, as its data sheet prints them.
struct Bjt {
    part: &'static str,
    pnp: bool,
    /// The figures of hFE, of the "ON" voltages and of the capacitances
    figs: (&'static str, &'static str, &'static str),
    /// The normalised hFE curve (VCE = 1 V) with its peak at 173 (the generator's choice: the data sheets have no typ)
    hfe_norm: [(f64, f64); 10],
    /// The table's min hFE at 0.1, 1, 10, 50, 100 mA
    hfe_min: [f64; 5],
    vbe_on: [(f64, f64); 4],
    vbe_sat: [(f64, f64, Option<f64>, Option<f64>); 2],
    vce_sat: [(f64, Option<f64>, Option<f64>); 3],
    /// (typ, max) pF: Cobo at VCB = 5 V, Cibo at VEB = 0.5 V
    cobo: (f64, f64),
    cibo: (f64, f64),
}

const HFE_PEAK: f64 = 173.0;

static BJTS: [Bjt; 2] = [
    Bjt {
        part: "2N3904",
        pnp: false,
        figs: ("図 15", "図 17", "図 3"),
        hfe_norm: [(0.1e-3, 0.443), (0.2e-3, 0.538), (0.5e-3, 0.676), (1e-3, 0.773), (2e-3, 0.868), (5e-3, 0.979), (10e-3, 0.988), (20e-3, 0.859), (50e-3, 0.493), (100e-3, 0.27)],
        hfe_min: [40.0, 70.0, 100.0, 60.0, 30.0],
        vbe_on: [(5e-3, 0.684), (10e-3, 0.706), (50e-3, 0.79), (100e-3, 0.869)],
        vbe_sat: [(10e-3, 0.745, Some(0.65), Some(0.85)), (50e-3, 0.848, None, Some(0.95))],
        vce_sat: [(10e-3, None, Some(0.2)), (50e-3, Some(0.194), Some(0.3)), (100e-3, Some(0.36), None)],
        cobo: (1.96, 4.0),
        cibo: (3.76, 8.0),
    },
    Bjt {
        part: "2N3906",
        pnp: true,
        figs: ("図 13", "図 15", "図 3"),
        hfe_norm: [(0.1e-3, 0.774), (0.2e-3, 0.806), (0.5e-3, 0.854), (1e-3, 0.883), (2e-3, 0.921), (5e-3, 0.971), (10e-3, 0.988), (20e-3, 0.936), (50e-3, 0.511), (100e-3, 0.22)],
        hfe_min: [60.0, 80.0, 100.0, 60.0, 30.0],
        vbe_on: [(5e-3, 0.71), (10e-3, 0.727), (50e-3, 0.81), (100e-3, 0.868)],
        vbe_sat: [(10e-3, 0.757, Some(0.65), Some(0.85)), (50e-3, 0.85, None, Some(0.95))],
        vce_sat: [(10e-3, None, Some(0.25)), (50e-3, Some(0.136), Some(0.4)), (100e-3, Some(0.245), None)],
        cobo: (3.19, 4.5),
        cibo: (4.72, 10.0),
    },
];

impl Bjt {
    fn hfe_ds(&self, ic: f64) -> f64 {
        let pts = self.hfe_norm;
        for w in pts.windows(2) {
            let ((a, na), (b, nb)) = (w[0], w[1]);
            if ic >= a && ic <= b {
                let t = (ic.ln() - a.ln()) / (b.ln() - a.ln());
                return HFE_PEAK * (na + t * (nb - na));
            }
        }
        HFE_PEAK * if ic < pts[0].0 { pts[0].1 } else { pts[pts.len() - 1].1 }
    }

    /// +1 for an NPN, -1 for a PNP: the sources' values are multiplied by it
    fn sign(&self) -> f64 {
        if self.pnp { -1.0 } else { 1.0 }
    }

    /// (|IC|, |IB|, |VBE|) at |VCE| = 1 V with a base current that gives about `ic` on the data sheet's curve.
    fn on(&self, ic: f64) -> (f64, f64, f64) {
        let (ib, s) = (ic / self.hfe_ds(ic), self.sign());
        let p = probes(&format!("new
add {} as=Q1
add V {} as=VC
add I {} as=IB
net C VC.+ Q1.C
net B IB.+ Q1.B
net GND VC.- IB.- Q1.E
", self.part, s, s * ib), &["I(VC)", "V(B)"]);
        (p[0].abs(), ib, p[1].abs())
    }

    /// (|VBE|, |VCE|) with IC forced and IB = IC / 10.
    fn sat(&self, ic: f64) -> (f64, f64) {
        let s = self.sign();
        let p = probes(&format!("new
add {} as=Q1
add I {} as=IC
add I {} as=IB
net C IC.+ Q1.C
net B IB.+ Q1.B
net GND IC.- IB.- Q1.E
", self.part, s * ic, s * ic / 10.0), &["V(B)", "V(C)"]);
        (p[0].abs(), p[1].abs())
    }
}

// ------------------------------------------------------------------ MOSFETs
fn vth(part: &str, id: f64) -> f64 {
    probes(&format!("new\nadd {part} as=M1\nadd I {id} as=ID\nnet D ID.+ M1.D M1.G\nnet GND ID.- M1.S\n"), &["V(D)"])[0]
}

fn rds(part: &str, vgs: f64, id: f64) -> f64 {
    probes(&format!("new\nadd {part} as=M1\nadd V {vgs} as=VG\nadd I {id} as=ID\nnet G VG.+ M1.G\nnet D ID.+ M1.D\nnet GND VG.- ID.- M1.S\n"), &["V(D)"])[0] / id
}

/// |ID| for the gate from `from` to `to` volts (200 points of one `sweep`) with the drain held at `vds`, as (VGS, |ID|).
fn id_sweep(part: &str, vds: f64, from: f64, to: f64) -> Vec<(f64, f64)> {
    let a = run(&format!("new\nadd {part} as=M1\nadd V {from} as=VG\nadd V {vds} as=VD\nnet G VG.+ M1.G\nnet D VD.+ M1.D\nnet GND VG.- VD.- M1.S\nsweep VG {from} {to} steps=200 probe=I(VD)\n"));
    let s = answer(&a, "sweep");
    s["rows"].as_array().unwrap().iter().map(|r| (r["value"].as_f64().unwrap(), r["I(VD)"].as_f64().unwrap_or_else(|| panic!("no I(VD): {r}")).abs())).collect()
}

/// Forward transconductance at drain current `id` and `vds`. The gate voltage for `id`: a coarse sweep finds the step it is in, a
/// fine one over that step finds it (two runs of `ccad`; a bisection took 42 and about 33 s per part). The slope there is that of the
/// parabola through the three nearest points of the fine sweep (exact for the square law; the points are about 0.5 mV apart).
fn gfs(part: &str, vds: f64, id: f64) -> f64 {
    let id = id.abs();
    let cross = |pts: &[(f64, f64)]| pts.windows(2).position(|w| w[0].1 < id && w[1].1 >= id).unwrap_or_else(|| panic!("{part}: ID = {id} A not reached in {:?}..{:?}", pts.first(), pts.last()));
    // the gate goes the way of the drain: up for an N channel, down for a P channel
    let coarse = id_sweep(part, vds, 0.0, 20.0 * vds.signum());
    let k = cross(&coarse);
    let fine = id_sweep(part, vds, coarse[k].0, coarse[k + 1].0);
    let j = cross(&fine);
    let ((v0, i0), (v1, i1)) = (fine[j], fine[j + 1]);
    let v = v0 + (id - i0) / (i1 - i0) * (v1 - v0);
    let m = j.clamp(1, fine.len() - 2);
    let ((xa, ya), (xb, yb), (xc, yc)) = (fine[m - 1], fine[m], fine[m + 1]);
    // |ID| falls with the gate of a P channel: the transconductance is the size of the slope
    (ya * (2.0 * v - xb - xc) / ((xa - xb) * (xa - xc)) + yb * (2.0 * v - xa - xc) / ((xb - xa) * (xb - xc)) + yc * (2.0 * v - xa - xb) / ((xc - xa) * (xc - xb))).abs()
}

struct Row {
    part: &'static str,
    item: String,
    ds: [Option<f64>; 3],
    unit: &'static str,
    at: String,
    measure: Box<dyn Fn() -> f64 + Send + Sync>,
    /// Allowed distance from the typ (or the max / min when only that is given) in `unit`; None = only reported.
    tol: Option<f64>,
}

fn source(part: &str) -> &'static str {
    match part {
        "2N3904" => "onsemi 2N3903/2N3904 データシート (2N3903/D Rev. 9, 2021-08)。25 °C",
        "2N3906" => "onsemi 2N3906 データシート (2N3906/D Rev. 4, 2010-02)。25 °C",
        "2N7000" => "onsemi 2N7000/2N7002/NDS7002A データシート (NDS7002A/D Rev. 11, 2025-03)。25 °C",
        "AO3401A" => "Alpha & Omega AO3401A データシート (Rev. 3.1, 2023-12)。25 °C",
        _ => "Alpha & Omega AO3400A データシート (Rev. 3.1, 2023-07)。25 °C",
    }
}

fn rows() -> Vec<Row> {
    let mut v = vec![];
    // bipolar: hFE and VBE at VCE = 1 V (the hFE and "ON" voltages graphs), the table's min hFE, saturation (the graph and the
    // table), capacitances
    for t in &BJTS {
        let (part, (fh, fon, fcap)) = (t.part, t.figs);
        for (k, ic) in [0.1e-3, 1e-3, 10e-3, 50e-3, 100e-3].into_iter().enumerate() {
            v.push(Row {
                part,
                item: format!("hFE ({} mA)", ic * 1e3),
                ds: [Some(t.hfe_min[k]), Some(t.hfe_ds(ic)), if ic == 10e-3 { Some(300.0) } else { None }],
                unit: "",
                at: format!("VCE = 1 V。typ は{fh}の相対値 × 173 (表の min と max の幾何平均。データシートに typ は無い)。min / max は表"),
                measure: Box::new(move || {
                    let (i, ib, _) = t.on(ic);
                    i / ib
                }),
                tol: Some(0.15 * t.hfe_ds(ic)),
            });
        }
        for (ic, typ) in t.vbe_on {
            v.push(Row { part, item: format!("VBE ({} mA)", ic * 1e3), ds: [None, Some(typ), None], unit: "V", at: format!("VCE = 1 V、{fon} (グラフから読んだ)"), measure: Box::new(move || t.on(ic).2), tol: Some(0.015) });
        }
        for (ic, typ, min, max) in t.vbe_sat {
            v.push(Row { part, item: format!("VBE(sat) ({} mA)", ic * 1e3), ds: [min, Some(typ), max], unit: "V", at: format!("IC/IB = 10。typ は{fon}、min / max は表"), measure: Box::new(move || t.sat(ic).0), tol: Some(0.02) });
        }
        for (ic, typ, max) in t.vce_sat {
            v.push(Row { part, item: format!("VCE(sat) ({} mA)", ic * 1e3), ds: [None, typ, max], unit: "V", at: format!("IC/IB = 10。typ は{fon}、max は表"), measure: Box::new(move || t.sat(ic).1), tol: typ.map(|_| 0.03) });
        }
        let s = t.sign();
        v.push(Row {
            part,
            item: "Cobo (5 V)".into(),
            ds: [None, Some(t.cobo.0), Some(t.cobo.1)],
            unit: "pF",
            at: format!("VCB = 5 V、IE = 0。typ は{fcap}、max は表"),
            measure: Box::new(move || cap_at(&format!("add {part} as=Q1
add R 1G as=RE
net X Q1.C
net GND Q1.B RE.2
net EM Q1.E RE.1
"), 5.0 * s, 2e-12) * 1e12),
            tol: Some(0.3),
        });
        v.push(Row {
            part,
            item: "Cibo (0.5 V)".into(),
            ds: [None, Some(t.cibo.0), Some(t.cibo.1)],
            unit: "pF",
            at: format!("VEB = 0.5 V、IC = 0。typ は{fcap}、max は表"),
            measure: Box::new(move || cap_at(&format!("add {part} as=Q1
add R 1G as=RC
net X Q1.E
net GND Q1.B RC.2
net CO Q1.C RC.1
"), 0.5 * s, 4e-12) * 1e12),
            tol: Some(0.5),
        });
    }
    // MOSFETs: VGS(th), RDS(on) at each gate voltage of the table, gFS, the gate capacitances
    // a P channel is measured with the signs of its data sheet: VGS, ID and VDS negative (the same circuits, the sources turned round)
    let mos: [(&'static str, (f64, f64), &[(f64, f64, f64, f64)], (f64, f64, f64), (f64, f64)); 3] = [
        ("2N7000", (2.1, 1e-3), &[(10.0, 0.5, 1.2, 5.0), (4.5, 0.075, 1.8, 5.3)], (0.32, 10.0, 0.2), (20.0, 4.0)),
        ("AO3400A", (1.05, 250e-6), &[(10.0, 5.7, 18.0e-3, 26.5e-3), (4.5, 5.0, 19.0e-3, 32.0e-3), (2.5, 3.0, 24.0e-3, 48.0e-3)], (33.0, 5.0, 5.7), (630.0, 50.0)),
        ("AO3401A", (-0.9, -250e-6), &[(-10.0, -4.0, 41.0e-3, 50.0e-3), (-4.5, -3.5, 47.0e-3, 60.0e-3), (-2.5, -2.5, 60.0e-3, 85.0e-3)], (17.0, -5.0, -4.0), (645.0, 55.0)),
    ];
    for (part, (vt, it), points, (gm, gvds, gid), (ciss, crss)) in mos {
        v.push(Row { part, item: "しきい値 VGS(th)".into(), ds: [None, Some(vt), None], unit: "V", at: format!("VDS = VGS、ID = {} mA (表)", it * 1e3), measure: Box::new(move || vth(part, it)), tol: Some(0.05) });
        for &(vgs, id, typ, max) in points {
            let (k, u) = if typ < 0.1 { (1e3, "mΩ") } else { (1.0, "Ω") };
            v.push(Row {
                part,
                item: format!("RDS(on) (VGS = {vgs} V)"),
                ds: [None, Some(typ * k), Some(max * k)],
                unit: u,
                at: format!("ID = {id} A (表)"),
                measure: Box::new(move || rds(part, vgs, id) * k),
                tol: Some(0.15 * typ * k),
            });
        }
        v.push(Row { part, item: "gFS".into(), ds: [None, Some(gm), None], unit: "S", at: format!("VDS = {gvds} V、ID = {gid} A (表){}", if part == "2N7000" { "。オン抵抗に合わせた 2乗則では、ここは合わない" } else { "" }), measure: Box::new(move || gfs(part, gvds, gid)), tol: None });
        v.push(Row {
            part,
            item: "Ciss".into(),
            ds: [None, Some(ciss), None],
            unit: "pF",
            at: "VGS = 0 (表。モデルの容量は電圧によらない)".into(),
            measure: Box::new(move || cap_at(&format!("add {part} as=M1\nnet X M1.G\nnet GND M1.S M1.D\n"), 0.0, ciss * 1e-12) * 1e12),
            tol: Some(0.05 * ciss),
        });
        v.push(Row {
            part,
            item: "Crss".into(),
            ds: [None, Some(crss), None],
            unit: "pF",
            at: "VGS = 0 (表)".into(),
            measure: Box::new(move || cap_at(&format!("add {part} as=M1\nnet X M1.D\nnet GND M1.S M1.G\n"), 0.0, crss * 1e-12) * 1e12),
            tol: Some(0.05 * crss),
        });
    }
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
        let unit = |t: String| if t == "—" || r.unit.is_empty() { t } else { format!("{t} {}", r.unit) };
        out += &format!("| {} | {} | {} | {} | {} |\n", r.item, unit(ds.clone()), r.at, unit(show(v)), unit(diff.clone()));
        tables.last_mut().unwrap().2.push(part_lib::model_checks::CheckRow {
            item: r.item.clone(),
            datasheet: unit(ds),
            target: match (r.ds[1], r.ds[2]) {
                (Some(t), _) => unit(show(t)),
                (None, Some(m)) => format!("max {}", unit(show(m))),
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
fn transistor_models_match_their_data_sheets() {
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
        // a table limit is a limit: the model's typical part stays inside it
        if let Some(min) = r.ds[0].filter(|_| r.tol.is_some())
            && v < min
        {
            bad.push(format!("{} {}: model {} below the data sheet's min {}", r.part, r.item, show(v), min));
        }
    }
    assert!(bad.is_empty(), "models left their data sheets (docs/models/transistors.md):\n{}", bad.join("\n"));
}

#[test]
#[ignore]
fn write_transistor_model_report() {
    let (body, tables) = report();
    checks::write(tables, "2026-11-14");
    let text = format!(
        "# 同梱のトランジスタ・MOSFET のモデルとデータシートの比べ\n\n\
         `crates/cli/tests/integration/transistor_models.rs` が作る表(`cargo test -p cli --test integration -- --ignored write_transistor_model_report`)。\
         方針は `docs/model-policy.md`、モデルを作るのは `tools/gen_transistor_models.py`(データシートから読んだ値はそこにある)。\n\n\
         - データシートの値は、各表の上に書いたデータシートの表と、描画したグラフから読んだ(2026-11-12。AO3401A は 2026-11-14)。グラフの読み取りは 1% ほどの誤差がある。\n\
         - 2N3904 の hFE の typ はデータシートに無い。図 15 の相対値に、表の min 100 と max 300(10 mA)の幾何平均 173 を掛けた値と比べる。実物の hFE は 100〜300 と大きく違う。\n\
         - MOSFET はレベル1(2乗則)に直列抵抗 RD を足したもの。オン抵抗としきい値に合わせたので、飽和領域の gFS は合うとは限らない(比べるだけ。2N7000 は 36% 高い)。容量は電圧によらない一定の値。\n\
         - 「差」はモデル − typ(typ が無い行は max)。合格の幅はテストの各行。\n{body}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/models/transistors.md");
    std::fs::write(&path, text).unwrap();
    println!("{body}");
}
