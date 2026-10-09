//! What a click does for the active tool (parts, pads, polygons, ...).

use super::*;

impl BoardEditor {
    /// Move the selected part to the other side of the board.
    pub(super) fn flip_selected_part(&mut self) {
        if let Some(Item::Part(i)) = self.selected
            && i < self.board.parts.len() {
                self.checkpoint();
                let r = self.board.parts[i].refdes.clone();
                self.board.flip_part(&r);
                let side = if self.board.parts[i].side == Side::Bottom { "裏面" } else { "表面" };
                self.status = format!("{r} を{side}に移しました");
            }
    }

    pub(super) fn finish_polygon(&mut self) {
        let mut poly = std::mem::take(&mut self.poly);
        while poly.len() > 1 && poly[poly.len() - 1].dist(poly[poly.len() - 2]) < 1e-9 {
            poly.pop();
        }
        if poly.len() < 3 {
            self.status = "多角形には3点以上が必要です".into();
            return;
        }
        self.checkpoint();
        match self.tool {
            Tool::OutlinePoly => {
                self.board.outline = poly;
                self.fit_pending = false;
                self.status = "基板外形を設定しました".into();
            }
            Tool::Zone => {
                let mut z = Zone::new(&self.zone_net, self.side, poly);
                z.clearance = self.zone_clearance;
                z.thermal = self.zone_thermal;
                self.board.zones.push(z);
                self.status = format!("ゾーンを追加しました (ネット {})", self.zone_net);
            }
            _ => {}
        }
    }

    pub(super) fn click(&mut self, p: Point) {
        match self.tool {
            Tool::Select => {
                self.multi.clear();
                self.selected = self.hit_test(p);
            }
            Tool::Track => match self.pending.take() {
                None => self.pending = Some(p),
                Some(a) => {
                    self.checkpoint();
                    self.push_route(a, p);
                    self.pending = Some(p); // continue the route
                }
            },
            Tool::Silk => match self.pending.take() {
                None => self.pending = Some(p),
                Some(a) => {
                    self.checkpoint();
                    self.board.silk.push(SilkLine { side: self.side, a, b: p, width: 0.15, refdes: String::new() });
                    self.pending = Some(p);
                }
            },
            Tool::OutlineRect => match self.pending.take() {
                None => self.pending = Some(p),
                Some(a) => {
                    let (x0, x1) = (a.x.min(p.x), a.x.max(p.x));
                    let (y0, y1) = (a.y.min(p.y), a.y.max(p.y));
                    if x1 > x0 && y1 > y0 {
                        self.checkpoint();
                        self.board.set_rect_outline(x0, y0, x1, y1);
                        self.size_w = x1 - x0;
                        self.size_h = y1 - y0;
                    }
                }
            },
            Tool::OutlinePoly | Tool::Zone => {
                if self.poly.len() >= 3 && self.poly[0].dist(p) < 6.0 / self.zoom as f64 {
                    self.finish_polygon();
                } else if self.poly.last().is_none_or(|l| l.dist(p) > 1e-9) {
                    self.poly.push(p);
                }
            }
            Tool::PadTh => {
                self.checkpoint();
                self.board.pads.push(Pad::new(p, PadShape::Circle { d: self.pad_size }, Some(self.pad_drill), self.side));
            }
            Tool::PadSmd => {
                self.checkpoint();
                self.board.pads.push(Pad::new(p, PadShape::Rect { w: self.pad_size, h: self.pad_size * 0.7 }, None, self.side));
            }
            Tool::Via => {
                self.checkpoint();
                self.board.vias.push(Via { pos: p, diameter: self.via_dia, drill: self.via_drill });
            }
            Tool::Hole => {
                self.checkpoint();
                self.board.holes.push(Hole { pos: p, drill: self.hole_drill });
            }
        }
    }
}
