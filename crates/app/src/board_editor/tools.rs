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
            Tool::OutlinePoly | Tool::Zone | Tool::ZoneHole => {
                if self.poly.len() >= 3 && self.poly[0].dist(p) < 6.0 / self.zoom as f64 {
                    self.finish_polygon();
                } else if self.poly.last().is_none_or(|l| l.dist(p) > 1e-9) {
                    self.poly.push(p);
                }
            }
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
