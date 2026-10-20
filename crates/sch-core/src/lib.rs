//! Schematic data model, netlist extraction, ERC and SPICE export.
//! Coordinates are integer grid steps, so connectivity is exact.

mod netlist;
mod ngspice;
mod spice;

pub use netlist::{erc, extract, net_at, Net, PinRef, Problem, Severity};
pub use spice::{node as spice_node, to_spice, to_spice_for, Dialect};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Library part with its own symbol (see `Component::custom`).
    Custom,
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
            Kind::VSource => "電源".into(),
            Kind::Npn => "NPNトランジスタ".into(),
            Kind::Header(n) => format!("ヘッダ {n}P"),
            Kind::Custom => "ライブラリ部品".into(),
            Kind::Ground => "GND".into(),
            Kind::Power => "電源ネット(VCC等)".into(),
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
            Kind::Custom => "U",
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
            Kind::Custom => "",
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
            Kind::Custom | Kind::Ground | Kind::Power => String::new(),
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
            Kind::Custom => vec![],
            Kind::Ground | Kind::Power => vec![p("1", 0, 0)],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Symbol definition for `Kind::Custom` parts (embedded, so the file is self-contained).
    #[serde(default)]
    pub custom: Option<CustomSymbol>,
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
        if let Some(sym) = &self.custom {
            return sym.layout().pins.into_iter().map(|p| (p.number, (self.pos.0 + rotate(p.tip, self.rot).0, self.pos.1 + rotate(p.tip, self.rot).1))).collect();
        }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub a: Pt,
    pub b: Pt,
}

/// Net label: names the net it sits on; equal names are the same net.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub pos: Pt,
    pub text: String,
}

/// Sheet layout version. 0 = files saved before two-lead glyph parts (electrolytic, chip R/C/L)
/// got short leads; they are fixed up when read (see `RawSchematic`).
pub const LAYOUT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawSchematic")]
pub struct Schematic {
    pub components: Vec<Component>,
    pub wires: Vec<Wire>,
    pub labels: Vec<Label>,
    pub layout_version: u32,
}

impl Default for Schematic {
    fn default() -> Self {
        Schematic { components: vec![], wires: vec![], labels: vec![], layout_version: LAYOUT_VERSION }
    }
}

#[derive(Deserialize)]
struct RawSchematic {
    components: Vec<Component>,
    wires: Vec<Wire>,
    labels: Vec<Label>,
    #[serde(default)]
    layout_version: u32,
}

impl From<RawSchematic> for Schematic {
    fn from(r: RawSchematic) -> Self {
        let mut s = Schematic { components: r.components, wires: r.wires, labels: r.labels, layout_version: r.layout_version };
        s.migrate();
        s
    }
}

impl Schematic {
    /// Bring an old sheet up to date. Version 0 drew two-lead glyph parts with leads 4 grid steps
    /// long; now they are 2, like the basic symbols. Wire ends and labels on the old pin move to the
    /// new one; a wire that would turn diagonal gets a short stub to the old spot instead.
    fn migrate(&mut self) {
        if self.layout_version >= LAYOUT_VERSION {
            return;
        }
        self.layout_version = LAYOUT_VERSION;
        let mut stubs: Vec<Wire> = vec![];
        for c in &self.components {
            let Some(sym) = c.custom.as_ref().filter(|s| s.is_two_lead_glyph()) else { continue };
            for p in &sym.pins {
                let sign = if p.side == PinSide::Left { -1 } else { 1 };
                let at = |x: i32| {
                    let r = rotate((x, 0), c.rot);
                    (c.pos.0 + r.0, c.pos.1 + r.1)
                };
                let (old, new) = (at(4 * sign), at(2 * sign));
                for w in &mut self.wires {
                    for (end, other) in [(0, w.b), (1, w.a)] {
                        let e = if end == 0 { &mut w.a } else { &mut w.b };
                        if *e != old {
                            continue;
                        }
                        if new.0 == other.0 || new.1 == other.1 {
                            *e = new;
                        } else if !stubs.iter().any(|s| s.a == new && s.b == old) {
                            stubs.push(Wire { a: new, b: old });
                        }
                    }
                }
                for l in &mut self.labels {
                    if l.pos == old {
                        l.pos = new;
                    }
                }
            }
        }
        self.wires.retain(|w| w.a != w.b);
        self.wires.extend(stubs);
    }
}

impl Schematic {
    /// Add a component with the next free reference designator.
    fn next_refdes(&self, prefix: &str) -> String {
        let used = self
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .filter_map(|c| c.refdes.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("{prefix}{}", used + 1)
    }

    pub fn add(&mut self, kind: Kind, pos: Pt, rot: u8) -> usize {
        let refdes = if kind.is_power_symbol() {
            format!("#PWR{:02}", self.components.iter().filter(|c| c.kind.is_power_symbol()).count() + 1)
        } else {
            self.next_refdes(kind.prefix())
        };
        self.components.push(Component {
            refdes,
            kind,
            value: kind.default_value().to_string(),
            pos,
            rot: rot % 4,
            footprint: kind.default_footprint(),
            custom: None,
        });
        self.components.len() - 1
    }

    /// Place a library part. `footprint` is the footprint name the part uses on the PCB.
    pub fn add_custom(&mut self, sym: CustomSymbol, value: &str, footprint: &str, pos: Pt, rot: u8) -> usize {
        let refdes = self.next_refdes(&sym.prefix);
        self.components.push(Component {
            refdes,
            kind: Kind::Custom,
            value: value.to_string(),
            pos,
            rot: rot % 4,
            footprint: footprint.to_string(),
            custom: Some(sym),
        });
        self.components.len() - 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinSide {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymPin {
    /// Pin number as printed in the datasheet; also the pad name in the footprint and the
    /// pin id used in netlists.
    pub number: String,
    pub name: String,
    pub side: PinSide,
    /// A pin that only exists for the simulation (e.g. the light or magnetic-field input of a
    /// sensor model): it has no pad and is left out of the board netlist.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sim_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimKind {
    /// `.model` card; instantiated as `<element><ref> nodes... <name>`.
    Model,
    /// `.subckt` ... `.ends`; instantiated as `X<ref> nodes... <name>`.
    Subckt,
}

/// Electrical behaviour of a part for simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimModel {
    pub kind: SimKind,
    /// SPICE element letter for `Model` (D, Q, M, J, ...). Ignored for subcircuits.
    #[serde(default)]
    pub element: String,
    /// Model or subcircuit name used on the instance line.
    pub name: String,
    /// Full `.model` card or `.subckt` block.
    pub text: String,
    /// Symbol pin numbers in the node order the model expects.
    pub pins: Vec<String>,
    /// Characteristics for built-in simulators (gain, Vf, ... ); free form.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    /// What the model leaves out or only approximates, one sentence each. Shown next to the part
    /// so that a result is not mistaken for the real part's behaviour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<String>,
}

/// One datasheet row: a rated or typical figure, e.g. ("順方向電圧", "0.9V", "IF = 1A").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpecRow {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
}

/// Datasheet information of a part: the absolute maximum ratings (beyond them the part can be
/// damaged) and any other figures worth keeping next to it. All of it is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartRatings {
    /// Largest voltage the part may see (reverse voltage, Vce, Vds, supply), in volts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_max: Option<f64>,
    /// Largest continuous current, in amperes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub i_max: Option<f64>,
    /// Largest power the part may dissipate, in watts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p_max: Option<f64>,
    /// Rated / typical figures from the data sheet, free form.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<SpecRow>,
    /// The ripple current an electrolytic capacitor may carry (from the data sheet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ripple: Option<RippleRating>,
    /// The endurance of an electrolytic capacitor: hours at its highest temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life: Option<LifeRating>,
}

/// Rated ripple current: `i_rms` amperes (rms) at `freq_hz`, at the capacitor's highest temperature.
/// `coeffs` is the data sheet's table of multipliers by frequency (low frequencies carry less).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RippleRating {
    pub i_rms: f64,
    /// The frequency the rated current is given at (100 kHz on most data sheets).
    pub freq_hz: f64,
    /// (frequency in Hz, multiplier), from the data sheet's "multiplier for ripple current" table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coeffs: Vec<(f64, f64)>,
}

impl RippleRating {
    /// The ripple current allowed at `freq` Hz: the rated current times the table's multiplier at that
    /// frequency relative to the one at the rated frequency (the table is interpolated on a log frequency axis,
    /// and held at its ends). Without a table, the rated current at every frequency.
    pub fn allowed_at(&self, freq: f64) -> f64 {
        let at = |f: f64| -> f64 {
            let pts = &self.coeffs;
            let (first, last) = match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => (a, b),
                _ => return 1.0,
            };
            if f <= first.0 {
                return first.1;
            }
            if f >= last.0 {
                return last.1;
            }
            let k = pts.windows(2).position(|w| f >= w[0].0 && f <= w[1].0).unwrap_or(0);
            let (a, b) = (pts[k], pts[k + 1]);
            let x = (f.ln() - a.0.ln()) / (b.0.ln() - a.0.ln());
            a.1 + (b.1 - a.1) * x
        };
        let reference = at(self.freq_hz.max(1.0));
        if reference <= 0.0 { self.i_rms } else { self.i_rms * at(freq.max(1.0)) / reference }
    }
}

/// Endurance: the capacitor lasts `hours` at `temp_c` (its highest temperature), rated voltage and ripple current.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LifeRating {
    pub hours: f64,
    pub temp_c: f64,
}

impl LifeRating {
    /// The usual rule: every 10 degrees below the rated temperature doubles the life, every 10 above halves it.
    pub fn at(&self, core_temp_c: f64) -> f64 {
        self.hours * 2f64.powf((self.temp_c - core_temp_c) / 10.0)
    }
}

impl PartRatings {
    pub fn is_empty(&self) -> bool {
        self.v_max.is_none() && self.i_max.is_none() && self.p_max.is_none() && self.rows.is_empty() && self.ripple.is_none() && self.life.is_none()
    }

    /// "最大定格: 電圧 40V・電流 1A・電力 1W" plus the free rows, for tooltips.
    pub fn summary(&self) -> String {
        let fmt = |v: f64, u: &str| format!("{}{u}", trim_float(v));
        let mut max = vec![];
        if let Some(v) = self.v_max {
            max.push(format!("電圧 {}", fmt(v, "V")));
        }
        if let Some(i) = self.i_max {
            max.push(format!("電流 {}", fmt(i, "A")));
        }
        if let Some(p) = self.p_max {
            max.push(format!("電力 {}", fmt(p, "W")));
        }
        let mut t = String::new();
        if !max.is_empty() {
            t.push_str(&format!("最大定格: {}", max.join("・")));
        }
        if let Some(r) = &self.ripple {
            if !t.is_empty() {
                t.push('\n');
            }
            t.push_str(&format!("定格リップル電流: {}A rms ({}Hz)", trim_float(r.i_rms), trim_float(r.freq_hz)));
        }
        if let Some(l) = &self.life {
            if !t.is_empty() {
                t.push('\n');
            }
            t.push_str(&format!("寿命: {} 時間 ({}℃)", trim_float(l.hours), trim_float(l.temp_c)));
        }
        for r in &self.rows {
            if !t.is_empty() {
                t.push('\n');
            }
            t.push_str(&format!("{}: {}", r.name, r.value));
            if !r.condition.is_empty() {
                t.push_str(&format!(" ({})", r.condition));
            }
        }
        t
    }
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomSymbol {
    pub name: String,
    pub prefix: String,
    pub pins: Vec<SymPin>,
    #[serde(default)]
    pub sim: Option<SimModel>,
    /// Data sheet ratings of the part (None = not given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratings: Option<PartRatings>,
    /// "capacitor" / "resistor" / "inductor": drawn as that symbol instead of a box (two pins, left and right).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub glyph: String,
}

pub struct PinGeom {
    pub number: String,
    pub name: String,
    pub side: PinSide,
    /// Connection point (relative to the component origin, before rotation).
    pub tip: Pt,
    /// Where the lead meets the body.
    pub edge: Pt,
}

pub struct SymLayout {
    /// Body half-extents.
    pub hw: i32,
    pub hh: i32,
    pub pins: Vec<PinGeom>,
}

impl CustomSymbol {
    /// A capacitor / resistor / inductor glyph with one lead on each of the left and right.
    pub fn is_two_lead_glyph(&self) -> bool {
        matches!(self.glyph.as_str(), "capacitor" | "electrolytic" | "resistor" | "inductor")
            && self.pins.len() == 2
            && self.pins.iter().all(|p| matches!(p.side, PinSide::Left | PinSide::Right))
    }

    /// The standard-symbol glyph a bundled part gets, guessed from its name and pin names ("" = none).
    pub fn inferred_glyph(&self) -> &'static str {
        let by_name: &[(&[&str], &str)] = &[
            (&["1N4001", "1N4007", "1N4148", "1N5408", "UF4007"], "diode"),
            (&["1N4728A", "1N4733A", "1N4742A"], "zener"),
            (&["1N5817", "1N5819", "1N5822", "SS14", "BAT54"], "schottky"),
            (&["2N3904", "BC547", "BD139", "PN2222A", "S8050", "TIP120"], "npn"),
            (&["2N3906", "BD140", "S8550", "TIP125"], "pnp"),
            (&["2N7000", "BSS138", "AO3400A", "IRF540N", "IRFZ44N", "IRLZ44N"], "nmos"),
            (&["AO3401A", "IRF9540N"], "pmos"),
            (&["LM324", "LM358", "LM393", "MCP6001", "MCP6002", "NE5532", "TL072"], "opamp"),
            (&["LDR_GL5528"], "ldr"),
            (&["BPW34"], "photodiode"),
            (&["TL431"], "shunt_reg"),
        ];
        if let Some((_, g)) = by_name.iter().find(|(names, _)| names.contains(&self.name.as_str())) {
            return g;
        }
        if self.name.starts_with("XTAL") {
            return "crystal";
        }
        let has = |n: &str| self.pins.iter().any(|p| p.name == n);
        if self.pins.len() == 4 && has("P1") && has("P2") && has("S1") && has("S2") {
            return "transformer";
        }
        if self.pins.len() == 2 && has("+") && has("-") {
            return "battery";
        }
        ""
    }

    /// Body half-extents of a standard symbol drawn small: diodes and batteries (two leads, left and
    /// right), crystals and transistors (left lead, top and bottom leads) take the room of a chip part.
    fn compact_size(&self) -> Option<(i32, i32)> {
        let on = |s: PinSide| self.pins.iter().filter(|p| p.side == s).count();
        match self.glyph.as_str() {
            "diode" | "zener" | "schottky" | "tvs" | "battery" if self.pins.len() == 2 && on(PinSide::Left) == 1 && on(PinSide::Right) == 1 => Some((0, 1)),
            // a crystal: leads left and right like a capacitor, the case pins (four-pin part) just below
            "crystal" if on(PinSide::Left) == 1 && on(PinSide::Right) == 1 && (self.pins.len() == 2 || (self.pins.len() == 4 && on(PinSide::Bottom) == 2)) => Some((0, 1)),
            "npn" | "pnp" | "nmos" | "pmos" if self.pins.len() == 3 && on(PinSide::Left) == 1 && on(PinSide::Top) == 1 && on(PinSide::Bottom) == 1 => Some((0, 0)),
            _ => None,
        }
    }

    /// Box symbol: pins on each side are spaced 2 grid steps apart, in the order given.
    pub fn layout(&self) -> SymLayout {
        let count = |s: PinSide| self.pins.iter().filter(|p| p.side == s).count() as i32;
        let (hw, hh) = if self.is_two_lead_glyph() {
            // drawn as the basic symbol: pin tips at +-2, same as the basic C/R/L
            (0, 1)
        } else if let Some(size) = self.compact_size() {
            size
        } else {
            (2.max(count(PinSide::Top).max(count(PinSide::Bottom))), 2.max(count(PinSide::Left).max(count(PinSide::Right))))
        };
        let mut idx = [0i32; 4];
        let mut pins = vec![];
        for p in &self.pins {
            let (slot, n) = match p.side {
                PinSide::Left => (0, count(PinSide::Left)),
                PinSide::Right => (1, count(PinSide::Right)),
                PinSide::Top => (2, count(PinSide::Top)),
                PinSide::Bottom => (3, count(PinSide::Bottom)),
            };
            let along = -(n - 1) + 2 * idx[slot];
            idx[slot] += 1;
            let (tip, edge) = match p.side {
                PinSide::Left => ((-(hw + 2), along), (-hw, along)),
                PinSide::Right => ((hw + 2, along), (hw, along)),
                PinSide::Top => ((along, -(hh + 2)), (along, -hh)),
                PinSide::Bottom => ((along, hh + 2), (along, hh)),
            };
            pins.push(PinGeom { number: p.number.clone(), name: p.name.clone(), side: p.side, tip, edge });
        }
        SymLayout { hw, hh, pins }
    }
}

/// Is `p` on segment `a`-`b` (inclusive of endpoints)? Exact for integer grids.
pub fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let cross = (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64;
    cross == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
}

#[cfg(test)]
mod tests;
