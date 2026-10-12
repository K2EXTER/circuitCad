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

/// Results of `connectivity` and `ratsnest` for the board as it was when `key` was taken.
#[derive(Default)]
struct CalcCache {
    key: u64,
    conn: Option<std::rc::Rc<drc::Connectivity>>,
    rats: Option<std::rc::Rc<Vec<pcb_core::ratsnest::Airwire>>>,
}

/// What a click or a rubber band may pick up. Switching parts off makes them safe from accidental moves
/// while selecting and moving vias and tracks.
#[derive(Clone, Copy)]
struct Pickable {
    parts: bool,
    tracks: bool,
    vias: bool,
    /// Loose pads and mounting holes.
    pads: bool,
    /// Copper pours, silk lines.
    other: bool,
    /// Only what is on the layer being worked on (and what goes through the board: vias, through-hole parts).
    active_side_only: bool,
}

impl Pickable {
    const ALL: Pickable = Pickable { parts: true, tracks: true, vias: true, pads: true, other: true, active_side_only: true };

    fn allows(&self, it: &Item) -> bool {
        match it {
            Item::Part(_) => self.parts,
            Item::Track(_) => self.tracks,
            Item::Via(_) => self.vias,
            Item::Pad(_) | Item::Hole(_) => self.pads,
            Item::Silk(_) | Item::Zone(_) => self.other,
        }
    }
}

#[derive(Clone, Copy)]
struct Visible {
    top: bool,
    bottom: bool,
    inner: bool,
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
    /// Voltage drop along the nets' copper, with a fingerprint of the board it was computed for.
    /// Open the width panel on the next frame (command line `--ir`).
    widths_open: Option<bool>,
    /// The two nets picked in the diff-pair panel for a new pair.
    pair_new: (String, String),
    ir_drop: Option<(f64, Vec<pcb_core::irdrop::NetDrop>)>,
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
    /// Dragging one track with the Select tool: (the board as it was, the track, where the pointer took hold).
    seg_drag: Option<(Board, usize, Point)>,
    /// Push other nets' tracks aside while routing and dragging tracks.
    shove: bool,
    /// The routing preview's verdict for the last cursor position: (key, tracks pushed, rule broken).
    preview_cache: std::cell::RefCell<Option<(u64, usize, bool)>>,
    pub status: String,
    /// Set by the small "操作" buttons; the app opens the guide on that page.
    pub help_request: Option<crate::help::HelpTab>,
    violations: Vec<drc::Violation>,
    import_notes: Vec<String>,
    undo: Vec<Board>,
    redo: Vec<Board>,
    vis: Visible,
    /// What the pointer may select / pick up (the "選択できるもの" checkboxes).
    pick: Pickable,
    /// The pre-order check (index into `order.checks`) whose place is being shown on the board.
    check_focus: Option<usize>,
    zone_net: String,
    zone_clearance: f64,
    zone_thermal: bool,
    size_w: f64,
    size_h: f64,
    /// Corner radius (mm) for rectangular outlines; 0 = square corners.
    size_r: f64,
    /// The "3Dで見る" window.
    view3d: view3d::View3d,
    fit_pending: bool,
    /// A left drag with Alt held pans the view (until the button is released).
    alt_pan: bool,
    /// The "発注の準備" window.
    order: order::OrderState,
    order_preview_src: Option<pcb_core::gerber_view::Preview>,
    /// The pours as geometry (cut to the outline, cleared around other nets), kept until the board changes.
    /// What is worked out from the copper every frame (connections, unrouted lines), kept until the board changes.
    calc_cache: std::cell::RefCell<CalcCache>,
    zone_cache: std::cell::RefCell<Option<(u64, Vec<Vec<pcb_core::zonefill::FillShape>>)>>,
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
            ir_drop: None,
            widths_open: None,
            pair_new: Default::default(),
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
            seg_drag: None,
            shove: true,
            preview_cache: Default::default(),
            status: "基板パターン: 「回路図から取り込み」で部品を配置します".into(),
            help_request: None,
            violations: vec![],
            import_notes: vec![],
            undo: vec![],
            redo: vec![],
            pick: Pickable::ALL,
            check_focus: None,
            vis: Visible { top: true, bottom: true, silk: true, airwires: true, nets: true, zones: true, ruler: true, inner: true },
            zone_net: "GND".into(),
            zone_clearance: 0.3,
            zone_thermal: true,
            size_w: 50.0,
            size_h: 40.0,
            size_r: 0.0,
            view3d: view3d::View3d::default(),
            fit_pending: true,
            alt_pan: false,
            order: order::OrderState::default(),
            order_preview_src: None,
            calc_cache: std::cell::RefCell::new(CalcCache::default()),
            zone_cache: std::cell::RefCell::new(None),
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
mod width;
mod raster3d;
mod view3d;
mod select;
mod route;
mod layers;
mod shoving;
mod pairs;
mod tools;
mod file;
mod actions;
mod canvas;
mod interact;
mod draw;
pub(crate) use draw::capsule;
mod order;
mod ruler;
pub mod lesson;
mod panel;
#[cfg(test)]
mod tests;
