//! `lib` (the part library folder: look at it, read it again) and `part import` (a maker's SPICE subcircuit becomes a library part).

use crate::session::{obj, opt, options, Session};
use crate::Res;
use part_lib::{PartDef, PinDef};
use sch_core::{PinSide, SimKind, SimModel};
use serde_json::json;

/// The text of a vendor model, or why it cannot be used. LTspice's encrypted models are binary files that start with `<Binary File>`.
fn read_model(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]).to_ascii_lowercase();
    let controls = bytes.iter().filter(|b| **b < 9 || (**b > 13 && **b < 32)).count();
    if head.contains("<binary file>") || controls * 20 > bytes.len().max(1) {
        return Err(format!("{path} is an encrypted (binary) model: only the maker's program can read it. Look on the maker's site for a plain-text SPICE model (often a separate .cir / .lib for ngspice), or build the part from the data sheet. このモデルは暗号化されていて読めません"));
    }
    let text = sim_core::decode_spice_text(&bytes);
    let low = text.to_ascii_lowercase();
    if low.contains("encrypted file") || low.contains("$encrypted_lib") || low.contains("begin:encrypted") {
        return Err(format!("{path} is an encrypted model (cannot be read by ccad). このモデルは暗号化されていて読めません"));
    }
    Ok(text)
}

/// The `.subckt NAME nodes... [params]` block called `want` (the first one when `want` is empty): (name, nodes, the block without comment lines).
fn find_subckt(text: &str, want: &str) -> Result<(String, Vec<String>, String), String> {
    let mut found: Option<(String, Vec<String>, Vec<String>)> = None;
    let mut names = vec![];
    for line in text.lines() {
        let t = line.trim();
        let low = t.to_ascii_lowercase();
        if let Some((n, nodes, body)) = found.as_mut() {
            if t.starts_with('*') || t.is_empty() {
                continue;
            }
            body.push(t.to_string());
            if low.starts_with(".ends") {
                let _ = (n, nodes);
                break;
            }
        } else if low.starts_with(".subckt") {
            let mut words = t.split_whitespace().skip(1);
            let Some(name) = words.next() else { continue };
            names.push(name.to_string());
            if !want.is_empty() && !name.eq_ignore_ascii_case(want) {
                continue;
            }
            let nodes: Vec<String> = words.take_while(|w| !w.contains('=')).map(str::to_string).collect();
            found = Some((name.to_string(), nodes, vec![t.to_string()]));
        }
    }
    match found {
        Some((n, nodes, body)) if body.last().is_some_and(|l| l.to_ascii_lowercase().starts_with(".ends")) => Ok((n, nodes, body.join("\n"))),
        Some((n, _, _)) => Err(format!("the .subckt {n} has no .ends line")),
        None if names.is_empty() => Err("no .subckt block in the file (only .subckt models can be imported; a single .model card needs a hand-written part)".into()),
        None => Err(format!("no .subckt called '{want}' (the file has: {})", names.join(", "))),
    }
}

/// Which side of the symbol a pin goes to, from its name (inputs left, outputs right, supplies top and bottom).
fn side_for(name: &str) -> Option<PinSide> {
    let n = name.to_ascii_uppercase();
    if n.contains("VCC") || n.contains("VDD") || n.contains("V+") || n == "VP" || n == "99" || n.contains("VPOS") || n.starts_with("+V") {
        Some(PinSide::Top)
    } else if n.contains("VSS") || n.contains("VEE") || n.contains("V-") || n == "VN" || n.contains("GND") || n == "50" || n.contains("VNEG") {
        Some(PinSide::Bottom)
    } else if n.contains("OUT") || n == "Q" || n == "45" {
        Some(PinSide::Right)
    } else if n.contains("IN") || n.contains('+') || n.contains('-') {
        Some(PinSide::Left)
    } else {
        None
    }
}

impl Session {
    /// `lib` shows the folder; `lib reload` reads it again (a part file written while ccad was running becomes visible).
    pub fn lib_cmd(&mut self, args: &[String]) -> Res {
        let reload = match args.first().map(|a| a.to_ascii_lowercase()) {
            None => false,
            Some(a) if a == "reload" => true,
            Some(_) => return Err("lib [reload]".into()),
        };
        let before = self.lib.len();
        if reload {
            let (lib, errors) = part_lib::load_dir(&self.lib_dir);
            self.lib = lib;
            self.lib_errors = errors;
        }
        Ok(obj(json!({"dir": self.lib_dir.display().to_string(), "parts": self.lib.len(), "before": before, "reloaded": reload, "errors": self.lib_errors})))
    }

    /// `part import FILE [name=NAME] [subckt=SUB] [pins=IN+,IN-,OUT,V+,V-] [map=3,2,1,8,4] [package=PART]`: write a library part from a
    /// maker's `.subckt`, so that the next `add NAME` simulates with the maker's own model.
    pub fn part_import(&mut self, args: &[String]) -> Res {
        let (pos, kv) = options(args);
        let Some(file) = pos.first() else {
            return Err("part import FILE [name=NAME] [subckt=SUB] [pins=A,B,..] [map=1,2,..] [package=PART]   (pins: the symbol's pin names in the subcircuit's node order; map: their pin numbers; package: a library part whose footprint to borrow, default TL072 = SOIC-8)".into());
        };
        let text = read_model(file)?;
        let (sub, nodes, body) = find_subckt(&text, opt(&kv, "subckt").unwrap_or(""))?;
        let list = |key: &str| opt(&kv, key).map(|s| s.split(',').map(|x| x.trim().to_string()).collect::<Vec<_>>());
        let names = list("pins").unwrap_or_else(|| nodes.clone());
        let numbers = list("map").unwrap_or_else(|| (1..=nodes.len()).map(|i| i.to_string()).collect());
        if names.len() != nodes.len() || numbers.len() != nodes.len() {
            return Err(format!("the subcircuit {sub} has {} nodes ({}): pins= and map= need one entry for each, in this order", nodes.len(), nodes.join(" ")));
        }
        let name = opt(&kv, "name").map(str::to_string).unwrap_or_else(|| sub.clone());
        let donor_name = opt(&kv, "package").unwrap_or("TL072");
        let donor = self.lib.iter().find(|p| p.name.eq_ignore_ascii_case(donor_name)).cloned().ok_or_else(|| format!("package=: no library part '{donor_name}' to borrow the footprint from (see 'parts')"))?;
        let donor_footprint = donor.footprint_name.clone().unwrap_or_default();
        // the model must load in the simulator before it is offered as a part
        let mut probe = sim_core::Circuit::new();
        let ids: Vec<usize> = (0..nodes.len()).map(|i| probe.node(&format!("n{i}"))).collect();
        sim_core::add_subckt_instance(&mut probe, "X1", &sub, &body, &ids).map_err(|e| format!("ccad cannot read this model: {e}"))?;
        let mut pins = vec![];
        let mut seen = std::collections::HashSet::new();
        for (n, num) in names.iter().zip(&numbers) {
            if !seen.insert(num.clone()) {
                return Err(format!("map= lists pin {num} twice"));
            }
            pins.push(PinDef { number: num.clone(), name: n.clone(), side: side_for(n), sim_only: false });
        }
        let part = PartDef {
            name: name.clone(),
            category: opt(&kv, "category").unwrap_or("取り込み").to_string(),
            description: format!("メーカーのモデル {} (.subckt {sub}) をそのまま使う部品。取り込んだ人の手元の部品で、再配布できないことがあります。ピンの位置・パッケージは取り込み時の仮のもの", std::path::Path::new(file).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()),
            datasheet: String::new(),
            prefix: "U".into(),
            value: None,
            pins,
            package: donor.package.clone(),
            footprint_name: donor.footprint_name.clone(),
            height: None,
            sim: Some(SimModel { kind: SimKind::Subckt, element: String::new(), name: sub.clone(), text: body, pins: numbers.clone(), params: Default::default(), limits: vec!["メーカーのモデルをそのまま使っています。モデルが扱う範囲・温度条件はメーカーの資料で確かめてください。".into()] }),
            ratings: None,
            glyph: String::new(),
            body: None,
            shops: Default::default(),
            model3d: None,
        };
        let path = part_lib::save(&self.lib_dir, &part).map_err(|e| format!("cannot write the part file: {e}"))?;
        let (lib, errors) = part_lib::load_dir(&self.lib_dir);
        self.lib = lib;
        self.lib_errors = errors;
        let todo = vec![
            format!("footprint: borrowed from {} ({}): check the pads against the data sheet", donor.name, donor_footprint),
            "pin numbers: check 'map' against the data sheet's pin out (the node order of the subcircuit is not the package's pin order)".to_string(),
        ];
        Ok(obj(json!({"saved": path.display().to_string(), "name": name, "subckt": sub, "nodes": nodes, "pins": names, "numbers": numbers, "todo": todo, "hint": format!("add {name} as=U1")})))
    }
}
