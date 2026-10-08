//! Schematic data model, netlist extraction, ERC and SPICE export.
//! Coordinates are integer grid steps, so connectivity is exact.

mod netlist;
mod spice;

pub use netlist::{erc, extract, Net, PinRef, Problem};
pub use spice::to_spice;

use serde::{Deserialize, Serialize};

pub type Pt = (i32, i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Led,
    /// DC voltage source: pin 1 is +, pin 2 is -.
    VSource,
    /// NPN transistor: pins C, B, E.
    Npn,
    /// Pin header with `n` pins in a column.
    Header(u8),
    /// Ground symbol (net GND).
    Ground,
    /// Power symbol; the value is the net name (VCC, +5V, ...).
    Power,
}

impl Kind {
    pub const PALETTE: &'static [Kind] = &[
        Kind::Resistor,
        Kind::Capacitor,
        Kind::Inductor,
        Kind::Diode,
        Kind::Led,
        Kind::VSource,
        Kind::Npn,
        Kind::Header(2),
        Kind::Header(4),
        Kind::Ground,
        Kind::Power,
    ];

    pub fn label(self) -> String {
        match self {
            Kind::Resistor => "抵抗".into(),
            Kind::Capacitor => "コンデンサ".into(),
            Kind::Inductor => "コイル".into(),
            Kind::Diode => "ダイオード".into(),
            Kind::Led => "LED".into(),
            Kind::VSource => "直流電源".into(),
            Kind::Npn => "NPNトランジスタ".into(),
            Kind::Header(n) => format!("ヘッダ {n}P"),
            Kind::Ground => "GND".into(),
            Kind::Power => "電源(VCC等)".into(),
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Resistor => "R",
            Kind::Capacitor => "C",
            Kind::Inductor => "L",
            Kind::Diode | Kind::Led => "D",
            Kind::VSource => "V",
            Kind::Npn => "Q",
            Kind::Header(_) => "J",
            Kind::Ground | Kind::Power => "#PWR",
        }
    }

    pub fn default_value(self) -> &'static str {
        match self {
            Kind::Resistor => "10k",
            Kind::Capacitor => "100n",
            Kind::Inductor => "10u",
            Kind::Diode => "1N4148",
            Kind::Led => "LED",
            Kind::VSource => "5",
            Kind::Npn => "2N3904",
            Kind::Header(_) => "Conn",
            Kind::Ground => "GND",
            Kind::Power => "VCC",
        }
    }

    /// Default PCB footprint name ("" for symbols that are not real parts).
    pub fn default_footprint(self) -> String {
        match self {
            Kind::Resistor => "R_0805".into(),
            Kind::Capacitor => "C_0805".into(),
            Kind::Inductor => "L_0805".into(),
            Kind::Diode => "D_SOD-123".into(),
            Kind::Led => "LED_0805".into(),
            Kind::VSource => "PinHeader_1x2_PM".into(),
            Kind::Npn => "SOT-23".into(),
            Kind::Header(n) => format!("PinHeader_1x{n}"),
            Kind::Ground | Kind::Power => String::new(),
        }
    }

    pub fn is_power_symbol(self) -> bool {
        matches!(self, Kind::Ground | Kind::Power)
    }

    /// Pins as (name, local offset) before rotation.
    pub fn pins(self) -> Vec<(String, Pt)> {
        let p = |n: &str, x, y| (n.to_string(), (x, y));
        match self {
            Kind::Resistor | Kind::Capacitor | Kind::Inductor => vec![p("1", -2, 0), p("2", 2, 0)],
            Kind::Diode | Kind::Led => vec![p("A", -2, 0), p("K", 2, 0)],
            Kind::VSource => vec![p("+", 0, -2), p("-", 0, 2)],
            Kind::Npn => vec![p("C", 2, -2), p("B", -2, 0), p("E", 2, 2)],
            Kind::Header(n) => (0..n as i32).map(|i| (format!("{}", i + 1), (2, 2 * i))).collect(),
            Kind::Ground | Kind::Power => vec![p("1", 0, 0)],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub refdes: String,
    pub kind: Kind,
    pub value: String,
    pub pos: Pt,
    /// Quarter turns clockwise on screen (0..=3).
    pub rot: u8,
    /// PCB footprint; empty means the kind's default.
    #[serde(default)]
    pub footprint: String,
}

pub fn rotate(p: Pt, rot: u8) -> Pt {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = (-q.1, q.0);
    }
    q
}

impl Component {
    pub fn footprint_name(&self) -> String {
        if self.footprint.is_empty() { self.kind.default_footprint() } else { self.footprint.clone() }
    }

    /// Pins as (name, absolute grid position).
    pub fn pins(&self) -> Vec<(String, Pt)> {
        self.kind
            .pins()
            .into_iter()
            .map(|(n, off)| {
                let r = rotate(off, self.rot);
                (n, (self.pos.0 + r.0, self.pos.1 + r.1))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wire {
    pub a: Pt,
    pub b: Pt,
}

/// Net label: names the net it sits on; equal names are the same net.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Label {
    pub pos: Pt,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Schematic {
    pub components: Vec<Component>,
    pub wires: Vec<Wire>,
    pub labels: Vec<Label>,
}

impl Schematic {
    /// Add a component with the next free reference designator.
    pub fn add(&mut self, kind: Kind, pos: Pt, rot: u8) -> usize {
        let refdes = if kind.is_power_symbol() {
            format!("#PWR{:02}", self.components.iter().filter(|c| c.kind.is_power_symbol()).count() + 1)
        } else {
            let used = self
                .components
                .iter()
                .filter(|c| c.kind.prefix() == kind.prefix())
                .filter_map(|c| c.refdes[kind.prefix().len()..].parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            format!("{}{}", kind.prefix(), used + 1)
        };
        self.components.push(Component { refdes, kind, value: kind.default_value().to_string(), pos, rot: rot % 4, footprint: kind.default_footprint() });
        self.components.len() - 1
    }
}

/// Is `p` on segment `a`-`b` (inclusive of endpoints)? Exact for integer grids.
pub fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let cross = (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64;
    cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

#[cfg(test)]
mod tests;
