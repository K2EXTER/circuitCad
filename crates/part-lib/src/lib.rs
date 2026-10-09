//! Library part definitions: one JSON file describes a part's schematic symbol, PCB package
//! and simulation model, typically transcribed from the datasheet.

use pcb_core::footprints::Footprint;
use pcb_core::package::PackageSpec;
use sch_core::{CustomSymbol, PinSide, SimKind, SimModel, SymPin};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_SUFFIX: &str = ".part.json";

fn default_prefix() -> String {
    "U".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PinDef {
    /// Datasheet pin number (or letter): must match a pad of the package.
    pub number: String,
    pub name: String,
    /// Omitted: the first half goes left, the rest right.
    #[serde(default)]
    pub side: Option<PinSide>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartDef {
    pub name: String,
    /// Group shown in the library list ("MOSFET", "オペアンプ", ...).
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub datasheet: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    /// Value shown in the schematic; defaults to `name`.
    #[serde(default)]
    pub value: Option<String>,
    pub pins: Vec<PinDef>,
    pub package: PackageSpec,
    /// Footprint name on the PCB; defaults to `name`.
    #[serde(default)]
    pub footprint_name: Option<String>,
    #[serde(default)]
    pub sim: Option<SimModel>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub error: bool,
    pub message: String,
}

impl PartDef {
    pub fn footprint_name(&self) -> String {
        self.footprint_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| self.name.clone())
    }

    pub fn default_value(&self) -> String {
        self.value.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| self.name.clone())
    }

    pub fn symbol(&self) -> CustomSymbol {
        let half = self.pins.len().div_ceil(2);
        CustomSymbol {
            name: self.name.clone(),
            prefix: self.prefix.clone(),
            pins: self
                .pins
                .iter()
                .enumerate()
                .map(|(i, p)| SymPin {
                    number: p.number.clone(),
                    name: p.name.clone(),
                    side: p.side.unwrap_or(if i < half { PinSide::Left } else { PinSide::Right }),
                })
                .collect(),
            sim: self.sim.clone(),
        }
    }

    pub fn footprint(&self) -> Result<Footprint, String> {
        self.package.build(&self.footprint_name())
    }

    /// Everything that can be checked without a simulator.
    pub fn validate(&self) -> Vec<Issue> {
        let mut v = vec![];
        let mut err = |m: String| v.push(Issue { error: true, message: m });
        let mut warns = vec![];

        if self.name.trim().is_empty() {
            err("name が空です".into());
        }
        if self.prefix.trim().is_empty() {
            err("prefix が空です".into());
        }
        if self.pins.is_empty() {
            err("ピンがありません".into());
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.pins {
            if p.number.trim().is_empty() {
                err("番号が空のピンがあります".into());
            } else if !seen.insert(p.number.as_str()) {
                err(format!("ピン番号 {} が重複しています", p.number));
            }
        }

        match self.footprint() {
            Err(e) => err(format!("package: {e}")),
            Ok(fp) => {
                for p in &self.pins {
                    if !fp.pads.iter().any(|pad| pad.pin == p.number) {
                        err(format!("ピン {} ({}) に対応するパッドがパッケージにありません", p.number, p.name));
                    }
                }
                for pad in &fp.pads {
                    if !self.pins.iter().any(|p| p.number == pad.pin) {
                        warns.push(format!("パッド {} は記号のピンに対応していません (サーマルパッド等なら問題ありません)", pad.pin));
                    }
                }
            }
        }

        if let Some(sim) = &self.sim {
            for p in &sim.pins {
                if !self.pins.iter().any(|d| &d.number == p) {
                    err(format!("sim.pins の {p} が記号のピンにありません"));
                }
            }
            let lower = sim.text.to_ascii_lowercase();
            match sim.kind {
                SimKind::Model => {
                    if sim.element.trim().is_empty() {
                        err("sim.element (D, Q, M, J など) が必要です".into());
                    }
                    let ok = lower.lines().any(|l| {
                        let mut t = l.split_whitespace();
                        t.next() == Some(".model") && t.next().is_some_and(|n| n.eq_ignore_ascii_case(&sim.name))
                    });
                    if !ok {
                        err(format!("sim.text に .model {} がありません", sim.name));
                    }
                }
                SimKind::Subckt => match subckt_ports(&sim.text) {
                    None => err("sim.text に .subckt 行がありません".into()),
                    Some((name, ports)) => {
                        if !name.eq_ignore_ascii_case(&sim.name) {
                            err(format!("sim.name {} と .subckt 名 {name} が違います", sim.name));
                        }
                        if ports != sim.pins.len() {
                            err(format!("サブサーキットのポート数 {ports} と sim.pins の数 {} が違います", sim.pins.len()));
                        }
                    }
                },
            }
        }
        v.extend(warns.into_iter().map(|m| Issue { error: false, message: m }));
        v
    }

    pub fn has_errors(&self) -> bool {
        self.validate().iter().any(|i| i.error)
    }
}

/// `(name, port count)` of the first `.subckt` line.
fn subckt_ports(text: &str) -> Option<(String, usize)> {
    let line = text.lines().find(|l| l.trim_start().to_ascii_lowercase().starts_with(".subckt"))?;
    let mut t = line.split_whitespace().skip(1);
    let name = t.next()?.to_string();
    let ports = t.take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).count();
    Some((name, ports))
}

// ---------------------------------------------------------------- files

/// `%APPDATA%\circuitCad\library` (or `~/.circuitcad/library`).
pub fn default_dir() -> PathBuf {
    if let Some(a) = std::env::var_os("APPDATA") {
        return PathBuf::from(a).join("circuitCad").join("library");
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join(".circuitcad").join("library")
}

pub fn file_name(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || "-_.+".contains(c) { c } else { '_' }).collect();
    format!("{safe}{FILE_SUFFIX}")
}

/// Parts found in `dir` plus one message per unreadable file.
pub fn load_dir(dir: &Path) -> (Vec<PartDef>, Vec<String>) {
    let mut parts = vec![];
    let mut errors = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return (parts, errors) };
    let mut files: Vec<PathBuf> =
        rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(FILE_SUFFIX)).collect();
    files.sort();
    for f in files {
        let label = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<PartDef>(&s).map_err(|e| e.to_string())) {
            Ok(p) => parts.push(p),
            Err(e) => errors.push(format!("{label}: {e}")),
        }
    }
    (parts, errors)
}

pub fn save(dir: &Path, part: &PartDef) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(&part.name));
    std::fs::write(&path, serde_json::to_string_pretty(part).map_err(std::io::Error::other)?)?;
    Ok(path)
}

/// Parts that ship with the program. They are copied into the user's library once.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("NE555_DIP-8", include_str!("../examples/NE555_DIP-8.part.json")),
    ("1N4148", include_str!("../examples/1N4148.part.json")),
    ("LM358_SOIC-8", include_str!("../examples/LM358_SOIC-8.part.json")),
    ("TL072_SOIC-8", include_str!("../examples/TL072_SOIC-8.part.json")),
    ("MCP6002_SOIC-8", include_str!("../examples/MCP6002_SOIC-8.part.json")),
    ("MCP6001_SOT-23-5", include_str!("../examples/MCP6001_SOT-23-5.part.json")),
    ("2N7000_TO-92", include_str!("../examples/2N7000_TO-92.part.json")),
    ("AO3400A_SOT-23", include_str!("../examples/AO3400A_SOT-23.part.json")),
    ("AO3401A_SOT-23", include_str!("../examples/AO3401A_SOT-23.part.json")),
    ("2N3904_TO-92", include_str!("../examples/2N3904_TO-92.part.json")),
    ("2N3906_TO-92", include_str!("../examples/2N3906_TO-92.part.json")),
    ("BC547_TO-92", include_str!("../examples/BC547_TO-92.part.json")),
    ("1N4001_DO-41", include_str!("../examples/1N4001_DO-41.part.json")),
    ("1N5819_DO-41", include_str!("../examples/1N5819_DO-41.part.json")),
    ("1N4733A_DO-41", include_str!("../examples/1N4733A_DO-41.part.json")),
    ("LM7805_TO-220", include_str!("../examples/LM7805_TO-220.part.json")),
    ("L78L05_TO-92", include_str!("../examples/L78L05_TO-92.part.json")),
    ("AMS1117-3.3_SOT-223", include_str!("../examples/AMS1117-3.3_SOT-223.part.json")),
    ("ATmega328P-AU_TQFP-32", include_str!("../examples/ATmega328P-AU_TQFP-32.part.json")),
    ("74HC04_DIP-14", include_str!("../examples/74HC04_DIP-14.part.json")),
    ("74HC14_DIP-14", include_str!("../examples/74HC14_DIP-14.part.json")),
    ("74HC00_DIP-14", include_str!("../examples/74HC00_DIP-14.part.json")),
    ("74HC08_DIP-14", include_str!("../examples/74HC08_DIP-14.part.json")),
    ("74HC32_DIP-14", include_str!("../examples/74HC32_DIP-14.part.json")),
    ("74HC86_DIP-14", include_str!("../examples/74HC86_DIP-14.part.json")),
    ("74HC02_DIP-14", include_str!("../examples/74HC02_DIP-14.part.json")),
    ("74HC74_DIP-14", include_str!("../examples/74HC74_DIP-14.part.json")),
];

/// Older bundled versions that are replaced when the user's copy is still unmodified:
/// (example name, the `sim.text` of the old version).
const LEGACY: &[(&str, &str)] = &[(
    "LM358_SOIC-8",
    ".subckt LM358 inp inn out vp vn\nRin inp inn 1Meg\nEop m 0 inp inn 100k\nRp m n 1k\nCp n 0 16u\nEo o 0 n 0 1\nRo o out 50\n.ends LM358",
)];

/// Is the file on disk an unmodified copy of an older bundled version? That is the case when it
/// carries the known old model text, or when it equals the bundled part except for the category.
fn is_unmodified_legacy(file: &Path, example: &str, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    if let Some((_, text)) = LEGACY.iter().find(|(n, _)| *n == example)
        && old.sim.as_ref().is_some_and(|m| m.text == *text) {
            return true;
        }
    match serde_json::from_str::<PartDef>(bundled) {
        Ok(new) => old.category.is_empty() && old == PartDef { category: String::new(), ..new },
        Err(_) => false,
    }
}

/// Bundled parts that gained a simulation model after their first release. An installed copy
/// that is still the model-less original (same pins and package, no `sim`) is replaced once;
/// the key is recorded in `.bundled` so that a later edit or deletion is respected.
const MODEL_UPGRADES: &[&str] = &["NE555_DIP-8"];

fn upgrade_key(name: &str) -> String {
    format!("{name}+model")
}

fn is_modelless_original(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    match serde_json::from_str::<PartDef>(bundled) {
        Ok(new) => old.sim.is_none() && old == PartDef { description: old.description.clone(), sim: None, ..new },
        Err(_) => false,
    }
}

/// Copy bundled parts the user has not been given yet. A `.bundled` file in the library records
/// what was installed, so parts the user deleted stay deleted and edited parts are never touched.
/// (Exception: an old bundled version that was never modified is upgraded, e.g. to gain a category.)
pub fn install_examples(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let marker = dir.join(".bundled");
    let mut done: std::collections::BTreeSet<String> =
        std::fs::read_to_string(&marker).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let before = done.len();
    for (name, text) in EXAMPLES {
        if done.contains(*name) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if !file.exists() || is_unmodified_legacy(&file, name, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(name.to_string());
    }
    for name in MODEL_UPGRADES {
        let key = upgrade_key(name);
        if done.contains(&key) {
            continue;
        }
        if let Some((_, text)) = EXAMPLES.iter().find(|(n, _)| n == name) {
            let file = dir.join(format!("{name}{FILE_SUFFIX}"));
            if file.exists() && is_modelless_original(&file, text) {
                std::fs::write(&file, text)?;
            }
        }
        done.insert(key);
    }
    if done.len() != before || !marker.exists() {
        std::fs::write(&marker, done.into_iter().collect::<Vec<_>>().join("\n"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
