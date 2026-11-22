//! Pin definitions for the firmware, from the schematic (candidates W1, W2): which pin of the microcontroller board each signal is
//! on, written as a C header for Arduino (`#define PIN_LED 13`), and a firmware file read back and checked against the schematic
//! (`LED_PIN` is 13 there, the schematic has it on D12). The schematic is what the board is built from: the header is made from it,
//! never the other way round.
//!
//! The microcontroller's pins are found by their names: a part's pin named D13, A0 or GPIO2 (an Arduino Nano, a Raspberry Pi Pico
//! drawn as a module), or a pin header's pin named so (`Component::pin_names`: the end of a cable to an Arduino Mega).
//!
//! The same signals also go out as a Zephyr devicetree overlay (`zephyr_overlay`), and an FPGA's (or any chip's) signals by its
//! package pins as a constraint file for its tools (`constraints_text`: .pcf for iCE40, .xdc for Xilinx, .qsf for Intel, .lpf for
//! Lattice ECP5). What was written last and the firmware file checked are kept with the schematic (`FirmwareLink`), so the ERC can
//! say when the schematic's pins have changed since (a pin traded on the board comes back into the schematic) and when the
//! firmware drives another pin than the schematic has.

use crate::{extract, Kind, Schematic};
use serde::{Deserialize, Serialize};

/// One signal on a pin of the microcontroller board.
#[derive(Clone, Debug, PartialEq)]
pub struct PinDef {
    /// The constant's name: `PIN_` and the net's name (or, for a net named only after the pin, the first other part pin on it).
    pub name: String,
    /// The board's own name of the pin (D13, A0, GPIO2) and the value written for it (13, A0, 2).
    pub pin: String,
    pub value: String,
    /// Where it was found (the part and its pin), the net, and what else is on the net.
    pub at: String,
    pub net: String,
    pub others: Vec<String>,
}

/// The value of a microcontroller board's pin name: D13 → 13, A0 → A0 (Arduino's own constant), GPIO2 / GP2 → 2; None for a pin
/// that is not an I/O (5V, GND, RST, AREF).
pub fn pin_value(name: &str) -> Option<String> {
    let up = name.trim().to_ascii_uppercase();
    let num = |rest: &str| rest.parse::<u32>().ok();
    for (prefix, keep) in [("GPIO", false), ("GP", false), ("D", false), ("IO", false), ("A", true)] {
        if let Some(rest) = up.strip_prefix(prefix)
            && let Some(n) = num(rest)
        {
            return Some(if keep { format!("A{n}") } else { n.to_string() });
        }
    }
    None
}

/// A C identifier from a net's name: letters, digits and `_` (others become `_`), upper case, not starting with a digit.
fn ident(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.starts_with(|c: char| c.is_ascii_digit()) { format!("_{out}") } else { out }
}

/// The signals on the microcontroller board's pins, by the pin's value (D2 before D10, then A0...).
pub fn pin_defs(s: &Schematic) -> Vec<PinDef> {
    let flat = s.flat();
    let nets = extract(&flat);
    let mut out: Vec<PinDef> = vec![];
    for c in &flat.components {
        // each pin with an I/O name: a part's own pin names, or a header's names for its pins
        let named: Vec<(String, String)> = match (&c.custom, c.kind) {
            (Some(sym), _) => sym.pins.iter().filter(|p| !p.sim_only).map(|p| (p.number.clone(), p.name.clone())).collect(),
            (None, Kind::Header(_)) => c.pin_names.iter().enumerate().map(|(k, n)| ((k + 1).to_string(), n.clone())).collect(),
            _ => continue,
        };
        for (number, name) in named {
            let Some(value) = pin_value(&name) else { continue };
            let Some(net) = nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == c.refdes && p.pin == number)) else { continue };
            let others: Vec<String> = net.pins.iter().filter(|p| p.refdes != c.refdes).map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
            if others.is_empty() {
                continue;
            }
            // a net named after the pin itself (D22, or an unnamed one) says nothing: the first other part's pin names it
            let plain = net.name.starts_with("N$") || pin_value(&net.name).is_some_and(|v| v == value) || net.name.eq_ignore_ascii_case(&name);
            let base = if plain { others[0].replace('.', "_") } else { net.name.clone() };
            out.push(PinDef { name: format!("PIN_{}", ident(&base)), pin: name.clone(), value, at: format!("{}.{number}", c.refdes), net: net.name.clone(), others });
        }
    }
    // the same constant twice (two pins on one net): the second keeps its pin in its name
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for d in &mut out {
        let k = seen.entry(d.name.clone()).or_insert(0);
        *k += 1;
        if *k > 1 {
            d.name = format!("{}_{}", d.name, ident(&d.pin));
        }
    }
    let key = |d: &PinDef| (d.value.starts_with('A'), d.value.trim_start_matches('A').parse::<u32>().unwrap_or(u32::MAX), d.name.clone());
    out.sort_by_key(key);
    out
}

/// The C header for Arduino (and C/C++ firmware in general): one `#define` a signal, its pin and what it goes to in a comment.
pub fn header_text(s: &Schematic, title: &str) -> String {
    let defs = pin_defs(s);
    let width = defs.iter().map(|d| d.name.len()).max().unwrap_or(0);
    let mut out = format!("// {title}: pin definitions written by circuitCad from the schematic.\n// Write it again from the schematic after changing it; edit the schematic, not this file.\n#pragma once\n\n");
    for d in &defs {
        out.push_str(&format!("#define {:width$} {:<4} // {} ({}): {}\n", d.name, d.value, d.pin, d.at, d.others.join(", "), width = width));
    }
    out
}

/// The property of a signal in a devicetree: `PIN_LED_RED` → `led-red-gpios`.
fn dt_name(d: &PinDef) -> String {
    format!("{}-gpios", d.name.trim_start_matches("PIN_").to_ascii_lowercase().replace('_', "-"))
}

/// The signals as a Zephyr devicetree overlay: a `zephyr,user` node with one `…-gpios` property a signal. D0〜D15 and A0〜A5 go
/// through the board's Arduino header (`&arduino_header`: A0〜A5 are 0〜5, D0〜D15 are 6〜21); a GPIO number (GPIO2, GP2) and a
/// pin past the Arduino header (an Arduino Mega's D22) to the first GPIO controller (`&gpio0`), marked to be checked against the
/// board's own devicetree.
pub fn zephyr_overlay(s: &Schematic, title: &str) -> String {
    let defs = pin_defs(s);
    let mut out = format!("/* {title}: pin definitions written by circuitCad from the schematic.\n * Write it again from the schematic after changing it; edit the schematic, not this file.\n * Read them in the firmware with GPIO_DT_SPEC_GET(DT_PATH(zephyr_user), led_gpios). */\n\n#include <zephyr/dt-bindings/gpio/gpio.h>\n\n/ {{\n\tzephyr,user {{\n");
    for d in &defs {
        let up = d.pin.to_ascii_uppercase();
        let n: u32 = d.value.trim_start_matches('A').parse().unwrap_or(0);
        let (target, check) = if up.starts_with('A') && n <= 5 {
            (format!("&arduino_header {n}"), "")
        } else if up.starts_with('D') && n <= 15 {
            (format!("&arduino_header {}", n + 6), "")
        } else {
            (format!("&gpio0 {n}"), " (check the GPIO controller in the board's devicetree)")
        };
        out.push_str(&format!("\t\t{} = <{target} GPIO_ACTIVE_HIGH>;\t/* {} ({}): {}{check} */\n", dt_name(d), d.pin, d.at, d.others.join(", ")));
    }
    out.push_str("\t};\n};\n");
    out
}

/// A constraint file's format: the FPGA tools that read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraints {
    /// iCE40 (nextpnr, icestorm): `set_io NAME PIN`.
    Pcf,
    /// Xilinx (Vivado): `set_property -dict { PACKAGE_PIN PIN IOSTANDARD LVCMOS33 } [get_ports { NAME }]`.
    Xdc,
    /// Intel (Quartus): `set_location_assignment PIN_PIN -to NAME`.
    Qsf,
    /// Lattice ECP5 (nextpnr, Diamond): `LOCATE COMP "NAME" SITE "PIN";`.
    Lpf,
}

impl Constraints {
    pub const ALL: [Constraints; 4] = [Constraints::Pcf, Constraints::Xdc, Constraints::Qsf, Constraints::Lpf];

    /// The file's extension, which also chooses the format.
    pub fn ext(self) -> &'static str {
        match self {
            Constraints::Pcf => "pcf",
            Constraints::Xdc => "xdc",
            Constraints::Qsf => "qsf",
            Constraints::Lpf => "lpf",
        }
    }

    pub fn from_ext(ext: &str) -> Option<Constraints> {
        Constraints::ALL.into_iter().find(|c| c.ext().eq_ignore_ascii_case(ext))
    }

    /// Whose tools read it, for the screen.
    pub fn tools(self) -> &'static str {
        match self {
            Constraints::Pcf => "iCE40 (nextpnr・icestorm)",
            Constraints::Xdc => "Xilinx (Vivado)",
            Constraints::Qsf => "Intel (Quartus)",
            Constraints::Lpf => "Lattice ECP5 (nextpnr・Diamond)",
        }
    }
}

/// A pin of a chip that is a supply, ground or not connected inside by its name (VCC, VCCIO_1, VDD, GND, VSS, NC): not a signal.
fn supply_pin(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["VCC", "VDD", "VSS", "GND", "AGND", "DGND", "VPP", "VREF"].iter().any(|p| up.starts_with(p)) || up == "NC" || up == "DNC"
}

/// The signals on a chip's pins by its package pins (the pin numbers: 35, A10): (port name, package pin, net), the port named after
/// the net (lower case, as an HDL's port), or after the first other part's pin for a net without a name of its own. Supply and
/// ground pins and pins on nothing else are left out.
pub fn chip_signals(s: &Schematic, part: &str) -> Result<Vec<(String, String, String)>, String> {
    let flat = s.flat();
    let c = flat.components.iter().find(|c| c.refdes == part).ok_or_else(|| format!("{part} が回路図にありません"))?;
    // the pins by number and name: a library part's own, else the part's pins with a header's names (`pin_names`) where given
    let pins: Vec<(String, String)> = match &c.custom {
        Some(sym) => sym.pins.iter().filter(|p| !p.sim_only).map(|p| (p.number.clone(), p.name.clone())).collect(),
        None => c.pins().into_iter().enumerate().map(|(k, (n, _))| (n.clone(), c.pin_names.get(k).cloned().unwrap_or(n))).collect(),
    };
    let nets = extract(&flat);
    let mut out: Vec<(String, String, String)> = vec![];
    for (number, _) in pins.iter().filter(|p| !supply_pin(&p.1) && !p.1.eq_ignore_ascii_case("5V") && !p.1.eq_ignore_ascii_case("3V3")) {
        let Some(net) = nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == part && q.pin == *number)) else { continue };
        if crate::breadboard::is_ground_name(&net.name) {
            continue;
        }
        let others: Vec<String> = net.pins.iter().filter(|q| q.refdes != part).map(|q| format!("{}_{}", q.refdes, q.pin)).collect();
        if others.is_empty() {
            continue;
        }
        let base = if net.name.starts_with("N$") { others[0].clone() } else { net.name.clone() };
        let mut port = ident(&base).to_ascii_lowercase();
        if out.iter().any(|o| o.0 == port) {
            port = format!("{port}_{}", ident(number).to_ascii_lowercase());
        }
        out.push((port, number.clone(), net.name.clone()));
    }
    if out.is_empty() {
        return Err(format!("{part} の信号のピンがありません (電源・GND 以外のピンが、ほかの部品につながっていません)"));
    }
    Ok(out)
}

/// A chip's signals as a constraint file for its FPGA tools.
pub fn constraints_text(s: &Schematic, part: &str, format: Constraints, title: &str) -> Result<String, String> {
    let sig = chip_signals(s, part)?;
    let mut out = format!("# {title}: pin constraints of {part} written by circuitCad from the schematic ({}).\n# Write it again from the schematic after changing it; edit the schematic, not this file.\n", format.tools());
    if format == Constraints::Xdc || format == Constraints::Lpf {
        out.push_str("# The I/O standard is LVCMOS33 (3.3 V): change it here when a bank runs at another voltage.\n");
    }
    for (port, pin, net) in &sig {
        let line = match format {
            Constraints::Pcf => format!("set_io {port} {pin}"),
            Constraints::Xdc => format!("set_property -dict {{ PACKAGE_PIN {pin} IOSTANDARD LVCMOS33 }} [get_ports {{ {port} }}]"),
            Constraints::Qsf => format!("set_location_assignment {} -to {port}", if pin.to_ascii_uppercase().starts_with("PIN_") { pin.clone() } else { format!("PIN_{pin}") }),
            Constraints::Lpf => format!("LOCATE COMP \"{port}\" SITE \"{pin}\";\nIOBUF PORT \"{port}\" IO_TYPE=LVCMOS33;"),
        };
        out.push_str(&format!("{line}  # {net}\n"));
    }
    Ok(out)
}

/// What kind of pin file: a C header, a Zephyr overlay, or a chip's constraints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinFile {
    Header,
    Zephyr,
    Fpga { part: String, format: Constraints },
}

impl PinFile {
    /// By the file's extension (.h, .overlay or .dts, .pcf/.xdc/.qsf/.lpf with the part).
    pub fn of(path: &str, part: Option<&str>) -> Result<PinFile, String> {
        let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "h" | "hpp" => Ok(PinFile::Header),
            "overlay" | "dts" | "dtsi" => Ok(PinFile::Zephyr),
            e => match (Constraints::from_ext(e), part) {
                (Some(format), Some(p)) => Ok(PinFile::Fpga { part: p.to_string(), format }),
                (Some(_), None) => Err(format!("{path}: FPGA の制約ファイルは、どの部品のピンかを part=U3 のように付けます")),
                _ => Err(format!("{path}: 拡張子で形式を決めます (.h・.overlay・.pcf・.xdc・.qsf・.lpf)")),
            },
        }
    }

    /// The file's text.
    pub fn text(&self, s: &Schematic, title: &str) -> Result<String, String> {
        match self {
            PinFile::Header => Ok(header_text(s, title)),
            PinFile::Zephyr => Ok(zephyr_overlay(s, title)),
            PinFile::Fpga { part, format } => constraints_text(s, part, *format, title),
        }
    }
}

/// The pin file written last and the firmware file checked, kept with the schematic: what the ERC looks at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FirmwareLink {
    /// The firmware file checked last (as chosen: absolute, or relative to the schematic's folder).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub firmware: String,
    /// The pin file written last: its path, the part of a constraint file (empty otherwise) and the pins it was written with.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub written: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub part: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pins: String,
}

impl FirmwareLink {
    pub fn is_empty(&self) -> bool {
        self.firmware.is_empty() && self.written.is_empty()
    }
}

/// The pins a file of `kind` stands for, one line each (what changes when a signal moves to another pin); the title and comments
/// left out.
pub fn pins_of(s: &Schematic, kind: &PinFile) -> String {
    match kind {
        PinFile::Fpga { part, .. } => chip_signals(s, part).map(|v| v.iter().map(|(p, pin, _)| format!("{p}={pin}")).collect::<Vec<_>>().join(";")).unwrap_or_default(),
        _ => pin_defs(s).iter().map(|d| format!("{}={}", d.name, d.value)).collect::<Vec<_>>().join(";"),
    }
}

/// Records that `path` was written from the schematic (for the ERC's "書き出し直す" note).
pub fn mark_written(s: &mut Schematic, path: &str, kind: &PinFile) {
    s.firmware_link.written = path.to_string();
    s.firmware_link.part = match kind {
        PinFile::Fpga { part, .. } => part.clone(),
        _ => String::new(),
    };
    s.firmware_link.pins = pins_of(s, kind);
}

/// The ERC's findings about the firmware: the pin file written last no longer matches the schematic's pins (a pin traded on the
/// board, a wire moved), and the firmware file checked last drives another pin than the schematic has. `dir` is the schematic's
/// folder, for a firmware path written relative to it.
pub fn firmware_problems(s: &Schematic, dir: Option<&std::path::Path>) -> Vec<crate::Problem> {
    let link = &s.firmware_link;
    let mut out = vec![];
    if !link.written.is_empty()
        && let Ok(kind) = PinFile::of(&link.written, Some(&link.part).filter(|p| !p.is_empty()).map(String::as_str))
        && pins_of(s, &kind) != link.pins
    {
        out.push(crate::Problem::new(
            crate::Severity::Warning,
            format!("ピン定義を書き出したあとで、回路図のピンが変わりました ({})", link.written),
            None,
            "基板でピンを入れ替えた・配線を直したなどで、信号のピンが書き出したときと違います。「ピンとファーム…」から書き出し直し、ファームをビルドし直します",
        ));
    }
    if !link.firmware.is_empty() {
        let p = std::path::Path::new(&link.firmware);
        let path = if p.is_relative() { dir.map(|d| d.join(p)).unwrap_or_else(|| p.to_path_buf()) } else { p.to_path_buf() };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                for f in check_firmware(s, &text).into_iter().filter(|f| f.wrong) {
                    let hint = format!("{} ({}): {}", link.firmware, f.line, f.hint);
                    out.push(crate::Problem::new(crate::Severity::Warning, format!("ファームのピンが回路図と違います: {}", f.message), None, &hint));
                }
            }
            Err(_) => out.push(crate::Problem::new(
                crate::Severity::Info,
                format!("照合するファームのファイルが見つかりません ({})", link.firmware),
                None,
                "ファイルを動かしたか消したときは、「ピンとファーム…」の「ファームのファイルと照合…」で選び直します",
            )),
        }
    }
    out
}

/// A constant read from a firmware file: its name, value and line (1 from the top).
#[derive(Clone, Debug, PartialEq)]
pub struct FirmwarePin {
    pub name: String,
    pub value: String,
    pub line: usize,
}

/// The pin constants of a firmware file: `#define NAME VALUE`, and `const int NAME = VALUE;` (also `const byte`, `const uint8_t`,
/// `static const`, `constexpr`); the value a number or A0..A15 (others are left out: not a pin).
pub fn read_firmware(text: &str) -> Vec<FirmwarePin> {
    let mut out = vec![];
    for (k, raw) in text.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        let words: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == '=' || c == ';').filter(|w| !w.is_empty()).collect();
        let (name, value) = if words.first() == Some(&"#define") && words.len() >= 3 {
            (words[1], words[2])
        } else if line.contains('=') && words.iter().any(|w| matches!(*w, "const" | "constexpr")) && words.len() >= 3 {
            (words[words.len() - 2], words[words.len() - 1])
        } else {
            continue;
        };
        let value = value.trim_matches(|c| c == '(' || c == ')');
        let ok = value.parse::<u32>().is_ok() || pin_value(value).is_some_and(|v| v.starts_with('A') && v == value.to_ascii_uppercase());
        if ok && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(FirmwarePin { name: name.to_string(), value: value.to_ascii_uppercase(), line: k + 1 });
        }
    }
    out
}

/// A difference between the firmware and the schematic: what, and what to do.
#[derive(Clone, Debug, PartialEq)]
pub struct PinFinding {
    pub message: String,
    pub hint: String,
    /// The firmware's line (0: not about a line, a signal of the schematic the firmware does not name).
    pub line: usize,
    /// A real difference (the firmware drives another pin than the schematic has); false: only worth a look.
    pub wrong: bool,
}

/// The firmware's pin constants checked against the schematic: a constant whose name is a signal of the schematic (as the header
/// names it, or without `PIN_`, or the net's own name, any case) on another pin is wrong; a signal the firmware does not name, and
/// a constant on a pin the schematic leaves unused, are worth a look.
pub fn check_firmware(s: &Schematic, text: &str) -> Vec<PinFinding> {
    let defs = pin_defs(s);
    let fw = read_firmware(text);
    let norm = |n: &str| ident(n).trim_start_matches("PIN_").trim_end_matches("_PIN").to_string();
    let mut out = vec![];
    let mut named: Vec<bool> = vec![false; defs.len()];
    for f in &fw {
        let key = norm(&f.name);
        let hit = defs.iter().position(|d| norm(&d.name) == key || norm(&d.net) == key);
        match hit {
            Some(i) => {
                named[i] = true;
                let d = &defs[i];
                if d.value != f.value {
                    out.push(PinFinding {
                        message: format!("{} は {} 番ですが、回路図では {} ({}) につながっています", f.name, f.value, d.pin, d.at),
                        hint: format!("ファームの {} 行目を {} にするか、回路図の配線を直します (つながる先: {})", f.line, d.value, d.others.join("、")),
                        line: f.line,
                        wrong: true,
                    });
                }
            }
            None => {
                if let Some(d) = defs.iter().find(|d| d.value == f.value) {
                    out.push(PinFinding {
                        message: format!("{} ({} 番) は、回路図の名前に無い定数です。回路図ではその番のピン {} は「{}」です", f.name, f.value, d.pin, d.name),
                        hint: "同じ信号なら、回路図のネットの名前か定数の名前をそろえると、照合できます".to_string(),
                        line: f.line,
                        wrong: false,
                    });
                }
            }
        }
    }
    for (i, d) in defs.iter().enumerate().filter(|(i, _)| !named[*i]) {
        let _ = i;
        out.push(PinFinding {
            message: format!("回路図の {} ({} 番、{}) を、ファームは名前で使っていません", d.name, d.value, d.others.join("、")),
            hint: "番号をじかに書いているか、まだ使っていない信号です。書き出したピン定義を #include すると、名前で使えます".to_string(),
            line: 0,
            wrong: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_pin_name_gives_its_value() {
        for (n, v) in [("D13", Some("13")), ("d2", Some("2")), ("A0", Some("A0")), ("GPIO15", Some("15")), ("GP2", Some("2")), ("5V", None), ("GND", None), ("RST", None), ("AREF", None), ("D", None)] {
            assert_eq!(pin_value(n).as_deref(), v, "{n}");
        }
    }

    /// The Nano's LED: one definition on the pin its net is on, written to the header; the firmware's constant on another pin
    /// is found with the line to fix.
    #[test]
    fn the_nanos_led_pin_is_written_and_a_firmware_on_another_pin_is_caught() {
        let s = load("arduino_blink.schjson");
        let defs = pin_defs(&s);
        assert_eq!(defs.len(), 1, "{defs:?}");
        let d = &defs[0];
        let h = header_text(&s, "arduino_blink");
        assert!(h.contains(&format!("#define {}", d.name)) && h.contains("#pragma once"), "{h}");
        let right = format!("const int {} = {};\nvoid setup() {{}}\n", d.name, d.value);
        assert!(check_firmware(&s, &right).iter().all(|f| !f.wrong), "{:?}", check_firmware(&s, &right));
        let other = if d.value == "13" { "12" } else { "13" };
        let wrong = format!("// the LED\n#define {} {other}\n", d.name.trim_start_matches("PIN_"));
        let f = check_firmware(&s, &wrong);
        assert!(f.iter().any(|x| x.wrong && x.line == 2 && x.message.contains(&d.pin)), "{f:?}");
    }

    #[test]
    fn the_constants_of_a_sketch_are_read() {
        let text = "#define LED_PIN 13 // the LED\nconst int BUTTON = 2;\nconstexpr uint8_t SENSOR = A0;\nconst float GAIN = 2.5;\nint x = 3;\n#define NAME \"x\"\n";
        let got: Vec<(String, String, usize)> = read_firmware(text).into_iter().map(|f| (f.name, f.value, f.line)).collect();
        assert_eq!(got, [("LED_PIN".into(), "13".into(), 1), ("BUTTON".into(), "2".into(), 2), ("SENSOR".into(), "A0".into(), 3)]);
    }

    /// The Nano's LED as a Zephyr overlay through the Arduino header (D13 → 19).
    #[test]
    fn the_signals_go_out_as_a_zephyr_overlay() {
        let s = load("arduino_blink.schjson");
        let d = &pin_defs(&s)[0];
        let o = zephyr_overlay(&s, "blink");
        assert!(o.contains("zephyr,user") && o.contains(&dt_name(d)), "{o}");
        let n: u32 = d.value.parse().unwrap();
        assert!(o.contains(&format!("<&arduino_header {} GPIO_ACTIVE_HIGH>", n + 6)), "{o}");
    }

    /// A chip's signals by its package pins in the four formats; the written pins kept, and the ERC says when they change.
    #[test]
    fn a_chips_pins_go_out_as_constraints_and_a_change_is_noticed() {
        let mut s = load("arduino_blink.schjson");
        let part = pin_defs(&s)[0].at.split('.').next().unwrap().to_string();
        let sig = chip_signals(&s, &part).unwrap();
        assert!(!sig.is_empty());
        let (port, pin, _) = &sig[0];
        for (f, want) in [(Constraints::Pcf, format!("set_io {port} {pin}")), (Constraints::Xdc, format!("PACKAGE_PIN {pin} ")), (Constraints::Qsf, format!("-to {port}")), (Constraints::Lpf, format!("SITE \"{pin}\""))] {
            let t = constraints_text(&s, &part, f, "t").unwrap();
            assert!(t.contains(&want), "{f:?}: {t}");
        }
        assert!(chip_signals(&s, "NOPE").is_err());
        let kind = PinFile::of("top.pcf", Some(&part)).unwrap();
        assert!(PinFile::of("top.pcf", None).is_err() && PinFile::of("x.txt", None).is_err());
        assert_eq!(PinFile::of("pins.h", None).unwrap(), PinFile::Header);
        mark_written(&mut s, "top.pcf", &kind);
        assert!(firmware_problems(&s, None).is_empty(), "just written");
        // the signal moves to another pin (as a pin traded on the board comes back): the ERC asks to write it again
        s.firmware_link.pins.push_str(";x=1");
        assert!(firmware_problems(&s, None).iter().any(|p| p.message.contains("書き出したあと")));
    }

    /// The firmware file checked is read again by the ERC: a pin that differs is a warning with the line to fix.
    #[test]
    fn the_erc_checks_the_firmware_file() {
        let mut s = load("arduino_blink.schjson");
        let d = pin_defs(&s)[0].clone();
        let dir = std::env::temp_dir().join(format!("ccad_fw_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let other = if d.value == "13" { "12" } else { "13" };
        std::fs::write(dir.join("blink.ino"), format!("#define {} {other}\n", d.name)).unwrap();
        s.firmware_link.firmware = "blink.ino".into();
        let p = firmware_problems(&s, Some(&dir));
        assert!(p.iter().any(|x| x.message.contains("ファームのピン") && x.hint.contains("blink.ino (1)")), "{p:?}");
        s.firmware_link.firmware = "gone.ino".into();
        assert!(firmware_problems(&s, Some(&dir)).iter().any(|x| x.message.contains("見つかりません")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
