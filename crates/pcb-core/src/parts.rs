//! Placing footprint instances on a board.

use crate::footprints;
use crate::model::*;
use std::collections::HashMap;

/// Rotate `p` by `rot` quarter turns counter-clockwise (y up).
pub fn rotate(p: Point, rot: u8) -> Point {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = Point::new(-q.y, q.x);
    }
    q
}

impl Board {
    /// Board-embedded footprints take precedence over the built-in library.
    pub fn footprint(&self, name: &str) -> Option<footprints::Footprint> {
        self.footprints.iter().find(|f| f.name == name).cloned().or_else(|| footprints::get(name))
    }

    /// Register (or replace) a user footprint.
    pub fn add_footprint(&mut self, fp: footprints::Footprint) {
        self.footprints.retain(|f| f.name != fp.name);
        self.footprints.push(fp);
    }

    /// Insert or replace a part, regenerating its pads and silkscreen outline.
    /// Pad nets are kept for pins that still exist.
    pub fn set_part(&mut self, part: Part) -> Result<(), String> {
        let fp = self.footprint(&part.footprint).ok_or_else(|| format!("フットプリント {} が見つかりません", part.footprint))?;
        let old_nets: HashMap<String, String> = self
            .pads
            .iter()
            .filter(|p| p.refdes == part.refdes)
            .map(|p| (p.pin.clone(), p.net.clone()))
            .collect();
        // a part given another footprint: its pads are not where they were, and the tracks that ended on them must follow
        let old_pads: Vec<Pad> = match self.parts.iter().find(|p| p.refdes == part.refdes) {
            Some(old) if old.footprint != part.footprint => self.pads.iter().filter(|p| p.refdes == part.refdes).cloned().collect(),
            _ => vec![],
        };
        // the pin names on the silk (`board pinlabels`) go with the part, keeping their place next to their pads
        if let Some(old) = self.parts.iter().find(|p| p.refdes == part.refdes).cloned() {
            self.carry_pin_labels(&old, &part);
        }
        self.pads.retain(|p| p.refdes != part.refdes);
        self.silk.retain(|s| s.refdes != part.refdes);
        // a designator the user turned (or made bigger) stays so when the part is placed again
        let label_rot = self.texts.iter().find(|t| t.refdes == part.refdes).map_or(0, |t| t.rot);
        let label_height = self.texts.iter().find(|t| t.refdes == part.refdes).map_or(1.0, |t| t.height);
        self.texts.retain(|t| t.refdes != part.refdes);

        let mut new_holes: Vec<Hole> = vec![];
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        let bottom = part.side == Side::Bottom;
        for fpad in &fp.pads {
            // a part on the bottom is mirrored left-to-right before it is rotated
            let local = if bottom { Point::new(-fpad.pos.x, fpad.pos.y) } else { fpad.pos };
            let r = rotate(local, part.rot);
            let pos = Point::new(part.pos.x + r.x, part.pos.y + r.y);
            let shape = match (fpad.shape, part.rot % 2) {
                (PadShape::Rect { w, h }, 1) => PadShape::Rect { w: h, h: w },
                (PadShape::Oval { w, h }, 1) => PadShape::Oval { w: h, h: w },
                (s, _) => s,
            };
            let (w, h) = shape.size();
            x0 = x0.min(pos.x - w / 2.0);
            x1 = x1.max(pos.x + w / 2.0);
            y0 = y0.min(pos.y - h / 2.0);
            y1 = y1.max(pos.y + h / 2.0);
            let mut pad = Pad::new(pos, shape, fpad.drill, part.side);
            pad.slot = fpad.slot;
            pad.slot_v = fpad.slot_v ^ (part.rot % 2 == 1);
            pad.refdes = part.refdes.clone();
            pad.net = old_nets.get(&fpad.pin).cloned().unwrap_or_default();
            pad.pin = fpad.pin.clone();
            self.pads.push(pad);
        }
        // the part's own holes (locating pegs) follow it, and count for its outline
        for fh in &fp.holes {
            let local = if bottom { Point::new(-fh.pos.x, fh.pos.y) } else { fh.pos };
            let r = rotate(local, part.rot);
            let pos = Point::new(part.pos.x + r.x, part.pos.y + r.y);
            let mut hole = Hole::new(pos, fh.drill);
            hole.slot = fh.slot;
            hole.slot_v = fh.slot_v ^ (part.rot % 2 == 1);
            hole.refdes = part.refdes.clone();
            let (a, b) = hole.ends();
            let rad = fh.drill / 2.0;
            x0 = x0.min(a.x.min(b.x) - rad);
            x1 = x1.max(a.x.max(b.x) + rad);
            y0 = y0.min(a.y.min(b.y) - rad);
            y1 = y1.max(a.y.max(b.y) + rad);
            new_holes.push(hole);
        }
        // holes of the same part are replaced in place, so the indices of other holes (a drag in progress) stay put
        let old: Vec<usize> = (0..self.holes.len()).filter(|&i| self.holes[i].refdes == part.refdes).collect();
        if old.len() == new_holes.len() {
            for (i, h) in old.into_iter().zip(new_holes) {
                self.holes[i] = h;
            }
        } else {
            self.holes.retain(|h| h.refdes != part.refdes);
            self.holes.extend(new_holes);
        }
        let m = 0.3;
        // the footprint's own body outline and polarity marks; a box around the pads when it has none.
        // (A footprint saved in a board or a library part may predate its outline: the built-in one of that name is used.)
        let outline = if fp.silk.is_empty() { footprints::get(&fp.name).map(|b| b.silk).unwrap_or_default() } else { fp.silk.clone() };
        if outline.is_empty() {
            let c = [Point::new(x0 - m, y0 - m), Point::new(x1 + m, y0 - m), Point::new(x1 + m, y1 + m), Point::new(x0 - m, y1 + m)];
            for i in 0..4 {
                self.silk.push(SilkLine { side: part.side, a: c[i], b: c[(i + 1) % 4], width: 0.15, refdes: part.refdes.clone() });
            }
        } else {
            let place = |q: Point| {
                let local = if bottom { Point::new(-q.x, q.y) } else { q };
                let r = rotate(local, part.rot);
                Point::new(part.pos.x + r.x, part.pos.y + r.y)
            };
            for l in &outline {
                let (a, b) = (place(l.a), place(l.b));
                x0 = x0.min(a.x.min(b.x));
                x1 = x1.max(a.x.max(b.x));
                y0 = y0.min(a.y.min(b.y));
                y1 = y1.max(a.y.max(b.y));
                self.silk.push(SilkLine { side: part.side, a, b, width: 0.15, refdes: part.refdes.clone() });
            }
        }

        // a diode / LED with a box and no polarity mark of its own: a bar across the box on the cathode side, as on the body of the
        // real part (the square pad alone does not tell a person with a soldering iron which end is which)
        if outline.is_empty() {
            let two: Vec<&Pad> = self.pads.iter().filter(|p| p.refdes == part.refdes).collect();
            if let (2, Some(k), Some(a)) = (two.len(), two.iter().find(|p| p.pin == "K"), two.iter().find(|p| p.pin == "A")) {
                let (dx, dy) = (a.pos.x - k.pos.x, a.pos.y - k.pos.y);
                // along the axis of the two pads only (parts are turned by quarter turns); the bar must have room between them
                let horizontal = dx.abs() > dy.abs();
                let (ext_k, ext_a) = if horizontal { (k.shape.size().0, a.shape.size().0) } else { (k.shape.size().1, a.shape.size().1) };
                let dist = if horizontal { dx.abs() } else { dy.abs() };
                let gap = dist - ext_k / 2.0 - ext_a / 2.0;
                if gap >= 2.0 && (horizontal && dy.abs() < 1e-6 || !horizontal && dx.abs() < 1e-6) {
                    let sign = if horizontal { dx.signum() } else { dy.signum() };
                    let off = ext_k / 2.0 + 0.5;
                    let (pa, pb) = if horizontal {
                        let bx = k.pos.x + sign * off;
                        (Point::new(bx, y0 - m), Point::new(bx, y1 + m))
                    } else {
                        let by = k.pos.y + sign * off;
                        (Point::new(x0 - m, by), Point::new(x1 + m, by))
                    };
                    self.silk.push(SilkLine { side: part.side, a: pa, b: pb, width: 0.3, refdes: part.refdes.clone() });
                }
            }
        }

        // designator above the part, centred (plus the offset the user gave it)
        let off = self.label_offsets.get(&part.refdes).copied().unwrap_or_default();
        let mut label = SilkText { side: part.side, pos: Point::default(), height: label_height, text: part.refdes.clone(), refdes: part.refdes.clone(), rot: label_rot };
        let half_h = label.size().1 / 2.0;
        label.pos = Point::new((x0 + x1) / 2.0 + off.x, y1 + m + 0.2 + half_h + off.y);
        if !self.hidden_labels.contains(&part.refdes) {
            self.texts.push(label);
        }

        if !old_pads.is_empty() {
            self.follow_pads(&part.refdes, &old_pads);
        }
        match self.parts.iter_mut().find(|p| p.refdes == part.refdes) {
            Some(slot) => *slot = part,
            None => self.parts.push(part),
        }
        Ok(())
    }

    /// Move the pin-name texts of a part (marked "REF#pin") from where the part was (`old`) to where it goes (`new`), as if they
    /// were drawn on the part: the same place relative to its pads, the other side of the board when it is turned over. The
    /// letters are not turned with it, so they stay upright.
    fn carry_pin_labels(&mut self, old: &Part, new: &Part) {
        if old.pos == new.pos && old.rot % 4 == new.rot % 4 && old.side == new.side {
            return;
        }
        let marker = crate::pinlabels::marker(&old.refdes);
        let mirror = |p: Point, side: Side| if side == Side::Bottom { Point::new(-p.x, p.y) } else { p };
        for t in self.texts.iter_mut().filter(|t| t.refdes == marker) {
            let rel = Point::new(t.pos.x - old.pos.x, t.pos.y - old.pos.y);
            let local = mirror(rotate(rel, (4 - old.rot % 4) % 4), old.side);
            let r = rotate(mirror(local, new.side), new.rot);
            t.pos = Point::new(new.pos.x + r.x, new.pos.y + r.y);
            if old.side != new.side {
                t.side = new.side;
            }
        }
    }

    /// After a part got another footprint: move the end of every track that sat on one of its old pads to the centre of the
    /// new pad of the same pin (the nearest one when a pin has several). Returns how many track ends moved.
    fn follow_pads(&mut self, refdes: &str, old_pads: &[Pad]) -> usize {
        let new_pads: Vec<Pad> = self.pads.iter().filter(|p| p.refdes == refdes).cloned().collect();
        let mut moved = 0;
        for t in &mut self.tracks {
            for end in [&mut t.a, &mut t.b] {
                let Some(old) = old_pads.iter().find(|p| {
                    let (w, h) = p.shape.size();
                    (p.drill.is_some() || p.side == t.side)
                        && (end.x - p.pos.x).abs() <= w / 2.0 + 1e-6
                        && (end.y - p.pos.y).abs() <= h / 2.0 + 1e-6
                }) else {
                    continue;
                };
                let target = new_pads
                    .iter()
                    .filter(|p| p.pin == old.pin && (p.drill.is_some() || p.side == t.side))
                    .min_by(|a, b| a.pos.dist(*end).total_cmp(&b.pos.dist(*end)));
                if let Some(n) = target
                    && n.pos != *end
                {
                    *end = n.pos;
                    moved += 1;
                }
            }
        }
        moved
    }

    /// Move a part's designator text to `to` (its centre). The offset from the automatic spot is kept, so the text stays
    /// there when the part moves. Returns false when the part has no text.
    pub fn move_label(&mut self, refdes: &str, to: Point) -> bool {
        let Some(t) = self.texts.iter_mut().find(|t| t.refdes == refdes) else { return false };
        let (dx, dy) = (to.x - t.pos.x, to.y - t.pos.y);
        t.pos = to;
        let o = self.label_offsets.entry(refdes.to_string()).or_default();
        o.x += dx;
        o.y += dy;
        true
    }

    /// Turn a part's designator text a quarter turn about its own centre. Returns false when the part has no text.
    pub fn turn_label(&mut self, refdes: &str) -> bool {
        let Some(t) = self.texts.iter_mut().find(|t| t.refdes == refdes) else { return false };
        t.rot = (t.rot + 1) % 4;
        true
    }

    /// Put a part's designator text back to the automatic spot. Returns false when it had not been moved.
    pub fn reset_label(&mut self, refdes: &str) -> bool {
        if self.label_offsets.remove(refdes).is_none() {
            return false;
        }
        if let Some(part) = self.parts.iter().find(|p| p.refdes == refdes).cloned() {
            let _ = self.set_part(part);
        }
        true
    }

    /// The parts whose outline on the silk has fewer lines than their footprint draws now: parts placed (or last moved) by an older
    /// version, from before a mark was added to the footprint (the cathode bar of a diode, the + of a tantalum capacitor). The silk
    /// of a part is kept in the board file as it was drawn, so a newer version does not add the mark by itself.
    pub fn stale_part_silk(&self) -> Vec<String> {
        let mut fresh = self.clone();
        let count = |b: &Board, r: &str| b.silk.iter().filter(|s| s.refdes == r).count();
        let mut out = vec![];
        for p in &self.parts {
            if fresh.set_part(p.clone()).is_ok() && count(&fresh, &p.refdes) > count(self, &p.refdes) {
                out.push(p.refdes.clone());
            }
        }
        out
    }

    /// Draw the silk of these parts again from their footprints, where they are (nothing moves; a silk line width changed by hand
    /// goes back to the footprint's). Returns how many parts were redrawn.
    pub fn refresh_part_silk(&mut self, refdes: &[String]) -> usize {
        let parts: Vec<Part> = self.parts.iter().filter(|p| refdes.contains(&p.refdes)).cloned().collect();
        parts.into_iter().filter(|p| self.set_part(p.clone()).is_ok()).count()
    }

    pub fn remove_part(&mut self, refdes: &str) {
        self.label_offsets.remove(refdes);
        self.parts.retain(|p| p.refdes != refdes);
        self.pads.retain(|p| p.refdes != refdes);
        self.silk.retain(|s| s.refdes != refdes);
        let pins = crate::pinlabels::marker(refdes);
        self.texts.retain(|t| t.refdes != refdes && t.refdes != pins);
        self.holes.retain(|h| h.refdes != refdes);
    }

    /// Turn a part over to the other side of the board (position and rotation are kept;
    /// the pad nets are kept per pin). Returns false for an unknown part.
    pub fn flip_part(&mut self, refdes: &str) -> bool {
        let Some(mut part) = self.parts.iter().find(|p| p.refdes == refdes).cloned() else { return false };
        part.side = part.side.flip();
        self.set_part(part).is_ok()
    }

    /// Move / rotate an existing part.
    pub fn place_part(&mut self, refdes: &str, pos: Point, rot: u8) {
        if let Some(mut part) = self.parts.iter().find(|p| p.refdes == refdes).cloned() {
            part.pos = pos;
            part.rot = rot % 4;
            let _ = self.set_part(part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_names_on_the_silk_go_with_a_moved_turned_or_flipped_connector() {
        let mut b = Board::new("t", 60.0, 40.0);
        b.set_part(Part { refdes: "J1".into(), value: "Conn".into(), footprint: "PinHeader_1x2".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top })
            .unwrap();
        let names = vec![("J1".to_string(), vec!["5V".to_string(), "GND".to_string()])];
        assert_eq!(crate::pinlabels::write_pin_labels(&mut b, &names, 0.8), 2);
        // each name keeps its distance from its own pad, whatever the part does
        let gaps = |b: &Board| -> Vec<(String, f64)> {
            let pin_of = |name: &str| if name == "5V" { "1" } else { "2" };
            let mut v: Vec<(String, f64)> = b
                .texts
                .iter()
                .filter(|t| t.refdes == "J1#pin")
                .map(|t| {
                    let pad = b.pads.iter().find(|p| p.refdes == "J1" && p.pin == pin_of(&t.text)).unwrap();
                    let nearest = b.pads.iter().filter(|p| p.refdes == "J1").min_by(|x, y| x.pos.dist(t.pos).total_cmp(&y.pos.dist(t.pos))).unwrap();
                    assert_eq!(nearest.pin, pad.pin, "{} is next to pin {}, not its own pin {}", t.text, nearest.pin, pad.pin);
                    (t.text.clone(), (pad.pos.dist(t.pos) * 1e6).round() / 1e6)
                })
                .collect();
            v.sort_by(|x, y| x.0.cmp(&y.0));
            v
        };
        let before = gaps(&b);
        assert_eq!(before.len(), 2);
        b.place_part("J1", Point::new(30.0, 25.0), 0);
        assert_eq!(gaps(&b), before, "moved");
        b.place_part("J1", Point::new(30.0, 25.0), 3);
        assert_eq!(gaps(&b), before, "turned");
        assert!(b.flip_part("J1"));
        assert_eq!(gaps(&b), before, "turned over");
        assert!(b.texts.iter().filter(|t| t.refdes == "J1#pin").all(|t| t.side == Side::Bottom), "the names go to the bottom silk too");
        b.remove_part("J1");
        assert!(b.texts.iter().all(|t| t.refdes != "J1#pin"), "and go away with the part");
    }

    #[test]
    fn silk_drawn_by_an_older_version_is_found_and_drawn_again() {
        let mut b = Board::new("t", 60.0, 40.0);
        b.set_part(Part { refdes: "D1".into(), value: "1N4148W".into(), footprint: "D_SOD-123".into(), pos: Point::new(20.0, 20.0), rot: 2, side: Side::Top })
            .unwrap();
        b.set_part(Part { refdes: "C2".into(), value: "22u".into(), footprint: "CT_3528-21".into(), pos: Point::new(40.0, 20.0), rot: 0, side: Side::Top }).unwrap();
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(30.0, 30.0), rot: 0, side: Side::Top }).unwrap();
        assert!(b.stale_part_silk().is_empty(), "freshly placed parts are up to date");
        let full = b.silk.len();
        // what an older version left: the diode without its cathode bar, the tantalum without its + (a plain box)
        b.silk.retain(|s| !(s.refdes == "D1" && s.width > 0.2));
        b.silk.retain(|s| s.refdes != "C2");
        for (a, c) in [((38.0, 18.5), (42.0, 18.5)), ((42.0, 18.5), (42.0, 21.5)), ((42.0, 21.5), (38.0, 21.5)), ((38.0, 21.5), (38.0, 18.5))] {
            b.silk.push(SilkLine { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(c.0, c.1), width: 0.15, refdes: "C2".into() });
        }
        assert_eq!(b.stale_part_silk(), vec!["D1".to_string(), "C2".to_string()]);
        let warn = crate::fab::preflight(&b, None).into_iter().find(|c| c.title.starts_with(crate::fab::STALE_SILK_TITLE));
        assert!(warn.is_some_and(|c| c.level == crate::fab::Level::Warn && c.title.contains("D1, C2")));
        let parts_before = b.parts.clone();
        assert_eq!(b.refresh_part_silk(&b.stale_part_silk()), 2);
        assert!(b.stale_part_silk().is_empty());
        assert_eq!(b.silk.len(), full, "the marks are back");
        let places = |ps: &[Part]| ps.iter().map(|p| (p.refdes.clone(), p.pos, p.rot)).collect::<Vec<_>>();
        assert_eq!(places(&b.parts), places(&parts_before), "nothing moved");
    }

    #[test]
    fn a_track_end_follows_its_pad_when_the_footprint_changes() {
        let mut b = Board::new("t", 40.0, 30.0);
        let part = |fp: &str| Part { refdes: "C1".into(), value: "1n".into(), footprint: fp.into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top };
        b.set_part(part("C_0603")).unwrap();
        let p1 = b.pads.iter().find(|p| p.pin == "1").unwrap().pos;
        let far = Point::new(p1.x - 3.0, p1.y);
        b.tracks.push(Track { side: Side::Top, a: far, b: p1, width: 0.25 });
        b.set_part(part("C_0402")).unwrap();
        let n1 = b.pads.iter().find(|p| p.pin == "1").unwrap().pos;
        assert_ne!(n1, p1);
        assert_eq!(b.tracks[0].b, n1, "the end on the pad moves with it");
        assert_eq!(b.tracks[0].a, far, "the other end stays");
        // the same footprint again changes nothing
        b.set_part(part("C_0402")).unwrap();
        assert_eq!(b.tracks[0].b, n1);
    }

    #[test]
    fn a_diode_with_room_between_its_pads_gets_a_cathode_bar_on_the_cathode_side() {
        let part = |fp: &str, rot: u8| Part { refdes: "D1".into(), value: "1N4148".into(), footprint: fp.into(), pos: Point::new(20.0, 15.0), rot, side: Side::Top };
        let bar = |b: &Board| b.silk.iter().filter(|l| l.refdes == "D1" && l.width > 0.2).cloned().collect::<Vec<_>>();
        // a library diode (a footprint with pads only, as from a part file): pads 7.62 apart, K on the left; the bar is just inside
        // the K pad, across the box. (The built-in D_DO-35_7.62 has its own outline with a bar and is not touched.)
        let lib = || {
            let mut fp = crate::footprints::get("D_DO-35_7.62").unwrap();
            fp.name = "D_DO-35_P7.62".into();
            fp.silk.clear();
            fp
        };
        let mut b = Board::new("t", 60.0, 40.0);
        b.add_footprint(lib());
        b.set_part(part("D_DO-35_P7.62", 0)).unwrap();
        let (k, a) = (b.pads.iter().find(|p| p.pin == "K").unwrap().pos, b.pads.iter().find(|p| p.pin == "A").unwrap().pos);
        let lines = bar(&b);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!((lines[0].a.x - lines[0].b.x).abs() < 1e-9 && lines[0].a.y != lines[0].b.y, "a bar across the part");
        let side = |x: f64| (x - k.x).abs() < (x - a.x).abs();
        assert!(side(lines[0].a.x), "nearer the cathode than the anode: bar {} K {} A {}", lines[0].a.x, k.x, a.x);
        // turned by 180 degrees the bar goes with the cathode to the other end
        b.set_part(part("D_DO-35_P7.62", 2)).unwrap();
        let (k2, a2) = (b.pads.iter().find(|p| p.pin == "K").unwrap().pos, b.pads.iter().find(|p| p.pin == "A").unwrap().pos);
        assert!(k2.x > a2.x);
        let l2 = bar(&b);
        assert_eq!(l2.len(), 1);
        assert!((l2[0].a.x - k2.x).abs() < (l2[0].a.x - a2.x).abs());
        // a quarter turn: the bar lies across the other way
        b.set_part(part("D_DO-35_P7.62", 1)).unwrap();
        let l3 = bar(&b);
        assert!(l3.len() == 1 && (l3[0].a.y - l3[0].b.y).abs() < 1e-9 && l3[0].a.x != l3[0].b.x, "{l3:?}");
        // a small LED (0.8 mm between the pads) has no room: nothing is added
        let mut b = Board::new("t", 60.0, 40.0);
        b.set_part(part("LED_0805", 0)).unwrap();
        assert!(bar(&b).is_empty());
        // the built-in DO-35 keeps its own outline (one bar of its own, not two)
        let mut b = Board::new("t", 60.0, 40.0);
        b.set_part(part("D_DO-35_7.62", 0)).unwrap();
        assert!(bar(&b).is_empty(), "no second bar: {:?}", bar(&b));
        // and a part without K and A pads is left alone
        let mut b = Board::new("t", 60.0, 40.0);
        b.set_part(Part { refdes: "D1".into(), value: "x".into(), footprint: "R_Axial_10.16".into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top }).unwrap();
        assert!(bar(&b).is_empty());
    }
}
