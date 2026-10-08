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
        self.pads.retain(|p| p.refdes != part.refdes);
        self.silk.retain(|s| s.refdes != part.refdes);
        self.texts.retain(|t| t.refdes != part.refdes);

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
            pad.refdes = part.refdes.clone();
            pad.net = old_nets.get(&fpad.pin).cloned().unwrap_or_default();
            pad.pin = fpad.pin.clone();
            self.pads.push(pad);
        }
        let m = 0.3;
        let c = [Point::new(x0 - m, y0 - m), Point::new(x1 + m, y0 - m), Point::new(x1 + m, y1 + m), Point::new(x0 - m, y1 + m)];
        for i in 0..4 {
            self.silk.push(SilkLine { side: part.side, a: c[i], b: c[(i + 1) % 4], width: 0.15, refdes: part.refdes.clone() });
        }

        // designator above the part, centred
        self.texts.push(SilkText {
            side: part.side,
            pos: Point::new((x0 + x1) / 2.0, y1 + m + 0.2 + 0.5),
            height: 1.0,
            text: part.refdes.clone(),
            refdes: part.refdes.clone(),
        });

        match self.parts.iter_mut().find(|p| p.refdes == part.refdes) {
            Some(slot) => *slot = part,
            None => self.parts.push(part),
        }
        Ok(())
    }

    pub fn remove_part(&mut self, refdes: &str) {
        self.parts.retain(|p| p.refdes != refdes);
        self.pads.retain(|p| p.refdes != refdes);
        self.silk.retain(|s| s.refdes != refdes);
        self.texts.retain(|t| t.refdes != refdes);
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
