//! PCB layout editor: place and move parts, route, pour copper, check, export.

use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pcb_core::autoroute::{self, RouteOptions};
use pcb_core::drc::{self, connectivity};
use pcb_core::geom::{inside_polygon, triangulate};
use pcb_core::netlist::{self, NetlistNet, NetlistPart};
use pcb_core::ratsnest::ratsnest;
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
        Tool::Select => "クリックで選択、部品はドラッグで移動。空いた所を左ドラッグ = 完全に囲んだものを選択、右ドラッグ = 触れたものも選択 (Shiftで追加、Ctrl で足す・外す。Ctrl+クリックも)",
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
    TOOLS.iter().position(|(x, _, _)| *x == t).filter(|i| *i < 10).and_then(|i| char::from_digit(((i + 1) % 10) as u32, 10))
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

/// What a click adds while a polygon is drawn (the board outline, a pour, a hole of a pour): like a drafting program, lines and arcs
/// close a shape, or a circle or a rectangle is drawn at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum PolyMode {
    /// A straight line to each clicked point.
    #[default]
    Lines,
    /// An arc: the click for a point it passes through, then the click for its end.
    ArcThrough,
    /// An arc that leaves the last line (or arc) in its direction: one click for the end.
    ArcTangent,
    /// The centre, then a point on the circle.
    Circle,
    /// Two opposite corners (rounded by `shape_round`).
    Rect,
}

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
    /// A part's silkscreen designator: index into `Board::texts` (only texts with a reference designator).
    Label(usize),
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

/// The answer of the requirements check and the measures of the table (`pcb_core::requirements::analyse`).
type ReqAnswer = (pcb_core::requirements::RequirementsReport, Vec<pcb_core::requirements::MeasureMark>);

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
            Item::Silk(_) | Item::Label(_) => self.silk,
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
    /// The parts: their pads, outlines and designators (with it off the copper under them can be seen).
    parts: bool,
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

/// The merged pours of the board, with the key they were made for (`zonefill::geometry_key`).
/// Per pour: its layer, its net, its pieces and the triangles of each piece (worked out once, not every frame).
type PourCache = (Side, String, Vec<pcb_core::zonefill::FillShape>, Vec<Vec<[Point; 3]>>);
type ZoneCache = Option<(u64, Vec<PourCache>)>;
/// The pours and islands being worked out on another thread, for the geometry key given.
type ZoneJob = Option<(u64, std::sync::mpsc::Receiver<(Vec<PourCache>, Vec<(Side, String, Vec<pcb_core::zonefill::FillShape>)>)>)>;

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
    /// After the auto-router, fill the top with GND too (only when the bottom has the GND pour).
    pour_top: bool,
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
    zoom: f32, // pixels per mm
    pan: Vec2, // world mm at the screen centre
    pending: Option<Point>,
    /// Polygon being drawn (board outline or zone).
    poly: Vec<Point>,
    /// The corners of the shape being drawn (a corner and the point an arc from it passes through); `poly` is the same shape as short
    /// straight pieces, for the picture and the checks.
    poly_verts: Vec<pcb_core::shape::Vertex>,
    /// The corner or arc middle point of a shape of the selected pour that is being dragged, the one that is picked (Delete takes it
    /// away), and where the right click landed (for its menu).
    vdrag: Option<VertexRef>,
    vsel: Option<VertexRef>,
    vmenu: Option<EditTarget>,
    poly_mode: PolyMode,
    /// The point an arc passes through (`PolyMode::ArcThrough`), clicked and waiting for the end.
    arc_mid: Option<Point>,
    /// The radius of the rounded corners of a rectangle (mm; 0: square corners).
    shape_round: f64,
    selected: Option<Item>,
    /// Two or more objects selected by a rubber band (then `selected` is empty).
    multi: Vec<Item>,
    boxsel: Option<BoxSel>,
    gdrag: Option<GroupDrag>,
    drag_part: Option<(String, Point)>,
    /// A designator text being dragged: its part and where the pointer grabbed it (from the text's centre).
    drag_label: Option<(String, Point)>,
    /// The wire tool in its trunk mode ("幹で引く"): two pads of a net side by side, then a place to go to.
    trunk_mode: bool,
    /// In trunk mode: the pads picked so far (up to two).
    trunk_pads: Vec<usize>,
    /// What the trunk tool last said (shown in its box; the status line is overwritten with the cursor position).
    trunk_note: Option<Result<String, String>>,
    /// Dragging one track with the Select tool: (the board as it was, the track, where the pointer took hold).
    seg_drag: Option<(Board, usize, Point)>,
    /// Push other nets' tracks aside while routing and dragging tracks.
    shove: bool,
    /// The copper with the board's finish applied, kept until the copper or the settings change.
    finish_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<pcb_core::smooth::Finished>)>>,
    /// The finish being worked out on another thread: for which key, and where the answer comes from (`finished_async`).
    finish_job: std::cell::RefCell<Option<(u64, std::sync::mpsc::Receiver<Option<pcb_core::smooth::Finished>>)>>,
    /// The tracks and vias (`tracks_key`) the kept finish was made for, and those the running job was started for: a finish of tracks
    /// that have moved since is not shown (it would stay where they were).
    finish_tracks: std::cell::Cell<u64>,
    finish_job_tracks: std::cell::Cell<u64>,
    /// The tracks followed into chains and the finish's polygons cut into triangles, kept until the copper changes (`draw_copper`).
    copper_cache: std::cell::RefCell<Option<ribbon::CopperCache>>,
    /// The silkscreen followed into chains (`draw_copper`).
    silk_cache: std::cell::RefCell<Option<ribbon::SilkCache>>,
    /// The routing preview's verdict for the last cursor position: (key, tracks pushed, rule broken).
    preview_cache: std::cell::RefCell<Option<shoving::PreviewCache>>,
    pub status: String,
    /// Set by the small "操作" buttons; the app opens the guide on that page.
    pub help_request: Option<crate::help::HelpTab>,
    /// Set by the voltage-drop panel: `Some(true)` = simulate with this board's copper resistance, `Some(false)` = back to ideal wires. `App` carries it to the schematic.
    pub post_layout_request: Option<bool>,
    /// Set by the width panel: ask `App` for the schematic's DC net currents (it answers with `use_simulated_currents`).
    pub currents_request: Option<CurrentSource>,
    /// "入れ替えを回路図に反映" was pressed: `main.rs` applies the pin trades to the schematic and hands the answer to `trades_applied`.
    pub to_schematic_request: bool,
    /// The reason typed for "承知する" (accept a finding of the requirements check).
    pub waive_reason: String,
    /// A waiver added (true) or taken back (false) in the list of the check: `main.rs` writes it into the schematic's table too.
    pub waiver_request: Option<(bool, design_spec::Waiver)>,
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
    /// What the shape drawn with the zone tool does to the pour: adds to it, or takes away (a hole).
    zone_op: pcb_core::model::ShapeOp,
    /// The net whose pours a shape that takes away cuts (empty: the pours of every net on the layer).
    hole_net: String,
    size_w: f64,
    size_h: f64,
    /// Corner radius (mm) for rectangular outlines; 0 = square corners.
    size_r: f64,
    /// The "3Dで見る" window.
    view3d: view3d::View3d,
    fit_pending: bool,
    /// A left drag with Alt held pans the view (until the button is released).
    alt_pan: bool,
    /// Alignment guides through snap points the pointer rested on (see `tracking.rs`).
    tracking: tracking::Tracking,
    /// The "発注の準備" window.
    order: order::OrderState,
    order_preview_src: Option<pcb_core::gerber_view::Preview>,
    /// The pours as geometry (cut to the outline, cleared around other nets), kept until the board changes.
    /// What is worked out from the copper every frame (connections, unrouted lines), kept until the board changes.
    calc_cache: std::cell::RefCell<CalcCache>,
    /// The requirements check and the measures of the table, with the key of the board they were worked out for.
    req_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<ReqAnswer>)>>,
    zone_cache: std::cell::RefCell<ZoneCache>,
    zone_job: std::cell::RefCell<ZoneJob>,
    /// The pieces of the pours that are left out as islands, kept for the state of the board they were worked out for.
    island_cache: std::cell::RefCell<Option<(u64, Vec<(Side, String, Vec<pcb_core::zonefill::FillShape>)>)>>,
    /// The outlines of the pours (the outer line and the holes of every shape) thinned to a level of detail (`ribbon::lod_level`) of the
    /// zoom, for the key of `zone_cache` they were made from.
    zone_outline_lod: std::cell::RefCell<Option<(u64, usize, std::rc::Rc<Vec<Vec<Vec<Vec<Point>>>>>)>>,
    /// How many heat pads lack thermal vias, with the geometry key it was counted for: the count fills every pour, so it is not worked out every frame.
    thermal_cache: std::cell::Cell<Option<(u64, usize)>>,
    /// The net under the pointer with the pointer's place; it glows (`netglow.rs`).
    hover_net: Option<(String, Pos2)>,
    /// A value of a net class is being dragged: the undo step was taken at its start.
    drag_edit: bool,
    /// The net whose window is open (from the right-click menu of its copper with the Select tool).
    net_win: Option<String>,
    /// The net under the pointer when the right-click menu was opened (Select tool, not on a pour's corner or edge).
    net_menu: Option<String>,
    /// Tint the parts by the schematic sheet they come from.
    pub show_sheets: bool,
    /// Frames round the groups of the design's requirements, and lines between the parts that must keep apart, drawn on the board.
    pub show_req_marks: bool,
    /// What the last placement could not do for the groups of the requirements (parts that are in a copy or an IC's block).
    group_notes: Vec<String>,
    /// The lines between the nearest pads of the parts that must keep apart (with the distance).
    pub show_req_lines: bool,
    /// Automatic placement keeps the parts of each schematic sheet together as a block (only with several sheets).
    place_by_sheet: bool,
    /// Select tool: a drag that starts on a track slides it (off by default: a slip of the mouse moved wiring)
    track_drag_on: bool,
    /// Does a rubber band take the objects of the other side too (the wiring on the back with the parts and the wiring on the front)?
    /// The lock of the other side keeps its meaning for clicks: it stops a click from grabbing what is drawn faintly behind.
    pub box_both_sides: bool,
    /// Is Ctrl held (read every frame by the pointer): a click then adds the object to the selection or takes it out.
    ctrl_down: bool,
    /// Set by the net window for a net with no current: `App` answers with the schematic's DC current of that net (`fill_net_current`).
    pub net_current_request: Option<String>,
    /// The schematic's DC currents are at hand (set by the app each frame): the net window then asks for them at once, otherwise it
    /// offers a button, because the DC solve of a big circuit takes seconds and the window must open at once.
    pub dc_currents_ready: bool,
    /// The net the DC current was asked for (once per opening of its window), and what came of it (net, text).
    net_win_asked: Option<String>,
    net_win_note: Option<(String, String)>,
    /// The window that lists the currents of all nets.
    pub current_win_open: bool,
    /// The impedance (Ω) last asked for in a net window.
    z_target: f64,
    /// The net a via is placed for with the Via tool: `None` = the net of what is under the pointer (a pad, a track, a pour).
    via_net: Option<String>,
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
            pour_top: true,
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
            zoom: 12.0,
            pan: Vec2::new(25.0, 20.0),
            pending: None,
            poly: vec![],
            poly_verts: vec![],
            vdrag: None,
            vsel: None,
            vmenu: None,
            poly_mode: PolyMode::default(),
            arc_mid: None,
            shape_round: 0.0,
            selected: None,
            multi: vec![],
            boxsel: None,
            gdrag: None,
            drag_part: None,
            drag_label: None,
            trunk_mode: false,
            trunk_pads: vec![],
            trunk_note: None,
            seg_drag: None,
            shove: true,
            finish_cache: Default::default(),
            finish_job: Default::default(),
            finish_tracks: Default::default(),
            finish_job_tracks: Default::default(),
            copper_cache: Default::default(),
            silk_cache: Default::default(),
            preview_cache: Default::default(),
            status: "基板パターン: 「回路図から取り込み」で部品を配置します".into(),
            help_request: None,
            post_layout_request: None,
            currents_request: None,
            to_schematic_request: false,
            waive_reason: String::new(),
            waiver_request: None,
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
            vis: Visible { top: true, bottom: true, silk: true, parts: true, airwires: true, nets: true, zones: true, ruler: true, tracks: true, vias: true, inner_hidden: 0 },
            zone_net: "GND".into(),
            zone_clearance: 0.3,
            zone_thermal: true,
            zone_op: pcb_core::model::ShapeOp::Add,
            hole_net: String::new(),
            size_w: 50.0,
            size_h: 40.0,
            size_r: 0.0,
            view3d: view3d::View3d::default(),
            fit_pending: true,
            alt_pan: false,
            tracking: Default::default(),
            order: order::OrderState::default(),
            order_preview_src: None,
            calc_cache: std::cell::RefCell::new(CalcCache::default()),
            req_cache: std::cell::RefCell::new(None),
            zone_cache: std::cell::RefCell::new(None),
            zone_job: Default::default(),
            island_cache: std::cell::RefCell::new(None),
            zone_outline_lod: Default::default(),
            thermal_cache: std::cell::Cell::new(None),
            hover_net: None,
            drag_edit: false,
            net_win: None,
            net_menu: None,
            show_sheets: true,
            show_req_marks: true,
            group_notes: vec![],
            show_req_lines: true,
            place_by_sheet: false,
            track_drag_on: false,
            box_both_sides: true,
            ctrl_down: false,
            net_current_request: None,
            dc_currents_ready: false,
            net_win_asked: None,
            net_win_note: None,
            current_win_open: false,
            z_target: 50.0,
            via_net: None,
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
mod zoneedit;
use zoneedit::{EditTarget, VertexRef};
mod trunk;
mod netglow;
mod tracking;
mod netclass;
mod netwindow;
mod requirements;
mod sheets;
mod currentwindow;
mod file;
mod actions;
mod canvas;
mod interact;
mod draw;
mod ribbon;
pub(crate) use draw::capsule;
mod order;
mod ruler;
pub mod lesson;
mod panel;
#[cfg(test)]
mod tests;
