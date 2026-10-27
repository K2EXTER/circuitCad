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
        self.pads.retain(|p| p.refdes != part.refdes);
        self.silk.retain(|s| s.refdes != part.refdes);
        // a designator the user turned stays turned when the part is placed again
        let label_rot = self.texts.iter().find(|t| t.refdes == part.refdes).map_or(0, |t| t.rot);
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

        // designator above the part, centred (plus the offset the user gave it)
        let off = self.label_offsets.get(&part.refdes).copied().unwrap_or_default();
        let mut label = SilkText { side: part.side, pos: Point::default(), height: 1.0, text: part.refdes.clone(), refdes: part.refdes.clone(), rot: label_rot };
        let half_h = label.size().1 / 2.0;
        label.pos = Point::new((x0 + x1) / 2.0 + off.x, y1 + m + 0.2 + half_h + off.y);
        self.texts.push(label);

        if !old_pads.is_empty() {
            self.follow_pads(&part.refdes, &old_pads);
        }
        match self.parts.iter_mut().find(|p| p.refdes == part.refdes) {
            Some(slot) => *slot = part,
            None => self.parts.push(part),
        }
        Ok(())
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

    pub fn remove_part(&mut self, refdes: &str) {
        self.label_offsets.remove(refdes);
        self.parts.retain(|p| p.refdes != refdes);
        self.pads.retain(|p| p.refdes != refdes);
        self.silk.retain(|s| s.refdes != refdes);
        self.texts.retain(|t| t.refdes != refdes);
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
}
