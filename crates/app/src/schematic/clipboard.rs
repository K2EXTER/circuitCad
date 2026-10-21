//! Copy, cut and paste of a selection of the sheet. The copy lives in the editor (`clipboard`), so it can also be
//! pasted into another circuit that is opened afterwards. A copied part brings the net labels that sit on its pins (they
//! are part of how it is wired); a pasted part gets a new reference designator.

use super::*;

impl SchEditor {
    /// The selection as indices: parts, wires, labels (the single selection counts too).
    fn selection_indices(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let (mut comps, mut wires, mut labels) = (vec![], vec![], vec![]);
        let mut all: Vec<Sel> = self.multi.clone();
        all.extend(self.selected);
        for s in all {
            match s {
                Sel::Comp(i) => comps.push(i),
                Sel::Wire(i) => wires.push(i),
                Sel::Label(i) => labels.push(i),
            }
        }
        (comps, wires, labels)
    }

    pub(super) fn has_selection(&self) -> bool {
        self.selected.is_some() || !self.multi.is_empty()
    }

    /// Copy the selection. False (and a word on the status line) when nothing is selected.
    pub(super) fn copy_selection(&mut self) -> bool {
        let (comps, wires, mut labels) = self.selection_indices();
        if comps.is_empty() && wires.is_empty() && labels.is_empty() {
            self.status = "コピーするものを選んでください (部品や線をクリックするか、範囲をドラッグで囲みます)".into();
            return false;
        }
        for &i in &comps {
            labels.extend(self.attached_labels(i));
        }
        self.clipboard = self.sch.copy_fragment(&comps, &wires, &labels);
        let f = &self.clipboard;
        self.status = format!("コピーしました: 部品 {}・線 {}・ラベル {}。Ctrl+V で貼り付けます", f.components.len(), f.wires.len(), f.labels.len());
        true
    }

    pub(super) fn cut_selection(&mut self) {
        if self.copy_selection() {
            self.delete_selected();
        }
    }

    /// Put the copy on the sheet, near where it came from but on free ground, and select it so that it can be dragged.
    pub(super) fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            self.status = "貼り付けるものがありません。部品や線を選んで、先に Ctrl+C でコピーしてください".into();
            return;
        }
        let Some((dx, dy)) = self.paste_offset() else {
            self.status = "貼り付けられる空きが見つかりませんでした。シートを広く使うか、先にほかの部品を動かしてください".into();
            return;
        };
        let p = self.sch.paste_fragment(&self.clipboard, dx, dy);
        let mut now: Vec<Sel> = p.components.iter().map(|&i| Sel::Comp(i)).collect();
        now.extend(p.wires.iter().map(|&i| Sel::Wire(i)));
        now.extend(p.labels.iter().map(|&i| Sel::Label(i)));
        self.status = format!("貼り付けました: 部品 {}・線 {}・ラベル {} (ドラッグで動かせます。部品番号は新しくなっています)", p.components.len(), p.wires.len(), p.labels.len());
        if now.len() == 1 {
            self.selected = now.pop();
            self.multi.clear();
        } else {
            self.selected = None;
            self.multi = now;
        }
    }

    /// Copy the selection and paste it at once.
    pub(super) fn duplicate_selection(&mut self) {
        if self.copy_selection() {
            self.paste_clipboard();
        }
    }

    /// The nearest move (grid steps, down and to the right first) at which the copy lies on no part and touches nothing on
    /// the sheet: no pin, wire end or label of the copy on one of the sheet (that would join nets by accident).
    fn paste_offset(&self) -> Option<(i32, i32)> {
        let f = &self.clipboard;
        let mut taken: Vec<Pt> = self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        taken.extend(self.sch.wires.iter().flat_map(|w| [w.a, w.b]));
        taken.extend(self.sch.labels.iter().map(|l| l.pos));
        let free = |dx: i32, dy: i32| {
            !f.components.iter().any(|c| self.would_overlap(c.kind, comp_extents(c), (c.pos.0 + dx, c.pos.1 + dy), c.rot, &[]))
                && !f.contact_points(dx, dy).iter().any(|p| taken.contains(p))
        };
        for d in 1..=80i32 {
            let mut ring: Vec<(i32, i32)> = vec![];
            for dx in -d..=d {
                for dy in -d..=d {
                    if dx.abs().max(dy.abs()) == d {
                        ring.push((dx, dy));
                    }
                }
            }
            // to the lower right first, then the sides, the upper left last; nearer first
            ring.sort_by_key(|&(dx, dy)| ((dx < 0) as i32 + (dy < 0) as i32, dx.abs() + dy.abs()));
            if let Some(&(dx, dy)) = ring.iter().find(|&&(dx, dy)| free(dx, dy)) {
                return Some((dx, dy));
            }
        }
        None
    }
}
