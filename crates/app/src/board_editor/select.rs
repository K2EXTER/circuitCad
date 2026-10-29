//! Hit testing, selection, moving and deleting items.

use super::*;

impl BoardEditor {
    pub(super) fn hit_test(&self, p: Point) -> Option<Item> {
        let tol = 6.0 / self.zoom as f64;
        let pick = self.pick;
        // what is on the other layer is out of reach while "作業中の面だけ" is on
        let side_ok = |s: Side| !pick.active_side_only || s == self.side;
        for (i, v) in self.board.vias.iter().enumerate() {
            if pick.vias && self.vis.vias && p.dist(v.pos) <= v.diameter / 2.0 + tol {
                return Some(Item::Via(i));
            }
        }
        for (i, pd) in self.board.pads.iter().enumerate().filter(|(_, pd)| (pd.drill.is_some() || self.side_visible(pd.side)) && (self.vis.parts || pd.refdes.is_empty())) {
            let (w, h) = pd.shape.size();
            if (p.x - pd.pos.x).abs() <= w / 2.0 + tol && (p.y - pd.pos.y).abs() <= h / 2.0 + tol {
                if !pd.refdes.is_empty()
                    && let Some(k) = self.board.parts.iter().position(|b| b.refdes == pd.refdes) {
                        let part = &self.board.parts[k];
                        let through = self.board.pads.iter().any(|q| q.refdes == part.refdes && q.drill.is_some());
                        if pick.parts && (side_ok(part.side) || through) {
                            return Some(Item::Part(k));
                        }
                        continue;
                    }
                if pick.pads && (pd.drill.is_some() || side_ok(pd.side)) {
                    return Some(Item::Pad(i));
                }
            }
        }
        // a part's own holes belong to the part: they are not picked on their own
        for (i, h) in self.board.holes.iter().enumerate().filter(|(_, h)| h.refdes.is_empty()) {
            if pick.pads && p.dist(h.pos) <= h.drill / 2.0 + tol {
                return Some(Item::Hole(i));
            }
        }
        // a part's designator on the silkscreen (above the tracks: a track under the text is still reachable from beside it)
        for (i, t) in self.board.texts.iter().enumerate().filter(|(_, t)| !t.refdes.is_empty() && self.vis.parts && self.side_visible(t.side)) {
            let (w, h) = t.size();
            if pick.silk && self.vis.silk && side_ok(t.side) && (p.x - t.pos.x).abs() <= w / 2.0 + tol && (p.y - t.pos.y).abs() <= h / 2.0 + tol {
                return Some(Item::Label(i));
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.side_visible(t.side)) {
            if pick.tracks && self.vis.tracks && side_ok(t.side) && seg_dist(p, t.a, t.b) <= t.width / 2.0 + tol {
                return Some(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty()) {
            if pick.silk && self.vis.silk && side_ok(l.side) && seg_dist(p, l.a, l.b) <= l.width / 2.0 + tol {
                return Some(Item::Silk(i));
            }
        }
        // Pours of different nets overlap (a small one inside a big GND pour): an outline under the
        // pointer wins, otherwise the smallest pour containing it, so the inner one stays pickable.
        let mut edge: Option<(usize, f64)> = None;
        let mut inner: Option<(usize, f64)> = None;
        for (i, z) in self.board.zones.iter().enumerate().filter(|(_, z)| self.vis.zones && self.side_visible(z.side)) {
            if !pick.zones || !side_ok(z.side) {
                continue;
            }
            // the outline under the pointer: the edge of any shape of the pour
            let d = z.shapes.iter().map(|zs| polygon_edge_dist(&zs.shape.flatten(), p)).fold(f64::MAX, f64::min);
            if d <= tol {
                if edge.is_none_or(|(_, best)| d < best) {
                    edge = Some((i, d));
                }
            } else if z.covers(p) {
                let area: f64 = z.shapes.iter().filter(|zs| zs.op == pcb_core::model::ShapeOp::Add).map(|zs| { let f = zs.shape.flatten(); f.iter().zip(f.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>().abs() / 2.0 }).sum();
                if inner.is_none_or(|(_, best)| area < best) {
                    inner = Some((i, area));
                }
            }
        }
        edge.or(inner).map(|(i, _)| Item::Zone(i))
    }

    /// Everything that is selected, one or many.
    pub(super) fn selection_items(&self) -> Vec<Item> {
        let mut all = self.multi.clone();
        for it in self.selected {
            if !all.contains(&it) {
                all.push(it);
            }
        }
        all
    }

    /// Ctrl+click and Ctrl+rubber band: an object that is selected leaves the selection, one that is not joins it.
    pub(super) fn toggle_in_selection(&mut self, items: &[Item]) {
        let mut all = self.selection_items();
        for &it in items {
            if let Some(k) = all.iter().position(|x| *x == it) {
                all.remove(k);
            } else if self.pick.allows(&it) {
                all.push(it);
            }
        }
        self.status = format!("{} 個を選択", all.len());
        self.set_selection(all);
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
        for (i, v) in self.board.vias.iter().enumerate().filter(|_| self.vis.vias) {
            if rect_hit(v.pos, v.diameter, v.diameter) {
                out.push(Item::Via(i));
            }
        }
        for (i, h) in self.board.holes.iter().enumerate().filter(|(_, h)| h.refdes.is_empty()) {
            if rect_hit(h.pos, h.drill, h.drill) {
                out.push(Item::Hole(i));
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.vis.tracks && self.side_visible(t.side)) {
            if seg_hit(t.a, t.b, t.width) {
                out.push(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty() && self.vis.silk && self.side_visible(l.side)) {
            if seg_hit(l.a, l.b, l.width) {
                out.push(Item::Silk(i));
            }
        }
        for (i, z) in self.board.zones.iter().enumerate().filter(|(_, z)| self.vis.zones && self.side_visible(z.side)) {
            let outline = z.outline();
            let n = outline.len();
            let hit = if crossing {
                (0..n).any(|k| segment_hits_rect(outline[k], outline[(k + 1) % n], lo, hi))
            } else {
                outline.iter().all(|p| rect_contains(lo, hi, *p))
            };
            if n >= 3 && hit {
                out.push(Item::Zone(i));
            }
        }
        // the lock of the other side is for clicks; a rubber band takes both sides unless `box_both_sides` is off
        out.retain(|it| self.pick.allows(it) && (self.box_both_sides || self.on_pickable_side(it)));
        out
    }

    /// With "作業中の面だけ" on: is the object on the layer being worked on, or through the board?
    fn on_pickable_side(&self, it: &Item) -> bool {
        if !self.pick.active_side_only {
            return true;
        }
        let side = match *it {
            Item::Track(i) => self.board.tracks.get(i).map(|t| t.side),
            Item::Silk(i) => self.board.silk.get(i).map(|l| l.side),
            Item::Label(i) => self.board.texts.get(i).map(|t| t.side),
            Item::Zone(i) => self.board.zones.get(i).map(|z| z.side),
            Item::Pad(i) => self.board.pads.get(i).filter(|p| p.drill.is_none()).map(|p| p.side),
            Item::Part(i) => self.board.parts.get(i).filter(|p| !self.board.pads.iter().any(|q| q.refdes == p.refdes && q.drill.is_some())).map(|p| p.side),
            Item::Via(_) | Item::Hole(_) => None,
        };
        side.is_none_or(|s| s == self.side)
    }

    /// Is this item a part that was fixed in place?
    pub(super) fn is_locked_item(&self, it: &Item) -> bool {
        matches!(*it, Item::Part(i) if self.board.parts.get(i).is_some_and(|p| self.board.is_locked(&p.refdes)))
    }

    /// Points to ring when an object is selected.
    /// Is the thing drawn at all (its layer and its kind are shown)? What is hidden gets no selection mark or glow.
    pub(super) fn item_shown(&self, it: Item) -> bool {
        match it {
            Item::Part(i) => self.vis.parts && self.board.parts.get(i).is_none_or(|p| self.side_visible(p.side) || self.board.pads.iter().any(|q| q.refdes == p.refdes && q.drill.is_some())),
            Item::Track(i) => self.vis.tracks && self.board.tracks.get(i).is_none_or(|t| self.side_visible(t.side)),
            Item::Via(_) => self.vis.vias,
            Item::Pad(i) => self.board.pads.get(i).is_none_or(|p| (p.drill.is_some() || self.side_visible(p.side)) && (self.vis.parts || p.refdes.is_empty())),
            Item::Silk(i) => self.vis.silk && self.board.silk.get(i).is_none_or(|s| self.side_visible(s.side)),
            Item::Label(i) => self.vis.silk && self.board.texts.get(i).is_none_or(|t| self.side_visible(t.side) && (self.vis.parts || t.refdes.is_empty())),
            Item::Zone(i) => self.vis.zones && self.board.zones.get(i).is_none_or(|z| self.side_visible(z.side)),
            _ => true,
        }
    }

    pub(super) fn item_points(&self, it: Item) -> Vec<Point> {
        match it {
            Item::Part(i) if i < self.board.parts.len() => {
                let r = &self.board.parts[i].refdes;
                self.board.pads.iter().filter(|p| &p.refdes == r).map(|p| p.pos).collect()
            }
            Item::Track(i) if i < self.board.tracks.len() => vec![self.board.tracks[i].a, self.board.tracks[i].b],
            Item::Silk(i) if i < self.board.silk.len() => vec![self.board.silk[i].a, self.board.silk[i].b],
            Item::Label(i) if i < self.board.texts.len() => vec![self.board.texts[i].pos],
            Item::Pad(i) if i < self.board.pads.len() => vec![self.board.pads[i].pos],
            Item::Via(i) if i < self.board.vias.len() => vec![self.board.vias[i].pos],
            Item::Hole(i) if i < self.board.holes.len() => vec![self.board.holes[i].pos],
            Item::Zone(i) if i < self.board.zones.len() => self.board.zones[i].outline(),
            _ => vec![],
        }
    }

    /// Move the grouped objects to `orig` shifted by (dx, dy).
    pub(super) fn move_group(&mut self, g: &GroupDrag, dx: f64, dy: f64) {
        let mv = |p: Point| Point::new(p.x + dx, p.y + dy);
        for it in &g.items {
            match *it {
                Item::Part(i) => {
                    if let Some(p) = g.orig.parts.get(i).filter(|p| !g.orig.is_locked(&p.refdes)) {
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
                        // every shape moves by what the first corner moved by
                        if let Some(first) = o.outline().first().copied() {
                            let to = mv(first);
                            let d = Point::new(to.x - first.x, to.y - first.y);
                            t.shapes = o.shapes.iter().map(|zs| pcb_core::model::ZoneShape { shape: zs.shape.moved(d), op: zs.op }).collect();
                        }
                    }
                }
                Item::Label(i) => {
                    // the text goes from where it was when the drag began; a text whose part moves in the same drag moves with
                    // the part already (its offset is kept), so only a text moved on its own gets here with a new place
                    if let Some(o) = g.orig.texts.get(i)
                        && !g.items.iter().any(|x| matches!(*x, Item::Part(k) if g.orig.parts.get(k).is_some_and(|p| p.refdes == o.refdes)))
                    {
                        let refdes = o.refdes.clone();
                        // restore the offset the drag began with, then move by the whole drag
                        match g.orig.label_offsets.get(&refdes) {
                            Some(off) => drop(self.board.label_offsets.insert(refdes.clone(), *off)),
                            None => drop(self.board.label_offsets.remove(&refdes)),
                        }
                        if let Some(t) = self.board.texts.iter_mut().find(|t| t.refdes == refdes) {
                            t.pos = o.pos;
                        }
                        self.board.move_label(&refdes, mv(o.pos));
                    }
                }
            }
        }
    }

    /// Turn the grouped objects a quarter turn about the middle of the group (the centre of the box round them).
    /// Parts keep their designator's place relative to them; fixed parts stay where they are. Returns how many objects turned.
    pub(super) fn rotate_group(&mut self, items: &[Item]) -> usize {
        let pts: Vec<Point> = items.iter().flat_map(|it| self.item_points(*it)).collect();
        if pts.is_empty() {
            return 0;
        }
        let (mut lo, mut hi) = (pts[0], pts[0]);
        for p in &pts {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        // a round centre keeps four turns exactly where they began
        let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
        let c = Point::new(r3((lo.x + hi.x) / 2.0), r3((lo.y + hi.y) / 2.0));
        let turn = |p: Point| Point::new(c.x - (p.y - c.y), c.y + (p.x - c.x));
        let mut n = 0;
        for it in items {
            match *it {
                Item::Part(i) => {
                    let Some(p) = self.board.parts.get(i).filter(|p| !self.board.is_locked(&p.refdes)).cloned() else { continue };
                    if let Some(off) = self.board.label_offsets.get_mut(&p.refdes) {
                        *off = Point::new(-off.y, off.x);
                    }
                    self.board.place_part(&p.refdes, turn(p.pos), (p.rot + 1) % 4);
                }
                Item::Track(i) => {
                    let Some(t) = self.board.tracks.get_mut(i) else { continue };
                    (t.a, t.b) = (turn(t.a), turn(t.b));
                }
                Item::Silk(i) => {
                    let Some(t) = self.board.silk.get_mut(i).filter(|t| t.refdes.is_empty()) else { continue };
                    (t.a, t.b) = (turn(t.a), turn(t.b));
                }
                Item::Pad(i) => {
                    let Some(t) = self.board.pads.get_mut(i).filter(|t| t.refdes.is_empty()) else { continue };
                    t.pos = turn(t.pos);
                    t.shape = match t.shape {
                        pcb_core::PadShape::Rect { w, h } => pcb_core::PadShape::Rect { w: h, h: w },
                        pcb_core::PadShape::Oval { w, h } => pcb_core::PadShape::Oval { w: h, h: w },
                        s => s,
                    };
                    t.slot_v = !t.slot_v;
                }
                Item::Via(i) => {
                    let Some(t) = self.board.vias.get_mut(i) else { continue };
                    t.pos = turn(t.pos);
                }
                Item::Hole(i) => {
                    let Some(t) = self.board.holes.get_mut(i).filter(|t| t.refdes.is_empty()) else { continue };
                    t.pos = turn(t.pos);
                    t.slot_v = !t.slot_v;
                }
                Item::Zone(i) => {
                    let Some(z) = self.board.zones.get_mut(i) else { continue };
                    for zs in z.shapes.iter_mut() {
                        zs.shape = zs.shape.rotated90(c);
                    }
                }
                Item::Label(i) => {
                    // a designator whose part turns with the group goes with the part
                    let Some(t) = self.board.texts.get(i) else { continue };
                    let refdes = t.refdes.clone();
                    let with_part = items.iter().any(|x| matches!(*x, Item::Part(k) if self.board.parts.get(k).is_some_and(|p| p.refdes == refdes)));
                    if with_part {
                        continue;
                    }
                    let to = turn(t.pos);
                    self.board.move_label(&refdes, to);
                    self.board.turn_label(&refdes);
                }
            }
            n += 1;
        }
        n
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
                Item::Part(_) | Item::Label(_) => {}
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
        // a designator is not deleted (it belongs to its part): Delete puts it back where it was placed automatically
        if let Item::Label(i) = it {
            if let Some(r) = self.board.texts.get(i).map(|t| t.refdes.clone()) {
                self.checkpoint();
                self.status = if self.board.reset_label(&r) { format!("{r} の文字を、元の位置に戻しました") } else { format!("{r} の文字は、もとの位置にあります") };
            }
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
            Item::Part(_) | Item::Label(_) => {}
        }
        self.selected = None;
    }
}
