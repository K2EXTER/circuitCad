//! Library part definitions: one JSON file describes a part's schematic symbol, PCB package
//! and simulation model, typically transcribed from the datasheet.

use pcb_core::footprints::Footprint;
use pcb_core::body::BodySpec;
use pcb_core::package::PackageSpec;
use sch_core::{CustomSymbol, PartRatings, PinSide, SimKind, SimModel, SymPin};
pub use sch_core::SpecRow;
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
    /// Simulation-only input (no pad): the sensor's light / field / ... stimulus, given as a voltage.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sim_only: bool,
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
    /// Height of the body above the board (mm) from the data sheet: the 3D view and the case-height check use it
    /// instead of guessing from the footprint name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default)]
    pub sim: Option<SimModel>,
    /// Data sheet ratings (absolute maximum voltage / current / power and other figures).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratings: Option<PartRatings>,
    /// Draw the part as a capacitor / resistor / inductor symbol ("capacitor", "resistor", "inductor") instead of a box.
    /// Only for two pins, one on each side.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub glyph: String,
    /// The body as the data sheet draws it (a rectangle or circle with an orientation mark): the silkscreen outline of the footprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodySpec>,
    /// Where to buy it: shop id ("akizuki", "digikey", "lcsc") -> the shop's own part number. Left out when not known.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub shops: std::collections::BTreeMap<String, String>,
    /// The pads were not read from the data sheet's land pattern drawing (an estimate): the pre-order check warns until this is removed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub land_unverified: bool,
    /// The body as a triangle mesh (from a STEP file the maker publishes): the 3D view draws it instead of a guessed box.
    /// In the footprint's frame, millimetres, z = 0 on the face the part sits on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model3d: Option<pcb_core::mesh::Mesh3d>,
    /// Gates and inputs that are alike, so their nets may trade places on the board (`ccad board swap`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap: Option<pcb_core::swap::SwapSpec>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub error: bool,
    pub message: String,
}

/// Glyphs that draw a standard body (diode, transistor ...) inside the box; the pins keep their box positions.
pub const BODY_GLYPHS: [&str; 17] = [
    "diode", "zener", "schottky", "tvs", "led", "npn", "pnp", "nmos", "pmos", "opamp", "transformer", "battery", "ldr", "photodiode", "shunt_reg", "ac3p", "crystal",
];

impl PartDef {
    pub fn footprint_name(&self) -> String {
        self.footprint_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| self.name.clone())
    }

    pub fn default_value(&self) -> String {
        self.value.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| self.name.clone())
    }

    pub fn symbol(&self) -> CustomSymbol {
        let mut sym = self.symbol_plain();
        // a bundled part without a glyph still gets its standard symbol (and the small layout that goes with it)
        if sym.glyph.is_empty() {
            sym.glyph = sym.inferred_glyph().into();
        }
        sym
    }

    fn symbol_plain(&self) -> CustomSymbol {
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
                    sim_only: p.sim_only,
                })
                .collect(),
            sim: self.sim.clone(),
            ratings: self.ratings.clone().filter(|r| !r.is_empty()),
            glyph: self.glyph.clone(),
            shops: self.shops.clone(),
        }
    }

    /// The kind of capacitor a library part is (for the tree under "コンデンサ"): 電解 / タンタル / フィルム /
    /// 積層セラミック, by the glyph, the name prefix (CE_ CT_ CF_) and the description. None for other parts.
    pub fn capacitor_family(&self) -> Option<&'static str> {
        if self.prefix != "C" {
            return None;
        }
        // the name prefix is checked first: a tantalum is polarised like an electrolytic (same glyph)
        if self.name.starts_with("CT_") {
            Some("タンタル")
        } else if self.glyph == "electrolytic" || self.name.starts_with("CE_") {
            Some("電解")
        } else if self.name.starts_with("CF_") || self.description.contains("フィルムコンデンサ") {
            Some("フィルム")
        } else if self.glyph == "capacitor" {
            Some("積層セラミック")
        } else {
            None
        }
    }

    pub fn footprint(&self) -> Result<Footprint, String> {
        let mut fp = self.package.build(&self.footprint_name())?;
        fp.height = self.height.filter(|h| h.is_finite() && *h > 0.0);
        if let Some(b) = self.body.as_ref().filter(|b| b.w > 0.0 && (b.shape == "circle" || b.h > 0.0)) {
            fp.silk = pcb_core::body::body_lines(b, &fp.pads);
            fp.body = Some(b.clone());
        }
        Ok(fp)
    }

    /// Everything that can be checked without a simulator.
    pub fn validate(&self) -> Vec<Issue> {
        let mut v = vec![];
        let mut err = |m: String| v.push(Issue { error: true, message: m });
        let mut warns = vec![];

        if self.name.trim().is_empty() {
            err("(name) 部品名が空です。データシートの型番などを入れてください (例: LM358、2N3904)".into());
        }
        if self.prefix.trim().is_empty() {
            err("(prefix) 部品番号の頭文字が空です。抵抗なら R、コンデンサなら C、IC なら U のように入れてください".into());
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

        if !self.glyph.is_empty() {
            let sides: Vec<_> = self.symbol().pins.iter().map(|p| p.side).collect();
            let chip = ["capacitor", "electrolytic", "resistor", "inductor"].contains(&self.glyph.as_str());
            if chip {
                if sides.len() != 2 || !sides.contains(&PinSide::Left) || !sides.contains(&PinSide::Right) {
                    err("glyph (コンデンサ・抵抗・コイルの記号) は、左右に 1 本ずつの 2 ピンの部品だけに使えます".into());
                }
            } else if !BODY_GLYPHS.contains(&self.glyph.as_str()) {
                err(format!("glyph「{}」は capacitor / electrolytic / resistor / inductor / {} のどれかにしてください", self.glyph, BODY_GLYPHS.join(" / ")));
            }
        }

        if let Some(b) = &self.body {
            let names: Vec<&str> = self.pins.iter().map(|p| p.number.as_str()).chain(self.pins.iter().map(|p| p.name.as_str())).collect();
            if let Err(e) = b.validate(&names) {
                err(e);
            }
        }

        match self.footprint() {
            Err(e) => err(format!("package: {e}")),
            Ok(fp) => {
                for p in self.pins.iter().filter(|p| !p.sim_only) {
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
                SimKind::Subckt => match subckt_ports(&sim.text, &sim.name) {
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

/// `(name, port count)` of the `.subckt` called `want` (else the first one).
fn subckt_ports(text: &str, want: &str) -> Option<(String, usize)> {
    let mut first = None;
    for line in text.lines().filter(|l| l.trim_start().to_ascii_lowercase().starts_with(".subckt")) {
        let mut t = line.split_whitespace().skip(1);
        let name = t.next()?.to_string();
        let ports = t.take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).count();
        if name.eq_ignore_ascii_case(want) {
            return Some((name, ports));
        }
        first.get_or_insert((name, ports));
    }
    first
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

/// The pin-trading rules of the part named `name` (the symbol's name in the schematic): the library's own, else the built-in ones of
/// the 74HC gates (a copy in the user's library from before the rules were written has none; "74HC32D" is the 74HC32 in a SOIC).
pub fn swap_for(name: &str, lib: &[PartDef]) -> Option<pcb_core::swap::SwapSpec> {
    if let Some(spec) = lib.iter().filter(|d| d.name == name).find_map(|d| d.swap.clone()) {
        return Some(spec);
    }
    let base = name.trim_end_matches(['D', 'N']);
    let pins = |g: &[&[&str]]| g.iter().map(|gate| gate.iter().map(|p| p.to_string()).collect()).collect::<Vec<Vec<String>>>();
    // (input, input, output) four times; the inverters (input, output) six times
    let three = |g: &[&[&str]]| pcb_core::swap::SwapSpec { gates: pins(g), inputs: vec![0, 1] };
    match base {
        "74HC00" | "74HC08" | "74HC32" | "74HC86" => Some(three(&[&["1", "2", "3"], &["4", "5", "6"], &["9", "10", "8"], &["12", "13", "11"]])),
        "74HC02" => Some(three(&[&["2", "3", "1"], &["5", "6", "4"], &["8", "9", "10"], &["11", "12", "13"]])),
        "74HC04" | "74HC14" => Some(pcb_core::swap::SwapSpec { gates: pins(&[&["1", "2"], &["3", "4"], &["5", "6"], &["9", "8"], &["11", "10"], &["13", "12"]]), inputs: vec![] }),
        _ => None,
    }
}

/// Parts that ship with the program. They are copied into the user's library once.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("NE555_DIP-8", include_str!("../examples/NE555_DIP-8.part.json")),
    ("USB_DEMO_TH", include_str!("../examples/USB_DEMO_TH.part.json")),
    ("C_100n_0603", include_str!("../examples/C_100n_0603.part.json")),
    ("C_10u_0805", include_str!("../examples/C_10u_0805.part.json")),
    ("CE_100u_16V", include_str!("../examples/CE_100u_16V.part.json")),
    ("CT_10u_16V", include_str!("../examples/CT_10u_16V.part.json")),
    ("CF_100n_63V", include_str!("../examples/CF_100n_63V.part.json")),
    ("L_10u_0805", include_str!("../examples/L_10u_0805.part.json")),
    ("R_10k_0603", include_str!("../examples/R_10k_0603.part.json")),
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
    ("ATmega2560-16AU_TQFP-100", include_str!("../examples/ATmega2560-16AU_TQFP-100.part.json")),
    ("CH340G_SOIC-16", include_str!("../examples/CH340G_SOIC-16.part.json")),
    ("XTAL_16MHz_3225", include_str!("../examples/XTAL_16MHz_3225.part.json")),
    ("XTAL_12MHz_3225", include_str!("../examples/XTAL_12MHz_3225.part.json")),
    ("74HC04_DIP-14", include_str!("../examples/74HC04_DIP-14.part.json")),
    ("74HC14_DIP-14", include_str!("../examples/74HC14_DIP-14.part.json")),
    ("74HC00_DIP-14", include_str!("../examples/74HC00_DIP-14.part.json")),
    ("74HC08_DIP-14", include_str!("../examples/74HC08_DIP-14.part.json")),
    ("74HC32_DIP-14", include_str!("../examples/74HC32_DIP-14.part.json")),
    ("74HC86_DIP-14", include_str!("../examples/74HC86_DIP-14.part.json")),
    ("74HC02_DIP-14", include_str!("../examples/74HC02_DIP-14.part.json")),
    ("74HC74_DIP-14", include_str!("../examples/74HC74_DIP-14.part.json")),
    ("TP4056_ESOP-8", include_str!("../examples/TP4056_ESOP-8.part.json")),
    ("TP5100_QFN-16", include_str!("../examples/TP5100_QFN-16.part.json")),
    ("IP2332_ESOP-8", include_str!("../examples/IP2332_ESOP-8.part.json")),
    ("MCP73831_SOT-23-5", include_str!("../examples/MCP73831_SOT-23-5.part.json")),
    ("1N4007_DO-41", include_str!("../examples/1N4007_DO-41.part.json")),
    ("1N4728A_DO-41", include_str!("../examples/1N4728A_DO-41.part.json")),
    ("1N5817_DO-41", include_str!("../examples/1N5817_DO-41.part.json")),
    ("74HC595_SOIC-16", include_str!("../examples/74HC595_SOIC-16.part.json")),
    ("74HC4017_DIP-16", include_str!("../examples/74HC4017_DIP-16.part.json")),
    ("AMS1117-5.0_SOT-223", include_str!("../examples/AMS1117-5.0_SOT-223.part.json")),
    ("ATtiny85_SOIC-8", include_str!("../examples/ATtiny85_SOIC-8.part.json")),
    ("BSS138_SOT-23", include_str!("../examples/BSS138_SOT-23.part.json")),
    ("SW_SAMPLE_DEMO", include_str!("../examples/SW_SAMPLE_DEMO.part.json")),
    ("IRLZ44N_TO-220", include_str!("../examples/IRLZ44N_TO-220.part.json")),
    ("LM317_TO-220", include_str!("../examples/LM317_TO-220.part.json")),
    ("LM324_SOIC-14", include_str!("../examples/LM324_SOIC-14.part.json")),
    ("LM393_SOIC-8", include_str!("../examples/LM393_SOIC-8.part.json")),
    ("ULN2803A_DIP-18", include_str!("../examples/ULN2803A_DIP-18.part.json")),
    ("LM7812_TO-220", include_str!("../examples/LM7812_TO-220.part.json")),
    ("MCP1700-3302_SOT-23", include_str!("../examples/MCP1700-3302_SOT-23.part.json")),
    ("NE5532_SOIC-8", include_str!("../examples/NE5532_SOIC-8.part.json")),
    ("PN2222A_TO-92", include_str!("../examples/PN2222A_TO-92.part.json")),
    ("S8050_SOT-23", include_str!("../examples/S8050_SOT-23.part.json")),
    ("S8550_SOT-23", include_str!("../examples/S8550_SOT-23.part.json")),
    ("SS14_SMA", include_str!("../examples/SS14_SMA.part.json")),
    ("BUCK_DEMO", include_str!("../examples/BUCK_DEMO.part.json")),
    ("AC3P_10V_50Hz", include_str!("../examples/AC3P_10V_50Hz.part.json")),
    ("CR2032", include_str!("../examples/CR2032.part.json")),
    ("AA_ALKALINE", include_str!("../examples/AA_ALKALINE.part.json")),
    ("LIION_18650", include_str!("../examples/LIION_18650.part.json")),
    ("LIION_18650_CHG", include_str!("../examples/LIION_18650_CHG.part.json")),
    ("TRANS_100V-12V_5VA", include_str!("../examples/TRANS_100V-12V_5VA.part.json")),
    ("TRANS_100V-9V_3VA", include_str!("../examples/TRANS_100V-9V_3VA.part.json")),
    ("TRANS_230V-12V_5VA", include_str!("../examples/TRANS_230V-12V_5VA.part.json")),
    ("LM7809_TO-220", include_str!("../examples/LM7809_TO-220.part.json")),
    ("TL431_SOT-23", include_str!("../examples/TL431_SOT-23.part.json")),
    ("IRF540N_TO-220", include_str!("../examples/IRF540N_TO-220.part.json")),
    ("IRFZ44N_TO-220", include_str!("../examples/IRFZ44N_TO-220.part.json")),
    ("IRF9540N_TO-220", include_str!("../examples/IRF9540N_TO-220.part.json")),
    ("TIP120_TO-220", include_str!("../examples/TIP120_TO-220.part.json")),
    ("TIP125_TO-220", include_str!("../examples/TIP125_TO-220.part.json")),
    ("BD139_TO-126", include_str!("../examples/BD139_TO-126.part.json")),
    ("BD140_TO-126", include_str!("../examples/BD140_TO-126.part.json")),
    ("1N5822_DO-201AD", include_str!("../examples/1N5822_DO-201AD.part.json")),
    ("1N5408_DO-201AD", include_str!("../examples/1N5408_DO-201AD.part.json")),
    ("UF4007_DO-41", include_str!("../examples/UF4007_DO-41.part.json")),
    ("BAT54_SOT-23", include_str!("../examples/BAT54_SOT-23.part.json")),
    ("1N4742A_DO-41", include_str!("../examples/1N4742A_DO-41.part.json")),
    ("LM35_TO-92", include_str!("../examples/LM35_TO-92.part.json")),
    ("TMP36_TO-92", include_str!("../examples/TMP36_TO-92.part.json")),
    ("NTC_10k_B3950", include_str!("../examples/NTC_10k_B3950.part.json")),
    ("PT100", include_str!("../examples/PT100.part.json")),
    ("LDR_GL5528", include_str!("../examples/LDR_GL5528.part.json")),
    ("BPW34", include_str!("../examples/BPW34.part.json")),
    ("SS49E_TO-92", include_str!("../examples/SS49E_TO-92.part.json")),
    ("ACS712-05B_SOIC-8", include_str!("../examples/ACS712-05B_SOIC-8.part.json")),
    ("INA180A1_SOT-23-5", include_str!("../examples/INA180A1_SOT-23-5.part.json")),
    ("INA180A2_SOT-23-5", include_str!("../examples/INA180A2_SOT-23-5.part.json")),
    ("INA180A3_SOT-23-5", include_str!("../examples/INA180A3_SOT-23-5.part.json")),
    ("ADS124S08_VQFN-32", include_str!("../examples/ADS124S08_VQFN-32.part.json")),
    ("ADG2712_LGA-16", include_str!("../examples/ADG2712_LGA-16.part.json")),
    ("TMUX1101_SOT-23-5", include_str!("../examples/TMUX1101_SOT-23-5.part.json")),
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

/// Key recorded in `.bundled` once a part has been offered the model notes (`sim.limits`).
fn limits_key(name: &str) -> String {
    format!("{name}+limits")
}

/// Is the file an unmodified copy of the bundled part from before it carried model notes?
/// (Equal to the bundled part in every field except the notes.)
fn is_original_without_limits(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(mut new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    let Some(sim) = new.sim.as_mut() else { return false };
    if sim.limits.is_empty() || old.sim.as_ref().is_none_or(|m| !m.limits.is_empty()) {
        return false;
    }
    sim.limits.clear();
    old == new
}

/// Key recorded in `.bundled` once a part has been offered its data sheet ratings.
fn ratings_key(name: &str) -> String {
    format!("{name}+ratings")
}

/// Is the file an unmodified copy of the bundled part from before it carried ratings?
/// (Equal to the bundled part in every field except the ratings.)
fn is_original_without_ratings(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(mut new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    if new.ratings.is_none() || old.ratings.is_some() {
        return false;
    }
    new.ratings = None;
    old == new
}

/// Key recorded in `.bundled` once a part has been offered its corrected pads.
fn pads_key(name: &str) -> String {
    format!("{name}+pads")
}

/// Is the file an unmodified copy of a bundled leaded part from when its pads had no hole (drawn as surface mount)?
/// (Equal to the bundled part in every field except the package.)
fn is_original_without_holes(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    let has_hole = |d: &PartDef| matches!(&d.package, PackageSpec::Explicit { pads, .. } if pads.iter().any(|p| p.drill.is_some()));
    has_hole(&new) && !has_hole(&old) && old == PartDef { package: old.package.clone(), height: old.height, ..new }
}

/// Key recorded in `.bundled` once a part has been offered its wire/connector footprint (not the battery holder's).
fn wire_key(name: &str) -> String {
    format!("{name}+wire")
}

/// Is the file an unmodified copy of a bundled battery from when its pads were those of the 77 mm battery holder?
/// (Equal to the bundled part in every field except the package and its name.)
fn is_original_with_holder(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    old.footprint_name.as_deref() == Some("BatteryHolder_18650") && new.footprint_name.as_deref() != old.footprint_name.as_deref() && old == PartDef { package: old.package.clone(), footprint_name: old.footprint_name.clone(), ..new }
}

/// Key recorded in `.bundled` once a part has been offered its ripple current and life.
fn ripple_key(name: &str) -> String {
    format!("{name}+ripple")
}

/// Is the file an unmodified copy of the bundled part from before it carried ripple current and life?
/// (Equal to the bundled part in every field except the ratings.)
fn is_original_without_ripple(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    let has = |d: &PartDef| d.ratings.as_ref().is_some_and(|r| r.ripple.is_some() || r.life.is_some());
    // the old ratings keep only what they had: the voltage
    has(&new) && !has(&old) && old == PartDef { ratings: old.ratings.clone(), ..new }
}

/// Key recorded in `.bundled` once a part has been offered its shop part numbers.
fn shops_key(name: &str) -> String {
    format!("{name}+shops")
}

/// Is the file an unmodified copy of the bundled part from before it carried shop part numbers?
/// (Equal to the bundled part in every field except the shops.)
fn is_original_without_shops(file: &Path, bundled: &str) -> bool {
    let Some(old) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<PartDef>(&t).ok()) else { return false };
    let Ok(new) = serde_json::from_str::<PartDef>(bundled) else { return false };
    !new.shops.is_empty() && old.shops.is_empty() && old == PartDef { shops: Default::default(), ..new }
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
    // parts that gained their model notes after the first release: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = limits_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_without_limits(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    // parts that gained their data sheet ratings after the first release: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = ratings_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_without_ratings(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    // leaded parts whose pads first came without holes: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = pads_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_without_holes(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    // 18650 cells that came with the battery holder's pads: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = wire_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_with_holder(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    // capacitors that gained their ripple current and life: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = ripple_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_without_ripple(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    // parts that gained their shop part numbers: replaced once while unmodified
    for (name, text) in EXAMPLES {
        let key = shops_key(name);
        if done.contains(&key) {
            continue;
        }
        let file = dir.join(format!("{name}{FILE_SUFFIX}"));
        if file.exists() && is_original_without_shops(&file, text) {
            std::fs::write(&file, text)?;
        }
        done.insert(key);
    }
    if done.len() != before || !marker.exists() {
        std::fs::write(&marker, done.into_iter().collect::<Vec<_>>().join("\n"))?;
    }
    Ok(())
}

pub mod diode;
pub mod kicad;
pub mod vendorzip;
pub mod passive;
pub mod templates;
pub mod thru_hole;

#[cfg(test)]
mod tests;
