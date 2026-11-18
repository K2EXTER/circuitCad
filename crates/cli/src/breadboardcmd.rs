//! `breadboard`: the schematic laid out on a solderless breadboard (`sch_core::breadboard`): the hole of each lead, the jumpers, the
//! supplies off the board, and the check of what the holes join against the schematic.

use crate::session::{obj, Session};
use crate::Res;
use serde_json::{json, Value};
use sch_core::breadboard::{auto_layout, check, Hole};

impl Session {
    /// `breadboard`.
    pub fn breadboard(&mut self, _args: &[String]) -> Res {
        let l = auto_layout(&self.sch);
        let name = |h: &Hole| h.name();
        let parts: Vec<Value> = l.parts.iter().map(|p| json!({"part": p.refdes, "look": format!("{:?}", p.look), "leads": p.leads.iter().map(|(pin, h)| json!({"pin": pin, "hole": name(h)})).collect::<Vec<_>>()})).collect();
        let wires: Vec<Value> = l.wires.iter().map(|w| json!({"from": name(&w.a), "to": name(&w.b), "net": w.net})).collect();
        let off: Vec<Value> = l.off_board.iter().map(|o| json!({"part": o.refdes, "pins": o.pins.iter().map(|(pin, net, h)| json!({"pin": pin, "net": net, "hole": name(h)})).collect::<Vec<_>>()})).collect();
        let problems: Vec<String> = check(&self.sch, &l).iter().map(|p| p.message()).collect();
        Ok(obj(json!({
            "columns": l.cols, "rails": l.rails, "parts": parts, "wires": wires, "off_board": off, "problems": problems,
            "note": "holes: a-e above the groove and f-j below, joined in fives per column; 上+ 上− 下− 下+ are the rails along the long edges",
        })))
    }
}
