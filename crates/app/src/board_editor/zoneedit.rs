//! Editing the shapes of the selected pour with the pointer: drag a corner or the middle point of an arc, double click on an edge to put a
//! corner there, Delete takes the picked corner away, and the right click opens a menu (corner away, line or arc for a piece, a corner here).

use super::*;
use pcb_core::model::ShapeOp;

/// A corner of a shape of a pour, or the point an arc passes through.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct VertexRef {
    pub zone: usize,
    pub shape: usize,
    pub vert: usize,
    /// The middle point of the arc that starts at the corner (not the corner itself).
    pub mid: bool,
}

/// Where the right click landed: on a corner, or on an edge (the piece that starts at `vert`, and the point on it).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum EditTarget {
    Corner(VertexRef),
    Edge { zone: usize, shape: usize, vert: usize, at: Point },
}

impl BoardEditor {
    /// The corner or arc middle point of the selected pour that is under `w` (within 7 pixels), if the pour is shown and can be picked.
    pub(super) fn zone_handle_at(&self, w: Point) -> Option<VertexRef> {
        let Some(Item::Zone(zi)) = self.selected else { return None };
        let z = self.board.zones.get(zi)?;
        if !self.vis.zones || !self.side_visible(z.side) || !self.pick.zones {
            return None;
        }
        let tol = 7.0 / self.zoom as f64;
        let mut best: Option<(f64, VertexRef)> = None;
        for (si, zs) in z.shapes.iter().enumerate() {
            for (vi, v) in zs.shape.verts.iter().enumerate() {
                // a corner wins over the middle point of an arc that lies on it
                for (mid, p) in [(false, Some(v.p)), (true, v.arc_mid)] {
                    let Some(p) = p else { continue };
                    let d = p.dist(w) + if mid { 1e-6 } else { 0.0 };
                    if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, VertexRef { zone: zi, shape: si, vert: vi, mid }));
                    }
                }
            }
        }
        best.map(|(_, h)| h)
    }

    /// The edge of a shape of the selected pour under `w` (within 6 pixels): (pour, shape, piece, the point on it).
    pub(super) fn zone_edge_at(&self, w: Point) -> Option<(usize, usize, usize, Point)> {
        let Some(Item::Zone(zi)) = self.selected else { return None };
        let z = self.board.zones.get(zi)?;
        if !self.vis.zones || !self.side_visible(z.side) || !self.pick.zones {
            return None;
        }
        let tol = 6.0 / self.zoom as f64;
        let mut best: Option<(f64, usize, usize, Point)> = None;
        for (si, zs) in z.shapes.iter().enumerate() {
            if let Some((vi, at, d)) = zs.shape.nearest_edge(w)
                && d <= tol
                && best.is_none_or(|(bd, ..)| d < bd)
            {
                best = Some((d, si, vi, at));
            }
        }
        best.map(|(_, si, vi, at)| (zi, si, vi, at))
    }

    /// Begin dragging a corner or an arc middle point (the board is saved for undo once).
    pub(super) fn start_vertex_drag(&mut self, h: VertexRef) {
        self.checkpoint();
        self.vdrag = Some(h);
        self.vsel = Some(h);
    }

    /// Move the dragged point to `p`.
    pub(super) fn drag_vertex(&mut self, p: Point) {
        let Some(h) = self.vdrag else { return };
        if let Some(v) = self.board.zones.get_mut(h.zone).and_then(|z| z.shapes.get_mut(h.shape)).and_then(|zs| zs.shape.verts.get_mut(h.vert)) {
            if h.mid {
                v.arc_mid = Some(p);
            } else {
                v.p = p;
            }
        }
    }

    /// Put a corner on the edge at `at`.
    pub(super) fn insert_zone_vertex(&mut self, zone: usize, shape: usize, vert: usize, at: Point) {
        self.checkpoint();
        if let Some(zs) = self.board.zones.get_mut(zone).and_then(|z| z.shapes.get_mut(shape)) {
            zs.shape.insert_vertex(vert, at);
            self.vsel = Some(VertexRef { zone, shape, vert: vert + 1, mid: false });
            self.status = "辺に頂点を足しました (ドラッグで動かせます。Delete で消せます)".into();
        }
    }

    /// Take the picked corner away.
    pub(super) fn delete_picked_vertex(&mut self) -> bool {
        let Some(h) = self.vsel.filter(|h| !h.mid) else { return false };
        self.vsel = None;
        let can = self.board.zones.get(h.zone).and_then(|z| z.shapes.get(h.shape)).is_some_and(|zs| {
            let mut t = zs.shape.clone();
            t.remove_vertex(h.vert)
        });
        if !can {
            self.status = "この頂点は消せません (図形が、面でなくなります)。図形ごと消すには、ベタの図形の一覧の「消す」を使います".into();
            return true;
        }
        self.checkpoint();
        if let Some(zs) = self.board.zones.get_mut(h.zone).and_then(|z| z.shapes.get_mut(h.shape)) {
            zs.shape.remove_vertex(h.vert);
        }
        self.status = "頂点を消しました".into();
        true
    }

    /// What the right click at `w` would act on: a corner, or an edge of a shape of the selected pour.
    pub(super) fn zone_edit_target(&self, w: Point) -> Option<EditTarget> {
        if let Some(h) = self.zone_handle_at(w) {
            return Some(EditTarget::Corner(h));
        }
        self.zone_edge_at(w).map(|(zone, shape, vert, at)| EditTarget::Edge { zone, shape, vert, at })
    }

    /// The menu of the right click on a corner or an edge of a shape of the selected pour.
    pub(super) fn zone_edit_menu(&mut self, ui: &mut egui::Ui) {
        let Some(target) = self.vmenu else { return };
        let (zone, shape, vert) = match target {
            EditTarget::Corner(h) => (h.zone, h.shape, h.vert),
            EditTarget::Edge { zone, shape, vert, .. } => (zone, shape, vert),
        };
        let Some(zs) = self.board.zones.get(zone).and_then(|z| z.shapes.get(shape)) else { return };
        let is_arc = zs.shape.verts.get(vert).is_some_and(|v| v.arc_mid.is_some());
        let n = zs.shape.verts.len();
        ui.label(match target {
            EditTarget::Corner(_) => format!("図形 {} の頂点 {}", shape + 1, vert + 1),
            EditTarget::Edge { .. } => format!("図形 {} の辺 {}", shape + 1, vert + 1),
        });
        if let EditTarget::Edge { at, .. } = target
            && ui.button("ここに頂点を足す").clicked()
        {
            self.insert_zone_vertex(zone, shape, vert, at);
            ui.close();
        }
        if let EditTarget::Corner(h) = target
            && !h.mid
            && ui.add_enabled(n > 3 || is_arc && n > 2, egui::Button::new("この頂点を消す")).clicked()
        {
            self.vsel = Some(h);
            self.delete_picked_vertex();
            ui.close();
        }
        let label = if is_arc { "この辺(この頂点から次の頂点まで)を直線にする" } else { "この辺(この頂点から次の頂点まで)を円弧にする" };
        if ui.button(label).clicked() {
            self.checkpoint();
            if let Some(zs) = self.board.zones.get_mut(zone).and_then(|z| z.shapes.get_mut(shape)) {
                if is_arc {
                    zs.shape.make_line(vert);
                } else {
                    zs.shape.make_arc(vert);
                }
            }
            ui.close();
        }
        let op = self.board.zones[zone].shapes[shape].op;
        if ui.button(if op == ShapeOp::Add { "この図形を、引く図形にする" } else { "この図形を、足す図形にする" }).clicked() {
            self.checkpoint();
            self.board.zones[zone].shapes[shape].op = if op == ShapeOp::Add { ShapeOp::Sub } else { ShapeOp::Add };
            ui.close();
        }
    }
}
