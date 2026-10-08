//! Schematic editor: place parts, wire them together, run ERC, export netlist.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use crate::part_form::PartForm;
use part_lib::PartDef;
use sim_core::{dc_op, from_schematic, DcResult};
use sch_core::{erc, extract, on_segment, to_spice, Component, CustomSymbol, Kind, Label, PinSide, Problem, Pt, Schematic, Wire};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Place,
    Wire,
    Label,
}

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    Comp(usize),
    Wire(usize),
    Label(usize),
}

pub struct SchEditor {
    pub sch: Schematic,
    path: Option<PathBuf>,
    tool: Tool,
    place: Kind,
    rot: u8,
    label_text: String,
    pending: Option<Pt>,
    selected: Option<Sel>,
    drag: Option<(usize, Pt)>,
    zoom: f32, // pixels per grid step
    pan: Vec2, // grid position at the screen centre
    problems: Vec<Problem>,
    pub status: String,
    pub library: Vec<PartDef>,
    lib_errors: Vec<String>,
    lib_dir: PathBuf,
    lib_filter: String,
    place_lib: Option<usize>,
    form: Option<PartForm>,
    dc: Option<DcView>,
    show_v: bool,
    sig_cache: String,
}

/// A finished DC analysis, remembered with the schematic it was computed from.
struct DcView {
    outcome: Result<DcResult, Vec<String>>,
    signature: String,
    /// Voltage at every pin position.
    pin_v: Vec<(Pt, f64)>,
}

/// Engineering notation: 4.700 mA, 12.00 V, 250.0 nA.
fn eng(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (scale, prefix) = if a == 0.0 {
        (1.0, "")
    } else if a >= 1e9 {
        (1e-9, "G")
    } else if a >= 1e6 {
        (1e-6, "M")
    } else if a >= 1e3 {
        (1e-3, "k")
    } else if a >= 1.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else if a >= 1e-6 {
        (1e6, "µ")
    } else if a >= 1e-9 {
        (1e9, "n")
    } else {
        (1e12, "p")
    };
    format!("{:.3} {prefix}{unit}", v * scale)
}

impl Default for SchEditor {
    fn default() -> Self {
        let lib_dir = part_lib::default_dir();
        let _ = part_lib::install_examples(&lib_dir);
        let (library, lib_errors) = part_lib::load_dir(&lib_dir);
        Self {
            library,
            lib_errors,
            lib_dir,
            lib_filter: String::new(),
            place_lib: None,
            form: None,
            dc: None,
            show_v: true,
            sig_cache: String::new(),
            sch: Schematic::default(),
            path: None,
            tool: Tool::Place,
            place: Kind::Resistor,
            rot: 0,
            label_text: "SIG".into(),
            pending: None,
            selected: None,
            drag: None,
            zoom: 22.0,
            pan: Vec2::new(10.0, 6.0),
            problems: vec![],
            status: "回路図: 部品を選んで配置し、配線ツールで線をつなぎます".into(),
        }
    }
}

// ------------------------------------------------------------- symbol drawing

enum Sh {
    L(f32, f32, f32, f32),
    C(f32, f32, f32),
}

fn symbol(kind: Kind) -> Vec<Sh> {
    use Sh::*;
    match kind {
        Kind::Resistor => vec![
            L(-2., 0., -1., 0.),
            L(1., 0., 2., 0.),
            L(-1., -0.4, 1., -0.4),
            L(-1., 0.4, 1., 0.4),
            L(-1., -0.4, -1., 0.4),
            L(1., -0.4, 1., 0.4),
        ],
        Kind::Capacitor => vec![
            L(-2., 0., -0.25, 0.),
            L(0.25, 0., 2., 0.),
            L(-0.25, -0.8, -0.25, 0.8),
            L(0.25, -0.8, 0.25, 0.8),
        ],
        Kind::Inductor => {
            let mut v = vec![L(-2., 0., -1.2, 0.), L(1.2, 0., 2., 0.)];
            let n = 48;
            let pt = |i: i32| {
                let x = -1.2 + 2.4 * i as f32 / n as f32;
                (x, -0.5 * (std::f32::consts::PI * (x + 1.2) / 0.6).sin().abs())
            };
            for i in 0..n {
                let (a, b) = (pt(i), pt(i + 1));
                v.push(L(a.0, a.1, b.0, b.1));
            }
            v
        }
        Kind::Diode | Kind::Led => {
            let mut v = vec![
                L(-2., 0., -0.6, 0.),
                L(0.6, 0., 2., 0.),
                L(-0.6, -0.6, -0.6, 0.6),
                L(-0.6, -0.6, 0.6, 0.),
                L(-0.6, 0.6, 0.6, 0.),
                L(0.6, -0.6, 0.6, 0.6),
            ];
            if kind == Kind::Led {
                v.extend([L(0., -0.9, 0.6, -1.5), L(0.6, -0.9, 1.2, -1.5)]);
            }
            v
        }
        Kind::VSource => vec![
            L(0., -2., 0., -1.2),
            L(0., 1.2, 0., 2.),
            C(0., 0., 1.2),
            L(-0.25, -0.6, 0.25, -0.6),
            L(0., -0.85, 0., -0.35),
            L(-0.25, 0.6, 0.25, 0.6),
        ],
        Kind::Npn => vec![
            C(0.5, 0., 1.7),
            L(-2., 0., -0.5, 0.),
            L(-0.5, -1., -0.5, 1.),
            L(-0.5, -0.4, 2., -2.),
            L(-0.5, 0.4, 2., 2.),
            L(1.2, 1.5, 2., 2.),
            L(1.5, 1.1, 2., 2.),
        ],
        Kind::Header(n) => {
            let mut v = vec![
                L(0., -1., 1.2, -1.),
                L(0., -1., 0., 2. * n as f32 - 1.),
                L(1.2, -1., 1.2, 2. * n as f32 - 1.),
                L(0., 2. * n as f32 - 1., 1.2, 2. * n as f32 - 1.),
            ];
            for i in 0..n as i32 {
                v.push(L(1.2, 2. * i as f32, 2., 2. * i as f32));
                v.push(C(0.6, 2. * i as f32, 0.25));
            }
            v
        }
        Kind::Ground => vec![
            L(0., 0., 0., 0.5),
            L(-0.8, 0.5, 0.8, 0.5),
            L(-0.5, 0.8, 0.5, 0.8),
            L(-0.2, 1.1, 0.2, 1.1),
        ],
        Kind::Power => vec![L(0., 0., 0., -0.6), L(-0.6, -0.6, 0.6, -0.6)],
        Kind::Custom => vec![],
    }
}

/// Local bounding box (xmin, xmax, ymin, ymax).
fn extents(kind: Kind) -> (f32, f32, f32, f32) {
    match kind {
        Kind::VSource => (-1.4, 1.4, -2., 2.),
        Kind::Npn => (-2., 2., -2., 2.),
        Kind::Header(n) => (0., 2., -1., 2. * n as f32 - 1.),
        Kind::Custom => (-2., 2., -2., 2.),
        Kind::Ground => (-1., 1., 0., 1.2),
        Kind::Power => (-1., 1., -1., 0.),
        _ => (-2., 2., -1., 1.),
    }
}

fn comp_extents(c: &Component) -> (f32, f32, f32, f32) {
    match &c.custom {
        Some(sym) => {
            let l = sym.layout();
            let (w, h) = ((l.hw + 2) as f32, (l.hh + 2) as f32);
            (-w, w, -h, h)
        }
        None => extents(c.kind),
    }
}

fn rot_f(p: (f32, f32), rot: u8) -> (f32, f32) {
    let mut q = p;
    for _ in 0..(rot % 4) {
        q = (-q.1, q.0);
    }
    q
}

const SCH_BG: Color32 = Color32::from_rgb(250, 248, 235);
const SYM: Color32 = Color32::from_rgb(120, 20, 20);
const WIRE: Color32 = Color32::from_rgb(0, 110, 30);

impl SchEditor {
    fn to_screen(&self, rect: Rect, p: (f32, f32)) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.0 - self.pan.x) * self.zoom, c.y + (p.1 - self.pan.y) * self.zoom)
    }

    fn to_grid(&self, rect: Rect, s: Pos2) -> (f32, f32) {
        let c = rect.center();
        ((s.x - c.x) / self.zoom + self.pan.x, (s.y - c.y) / self.zoom + self.pan.y)
    }

    fn draw_symbol(&self, painter: &egui::Painter, rect: Rect, kind: Kind, pos: Pt, rot: u8, color: Color32) {
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        for s in symbol(kind) {
            match s {
                Sh::L(a, b, c, d) => {
                    painter.line_segment([tr(a, b), tr(c, d)], stroke);
                }
                Sh::C(x, y, r) => {
                    painter.circle_stroke(tr(x, y), r * self.zoom, stroke);
                }
            }
        }
    }

    fn draw_component(&self, painter: &egui::Painter, rect: Rect, c: &Component, color: Color32) {
        match &c.custom {
            Some(sym) => self.draw_custom(painter, rect, sym, c.pos, c.rot, color),
            None => self.draw_symbol(painter, rect, c.kind, c.pos, c.rot, color),
        }
    }

    fn draw_custom(&self, painter: &egui::Painter, rect: Rect, sym: &CustomSymbol, pos: Pt, rot: u8, color: Color32) {
        let l = sym.layout();
        let tr = |x: f32, y: f32| {
            let r = rot_f((x, y), rot);
            self.to_screen(rect, (pos.0 as f32 + r.0, pos.1 as f32 + r.1))
        };
        let stroke = Stroke::new(1.6, color);
        let (hw, hh) = (l.hw as f32, l.hh as f32);
        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            painter.line_segment([tr(a.0, a.1), tr(b.0, b.1)], stroke);
        }
        let name_font = FontId::proportional((self.zoom * 0.42).clamp(7.0, 14.0));
        let num_font = FontId::proportional((self.zoom * 0.34).clamp(6.0, 11.0));
        for p in &l.pins {
            let (tx, ty, ex, ey) = (p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32);
            painter.line_segment([tr(tx, ty), tr(ex, ey)], stroke);
            // name inside the body, anchored so it stays upright after rotation
            let inward = match p.side {
                PinSide::Left => (1.0, 0.0),
                PinSide::Right => (-1.0, 0.0),
                PinSide::Top => (0.0, 1.0),
                PinSide::Bottom => (0.0, -1.0),
            };
            let r = rot_f(inward, rot);
            let anchor = if r.0 > 0.5 {
                Align2::LEFT_CENTER
            } else if r.0 < -0.5 {
                Align2::RIGHT_CENTER
            } else if r.1 > 0.5 {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_BOTTOM
            };
            painter.text(tr(ex + inward.0 * 0.3, ey + inward.1 * 0.3), anchor, &p.name, name_font.clone(), color);
            // pin number beside the lead
            let (mx, my) = ((tx + ex) / 2.0, (ty + ey) / 2.0);
            let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
            painter.text(tr(mx + off.0, my + off.1), Align2::CENTER_CENTER, &p.number, num_font.clone(), Color32::from_gray(110));
        }
    }

    fn signature_cached(&self) -> &str {
        &self.sig_cache
    }

    fn signature(&self) -> String {
        serde_json::to_string(&self.sch).unwrap_or_default()
    }

    fn run_dc(&mut self) {
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => dc_op(&c).map_err(|e| vec![e.to_string()]),
        };
        let mut pin_v = vec![];
        if let Ok(res) = &outcome {
            for net in extract(&self.sch) {
                let Some(v) = res.voltage(&net.name) else { continue };
                for p in &net.pins {
                    if let Some(c) = self.sch.components.iter().find(|c| c.refdes == p.refdes) {
                        if let Some((_, pt)) = c.pins().into_iter().find(|(n, _)| *n == p.pin) {
                            pin_v.push((pt, v));
                        }
                    }
                }
            }
            // ground symbols and named power symbols show their net voltage too
            for c in self.sch.components.iter().filter(|c| c.kind.is_power_symbol()) {
                let name = if c.kind == Kind::Ground { "GND".to_string() } else { c.value.clone() };
                if let Some(v) = res.voltage(&name) {
                    pin_v.push((c.pins()[0].1, v));
                }
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("DC動作点: 収束 ({} 回, {})", r.iterations, r.strategy),
            Err(e) => format!("DC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.dc = Some(DcView { outcome, signature: self.signature(), pin_v });
    }

    fn reload_library(&mut self) {
        let (lib, errs) = part_lib::load_dir(&self.lib_dir);
        self.library = lib;
        self.lib_errors = errs;
        self.place_lib = None;
    }

    fn import_part_json(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("部品定義 JSON", &["json"]).pick_file() else { return };
        let def: Result<PartDef, String> =
            std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
        self.status = match def {
            Err(e) => format!("読み込み失敗: {e}"),
            Ok(d) => match d.validate().into_iter().find(|i| i.error) {
                Some(i) => format!("{} に問題があります: {}", d.name, i.message),
                None => match part_lib::save(&self.lib_dir, &d) {
                    Ok(_) => {
                        self.reload_library();
                        format!("ライブラリに追加しました: {}", d.name)
                    }
                    Err(e) => format!("保存失敗: {e}"),
                },
            },
        };
    }

    fn comp_at(&self, g: (f32, f32)) -> Option<usize> {
        self.sch.components.iter().enumerate().rev().find_map(|(i, c)| {
            let rel = (g.0 - c.pos.0 as f32, g.1 - c.pos.1 as f32);
            let l = rot_f(rel, (4 - c.rot % 4) % 4);
            let (x0, x1, y0, y1) = comp_extents(c);
            (l.0 >= x0 && l.0 <= x1 && l.1 >= y0 && l.1 <= y1).then_some(i)
        })
    }

    fn pick(&self, g: (f32, f32)) -> Option<Sel> {
        if let Some(i) = self.comp_at(g) {
            return Some(Sel::Comp(i));
        }
        if let Some(i) = self.sch.labels.iter().position(|l| (l.pos.0 as f32 - g.0).abs() < 0.8 && (l.pos.1 as f32 - g.1).abs() < 0.6) {
            return Some(Sel::Label(i));
        }
        let d = |w: &Wire| {
            let (a, b) = ((w.a.0 as f32, w.a.1 as f32), (w.b.0 as f32, w.b.1 as f32));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 { 0.0 } else { (((g.0 - a.0) * dx + (g.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
            ((g.0 - a.0 - t * dx).powi(2) + (g.1 - a.1 - t * dy).powi(2)).sqrt()
        };
        self.sch.wires.iter().position(|w| d(w) < 0.3).map(Sel::Wire)
    }

    fn delete_selected(&mut self) {
        match self.selected.take() {
            Some(Sel::Comp(i)) => drop(self.sch.components.remove(i)),
            Some(Sel::Wire(i)) => drop(self.sch.wires.remove(i)),
            Some(Sel::Label(i)) => drop(self.sch.labels.remove(i)),
            None => {}
        }
    }

    fn add_wire(&mut self, a: Pt, b: Pt) {
        if a == b {
            return;
        }
        if a.0 == b.0 || a.1 == b.1 {
            self.sch.wires.push(Wire { a, b });
        } else {
            let m = (b.0, a.1);
            self.sch.wires.push(Wire { a, b: m });
            self.sch.wires.push(Wire { a: m, b });
        }
    }

    // ------------------------------------------------------------- canvas

    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, SCH_BG);
        if self.dc.is_some() {
            self.sig_cache = self.signature();
        }

        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            self.pan.x -= d.x / self.zoom;
            self.pan.y -= d.y / self.zoom;
        }
        let hover = resp.hover_pos();
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let cur = hover.unwrap_or(rect.center());
                let before = self.to_grid(rect, cur);
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(6.0, 120.0);
                let after = self.to_grid(rect, cur);
                self.pan.x += before.0 - after.0;
                self.pan.y += before.1 - after.1;
            }
        }

        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing {
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                match self.selected {
                    Some(Sel::Comp(i)) => self.sch.components[i].rot = (self.sch.components[i].rot + 1) % 4,
                    _ => self.rot = (self.rot + 1) % 4,
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                self.delete_selected();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending = None;
                self.selected = None;
                self.tool = Tool::Select;
            }
        }

        // grid dots
        let tl = self.to_grid(rect, rect.left_top());
        let br = self.to_grid(rect, rect.right_bottom());
        if self.zoom >= 10.0 {
            for x in (tl.0.floor() as i32)..=(br.0.ceil() as i32) {
                for y in (tl.1.floor() as i32)..=(br.1.ceil() as i32) {
                    painter.circle_filled(self.to_screen(rect, (x as f32, y as f32)), 0.9, Color32::from_gray(190));
                }
            }
        }

        // wires
        for (i, w) in self.sch.wires.iter().enumerate() {
            let sel = self.selected == Some(Sel::Wire(i));
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 3.0 } else { 1.8 }, if sel { Color32::from_rgb(0, 80, 220) } else { WIRE }),
            );
        }
        // junction dots
        let pins: Vec<Pt> = self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|p| p.1)).collect();
        for w in &self.sch.wires {
            for p in [w.a, w.b] {
                let ends = self.sch.wires.iter().map(|x| (x.a == p) as u32 + (x.b == p) as u32).sum::<u32>()
                    + pins.iter().filter(|&&q| q == p).count() as u32;
                let on_other = self.sch.wires.iter().any(|x| x.a != p && x.b != p && on_segment(p, x.a, x.b));
                if ends >= 3 || on_other {
                    painter.circle_filled(self.to_screen(rect, (p.0 as f32, p.1 as f32)), 3.5, WIRE);
                }
            }
        }

        // components
        for (i, c) in self.sch.components.iter().enumerate() {
            let sel = self.selected == Some(Sel::Comp(i));
            let col = if sel { Color32::from_rgb(0, 80, 220) } else { SYM };
            self.draw_component(&painter, rect, c, col);
            for (_, p) in c.pins() {
                painter.rect_filled(
                    Rect::from_center_size(self.to_screen(rect, (p.0 as f32, p.1 as f32)), Vec2::splat(4.0)),
                    0.0,
                    Color32::from_rgb(190, 40, 40),
                );
            }
            let f = FontId::proportional((self.zoom * 0.55).clamp(8.0, 18.0));
            let (x0, x1, y0, y1) = comp_extents(c);
            let ctr = (c.pos.0 as f32 + (x0 + x1) / 2.0, c.pos.1 as f32);
            if c.kind.is_power_symbol() {
                if c.kind == Kind::Power {
                    painter.text(self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32 - 0.8)), Align2::CENTER_BOTTOM, &c.value, f.clone(), SYM);
                }
            } else {
                let top = c.pos.1 as f32 + if c.rot % 2 == 0 { y0.min(-1.0) - 0.3 } else { -x1.max(2.0) - 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, top)), Align2::CENTER_BOTTOM, &c.refdes, f.clone(), Color32::from_rgb(20, 60, 140));
                let bot = c.pos.1 as f32 + if c.rot % 2 == 0 { y1.max(1.0) + 0.3 } else { x1.max(2.0) + 0.3 };
                painter.text(self.to_screen(rect, (ctr.0, bot)), Align2::CENTER_TOP, &c.value, f, Color32::from_gray(60));
            }
        }
        for (i, l) in self.sch.labels.iter().enumerate() {
            let sel = self.selected == Some(Sel::Label(i));
            let p = self.to_screen(rect, (l.pos.0 as f32, l.pos.1 as f32));
            painter.circle_filled(p, 3.0, WIRE);
            painter.text(
                p + Vec2::new(5.0, -3.0),
                Align2::LEFT_BOTTOM,
                &l.text,
                FontId::proportional(13.0),
                if sel { Color32::from_rgb(0, 80, 220) } else { Color32::from_rgb(0, 100, 100) },
            );
        }
        if self.show_v {
            if let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached()) {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let mut seen: Vec<Pt> = vec![];
                for (pt, v) in &dc.pin_v {
                    if seen.contains(pt) {
                        continue;
                    }
                    seen.push(*pt);
                    let p = self.to_screen(rect, (pt.0 as f32, pt.1 as f32)) + Vec2::new(5.0, -4.0);
                    let galley = painter.layout_no_wrap(eng(*v, "V"), f.clone(), Color32::from_rgb(10, 60, 200));
                    painter.rect_filled(Rect::from_min_size(p - Vec2::new(0.0, galley.size().y), galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(p - Vec2::new(0.0, galley.size().y), galley, Color32::from_rgb(10, 60, 200));
                }
            }
        }
        for pr in &self.problems {
            if let Some(a) = pr.at {
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), 11.0, Stroke::new(2.0, Color32::from_rgb(230, 0, 200)));
            }
        }

        // interaction
        let Some(pos) = hover else { return };
        let g = self.to_grid(rect, pos);
        let snapped: Pt = (g.0.round() as i32, g.1.round() as i32);
        let sp = self.to_screen(rect, (snapped.0 as f32, snapped.1 as f32));

        match self.tool {
            Tool::Place => {
                let ghost = Color32::from_rgba_unmultiplied(120, 20, 20, 120);
                match self.place_lib.and_then(|k| self.library.get(k)) {
                    Some(d) => self.draw_custom(&painter, rect, &d.symbol(), snapped, self.rot, ghost),
                    None => self.draw_symbol(&painter, rect, self.place, snapped, self.rot, ghost),
                }
            }
            Tool::Wire => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, WIRE));
                if let Some(a) = self.pending {
                    let a_s = self.to_screen(rect, (a.0 as f32, a.1 as f32));
                    let m = self.to_screen(rect, (snapped.0 as f32, a.1 as f32));
                    let st = Stroke::new(1.5, Color32::from_rgba_unmultiplied(0, 110, 30, 150));
                    painter.line_segment([a_s, m], st);
                    painter.line_segment([m, sp], st);
                }
            }
            Tool::Label => {
                painter.circle_stroke(sp, 4.0, Stroke::new(1.5, Color32::from_rgb(0, 100, 100)));
            }
            Tool::Select => {}
        }
        self.status = format!("({}, {})  部品 {}  配線 {}", snapped.0, snapped.1, self.sch.components.len(), self.sch.wires.len());

        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select {
            if let Some(i) = self.comp_at(g) {
                let c = &self.sch.components[i];
                self.drag = Some((i, (snapped.0 - c.pos.0, snapped.1 - c.pos.1)));
                self.selected = Some(Sel::Comp(i));
            }
        }
        if let Some((i, off)) = self.drag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                self.sch.components[i].pos = (snapped.0 - off.0, snapped.1 - off.1);
            } else {
                self.drag = None;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }

        if resp.clicked_by(egui::PointerButton::Primary) {
            match self.tool {
                Tool::Select => self.selected = self.pick(g),
                Tool::Place => {
                    match self.place_lib.and_then(|k| self.library.get(k)).cloned() {
                        Some(d) => {
                            self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), snapped, self.rot);
                        }
                        None => {
                            self.sch.add(self.place, snapped, self.rot);
                        }
                    }
                }
                Tool::Wire => match self.pending.take() {
                    None => self.pending = Some(snapped),
                    Some(a) => {
                        self.add_wire(a, snapped);
                        self.pending = Some(snapped);
                    }
                },
                Tool::Label => {
                    if !self.label_text.is_empty() {
                        self.sch.labels.push(Label { pos: snapped, text: self.label_text.clone() });
                    }
                }
            }
        }
        if resp.double_clicked() && self.tool == Tool::Wire {
            self.pending = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.pending = None;
        }
    }

    // ------------------------------------------------------------- side panel

    pub fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("回路図");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "選択");
            ui.selectable_value(&mut self.tool, Tool::Wire, "配線");
            ui.selectable_value(&mut self.tool, Tool::Label, "ラベル");
        });
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text);
        }
        ui.separator();
        ui.label("部品 (クリックして配置 / Rキーで回転)");
        for k in Kind::PALETTE {
            if ui.selectable_label(self.tool == Tool::Place && self.place_lib.is_none() && self.place == *k, k.label()).clicked() {
                self.tool = Tool::Place;
                self.place_lib = None;
                self.place = *k;
                self.pending = None;
            }
        }
        ui.separator();
        let mut chosen = None;
        ui.collapsing(format!("ライブラリ部品 ({})", self.library.len()), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索"));
            let f = self.lib_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for (i, d) in self.library.iter().enumerate() {
                    if !f.is_empty() && !d.name.to_lowercase().contains(&f) && !d.description.to_lowercase().contains(&f) {
                        continue;
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let selected = self.tool == Tool::Place && self.place_lib == Some(i);
                    if ui.selectable_label(selected, format!("{}{mark}", d.name)).on_hover_text(&d.description).clicked() {
                        chosen = Some(i);
                    }
                }
            });
            ui.small("◎ = シミュレーションモデルあり");
            ui.horizontal(|ui| {
                if ui.button("新規作成…").clicked() {
                    self.form = Some(PartForm::default());
                }
                if ui.button("JSONから追加…").clicked() {
                    self.import_part_json();
                }
                if ui.button("再読み込み").clicked() {
                    self.reload_library();
                }
            });
            ui.small(format!("保存先: {}", self.lib_dir.display()));
            for e in &self.lib_errors {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
            }
        });
        if let Some(i) = chosen {
            self.tool = Tool::Place;
            self.place_lib = Some(i);
            self.pending = None;
        }
        ui.separator();
        if let Some(Sel::Comp(i)) = self.selected {
            if i < self.sch.components.len() {
                let c = &mut self.sch.components[i];
                ui.label("選択中の部品");
                if !c.kind.is_power_symbol() {
                    ui.horizontal(|ui| {
                        ui.label("番号");
                        ui.text_edit_singleline(&mut c.refdes);
                    });
                }
                if c.kind != Kind::Ground {
                    ui.horizontal(|ui| {
                        ui.label("値");
                        ui.text_edit_singleline(&mut c.value);
                    });
                }
                if let Some(sym) = &c.custom {
                    ui.label(format!("フットプリント: {}", c.footprint));
                    match &sym.sim {
                        Some(m) => ui.label(format!("モデル: {}", m.name)),
                        None => ui.colored_label(Color32::from_rgb(220, 160, 40), "シミュレーションモデルなし"),
                    };
                } else if !c.kind.is_power_symbol() {
                    let current = c.footprint_name();
                    egui::ComboBox::from_label("フットプリント").selected_text(current).show_ui(ui, |ui| {
                        for n in pcb_core::footprints::names() {
                            ui.selectable_value(&mut c.footprint, n.clone(), n);
                        }
                    });
                }
                if ui.button("回転 (R)").clicked() {
                    c.rot = (c.rot + 1) % 4;
                }
            }
        }
        if self.selected.is_some() && ui.button("削除 (Delete)").clicked() {
            self.delete_selected();
        }
        ui.separator();
        if ui.button("ERC (電気ルールチェック)").clicked() {
            self.problems = erc(&self.sch);
            self.status = format!("ERC: 問題 {} 件", self.problems.len());
        }
        for p in &self.problems {
            ui.colored_label(Color32::from_rgb(230, 0, 200), &p.message);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("DC動作点を解析").clicked() {
                self.run_dc();
            }
            ui.checkbox(&mut self.show_v, "電圧表示");
        });
        let stale = self.dc.as_ref().is_some_and(|d| d.signature != self.signature());
        if let Some(dc) = &self.dc {
            if stale {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "回路が変更されました。再度解析してください");
            }
            match &dc.outcome {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                    }
                }
                Ok(res) => {
                    for w in &res.warnings {
                        ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                    }
                    egui::CollapsingHeader::new("解析結果").default_open(true).show(ui, |ui| {
                        egui::Grid::new("dc_nodes").striped(true).show(ui, |ui| {
                            for (n, v) in res.voltages.iter().filter(|(n, _)| !n.contains('.')) {
                                ui.label(n);
                                ui.label(eng(*v, "V"));
                                ui.end_row();
                            }
                        });
                        ui.separator();
                        egui::Grid::new("dc_elems").striped(true).show(ui, |ui| {
                            ui.strong("素子");
                            ui.strong("電流");
                            ui.strong("電圧");
                            ui.strong("電力");
                            ui.end_row();
                            for e in res.elements.iter().filter(|e| !e.name.contains('.')) {
                                ui.label(&e.name);
                                ui.label(eng(e.current, "A"));
                                ui.label(eng(e.voltage, "V"));
                                ui.label(eng(e.power, "W"));
                                ui.end_row();
                            }
                        });
                        ui.small("電流は素子の第1端子→第2端子の向き。電源は電力を供給すると負");
                    });
                }
            }
        }
        ui.collapsing("ネットリスト", |ui| {
            for n in extract(&self.sch) {
                let pins: Vec<String> = n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                ui.label(format!("{}: {}", n.name, pins.join(", ")));
            }
        });
        if ui.button("SPICEネットリストを書き出し…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("SPICE", &["cir"]).set_file_name("circuit.cir").save_file() {
                self.status = match std::fs::write(&p, to_spice(&self.sch, "circuitCad")) {
                    Ok(()) => format!("書き出しました: {}", p.display()),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
        }
        ui.separator();
        if let Some(mut form) = self.form.take() {
            let mut open = true;
            let mut saved = None;
            egui::Window::new("部品を作成").open(&mut open).default_width(560.0).vscroll(true).show(ui.ctx(), |ui| {
                saved = form.ui(ui);
            });
            match saved {
                Some(def) => match part_lib::save(&self.lib_dir, &def) {
                    Ok(p) => {
                        self.reload_library();
                        self.status = format!("ライブラリに保存しました: {}", p.display());
                    }
                    Err(e) => {
                        self.status = format!("保存失敗: {e}");
                        self.form = Some(form);
                    }
                },
                None if open => self.form = Some(form),
                None => {}
            }
        }
        ui.small("左クリック: 配置 / 配線 (斜めは自動でL字)\n右クリック・ダブルクリック: 配線を終了\n中・右ドラッグ: 移動 / ホイール: 拡大縮小");
    }

    // ------------------------------------------------------------- files

    pub fn new_file(&mut self) {
        *self = SchEditor::default();
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).set_file_name("circuit.schjson").save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            self.status = match serde_json::to_string_pretty(&self.sch).map(|s| std::fs::write(&p, s)) {
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
        if let Some(p) = rfd::FileDialog::new().add_filter("circuitCad 回路図", &["schjson"]).pick_file() {
            self.load_path(p);
        }
    }

    pub fn load_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(s) => {
                self.sch = s;
                self.path = Some(p.clone());
                self.selected = None;
                self.pending = None;
                self.problems.clear();
                self.dc = None;
                self.status = format!("開きました: {}", p.display());
            }
            Err(e) => self.status = format!("読み込み失敗: {e}"),
        }
    }

    pub fn analyze_dc(&mut self) {
        self.run_dc();
    }

    #[cfg(test)]
    pub fn pin_voltage(&self, refdes: &str, pin: &str) -> Option<f64> {
        let c = self.sch.components.iter().find(|c| c.refdes == refdes)?;
        let (_, pt) = c.pins().into_iter().find(|(n, _)| n == pin)?;
        self.dc.as_ref()?.pin_v.iter().find(|(p, _)| *p == pt).map(|(_, v)| *v)
    }
}
