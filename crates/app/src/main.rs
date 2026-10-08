// No console window on Windows (panics are written to %APPDATA%\circuitCad\crash.log instead).
#![windows_subsystem = "windows"]

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use pcb_core::ratsnest::ratsnest;
use pcb_core::{drc, export, Board, Hole, Pad, PadShape, Point, Side, SilkLine, Track, Via};
use schematic::SchEditor;
use std::path::PathBuf;

mod part_form;
mod plot;
mod schematic;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Schematic,
    Board,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Track,
    PadTh,
    PadSmd,
    Via,
    Hole,
    Silk,
    Outline,
}

const TOOLS: &[(Tool, &str)] = &[
    (Tool::Select, "選択/削除"),
    (Tool::Track, "配線"),
    (Tool::PadTh, "パッド(スルーホール)"),
    (Tool::PadSmd, "パッド(SMD)"),
    (Tool::Via, "ビア"),
    (Tool::Hole, "取付穴"),
    (Tool::Silk, "シルク線"),
    (Tool::Outline, "基板外形(矩形)"),
];

#[derive(Clone, Copy, PartialEq)]
enum Item {
    Track(usize),
    Pad(usize),
    Via(usize),
    Hole(usize),
    Silk(usize),
    /// Index into `Board::parts`.
    Part(usize),
}

struct App {
    mode: Mode,
    sch: SchEditor,
    board: Board,
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
    zoom: f32, // pixels per mm
    pan: Vec2, // world mm at screen centre (x,y)
    pending: Option<Point>,
    selected: Option<Item>,
    status: String,
    violations: Vec<drc::Violation>,
    drag_part: Option<(String, Point)>,
    import_notes: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        let board = Board::new("board", 50.0, 40.0);
        Self {
            mode: Mode::Schematic,
            sch: SchEditor::default(),
            board,
            path: None,
            tool: Tool::Track,
            side: Side::Top,
            grid: 0.5,
            track_width: 0.25,
            pad_size: 1.6,
            pad_drill: 0.8,
            via_dia: 0.6,
            via_drill: 0.3,
            hole_drill: 3.2,
            zoom: 12.0,
            pan: Vec2::new(25.0, 20.0),
            pending: None,
            selected: None,
            status: "準備完了".into(),
            violations: vec![],
            drag_part: None,
            import_notes: vec![],
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

impl App {
    fn to_screen(&self, rect: Rect, p: Point) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + (p.x as f32 - self.pan.x) * self.zoom,
            c.y - (p.y as f32 - self.pan.y) * self.zoom,
        )
    }

    fn to_world(&self, rect: Rect, s: Pos2) -> Point {
        let c = rect.center();
        Point::new(
            ((s.x - c.x) / self.zoom + self.pan.x) as f64,
            (-(s.y - c.y) / self.zoom + self.pan.y) as f64,
        )
    }

    fn side_color(&self, s: Side) -> Color32 {
        match s {
            Side::Top => Color32::from_rgb(220, 60, 60),
            Side::Bottom => Color32::from_rgb(70, 110, 230),
        }
    }

    fn hit_test(&self, p: Point) -> Option<Item> {
        let tol = 6.0 / self.zoom as f64;
        for (i, v) in self.board.vias.iter().enumerate() {
            if p.dist(v.pos) <= v.diameter / 2.0 + tol {
                return Some(Item::Via(i));
            }
        }
        for (i, pd) in self.board.pads.iter().enumerate() {
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
        for (i, t) in self.board.tracks.iter().enumerate() {
            if seg_dist(p, t.a, t.b) <= t.width / 2.0 + tol {
                return Some(Item::Track(i));
            }
        }
        for (i, l) in self.board.silk.iter().enumerate().filter(|(_, l)| l.refdes.is_empty()) {
            if seg_dist(p, l.a, l.b) <= l.width / 2.0 + tol {
                return Some(Item::Silk(i));
            }
        }
        None
    }

    fn delete(&mut self, it: Item) {
        match it {
            Item::Track(i) => drop(self.board.tracks.remove(i)),
            Item::Pad(i) => drop(self.board.pads.remove(i)),
            Item::Via(i) => drop(self.board.vias.remove(i)),
            Item::Hole(i) => drop(self.board.holes.remove(i)),
            Item::Silk(i) => drop(self.board.silk.remove(i)),
            Item::Part(_) => {
                self.status = "部品は回路図側で削除してから「回路図から取り込み」を実行してください".into();
                return;
            }
        }
        self.selected = None;
    }

    fn click(&mut self, p: Point) {
        match self.tool {
            Tool::Select => self.selected = self.hit_test(p),
            Tool::Track | Tool::Silk | Tool::Outline => match self.pending.take() {
                None => self.pending = Some(p),
                Some(a) => match self.tool {
                    Tool::Track => {
                        self.board.tracks.push(Track { side: self.side, a, b: p, width: self.track_width });
                        self.pending = Some(p); // continue the route
                    }
                    Tool::Silk => {
                        self.board.silk.push(SilkLine { side: self.side, a, b: p, width: 0.15, refdes: String::new() });
                        self.pending = Some(p);
                    }
                    _ => {
                        let (x0, x1) = (a.x.min(p.x), a.x.max(p.x));
                        let (y0, y1) = (a.y.min(p.y), a.y.max(p.y));
                        if x1 > x0 && y1 > y0 {
                            self.board.outline = vec![
                                Point::new(x0, y0),
                                Point::new(x1, y0),
                                Point::new(x1, y1),
                                Point::new(x0, y1),
                            ];
                        }
                    }
                },
            },
            Tool::PadTh => {
                self.board.pads.push(Pad::new(p, PadShape::Circle { d: self.pad_size }, Some(self.pad_drill), self.side))
            }
            Tool::PadSmd => {
                self.board.pads.push(Pad::new(p, PadShape::Rect { w: self.pad_size, h: self.pad_size * 0.7 }, None, self.side))
            }
            Tool::Via => self.board.vias.push(Via { pos: p, diameter: self.via_dia, drill: self.via_drill }),
            Tool::Hole => self.board.holes.push(Hole { pos: p, drill: self.hole_drill }),
        }
    }

    fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad", &["pcbjson"]).set_file_name("board.pcbjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            match serde_json::to_string_pretty(&self.board).map(|s| std::fs::write(&p, s)) {
                Ok(Ok(())) => {
                    self.status = format!("保存しました: {}", p.display());
                    self.path = Some(p);
                }
                Ok(Err(e)) => self.status = format!("保存失敗: {e}"),
                Err(e) => self.status = format!("保存失敗: {e}"),
            }
        }
    }

    fn open(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad", &["pcbjson"]).pick_file() {
            match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
                Ok(b) => {
                    self.board = b;
                    self.status = format!("開きました: {}", p.display());
                    self.path = Some(p);
                    self.selected = None;
                    self.pending = None;
                }
                Err(e) => self.status = format!("読み込み失敗: {e}"),
            }
        }
    }

    fn export(&mut self) {
        self.violations = drc::check(&self.board);
        if !self.violations.is_empty() {
            self.status = format!("DRC違反が{}件あります。修正してから書き出してください", self.violations.len());
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("製造ファイルの出力先フォルダ").pick_folder() else { return };
        let mut result = Ok(());
        for (name, data) in export::fab_files(&self.board) {
            if let Err(e) = std::fs::write(dir.join(&name), data) {
                result = Err(e.to_string());
                break;
            }
        }
        if result.is_ok() {
            result = export::zip_bytes(&self.board)
                .map_err(|e| e.to_string())
                .and_then(|z| std::fs::write(dir.join(format!("{}_gerber.zip", self.board.name)), z).map_err(|e| e.to_string()));
        }
        self.status = match result {
            Ok(()) => format!("書き出し完了: {} (Gerber + ドリル + zip)", dir.display()),
            Err(e) => format!("書き出し失敗: {e}"),
        };
    }

    fn import_from_schematic(&mut self) {
        let sch = &self.sch.sch;
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
        for c in sch.components.iter().filter(|c| c.custom.is_some()) {
            if let Some(def) = self.sch.library.iter().find(|d| d.footprint_name() == c.footprint) {
                if let Ok(fp) = def.footprint() {
                    self.board.add_footprint(fp);
                }
            }
        }
        let rep = netlist::import(&mut self.board, &parts, &nets);
        self.selected = None;
        self.import_notes = rep.warnings.clone();
        self.status = format!(
            "取り込み完了: 追加 {} / 更新 {} / 削除 {} / 警告 {}",
            rep.added.len(),
            rep.updated,
            rep.removed.len(),
            rep.warnings.len()
        );
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));

        // pan (middle/right drag) and zoom (wheel, around cursor)
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

        // grid
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

        let z = self.zoom;
        let o = &self.board.outline;
        for i in 0..o.len() {
            let (a, b) = (self.to_screen(rect, o[i]), self.to_screen(rect, o[(i + 1) % o.len()]));
            painter.line_segment([a, b], Stroke::new(2.0, Color32::YELLOW));
        }
        // draw bottom first, active side on top
        for s in [self.side.flip(), self.side] {
            let alpha = if s == self.side { 255 } else { 90 };
            let col = self.side_color(s).gamma_multiply(alpha as f32 / 255.0);
            for t in self.board.tracks.iter().filter(|t| t.side == s) {
                painter.line_segment(
                    [self.to_screen(rect, t.a), self.to_screen(rect, t.b)],
                    Stroke::new((t.width as f32 * z).max(1.0), col),
                );
            }
            for l in self.board.silk.iter().filter(|l| l.side == s) {
                painter.line_segment(
                    [self.to_screen(rect, l.a), self.to_screen(rect, l.b)],
                    Stroke::new((l.width as f32 * z).max(1.0), Color32::from_rgba_unmultiplied(230, 230, 230, alpha)),
                );
            }
        }
        for p in &self.board.pads {
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
                painter.circle_filled(c, d as f32 * z / 2.0, Color32::from_rgb(18, 22, 18));
            }
        }
        for v in &self.board.vias {
            let c = self.to_screen(rect, v.pos);
            painter.circle_filled(c, v.diameter as f32 * z / 2.0, Color32::from_rgb(60, 170, 90));
            painter.circle_filled(c, v.drill as f32 * z / 2.0, Color32::from_rgb(18, 22, 18));
        }
        for h in &self.board.holes {
            painter.circle_stroke(self.to_screen(rect, h.pos), h.drill as f32 * z / 2.0, Stroke::new(1.5, Color32::LIGHT_GRAY));
        }
        for w in ratsnest(&self.board) {
            painter.line_segment(
                [self.to_screen(rect, w.a), self.to_screen(rect, w.b)],
                Stroke::new(1.0, Color32::from_rgb(240, 220, 90)),
            );
        }
        if z >= 14.0 {
            let f = egui::FontId::proportional(10.0);
            for p in self.board.pads.iter().filter(|p| !p.net.is_empty()) {
                painter.text(self.to_screen(rect, p.pos), egui::Align2::CENTER_CENTER, &p.net, f.clone(), Color32::WHITE);
            }
        }
        for part in &self.board.parts {
            let top = self.board.silk.iter().filter(|l| l.refdes == part.refdes).map(|l| l.a.y.max(l.b.y)).fold(f64::MIN, f64::max);
            if top > f64::MIN {
                painter.text(
                    self.to_screen(rect, Point::new(part.pos.x, top)) - Vec2::new(0.0, 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{} {}", part.refdes, part.value),
                    egui::FontId::proportional(11.0),
                    Color32::from_rgb(230, 230, 230),
                );
            }
        }
        for v in &self.violations {
            painter.circle_stroke(self.to_screen(rect, v.at), 10.0, Stroke::new(2.0, Color32::from_rgb(255, 0, 255)));
        }
        if let Some(sel) = self.selected {
            let pts: Vec<Point> = match sel {
                Item::Part(i) => {
                    let r = &self.board.parts[i].refdes;
                    self.board.pads.iter().filter(|p| &p.refdes == r).map(|p| p.pos).collect()
                }
                Item::Track(i) => vec![self.board.tracks[i].a, self.board.tracks[i].b],
                Item::Silk(i) => vec![self.board.silk[i].a, self.board.silk[i].b],
                Item::Pad(i) => vec![self.board.pads[i].pos],
                Item::Via(i) => vec![self.board.vias[i].pos],
                Item::Hole(i) => vec![self.board.holes[i].pos],
            };
            for p in pts {
                painter.circle_stroke(self.to_screen(rect, p), 8.0, Stroke::new(2.0, Color32::WHITE));
            }
        }

        // cursor / rubber band
        if let Some(pos) = resp.hover_pos() {
            let w = self.to_world(rect, pos);
            let mut sp = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            if self.tool == Tool::Track {
                // snap routing to pad / via centres so footprints off the grid can be connected
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
                    sp = c;
                }
            }
            let scr = self.to_screen(rect, sp);
            painter.circle_stroke(scr, 4.0, Stroke::new(1.0, Color32::WHITE));
            if let Some(a) = self.pending {
                painter.line_segment([self.to_screen(rect, a), scr], Stroke::new(1.0, Color32::WHITE));
            }
            self.status = format!("X {:.2}  Y {:.2} mm   [{}]", sp.x, sp.y, if self.side == Side::Top { "表面" } else { "裏面" });
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select {
                if let Some(Item::Part(i)) = self.hit_test(w) {
                    let part = &self.board.parts[i];
                    self.drag_part = Some((part.refdes.clone(), Point::new(sp.x - part.pos.x, sp.y - part.pos.y)));
                    self.selected = Some(Item::Part(i));
                }
            }
            if let Some((r, off)) = self.drag_part.clone() {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    if let Some(rot) = self.board.parts.iter().find(|p| p.refdes == r).map(|p| p.rot) {
                        self.board.place_part(&r, Point::new(sp.x - off.x, sp.y - off.y), rot);
                    }
                }
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                self.click(sp);
            }
        }
        if resp.drag_stopped() {
            self.drag_part = None;
        }
        if let Some(Item::Part(i)) = self.selected {
            if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::R)) {
                let p = self.board.parts[i].clone();
                self.board.place_part(&p.refdes, p.pos, (p.rot + 1) % 4);
            }
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Schematic, "回路図");
                ui.selectable_value(&mut self.mode, Mode::Board, "基板パターン");
                ui.separator();
                let sch_mode = self.mode == Mode::Schematic;
                ui.menu_button("ファイル", |ui| {
                    if ui.button("新規").clicked() {
                        if sch_mode {
                            self.sch.new_file();
                        } else {
                            let sch = std::mem::take(&mut self.sch);
                            *self = App { sch, ..App::default() };
                        }
                    }
                    if ui.button("開く…").clicked() {
                        if sch_mode { self.sch.open() } else { self.open() }
                    }
                    if ui.button("保存").clicked() {
                        if sch_mode { self.sch.save(false) } else { self.save(false) }
                    }
                    if ui.button("名前を付けて保存…").clicked() {
                        if sch_mode { self.sch.save(true) } else { self.save(true) }
                    }
                    if !sch_mode {
                        ui.separator();
                        if ui.button("製造ファイルを書き出し (Gerber/ドリル)…").clicked() {
                            self.export();
                        }
                    }
                });
                if !sch_mode && ui.button("DRC").clicked() {
                    self.violations = drc::check(&self.board);
                    self.status = format!("DRC: 違反 {} 件", self.violations.len());
                }
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(if self.mode == Mode::Schematic { &self.sch.status } else { &self.status });
        });
        if self.mode == Mode::Schematic {
            egui::Panel::left("sch_tools").min_size(220.0).show(ui, |ui| self.sch.side_panel(ui));
            if self.sch.has_wave() {
                egui::Panel::bottom("wave").resizable(true).default_size(320.0).show(ui, |ui| self.sch.wave_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| self.sch.canvas(ui));
            return;
        }
        egui::Panel::left("tools").min_size(220.0).show(ui, |ui| {
            ui.heading("ツール");
            for (t, name) in TOOLS {
                if ui.selectable_label(self.tool == *t, *name).clicked() {
                    self.tool = *t;
                    self.pending = None;
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.side, Side::Top, "表面");
                ui.selectable_value(&mut self.side, Side::Bottom, "裏面");
            });
            ui.add(egui::DragValue::new(&mut self.grid).speed(0.05).range(0.0..=5.0).prefix("グリッド ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.track_width).speed(0.01).range(0.05..=5.0).prefix("線幅 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.pad_size).speed(0.05).range(0.3..=10.0).prefix("パッド径 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.pad_drill).speed(0.05).range(0.2..=6.0).prefix("パッド穴 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.via_dia).speed(0.05).range(0.3..=3.0).prefix("ビア径 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.via_drill).speed(0.05).range(0.2..=2.0).prefix("ビア穴 ").suffix(" mm"));
            ui.add(egui::DragValue::new(&mut self.hole_drill).speed(0.1).range(0.5..=10.0).prefix("取付穴 ").suffix(" mm"));
            ui.separator();
            if ui.button("回路図から取り込み").clicked() {
                self.import_from_schematic();
            }
            for n in &self.import_notes {
                ui.colored_label(Color32::from_rgb(255, 180, 60), n);
            }
            let unrouted = ratsnest(&self.board).len();
            ui.label(format!("未配線: {unrouted} 本 (黄色の線)"));
            ui.label("基板名 (出力ファイル名)");
            ui.text_edit_singleline(&mut self.board.name);
            if let Some(it) = self.selected {
                if ui.button("選択項目を削除").clicked() {
                    self.delete(it);
                }
            }
            if !self.violations.is_empty() {
                ui.separator();
                ui.colored_label(Color32::from_rgb(255, 80, 255), "DRC違反");
                for v in &self.violations {
                    ui.label(format!("({:.1}, {:.1}) {}", v.at.x, v.at.y, v.message));
                }
            }
            ui.separator();
            ui.small("左クリック: 配置 / 右クリック: 配線を中断\n中・右ドラッグ: 移動 / ホイール: 拡大縮小");
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
    }
}

/// Without a console a panic would vanish silently: keep it in a log file.
fn install_crash_log() {
    let dir = part_lib::default_dir().parent().map(|p| p.to_path_buf());
    std::panic::set_hook(Box::new(move |info| {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::write(d.join("crash.log"), format!("{info}
"));
        }
    }));
}

fn main() -> eframe::Result {
    install_crash_log();
    eframe::run_native(
        "circuitCad",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_maximized(true).with_title("circuitCad"),
            ..Default::default()
        },
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            let mut app = App::default();
            // usage: app [circuit.schjson] [--dc]
            let args: Vec<String> = std::env::args().skip(1).collect();
            if let Some(p) = args.iter().find(|a| a.ends_with(".schjson")) {
                app.sch.load_path(p.into());
                if args.iter().any(|a| a == "--dc") {
                    app.sch.analyze_dc();
                }
                // --ac <fstart> <fstop> <points/decade>   e.g. --ac 10 1Meg 20
                if let Some(i) = args.iter().position(|a| a == "--ac") {
                    if let (Some(f0), Some(f1), Some(p)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                        app.sch.set_ac_settings(f0, f1, p);
                        app.sch.analyze_ac();
                    }
                }
                // --tran <stop> <step>   e.g. --tran 20m 20u
                if let Some(i) = args.iter().position(|a| a == "--tran") {
                    if let (Some(stop), Some(step)) = (args.get(i + 1), args.get(i + 2)) {
                        app.sch.set_tran_settings(stop, step);
                        app.sch.analyze_tran();
                        if args.iter().any(|a| a == "--spectrum") {
                            app.sch.show_spectrum();
                        }
                    }
                }
            }
            Ok(Box::new(app))
        }),
    )
}

/// egui's built-in fonts lack Japanese glyphs; borrow one from Windows.
fn install_japanese_font(ctx: &egui::Context) {
    for path in [r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\meiryo.ttc", r"C:\Windows\Fonts\msgothic.ttc"] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("jp".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push("jp".into());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sch_core::{Kind, Wire};

    /// V1 -> R1 -> R2 -> GND drawn as a schematic, imported into the board.
    #[test]
    fn schematic_to_board_flow() {
        let mut app = App::default();
        let s = &mut app.sch.sch;
        s.add(Kind::VSource, (0, 0), 0);
        s.add(Kind::Resistor, (6, -6), 0);
        s.add(Kind::Resistor, (8, 0), 1);
        s.add(Kind::Ground, (0, 6), 0);
        let r2 = s.components[2].pins();
        for (a, b) in [
            ((0, -2), (0, -6)),
            ((0, -6), (4, -6)),
            ((8, -6), r2[0].1),
            (r2[1].1, (8, 4)),
            ((8, 4), (0, 4)),
            ((0, 2), (0, 6)),
        ] {
            s.wires.push(Wire { a, b });
        }

        app.import_from_schematic();
        assert!(app.import_notes.is_empty(), "{:?}", app.import_notes);
        assert_eq!(app.board.parts.len(), 3); // V1, R1, R2 (GND symbol is not a part)
        let net = |r: &str, p: &str| app.board.pads.iter().find(|x| x.refdes == r && x.pin == p).unwrap().net.clone();
        assert_eq!(net("V1", "-"), "GND");
        assert_eq!(net("R2", "2"), "GND");
        assert_eq!(net("R1", "2"), net("R2", "1"));
        // GND: V1.- and R2.2 ; nets +V/R1.1 and R1.2/R2.1 : three 2-pad nets => 3 airwires
        assert_eq!(ratsnest(&app.board).len(), 3);

        // route one net by hand and the airwire disappears
        let a = app.board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "2").unwrap().pos;
        let b = app.board.pads.iter().find(|p| p.refdes == "R2" && p.pin == "1").unwrap().pos;
        app.board.tracks.push(Track { side: Side::Top, a, b, width: 0.25 });
        assert_eq!(ratsnest(&app.board).len(), 2);
    }
    /// A library part (custom symbol + its own package) travels schematic -> netlist -> board.
    #[test]
    fn library_part_reaches_the_board() {
        let mut app = App::default();
        let def = part_lib::EXAMPLES
            .iter()
            .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
            .find(|d| d.name == "LM358")
            .unwrap();
        app.sch.library = vec![def.clone()];
        let u = app.sch.sch.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 10), 0);
        let r = app.sch.sch.add(Kind::Resistor, (34, 10), 0);
        let _ = r;
        let out = app.sch.sch.components[u].pins().iter().find(|p| p.0 == "1").unwrap().1;
        app.sch.sch.wires.push(Wire { a: out, b: (32, 10) });

        app.import_from_schematic();
        assert!(app.import_notes.is_empty(), "{:?}", app.import_notes);
        assert!(app.board.footprints.iter().any(|f| f.name == "SOIC-8_P1.27"));
        assert_eq!(app.board.pads.iter().filter(|p| p.refdes == "U1").count(), 8);
        let u1_out = app.board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap().clone();
        let r1_in = app.board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "1").unwrap();
        assert!(!u1_out.net.is_empty() && u1_out.net == r1_in.net);
        // moving the part keeps its nets
        app.board.place_part("U1", Point::new(10.0, 10.0), 0);
        let again = app.board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap();
        assert_eq!(again.net, u1_out.net);
        assert_eq!(ratsnest(&app.board).len(), 1);
    }

    #[test]
    fn dc_analysis_from_the_editor() {
        let mut app = App::default();
        let s = &mut app.sch.sch;
        s.add(Kind::VSource, (0, 0), 0);
        s.components[0].value = "10".into();
        s.add(Kind::Resistor, (6, -6), 0);
        s.add(Kind::Resistor, (12, 0), 1);
        s.add(Kind::Ground, (0, 6), 0);
        let r2 = s.components[2].pins();
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), r2[0].1), (r2[1].1, (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
            s.wires.push(Wire { a, b });
        }
        app.sch.analyze_dc();
        assert!(app.sch.status.contains("収束"), "{}", app.sch.status);
        // R1 = R2 = 10k default: the midpoint is at half the supply
        assert!((app.sch.pin_voltage("R2", "1").unwrap() - 5.0).abs() < 1e-6);
        assert!(app.sch.pin_voltage("V1", "-").unwrap().abs() < 1e-9);

        // a broken value is reported instead of a result
        app.sch.sch.components[1].value = "oops".into();
        app.sch.analyze_dc();
        assert!(app.sch.status.contains("失敗"));
    }
}
