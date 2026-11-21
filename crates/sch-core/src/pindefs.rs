//! Pin definitions for the firmware, from the schematic (candidates W1, W2): which pin of the microcontroller board each signal is
//! on, written as a C header for Arduino (`#define PIN_LED 13`), and a firmware file read back and checked against the schematic
//! (`LED_PIN` is 13 there, the schematic has it on D12). The schematic is what the board is built from: the header is made from it,
//! never the other way round.
//!
//! The microcontroller's pins are found by their names: a part's pin named D13, A0 or GPIO2 (an Arduino Nano, a Raspberry Pi Pico
//! drawn as a module), or a pin header's pin named so (`Component::pin_names`: the end of a cable to an Arduino Mega).

use crate::{extract, Kind, Schematic};

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
}
