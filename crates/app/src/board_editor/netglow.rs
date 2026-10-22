//! Seeing one net at a glance: the net under the pointer glows (Select tool), a click pins it, Esc lets it go. The big pours also
//! carry their net name, so two pours of one colour are told apart by their names.

use super::*;
use pcb_core::geom::inside_polygon;
use pcb_core::zonefill::FillShape;

const PINNED: Color32 = Color32::from_rgb(255, 214, 60);
const HOVER: Color32 = Color32::from_rgb(120, 225, 255);

fn inside_shape(s: &FillShape, p: Point) -> bool {
    inside_polygon(&s.outer, p) && !s.holes.iter().any(|h| inside_polygon(h, p))
}

impl BoardEditor {
    /// The net of the copper at `p` on a layer that is shown: a pad, a via, a track, then a pour (its copper, not its outline).
    pub(super) fn net_under(&self, p: Point) -> Option<String> {
        let tol = 3.0 / self.zoom as f64;
        let conn = self.cached_connectivity();
        for (j, pd) in self.board.pads.iter().enumerate() {
            let (w, h) = pd.shape.size();
            if (pd.drill.is_some() || self.side_visible(pd.side)) && (p.x - pd.pos.x).abs() <= w / 2.0 + tol && (p.y - pd.pos.y).abs() <= h / 2.0 + tol {
                return conn.pad_net(j).map(str::to_string).filter(|n| !n.is_empty());
            }
        }
        for (k, v) in self.board.vias.iter().enumerate() {
            if p.dist(v.pos) <= v.diameter / 2.0 + tol {
                return conn.via_net(k).map(str::to_string);
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.side_visible(t.side)) {
            if seg_dist(p, t.a, t.b) <= t.width / 2.0 + tol {
                return conn.track_net(i).map(str::to_string);
            }
        }
        let cache = self.zone_cache.borrow();
        let (_, fills) = cache.as_ref()?;
        // the working layer's pour first
        let mut order: Vec<&(Side, String, Vec<FillShape>)> = fills.iter().filter(|(s, n, _)| !n.is_empty() && self.side_visible(*s)).collect();
        order.sort_by_key(|(s, _, _)| *s != self.side);
        order.into_iter().find(|(_, _, shapes)| shapes.iter().any(|s| inside_shape(s, p))).map(|(_, n, _)| n.clone())
    }

    /// Work out which net the pointer is on (Select tool, no drag going on), and pin it on a click.
    pub(super) fn update_net_focus(&mut self, resp: &egui::Response, rect: Rect) {
        self.hover_net = None;
        if self.tool != Tool::Select || self.alt_pan || self.boxsel.is_some() || self.gdrag.is_some() || resp.dragged() {
            return;
        }
        let Some(pos) = resp.hover_pos() else { return };
        let net = self.net_under(self.to_world(rect, pos));
        if resp.clicked_by(egui::PointerButton::Primary) {
            // a click on the pinned net lets it go, on another net pins that one, on nothing lets go
            self.pinned_net = net.clone().filter(|n| self.pinned_net.as_ref() != Some(n));
        }
        self.hover_net = net.map(|n| (n, pos));
    }

    /// The glow of the pinned net and the net under the pointer, over the copper.
    pub(super) fn draw_net_focus(&self, painter: &egui::Painter, rect: Rect) {
        let hover = self.hover_net.as_ref().filter(|(n, _)| self.pinned_net.as_ref() != Some(n));
        let nets: Vec<(&str, Color32)> =
            hover.map(|(n, _)| (n.as_str(), HOVER)).into_iter().chain(self.pinned_net.as_deref().map(|n| (n, PINNED))).collect();
        if nets.is_empty() {
            return;
        }
        let conn = self.cached_connectivity();
        let z = self.zoom;
        for (net, col) in nets {
            let glow = col.gamma_multiply(0.55);
            for (i, t) in self.board.tracks.iter().enumerate().filter(|(i, t)| self.side_visible(t.side) && conn.track_net(*i) == Some(net)) {
                let _ = i;
                painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((t.width as f32 * z).max(1.0) + 5.0, glow));
            }
            for (j, p) in self.board.pads.iter().enumerate().filter(|(j, p)| conn.pad_net(*j) == Some(net) && (p.drill.is_some() || self.side_visible(p.side))) {
                let _ = j;
                let (w, h) = p.shape.size();
                painter.rect_stroke(
                    Rect::from_center_size(self.to_screen(rect, p.pos), Vec2::new(w as f32 * z + 6.0, h as f32 * z + 6.0)),
                    0.0,
                    Stroke::new(2.5, col),
                    egui::StrokeKind::Outside,
                );
            }
            for (k, v) in self.board.vias.iter().enumerate().filter(|(k, _)| conn.via_net(*k) == Some(net)) {
                let _ = k;
                painter.circle_stroke(self.to_screen(rect, v.pos), v.diameter as f32 * z / 2.0 + 3.0, Stroke::new(2.5, col));
            }
            if let Some((_, fills)) = self.zone_cache.borrow().as_ref() {
                for (side, _, shapes) in fills.iter().filter(|(s, n, _)| n == net && self.side_visible(*s)) {
                    let _ = side;
                    for shape in shapes {
                        let mut mesh = egui::Mesh::default();
                        for t in shape.triangles() {
                            let base = mesh.vertices.len() as u32;
                            for p in t {
                                mesh.colored_vertex(self.to_screen(rect, p), col.gamma_multiply(0.22));
                            }
                            mesh.add_triangle(base, base + 1, base + 2);
                        }
                        painter.add(egui::Shape::mesh(mesh));
                        for contour in std::iter::once(&shape.outer).chain(shape.holes.iter()) {
                            let pts: Vec<Pos2> = contour.iter().map(|p| self.to_screen(rect, *p)).collect();
                            painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, col)));
                        }
                    }
                }
            }
        }
        if let Some((n, pos)) = hover {
            let galley = painter.layout_no_wrap(n.clone(), egui::FontId::proportional(14.0), Color32::BLACK);
            let at = *pos + Vec2::new(14.0, -26.0);
            painter.rect_filled(Rect::from_min_size(at, galley.size() + Vec2::new(10.0, 4.0)), 3.0, HOVER);
            painter.galley(at + Vec2::new(5.0, 2.0), galley, Color32::BLACK);
        }
    }

    /// "固定中: BAT" under the layer badge, while a net is pinned.
    pub(super) fn draw_pin_badge(&self, painter: &egui::Painter, rect: Rect) {
        let Some(n) = &self.pinned_net else { return };
        let galley = painter.layout_no_wrap(format!("ネット {n} を固定中  (クリックか Esc で解除)"), egui::FontId::proportional(14.0), Color32::BLACK);
        let at = rect.min + Vec2::new(30.0, 56.0);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + Vec2::new(16.0, 8.0)), 4.0, PINNED);
        painter.galley(at + Vec2::new(8.0, 4.0), galley, Color32::BLACK);
    }

    /// The name of a pour in its piece (one name per piece that is large enough on the screen).
    pub(super) fn draw_pour_name(&self, painter: &egui::Painter, rect: Rect, net: &str, shape: &FillShape) {
        if net.is_empty() || !self.vis.nets {
            return;
        }
        let pts: Vec<Pos2> = shape.outer.iter().map(|p| self.to_screen(rect, *p)).collect();
        let bb = Rect::from_points(&pts);
        if bb.width() < 70.0 || bb.height() < 28.0 || !bb.intersects(rect) {
            return;
        }
        let Some(t) = shape.triangles().into_iter().max_by(|a, b| pcb_core::geom::signed_area(a).abs().total_cmp(&pcb_core::geom::signed_area(b).abs())) else { return };
        let c = Point::new((t[0].x + t[1].x + t[2].x) / 3.0, (t[0].y + t[1].y + t[2].y) / 3.0);
        painter.text(self.to_screen(rect, c), egui::Align2::CENTER_CENTER, net, egui::FontId::proportional(15.0), Color32::from_white_alpha(120));
    }
}
