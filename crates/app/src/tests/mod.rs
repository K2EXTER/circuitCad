mod features;
mod examples;
mod converge;
mod flow;
mod mixed;
mod uln2803a;
mod sample_models;
mod sim_snapshot;

use super::*;

/// A library part's symbol as one block: the samples drawn before the gate ICs' gates became units (Q6, 2026-11-24) keep the
/// 74HC00 and the like as the one box with all fourteen pins they were drawn with.
pub(crate) fn one_block(mut sym: sch_core::CustomSymbol) -> sch_core::CustomSymbol {
    for p in &mut sym.pins {
        p.unit = 0;
    }
    sym
}
use pcb_core::ratsnest::ratsnest;
use pcb_core::{Point, Side, Track};
use sch_core::{Kind, Wire};

/// Short wire out of a pin with the net name at its end (shared by the sample generators below).
pub(super) fn label_pin(s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str) {
    let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
    let c = s.components[idx].pos;
    let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
    let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
    s.wires.push(Wire { a: pt, b: end });
    s.labels.push(sch_core::Label { pos: end, text: text.into() });
}

pub(super) fn library_part(name: &str) -> part_lib::PartDef {
    part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == name).unwrap()
}
