use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pcb_core::{drc, export, Board, Hole, Pad, PadShape, Point, Side, SilkLine, Track, Via};
use schematic::SchEditor;
use std::path::PathBuf;

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
        for (i, l) in self.board.silk.iter().enumerate() {
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
                        self.board.silk.push(SilkLine { side: self.side, a, b: p, width: 0.15 });
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
            Tool::PadTh => self.board.pads.push(Pad {
                pos: p,
                shape: PadShape::Circle { d: self.pad_size },
                drill: Some(self.pad_drill),
                side: self.side,
            }),
            Tool::PadSmd => self.board.pads.push(Pad {
                pos: p,
                shape: PadShape::Rect { w: self.pad_size, h: self.pad_size * 0.7 },
                drill: None,
                side: self.side,
            }),
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
        for v in &self.violations {
            painter.circle_stroke(self.to_screen(rect, v.at), 10.0, Stroke::new(2.0, Color32::from_rgb(255, 0, 255)));
        }
        if let Some(sel) = self.selected {
            let pts: Vec<Point> = match sel {
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
            let sp = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            let scr = self.to_screen(rect, sp);
            painter.circle_stroke(scr, 4.0, Stroke::new(1.0, Color32::WHITE));
            if let Some(a) = self.pending {
                painter.line_segment([self.to_screen(rect, a), scr], Stroke::new(1.0, Color32::WHITE));
            }
            self.status = format!("X {:.2}  Y {:.2} mm   [{}]", sp.x, sp.y, if self.side == Side::Top { "表面" } else { "裏面" });
            if resp.clicked_by(egui::PointerButton::Primary) {
                self.click(sp);
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

fn main() -> eframe::Result {
    eframe::run_native(
        "circuitCad",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            Ok(Box::new(App::default()))
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
