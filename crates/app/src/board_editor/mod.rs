//! PCB layout editor: place and move parts, route, pour copper, check, export.

use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pcb_core::autoroute::{self, RouteOptions};
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
        Tool::Select => "クリックで選択、部品はドラッグで移動。空いた所を左ドラッグ = 完全に囲んだものを選択、右ドラッグ = 触れたものも選択 (Shiftで追加)",
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

#[derive(Clone, Copy, PartialEq, Debug)]
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

/// Rubber-band selection in progress.
struct BoxSel {
    start: Point,
    /// Right button: anything the box touches is selected. Left button: only what it fully contains.
    crossing: bool,
}

/// Several selected objects being moved together; `orig` is the board when the drag began.
struct GroupDrag {
    anchor: Point,
    orig: Board,
    items: Vec<Item>,
}

fn rect_contains(lo: Point, hi: Point, p: Point) -> bool {
    p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
}

fn rects_overlap(alo: Point, ahi: Point, blo: Point, bhi: Point) -> bool {
    alo.x <= bhi.x && ahi.x >= blo.x && alo.y <= bhi.y && ahi.y >= blo.y
}

/// Does the segment a-b touch the rectangle? (Liang-Barsky clipping.)
fn segment_hits_rect(a: Point, b: Point, lo: Point, hi: Point) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (p, q) in [(-dx, a.x - lo.x), (dx, hi.x - a.x), (-dy, a.y - lo.y), (dy, hi.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy)]
struct Visible {
    top: bool,
    bottom: bool,
    silk: bool,
    airwires: bool,
    nets: bool,
    zones: bool,
    ruler: bool,
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
    /// Two or more objects selected by a rubber band (then `selected` is empty).
    multi: Vec<Item>,
    boxsel: Option<BoxSel>,
    gdrag: Option<GroupDrag>,
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
    /// A left drag with Alt held pans the view (until the button is released).
    alt_pan: bool,
    /// The "発注の準備" window.
    order: order::OrderState,
    order_preview_src: Option<pcb_core::gerber_view::Preview>,
    last_export_dir: Option<PathBuf>,
    /// The running guided lesson, and whether the DRC has been run since the last edit.
    lesson: Option<lesson::BoardLesson>,
    drc_ran: bool,
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
            multi: vec![],
            boxsel: None,
            gdrag: None,
            drag_part: None,
            status: "基板パターン: 「回路図から取り込み」で部品を配置します".into(),
            violations: vec![],
            import_notes: vec![],
            undo: vec![],
            redo: vec![],
            vis: Visible { top: true, bottom: true, silk: true, airwires: true, nets: true, zones: true, ruler: true },
            zone_net: "GND".into(),
            zone_clearance: 0.3,
            zone_thermal: true,
            size_w: 50.0,
            size_h: 40.0,
            fit_pending: true,
            alt_pan: false,
            order: order::OrderState::default(),
            order_preview_src: None,
            last_export_dir: None,
            lesson: None,
            drc_ran: false,
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

// The editor is one type spread over several files; the child modules see its private fields.
mod history;
mod view;
mod select;
mod route;
mod tools;
mod file;
mod actions;
mod canvas;
mod interact;
mod draw;
mod order;
mod ruler;
pub mod lesson;
mod panel;
#[cfg(test)]
mod tests;
