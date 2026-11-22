//! `shape [TEMPLATE|auto] part=NAME [DIM=mm ...] [dir=DIR] [max=TRIANGLES] [write]`: the 3D body of a library part from a template
//! (`pcb_core::shape_script`). Writes the SolidCraft script; with SolidCraft installed it also builds the body and answers its
//! triangles, and `write` puts the body into the part (`<library>/<part>.part.json`). `shape` alone lists the templates.

use crate::Res;
use crate::session::{obj, opt, options};
use pcb_core::shape_script::{self, Template};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

const USAGE: &str = "shape [dc_jack|tact|relay|terminal|auto] part=NAME [DIM=mm ...] [dir=DIR] [max=TRIANGLES] [write]   ('shape' lists the templates and their dimensions)";

impl crate::session::Session {
    pub fn shape(&mut self, args: &[String]) -> Res {
        let (words, kv) = options(args);
        let Some(key) = words.iter().find(|w| !w.eq_ignore_ascii_case("write")) else {
            let list: Vec<_> = Template::ALL
                .iter()
                .map(|t| json!({"template": t.key(), "what": t.label(), "dims": t.params().iter().map(|p| json!({"key": p.key, "default": p.default, "what": p.label})).collect::<Vec<_>>()}))
                .collect();
            return Ok(obj(json!({"templates": list, "usage": USAGE})));
        };
        let name = opt(&kv, "part").ok_or(USAGE)?;
        let part = self.lib.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| format!("part=: no library part '{name}' (see 'parts')"))?;
        let fp = part.footprint()?;
        let template = if key.eq_ignore_ascii_case("auto") {
            Template::for_footprint(&part.prefix, &fp.name).ok_or_else(|| format!("{}: no template fits the footprint {} (name one: dc_jack tact relay terminal)", part.name, fp.name))?
        } else {
            Template::from_key(&key.to_ascii_lowercase()).ok_or_else(|| format!("no template '{key}': {USAGE}"))?
        };
        let mut dims = BTreeMap::new();
        for (k, v) in &kv {
            if ["part", "dir", "max"].contains(&k.as_str()) {
                continue;
            }
            let x: f64 = v.trim().trim_end_matches("mm").parse().map_err(|_| format!("{k}={v}: not a number (mm)"))?;
            dims.insert(k.clone(), x);
        }
        let script = shape_script::script(template, &dims, &fp.pads)?;
        let dir = opt(&kv, "dir").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("circuitcad_solidcraft").join(&part.name));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = dir.join("shape.json");
        std::fs::write(&file, serde_json::to_string_pretty(&script).unwrap_or_default()).map_err(|e| format!("{}: {e}", file.display()))?;
        let used: BTreeMap<&str, f64> = shape_script::values(template, &dims)?;
        let mut out = json!({"part": part.name, "template": template.key(), "dims": used, "script": file.display().to_string(), "commands": script.as_array().map_or(0, Vec::len)});
        let Some((cli, _)) = shape_script::find_solidcraft() else {
            out["solidcraft"] = json!(false);
            out["hint"] = json!("SolidCraft is not installed: open the script in it by hand (solidcraft-cli run shape.json --out part.step), or install it (winget install bherbruck.SolidCraft) and run this again");
            return Ok(obj(out));
        };
        let mut mesh = shape_script::build(&cli, &script, &dir)?;
        if let Some(m) = opt(&kv, "max") {
            let keep: usize = m.parse().map_err(|_| format!("max={m}: a number of triangles"))?;
            mesh = pcb_core::mesh::thin_out(mesh, keep.max(100));
        }
        out["solidcraft"] = json!(true);
        out["triangles"] = json!(mesh.triangles.len());
        out["height"] = json!(mesh.vertices.iter().map(|v| f64::from(v[2])).fold(0.0, f64::max));
        if words.iter().any(|w| w.eq_ignore_ascii_case("write")) {
            let mut p = part.clone();
            p.model3d = Some(mesh);
            let path = part_lib::save(&self.lib_dir, &p).map_err(|e| format!("cannot write the part file: {e}"))?;
            let (lib, errors) = part_lib::load_library(&self.lib_dir);
            self.lib = lib.into();
            self.lib_errors = errors;
            out["saved"] = json!(path.display().to_string());
        } else {
            out["hint"] = json!("add 'write' to put this body into the part");
        }
        Ok(obj(out))
    }
}
