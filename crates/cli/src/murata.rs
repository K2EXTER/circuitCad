//! `murata`: search Murata's SPICE netlist zip for chip ceramic capacitors (the user's own download; see `part_lib::murata`).

use crate::session::{obj, opt, options};
use crate::Res;
use part_lib::murata::{Filter, MurataIndex};
use serde_json::json;
use std::path::PathBuf;

/// The zip path the app keeps in `settings.json` (key `murata_zip`), if any.
fn saved_zip() -> Option<String> {
    let file = part_lib::default_dir().parent()?.join("settings.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    v.get("murata_zip")?.as_str().filter(|s| !s.is_empty()).map(String::from)
}

fn farads(key: &str, v: Option<&str>) -> Result<Option<f64>, String> {
    let Some(v) = v else { return Ok(None) };
    sim_core::parse_value(v.trim().to_ascii_lowercase().trim_end_matches('f'))
        .filter(|x| *x > 0.0)
        .map(Some)
        .ok_or_else(|| format!("{key}='{v}' is not a capacitance (write 10u, 100n, 22p)"))
}

pub fn murata(args: &[String]) -> Res {
    let (words, kv) = options(args);
    let zip = opt(&kv, "zip").map(String::from).or_else(saved_zip).ok_or(
        "no Murata zip: download 'SPICE netlist' (GRM series) from Murata's site, then give zip=PATH or choose it once in the app (part form > chip capacitor > Murata)",
    )?;
    let idx = MurataIndex::open(&PathBuf::from(&zip))?;
    if let Some(pn) = opt(&kv, "model") {
        let e = idx.entries.iter().find(|e| e.part_number.eq_ignore_ascii_case(pn)).ok_or_else(|| format!("'{pn}' is not in {zip} (search with 'murata WORD')"))?;
        let g = idx.generate(e)?;
        return Ok(obj(json!({"part_number": e.part_number, "size": e.size_inch, "dielectric": e.dielectric, "farads": e.capacitance, "volts": e.voltage, "pins": g.sim_pins, "limits": g.limits, "subckt": g.sim_text})));
    }
    let v_min = match opt(&kv, "vmin") {
        None => None,
        Some(v) => Some(v.trim_end_matches(['V', 'v']).parse::<f64>().map_err(|_| format!("vmin='{v}' is not a voltage (write 16)"))?),
    };
    let filter = Filter {
        text: words.join(""),
        size_inch: opt(&kv, "size").unwrap_or("").to_string(),
        dielectric: opt(&kv, "diel").unwrap_or("").to_string(),
        c_min: farads("cmin", opt(&kv, "cmin"))?,
        c_max: farads("cmax", opt(&kv, "cmax"))?,
        v_min,
    };
    let hits = idx.search(&filter);
    let shown: Vec<_> = hits
        .iter()
        .take(40)
        .map(|e| json!({"part_number": e.part_number, "farads": e.capacitance, "volts": e.voltage, "dielectric": e.dielectric, "size": e.size_inch, "metric": e.size_metric}))
        .collect();
    Ok(obj(json!({
        "count": hits.len(), "shown": shown.len(), "parts": shown, "sizes": idx.sizes(), "dielectrics": idx.dielectrics(),
        "note": "models are 25C, 0 V DC bias, small signal: capacitance does not fall with DC voltage (X5R/X7R lose a lot near rated voltage). 'murata model=PN' prints the subcircuit",
    })))
}
