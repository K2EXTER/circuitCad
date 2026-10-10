//! Hit testing, selection, moving and deleting items.

use super::*;

impl BoardEditor {
    pub(super) fn hit_test(&self, p: Point) -> Option<Item> {
        let tol = 6.0 / self.zoom as f64;
        for (i, v) in self.board.vias.iter().enumerate() {
            if p.dist(v.pos) <= v.diameter / 2.0 + tol {
                return Some(Item::Via(i));
            }
        }
        for (i, pd) in self.board.pads.iter().enumerate().filter(|(_, pd)| pd.drill.is_some() || self.side_visible(pd.side)) {
            let (w, h) = pd.shape.size();
            if (p.x - pd.pos.x).abs() <= w / 2.0 + tol && (p.y - pd.pos.y).abs() <= h / 2.0 + tol {
                if !pd.refdes.is_empty()
                    && let Some(k) = self.board.parts.iter().position(|b| b.refdes == pd.refdes) {
                        return Some(Item::Part(k));
                    }
                return Some(Item::Pad(i));
            }
        }
        for (i, h) in self.board.holes.iter().enumerate() {
            if p.dist(h.pos) <= h.drill / 2.0 + tol {
                return Some(Item::Hole(i));
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.side_visible(t.side)) {
            if seg_dist(p, t.a, t.b) <= t.width / 2.0 + tol {
                return Some(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty()) {
            if seg_dist(p, l.a, l.b) <= l.width / 2.0 + tol {
                return Some(Item::Silk(i));
            }
        }
        for (i, z) in self.board.zones.iter().enumerate().filter(|(_, z)| self.vis.zones && self.side_visible(z.side)) {
            if polygon_edge_dist(&z.polygon, p) <= tol || inside_polygon(&z.polygon, p) {
                return Some(Item::Zone(i));
            }
        }
        None
    }

    pub(super) fn set_selection(&mut self, items: Vec<Item>) {
        if items.len() == 1 {
            self.selected = Some(items[0]);
            self.multi.clear();
        } else {
            self.selected = None;
            self.multi = items;
        }
    }

    /// Objects touched by the box. `crossing`: any overlap counts, otherwise the object must lie
    /// completely inside. Hidden layers are skipped, and zones count only by their outline so that
    /// a big copper pour is not grabbed by every box drawn over it.
    pub(super) fn items_in_box(&self, a: Point, b: Point, crossing: bool) -> Vec<Item> {
        let (lo, hi) = (Point::new(a.x.min(b.x), a.y.min(b.y)), Point::new(a.x.max(b.x), a.y.max(b.y)));
        let rect_hit = |c: Point, w: f64, h: f64| {
            let (rl, rh) = (Point::new(c.x - w / 2.0, c.y - h / 2.0), Point::new(c.x + w / 2.0, c.y + h / 2.0));
            if crossing { rects_overlap(lo, hi, rl, rh) } else { rect_contains(lo, hi, rl) && rect_contains(lo, hi, rh) }
        };
        let seg_hit = |p: Point, q: Point, width: f64| {
            if crossing {
                let m = width / 2.0;
                segment_hits_rect(p, q, Point::new(lo.x - m, lo.y - m), Point::new(hi.x + m, hi.y + m))
            } else {
                rect_contains(lo, hi, p) && rect_contains(lo, hi, q)
            }
        };
        let mut out = vec![];
        for (i, part) in self.board.parts.iter().enumerate() {
            let pads: Vec<&Pad> = self.board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
            if pads.is_empty() || !(self.side_visible(part.side) || pads.iter().any(|p| p.drill.is_some())) {
                continue;
            }
            let hits = |p: &&Pad| {
                let (w, h) = p.shape.size();
                rect_hit(p.pos, w, h)
            };
            let hit = if crossing { pads.iter().any(hits) } else { pads.iter().all(hits) };
            if hit {
                out.push(Item::Part(i));
            }
        }
        for (i, pd) in self.board.pads.iter().enumerate().filter(|(_, p)| p.refdes.is_empty() && (p.drill.is_some() || self.side_visible(p.side))) {
            let (w, h) = pd.shape.size();
            if rect_hit(pd.pos, w, h) {
                out.push(Item::Pad(i));
            }
        }
        for (i, v) in self.board.vias.iter().enumerate() {
            if rect_hit(v.pos, v.diameter, v.diameter) {
                out.push(Item::Via(i));
            }
        }
        for (i, h) in self.board.holes.iter().enumerate() {
            if rect_hit(h.pos, h.drill, h.drill) {
                out.push(Item::Hole(i));
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.side_visible(t.side)) {
            if seg_hit(t.a, t.b, t.width) {
                out.push(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty() && self.side_visible(l.side)) {
            if seg_hit(l.a, l.b, l.width) {
                out.push(Item::Silk(i));
            }
        }
        for (i, z) in self.board.zones.iter().enumerate().filter(|(_, z)| self.vis.zones && self.side_visible(z.side)) {
            let n = z.polygon.len();
            let hit = if crossing {
                (0..n).any(|k| segment_hits_rect(z.polygon[k], z.polygon[(k + 1) % n], lo, hi))
            } else {
                z.polygon.iter().all(|p| rect_contains(lo, hi, *p))
            };
            if n >= 3 && hit {
                out.push(Item::Zone(i));
            }
        }
        out
    }

    /// Points to ring when an object is selected.
    pub(super) fn item_points(&self, it: Item) -> Vec<Point> {
        match it {
            Item::Part(i) if i < self.board.parts.len() => {
                let r = &self.board.parts[i].refdes;
                self.board.pads.iter().filter(|p| &p.refdes == r).map(|p| p.pos).collect()
            }
            Item::Track(i) if i < self.board.tracks.len() => vec![self.board.tracks[i].a, self.board.tracks[i].b],
            Item::Silk(i) if i < self.board.silk.len() => vec![self.board.silk[i].a, self.board.silk[i].b],
            Item::Pad(i) if i < self.board.pads.len() => vec![self.board.pads[i].pos],
            Item::Via(i) if i < self.board.vias.len() => vec![self.board.vias[i].pos],
            Item::Hole(i) if i < self.board.holes.len() => vec![self.board.holes[i].pos],
            Item::Zone(i) if i < self.board.zones.len() => self.board.zones[i].polygon.clone(),
            _ => vec![],
        }
    }

    /// Move the grouped objects to `orig` shifted by (dx, dy).
    pub(super) fn move_group(&mut self, g: &GroupDrag, dx: f64, dy: f64) {
        let mv = |p: Point| Point::new(p.x + dx, p.y + dy);
        for it in &g.items {
            match *it {
                Item::Part(i) => {
                    if let Some(p) = g.orig.parts.get(i) {
                        self.board.place_part(&p.refdes, mv(p.pos), p.rot);
                    }
                }
                Item::Track(i) => {
                    if let (Some(o), Some(t)) = (g.orig.tracks.get(i), self.board.tracks.get_mut(i)) {
                        (t.a, t.b) = (mv(o.a), mv(o.b));
                    }
                }
                Item::Silk(i) => {
                    if let (Some(o), Some(t)) = (g.orig.silk.get(i), self.board.silk.get_mut(i)) {
                        (t.a, t.b) = (mv(o.a), mv(o.b));
                    }
                }
                Item::Pad(i) => {
                    if let (Some(o), Some(t)) = (g.orig.pads.get(i), self.board.pads.get_mut(i)) {
                        t.pos = mv(o.pos);
                    }
                }
                Item::Via(i) => {
                    if let (Some(o), Some(t)) = (g.orig.vias.get(i), self.board.vias.get_mut(i)) {
                        t.pos = mv(o.pos);
                    }
                }
                Item::Hole(i) => {
                    if let (Some(o), Some(t)) = (g.orig.holes.get(i), self.board.holes.get_mut(i)) {
                        t.pos = mv(o.pos);
                    }
                }
                Item::Zone(i) => {
                    if let (Some(o), Some(t)) = (g.orig.zones.get(i), self.board.zones.get_mut(i)) {
                        t.polygon = o.polygon.iter().map(|p| mv(*p)).collect();
                    }
                }
            }
        }
    }

    /// Delete everything in the multi-selection except parts (those belong to the schematic).
    pub(super) fn delete_multi(&mut self) {
        let items = std::mem::take(&mut self.multi);
        let had_parts = items.iter().any(|i| matches!(i, Item::Part(_)));
        let mut idx: [Vec<usize>; 6] = Default::default();
        for it in &items {
            match *it {
                Item::Track(i) => idx[0].push(i),
                Item::Pad(i) => idx[1].push(i),
                Item::Via(i) => idx[2].push(i),
                Item::Hole(i) => idx[3].push(i),
                Item::Silk(i) => idx[4].push(i),
                Item::Zone(i) => idx[5].push(i),
                Item::Part(_) => {}
            }
        }
        let n: usize = idx.iter().map(Vec::len).sum();
        if n > 0 {
            self.checkpoint();
            for v in idx.iter_mut() {
                v.sort_unstable_by(|a, b| b.cmp(a));
            }
            for &i in &idx[0] { if i < self.board.tracks.len() { self.board.tracks.remove(i); } }
            for &i in &idx[1] { if i < self.board.pads.len() { self.board.pads.remove(i); } }
            for &i in &idx[2] { if i < self.board.vias.len() { self.board.vias.remove(i); } }
            for &i in &idx[3] { if i < self.board.holes.len() { self.board.holes.remove(i); } }
            for &i in &idx[4] { if i < self.board.silk.len() { self.board.silk.remove(i); } }
            for &i in &idx[5] { if i < self.board.zones.len() { self.board.zones.remove(i); } }
        }
        self.selected = None;
        self.status = match (n, had_parts) {
            (0, true) => "部品は回路図側で削除してから「回路図から取り込み」を実行してください".into(),
            (_, true) => format!("{n} 個を削除しました (部品は回路図側で削除します)"),
            _ => format!("{n} 個を削除しました"),
        };
    }

    pub(super) fn delete(&mut self, it: Item) {
        if let Item::Part(_) = it {
            self.status = "部品は回路図側で削除してから「回路図から取り込み」を実行してください".into();
            return;
        }
        self.checkpoint();
        match it {
            Item::Track(i) => drop(self.board.tracks.remove(i)),
            Item::Pad(i) => drop(self.board.pads.remove(i)),
            Item::Via(i) => drop(self.board.vias.remove(i)),
            Item::Hole(i) => drop(self.board.holes.remove(i)),
            Item::Silk(i) => drop(self.board.silk.remove(i)),
            Item::Zone(i) => drop(self.board.zones.remove(i)),
            Item::Part(_) => {}
        }
        self.selected = None;
    }
}
