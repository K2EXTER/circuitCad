//! What a click does for the active tool (parts, pads, polygons, ...).

use super::*;

impl BoardEditor {
    /// Move the selected part to the other side of the board.
    pub(super) fn flip_selected_part(&mut self) {
        if let Some(Item::Part(i)) = self.selected
            && i < self.board.parts.len() {
                if self.board.is_locked(&self.board.parts[i].refdes) {
                    self.status = "この部品は固定されています。固定を外すと付け替えられます (選択パネルのボタン)".into();
                    return;
                }
                self.checkpoint();
                let r = self.board.parts[i].refdes.clone();
                self.board.flip_part(&r);
                let side = if self.board.parts[i].side == Side::Bottom { "裏面" } else { "表面" };
                self.status = format!("{r} を{side}に移しました");
            }
    }

    pub(super) fn finish_polygon(&mut self) {
        let mut poly = std::mem::take(&mut self.poly);
        self.arc_mid = None;
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
            Tool::ZoneHole => {
                let net = Some(self.hole_net.as_str()).filter(|n| !n.is_empty());
                let n = pcb_core::zonefill::add_zone_hole(&mut self.board, self.side, net, &poly);
                let which = net.map_or("この層のベタ".to_string(), |n| format!("ネット {n} のベタ"));
                self.status = if n > 0 {
                    format!("{which} {n} 枚に、銅を抜く穴を開けました (ベタを選ぶと、設定の「穴を消す」で戻せます)")
                } else {
                    format!("{which}が、描いた範囲にありません。穴を開けるベタの上に描いてください (層は、いま作業している面です)")
                };
            }
            Tool::Zone => {
                let mut z = Zone::new(&self.zone_net, self.side, poly);
                z.clearance = self.zone_clearance;
                z.thermal = self.zone_thermal;
                z.remove_islands = self.zone_remove_islands;
                self.board.zones.push(z);
                let last = self.board.zones.len() - 1;
                let (joined, _) = pcb_core::zonefill::merge_zone_outline(&mut self.board, last);
                self.status = if joined > 0 {
                    format!("ゾーンを追加し、重なる同じネットのゾーン {joined} 枚と1つにまとめました (ネット {})", self.zone_net)
                } else {
                    format!("ゾーンを追加しました (ネット {})", self.zone_net)
                };
            }
            _ => {}
        }
    }

    /// The direction the drawn outline leaves its last point in (for a tangent arc), if it has two points.
    fn poly_tangent(&self) -> Option<(f64, f64)> {
        let n = self.poly.len();
        (n >= 2).then(|| (self.poly[n - 1].x - self.poly[n - 2].x, self.poly[n - 1].y - self.poly[n - 2].y))
    }

    /// The points a click at `p` would add in the arc modes (the arc, or the straight line when there is no arc).
    fn arc_to(&self, p: Point) -> Vec<Point> {
        let Some(&last) = self.poly.last() else { return vec![p] };
        let arc = match self.poly_mode {
            PolyMode::ArcThrough => self.arc_mid.and_then(|m| pcb_core::outline::arc_through(last, m, p)),
            PolyMode::ArcTangent => self.poly_tangent().and_then(|t| pcb_core::outline::arc_tangent(last, t, p)),
            _ => None,
        };
        arc.unwrap_or_else(|| vec![p])
    }

    /// The outline of the circle or rectangle being drawn from `self.pending` to `p`.
    fn shape_to(&self, p: Point) -> Option<Vec<Point>> {
        let a = self.pending?;
        match self.poly_mode {
            PolyMode::Circle => {
                let r = a.dist(p);
                (r > 1e-6).then(|| pcb_core::outline::ellipse(a.x - r, a.y - r, a.x + r, a.y + r))
            }
            PolyMode::Rect => ((a.x - p.x).abs() > 1e-6 && (a.y - p.y).abs() > 1e-6).then(|| pcb_core::outline::rounded_rect(a.x, a.y, p.x, p.y, self.shape_round)),
            _ => None,
        }
    }

    /// What is drawn from the last point to the cursor `p` for the picture while the polygon is drawn: the line, the arc or the shape.
    pub(super) fn polygon_preview(&self, p: Point) -> Vec<Point> {
        match self.poly_mode {
            PolyMode::Lines => self.poly.last().map(|l| vec![*l, p]).unwrap_or_default(),
            PolyMode::ArcThrough if self.arc_mid.is_none() => self.poly.last().map(|l| vec![*l, p]).unwrap_or_default(),
            PolyMode::ArcThrough | PolyMode::ArcTangent => {
                let mut v: Vec<Point> = self.poly.last().copied().into_iter().collect();
                v.extend(self.arc_to(p));
                v
            }
            PolyMode::Circle | PolyMode::Rect => {
                let mut v = self.shape_to(p).unwrap_or_default();
                if let Some(first) = v.first().copied() {
                    v.push(first);
                }
                v
            }
        }
    }

    /// A click while a polygon is drawn: it adds a corner, an arc, or finishes a circle or a rectangle. A click on the first point closes it.
    fn polygon_click(&mut self, p: Point) {
        let near_first = self.poly.len() >= 3 && self.poly[0].dist(p) < 6.0 / self.zoom as f64;
        match self.poly_mode {
            PolyMode::Lines => {
                if near_first {
                    self.finish_polygon();
                } else if self.poly.last().is_none_or(|l| l.dist(p) > 1e-9) {
                    self.poly.push(p);
                }
            }
            PolyMode::ArcThrough => {
                if self.poly.is_empty() {
                    self.poly.push(p);
                } else if self.arc_mid.is_none() {
                    self.arc_mid = Some(p);
                    self.status = "円弧: 終点をクリック (途中の点を通る円弧になります)".into();
                } else {
                    let end = if near_first { self.poly[0] } else { p };
                    let pts = self.arc_to(end);
                    self.arc_mid = None;
                    self.poly.extend(pts);
                    if near_first {
                        self.finish_polygon();
                    }
                }
            }
            PolyMode::ArcTangent => {
                if self.poly.is_empty() {
                    self.poly.push(p);
                } else if near_first {
                    let pts = self.arc_to(self.poly[0]);
                    self.poly.extend(pts);
                    self.finish_polygon();
                } else {
                    let pts = self.arc_to(p);
                    self.poly.extend(pts);
                }
            }
            PolyMode::Circle | PolyMode::Rect => {
                if self.pending.is_none() {
                    self.pending = Some(p);
                    self.status = if self.poly_mode == PolyMode::Circle { "円: 円周の点をクリック (半径が決まります)".into() } else { "長方形: 反対側の角をクリック".into() };
                } else if let Some(shape) = self.shape_to(p) {
                    self.pending = None;
                    self.poly = shape;
                    self.finish_polygon();
                }
            }
        }
    }

    pub(super) fn click(&mut self, p: Point) {
        match self.tool {
            Tool::Select => {
                self.multi.clear();
                self.selected = self.hit_test(p);
                if let Some(Item::Label(i)) = self.selected
                    && let Some(r) = self.board.texts.get(i).map(|t| t.refdes.clone()) {
                        self.status = format!("{r} のシルク文字: ドラッグで動かせます。Delete で、元の位置に戻ります");
                    }
            }
            Tool::Track if self.trunk_mode => self.trunk_click(p),
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
                    self.board.silk.push(SilkLine { side: self.side.outer(), a, b: p, width: 0.15, refdes: String::new() });
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
                        self.set_outline_box(x0, y0, x1, y1);
                        self.size_w = x1 - x0;
                        self.size_h = y1 - y0;
                    }
                }
            },
            Tool::OutlinePoly | Tool::Zone | Tool::ZoneHole => self.polygon_click(p),
            Tool::PadTh => {
                self.checkpoint();
                self.board.pads.push(Pad::new(p, PadShape::Circle { d: self.pad_size }, Some(self.pad_drill), self.side.outer()));
            }
            Tool::PadSmd => {
                self.checkpoint();
                self.board.pads.push(Pad::new(p, PadShape::Rect { w: self.pad_size, h: self.pad_size * 0.7 }, None, self.side.outer()));
            }
            Tool::Via => {
                self.checkpoint();
                // the via is for a net: the one chosen in the tool's settings, or else the net of what it is put on
                let net = self.via_net_at(p);
                self.status = if net.is_empty() { "ビアを置きました (ネットなし。配線でつなぐと、そのネットになります)".into() } else { format!("ビアを置きました (ネット {net})") };
                self.board.vias.push(Via { pos: p, diameter: self.via_dia, drill: self.via_drill, net });
            }
            Tool::Hole => {
                self.checkpoint();
                self.board.holes.push(Hole::new(p, self.hole_drill));
            }
        }
    }
}
