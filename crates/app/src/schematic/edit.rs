//! Selection, hit testing and edit operations on the sheet.

use super::*;

impl SchEditor {
    /// Selects the part with this number (for scripts and screenshots). False if there is none.
    pub fn select_part(&mut self, refdes: &str) -> bool {
        match self.sch.components.iter().position(|c| c.refdes == refdes) {
            Some(i) => {
                self.set_selection(vec![Sel::Comp(i)]);
                self.panel_tab = PanelTab::Draw;
                true
            }
            None => false,
        }
    }

    /// Opens the new-part form (for scripts and screenshots).
    /// Switches the open new-part form to a tab (`--form-tab <name>`).
    pub fn set_form_tab(&mut self, name: &str) {
        if let Some(f) = self.form.as_mut() {
            f.show_tab(name);
        }
    }

    /// Reads a STEP file into the open new-part form (`--form-step <file>`).
    pub fn load_step_into_form(&mut self, path: &std::path::Path) {
        if let Some(f) = self.form.as_mut() {
            f.load_step(path);
        }
    }

    /// Opens the new-part form already set to a kind of part (`--new-part <kind>`).
    pub fn open_part_form_for(&mut self, kind: &str) {
        self.form = Some(PartForm::for_type(kind));
        self.panel_tab = PanelTab::Draw;
    }

    pub(super) fn comp_at(&self, g: (f32, f32)) -> Option<usize> {
        self.sch.components.iter().enumerate().rev().find_map(|(i, c)| {
            let rel = (g.0 - c.pos.0 as f32, g.1 - c.pos.1 as f32);
            let l = rot_f(rel, (4 - c.rot % 4) % 4);
            let (x0, x1, y0, y1) = comp_extents(c);
            (l.0 >= x0 && l.0 <= x1 && l.1 >= y0 && l.1 <= y1).then_some(i)
        })
    }

    pub(super) fn pick(&self, g: (f32, f32)) -> Option<Sel> {
        if let Some(i) = self.comp_at(g) {
            return Some(Sel::Comp(i));
        }
        if let Some(i) = self.sch.labels.iter().position(|l| (l.pos.0 as f32 - g.0).abs() < 0.8 && (l.pos.1 as f32 - g.1).abs() < 0.6) {
            return Some(Sel::Label(i));
        }
        let d = |w: &Wire| {
            let (a, b) = ((w.a.0 as f32, w.a.1 as f32), (w.b.0 as f32, w.b.1 as f32));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 { 0.0 } else { (((g.0 - a.0) * dx + (g.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
            ((g.0 - a.0 - t * dx).powi(2) + (g.1 - a.1 - t * dy).powi(2)).sqrt()
        };
        self.sch.wires.iter().position(|w| d(w) < 0.3).map(Sel::Wire)
    }

    pub(super) fn is_sel(&self, s: Sel) -> bool {
        self.selected == Some(s) || self.multi.contains(&s)
    }

    pub(super) fn set_selection(&mut self, items: Vec<Sel>) {
        if items.len() == 1 {
            self.selected = Some(items[0]);
            self.multi.clear();
        } else {
            self.selected = None;
            self.multi = items;
        }
    }

    /// Objects touched by the box (grid coordinates). `crossing`: any overlap counts, otherwise
    /// the object must lie completely inside.
    pub(super) fn objects_in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Sel> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let mut out = vec![];
        for (i, c) in self.sch.components.iter().enumerate() {
            let (x0, x1, y0, y1) = comp_extents(c);
            let pts: Vec<(f32, f32)> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .map(|&p| {
                    let w = rot_f(p, c.rot % 4);
                    (w.0 + c.pos.0 as f32, w.1 + c.pos.1 as f32)
                })
                .collect();
            let (bl, bh) = (
                (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min), pts.iter().map(|p| p.1).fold(f32::MAX, f32::min)),
                (pts.iter().map(|p| p.0).fold(f32::MIN, f32::max), pts.iter().map(|p| p.1).fold(f32::MIN, f32::max)),
            );
            let hit = if crossing { bl.0 <= hi.0 && bh.0 >= lo.0 && bl.1 <= hi.1 && bh.1 >= lo.1 } else { rect_contains(lo, hi, bl) && rect_contains(lo, hi, bh) };
            if hit {
                out.push(Sel::Comp(i));
            }
        }
        for (i, w) in self.sch.wires.iter().enumerate() {
            let (pa, pb) = ((w.a.0 as f32, w.a.1 as f32), (w.b.0 as f32, w.b.1 as f32));
            let hit = if crossing { segment_hits_rect(pa, pb, lo, hi) } else { rect_contains(lo, hi, pa) && rect_contains(lo, hi, pb) };
            if hit {
                out.push(Sel::Wire(i));
            }
        }
        for (i, l) in self.sch.labels.iter().enumerate() {
            if rect_contains(lo, hi, (l.pos.0 as f32, l.pos.1 as f32)) {
                out.push(Sel::Label(i));
            }
        }
        out
    }

    /// Would a part of this kind, placed at `pos` with `rot`, overlap another part? `ignore` lists
    /// parts that are being moved themselves.
    pub(super) fn would_overlap(&self, kind: Kind, ext: (f32, f32, f32, f32), pos: Pt, rot: u8, ignore: &[usize]) -> bool {
        let Some(b) = sheet_box(kind, ext, pos, rot) else { return false };
        self.sch.components.iter().enumerate().any(|(j, o)| {
            !ignore.contains(&j) && sheet_box(o.kind, comp_extents(o), o.pos, o.rot).is_some_and(|ob| boxes_overlap(b, ob))
        })
    }

    /// Rotate a part a quarter turn unless that would put it on top of another part.
    pub(super) fn rotate_comp(&mut self, i: usize) {
        let c = &self.sch.components[i];
        let next = (c.rot + 1) % 4;
        if self.would_overlap(c.kind, comp_extents(c), c.pos, next, &[i]) {
            self.status = "回すと他の部品と重なるので、回せません".into();
        } else {
            // the labels set on the pins turn with the part: each goes to where its pin ends up
            let before = self.sch.components[i].pins();
            let carried: Vec<(usize, String)> = self
                .attached_labels(i)
                .into_iter()
                .filter_map(|l| before.iter().find(|p| p.1 == self.sch.labels[l].pos).map(|p| (l, p.0.clone())))
                .collect();
            self.sch.components[i].rot = next;
            let after = self.sch.components[i].pins();
            for (l, pin) in carried {
                if let Some(p) = after.iter().find(|p| p.0 == pin) {
                    self.sch.labels[l].pos = p.1;
                }
            }
            // wires ending on the pins follow them
            let moved: Vec<(Pt, Pt)> = before.iter().filter_map(|b| after.iter().find(|a| a.0 == b.0).map(|a| (b.1, a.1))).collect();
            self.sch.wires = super::stretch::stretch_wires(&self.sch.wires, &moved, &[]);
        }
    }

    /// Net labels that sit on a pin of part `i` and on no wire end: they belong to the part and move with it.
    pub(super) fn attached_labels(&self, i: usize) -> Vec<usize> {
        let pins: Vec<Pt> = self.sch.components[i].pins().into_iter().map(|p| p.1).collect();
        let on_wire = |p: Pt| self.sch.wires.iter().any(|w| w.a == p || w.b == p);
        self.sch.labels.iter().enumerate().filter(|(_, l)| pins.contains(&l.pos) && !on_wire(l.pos)).map(|(k, _)| k).collect()
    }

    pub(super) fn start_group_drag(&mut self, anchor: Pt) {
        let mut g = GroupDrag { anchor, comps: vec![], wires: vec![], labels: vec![] };
        for s in &self.multi {
            match *s {
                Sel::Comp(i) => g.comps.push((i, self.sch.components[i].pos)),
                Sel::Wire(i) => g.wires.push((i, self.sch.wires[i].a, self.sch.wires[i].b)),
                Sel::Label(i) => g.labels.push((i, self.sch.labels[i].pos)),
            }
        }
        // the labels set on the pins of the moved parts go along, unless they are selected themselves
        for s in &self.multi {
            if let Sel::Comp(i) = *s {
                for l in self.attached_labels(i) {
                    if !g.labels.iter().any(|(k, _)| *k == l) {
                        g.labels.push((l, self.sch.labels[l].pos));
                    }
                }
            }
        }
        let comps: Vec<usize> = g.comps.iter().map(|c| c.0).collect();
        self.begin_stretch(&comps);
        self.gdrag = Some(g);
    }

    pub(super) fn delete_selected(&mut self) {
        if !self.multi.is_empty() {
            let mut comps: Vec<usize> = vec![];
            let (mut wires, mut labels) = (vec![], vec![]);
            for s in self.multi.drain(..) {
                match s {
                    Sel::Comp(i) => comps.push(i),
                    Sel::Wire(i) => wires.push(i),
                    Sel::Label(i) => labels.push(i),
                }
            }
            for (v, n) in [(&mut comps, 0), (&mut wires, 1), (&mut labels, 2)] {
                v.sort_unstable_by(|a, b| b.cmp(a));
                for &i in v.iter() {
                    match n {
                        0 if i < self.sch.components.len() => drop(self.sch.components.remove(i)),
                        1 if i < self.sch.wires.len() => drop(self.sch.wires.remove(i)),
                        2 if i < self.sch.labels.len() => drop(self.sch.labels.remove(i)),
                        _ => {}
                    }
                }
            }
            self.selected = None;
            return;
        }
        match self.selected.take() {
            Some(Sel::Comp(i)) => drop(self.sch.components.remove(i)),
            Some(Sel::Wire(i)) => drop(self.sch.wires.remove(i)),
            Some(Sel::Label(i)) => drop(self.sch.labels.remove(i)),
            None => {}
        }
    }

    pub(super) fn add_wire(&mut self, a: Pt, b: Pt) {
        if a == b {
            return;
        }
        if a.0 == b.0 || a.1 == b.1 {
            self.sch.wires.push(Wire { a, b });
        } else {
            let m = (b.0, a.1);
            self.sch.wires.push(Wire { a, b: m });
            self.sch.wires.push(Wire { a: m, b });
        }
    }
}
