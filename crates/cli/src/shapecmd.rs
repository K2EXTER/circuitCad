//! `shape [TEMPLATE|auto] part=NAME [DIM=mm ...] [dir=DIR] [max=TRIANGLES] [write]`: the 3D body of a library part from a template
//! (`pcb_core::shape_script`). Writes the SolidCraft script; with SolidCraft installed it also builds the body and answers its
//! triangles, and `write` puts the body into the part (`<library>/<part>.part.json`). `shape` alone lists the templates.

use crate::Res;
use crate::session::{obj, opt, options};
use pcb_core::shape_script::{self, Template};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

const USAGE: &str = "shape [TEMPLATE|auto] part=NAME [DIM=mm ...] [dir=DIR] [max=TRIANGLES] [write] | shape sketch part=NAME pdf=FILE top=PAGE:x0,y0,x1,y1 [side=PAGE:x0,y0,x1,y1] [scale=x0,y0,x1,y1,LEN[in]] [side_scale=...] [body=W,H,HEIGHT] [dir=DIR] [open] | shape take part=NAME [dir=DIR] [max=TRIANGLES]   ('shape' lists the templates and their dimensions. sketch: the outline drawings cut out of the data sheet (PDF points from the top left, pages from 1) as SolidCraft's canvases to model over, scaled by a line of the drawing and its length (PDF points; LEN in mm, or inches with 'in'), else as printed; the numbers printed in each drawing are answered with where they stand; open makes the design and opens SolidCraft. take: the STEP saved as part.step in that folder read into the part)";

impl crate::session::Session {
    pub fn shape(&mut self, args: &[String]) -> Res {
        let (words, kv) = options(args);
        match words.first().map(String::as_str) {
            Some("sketch") => return self.shape_sketch(&words, &kv),
            Some("take") => return self.shape_take(&kv),
            _ => {}
        }
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

    /// The folder of a part's hand-made shape (the app's form uses the same one).
    fn sketch_dir(kv: &[(String, String)], part: &str) -> PathBuf {
        let safe: String = part.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        opt(kv, "dir").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("circuitcad_solidcraft").join(if safe.is_empty() { "part".into() } else { safe }))
    }

    /// `shape sketch`: the drawings cut out of the data sheet into the part's folder with the SolidCraft script over them (E7 ④).
    fn shape_sketch(&mut self, words: &[String], kv: &[(String, String)]) -> Res {
        let name = opt(kv, "part").ok_or(USAGE)?;
        let part = self.lib.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| format!("part=: no library part '{name}' (see 'parts')"))?;
        let pdf = opt(kv, "pdf").ok_or(USAGE)?;
        let ds = datasheet::Datasheet::open_file(std::path::Path::new(pdf))?;
        let dir = Self::sketch_dir(kv, &part.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let nums = |s: &str, n: usize, what: &str| -> Result<Vec<f64>, String> {
            let v: Vec<f64> = s.split([',', ':']).map(|t| t.trim().trim_end_matches("in").trim_end_matches("mm").parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("{what}={s}: numbers"))?;
            if v.len() == n { Ok(v) } else { Err(format!("{what}={s}: {n} numbers")) }
        };
        let mut sketches = vec![];
        let mut answer = vec![];
        for (key, side) in [("top", false), ("side", true)] {
            let Some(spec) = opt(kv, key) else { continue };
            let v = nums(spec, 5, key)?;
            let (page, area) = ((v[0] as usize).max(1) - 1, datasheet::Rect { x0: v[1] as f32, y0: v[2] as f32, x1: v[3] as f32, y1: v[4] as f32 });
            let (w, h) = (f64::from(area.x1 - area.x0), f64::from(area.y1 - area.y0));
            if w <= 1.0 || h <= 1.0 {
                return Err(format!("{key}={spec}: an empty rectangle"));
            }
            // the picture, cut as the form cuts it (2 to 6 pixels a point)
            let scale = (1600.0 / w.max(h)).clamp(2.0, 6.0) as f32;
            let img = ds.render(page, scale)?;
            let (x0, y0) = (((area.x0 * scale).max(0.0) as usize).min(img.width), ((area.y0 * scale).max(0.0) as usize).min(img.height));
            let (x1, y1) = (((area.x1 * scale) as usize).min(img.width), ((area.y1 * scale) as usize).min(img.height));
            let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for y in y0..y1 {
                rgba.extend_from_slice(&img.rgba[(y * img.width + x0) * 4..(y * img.width + x1) * 4]);
            }
            let png = dir.join(if side { "side.png" } else { "top.png" });
            image::RgbaImage::from_raw((x1 - x0) as u32, (y1 - y0) as u32, rgba).ok_or("the picture's size")?.save(&png).map_err(|e| format!("{}: {e}", png.display()))?;
            // the scale: a line of the drawing (PDF points) and its length, else as printed
            let line = opt(kv, if side { "side_scale" } else { "scale" }).or_else(|| opt(kv, "scale"));
            let width = match line {
                Some(l) => {
                    let inch = l.trim_end().ends_with("in");
                    let v = nums(l, 5, "scale")?;
                    let pts = ((v[2] - v[0]).powi(2) + (v[3] - v[1]).powi(2)).sqrt();
                    if pts < 1.0 || v[4] <= 0.0 {
                        return Err(format!("scale={l}: two points apart and a length above 0"));
                    }
                    w * v[4] * if inch { 25.4 } else { 1.0 } / pts
                }
                None => w * 25.4 / 72.0,
            };
            let numbers: Vec<serde_json::Value> = ds.lengths_in(page, area).iter().filter(|n| n.text.contains(['.', ','])).map(|n| json!({"text": n.text, "value": n.value, "at": [(n.rect.x0 + n.rect.x1) / 2.0, (n.rect.y0 + n.rect.y1) / 2.0], "inch": n.inch})).collect();
            answer.push(json!({"view": key, "png": png.display().to_string(), "width_mm": width, "scaled": line.is_some(), "numbers": numbers}));
            sketches.push(pcb_core::shape_script::Sketch { side, path: png.display().to_string(), width });
        }
        if sketches.is_empty() {
            return Err(format!("give top= (and side=): {USAGE}"));
        }
        let starter = opt(kv, "body").map(|b| nums(b, 3, "body")).transpose()?.map(|v| [v[0], v[1], v[2]]);
        let pads = part.footprint().map(|f| f.pads).unwrap_or_default();
        let script = pcb_core::shape_script::sketch_script(&sketches, starter, &pads);
        let file = dir.join("script.json");
        std::fs::write(&file, serde_json::to_string_pretty(&script).unwrap_or_default()).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut out = json!({"part": part.name, "dir": dir.display().to_string(), "script": file.display().to_string(), "drawings": answer,
            "legs": pads.iter().filter(|p| p.drill.is_some()).count(),
            "next": format!("model the part over the drawings in SolidCraft, export it as {} in this folder, then 'shape take part={}'", "part.step", part.name)});
        if words.iter().any(|w| w == "open") {
            let (cli, app) = shape_script::find_solidcraft().ok_or("SolidCraft is not installed (winget install bherbruck.SolidCraft)")?;
            let design = dir.join("part.sc");
            let r = std::process::Command::new(&cli).arg("run").arg(&file).arg("--save").arg(&design).arg("--quiet").output().map_err(|e| format!("{}: {e}", cli.display()))?;
            if !r.status.success() {
                return Err(format!("SolidCraft could not make the design: {}", String::from_utf8_lossy(&r.stderr).lines().last().unwrap_or("")));
            }
            std::process::Command::new(&app).arg(&design).spawn().map_err(|e| format!("{}: {e}", app.display()))?;
            out["opened"] = json!(design.display().to_string());
        }
        Ok(obj(out))
    }

    /// `shape take`: the STEP saved in the part's folder read as the part's 3D shape and written into the part.
    fn shape_take(&mut self, kv: &[(String, String)]) -> Res {
        let name = opt(kv, "part").ok_or(USAGE)?;
        let part = self.lib.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| format!("part=: no library part '{name}' (see 'parts')"))?;
        let step = Self::sketch_dir(kv, &part.name).join("part.step");
        let bytes = std::fs::read(&step).map_err(|e| format!("{}: {e} (export the shape from SolidCraft as part.step there first)", step.display()))?;
        let mut mesh = pcb_core::step::read_step(&bytes)?;
        let keep = opt(kv, "max").map(|m| m.parse::<usize>().map_err(|_| format!("max={m}: a number of triangles"))).transpose()?.unwrap_or(pcb_core::mesh::KEEP_TRIANGLES);
        mesh = pcb_core::mesh::thin_out(mesh, keep.max(100));
        let height = mesh.vertices.iter().map(|v| f64::from(v[2])).fold(0.0, f64::max);
        let triangles = mesh.triangles.len();
        let mut p = part.clone();
        p.model3d = Some(mesh);
        p.model3d_src = std::path::absolute(&step).ok().map(|a| pcb_core::mesh::ModelSource::new(&a.to_string_lossy()));
        let path = part_lib::save(&self.lib_dir, &p).map_err(|e| format!("cannot write the part file: {e}"))?;
        let (lib, errors) = part_lib::load_library(&self.lib_dir);
        self.lib = lib.into();
        self.lib_errors = errors;
        Ok(obj(json!({"part": part.name, "step": step.display().to_string(), "triangles": triangles, "height": height, "saved": path.display().to_string()})))
    }
}
