//! Painting the board: grid, zones, copper, highlights and overlays.

use super::*;

impl BoardEditor {
    pub(super) fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        if self.grid > 0.0 && self.grid as f32 * self.zoom >= 6.0 {
            let tl = self.to_world(rect, rect.left_top());
            let br = self.to_world(rect, rect.right_bottom());
            // a zoomed-out view of a big board would need tens of thousands of dots: show every 2nd, 3rd ... point
            let (nx, ny) = ((br.x - tl.x) / self.grid, (tl.y - br.y) / self.grid);
            let m = (nx * ny / 5000.0).sqrt().ceil().max(1.0);
            let grid = self.grid * m;
            let (mut x, y_end) = ((tl.x / grid).floor() * grid, (br.y / grid).floor() * grid);
            // one mesh of little squares: thousands of separate circles cost a frame in tessellation alone
            let mut dots = egui::epaint::Mesh::default();
            while x <= br.x {
                let mut y = (tl.y / grid).ceil() * grid;
                while y >= y_end {
                    dots.add_colored_rect(Rect::from_center_size(self.to_screen(rect, Point::new(x, y)), egui::Vec2::splat(1.6)), Color32::from_gray(60));
                    y -= grid;
                }
                x += grid;
            }
            painter.add(egui::Shape::mesh(dots));
        }
    }

    /// The pours (with their triangles) and the islands, for the board as it is. The first time they are worked out here; after a change
    /// they are worked out on another thread (50 ms or more on a board with guards) and the last ones stay on the screen until the new
    /// ones are in, so that drawing a track with the pours shown does not stop at each corner. While parts or a group are dragged they stay as they were.
    fn refresh_zone_caches(&self, ctx: &egui::Context, conn: Option<&drc::Connectivity>) {
        let key = pcb_core::zonefill::geometry_key(&self.board);
        let make = |board: &pcb_core::Board, conn: &drc::Connectivity| {
            let pours: Vec<PourCache> = pcb_core::zonefill::merged_fills(board, conn)
                .into_iter()
                .map(|(s, n, shapes)| {
                    let tris = shapes.iter().map(|sh| sh.triangles()).collect();
                    (s, n, shapes, tris)
                })
                .collect();
            (pours, pcb_core::zonefill::dropped_islands(board, conn))
        };
        // an answer that came in
        let mut done = None;
        {
            let mut job = self.zone_job.borrow_mut();
            if let Some((jk, rx)) = job.as_ref() {
                match rx.try_recv() {
                    Ok(r) => {
                        done = Some((*jk, r));
                        *job = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(30)),
                    Err(_) => *job = None,
                }
            }
        }
        if let Some((k, (pours, islands))) = done {
            *self.zone_cache.borrow_mut() = Some((k, pours));
            *self.island_cache.borrow_mut() = Some((k, islands));
        }
        let have = self.zone_cache.borrow().as_ref().map(|(k, _)| *k);
        if have == Some(key) || (have.is_some() && self.slow_results_kept()) {
            return;
        }
        if have.is_none() {
            let fresh;
            let conn = match conn {
                Some(c) => c,
                None => {
                    fresh = self.cached_connectivity();
                    &*fresh
                }
            };
            let (pours, islands) = make(&self.board, conn);
            *self.zone_cache.borrow_mut() = Some((key, pours));
            *self.island_cache.borrow_mut() = Some((key, islands));
            return;
        }
        // the board changed since the last answer: a new one is asked for, unless one is on its way (it is asked for again when it is in)
        if self.zone_job.borrow().is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let board = self.board.clone();
            std::thread::spawn(move || {
                let conn = drc::connectivity(&board);
                let _ = tx.send(make(&board, &conn));
            });
            *self.zone_job.borrow_mut() = Some((key, rx));
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    /// The outlines of the pours (for each pour, each shape: the outer line, then the holes) thinned to the zoom; kept until the pours or the
    /// level of detail change.
    pub(super) fn pour_outlines(&self, key: u64, fills: &[PourCache]) -> std::rc::Rc<Vec<Vec<Vec<Vec<Point>>>>> {
        // a one pixel line can be half a pixel off the fill
        let level = ribbon::lod_level_px(self.zoom, 0.5);
        if let Some((k, l, v)) = self.zone_outline_lod.borrow().as_ref()
            && *k == key
            && *l == level
        {
            return v.clone();
        }
        let tol = ribbon::lod_tolerance(level);
        let thin = |c: &Vec<Point>| if tol > 0.0 { ribbon::simplify_closed(c, tol) } else { c.clone() };
        let v: Vec<Vec<Vec<Vec<Point>>>> =
            fills.iter().map(|(_, _, shapes, _)| shapes.iter().map(|sh| std::iter::once(&sh.outer).chain(sh.holes.iter()).map(thin).collect()).collect()).collect();
        let v = std::rc::Rc::new(v);
        *self.zone_outline_lod.borrow_mut() = Some((key, level, v.clone()));
        v
    }

    /// Copper pours as the fab will get them: cut to the board outline, cleared around other nets and the
    /// edge (thermal spokes not drawn). The shapes come from the same code as the Gerber output.
    pub(super) fn draw_zones(&self, painter: &egui::Painter, rect: Rect, conn: Option<&drc::Connectivity>) {
        if !self.vis.zones || self.board.zones.is_empty() {
            return;
        }
        self.refresh_zone_caches(painter.ctx(), conn);
        let cache = self.zone_cache.borrow();
        let Some((key, fills)) = cache.as_ref() else { return };
        let outlines = self.pour_outlines(*key, fills);
        let area = rect.expand(4.0);
        for s in self.layer_order() {
            if !self.side_visible(s) {
                continue;
            }
            let alpha = if s == self.side { 90 } else { 40 };
            let fill = self.side_color(s).gamma_multiply(alpha as f32 / 255.0);
            let edge = self.side_color(s).gamma_multiply(if s == self.side { 0.8 } else { 0.35 });
            let mut edges = egui::Mesh::default();
            for (fi, (_, net, shapes, tris)) in fills.iter().enumerate().filter(|(_, (side, _, _, _))| *side == s) {
                for (si, (shape, tris)) in shapes.iter().zip(tris).enumerate() {
                    if !self.shape_on_screen(rect, shape) {
                        continue;
                    }
                    // only what is on the screen: a big pour is one shape, and its triangles and its many small holes (the clearance
                    // round each pad) mostly lie outside the view when zoomed in, or are a pixel or two wide when zoomed out
                    let mut mesh = egui::Mesh::default();
                    for t in tris {
                        let s = [self.to_screen(rect, t[0]), self.to_screen(rect, t[1]), self.to_screen(rect, t[2])];
                        if !Rect::from_points(&s).intersects(area) {
                            continue;
                        }
                        let base = mesh.vertices.len() as u32;
                        for q in s {
                            mesh.colored_vertex(q, fill);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    painter.add(egui::Shape::mesh(mesh));
                    // the lines are the outlines thinned to the zoom (a corner that is under half a pixel off the line is dropped), all
                    // of a layer in one mesh of thin quads
                    for (k, contour) in outlines[fi][si].iter().enumerate() {
                        let pts: Vec<Pos2> = contour.iter().map(|p| self.to_screen(rect, *p)).collect();
                        let bb = Rect::from_points(&pts);
                        if !bb.intersects(area) || (k > 0 && bb.width().max(bb.height()) < 3.0) {
                            continue;
                        }
                        ribbon::add_hairline_ring(&mut edges, &pts, edge);
                        if edges.vertices.len() > 60_000 {
                            painter.add(egui::Shape::mesh(std::mem::take(&mut edges)));
                        }
                    }
                    self.draw_pour_name(painter, rect, net, shape, tris);
                }
            }
            painter.add(egui::Shape::mesh(edges));
        }
    }

    /// The pieces of the pours that are dropped as islands, in a faint grey with a dashed outline (they are not in the copper that is made).
    pub(super) fn draw_islands(&self, painter: &egui::Painter, rect: Rect, conn: Option<&drc::Connectivity>) {
        if conn.is_none() {
            return;
        }
        if !self.vis.zones || self.board.zones.is_empty() {
            return;
        }
        let cache = self.island_cache.borrow();
        let Some((_, islands)) = cache.as_ref() else { return };
        let grey = Color32::from_rgba_unmultiplied(170, 170, 170, 160);
        for (side, _net, shapes) in islands.iter().filter(|(s, _, _)| self.side_visible(*s)) {
            let _ = side;
            for shape in shapes {
                if !self.shape_on_screen(rect, shape) {
                    continue;
                }
                let pts: Vec<Pos2> = shape.outer.iter().map(|p| self.to_screen(rect, *p)).collect();
                if pts.len() < 3 {
                    continue;
                }
                let mut closed = pts.clone();
                closed.push(pts[0]);
                painter.extend(egui::Shape::dashed_line(&closed, Stroke::new(1.2, grey), 6.0, 4.0));
            }
        }
    }

    /// The shapes of the selected pour: the outline of each, solid for the ones that add and dashed red for the ones that take away, a dot at
    /// each corner and a diamond on the point each arc passes through. Shown while the pour is selected, so the shapes can be seen (and later edited).
    pub(super) fn draw_zone_shapes(&self, painter: &egui::Painter, rect: Rect) {
        let Some(Item::Zone(i)) = self.selected else { return };
        let Some(z) = self.board.zones.get(i) else { return };
        if !self.vis.zones || !self.side_visible(z.side) {
            return;
        }
        self.draw_picked_vertex(painter, rect);
        for zs in &z.shapes {
            let adds = zs.op == pcb_core::model::ShapeOp::Add;
            let col = if adds { self.side_color(z.side) } else { Color32::from_rgb(255, 90, 90) };
            let pts: Vec<Pos2> = zs.shape.flatten().iter().map(|p| self.to_screen(rect, *p)).collect();
            if pts.len() < 2 {
                continue;
            }
            let mut closed = pts.clone();
            closed.push(pts[0]);
            if adds {
                painter.add(egui::Shape::line(closed, Stroke::new(2.0, col)));
            } else {
                painter.extend(egui::Shape::dashed_line(&closed, Stroke::new(2.0, col), 8.0, 5.0));
            }
            for v in &zs.shape.verts {
                painter.circle_filled(self.to_screen(rect, v.p), 3.5, col);
                if let Some(m) = v.arc_mid {
                    let c = self.to_screen(rect, m);
                    painter.add(egui::Shape::convex_polygon(vec![c + Vec2::new(0.0, -4.5), c + Vec2::new(4.5, 0.0), c + Vec2::new(0.0, 4.5), c + Vec2::new(-4.5, 0.0)], Color32::WHITE, Stroke::new(1.0, col)));
                }
            }
        }
    }

    /// A ring round the corner of a shape that is picked.
    fn draw_picked_vertex(&self, painter: &egui::Painter, rect: Rect) {
        let Some(h) = self.vsel else { return };
        let Some(v) = self.board.zones.get(h.zone).and_then(|z| z.shapes.get(h.shape)).and_then(|zs| zs.shape.verts.get(h.vert)) else { return };
        let p = if h.mid { v.arc_mid } else { Some(v.p) };
        if let Some(p) = p {
            painter.circle_stroke(self.to_screen(rect, p), 8.0, Stroke::new(2.0, Color32::WHITE));
        }
    }

    /// Is any of the points (grown by `margin` mm) in the drawing area? What is not is left out of the picture.
    pub(super) fn near_screen(&self, rect: Rect, pts: &[Point], margin: f64) -> bool {
        let m = margin as f32 * self.zoom + 4.0;
        let area = rect.expand(m);
        let (mut lo, mut hi) = (Pos2::new(f32::MAX, f32::MAX), Pos2::new(f32::MIN, f32::MIN));
        for p in pts {
            let s = self.to_screen(rect, *p);
            lo = Pos2::new(lo.x.min(s.x), lo.y.min(s.y));
            hi = Pos2::new(hi.x.max(s.x), hi.y.max(s.y));
        }
        Rect::from_min_max(lo, hi).intersects(area)
    }

    fn shape_on_screen(&self, rect: Rect, shape: &pcb_core::zonefill::FillShape) -> bool {
        self.near_screen(rect, &shape.outer, 0.0)
    }

    pub(super) fn draw_copper(&self, painter: &egui::Painter, rect: Rect) {
        let z = self.zoom;
        let o = &self.board.outline;
        for i in 0..o.len() {
            painter.line_segment([self.to_screen(rect, o[i]), self.to_screen(rect, o[(i + 1) % o.len()])], Stroke::new(2.0, Color32::YELLOW));
            // round joints, so a curve made of many short pieces does not show its corners
            painter.circle_filled(self.to_screen(rect, o[i]), 1.0, Color32::YELLOW);
        }
        // with the finish on, the copper is drawn as it will be made
        let finished = self.finished_async(painter.ctx());
        // the tracks that join end to end are drawn as one strip (see `ribbon`), the polygons of the finish as one mesh a layer
        if self.vis.tracks {
            let tracks = finished.as_ref().map_or(self.board.tracks.as_slice(), |f| f.board.tracks.as_slice());
            {
                let mut cache = self.copper_cache.borrow_mut();
                if !cache.as_ref().is_some_and(|c| c.is_for(tracks, finished.as_ref())) {
                    *cache = Some(ribbon::CopperCache::new(tracks, finished.as_ref()));
                }
            }
            let cache = self.copper_cache.borrow();
            let copper = cache.as_ref().expect("made above");
            let lod = copper.lod(z);
            // inactive layer first, so the active one is on top
            for s in self.layer_order() {
                if !self.side_visible(s) {
                    continue;
                }
                let alpha = if s == self.side { 255 } else { 60 };
                let col = self.side_color(s).gamma_multiply(alpha as f32 / 255.0);
                let mut mesh = egui::Mesh::default();
                // a mesh of more than 60000 corners is sent in pieces
                let flush = |mesh: &mut egui::Mesh| {
                    if mesh.vertices.len() > 60_000 {
                        painter.add(egui::Shape::mesh(std::mem::take(mesh)));
                    }
                };
                for (ch, pts) in copper.chains.iter().zip(&lod.chains).filter(|(c, _)| c.side == s) {
                    let (lo, hi) = (self.to_screen(rect, ch.lo), self.to_screen(rect, ch.hi));
                    if !ribbon::on_screen(rect, lo, hi, ch.width as f32 * z / 2.0 + 4.0) {
                        continue;
                    }
                    let pts: Vec<Pos2> = pts.iter().map(|p| self.to_screen(rect, *p)).collect();
                    ribbon::add_chain(&mut mesh, &pts, ch.width as f32 * z, col);
                    flush(&mut mesh);
                }
                for (patch, tris) in copper.patches.iter().zip(&lod.patches).filter(|(p, _)| p.side == s) {
                    if !ribbon::on_screen(rect, self.to_screen(rect, patch.lo), self.to_screen(rect, patch.hi), 4.0) {
                        continue;
                    }
                    for t in tris {
                        let base = mesh.vertices.len() as u32;
                        for p in t {
                            mesh.colored_vertex(self.to_screen(rect, *p), col);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    flush(&mut mesh);
                }
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        for p in &self.board.pads {
            let visible = p.drill.is_some() && (self.vis.top || self.vis.bottom) || p.drill.is_none() && self.side_visible(p.side);
            if !visible || (!self.vis.parts && !p.refdes.is_empty()) {
                continue;
            }
            let c = self.to_screen(rect, p.pos);
            // a surface-mount pad on the layer that is not being worked on is shown dim
            let col = self.side_color(p.side).gamma_multiply(if p.drill.is_some() || p.side == self.side { 0.9 } else { 0.35 });
            let (w, h) = p.shape.size();
            let sz = Vec2::new(w as f32 * z, h as f32 * z);
            match p.shape {
                PadShape::Circle { .. } => {
                    painter.circle_filled(c, sz.x / 2.0, col);
                }
                PadShape::Rect { .. } => {
                    painter.rect_filled(Rect::from_center_size(c, sz), 0.0, col);
                }
                PadShape::Oval { .. } => {
                    painter.rect_filled(Rect::from_center_size(c, sz), sz.min_elem() / 2.0, col);
                }
            }
            if let Some(d) = p.drill {
                if p.slot > d + 1e-9 {
                    let (a, b) = pcb_core::slot_ends(p.pos, d, p.slot, p.slot_v);
                    capsule(painter, self.to_screen(rect, a), self.to_screen(rect, b), d as f32 * z / 2.0, BG, None);
                } else {
                    painter.circle_filled(c, d as f32 * z / 2.0, BG);
                }
            }
        }
        // a small padlock at the corner of every fixed part
        for part in self.board.parts.iter().filter(|p| self.board.is_locked(&p.refdes) && self.side_visible(p.side)) {
            let pads: Vec<&pcb_core::Pad> = self.board.pads.iter().filter(|q| q.refdes == part.refdes).collect();
            if pads.is_empty() {
                continue;
            }
            let top = pads.iter().map(|q| q.pos.y + q.shape.size().1 / 2.0).fold(f64::MIN, f64::max);
            let right = pads.iter().map(|q| q.pos.x + q.shape.size().0 / 2.0).fold(f64::MIN, f64::max);
            let c = self.to_screen(rect, Point::new(right, top)) + Vec2::new(6.0, -8.0);
            let col = Color32::from_rgb(250, 200, 70);
            painter.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, 2.0), Vec2::new(9.0, 7.0)), 1.5, col);
            painter.circle_stroke(c + Vec2::new(0.0, -2.0), 3.0, Stroke::new(1.5, col));
        }
        for v in self.board.vias.iter().filter(|_| self.vis.vias) {
            let c = self.to_screen(rect, v.pos);
            painter.circle_filled(c, v.diameter as f32 * z / 2.0, Color32::from_rgb(60, 170, 90));
            painter.circle_filled(c, v.drill as f32 * z / 2.0, BG);
        }
        for h in &self.board.holes {
            let (a, b) = h.ends();
            capsule(painter, self.to_screen(rect, a), self.to_screen(rect, b), h.drill as f32 * z / 2.0, Color32::TRANSPARENT, Some(Stroke::new(1.5, Color32::LIGHT_GRAY)));
        }
        if self.vis.silk {
            let silk = Color32::from_rgb(235, 235, 235);
            // the lines and the strokes of the texts, followed into chains, as one mesh
            {
                let mut cache = self.silk_cache.borrow_mut();
                if !cache.as_ref().is_some_and(|c| c.is_for(&self.board)) {
                    *cache = Some(ribbon::SilkCache::new(&self.board));
                }
            }
            let cache = self.silk_cache.borrow();
            let cache = cache.as_ref().expect("made above");
            let mut mesh = egui::Mesh::default();
            let shown = cache.plain.iter().chain(cache.of_parts.iter().filter(|_| self.vis.parts));
            for ch in shown.filter(|c| self.side_visible(c.side)) {
                let (lo, hi) = (self.to_screen(rect, ch.lo), self.to_screen(rect, ch.hi));
                if !ribbon::on_screen(rect, lo, hi, ch.width as f32 * z / 2.0 + 4.0) {
                    continue;
                }
                let pts: Vec<Pos2> = ch.pts.iter().map(|p| self.to_screen(rect, *p)).collect();
                ribbon::add_chain(&mut mesh, &pts, ch.width as f32 * z, silk);
                if mesh.vertices.len() > 60_000 {
                    painter.add(egui::Shape::mesh(std::mem::take(&mut mesh)));
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            if let Some(Item::Label(i)) = self.selected
                && let Some(t) = self.board.texts.get(i)
                && self.side_visible(t.side)
                && (self.vis.parts || t.refdes.is_empty())
            {
                // a box round the picked designator, so that it is clear what the drag will move
                let (w, h) = t.size();
                painter.rect_stroke(
                    Rect::from_center_size(self.to_screen(rect, t.pos), Vec2::new(w as f32 * z + 8.0, h as f32 * z + 8.0)),
                    2.0,
                    Stroke::new(1.5, Color32::from_rgb(90, 230, 120)),
                    egui::StrokeKind::Outside,
                );
            }
        }
    }

    /// Everything electrically connected to the selected track / pad / via.
    pub(super) fn draw_highlight(&mut self, painter: &egui::Painter, rect: Rect, conn: Option<&drc::Connectivity>) {
        let (Some(conn), Some(sel)) = (conn, self.selected) else { return };
        if !self.item_shown(sel) {
            return;
        }
        let g = match sel {
            Item::Track(i) if i < self.board.tracks.len() => conn.track_group(i),
            Item::Pad(i) if i < self.board.pads.len() => conn.pad_group(i),
            Item::Via(i) if i < self.board.vias.len() => conn.via_group(i),
            _ => return,
        };
        let z = self.zoom;
        let hl = Color32::from_rgba_unmultiplied(255, 255, 255, 140);
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(i, t)| conn.track_group(*i) == g && self.vis.tracks && self.side_visible(t.side)) {
            let _ = i;
            painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((t.width as f32 * z).max(1.0) + 4.0, hl));
        }
        for (j, p) in self.board.pads.iter().enumerate().filter(|(j, p)| conn.pad_group(*j) == g && (p.drill.is_some() || self.side_visible(p.side))) {
            let _ = j;
            let (w, h) = p.shape.size();
            painter.rect_stroke(
                Rect::from_center_size(self.to_screen(rect, p.pos), Vec2::new(w as f32 * z + 6.0, h as f32 * z + 6.0)),
                0.0,
                Stroke::new(2.0, hl),
                egui::StrokeKind::Outside,
            );
        }
        for (k, v) in self.board.vias.iter().enumerate().filter(|(k, _)| conn.via_group(*k) == g && self.vis.vias) {
            let _ = k;
            painter.circle_stroke(self.to_screen(rect, v.pos), v.diameter as f32 * z / 2.0 + 3.0, Stroke::new(2.0, hl));
        }
        let net = conn.group_net(g).map(|n| format!("ネット {n}")).unwrap_or_else(|| "ネット未割り当て".into());
        self.status = format!("選択中の接続: {net}");
    }

    pub(super) fn draw_overlays(&self, painter: &egui::Painter, rect: Rect) {
        self.draw_sheet_groups(painter, rect);
        self.draw_requirement_marks(painter, rect);
        if self.vis.airwires {
            for w in self.cached_airwires().iter() {
                painter.line_segment([self.to_screen(rect, w.a), self.to_screen(rect, w.b)], Stroke::new(1.0, Color32::from_rgb(240, 220, 90)));
            }
        }
        if self.vis.nets && self.zoom >= 14.0 {
            let f = egui::FontId::proportional(10.0);
            for p in self.board.pads.iter().filter(|p| !p.net.is_empty()) {
                painter.text(self.to_screen(rect, p.pos), egui::Align2::CENTER_CENTER, &p.net, f.clone(), Color32::WHITE);
            }
        }
        for v in &self.violations {
            painter.circle_stroke(self.to_screen(rect, v.at), 10.0, Stroke::new(2.0, Color32::from_rgb(255, 0, 255)));
        }
        self.draw_check_marks(painter, rect);
        for sel in self.selected.iter().chain(self.multi.iter()).filter(|s| self.item_shown(**s)) {
            for p in self.item_points(*sel) {
                painter.circle_stroke(self.to_screen(rect, p), 8.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
    }

    /// While the "発注の準備" window is open: a numbered mark at the place of every note of the pre-order check
    /// (the same numbers as in the list). The one that was clicked is larger and pulses, with its text beside it.
    pub(super) fn draw_check_marks(&self, painter: &egui::Painter, rect: Rect) {
        use pcb_core::fab::Level;
        if !self.order.open || !self.order.ran {
            return;
        }
        let t = painter.ctx().input(|i| i.time);
        let mut number = 0;
        for (idx, c) in self.order.checks.iter().enumerate() {
            let Some(at) = c.at else { continue };
            if c.level == Level::Ok {
                continue;
            }
            number += 1;
            let color = match c.level {
                Level::Error => Color32::from_rgb(235, 50, 70),
                Level::Warn => Color32::from_rgb(240, 170, 20),
                _ => Color32::from_rgb(120, 170, 230),
            };
            // the other places of the same note: a smaller ring with the same number
            for q in &c.also {
                let qc = self.to_screen(rect, *q);
                painter.circle_stroke(qc, 7.0, Stroke::new(2.0, color));
                painter.text(qc + Vec2::new(11.0, -9.0), egui::Align2::LEFT_CENTER, number.to_string(), egui::FontId::proportional(12.0), color);
            }
            let center = self.to_screen(rect, at);
            let focused = self.check_focus == Some(idx);
            let r = if focused { 16.0 + 5.0 * (t * 4.0).sin().abs() as f32 } else { 10.0 };
            painter.circle_stroke(center, r, Stroke::new(if focused { 3.0 } else { 2.0 }, color));
            if focused {
                painter.circle_stroke(center, r + 5.0, Stroke::new(1.5, color.gamma_multiply(0.5)));
                painter.ctx().request_repaint();
            }
            let tag = egui::Rect::from_center_size(center + Vec2::new(r + 8.0, -r - 2.0), Vec2::new(18.0, 16.0));
            painter.rect_filled(tag, 3.0, color);
            painter.text(tag.center(), egui::Align2::CENTER_CENTER, number.to_string(), egui::FontId::proportional(13.0), Color32::BLACK);
            if focused {
                let msg = painter.layout(c.title.clone(), egui::FontId::proportional(13.0), Color32::WHITE, 320.0);
                let box_pos = tag.right_top() + Vec2::new(6.0, 0.0);
                painter.rect_filled(Rect::from_min_size(box_pos, msg.size() + Vec2::new(10.0, 6.0)), 3.0, Color32::from_black_alpha(210));
                painter.galley(box_pos + Vec2::new(5.0, 3.0), msg, Color32::WHITE);
            }
        }
    }

    /// A frame and a label in the colour of the layer being worked on, so the front and the back cannot be mixed up.
    pub(super) fn draw_layer_badge(&self, painter: &egui::Painter, rect: Rect) {
        let col = self.side_color(self.side);
        painter.rect_stroke(rect.shrink(1.5), 0.0, Stroke::new(3.0, col.gamma_multiply(0.85)), egui::StrokeKind::Inside);
        let (name, color_name) = match self.side {
            Side::Top => ("表面".to_string(), "赤"),
            Side::Bottom => ("裏面".to_string(), "青"),
            s @ Side::Inner(n) => (s.label(), ["金", "緑", "紫", "青緑"][(n as usize - 1) % 4]),
        };
        let text = format!("作業中: {name} ({color_name})   Lキーで切り替え  ・  もう一方の面は薄く表示");
        let font = egui::FontId::proportional(14.0);
        let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
        let pos = rect.min + Vec2::new(30.0, 26.0);
        let bg = Rect::from_min_size(pos, galley.size() + Vec2::new(16.0, 8.0));
        painter.rect_filled(bg, 4.0, col.gamma_multiply(0.85));
        painter.galley(pos + Vec2::new(8.0, 4.0), galley, Color32::WHITE);
    }

    pub(super) fn draw_cursor(&self, painter: &egui::Painter, rect: Rect, p: Point) {
        let scr = self.to_screen(rect, p);
        painter.circle_stroke(scr, 4.0, Stroke::new(1.0, Color32::WHITE));
        let preview = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
        match self.tool {
            Tool::Track if self.trunk_mode => self.draw_trunk_ghost(painter, rect, p, scr),
            Tool::Via => {
                // the via as it would be, and the net it would be for
                painter.circle_stroke(scr, self.via_dia as f32 * self.zoom / 2.0, Stroke::new(1.5, Color32::from_rgb(60, 170, 90)));
                let net = self.via_net_at(p);
                let text = if net.is_empty() { "ビア (ネットなし)".to_string() } else { format!("ビア → ネット {net}") };
                painter.text(scr + Vec2::new(14.0, -14.0), egui::Align2::LEFT_BOTTOM, text, egui::FontId::proportional(13.0), Color32::from_rgb(120, 225, 160));
            }
            Tool::Track => {
                if let Some(a) = self.pending {
                    let preview = self.route_preview(a, p);
                    let (pushed, bad) = (preview.pushed, preview.bad);
                    let base = if bad { Color32::from_rgb(255, 60, 60) } else if pushed > 0 { Color32::from_rgb(255, 190, 40) } else { self.side_color(self.side) };
                    // the tracks that would be pushed aside: where they are (a thin line) and where they would go
                    if !preview.throttled {
                        let orange = Color32::from_rgb(255, 190, 40);
                        for (old, new) in &preview.moved {
                            painter.line_segment([self.to_screen(rect, old.a), self.to_screen(rect, old.b)], Stroke::new(1.0, orange.gamma_multiply(0.6)));
                            painter.line_segment([self.to_screen(rect, new.a), self.to_screen(rect, new.b)], Stroke::new((new.width as f32 * self.zoom).max(1.5), orange.gamma_multiply(0.55)));
                        }
                    } else {
                        // a slow board: the picture is of an earlier cursor position, so it is not drawn; look again soon
                        painter.ctx().request_repaint_after(super::shoving::PREVIEW_THROTTLE);
                    }
                    // a searched route is drawn in a cooler colour, so it is clear that it goes round what is in the way
                    let col = if preview.found && !bad { Color32::from_rgb(90, 200, 255) } else { base };
                    for t in &preview.tracks {
                        painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((self.track_width as f32 * self.zoom).max(1.0), col.gamma_multiply(0.7)));
                    }
                    if bad || pushed > 0 || preview.throttled {
                        let msg = if bad {
                            preview.why.clone()
                        } else if preview.throttled {
                            "押しのけの計算が重いので、少し遅れて表示します".to_string()
                        } else {
                            format!("{pushed} 本を押しのけます")
                        };
                        painter.text(scr + Vec2::new(14.0, -14.0), egui::Align2::LEFT_BOTTOM, msg, egui::FontId::proportional(13.0), base);
                    }
                }
            }
            Tool::Silk | Tool::OutlineRect => {
                if let Some(a) = self.pending {
                    if self.tool == Tool::OutlineRect {
                        painter.rect_stroke(Rect::from_two_pos(self.to_screen(rect, a), scr), 0.0, preview, egui::StrokeKind::Middle);
                    } else {
                        painter.line_segment([self.to_screen(rect, a), scr], preview);
                    }
                }
            }
            Tool::OutlinePoly | Tool::Zone => {
                let col = match (self.tool, self.zone_op) {
                    (Tool::Zone, pcb_core::model::ShapeOp::Sub) => Color32::LIGHT_RED,
                    (Tool::Zone, _) => self.side_color(self.side),
                    _ => Color32::YELLOW,
                };
                for w in self.poly.windows(2) {
                    painter.line_segment([self.to_screen(rect, w[0]), self.to_screen(rect, w[1])], Stroke::new(1.5, col));
                }
                // the next piece: the line, the arc, or the circle or rectangle that the next click finishes
                let ahead: Vec<Pos2> = self.polygon_preview(p).iter().map(|q| self.to_screen(rect, *q)).collect();
                for w in ahead.windows(2) {
                    painter.line_segment([w[0], w[1]], preview);
                }
                if let Some(m) = self.arc_mid {
                    painter.circle_filled(self.to_screen(rect, m), 3.0, col);
                }
                if let Some(a) = self.pending.filter(|_| matches!(self.poly_mode, PolyMode::Circle | PolyMode::Rect)) {
                    painter.circle_filled(self.to_screen(rect, a), 3.0, col);
                }
                if self.poly.len() >= 3 {
                    painter.circle_stroke(self.to_screen(rect, self.poly[0]), 6.0, Stroke::new(1.5, Color32::WHITE));
                }
            }
            _ => {}
        }
    }
}

/// A slot (or a round hole when `a == b`): a line `r` away on both sides with round ends.
pub(crate) fn capsule(painter: &egui::Painter, a: Pos2, b: Pos2, r: f32, fill: Color32, stroke: Option<Stroke>) {
    let d = b - a;
    let len = d.length();
    if len < 0.5 {
        match stroke {
            Some(s) => painter.circle_stroke(a, r, s),
            None => painter.circle_filled(a, r, fill),
        };
        return;
    }
    // the ends are circles, the middle a quad; the outline of both together is the capsule
    let mut pts = vec![];
    let ang = d.y.atan2(d.x);
    for k in 0..=12 {
        let t = ang - std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / 12.0;
        pts.push(b + Vec2::new(t.cos(), t.sin()) * r);
    }
    for k in 0..=12 {
        let t = ang + std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / 12.0;
        pts.push(a + Vec2::new(t.cos(), t.sin()) * r);
    }
    match stroke {
        Some(s) => {
            painter.add(egui::Shape::closed_line(pts, s));
        }
        None => {
            painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
        }
    }
}
