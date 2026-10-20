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

use crate::icons::Icon;

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

/// The number key that picks a tool: 1-9 and 0 in the order of `TOOLS`.
fn tool_key(t: Tool) -> Option<char> {
    TOOLS.iter().position(|(x, _, _)| *x == t).and_then(|i| char::from_digit(((i + 1) % 10) as u32, 10))
}

const TOOLS: &[(Tool, &str, crate::icons::Icon)] = &[
    (Tool::Select, "選択 / 部品の移動", Icon::Select),
    (Tool::Track, "配線", Icon::Wire),
    (Tool::Via, "ビア", Icon::Via),
    (Tool::Zone, "銅箔ゾーン (ベタ)", Icon::Zone),
    (Tool::OutlineRect, "基板外形 (矩形)", Icon::OutlineRect),
    (Tool::OutlinePoly, "基板外形 (多角形)", Icon::OutlinePoly),
    (Tool::PadTh, "パッド(スルーホール)", Icon::PadTh),
    (Tool::PadSmd, "パッド(SMD)", Icon::PadSmd),
    (Tool::Hole, "取付穴", Icon::Hole),
    (Tool::Silk, "シルク線", Icon::Silk),
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
    /// Copper pours.
    zones: bool,
    /// Silk lines that belong to no part.
    silk: bool,
    /// Only what is on the layer being worked on (and what goes through the board: vias, through-hole parts).
    active_side_only: bool,
}

impl Pickable {
    const ALL: Pickable = Pickable { parts: true, tracks: true, vias: true, pads: true, zones: true, silk: true, active_side_only: true };

    fn allows(&self, it: &Item) -> bool {
        match it {
            Item::Part(_) => self.parts,
            Item::Track(_) => self.tracks,
            Item::Via(_) => self.vias,
            Item::Pad(_) | Item::Hole(_) => self.pads,
            Item::Silk(_) => self.silk,
            Item::Zone(_) => self.zones,
        }
    }
}

#[derive(Clone, Copy)]
struct Visible {
    top: bool,
    bottom: bool,
    /// Inner layers switched off in the layer overlay.
    inner_hidden: u32,
    silk: bool,
    airwires: bool,
    nets: bool,
    zones: bool,
    ruler: bool,
    tracks: bool,
    vias: bool,
}

/// Where the declared net currents come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentSource {
    /// The DC operating point.
    Dc,
    /// The RMS of the settled part of the last transient run.
    Tran,
}

pub struct BoardEditor {
    pub board: Board,
    pub path: Option<PathBuf>,
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
    /// Length matching (the serpentine panel): the nets chosen, the options and the target length (mm).
    meander: pcb_core::meander::MeanderOptions,
    meander_nets: Vec<String>,
    meander_target: f64,
    /// The Track tool searches a route to the pad under the cursor (round what is in the way), and uses the shove
    /// where there is none. Off: the plain bent line, no shove.
    auto_route: bool,
    /// The obstacle grid of the board, kept until the copper or the settings change.
    router_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<pcb_core::autoroute::Router>)>>,
    /// The copper as the pad leads see it, and the snap points at the ends of the pads (see `padlead.rs`).
    stretch_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<pcb_core::drc::StretchCheck>)>>,
    ends_cache: std::cell::RefCell<Option<(u64, Vec<Point>)>>,
    /// Leave and enter pads at a right angle to their edge, for `pad_lead_len` mm.
    pad_lead: bool,
    pad_lead_len: f64,
    /// Move two tracks that leave a wide pad from one point to its two ends ("パッドの根元をそろえる").
    pad_spread: bool,
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
    /// The copper with the board's finish applied, kept until the copper or the settings change.
    finish_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<pcb_core::smooth::Finished>)>>,
    /// The routing preview's verdict for the last cursor position: (key, tracks pushed, rule broken).
    preview_cache: std::cell::RefCell<Option<shoving::PreviewCache>>,
    pub status: String,
    /// Set by the small "操作" buttons; the app opens the guide on that page.
    pub help_request: Option<crate::help::HelpTab>,
    /// Set by the voltage-drop panel: `Some(true)` = simulate with this board's copper resistance, `Some(false)` = back to ideal wires. `App` carries it to the schematic.
    pub post_layout_request: Option<bool>,
    /// Set by the width panel: ask `App` for the schematic's DC net currents (it answers with `use_simulated_currents`).
    pub currents_request: Option<CurrentSource>,
    /// Set when "全部" found no declared currents and asked for the schematic's DC currents first: the route (all nets, or
    /// the named one) is made when `App` has answered. `currents_tried` stops it from asking twice.
    pub route_after_currents: Option<Option<String>>,
    currents_tried: bool,
    /// What the last transient run said about each net's current (RMS, burst, peak), shown under the width table.
    pub current_stats: Vec<sim_core::NetStat>,
    /// The copper resistance of the current copper is in the schematic's analyses (set by `App`, cleared when the copper is widened).
    pub post_layout_fresh: bool,
    violations: Vec<drc::Violation>,
    import_notes: Vec<String>,
    /// Counts changes (one per `checkpoint`) and replaced boards; `saved_rev` is its value when the board was last saved or opened.
    pub rev: u64,
    saved_rev: u64,
    /// How the schematic differs from this board, with the (schematic rev, board rev) it was worked out for.
    sync_cache: Option<((u64, u64), file::SchDiff)>,
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
    zone_remove_islands: bool,
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
    zone_cache: std::cell::RefCell<Option<(u64, Vec<(Side, Vec<pcb_core::zonefill::FillShape>)>)>>,
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
            meander: Default::default(),
            meander_nets: vec![],
            meander_target: 0.0,
            auto_route: false,
            router_cache: Default::default(),
            stretch_cache: Default::default(),
            ends_cache: Default::default(),
            pad_lead: true,
            pad_lead_len: 0.5,
            pad_spread: true,
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
            finish_cache: Default::default(),
            preview_cache: Default::default(),
            status: "基板パターン: 「回路図から取り込み」で部品を配置します".into(),
            help_request: None,
            post_layout_request: None,
            currents_request: None,
            route_after_currents: None,
            currents_tried: false,
            current_stats: vec![],
            post_layout_fresh: false,
            violations: vec![],
            import_notes: vec![],
            rev: 0,
            saved_rev: 0,
            sync_cache: None,
            undo: vec![],
            redo: vec![],
            pick: Pickable::ALL,
            check_focus: None,
            vis: Visible { top: true, bottom: true, silk: true, airwires: true, nets: true, zones: true, ruler: true, tracks: true, vias: true, inner_hidden: 0 },
            zone_net: "GND".into(),
            zone_clearance: 0.3,
            zone_thermal: true,
            zone_remove_islands: true,
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
mod smoothing;
mod padlead;
mod livepath;
mod lengths;
mod pairs;
mod align;
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
