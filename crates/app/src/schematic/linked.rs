//! The pick made in the board or the 3D view, shown on the sheet: a ring round the part and the wires of the net in the colour of
//! the link (`crate::link`). And the other way: the part or net picked on the sheet, as a `Focus`.

use super::*;
use crate::link::Focus;

const LINK: Color32 = Color32::from_rgb(0, 170, 190);

impl SchEditor {
    /// What is picked on the sheet: the parts (not power symbols, not sheet symbols; several when several are picked), or the net of
    /// one wire or label. On a copy of a child sheet the parts are named as the board knows them (`R1_CH1`, `sch_core::flat_refdes`).
    pub fn focus(&self) -> Focus {
        let path = self.current_path();
        let name = |i: usize| self.sch.components.get(i).filter(|c| !c.kind.is_power_symbol() && c.sub.is_none()).map(|c| sch_core::flat_refdes(&c.refdes, &path));
        if !self.multi.is_empty() {
            let parts = self.multi.iter().filter_map(|s| if let Sel::Comp(i) = s { name(*i) } else { None }).collect();
            return Focus::of_parts(parts);
        }
        match self.selected {
            Some(Sel::Comp(i)) => Focus::of_parts(name(i).into_iter().collect()),
            Some(Sel::Wire(i)) => self.sch.wires.get(i).and_then(|w| net_at(&self.sch, w.a)).map(|n| Focus { net: Some(n), ..Default::default() }).unwrap_or_default(),
            Some(Sel::Label(i)) => self.sch.labels.get(i).and_then(|l| net_at(&self.sch, l.pos)).map(|n| Focus { net: Some(n), ..Default::default() }).unwrap_or_default(),
            _ => Focus::default(),
        }
    }

    /// The copy of the sheet being edited (the symbols it is reached through): the one it was opened through, else its first copy;
    /// empty when it is not a copy of a child sheet.
    pub(super) fn current_path(&self) -> Vec<String> {
        if self.sch.sheets.is_empty() {
            return vec![];
        }
        let copies = self.sch.copies_of(self.sch.active_sheet);
        if copies.contains(&self.sheet_path) { self.sheet_path.clone() } else { copies.into_iter().next().unwrap_or_default() }
    }

    /// The part picked elsewhere (the board, the 3D view) is on another sheet, or in another copy of this one: open that sheet (and
    /// copy) so the ring shows it. Returns whether the sheet changed.
    pub fn reveal_linked(&mut self) -> bool {
        let Some(r) = self.linked.part.clone() else { return false };
        let path = self.current_path();
        if self.sch.components.iter().any(|c| c.sub.is_none() && sch_core::flat_refdes(&c.refdes, &path) == r) {
            return false;
        }
        let Some(place) = self.sch.locate_part(&r) else { return false };
        self.show_sheet(place.sheet);
        self.sheet_trail.clear();
        self.path_trail.clear();
        self.sheet_path = place.path.clone();
        let copy = if place.path.is_empty() { String::new() } else { i18n::trf!(" ({} の中)", place.path.join(" → ")) };
        self.status = i18n::trf!("基板で選んだ {r} のあるシート「{}」{copy}を開きました", self.sch.sheet_name(place.sheet));
        true
    }

    /// The wires of the linked net, worked out again only when the sheet or the net changes.
    fn linked_wires(&self) -> Vec<usize> {
        let Some(net) = self.linked.net.as_deref() else { return vec![] };
        let key = (net.to_string(), self.rev, self.sch.wires.len());
        let mut cache = self.linked_cache.borrow_mut();
        if cache.0 != key {
            let wires = self.sch.wires.iter().enumerate().filter(|(_, w)| net_at(&self.sch, w.a).as_deref() == Some(net)).map(|(i, _)| i).collect();
            *cache = (key, wires);
        }
        cache.1.clone()
    }

    /// The ring and the wires of what was picked elsewhere (drawn over the wires and parts).
    pub(super) fn draw_linked(&self, painter: &egui::Painter, rect: Rect) {
        if self.linked.is_empty() {
            return;
        }
        let at = |p: Pt| self.to_screen(rect, (p.0 as f32, p.1 as f32));
        for i in self.linked_wires() {
            let w = &self.sch.wires[i];
            painter.line_segment([at(w.a), at(w.b)], Stroke::new(4.5, LINK.gamma_multiply(0.55)));
        }
        // a ring round every part picked there (as the board names them: `R1_CH1` on a copy of a child sheet)
        let path = self.current_path();
        for c in self.sch.components.iter().filter(|c| c.sub.is_none() && self.linked.parts().any(|r| r == sch_core::flat_refdes(&c.refdes, &path))) {
            let ext = comp_extents(c);
            let (x0, x1, y0, y1) = sheet_box(c.kind, ext, c.pos, c.rot).unwrap_or((c.pos.0 as f32 - 2.0, c.pos.0 as f32 + 2.0, c.pos.1 as f32 - 1.0, c.pos.1 as f32 + 1.0));
            let r = Rect::from_min_max(self.to_screen(rect, (x0, y0)), self.to_screen(rect, (x1, y1))).expand(6.0);
            painter.rect_stroke(r, 4.0, Stroke::new(2.5, LINK), egui::StrokeKind::Outside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wires_of_the_linked_net_are_found_and_a_wire_pick_gives_its_net() {
        let mut e = SchEditor::default();
        let s = &mut e.sch;
        s.add(Kind::VSource, (0, 0), 0);
        s.add(Kind::Resistor, (6, -6), 0);
        s.add(Kind::Resistor, (8, 0), 1);
        s.add(Kind::Ground, (0, 6), 0);
        let r2 = s.components[2].pins();
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), r2[0].1), (r2[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 6))] {
            s.wires.push(Wire { a, b });
        }
        // the net between R1 and R2 is the wire from R1's right pin down to R2's top pin
        let net = net_at(&e.sch, (8, -6)).unwrap();
        e.linked = Focus { net: Some(net.clone()), ..Default::default() };
        assert_eq!(e.linked_wires(), vec![2]);
        // picking that wire gives the net back as a focus
        e.selected = Some(Sel::Wire(2));
        assert_eq!(e.focus(), Focus { net: Some(net), ..Default::default() });
        // a part (not a power symbol) gives its reference; several things picked give none
        e.selected = Some(Sel::Comp(1));
        assert_eq!(e.focus().part.as_deref(), Some("R1"));
        e.selected = Some(Sel::Comp(3));
        assert!(e.focus().is_empty(), "the ground symbol is not a part");
        // several parts picked: all of them (the power symbol left out)
        e.multi = vec![Sel::Comp(1), Sel::Comp(3), Sel::Comp(2)];
        let f = e.focus();
        assert_eq!(f.parts().collect::<Vec<_>>(), ["R1", "R2"]);
        e.multi = vec![Sel::Comp(3)];
        assert!(e.focus().is_empty());
    }
}
