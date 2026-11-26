use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `fits/*.fit.json` with its fit and the bundled part file's text. The fits are worked out once for all the tests below and
/// on all the cores (one after another they took 17 s, and each of four tests did them all again).
fn bundled() -> &'static [(Spec, Fitted, String)] {
    static FITS: std::sync::OnceLock<Vec<(Spec, Fitted, String)>> = std::sync::OnceLock::new();
    FITS.get_or_init(|| {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(root().join("fits"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().ends_with(".fit.json"))
            .collect();
        paths.sort();
        assert!(paths.len() >= 11, "{paths:?}");
        pcb_core::par::par_map(&paths, |p| {
            let s = Spec::load(p).unwrap();
            let f = fit(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let part = std::fs::read_to_string(root().join("examples").join(format!("{}.part.json", f.part))).unwrap();
            (s, f, part)
        })
    })
}

/// The two texts are the same but for numbers that differ in their last digit (1e-5 of the number): the text cut into runs of
/// the characters a number is written with, each run the same or both numbers that close.
fn same_but_the_last_digit(a: &str, b: &str) -> bool {
    let runs = |t: &str| {
        let mut out: Vec<String> = Vec::new();
        let mut num = None;
        for c in t.chars() {
            let n = c.is_ascii_digit() || ".eE+-".contains(c);
            if num != Some(n) {
                out.push(String::new());
                num = Some(n);
            }
            out.last_mut().unwrap().push(c);
        }
        out
    };
    let (ra, rb) = (runs(a), runs(b));
    ra.len() == rb.len()
        && ra.iter().zip(&rb).all(|(x, y)| {
            x == y
                || match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(p), Ok(q)) => (p - q).abs() <= 1e-5 * p.abs().max(q.abs()),
                    _ => false,
                }
        })
}

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
    for (spec, f, part) in bundled() {
        let d: crate::PartDef = serde_json::from_str(&part).unwrap();
        let sim = d.sim.unwrap();
        // the Python generator (SciPy) made them; the same steps here land on the same numbers on the system they were made on, apart
        // from the last digit of a BJT parameter (the old saturation solve stopped at a looser tolerance)
        let (was, now) = (params(&sim.text), params(&f.text));
        assert_eq!(was.keys().collect::<Vec<_>>(), now.keys().collect::<Vec<_>>(), "{}", f.part);
        // a transistor's model as written is inside the fit's ranges: the comparison below means something (BC547's BF sat a
        // rounding past its edge, and its measure was the 1e6 of a model out of range)
        if let Some(a) = objective(&spec, &sim.text) {
            assert!(a < 1e5, "{}: the bundled model is out of the fit's ranges ({a})", f.part);
        }
        let apart: Vec<String> = was.iter().filter(|(k, a)| (now[*k] - **a).abs() > 2e-3 * a.abs() + 1e-30).map(|(k, a)| format!("{k}: bundled {a}, fit {}", now[k])).collect();
        if !apart.is_empty() {
            // a transistor's fit is a search in nine dimensions: the last bit of exp and log differs between systems (Windows, Linux,
            // macOS), and the search can end in another place of a flat valley. Then the fit must match the readings as well as the
            // bundled model does, by the fit's own measure: within a quarter (BD139 ends at 1.54 on Linux, 1.82 on Windows and 2.04
            // on macOS)
            let (Some(a), Some(b)) = (objective(&spec, &sim.text), objective(&spec, &f.text)) else { panic!("{}: {apart:?}", f.part) };
            eprintln!("{}: bundled {a:.4}, fit {b:.4}: {apart:?}", f.part);
            assert!(b <= a * 1.25 + 1e-6, "{}: the fit matches the readings worse ({b:.4}) than the bundled model ({a:.4}): {apart:?}", f.part);
        }
        assert_eq!(sim.limits, f.limits, "{}", f.part);
        assert_eq!(sim.origin.map(|o| o.source), Some(f.source.clone()), "{}", f.part);
        assert_eq!(d.datasheet, f.url, "{}", f.part);
    }
}

/// The op amps, regulators and the TL431 (made by `tools/gen_opamp_models.py` and `gen_regulator_models.py` before M3):
/// the templates give the bundled model to the character, and the description and parameters the part has.
#[test]
fn the_templates_give_the_bundled_op_amps_and_regulators_exactly() {
    let mut n = 0;
    for (spec, f, part) in bundled() {
        if !matches!(spec, Spec::Opamp(_) | Spec::Regulator(_) | Spec::Shunt(_) | Spec::Reference(_) | Spec::SetLdo(_) | Spec::Buck(_)) {
            continue;
        }
        n += 1;
        let d: crate::PartDef = serde_json::from_str(&part).unwrap();
        let sim = d.sim.unwrap();
        // to the character but the last digit of a number: a fit to a graph (the ripple's high-passes) lands a rounding apart on
        // macOS (MCP1700's 7.62576e-08 for 7.62575e-08)
        assert!(same_but_the_last_digit(&sim.text, &f.text), "{}:
{}
{}", f.part, sim.text, f.text);
        if matches!(spec, Spec::Opamp(_)) {
            assert_eq!(d.description, f.description, "{}", f.part);
        }
        let back = edit::edit_part(part, f).unwrap();
        assert!(same_but_the_last_digit(&back, part), "{}: writing the model back changes the file", f.part);
    }
    assert_eq!(n, 20, "7 op amps (ADA4522-1 since 2026-11-23), 9 regulators (LM317L), the TL431, the REF5025, the LT3045 and the TPS62933 (P6)");
}

#[test]
fn writing_the_fit_back_changes_nothing_else_in_the_file() {
    for (_, f, part) in bundled() {
        let out = edit::edit_part(&part, &f).unwrap();
        // everything but the model's line is the same (the line itself is too unless a number moved: see above). A model is the
        // line of a `.model` card, or of a `.subckt` with its cards (a Darlington pair, a MOSFET with its body diode)
        let differ: Vec<_> = part.lines().zip(out.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(part.lines().count(), out.lines().count(), "{}", f.part);
        assert!(differ.iter().all(|(a, _)| ["\"text\": \".model", "\"text\": \".subckt"].iter().any(|t| a.trim_start().starts_with(t))), "{}: {differ:?}", f.part);
    }
}

#[test]
fn each_reading_has_a_row_and_the_rows_stay_near_the_readings() {
    for (s, f, _) in bundled() {
        let n = match &s {
            Spec::Diode(d) => {
                d.forward.points.len()
                    + d.hot.as_ref().map_or(0, |h| h.points.len())
                    + d.capacitance.points.len()
                    + d.zener.as_ref().map_or(0, |z| 2 * z.zz.len() - 1 + usize::from(z.zzt_typ.is_some()) + z.leak.len() + z.leak.iter().filter(|p| p[1] >= 1e-6).count())
            }
            Spec::Bjt(b) => b.hfe_norm.len() + b.vbe_on.len() + b.vbe_sat.len() + b.vce_sat.len() + b.cibo.len() + b.cobo.len(),
            Spec::Darlington(d) => d.hfe.len() + d.vbe_on.len() + d.vbe_sat.len() + d.vce_sat.len() + if d.cib.len() >= 2 && d.cob.len() >= 2 { d.cib.len() + d.cob.len() } else { 0 },
            Spec::Photodiode(p) => p.light_points.len() + usize::from(p.isc.is_some()) + 1 + usize::from(p.voc_tc.is_some()) + 1 + p.capacitance.len(),
            Spec::Optocoupler(o) => o.led.len() + usize::from(o.led_typ.is_some()) + o.output.len() + o.ctr_curve.len() + 1,
            Spec::Mos(m) => {
                let threshold = if m.transfer.is_some() { usize::from(m.vth_range.is_some()) } else { 1 };
                threshold + m.transfer.as_ref().map_or(0, |t| t.points.len()) + m.rds.len() + 2 + m.gfs.map_or(0, |_| 1) + m.body_diode.as_ref().map_or(0, |b| b.points.len()) + m.rds_temp.as_ref().map_or(0, |t| t.points.len())
            }
            // built from typ values with a template, not fitted: nothing to compare here (the measuring tests in
            // crates/cli/tests/integration/{opamp,regulator}_models.rs compare them with their data sheets)
            Spec::Opamp(_) | Spec::Regulator(_) | Spec::Shunt(_) => 0,
            // the PSRR shape against the table's and the graph's points
            Spec::Reference(r) => r.psrr.len(),
            Spec::SetLdo(r) => r.psrr.len(),
            // the frequency law against the table's RT points
            Spec::Buck(b) => if b.fsw.rt.len() >= 2 { b.fsw.rt.len() } else { 0 },
            // the template's own response times and VOL at the data sheet's conditions (comparator_models.rs measures them)
            Spec::Comparator(_) => 3,
            Spec::Ntc(n) => n.table.len(),
            Spec::Crystal(c) => 1 + usize::from(c.cl.is_some()),
            Spec::Tvs(_) => 3,
            Spec::Relay(_) => 2,
        };
        assert_eq!(f.rows.len(), n, "{}", f.part);
        // a reading the model's form cannot follow is said in a note (with its condition); the others stay near
        for r in f.rows.iter().filter(|r| !r.compare_only && !f.notes.iter().any(|n| n.contains(&r.at) && n.starts_with(&r.item))) {
            // voltages to 35 mV (a Schottky's hot curve is the loosest; twice that for a Darlington's two junctions), the rest to 25 %
            let volts = if matches!(s, Spec::Darlington(_)) { 0.07 } else { 0.035 };
            let ok = if r.unit == "V" { (r.model - r.datasheet).abs() < volts } else { r.error_pct().abs() < 25.0 };
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

#[test]
fn a_last_digit_apart_is_the_same_text_but_a_changed_word_or_value_is_not() {
    assert!(same_but_the_last_digit("Crh0 rhi rh0 7.62576e-08\n", "Crh0 rhi rh0 7.62575e-08\n"));
    assert!(!same_but_the_last_digit("Rrh0 rh0 0 1k", "Rrh0 rh0 0 2k"));
    assert!(!same_but_the_last_digit("C 7.6e-08", "C 7.7e-08"));
    assert!(!same_but_the_last_digit("Elf lfa gnd in gnd 1", "Elf lfa gnd out gnd 1"));
}
