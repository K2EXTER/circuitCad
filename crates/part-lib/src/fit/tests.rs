use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `fits/*.fit.json` with its fit and the bundled part file's text.
fn bundled() -> Vec<(Spec, Fitted, String)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root().join("fits"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".fit.json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 11, "{paths:?}");
    paths
        .iter()
        .map(|p| {
            let s = Spec::load(p).unwrap();
            let f = fit(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let part = std::fs::read_to_string(root().join("examples").join(format!("{}.part.json", f.part))).unwrap();
            (s, f, part)
        })
        .collect()
}

/// `NAME=value` pairs of a `.model` card.
/// `NAME=value` pairs of the `.model` cards in the text (a `.subckt` has two: the channel and the body diode; a
/// name is prefixed with its model's name, `DBODY.RS`).
fn params(text: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for card in text.lines().filter(|l| l.trim_start().starts_with(".model")) {
        let model = card.split_whitespace().nth(1).unwrap_or("");
        let inner = card.split_once('(').unwrap().1.trim_end_matches(')');
        for (k, v) in inner.split_whitespace().map(|kv| kv.split_once('=').unwrap()) {
            out.insert(format!("{model}.{k}"), v.parse().unwrap());
        }
    }
    out
}

#[test]
fn the_bundled_models_are_what_their_readings_give() {
    for (_, f, part) in bundled() {
        let d: crate::PartDef = serde_json::from_str(&part).unwrap();
        let sim = d.sim.unwrap();
        // the Python generator (SciPy) made them; the same steps here land on the same numbers, apart from the last
        // digit of a BJT parameter (the old saturation solve stopped at a looser tolerance)
        let (was, now) = (params(&sim.text), params(&f.text));
        assert_eq!(was.keys().collect::<Vec<_>>(), now.keys().collect::<Vec<_>>(), "{}", f.part);
        for (k, a) in &was {
            let b = now[k];
            assert!((b - a).abs() <= 2e-3 * a.abs() + 1e-30, "{} {k}: bundled {a}, fit {b}", f.part);
        }
        assert_eq!(sim.limits, f.limits, "{}", f.part);
        assert_eq!(sim.origin.map(|o| o.source), Some(f.source.clone()), "{}", f.part);
        assert_eq!(d.datasheet, f.url, "{}", f.part);
    }
}

#[test]
fn writing_the_fit_back_changes_nothing_else_in_the_file() {
    for (_, f, part) in bundled() {
        let out = edit::edit_part(&part, &f).unwrap();
        // everything but the model line is the same (the line itself is too unless a last digit moved)
        let differ: Vec<_> = part.lines().zip(out.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(part.lines().count(), out.lines().count(), "{}", f.part);
        assert!(differ.iter().all(|(a, _)| a.trim_start().starts_with("\"text\": \".model")), "{}: {differ:?}", f.part);
    }
}

#[test]
fn each_reading_has_a_row_and_the_rows_stay_near_the_readings() {
    for (s, f, _) in bundled() {
        let n = match &s {
            Spec::Diode(d) => d.forward.points.len() + d.hot.as_ref().map_or(0, |h| h.points.len()) + d.capacitance.points.len(),
            Spec::Bjt(b) => b.hfe_norm.len() + b.vbe_on.len() + b.vbe_sat.len() + b.vce_sat.len() + b.cibo.len() + b.cobo.len(),
            Spec::Mos(m) => {
                let threshold = if m.transfer.is_some() { usize::from(m.vth_range.is_some()) } else { 1 };
                threshold + m.transfer.as_ref().map_or(0, |t| t.points.len()) + m.rds.len() + 2 + m.gfs.map_or(0, |_| 1) + m.body_diode.as_ref().map_or(0, |b| b.points.len())
            }
        };
        assert_eq!(f.rows.len(), n, "{}", f.part);
        // a reading the model's form cannot follow is said in a note (with its condition); the others stay near
        for r in f.rows.iter().filter(|r| !r.compare_only && !f.notes.iter().any(|n| n.contains(&r.at) && n.starts_with(&r.item))) {
            // voltages to 35 mV (a Schottky's hot curve is the loosest), the rest to 25 %
            let ok = if r.unit == "V" { (r.model - r.datasheet).abs() < 0.035 } else { r.error_pct().abs() < 25.0 };
            assert!(ok, "{} {} {}: {} vs {}", f.part, r.item, r.at, r.model, r.datasheet);
        }
    }
}

#[test]
fn too_few_or_impossible_readings_are_explained() {
    let diode = r#"{"kind": "diode", "part": "D", "name": "D", "source": "s", "forward": {"at": "x", "points": [[0.01, 0.7], [0.1, 0.8]]},
        "breakdown": {"v": 50, "i": 1e-6, "note": "n"}, "capacitance": {"at": "none"}}"#;
    let e = fit(&serde_json::from_str(diode).unwrap()).unwrap_err();
    assert!(e.contains("3 点以上"), "{e}");
    let mos = r#"{"kind": "mos", "part": "M", "name": "M", "source": "s", "vth": [2.0, 1e-3], "rds": [[1.5, 0.1, 1.0]], "caps": {"ciss": 1e-10, "crss": 1e-11}}"#;
    let e = fit(&serde_json::from_str(mos).unwrap()).unwrap_err();
    assert!(e.contains("しきい値より大きく"), "{e}");
    let e = Spec::load(&root().join("fits").join("no such part.fit.json")).unwrap_err();
    assert!(e.contains("no such part"), "{e}");
}

#[test]
fn one_on_resistance_is_all_channel() {
    let mos = r#"{"kind": "mos", "part": "M", "name": "M", "source": "s", "vth": [2.0, 1e-3], "rds": [[10.0, 0.5, 1.2]], "caps": {"ciss": 2e-11, "crss": 4e-12}}"#;
    let f = fit(&serde_json::from_str(mos).unwrap()).unwrap();
    assert!(f.text.contains("RD=0 "), "{}", f.text);
    let r = f.rows.iter().find(|r| r.item == "RDS(on)").unwrap();
    assert!(r.error_pct().abs() < 1e-6, "{r:?}");
}
