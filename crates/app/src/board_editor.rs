//! PCB layout editor: place and move parts, route, pour copper, check, export.

use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pcb_core::drc::{self, connectivity};
use pcb_core::geom::{inside_polygon, triangulate};
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use pcb_core::ratsnest::ratsnest;
use pcb_core::strokefont::text_strokes;
use pcb_core::{assembly, export, Board, Hole, Pad, PadShape, Point, Side, SilkLine, Track, Via, Zone};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Track,
    PadTh,
    PadSmd,
    Via,
    Hole,
    Silk,
    OutlineRect,
    OutlinePoly,
    Zone,
}

fn tool_tip(t: Tool) -> &'static str {
    match t {
        Tool::Select => "部品や配線をクリックで選択、部品はドラッグで移動します",
        Tool::Track => "銅の配線 (トラック) を引きます。クリックで始点、クリックで曲がり、ダブルクリックで終了。V でビアを打って裏面に移れます",
        Tool::Via => "表面と裏面をつなぐ穴 (ビア) を置きます",
        Tool::Zone => "広い面を銅で埋めます (ベタ)。GND に使うと配線が楽になり、ノイズにも強くなります",
        Tool::OutlineRect => "基板の外形を矩形でドラッグして決めます",
        Tool::OutlinePoly => "基板の外形を多角形で決めます。クリックで頂点、ダブルクリックで確定",
        Tool::PadTh => "穴付き (スルーホール) の単独パッドを置きます",
        Tool::PadSmd => "表面実装の単独パッドを置きます",
        Tool::Hole => "ネジ止め用の取付穴を置きます",
        Tool::Silk => "部品名や目印などの白い印刷 (シルク) の線を引きます",
    }
}

const TOOLS: &[(Tool, &str)] = &[
    (Tool::Select, "選択 / 部品の移動"),
    (Tool::Track, "配線"),
    (Tool::Via, "ビア"),
    (Tool::Zone, "銅箔ゾーン (ベタ)"),
    (Tool::OutlineRect, "基板外形 (矩形)"),
    (Tool::OutlinePoly, "基板外形 (多角形)"),
    (Tool::PadTh, "パッド(スルーホール)"),
    (Tool::PadSmd, "パッド(SMD)"),
    (Tool::Hole, "取付穴"),
    (Tool::Silk, "シルク線"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteMode {
    Free,
    Angle45,
    Orthogonal,
}

#[derive(Clone, Copy, PartialEq)]
enum Item {
    Track(usize),
    Pad(usize),
    Via(usize),
    Hole(usize),
    Silk(usize),
    /// Index into `Board::parts`.
    Part(usize),
    Zone(usize),
}

#[derive(Clone, Copy)]
struct Visible {
    top: bool,
    bottom: bool,
    silk: bool,
    airwires: bool,
    nets: bool,
    zones: bool,
}

pub struct BoardEditor {
    pub board: Board,
    path: Option<PathBuf>,
    tool: Tool,
    side: Side,
    grid: f64,
    track_width: f64,
    pad_size: f64,
    pad_drill: f64,
    via_dia: f64,
    via_drill: f64,
    hole_drill: f64,
    route_mode: RouteMode,
    bend_flip: bool,
    zoom: f32, // pixels per mm
    pan: Vec2, // world mm at the screen centre
    pending: Option<Point>,
    /// Polygon being drawn (board outline or zone).
    poly: Vec<Point>,
    selected: Option<Item>,
    drag_part: Option<(String, Point)>,
    pub status: String,
    violations: Vec<drc::Violation>,
    import_notes: Vec<String>,
    undo: Vec<Board>,
    redo: Vec<Board>,
    vis: Visible,
    zone_net: String,
    zone_clearance: f64,
    zone_thermal: bool,
    size_w: f64,
    size_h: f64,
    fit_pending: bool,
}

impl Default for BoardEditor {
    fn default() -> Self {
        Self {
            board: Board::new("board", 50.0, 40.0),
            path: None,
            tool: Tool::Select,
            side: Side::Top,
            grid: 0.5,
            track_width: 0.25,
            pad_size: 1.6,
            pad_drill: 0.8,
            via_dia: 0.6,
            via_drill: 0.3,
            hole_drill: 3.2,
            route_mode: RouteMode::Angle45,
            bend_flip: false,
            zoom: 12.0,
            pan: Vec2::new(25.0, 20.0),
            pending: None,
            poly: vec![],
            selected: None,
            drag_part: None,
            status: "基板パターン: 「回路図から取り込み」で部品を配置します".into(),
            violations: vec![],
            import_notes: vec![],
            undo: vec![],
            redo: vec![],
            vis: Visible { top: true, bottom: true, silk: true, airwires: true, nets: true, zones: true },
            zone_net: "GND".into(),
            zone_clearance: 0.3,
            zone_thermal: true,
            size_w: 50.0,
            size_h: 40.0,
            fit_pending: true,
        }
    }
}

fn snap(v: f64, g: f64) -> f64 {
    if g > 0.0 { (v / g).round() * g } else { v }
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

fn polygon_edge_dist(poly: &[Point], p: Point) -> f64 {
    let n = poly.len();
    (0..n).map(|i| seg_dist(p, poly[i], poly[(i + 1) % n])).fold(f64::MAX, f64::min)
}

const BG: Color32 = Color32::from_rgb(18, 22, 18);

impl BoardEditor {
    // ------------------------------------------------------------- history

    /// Remember the current board before a change.
    fn checkpoint(&mut self) {
        self.undo.push(self.board.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.board, prev));
            self.after_history("元に戻しました");
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.board, next));
            self.after_history("やり直しました");
        }
    }

    fn after_history(&mut self, msg: &str) {
        self.selected = None;
        self.pending = None;
        self.poly.clear();
        self.violations.clear();
        self.status = msg.into();
    }

    // ------------------------------------------------------------- view

    fn to_screen(&self, rect: Rect, p: Point) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.x as f32 - self.pan.x) * self.zoom, c.y - (p.y as f32 - self.pan.y) * self.zoom)
    }

    fn to_world(&self, rect: Rect, s: Pos2) -> Point {
        let c = rect.center();
        Point::new(((s.x - c.x) / self.zoom + self.pan.x) as f64, (-(s.y - c.y) / self.zoom + self.pan.y) as f64)
    }

    fn fit_view(&mut self, rect: Rect) {
        let Some((lo, hi)) = self.board.bounds() else { return };
        let (w, h) = ((hi.x - lo.x).max(5.0) + 10.0, (hi.y - lo.y).max(5.0) + 10.0);
        self.zoom = ((rect.width() as f64 / w).min(rect.height() as f64 / h) as f32).clamp(2.0, 60.0);
        self.pan = Vec2::new(((lo.x + hi.x) / 2.0) as f32, ((lo.y + hi.y) / 2.0) as f32);
    }

    fn side_color(&self, s: Side) -> Color32 {
        match s {
            Side::Top => Color32::from_rgb(220, 60, 60),
            Side::Bottom => Color32::from_rgb(70, 110, 230),
        }
    }

    fn side_visible(&self, s: Side) -> bool {
        match s {
            Side::Top => self.vis.top,
            Side::Bottom => self.vis.bottom,
        }
    }

    fn nets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    // ------------------------------------------------------------- picking

    fn hit_test(&self, p: Point) -> Option<Item> {
        let tol = 6.0 / self.zoom as f64;
        for (i, v) in self.board.vias.iter().enumerate() {
            if p.dist(v.pos) <= v.diameter / 2.0 + tol {
                return Some(Item::Via(i));
            }
        }
        for (i, pd) in self.board.pads.iter().enumerate().filter(|(_, pd)| pd.drill.is_some() || self.side_visible(pd.side)) {
            let (w, h) = pd.shape.size();
            if (p.x - pd.pos.x).abs() <= w / 2.0 + tol && (p.y - pd.pos.y).abs() <= h / 2.0 + tol {
                if !pd.refdes.is_empty() {
                    if let Some(k) = self.board.parts.iter().position(|b| b.refdes == pd.refdes) {
                        return Some(Item::Part(k));
                    }
                }
                return Some(Item::Pad(i));
            }
        }
        for (i, h) in self.board.holes.iter().enumerate() {
            if p.dist(h.pos) <= h.drill / 2.0 + tol {
                return Some(Item::Hole(i));
            }
        }
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| self.side_visible(t.side)) {
            if seg_dist(p, t.a, t.b) <= t.width / 2.0 + tol {
                return Some(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty()) {
            if seg_dist(p, l.a, l.b) <= l.width / 2.0 + tol {
                return Some(Item::Silk(i));
            }
        }
        for (i, z) in self.board.zones.iter().enumerate().filter(|(_, z)| self.vis.zones && self.side_visible(z.side)) {
            if polygon_edge_dist(&z.polygon, p) <= tol || inside_polygon(&z.polygon, p) {
                return Some(Item::Zone(i));
            }
        }
        None
    }

    fn delete(&mut self, it: Item) {
        if let Item::Part(_) = it {
            self.status = "部品は回路図側で削除してから「回路図から取り込み」を実行してください".into();
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
            Item::Part(_) => {}
        }
        self.selected = None;
    }

    // ------------------------------------------------------------- routing

    /// Points to visit from `a` to `p` (the last one is `p`), honouring the route mode.
    fn route_points(&self, a: Point, p: Point) -> Vec<Point> {
        let (dx, dy) = (p.x - a.x, p.y - a.y);
        let eps = 1e-9;
        if dx.abs() < eps || dy.abs() < eps || self.route_mode == RouteMode::Free {
            return vec![p];
        }
        match self.route_mode {
            RouteMode::Orthogonal => vec![if self.bend_flip { Point::new(a.x, p.y) } else { Point::new(p.x, a.y) }, p],
            _ => {
                if (dx.abs() - dy.abs()).abs() < eps {
                    return vec![p];
                }
                let d = dx.abs().min(dy.abs());
                let (sx, sy) = (dx.signum(), dy.signum());
                let diag_first = Point::new(a.x + sx * d, a.y + sy * d);
                let straight_first = if dx.abs() > dy.abs() {
                    Point::new(a.x + sx * (dx.abs() - d), a.y)
                } else {
                    Point::new(a.x, a.y + sy * (dy.abs() - d))
                };
                vec![if self.bend_flip { diag_first } else { straight_first }, p]
            }
        }
    }

    /// Run from `a` to `p`, drop a via there and carry on from `p` on the other layer.
    fn route_via(&mut self, a: Point, p: Point) {
        self.checkpoint();
        self.push_route(a, p);
        self.board.vias.push(Via { pos: p, diameter: self.via_dia, drill: self.via_drill });
        self.side = self.side.flip();
        self.pending = Some(p);
    }

    fn push_route(&mut self, a: Point, p: Point) {
        let mut from = a;
        for q in self.route_points(a, p) {
            if from.dist(q) > 1e-9 {
                self.board.tracks.push(Track { side: self.side, a: from, b: q, width: self.track_width });
            }
            from = q;
        }
    }

    /// Move the selected part to the other side of the board.
    fn flip_selected_part(&mut self) {
        if let Some(Item::Part(i)) = self.selected {
            if i < self.board.parts.len() {
                self.checkpoint();
                let r = self.board.parts[i].refdes.clone();
                self.board.flip_part(&r);
                let side = if self.board.parts[i].side == Side::Bottom { "裏面" } else { "表面" };
                self.status = format!("{r} を{side}に移しました");
            }
        }
    }

    // ------------------------------------------------------------- tools

    fn finish_polygon(&mut self) {
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

    fn click(&mut self, p: Point) {
        match self.tool {
            Tool::Select => self.selected = self.hit_test(p),
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

    // ------------------------------------------------------------- files / actions

    pub fn new_file(&mut self) {
        *self = BoardEditor::default();
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 基板", &["pcbjson"]).set_file_name("board.pcbjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match serde_json::to_string_pretty(&self.board).map(|s| std::fs::write(&p, s)) {
                Ok(Ok(())) => {
                    self.path = Some(p.clone());
                    format!("保存しました: {}", p.display())
                }
                Ok(Err(e)) => format!("保存失敗: {e}"),
                Err(e) => format!("保存失敗: {e}"),
            };
        }
    }

    pub fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 基板", &["pcbjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Board>(&s).map_err(|e| e.to_string())) {
            Ok(b) => {
                let (lo, hi) = b.bounds().unwrap_or((Point::default(), Point::default()));
                self.size_w = hi.x - lo.x;
                self.size_h = hi.y - lo.y;
                self.board = b;
                self.path = Some(p.clone());
                self.selected = None;
                self.pending = None;
                self.poly.clear();
                self.violations.clear();
                self.undo.clear();
                self.redo.clear();
                self.fit_pending = true;
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    pub fn run_drc(&mut self) {
        self.violations = drc::check(&self.board);
        self.status = format!("DRC: 違反 {} 件", self.violations.len());
    }

    pub fn export(&mut self) {
        self.violations = drc::check(&self.board);
        if !self.violations.is_empty() {
            self.status = format!("DRC違反が{}件あります。修正してから書き出してください", self.violations.len());
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("製造ファイルの出力先フォルダ").pick_folder() else { return };
        self.status = match self.write_fab_files(&dir) {
            Ok(n) => format!("書き出し完了: {} ({} ファイル + zip)", dir.display(), n),
            Err(e) => format!("書き出し失敗: {e}"),
        };
    }

    /// Gerber + drill files, the zip for the fab, and BOM / placement CSVs.
    pub fn write_fab_files(&self, dir: &std::path::Path) -> Result<usize, String> {
        let mut n = 0;
        for (name, data) in export::fab_files(&self.board).into_iter().chain(assembly::assembly_files(&self.board)) {
            std::fs::write(dir.join(&name), data).map_err(|e| e.to_string())?;
            n += 1;
        }
        let zip = export::zip_bytes(&self.board).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{}_gerber.zip", self.board.name)), zip).map_err(|e| e.to_string())?;
        Ok(n)
    }

    pub fn import_from_schematic(&mut self, sch_ed: &SchEditor) {
        let sch = &sch_ed.sch;
        let parts: Vec<NetlistPart> = sch
            .components
            .iter()
            .filter(|c| !c.kind.is_power_symbol())
            .map(|c| NetlistPart { refdes: c.refdes.clone(), value: c.value.clone(), footprint: c.footprint_name() })
            .collect();
        let nets: Vec<NetlistNet> = sch_core::extract(sch)
            .into_iter()
            .map(|n| NetlistNet { name: n.name, pins: n.pins.into_iter().map(|p| (p.refdes, p.pin)).collect() })
            .collect();
        // library parts bring their own footprint
        self.checkpoint();
        for c in sch.components.iter().filter(|c| c.custom.is_some()) {
            if let Some(def) = sch_ed.library.iter().find(|d| d.footprint_name() == c.footprint) {
                if let Ok(fp) = def.footprint() {
                    self.board.add_footprint(fp);
                }
            }
        }
        let rep = netlist::import(&mut self.board, &parts, &nets);
        self.selected = None;
        self.import_notes = rep.warnings.clone();
        self.status = format!("取り込み完了: 追加 {} / 更新 {} / 削除 {} / 警告 {}", rep.added.len(), rep.updated, rep.removed.len(), rep.warnings.len());
    }

    // ------------------------------------------------------------- canvas

    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, BG);
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        // pan (middle/right drag) and zoom (wheel, around the cursor)
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y += d.y / self.zoom;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let before = self.to_world(rect, cursor);
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(1.0, 400.0);
                let after = self.to_world(rect, cursor);
                self.pan.x += (before.x - after.x) as f32;
                self.pan.y += (before.y - after.y) as f32;
            }
        }

        self.draw_grid(&painter, rect);
        let conn = (!self.board.zones.is_empty() || self.selected.is_some()).then(|| connectivity(&self.board));
        self.draw_zones(&painter, rect, conn.as_ref());
        self.draw_copper(&painter, rect);
        self.draw_highlight(&painter, rect, conn.as_ref());
        self.draw_overlays(&painter, rect);

        // cursor handling
        let typing = ui.ctx().egui_wants_keyboard_input();
        let mut sp = None;
        if let Some(pos) = resp.hover_pos() {
            let w = self.to_world(rect, pos);
            let mut p = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            if matches!(self.tool, Tool::Track) {
                // snap routing to pad / via centres so off-grid footprints can be connected
                let tol = 10.0 / self.zoom as f64;
                let near = self
                    .board
                    .pads
                    .iter()
                    .map(|p| p.pos)
                    .chain(self.board.vias.iter().map(|v| v.pos))
                    .filter(|c| c.dist(w) <= tol)
                    .min_by(|a, b| a.dist(w).total_cmp(&b.dist(w)));
                if let Some(c) = near {
                    p = c;
                }
            }
            sp = Some(p);
            self.draw_cursor(&painter, rect, p);

            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select {
                if let Some(Item::Part(i)) = self.hit_test(w) {
                    self.checkpoint();
                    let part = &self.board.parts[i];
                    self.drag_part = Some((part.refdes.clone(), Point::new(p.x - part.pos.x, p.y - part.pos.y)));
                    self.selected = Some(Item::Part(i));
                }
            }
            if let Some((r, off)) = self.drag_part.clone() {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    if let Some(rot) = self.board.parts.iter().find(|q| q.refdes == r).map(|q| q.rot) {
                        self.board.place_part(&r, Point::new(p.x - off.x, p.y - off.y), rot);
                    }
                }
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                self.click(p);
            }
            if resp.double_clicked() && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
                self.finish_polygon();
            }
            let layer = if self.side == Side::Top { "表面" } else { "裏面" };
            self.status = format!("X {:.2}  Y {:.2} mm   [{layer}]", p.x, p.y);
        }
        if resp.drag_stopped() {
            self.drag_part = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
            self.poly.clear();
        }

        if !typing {
            self.handle_keys(ui, sp);
        }
    }

    fn handle_keys(&mut self, ui: &egui::Ui, cursor: Option<Point>) {
        use egui::Key;
        let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ui.input(|i| i.key_pressed(k));
        if cmd && pressed(Key::Z) {
            if shift { self.redo() } else { self.undo() }
            return;
        }
        if cmd && pressed(Key::Y) {
            self.redo();
            return;
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            if let Some(it) = self.selected {
                self.delete(it);
            }
        }
        if pressed(Key::Escape) {
            self.pending = None;
            self.poly.clear();
        }
        if pressed(Key::Enter) && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
            self.finish_polygon();
        }
        if pressed(Key::L) {
            self.side = self.side.flip();
        }
        if pressed(Key::F) {
            self.fit_pending = true;
        }
        if pressed(Key::Space) {
            self.bend_flip = !self.bend_flip;
        }
        if pressed(Key::B) {
            self.flip_selected_part();
        }
        if pressed(Key::R) {
            if let Some(Item::Part(i)) = self.selected {
                self.checkpoint();
                let p = self.board.parts[i].clone();
                self.board.place_part(&p.refdes, p.pos, (p.rot + 1) % 4);
            }
        }
        // V while routing: run to the cursor, drop a via and continue on the other layer
        if pressed(Key::V) && self.tool == Tool::Track {
            if let (Some(a), Some(p)) = (self.pending, cursor) {
                self.route_via(a, p);
            }
        }
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect) {
        if self.grid > 0.0 && self.grid as f32 * self.zoom >= 6.0 {
            let tl = self.to_world(rect, rect.left_top());
            let br = self.to_world(rect, rect.right_bottom());
            let (mut x, y_end) = ((tl.x / self.grid).floor() * self.grid, (br.y / self.grid).floor() * self.grid);
            while x <= br.x {
                let mut y = (tl.y / self.grid).ceil() * self.grid;
                while y >= y_end {
                    painter.circle_filled(self.to_screen(rect, Point::new(x, y)), 0.8, Color32::from_gray(60));
                    y -= self.grid;
                }
                x += self.grid;
            }
        }
    }

    /// Copper pours with the same cut-outs the Gerber output has (thermal spokes not drawn).
    fn draw_zones(&self, painter: &egui::Painter, rect: Rect, conn: Option<&drc::Connectivity>) {
        if !self.vis.zones {
            return;
        }
        let z = self.zoom;
        for s in [self.side.flip(), self.side] {
            if !self.side_visible(s) {
                continue;
            }
            let alpha = if s == self.side { 90 } else { 45 };
            let fill = self.side_color(s).gamma_multiply(alpha as f32 / 255.0);
            for zone in self.board.zones.iter().filter(|zn| zn.side == s) {
                for t in triangulate(&zone.polygon) {
                    let pts: Vec<Pos2> = t.iter().map(|p| self.to_screen(rect, *p)).collect();
                    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                }
                let n = zone.polygon.len();
                for i in 0..n {
                    painter.line_segment(
                        [self.to_screen(rect, zone.polygon[i]), self.to_screen(rect, zone.polygon[(i + 1) % n])],
                        Stroke::new(1.0, self.side_color(s)),
                    );
                }
                // clearance around everything of another net
                let Some(conn) = conn else { continue };
                let c = zone.clearance as f32;
                let other = |net: Option<&str>| zone.net.is_empty() || net != Some(zone.net.as_str());
                for (i, t) in self.board.tracks.iter().enumerate().filter(|(_, t)| t.side == s) {
                    if other(conn.track_net(i)) {
                        painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((t.width as f32 + 2.0 * c) * z, BG));
                    }
                }
                for (j, p) in self.board.pads.iter().enumerate().filter(|(_, p)| p.drill.is_some() || p.side == s) {
                    if other(conn.pad_net(j)) {
                        let (w, h) = p.shape.size();
                        let r = Rect::from_center_size(self.to_screen(rect, p.pos), Vec2::new((w as f32 + 2.0 * c) * z, (h as f32 + 2.0 * c) * z));
                        match p.shape {
                            PadShape::Circle { .. } => painter.circle_filled(r.center(), r.width() / 2.0, BG),
                            _ => painter.rect_filled(r, 0.0, BG),
                        };
                    }
                }
                for (k, v) in self.board.vias.iter().enumerate() {
                    if other(conn.via_net(k)) {
                        painter.circle_filled(self.to_screen(rect, v.pos), (v.diameter as f32 / 2.0 + c) * z, BG);
                    }
                }
                for h in &self.board.holes {
                    painter.circle_filled(self.to_screen(rect, h.pos), (h.drill as f32 / 2.0 + c) * z, BG);
                }
            }
        }
    }

    fn draw_copper(&self, painter: &egui::Painter, rect: Rect) {
        let z = self.zoom;
        let o = &self.board.outline;
        for i in 0..o.len() {
            painter.line_segment([self.to_screen(rect, o[i]), self.to_screen(rect, o[(i + 1) % o.len()])], Stroke::new(2.0, Color32::YELLOW));
        }
        // inactive layer first, so the active one is on top
        for s in [self.side.flip(), self.side] {
            if !self.side_visible(s) {
                continue;
            }
            let alpha = if s == self.side { 255 } else { 90 };
            let col = self.side_color(s).gamma_multiply(alpha as f32 / 255.0);
            for t in self.board.tracks.iter().filter(|t| t.side == s) {
                painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((t.width as f32 * z).max(1.0), col));
                // round ends, like the Gerber aperture
                for e in [t.a, t.b] {
                    painter.circle_filled(self.to_screen(rect, e), (t.width as f32 * z / 2.0).max(0.5), col);
                }
            }
        }
        for p in &self.board.pads {
            let visible = p.drill.is_some() && (self.vis.top || self.vis.bottom) || p.drill.is_none() && self.side_visible(p.side);
            if !visible {
                continue;
            }
            let c = self.to_screen(rect, p.pos);
            let col = self.side_color(p.side).gamma_multiply(0.9);
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
                painter.circle_filled(c, d as f32 * z / 2.0, BG);
            }
        }
        for v in &self.board.vias {
            let c = self.to_screen(rect, v.pos);
            painter.circle_filled(c, v.diameter as f32 * z / 2.0, Color32::from_rgb(60, 170, 90));
            painter.circle_filled(c, v.drill as f32 * z / 2.0, BG);
        }
        for h in &self.board.holes {
            painter.circle_stroke(self.to_screen(rect, h.pos), h.drill as f32 * z / 2.0, Stroke::new(1.5, Color32::LIGHT_GRAY));
        }
        if self.vis.silk {
            let silk = Color32::from_rgb(235, 235, 235);
            for l in self.board.silk.iter().filter(|l| self.side_visible(l.side)) {
                painter.line_segment([self.to_screen(rect, l.a), self.to_screen(rect, l.b)], Stroke::new((l.width as f32 * z).max(1.0), silk));
            }
            for t in self.board.texts.iter().filter(|t| self.side_visible(t.side)) {
                let stroke = Stroke::new(((t.height * 0.15).max(0.12) as f32 * z).max(1.0), silk);
                for [a, b] in text_strokes(&t.text, t.height, t.side == Side::Bottom) {
                    let (pa, pb) = (Point::new(t.pos.x + a.x, t.pos.y + a.y), Point::new(t.pos.x + b.x, t.pos.y + b.y));
                    painter.line_segment([self.to_screen(rect, pa), self.to_screen(rect, pb)], stroke);
                }
            }
        }
    }

    /// Everything electrically connected to the selected track / pad / via.
    fn draw_highlight(&mut self, painter: &egui::Painter, rect: Rect, conn: Option<&drc::Connectivity>) {
        let (Some(conn), Some(sel)) = (conn, self.selected) else { return };
        let g = match sel {
            Item::Track(i) if i < self.board.tracks.len() => conn.track_group(i),
            Item::Pad(i) if i < self.board.pads.len() => conn.pad_group(i),
            Item::Via(i) if i < self.board.vias.len() => conn.via_group(i),
            _ => return,
        };
        let z = self.zoom;
        let hl = Color32::from_rgba_unmultiplied(255, 255, 255, 140);
        for (i, t) in self.board.tracks.iter().enumerate().filter(|(i, _)| conn.track_group(*i) == g) {
            let _ = i;
            painter.line_segment([self.to_screen(rect, t.a), self.to_screen(rect, t.b)], Stroke::new((t.width as f32 * z).max(1.0) + 4.0, hl));
        }
        for (j, p) in self.board.pads.iter().enumerate().filter(|(j, _)| conn.pad_group(*j) == g) {
            let _ = j;
            let (w, h) = p.shape.size();
            painter.rect_stroke(
                Rect::from_center_size(self.to_screen(rect, p.pos), Vec2::new(w as f32 * z + 6.0, h as f32 * z + 6.0)),
                0.0,
                Stroke::new(2.0, hl),
                egui::StrokeKind::Outside,
            );
        }
        for (k, v) in self.board.vias.iter().enumerate().filter(|(k, _)| conn.via_group(*k) == g) {
            let _ = k;
            painter.circle_stroke(self.to_screen(rect, v.pos), v.diameter as f32 * z / 2.0 + 3.0, Stroke::new(2.0, hl));
        }
        let net = conn.group_net(g).map(|n| format!("ネット {n}")).unwrap_or_else(|| "ネット未割り当て".into());
        self.status = format!("選択中の接続: {net}");
    }

    fn draw_overlays(&self, painter: &egui::Painter, rect: Rect) {
        if self.vis.airwires {
            for w in ratsnest(&self.board) {
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
        if let Some(sel) = self.selected {
            let pts: Vec<Point> = match sel {
                Item::Part(i) if i < self.board.parts.len() => {
                    let r = &self.board.parts[i].refdes;
                    self.board.pads.iter().filter(|p| &p.refdes == r).map(|p| p.pos).collect()
                }
                Item::Track(i) if i < self.board.tracks.len() => vec![self.board.tracks[i].a, self.board.tracks[i].b],
                Item::Silk(i) if i < self.board.silk.len() => vec![self.board.silk[i].a, self.board.silk[i].b],
                Item::Pad(i) if i < self.board.pads.len() => vec![self.board.pads[i].pos],
                Item::Via(i) if i < self.board.vias.len() => vec![self.board.vias[i].pos],
                Item::Hole(i) if i < self.board.holes.len() => vec![self.board.holes[i].pos],
                Item::Zone(i) if i < self.board.zones.len() => self.board.zones[i].polygon.clone(),
                _ => vec![],
            };
            for p in pts {
                painter.circle_stroke(self.to_screen(rect, p), 8.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
    }

    fn draw_cursor(&self, painter: &egui::Painter, rect: Rect, p: Point) {
        let scr = self.to_screen(rect, p);
        painter.circle_stroke(scr, 4.0, Stroke::new(1.0, Color32::WHITE));
        let preview = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
        match self.tool {
            Tool::Track => {
                if let Some(a) = self.pending {
                    let mut from = a;
                    for q in self.route_points(a, p) {
                        painter.line_segment([self.to_screen(rect, from), self.to_screen(rect, q)], Stroke::new((self.track_width as f32 * self.zoom).max(1.0), self.side_color(self.side).gamma_multiply(0.6)));
                        from = q;
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
                let col = if self.tool == Tool::Zone { self.side_color(self.side) } else { Color32::YELLOW };
                for w in self.poly.windows(2) {
                    painter.line_segment([self.to_screen(rect, w[0]), self.to_screen(rect, w[1])], Stroke::new(1.5, col));
                }
                if let Some(last) = self.poly.last() {
                    painter.line_segment([self.to_screen(rect, *last), scr], preview);
                }
                if self.poly.len() >= 3 {
                    painter.circle_stroke(self.to_screen(rect, self.poly[0]), 6.0, Stroke::new(1.5, Color32::WHITE));
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------- side panel

    /// A tip for what to do next on the board.
    pub fn hint(&self) -> String {
        if self.board.parts.is_empty() {
            return "まず回路図タブで回路を作り、ここで「回路図から取り込み」を押すと、部品が基板に読み込まれます".into();
        }
        if self.board.bounds().is_none() || self.board.outline.is_empty() {
            return "基板の外形を決めましょう。左の「矩形に設定」か、ツールの「基板外形」で描きます。発注にはこの外形が必要です".into();
        }
        let unrouted = ratsnest(&self.board).len();
        if unrouted > 0 {
            return format!("黄色の線 (未配線 {unrouted} 本) が残っています。「配線」ツールでつなぎましょう。広い GND は「銅箔ゾーン」にすると簡単です");
        }
        if self.violations.is_empty() {
            return "配線が終わりました。「DRC を実行」でルール違反がないか確認し、問題なければ「ファイル → 製造ファイルを書き出し」で発注用データ (ガーバー) を作れます".into();
        }
        "DRC の指摘は、クリックするとその場所に飛びます。直したらもう一度 DRC を実行しましょう。0 件になれば発注データを書き出せます".into()
    }

    pub fn side_panel(&mut self, ui: &mut egui::Ui, sch: &SchEditor) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("基板パターン");
            for (t, name) in TOOLS {
                if ui.selectable_label(self.tool == *t, *name).on_hover_text(tool_tip(*t)).clicked() {
                    self.tool = *t;
                    self.pending = None;
                    self.poly.clear();
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("作業層 (L)").on_hover_text("いま配線や部品を置く面。基板は表と裏の2層に銅を張れます");
                ui.selectable_value(&mut self.side, Side::Top, "表面");
                ui.selectable_value(&mut self.side, Side::Bottom, "裏面");
            });
            ui.horizontal(|ui| {
                ui.label("配線");
                ui.selectable_value(&mut self.route_mode, RouteMode::Angle45, "45°");
                ui.selectable_value(&mut self.route_mode, RouteMode::Orthogonal, "直角");
                ui.selectable_value(&mut self.route_mode, RouteMode::Free, "自由");
            });
            ui.small("配線中: V = ビアで層を切替 / Space = 曲がり方を反転");
            ui.add(egui::DragValue::new(&mut self.grid).speed(0.05).range(0.0..=5.0).prefix("グリッド ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.track_width).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"))
                .on_hover_text("配線の太さ。大きい電流を流す線は太く。発注先の最小線幅 (多くは 0.15〜0.2 mm) を下回らないように");
            ui.add(egui::DragValue::new(&mut self.via_dia).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.via_drill).speed(0.05).range(0.2..=2.0).prefix("ビア穴 ").suffix(" mm"))
                .on_hover_text("ビアの穴径。小さいほど基板が高くなることがあります。発注先の最小穴径を確認しましょう");
            if matches!(self.tool, Tool::PadTh | Tool::PadSmd) {
                ui.add(egui::DragValue::new(&mut self.pad_size).speed(0.05).range(0.3..=10.0).prefix("パッド径 ").suffix(" mm"));
                ui.add(egui::DragValue::new(&mut self.pad_drill).speed(0.05).range(0.2..=6.0).prefix("パッド穴 ").suffix(" mm"));
            }
            if self.tool == Tool::Hole {
                ui.add(egui::DragValue::new(&mut self.hole_drill).speed(0.1).range(0.5..=10.0).prefix("取付穴 ").suffix(" mm"));
            }

            ui.separator();
            ui.label("表示");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.vis.top, "表面");
                ui.checkbox(&mut self.vis.bottom, "裏面");
                ui.checkbox(&mut self.vis.silk, "シルク");
                ui.checkbox(&mut self.vis.zones, "ゾーン");
                ui.checkbox(&mut self.vis.airwires, "未配線");
                ui.checkbox(&mut self.vis.nets, "ネット名");
            });

            ui.separator();
            ui.label("基板外形");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.size_w).speed(0.5).range(5.0..=500.0).prefix("幅 "));
                ui.add(egui::DragValue::new(&mut self.size_h).speed(0.5).range(5.0..=500.0).prefix("高さ "));
                if ui.button("矩形に設定").on_hover_text("上の幅と高さで基板の外形を作ります。発注するときの基板サイズになります").clicked() {
                    self.checkpoint();
                    let (lo, _) = self.board.bounds().unwrap_or((Point::default(), Point::default()));
                    self.board.set_rect_outline(lo.x, lo.y, lo.x + self.size_w, lo.y + self.size_h);
                }
            });
            if let Some((lo, hi)) = self.board.bounds() {
                ui.small(format!("現在 {:.1} × {:.1} mm ({}点)", hi.x - lo.x, hi.y - lo.y, self.board.outline.len()));
            }

            if self.tool == Tool::Zone {
                ui.separator();
                ui.label("ゾーンの設定 (多角形を描き、ダブルクリックか Enter で確定)");
                let nets = self.nets();
                egui::ComboBox::from_label("ネット").selected_text(self.zone_net.clone()).show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut self.zone_net, n.clone(), n);
                    }
                    if !nets.iter().any(|n| n == "GND") {
                        ui.selectable_value(&mut self.zone_net, "GND".to_string(), "GND");
                    }
                });
                ui.add(egui::DragValue::new(&mut self.zone_clearance).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm"));
                ui.checkbox(&mut self.zone_thermal, "スルーホールはサーマルリリーフ");
            }

            self.selection_panel(ui);

            ui.separator();
            if ui.button("回路図から取り込み").on_hover_text("回路図の部品とつながり (ネット) を基板に読み込みます。部品が並び、つなぐべき所に黄色の線 (未配線) が出ます。回路図を直したらもう一度押します").clicked() {
                self.import_from_schematic(sch);
            }
            for n in &self.import_notes {
                ui.colored_label(Color32::from_rgb(255, 180, 60), n);
            }
            ui.label(format!("未配線: {} 本 (黄色の線)", ratsnest(&self.board).len()));
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("元に戻す")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("やり直し")).clicked() {
                    self.redo();
                }
            });
            ui.label("基板名 (出力ファイル名)");
            ui.text_edit_singleline(&mut self.board.name);

            ui.separator();
            if ui.button("DRC を実行").on_hover_text("設計ルールチェック。線同士が近すぎる、銅が基板の端にはみ出している、などを調べます。発注前に必ず実行しましょう").clicked() {
                self.run_drc();
            }
            let mut jump = None;
            for v in &self.violations {
                if ui.link(format!("({:.1}, {:.1}) {}", v.at.x, v.at.y, v.message)).clicked() {
                    jump = Some(v.at);
                }
            }
            if let Some(at) = jump {
                self.pan = Vec2::new(at.x as f32, at.y as f32);
                self.zoom = self.zoom.max(25.0);
            }
            ui.separator();
            ui.small(
                "左クリック: 配置・選択 / 右クリック: 中断\n中・右ドラッグ: 移動 / ホイール: 拡大縮小\nCtrl+Z: 元に戻す / Ctrl+Y: やり直し / Delete: 削除\nR: 部品を回転 / B: 部品の表裏反転 / L: 層切替 / F: 全体表示",
            );
        });
    }

    fn selection_panel(&mut self, ui: &mut egui::Ui) {
        let Some(sel) = self.selected else { return };
        ui.separator();
        ui.label("選択中");
        match sel {
            Item::Track(i) if i < self.board.tracks.len() => {
                let mut w = self.board.tracks[i].width;
                let r = ui.add(egui::DragValue::new(&mut w).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"));
                if r.drag_started() || r.gained_focus() {
                    self.checkpoint();
                }
                if r.changed() {
                    self.board.tracks[i].width = w;
                }
                let mut s = self.board.tracks[i].side;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s, Side::Top, "表面");
                    ui.selectable_value(&mut s, Side::Bottom, "裏面");
                });
                if s != self.board.tracks[i].side {
                    self.checkpoint();
                    self.board.tracks[i].side = s;
                }
            }
            Item::Via(i) if i < self.board.vias.len() => {
                let (mut d, mut dr) = (self.board.vias[i].diameter, self.board.vias[i].drill);
                let r1 = ui.add(egui::DragValue::new(&mut d).speed(0.05).range(0.3..=3.0).prefix("径 ").suffix(" mm"));
                let r2 = ui.add(egui::DragValue::new(&mut dr).speed(0.05).range(0.2..=2.0).prefix("穴 ").suffix(" mm"));
                if r1.drag_started() || r1.gained_focus() || r2.drag_started() || r2.gained_focus() {
                    self.checkpoint();
                }
                if r1.changed() || r2.changed() {
                    self.board.vias[i].diameter = d;
                    self.board.vias[i].drill = dr;
                }
            }
            Item::Zone(i) if i < self.board.zones.len() => {
                let nets = self.nets();
                let mut net = self.board.zones[i].net.clone();
                let before = net.clone();
                egui::ComboBox::from_label("ゾーンのネット").selected_text(net.clone()).show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut net, n.clone(), n);
                    }
                });
                let mut c = self.board.zones[i].clearance;
                let r = ui.add(egui::DragValue::new(&mut c).speed(0.05).range(0.1..=2.0).prefix("隙間 ").suffix(" mm"));
                let mut th = self.board.zones[i].thermal;
                let rt = ui.checkbox(&mut th, "サーマルリリーフ");
                if net != before || r.drag_started() || r.gained_focus() || rt.changed() {
                    self.checkpoint();
                }
                let z = &mut self.board.zones[i];
                z.net = net;
                if r.changed() {
                    z.clearance = c;
                }
                z.thermal = th;
            }
            Item::Part(i) if i < self.board.parts.len() => {
                let p = &self.board.parts[i];
                let (r, side) = (p.refdes.clone(), p.side);
                ui.label(format!("{}  {}  [{}]  {}", p.refdes, p.value, p.footprint, if side == Side::Bottom { "裏面実装" } else { "表面実装" }));
                ui.small("ドラッグで移動 / R で回転 / B で表裏を反転");
                let _ = r;
                if ui.button(if side == Side::Bottom { "表面に移す (B)" } else { "裏面に移す (B)" }).clicked() {
                    self.flip_selected_part();
                }
            }
            Item::Hole(i) if i < self.board.holes.len() => {
                ui.label(format!("取付穴 φ{:.2} mm", self.board.holes[i].drill));
            }
            _ => {}
        }
        if !matches!(sel, Item::Part(_)) && ui.button("削除 (Delete)").clicked() {
            self.delete(sel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn route_modes_bend_as_documented() {
        let mut e = BoardEditor::default();
        // 45 degree: straight run first, then the diagonal
        assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(6.0, 0.0), p(10.0, 4.0)]);
        assert_eq!(e.route_points(p(0.0, 0.0), p(4.0, -10.0)), vec![p(0.0, -6.0), p(4.0, -10.0)]);
        e.bend_flip = true;
        assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(4.0, 4.0), p(10.0, 4.0)]);
        // exact diagonal and axis-aligned moves need no bend
        assert_eq!(e.route_points(p(0.0, 0.0), p(5.0, 5.0)), vec![p(5.0, 5.0)]);
        assert_eq!(e.route_points(p(0.0, 0.0), p(5.0, 0.0)), vec![p(5.0, 0.0)]);
        e.route_mode = RouteMode::Orthogonal;
        assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(0.0, 4.0), p(10.0, 4.0)]);
        e.bend_flip = false;
        assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(10.0, 0.0), p(10.0, 4.0)]);
        e.route_mode = RouteMode::Free;
        assert_eq!(e.route_points(p(0.0, 0.0), p(10.0, 4.0)), vec![p(10.0, 4.0)]);
    }

    #[test]
    fn routing_clicks_create_connected_segments_and_undo_removes_them() {
        let mut e = BoardEditor::default();
        e.tool = Tool::Track;
        e.click(p(5.0, 5.0));
        e.click(p(15.0, 9.0)); // 45 degree route: two segments
        assert_eq!(e.board.tracks.len(), 2);
        let (t0, t1) = (&e.board.tracks[0], &e.board.tracks[1]);
        assert_eq!((t0.a, t1.b), (p(5.0, 5.0), p(15.0, 9.0)));
        assert_eq!(t0.b, t1.a);
        assert_eq!(e.pending, Some(p(15.0, 9.0)), "the route continues from the last point");
        e.click(p(15.0, 9.0)); // zero-length: nothing added
        assert_eq!(e.board.tracks.len(), 2);

        e.undo();
        assert_eq!(e.board.tracks.len(), 2, "the zero-length click made a checkpoint of the same state");
        e.undo();
        assert_eq!(e.board.tracks.len(), 0);
        e.redo();
        assert_eq!(e.board.tracks.len(), 2);
        // the first click of a route only picks the start, it is not an edit
        e.click(p(1.0, 1.0));
        assert!(!e.redo.is_empty());
        // a real edit clears the redo history
        e.click(p(3.0, 1.0));
        assert!(e.redo.is_empty());
    }

    #[test]
    fn via_while_routing_switches_layer() {
        let mut e = BoardEditor::default();
        e.tool = Tool::Track;
        e.route_mode = RouteMode::Free;
        e.pending = Some(p(5.0, 5.0));
        e.route_via(p(5.0, 5.0), p(10.0, 5.0));
        assert_eq!((e.board.tracks.len(), e.board.vias.len()), (1, 1));
        assert_eq!(e.board.tracks[0].side, Side::Top);
        assert_eq!(e.side, Side::Bottom);
        assert_eq!(e.pending, Some(p(10.0, 5.0)));
        e.click(p(10.0, 12.0));
        assert_eq!(e.board.tracks[1].side, Side::Bottom);
        // top and bottom tracks are joined through the via
        let c = connectivity(&e.board);
        assert_eq!(c.track_group(0), c.track_group(1));
        assert_eq!(c.via_group(0), c.track_group(0));
    }

    #[test]
    fn polygon_tools_need_three_points() {
        let mut e = BoardEditor::default();
        e.tool = Tool::OutlinePoly;
        e.click(p(0.0, 0.0));
        e.click(p(10.0, 0.0));
        e.finish_polygon();
        assert_eq!(e.board.outline.len(), 4, "unchanged: two points are not a polygon");
        assert!(e.status.contains("3点"));

        e.tool = Tool::OutlinePoly;
        e.poly = vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(10.0, 10.0), p(10.0, 20.0), p(0.0, 20.0)];
        e.finish_polygon();
        assert_eq!(e.board.outline.len(), 6, "an L-shaped board");
        e.undo();
        assert_eq!(e.board.outline.len(), 4);

        // zone: the duplicate last point from a double-click is dropped
        e.tool = Tool::Zone;
        e.zone_net = "GND".into();
        e.poly = vec![p(1.0, 1.0), p(9.0, 1.0), p(9.0, 9.0), p(9.0, 9.0)];
        e.finish_polygon();
        assert_eq!(e.board.zones.len(), 1);
        assert_eq!(e.board.zones[0].polygon.len(), 3);
        assert_eq!((e.board.zones[0].net.as_str(), e.board.zones[0].side), ("GND", Side::Top));
    }

    #[test]
    fn closing_a_polygon_by_clicking_the_first_point() {
        let mut e = BoardEditor::default();
        e.tool = Tool::Zone;
        for q in [p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0)] {
            e.click(q);
        }
        assert!(e.board.zones.is_empty());
        e.click(p(2.0, 2.0));
        assert_eq!(e.board.zones.len(), 1);
        assert!(e.poly.is_empty());
    }

    #[test]
    fn picking_and_deleting() {
        let mut e = BoardEditor::default();
        e.board.tracks.push(Track { side: Side::Top, a: p(5.0, 5.0), b: p(15.0, 5.0), width: 0.25 });
        e.board.zones.push(Zone::new("GND", Side::Top, vec![p(20.0, 20.0), p(30.0, 20.0), p(30.0, 30.0), p(20.0, 30.0)]));
        assert!(matches!(e.hit_test(p(10.0, 5.1)), Some(Item::Track(0))));
        assert!(matches!(e.hit_test(p(25.0, 25.0)), Some(Item::Zone(0))), "inside a zone selects it");
        assert!(e.hit_test(p(40.0, 5.0)).is_none());
        // hidden layers cannot be picked
        e.vis.top = false;
        assert!(e.hit_test(p(10.0, 5.1)).is_none());
        e.vis.top = true;

        e.delete(Item::Track(0));
        e.delete(Item::Zone(0));
        assert!(e.board.tracks.is_empty() && e.board.zones.is_empty());
        e.undo();
        assert_eq!(e.board.zones.len(), 1);
        e.undo();
        assert_eq!(e.board.tracks.len(), 1);
    }

    #[test]
    fn parts_cannot_be_deleted_from_the_board() {
        let mut e = BoardEditor::default();
        e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        e.delete(Item::Part(0));
        assert_eq!(e.board.parts.len(), 1);
        assert!(e.status.contains("回路図"));
        assert!(e.undo.is_empty());
    }

    #[test]
    fn fab_files_include_assembly_csv() {
        let mut e = BoardEditor::default();
        e.board.name = "unit".into();
        e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        let dir = std::env::temp_dir().join(format!("circuitcad_fab_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let n = e.write_fab_files(&dir).unwrap();
        assert_eq!(n, 11); // 9 Gerber/drill files + BOM + CPL
        assert!(std::fs::read_to_string(dir.join("unit-BOM.csv")).unwrap().contains("R1"));
        assert!(std::fs::read_to_string(dir.join("unit-CPL.csv")).unwrap().contains("R1,10.0000mm"));
        assert!(dir.join("unit_gerber.zip").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flipping_the_selected_part_is_undoable() {
        let mut e = BoardEditor::default();
        e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
        e.flip_selected_part(); // nothing selected: no-op
        assert!(e.undo.is_empty());
        e.selected = Some(Item::Part(0));
        e.flip_selected_part();
        assert_eq!(e.board.parts[0].side, Side::Bottom);
        assert!(e.board.pads.iter().all(|q| q.side == Side::Bottom));
        assert!(e.status.contains("裏面"), "{}", e.status);
        e.flip_selected_part();
        assert_eq!(e.board.parts[0].side, Side::Top);
        e.undo();
        assert_eq!(e.board.parts[0].side, Side::Bottom);
        e.undo();
        assert_eq!(e.board.parts[0].side, Side::Top);
    }

    #[test]
    fn pads_on_a_hidden_side_cannot_be_picked() {
        let mut e = BoardEditor::default();
        e.board.set_part(pcb_core::Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: p(10.0, 10.0), rot: 0, side: Side::Bottom }).unwrap();
        let on_pad = e.board.pads[0].pos;
        assert!(matches!(e.hit_test(on_pad), Some(Item::Part(0))));
        e.vis.bottom = false;
        assert!(e.hit_test(on_pad).is_none());
    }
}
