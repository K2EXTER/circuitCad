//! How a part goes onto a breadboard, stripboard or perfboard built by hand (B5), from its footprint: a DIP chip into a turned-pin
//! IC socket, a surface-mount chip (SOP, QFN, QFP, SOT-23, a crystal) soldered on an adapter board whose pins go into turned-pin
//! socket strips like a DIP or a SIP, a surface-mount resistor or capacitor swapped for its leaded twin, the rest straight in.
//!
//! The adapter keeps the chip's pin numbers (pin 1 of the chip is pin 1 of the adapter, the DIP numbering around), so a pin's
//! place on the board follows its number, also when the symbol leaves some pins out (`Mount::slots`).

use crate::{Component, Kind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mount {
    /// Leads straight into the board (a header, a TO-92 or TO-220 part, a leaded resistor).
    #[default]
    Direct,
    /// A DIP chip in a turned-pin IC socket of `pins` pins, its rows `width` holes apart.
    Socket { pins: u16, width: u16 },
    /// A surface-mount two-lead part (a 0805 resistor, a SOD-123 diode) built with the leaded part of the same value instead.
    Leaded,
    /// A surface-mount part soldered on an adapter board of `pins` pins: two rows `width` holes apart like a DIP, or one row (a SIP)
    /// when `width` is 0. Its pins go into turned-pin socket strips.
    Adapter { package: String, pins: u16, width: u16 },
}

/// The surface-mount sizes of two-lead parts.
const CHIP_SIZES: [&str; 14] = ["0201", "0402", "0603", "0805", "1206", "1210", "1812", "2010", "2512", "SOD-", "SMA", "SMB", "SMC", "MELF"];
/// Packages with their leads on two sides (or four, opened out to two rows on the adapter).
const TWO_ROW: [&str; 15] = ["SOIC", "SOP", "ESOP", "SSOP", "TSSOP", "MSOP", "QSOP", "HSOP", "SO-", "QFN", "DFN", "QFP", "TQFP", "LQFP", "LGA"];
/// Small packages put on a one-row adapter.
const ONE_ROW: [&str; 5] = ["SOT-", "SC-70", "SC70", "XTAL", "SOT"];

/// The first number in `s` after its first `-` (`SOIC-8_P1.27` → 8, `SOT-23-5` → 23).
fn count_after_dash(s: &str) -> Option<u16> {
    let rest = &s[s.find('-')? + 1..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The pins of a one-row package: SOT-23 3, SOT-23-5 5, SOT-143 4, SOT-223 3 (the tab is pin 2 again), SOT-89 3, SC-70-5 5.
fn one_row_pins(name: &str, leads: usize) -> u16 {
    let parts: Vec<&str> = name.split('-').collect();
    let n = match parts.as_slice() {
        [_, "23", n, ..] | [_, "70", n, ..] | [_, "363", n, ..] => n.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok(),
        [_, "23", ..] | [_, "223", ..] | [_, "89", ..] => Some(3),
        [_, "143", ..] => Some(4),
        [_, "363", ..] => Some(6),
        _ => None,
    };
    n.unwrap_or(0).max(leads as u16)
}

/// How the part goes onto a board built by hand; `leads` is how many of its pins have a lead.
pub fn mount_of(c: &Component, leads: usize) -> Mount {
    let fp = if c.footprint.is_empty() { c.kind.default_footprint() } else { c.footprint.clone() };
    let up = fp.to_ascii_uppercase();
    let name = up.split('_').next().unwrap_or(&up).to_string();
    if leads <= 2 {
        if !CHIP_SIZES.iter().any(|t| up.contains(t)) && !name.starts_with("XTAL") {
            return Mount::Direct;
        }
        let leaded_twin = matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor | Kind::Diode | Kind::Led) || matches!(name.as_str(), "R" | "C" | "L" | "LED" | "D" | "CP");
        return if leaded_twin { Mount::Leaded } else { Mount::Adapter { package: fp.split('_').take(2).collect::<Vec<_>>().join("_"), pins: 2, width: 0 } };
    }
    if name.starts_with("DIP-") {
        let pins = count_after_dash(&name).unwrap_or(leads as u16).max(leads as u16);
        // the row spacing from the name (DIP-28_W15.24), 0.3 inch when it does not say
        let width = up.split('_').find_map(|t| t.strip_prefix('W')?.parse::<f32>().ok()).map(|w| (w / 2.54).round() as u16).unwrap_or(3).max(3);
        return Mount::Socket { pins: pins + pins % 2, width };
    }
    if TWO_ROW.iter().any(|t| name.starts_with(t)) {
        let pins = count_after_dash(&name).unwrap_or(0).max(leads as u16);
        let pins = pins + pins % 2;
        let width = if name.starts_with(['Q', 'D', 'T', 'L']) && !name.starts_with("TSSOP") {
            match pins {
                0..=20 => 3,
                21..=64 => 6,
                _ => 10,
            }
        } else if pins <= 20 {
            3
        } else {
            6
        };
        return Mount::Adapter { package: name, pins, width };
    }
    if ONE_ROW.iter().any(|t| name.starts_with(t)) {
        let pins = if name.starts_with("XTAL") { (leads as u16).max(4) } else { one_row_pins(&name, leads) };
        let package = if name.starts_with("XTAL") { fp.split('_').take(2).collect::<Vec<_>>().join("_") } else { name };
        return Mount::Adapter { package, pins, width: 0 };
    }
    Mount::Direct
}

impl Mount {
    /// The holes between the two rows of a socket or a two-row adapter (3 = 0.3 inch); None for one row or two leads.
    pub fn width(&self) -> Option<u16> {
        match self {
            Mount::Socket { width, .. } => Some(*width),
            Mount::Adapter { width, .. } if *width > 0 => Some(*width),
            _ => None,
        }
    }

    /// A one-row adapter (a SIP).
    pub fn one_row(&self) -> bool {
        matches!(self, Mount::Adapter { width: 0, pins, .. } if *pins > 2)
    }

    /// Where each lead sits on the socket or the adapter: the number of places, and each lead's place (from 0, pin 1 first),
    /// from the leads' pin numbers. A part without a socket or an adapter, or whose pins are not all numbers within the places,
    /// takes its leads in their order.
    pub fn slots(&self, leads: &[(String, String)]) -> (usize, Vec<usize>) {
        let places = match self {
            Mount::Socket { pins, .. } | Mount::Adapter { pins, .. } => *pins as usize,
            _ => 0,
        };
        let numbers: Option<Vec<usize>> = leads.iter().map(|(p, _)| p.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= places)).collect();
        match numbers {
            Some(v) if places >= leads.len() && places > 0 => (places, v.into_iter().map(|n| n - 1).collect()),
            _ => (leads.len(), (0..leads.len()).collect()),
        }
    }

    /// One line on how the part goes on (empty for a part that goes straight in).
    pub fn describe(&self) -> String {
        match self {
            Mount::Direct => String::new(),
            Mount::Socket { pins, width } => format!("丸ピン IC ソケット {pins} ピン ({}) に差す", inches(*width)),
            Mount::Leaded => "同じ値のリード部品で組む".into(),
            Mount::Adapter { package, pins, width: 0 } if *pins <= 2 => format!("{package} を 2 ピンの変換基板に載せる"),
            Mount::Adapter { package, pins, width: 0 } => format!("{package} を {pins} ピンの SIP 変換基板に載せ、丸ピンの連結ソケット 1×{pins} に差す"),
            Mount::Adapter { package, pins, width } => format!("{package} を DIP {pins} ピン ({}) の変換基板に載せ、丸ピンの連結ソケット 1×{} を 2 本に差す", inches(*width), pins / 2),
        }
    }
}

/// A row spacing in holes as inches (3 → 0.3 インチ幅).
fn inches(width: u16) -> String {
    format!("{:.1} インチ幅", width as f32 * 0.1)
}

/// What to get for the sockets and the adapters: (item, how many, the parts), one line per kind of item.
pub fn shopping(parts: &[(String, Mount)]) -> Vec<(String, usize, Vec<String>)> {
    let mut out: Vec<(String, usize, Vec<String>)> = vec![];
    let mut add = |item: String, n: usize, refdes: &str| match out.iter_mut().find(|(i, _, _)| *i == item) {
        Some(row) => {
            row.1 += n;
            if !row.2.iter().any(|r| r == refdes) {
                row.2.push(refdes.to_string());
            }
        }
        None => out.push((item, n, vec![refdes.to_string()])),
    };
    for (r, m) in parts {
        match m {
            Mount::Direct => {}
            Mount::Socket { pins, width } => add(format!("丸ピン IC ソケット {pins} ピン ({})", inches(*width)), 1, r),
            Mount::Leaded => add("リード部品に替える (チップ部品の代わり)".into(), 1, r),
            Mount::Adapter { package, pins, width } => {
                if *pins <= 2 {
                    add(format!("{package} の 2 ピン変換基板"), 1, r);
                } else if *width == 0 {
                    add(format!("{package} → SIP {pins} ピンの変換基板"), 1, r);
                    add(format!("丸ピン連結ソケット 1×{pins}"), 1, r);
                } else {
                    add(format!("{package} → DIP {pins} ピン ({}) の変換基板", inches(*width)), 1, r);
                    add(format!("丸ピン連結ソケット 1×{}", pins / 2), 2, r);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: Kind, footprint: &str) -> Component {
        let mut s = crate::Schematic::default();
        s.add(kind, (0, 0), 0);
        let mut c = s.components.remove(0);
        c.footprint = footprint.into();
        c
    }

    /// One row per footprint: how it goes on and where its leads sit.
    #[test]
    fn mounts_from_footprints() {
        let cases: [(Kind, &str, usize, Mount); 12] = [
            (Kind::Resistor, "R_0805", 2, Mount::Leaded),
            (Kind::Resistor, "R_Axial_10.16", 2, Mount::Direct),
            (Kind::Custom, "XTAL_3225", 2, Mount::Adapter { package: "XTAL_3225".into(), pins: 2, width: 0 }),
            (Kind::Custom, "XTAL_3225", 4, Mount::Adapter { package: "XTAL_3225".into(), pins: 4, width: 0 }),
            (Kind::Custom, "DIP-14_W7.62", 14, Mount::Socket { pins: 14, width: 3 }),
            (Kind::Custom, "DIP-40_W15.24", 40, Mount::Socket { pins: 40, width: 6 }),
            (Kind::Custom, "SOIC-8_P1.27", 8, Mount::Adapter { package: "SOIC-8".into(), pins: 8, width: 3 }),
            (Kind::Custom, "QFN-16_4x4_EP", 12, Mount::Adapter { package: "QFN-16".into(), pins: 16, width: 3 }),
            (Kind::Custom, "TQFP-32_7x7_P0.8", 32, Mount::Adapter { package: "TQFP-32".into(), pins: 32, width: 6 }),
            (Kind::Custom, "SOT-23-5_P0.95", 5, Mount::Adapter { package: "SOT-23-5".into(), pins: 5, width: 0 }),
            (Kind::Custom, "SOT-223", 3, Mount::Adapter { package: "SOT-223".into(), pins: 3, width: 0 }),
            (Kind::Custom, "TO-220-3", 3, Mount::Direct),
        ];
        for (kind, fp, leads, want) in cases {
            assert_eq!(mount_of(&part(kind, fp), leads), want, "{fp}");
        }
        // the default footprint of a basic part counts: an NPN is a SOT-23
        assert_eq!(mount_of(&part(Kind::Npn, ""), 3), Mount::Adapter { package: "SOT-23".into(), pins: 3, width: 0 });
    }

    #[test]
    fn leads_sit_by_their_pin_numbers() {
        let pins = |v: &[&str]| v.iter().map(|p| (p.to_string(), String::new())).collect::<Vec<_>>();
        let qfn = Mount::Adapter { package: "QFN-16".into(), pins: 16, width: 3 };
        assert_eq!(qfn.slots(&pins(&["1", "2", "13", "15"])), (16, vec![0, 1, 12, 14]));
        // names that are not numbers: in their order
        assert_eq!(Mount::Adapter { package: "SOT-23".into(), pins: 3, width: 0 }.slots(&pins(&["C", "B", "E"])), (3, vec![0, 1, 2]));
        assert_eq!(Mount::Direct.slots(&pins(&["1", "5"])), (2, vec![0, 1]));
        let list = shopping(&[("U1".into(), qfn.clone()), ("U2".into(), qfn), ("R1".into(), Mount::Leaded)]);
        assert_eq!(list[0], ("QFN-16 → DIP 16 ピン (0.3 インチ幅) の変換基板".into(), 2, vec!["U1".into(), "U2".into()]));
        assert_eq!(list[1].1, 4, "two socket strips for each two-row adapter");
    }
}
