//! `models` (alias `murata`): search the SPICE model zips the makers publish (see `part_lib::vendorzip`). The zips are the
//! user's own downloads; the app keeps their paths in `settings.json` (key `model_zips`).

use crate::session::{obj, opt, options};
use crate::Res;
use part_lib::vendorzip::{Filter, VendorIndex};
use serde_json::json;
use std::path::PathBuf;

/// The zip paths the app keeps in `settings.json`: `model_zips`, and the first version's single `murata_zip`.
fn saved_zips() -> Vec<String> {
    let Some(file) = part_lib::default_dir().parent().map(|d| d.join("settings.json")) else { return vec![] };
    let Some(v) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return vec![] };
    let mut zips: Vec<String> = v.get("model_zips").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str()).map(String::from).collect()).unwrap_or_default();
    if let Some(old) = v.get("murata_zip").and_then(|s| s.as_str()).filter(|s| !s.is_empty())
        && !zips.iter().any(|z| z == old)
    {
        zips.push(old.to_string());
    }
    zips
}

fn farads(key: &str, v: Option<&str>) -> Result<Option<f64>, String> {
    let Some(v) = v else { return Ok(None) };
    sim_core::parse_value(v.trim().to_ascii_lowercase().trim_end_matches('f'))
        .filter(|x| *x > 0.0)
        .map(Some)
        .ok_or_else(|| format!("{key}='{v}' is not a capacitance (write 10u, 100n, 22p)"))
}

impl crate::session::Session {
    pub fn models(&mut self, args: &[String]) -> Res {
        models(self, args)
    }
}

fn models(session: &mut crate::session::Session, args: &[String]) -> Res {
    let (words, kv) = options(args);
    let zips = match opt(&kv, "zip") {
        Some(z) => vec![z.to_string()],
        None => saved_zips(),
    };
    if zips.is_empty() {
        return Err("no model zip: download a maker's SPICE model zip (e.g. Murata 'SPICE netlist', GRM series), then give zip=PATH or add it once in the app (part form > chip capacitor > maker models)".into());
    }
    let mut indexes = vec![];
    let mut skipped = vec![];
    for z in &zips {
        match VendorIndex::open(&PathBuf::from(z)) {
            Ok(i) => indexes.push(i),
            Err(e) => skipped.push(json!({"zip": z, "error": e})),
        }
    }
    if indexes.is_empty() {
        return Err(skipped.iter().filter_map(|s| s["error"].as_str()).collect::<Vec<_>>().join("; "));
    }
    // `models use PART_NUMBER`: put the model into this session's library, so that `add PART_NUMBER` places it
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("use")) {
        let pn = words.get(1).ok_or("models use PART_NUMBER (search with 'models WORD')")?;
        for idx in &indexes {
            if let Some(e) = idx.entries.iter().find(|e| e.part_number.eq_ignore_ascii_case(pn)) {
                let made = idx.part_def(e)?;
                let name = made.part.name.clone();
                let pins: Vec<_> = made.part.pins.iter().map(|p| json!({"pin": p.number, "name": p.name})).collect();
                let out = json!({"part": name, "value": made.part.default_value(), "pins": pins, "footprint": made.footprint, "todo": made.todo, "simulates": true, "hint": format!("add {name} [VALUE] as=C1")});
                session.lib.retain(|d| !d.name.eq_ignore_ascii_case(&name));
                session.lib.push(made.part);
                return Ok(obj(out));
            }
        }
        return Err(format!("'{pn}' is not in the model zips (search with 'models WORD')"));
    }
    if let Some(pn) = opt(&kv, "model") {
        for idx in &indexes {
            if let Some(e) = idx.entries.iter().find(|e| e.part_number.eq_ignore_ascii_case(pn)) {
                let g = idx.generate(e)?;
                let figures = e.meta.as_ref().map(|m| json!({"size": m.size_inch, "dielectric": m.dielectric, "farads": m.capacitance, "volts": m.voltage}));
                return Ok(obj(json!({"part_number": e.part_number, "maker": e.maker, "figures": figures, "pins": g.sim_pins, "limits": g.limits, "subckt": g.sim_text})));
            }
        }
        return Err(format!("'{pn}' is not in the model zips (search with 'models WORD')"));
    }
    let v_min = match opt(&kv, "vmin") {
        None => None,
        Some(v) => Some(v.trim_end_matches(['V', 'v']).parse::<f64>().map_err(|_| format!("vmin='{v}' is not a voltage (write 16)"))?),
    };
    let filter = Filter {
        text: words.join(" "),
        size_inch: opt(&kv, "size").unwrap_or("").to_string(),
        dielectric: opt(&kv, "diel").unwrap_or("").to_string(),
        c_min: farads("cmin", opt(&kv, "cmin"))?,
        c_max: farads("cmax", opt(&kv, "cmax"))?,
        v_min,
    };
    let hits: Vec<_> = indexes.iter().flat_map(|i| i.search(&filter)).collect();
    let shown: Vec<_> = hits
        .iter()
        .take(40)
        .map(|e| match &e.meta {
            Some(m) => json!({"part_number": e.part_number, "maker": e.maker, "nodes": e.nodes, "farads": m.capacitance, "volts": m.voltage, "dielectric": m.dielectric, "size": m.size_inch, "metric": m.size_metric}),
            None => json!({"part_number": e.part_number, "nodes": e.nodes}),
        })
        .collect();
    let mut sizes: Vec<String> = indexes.iter().flat_map(|i| i.sizes()).collect();
    sizes.dedup();
    let mut dielectrics: Vec<String> = indexes.iter().flat_map(|i| i.dielectrics()).collect();
    dielectrics.sort();
    dielectrics.dedup();
    Ok(obj(json!({
        "count": hits.len(), "shown": shown.len(), "parts": shown, "sizes": sizes, "dielectrics": dielectrics, "skipped_zips": skipped,
        "note": "size/diel/cmin/cmax/vmin only match models a maker reader described (now Murata, Nippon Chemi-Con and Nichicon: Murata is 25C, 0 V DC bias, small signal; capacitance does not fall with DC voltage, which X5R/X7R lose a lot near rated voltage). Other models are found by name only. 'models model=PN' prints the subcircuit",
    })))
}
