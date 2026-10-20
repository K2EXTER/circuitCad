//! Import a schematic netlist into a board.

use crate::model::*;

#[derive(Clone, Debug)]
pub struct NetlistPart {
    pub refdes: String,
    pub value: String,
    pub footprint: String,
}

#[derive(Clone, Debug)]
pub struct NetlistNet {
    pub name: String,
    /// (refdes, pin)
    pub pins: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub updated: usize,
    pub warnings: Vec<String>,
}

/// Synchronise the board with the netlist: add new parts (outside the board, ready to be
/// dragged in), update value/footprint of existing ones while keeping their position,
/// remove parts that left the schematic, and assign nets to pads.
pub fn import(board: &mut Board, parts: &[NetlistPart], nets: &[NetlistNet]) -> ImportReport {
    let mut rep = ImportReport::default();

    let gone: Vec<String> =
        board.parts.iter().filter(|b| !parts.iter().any(|p| p.refdes == b.refdes)).map(|b| b.refdes.clone()).collect();
    for r in gone {
        board.remove_part(&r);
        rep.removed.push(r);
    }

    let (lo, hi) = board.bounds().unwrap_or((Point::default(), Point::default()));
    for np in parts {
        match board.parts.iter().find(|b| b.refdes == np.refdes).cloned() {
            Some(existing) => {
                let part = Part { value: np.value.clone(), footprint: np.footprint.clone(), ..existing };
                match board.set_part(part) {
                    Ok(()) => rep.updated += 1,
                    Err(e) => rep.warnings.push(format!("{}: {e}", np.refdes)),
                }
            }
            None => {
                let k = rep.added.len();
                let pos = Point::new(hi.x + 8.0 + (k / 6) as f64 * 14.0, lo.y + 4.0 + (k % 6) as f64 * 9.0);
                let part = Part { refdes: np.refdes.clone(), value: np.value.clone(), footprint: np.footprint.clone(), pos, rot: 0, side: Side::Top };
                match board.set_part(part) {
                    Ok(()) => rep.added.push(np.refdes.clone()),
                    Err(e) => rep.warnings.push(format!("{}: {e}", np.refdes)),
                }
            }
        }
    }

    for p in board.pads.iter_mut().filter(|p| !p.refdes.is_empty()) {
        p.net.clear();
    }
    for net in nets {
        for (r, pin) in &net.pins {
            // every pad of the pin gets the net (a tab or exposed pad may share the pin number)
            let mut found = false;
            for p in board.pads.iter_mut().filter(|p| &p.refdes == r && &p.pin == pin) {
                p.net = net.name.clone();
                found = true;
            }
            if !found && board.parts.iter().any(|b| &b.refdes == r) {
                rep.warnings.push(format!("{r} のピン {pin} がフットプリントにありません"));
            }
        }
    }
    rep
}

/// What differs between a netlist and the board (what `import` would change).
#[derive(Default, Clone, Debug)]
pub struct NetlistDiff {
    /// Parts only the schematic has.
    pub added: Vec<String>,
    /// Parts only the board has.
    pub removed: Vec<String>,
    /// Parts whose value or footprint differs.
    pub changed: Vec<String>,
    /// Pads whose net differs from the schematic's.
    pub nets: usize,
}

impl NetlistDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty() && self.nets == 0
    }

    /// A different net or a part on one side only makes the board another circuit; a value or footprint alone does not.
    pub fn blocking(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || self.nets > 0
    }

    /// "追加 2 (R3 C4) / 削除 1 …", for the status line and the tooltip.
    pub fn summary(&self) -> String {
        let list = |v: &Vec<String>| {
            let shown: Vec<&str> = v.iter().take(4).map(|s| s.as_str()).collect();
            format!("{} ({}{})", v.len(), shown.join(" "), if v.len() > 4 { " …" } else { "" })
        };
        let mut t = vec![];
        if !self.added.is_empty() {
            t.push(format!("追加 {}", list(&self.added)));
        }
        if !self.removed.is_empty() {
            t.push(format!("削除 {}", list(&self.removed)));
        }
        if !self.changed.is_empty() {
            t.push(format!("値・形が違う {}", list(&self.changed)));
        }
        if self.nets > 0 {
            t.push(format!("つなぎが違うパッド {}", self.nets));
        }
        t.join(" / ")
    }
}

/// How the board differs from the netlist, without changing it.
pub fn diff(board: &Board, parts: &[NetlistPart], nets: &[NetlistNet]) -> NetlistDiff {
    let mut d = NetlistDiff::default();
    for p in parts {
        match board.parts.iter().find(|b| b.refdes == p.refdes) {
            None => d.added.push(p.refdes.clone()),
            Some(b) if b.value != p.value || b.footprint != p.footprint => d.changed.push(p.refdes.clone()),
            Some(_) => {}
        }
    }
    d.removed = board.parts.iter().filter(|b| !parts.iter().any(|p| p.refdes == b.refdes)).map(|b| b.refdes.clone()).collect();
    let expected: std::collections::HashMap<(&str, &str), &str> = nets.iter().flat_map(|n| n.pins.iter().map(move |(r, p)| ((r.as_str(), p.as_str()), n.name.as_str()))).collect();
    d.nets = board
        .pads
        .iter()
        .filter(|p| !p.refdes.is_empty() && parts.iter().any(|q| q.refdes == p.refdes))
        .filter(|p| p.net != expected.get(&(p.refdes.as_str(), p.pin.as_str())).copied().unwrap_or(""))
        .count();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{drc, ratsnest::ratsnest};

    fn parts() -> Vec<NetlistPart> {
        vec![
            NetlistPart { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into() },
            NetlistPart { refdes: "R2".into(), value: "1k".into(), footprint: "R_0805".into() },
            NetlistPart { refdes: "R3".into(), value: "1k".into(), footprint: "R_0805".into() },
        ]
    }

    fn nets() -> Vec<NetlistNet> {
        let n = |name: &str, pins: &[(&str, &str)]| NetlistNet {
            name: name.into(),
            pins: pins.iter().map(|(r, p)| (r.to_string(), p.to_string())).collect(),
        };
        vec![n("VCC", &[("R1", "1")]), n("MID", &[("R1", "2"), ("R2", "1"), ("R3", "1")]), n("GND", &[("R2", "2"), ("R3", "2")])]
    }

    #[test]
    fn a_schematic_saved_with_the_old_18650_holder_footprint_still_imports() {
        let mut b = Board::new("t", 100.0, 40.0);
        let parts = [NetlistPart { refdes: "BT1".into(), value: "LIION_18650".into(), footprint: "BatteryHolder_18650".into() }];
        let nets = [NetlistNet { name: "BAT".into(), pins: vec![("BT1".into(), "1".into())] }, NetlistNet { name: "GND".into(), pins: vec![("BT1".into(), "2".into())] }];
        let rep = import(&mut b, &parts, &nets);
        assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
        let pads: Vec<&Pad> = b.pads.iter().filter(|p| p.refdes == "BT1").collect();
        assert_eq!(pads.len(), 2);
        assert!((pads[0].pos.dist(pads[1].pos) - 77.0).abs() < 1e-9, "the pads are those of the holder, 77 mm apart");
        assert!(pads.iter().all(|p| !p.net.is_empty()));
    }

    #[test]
    fn import_places_parts_outside_board_and_assigns_nets() {
        let mut b = Board::new("t", 30.0, 20.0);
        let rep = import(&mut b, &parts(), &nets());
        assert_eq!(rep.added.len(), 3);
        assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
        assert_eq!(b.pads.len(), 6);
        assert!(b.parts.iter().all(|p| p.pos.x > 30.0));
        let net_of = |r: &str, pin: &str| b.pads.iter().find(|p| p.refdes == r && p.pin == pin).unwrap().net.clone();
        assert_eq!(net_of("R1", "2"), "MID");
        assert_eq!(net_of("R3", "2"), "GND");
    }

    #[test]
    fn reimport_keeps_positions_and_removes_deleted_parts() {
        let mut b = Board::new("t", 30.0, 20.0);
        import(&mut b, &parts(), &nets());
        b.place_part("R1", Point::new(10.0, 10.0), 1);
        let mut p2 = parts();
        p2.pop(); // R3 removed from schematic
        let rep = import(&mut b, &p2, &nets());
        assert_eq!(rep.removed, vec!["R3".to_string()]);
        assert_eq!(rep.updated, 2);
        let r1 = b.parts.iter().find(|p| p.refdes == "R1").unwrap();
        assert_eq!((r1.pos.x, r1.pos.y, r1.rot), (10.0, 10.0, 1));
        assert_eq!(b.pads.len(), 4);
        assert_eq!(b.silk.iter().filter(|s| s.refdes == "R3").count(), 0);
        // R3 pins are gone, so the netlist reference must not produce a pad
        assert!(b.pads.iter().all(|p| p.refdes != "R3"));
    }

    #[test]
    fn rotation_turns_pads_vertical() {
        let mut b = Board::new("t", 30.0, 20.0);
        import(&mut b, &parts(), &nets());
        b.place_part("R1", Point::new(10.0, 10.0), 1);
        let p: Vec<_> = b.pads.iter().filter(|p| p.refdes == "R1").collect();
        assert!((p[0].pos.x - 10.0).abs() < 1e-9 && (p[0].pos.y - 10.0 + 0.9125).abs() < 1e-9);
        assert_eq!(p[0].shape, PadShape::Rect { w: 1.4, h: 1.025 });
    }

    #[test]
    fn unknown_pin_and_footprint_are_reported() {
        let mut b = Board::new("t", 30.0, 20.0);
        let mut ps = parts();
        ps[2].footprint = "NOPE".into();
        let mut ns = nets();
        ns[0].pins.push(("R1".into(), "9".into()));
        let rep = import(&mut b, &ps, &ns);
        assert_eq!(rep.warnings.len(), 2, "{:?}", rep.warnings);
    }

    #[test]
    fn ratsnest_shrinks_as_routing_progresses() {
        let mut b = Board::new("t", 30.0, 20.0);
        import(&mut b, &parts(), &nets());
        // MID net has 3 pads in 3 groups -> 2 airwires; VCC single pad and GND 2 pads -> 1
        let w = ratsnest(&b);
        assert_eq!(w.iter().filter(|a| a.net == "MID").count(), 2);
        assert_eq!(w.iter().filter(|a| a.net == "GND").count(), 1);
        assert_eq!(w.iter().filter(|a| a.net == "VCC").count(), 0);
        // route R1.2 to R2.1
        let a = b.pads.iter().find(|p| p.refdes == "R1" && p.pin == "2").unwrap().pos;
        let c = b.pads.iter().find(|p| p.refdes == "R2" && p.pin == "1").unwrap().pos;
        b.tracks.push(Track { side: Side::Top, a, b: c, width: 0.25 });
        let w = ratsnest(&b);
        assert_eq!(w.iter().filter(|a| a.net == "MID").count(), 1);
    }

    #[test]
    fn drc_detects_net_short() {
        let mut b = Board::new("t", 30.0, 20.0);
        import(&mut b, &parts(), &nets());
        let a = b.pads.iter().find(|p| p.refdes == "R1" && p.pin == "1").unwrap().pos; // VCC
        let c = b.pads.iter().find(|p| p.refdes == "R2" && p.pin == "2").unwrap().pos; // GND
        assert!(drc::shorts(&b).is_empty());
        b.tracks.push(Track { side: Side::Top, a, b: c, width: 0.25 });
        let s = drc::shorts(&b);
        assert_eq!(s.len(), 1);
        assert!(s[0].message.contains("VCC") && s[0].message.contains("GND"));
    }

    #[test]
    fn all_footprints_resolve() {
        for n in crate::footprints::names() {
            assert!(crate::footprints::get(&n).is_some(), "{n}");
        }
    }
}
