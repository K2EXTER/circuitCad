//! Schematic editor: place parts, wire them together, run ERC, export netlist.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use crate::part_form::PartForm;
use part_lib::PartDef;
use crate::plot::{self, PlotState, Style, Trace, PALETTE};
use sim_core::{
    ac_analysis, dc_sweep, describe_source, inverter_metrics, noise_analysis, DcSweepResult, DcSweepSettings, DcSweepSignal, NoiseKind, NoiseResult, NoiseSettings, dc_op, from_schematic, format_value, parse_value, spectrum as fft_spectrum, stats as signal_stats, transient, AcResult, AcSettings, AcSweep, DcResult, Spectrum,
    TranResult, TranSettings,
};
use std::collections::BTreeSet;
use crate::source_form::SourceForm;
use sch_core::{erc, extract, net_at, on_segment, to_spice, Component, CustomSymbol, Kind, Label, PinSide, Problem, Pt, Schematic, Wire};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Place,
    Wire,
    Label,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sel {
    Comp(usize),
    Wire(usize),
    Label(usize),
}

/// Rubber-band selection in progress (grid coordinates).
struct BoxSel {
    start: (f32, f32),
    /// Right button: anything the box touches is selected. Left button: only what it fully contains.
    crossing: bool,
}

/// Several selected objects being moved together: their positions when the drag began.
struct GroupDrag {
    anchor: Pt,
    comps: Vec<(usize, Pt)>,
    wires: Vec<(usize, Pt, Pt)>,
    labels: Vec<(usize, Pt)>,
}

fn rect_contains(lo: (f32, f32), hi: (f32, f32), p: (f32, f32)) -> bool {
    p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1
}

/// Does the segment a-b touch the rectangle? (Liang-Barsky clipping.)
fn segment_hits_rect(a: (f32, f32), b: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    for (p, q) in [(-dx, a.0 - lo.0), (dx, hi.0 - a.0), (-dy, a.1 - lo.1), (dy, hi.1 - a.1)] {
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

pub struct SchEditor {
    pub sch: Schematic,
    /// Undo / redo stacks and the last committed state (changes are detected by comparing against it).
    undo: Vec<Schematic>,
    redo: Vec<Schematic>,
    committed: Schematic,
    /// Value slider: (component, the value the slider is centred on, the text it last wrote).
    slider: Option<(usize, f64, String)>,
    /// Re-run the open analysis whenever the circuit changes.
    live: bool,
    live_snap: Schematic,
    path: Option<PathBuf>,
    tool: Tool,
    place: Kind,
    rot: u8,
    label_text: String,
    pending: Option<Pt>,
    selected: Option<Sel>,
    /// Extra objects selected by a rubber band (when two or more are selected `selected` is empty).
    multi: Vec<Sel>,
    boxsel: Option<BoxSel>,
    gdrag: Option<GroupDrag>,
    /// A rotation asked for by the property panel, applied once the panel's borrow has ended.
    rotate_req: Option<usize>,
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
    show_i: bool,
    sig_cache: String,
    tran: Option<TranView>,
    tran_tstop: String,
    tran_tstep: String,
    tran_uic: bool,
    ac: Option<AcView>,
    ac_fstart: String,
    ac_fstop: String,
    ac_ppd: String,
    noise: Option<NoiseView>,
    noise_out: String,
    noise_in: String,
    noise_temp: String,
    /// Waiting for a click on the canvas to choose the noise output node.
    /// The voltage-source form for the selected source: (component index, the value text it was
    /// read from, the form). Read again whenever the text changes from elsewhere.
    src_form: Option<(usize, String, SourceForm)>,
    sweep: Option<SweepView>,
    sweep_src: String,
    sweep_start: String,
    sweep_stop: String,
    sweep_step: String,
    noise_pick: bool,
    /// Net name under the cursor while picking, with the grid point it was computed for.
    pick_hover: Option<(Pt, Option<String>)>,
    /// Grid points of the picked output / reference nodes, drawn as rings.
    noise_marks: Vec<(Pt, bool)>,
    wave_mode: WaveMode,
    fit_pending: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum WaveMode {
    Tran,
    Ac,
    Noise,
    Sweep,
}

/// A finished DC sweep: transfer curves with the signals to draw.
struct SweepView {
    outcome: Result<DcSweepResult, Vec<String>>,
    selected: BTreeSet<String>,
    /// Draw the line output = input (its crossing with the curve is the switching threshold).
    show_diag: bool,
    plot: PlotState,
}

/// A finished noise analysis: output density plot plus the per-element breakdown.
struct NoiseView {
    outcome: Result<NoiseResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
}

/// A finished AC sweep (Bode plot).
struct AcView {
    outcome: Result<AcResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
}

/// A finished transient run with the waveform viewer state.
struct TranView {
    outcome: Result<TranResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
    /// Show the amplitude spectrum instead of the waveform.
    spectrum: bool,
    spec_t0: String,
    spec_t1: String,
    spec_plot: Option<PlotState>,
    spec_cache: Option<(String, Vec<(String, Spectrum)>)>,
}

/// A finished DC analysis, remembered with the schematic it was computed from.
struct DcView {
    outcome: Result<DcResult, Vec<String>>,
    signature: String,
    /// Voltage at every pin position.
    pin_v: Vec<(Pt, f64)>,
    /// Voltage of each wire's net (same order as `Schematic::wires`).
    wire_v: Vec<Option<f64>>,
    /// Range of the node voltages, for colouring.
    v_range: (f64, f64),
}

/// Wire colour for a net voltage: blue at the lowest voltage of the circuit, red at the highest.
fn volt_color(v: f64, (lo, hi): (f64, f64)) -> Color32 {
    if hi - lo < 1e-9 {
        return WIRE;
    }
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
    let (a, b) = ([40.0, 90.0, 220.0], [225.0, 50.0, 40.0]);
    let m = |k: usize| (a[k] + (b[k] - a[k]) * t) as u8;
    Color32::from_rgb(m(0), m(1), m(2))
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

/// One-line explanation of a basic part, shown when hovering the palette.
fn kind_tip(k: Kind) -> &'static str {
    match k {
        Kind::Resistor => "抵抗: 電流を制限したり、電圧を分けたりします (V = I × R)",
        Kind::Capacitor => "コンデンサ: 電気をためます。直流は通さず交流は通すので、ノイズ除去や電源の安定化に使います",
        Kind::Inductor => "コイル: 電流の変化に逆らいます。直流は通し、速い変化は通しにくい性質があります",
        Kind::Diode => "ダイオード: 電流を一方向 (A→K) にだけ流します。約 0.6〜0.7 V の壁があります",
        Kind::Led => "LED: 光るダイオード。必ず抵抗を直列に入れて電流を制限します",
        Kind::VSource => "電源: 直流・正弦波・パルスなど。値の書き方は、置いて選択すると下に出ます",
        Kind::Npn => "NPN トランジスタ: ベースに小さい電流を流すと、コレクタ→エミッタに大きな電流が流れます",
        Kind::Ground => "GND: 電圧の基準 (0 V)。回路には必ず 1 つ以上必要です",
        Kind::Power => "電源ラベル: 同じ名前 (VCC など) の場所を線なしでつなぎます",
        _ => "",
    }
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
            show_i: true,
            show_v: true,
            sig_cache: String::new(),
            tran: None,
            tran_tstop: "10m".into(),
            tran_tstep: "10u".into(),
            tran_uic: false,
            ac: None,
            ac_fstart: "10".into(),
            ac_fstop: "1Meg".into(),
            ac_ppd: "20".into(),
            noise: None,
            noise_out: String::new(),
            noise_in: String::new(),
            noise_temp: "27".into(),
            src_form: None,
            sweep: None,
            sweep_src: String::new(),
            sweep_start: "0".into(),
            sweep_stop: "5".into(),
            sweep_step: "0.05".into(),
            noise_pick: false,
            pick_hover: None,
            noise_marks: vec![],
            wave_mode: WaveMode::Tran,
            fit_pending: false,
            sch: Schematic::default(),
            undo: vec![],
            redo: vec![],
            committed: Schematic::default(),
            slider: None,
            live: false,
            live_snap: Schematic::default(),
            path: None,
            tool: Tool::Place,
            place: Kind::Resistor,
            rot: 0,
            label_text: "SIG".into(),
            pending: None,
            selected: None,
            multi: vec![],
            boxsel: None,
            gdrag: None,
            rotate_req: None,
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

fn sym_extents(sym: &CustomSymbol) -> (f32, f32, f32, f32) {
    let l = sym.layout();
    let (w, h) = ((l.hw + 2) as f32, (l.hh + 2) as f32);
    (-w, w, -h, h)
}

fn comp_extents(c: &Component) -> (f32, f32, f32, f32) {
    match &c.custom {
        Some(sym) => sym_extents(sym),
        None => extents(c.kind),
    }
}

const OVERLAP_MSG: &str = "部品が重なるので、そこには置けません";

/// The area a part occupies on the sheet (xmin, xmax, ymin, ymax) after rotation. Ground and
/// power symbols have none: they are meant to sit on a pin or wire next to a part.
fn sheet_box(kind: Kind, ext: (f32, f32, f32, f32), pos: Pt, rot: u8) -> Option<(f32, f32, f32, f32)> {
    if kind.is_power_symbol() {
        return None;
    }
    let mut b = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for corner in [(ext.0, ext.2), (ext.1, ext.2), (ext.0, ext.3), (ext.1, ext.3)] {
        let (x, y) = rot_f(corner, rot);
        b = (b.0.min(x), b.1.max(x), b.2.min(y), b.3.max(y));
    }
    Some((b.0 + pos.0 as f32, b.1 + pos.0 as f32, b.2 + pos.1 as f32, b.3 + pos.1 as f32))
}

/// Parts that merely touch (a pin on a neighbour's edge) do not overlap.
fn boxes_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    const EPS: f32 = 1e-3;
    a.0 < b.1 - EPS && b.0 < a.1 - EPS && a.2 < b.3 - EPS && b.2 < a.3 - EPS
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

    /// Zoom and pan so that the whole drawing is visible.
    fn fit_view(&mut self, rect: Rect) {
        let mut pts: Vec<Pt> = vec![];
        for c in &self.sch.components {
            let (x0, x1, y0, y1) = comp_extents(c);
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let r = rot_f((x, y), c.rot);
                pts.push((c.pos.0 + r.0.round() as i32, c.pos.1 + r.1.round() as i32));
            }
        }
        for w in &self.sch.wires {
            pts.push(w.a);
            pts.push(w.b);
        }
        for l in &self.sch.labels {
            pts.push(l.pos);
        }
        if pts.is_empty() {
            return;
        }
        let (x0, x1) = (pts.iter().map(|p| p.0).min().unwrap() as f32, pts.iter().map(|p| p.0).max().unwrap() as f32);
        let (y0, y1) = (pts.iter().map(|p| p.1).min().unwrap() as f32, pts.iter().map(|p| p.1).max().unwrap() as f32);
        let (w, h) = ((x1 - x0).max(4.0) + 6.0, (y1 - y0).max(4.0) + 6.0);
        self.zoom = (rect.width() / w).min(rect.height() / h).clamp(6.0, 40.0);
        self.pan = Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    }

    pub fn has_wave(&self) -> bool {
        match self.wave_mode {
            WaveMode::Tran => self.tran.is_some(),
            WaveMode::Ac => self.ac.is_some(),
            WaveMode::Noise => self.noise.is_some(),
            WaveMode::Sweep => self.sweep.is_some(),
        }
    }

    pub fn analyze_ac(&mut self) {
        self.run_ac();
    }

    pub fn set_ac_settings(&mut self, f0: &str, f1: &str, ppd: &str) {
        self.ac_fstart = f0.into();
        self.ac_fstop = f1.into();
        self.ac_ppd = ppd.into();
    }

    fn run_ac(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => ac_analysis(&c, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("AC解析: 完了 ({} 点)", r.freq.len()),
            Err(e) => format!("AC解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Ac;
    }

    pub fn analyze_sweep(&mut self) {
        self.run_sweep();
    }

    pub fn set_sweep_settings(&mut self, src: &str, start: &str, stop: &str, step: &str) {
        self.sweep_src = src.into();
        self.sweep_start = start.into();
        self.sweep_stop = stop.into();
        self.sweep_step = step.into();
    }

    fn run_sweep(&mut self) {
        let (Some(a), Some(b), Some(st)) = (parse_value(&self.sweep_start), parse_value(&self.sweep_stop), parse_value(&self.sweep_step)) else {
            self.status = "開始・終了・刻みを数値で入力してください (例: 0, 5, 0.05)".into();
            return;
        };
        let src = self.sweep_src.trim().to_string();
        let outcome = if src.is_empty() {
            Err(vec!["スイープする電源の番号を入力してください (例: V2)。入力にあたる電源です".to_string()])
        } else {
            match from_schematic(&self.sch) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => dc_sweep(&c, &DcSweepSettings { source: src.clone(), start: a, stop: b, step: st }).map_err(|e| vec![e.to_string()]),
            }
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            // the output of an inverter is the most interesting signal: prefer a falling curve
            let falling = r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).find(|s| inverter_metrics(&r.x, &s.values).is_some());
            match falling {
                Some(s) => {
                    selected.insert(s.name.clone());
                }
                None => {
                    for s in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(3) {
                        selected.insert(s.name.clone());
                    }
                }
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("DCスイープ: 完了 ({} 点, {} = {} 〜 {})", r.x.len(), src, a, b),
            Err(e) => format!("DCスイープに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if (b - a).abs() > 1e-12 { (a.min(b), a.max(b)) } else { (a - 1.0, a + 1.0) };
        self.sweep = Some(SweepView { outcome, selected, show_diag: true, plot: PlotState { x: (lo, hi), full_x: (lo, hi), log_x: false, y: [(-1.0, 1.0); 2], auto_y: [true; 2] } });
        self.wave_mode = WaveMode::Sweep;
    }

    fn sweep_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.sweep.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("DCスイープ (入力を変えたときの出力)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("横軸: スイープした電源の電圧。縦軸: 各ノードの電圧 (上) と電流 (下)");
        });
        let SweepView { outcome, selected, show_diag, plot: st } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                let names: Vec<&DcSweepSignal> = res.signals.iter().filter(|s| !s.name.contains('.')).collect();
                // key numbers of the first selected voltage that is an inverting curve
                let metrics = names.iter().filter(|s| s.unit == "V" && selected.contains(&s.name)).find_map(|s| inverter_metrics(&res.x, &s.values).map(|m| (s.name.clone(), m)));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(260.0);
                        if let Some((name, m)) = &metrics {
                            ui.colored_label(Color32::from_rgb(220, 150, 40), format!("{name} は反転する特性です"));
                            egui::Grid::new("inv_metrics").num_columns(2).show(ui, |ui| {
                                let row = |ui: &mut egui::Ui, label: &str, val: String, tip: &str| {
                                    ui.label(label).on_hover_text(tip);
                                    ui.label(val).on_hover_text(tip);
                                    ui.end_row();
                                };
                                row(ui, "VOH (出力 High)", eng(m.voh, "V"), "入力が Low のときの出力電圧。電源電圧に近いほど良い High です");
                                row(ui, "VOL (出力 Low)", eng(m.vol, "V"), "入力が High のときの出力電圧。0 V に近いほど良い Low です");
                                row(ui, "Vth (切替電圧)", eng(m.vth, "V"), "入力と出力が等しくなる電圧。ここを境に出力の High/Low が入れ替わります。電源の半分が理想です");
                                row(ui, "VIL", eng(m.vil, "V"), "これより低い入力は確実に Low と見なされます (利得が -1 になる点)");
                                row(ui, "VIH", eng(m.vih, "V"), "これより高い入力は確実に High と見なされます (利得が -1 になる点)");
                                row(ui, "NML (Lowの余裕)", eng(m.nml(), "V"), "ノイズマージン = VIL − VOL。Low の信号に、これだけノイズが乗っても誤動作しません");
                                row(ui, "NMH (Highの余裕)", eng(m.nmh(), "V"), "ノイズマージン = VOH − VIH。High の信号に、これだけノイズが落ち込んでも誤動作しません");
                                row(ui, "最大利得", format!("{:.1}", m.max_gain), "切り替わり部分の傾き。大きい (絶対値) ほどスパッと切り替わります");
                            });
                            let ratio = (m.nml().min(m.nmh())) / (m.voh - m.vol).max(1e-12);
                            ui.small(if ratio > 0.35 {
                                "ノイズマージンは十分です。ロジックとして安心して使えます。"
                            } else if ratio > 0.15 {
                                "ノイズマージンは小さめです。ノイズの多い所では誤動作に注意しましょう。"
                            } else {
                                "ノイズマージンがほとんどありません。回路や部品の組み合わせを見直しましょう。"
                            });
                        } else {
                            ui.small("電圧の信号を選ぶと、反転する (インバーターの) 特性なら VOH / VOL / 切替電圧 / ノイズマージンを計算します");
                        }
                        ui.checkbox(show_diag, "「出力 = 入力」の線").on_hover_text("この線と曲線が交わる所が切替電圧 Vth です");
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let sel_order: Vec<&str> = names.iter().filter(|s| selected.contains(&s.name)).map(|s| s.name.as_str()).collect();
                            for sg in &names {
                                let mut on = selected.contains(&sg.name);
                                let color = sel_order.iter().position(|n| *n == sg.name).map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(&sg.name).color(c),
                                    _ => egui::RichText::new(&sg.name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(sg.name.clone());
                                    } else {
                                        selected.remove(&sg.name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(usize, &DcSweepSignal)> = names.iter().filter(|s| selected.contains(&s.name)).enumerate().map(|(k, s)| (k, *s)).collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mut volts: Vec<Trace> = chosen.iter().filter(|(_, s)| s.unit == "V").map(|(k, s)| Trace { name: &s.name, color: PALETTE[*k % PALETTE.len()], t: &res.x, v: &s.values }).collect();
                    if *show_diag && !volts.is_empty() {
                        volts.push(Trace { name: "出力=入力", color: Color32::from_gray(120), t: &res.x, v: &res.x });
                    }
                    let amps: Vec<Trace> = chosen.iter().filter(|(_, s)| s.unit == "A").map(|(k, s)| Trace { name: &s.name, color: PALETTE[*k % PALETTE.len()], t: &res.x, v: &s.values }).collect();
                    let vs = Style { unit: "V", x_unit: "V", x_name: &res.source, plain_y: false };
                    let is = Style { unit: "A", x_unit: "V", x_name: &res.source, plain_y: false };
                    match (volts.is_empty(), amps.is_empty()) {
                        (false, false) => {
                            let h = rect.height() / 2.0;
                            let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                            let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                            plot::draw_group(ui, top, egui::Id::new("sweep_v"), &volts, st, 0, &vs, false);
                            plot::draw_group(ui, bottom, egui::Id::new("sweep_i"), &amps, st, 1, &is, true);
                        }
                        (false, true) => plot::draw_group(ui, rect, egui::Id::new("sweep_v"), &volts, st, 0, &vs, true),
                        _ => plot::draw_group(ui, rect, egui::Id::new("sweep_i"), &amps, st, 1, &is, true),
                    }
                });
            }
        }
        if close {
            self.sweep = None;
        }
    }

    /// A tip for what to try next, based on what is on screen.
    pub fn hint(&self) -> String {
        if self.noise_pick {
            return "ノイズを測りたいノードをクリックします。Shift+クリックで基準ノード、Esc で中止".into();
        }
        if self.sch.components.is_empty() {
            return "まず部品を置きましょう。左の一覧から選んで画面をクリック。「ファイル → 開く」で docs/examples のサンプル回路も試せます".into();
        }
        if self.pending.is_some() {
            return "配線中: 次の点をクリックして線を延長、ダブルクリックか右クリックで終了します".into();
        }
        match self.tool {
            Tool::Place if !self.has_wave() => return "クリックで部品を置きます。R キーで回転、右クリックで中断。置いた後は「選択」で値を変えられます".into(),
            Tool::Wire => return "部品のピン (赤い点) から点へクリックしてつなぎます。T字に分岐する場合は、線の途中に別の線の端を置きます".into(),
            Tool::Label => return "ラベル名を決めて回路図の点をクリック。同じ名前の場所は線なしでつながります (電源 VCC や GND に便利)".into(),
            Tool::Select | Tool::Place => {}
        }
        if let Some(Sel::Comp(i)) = self.selected
            && let Some(c) = self.sch.components.get(i) {
                return match c.kind {
                    Kind::Resistor => "値を書き換えるか「スライダー」を動かしてみましょう。「自動で再計算」をオンにすると、電圧や波形が動きに合わせて変わります".into(),
                    Kind::Capacitor => "コンデンサは直流では電流を通さず、周波数が高いほど通しやすくなります。AC解析で効き方を見てみましょう".into(),
                    Kind::VSource => "電源の種類は「直流 / 交流 / パルス」のボタンで切り替えられます。「AC解析の入力にする」をオンにすると周波数特性の測定に使えます (ノイズ解析・DCスイープの入力にも指定できます)".into(),
                    Kind::Npn => "トランジスタはベース電流で大きなコレクタ電流を制御します。ベースに抵抗を付けて電圧を見てみましょう".into(),
                    _ => "選んだ部品の値は左の欄で変えられます。R キーで回転、Delete で削除".into(),
                };
            }
        if self.sweep.is_some() && self.wave_mode == WaveMode::Sweep {
            return "出力 = 入力の線と曲線が交わる所が切替電圧です。VIL・VIH の間が「どちらとも言えない」領域で、狭いほどスパッと切り替わります。電源電圧や部品を変えて再実行してみましょう".into();
        }
        if self.noise.is_some() && self.wave_mode == WaveMode::Noise {
            return "ノイズ一覧の素子名にカーソルを合わせると原因と対策が出ます。抵抗値を変えて再実行すると、雑音が √(抵抗値) に比例して変わるのが分かります".into();
        }
        if self.ac.is_some() && self.wave_mode == WaveMode::Ac {
            return "ボード線図: 上が大きさ(dB)、下が位相。-3 dB になる周波数がカットオフ周波数です。グラフはドラッグで移動、ホイールで拡大縮小".into();
        }
        if self.tran.is_some() && self.wave_mode == WaveMode::Tran {
            return "波形はドラッグで移動、ホイールで拡大縮小、ダブルクリックで全体表示。左の一覧で表示する信号を選べます".into();
        }
        if self.dc.is_none() {
            return "回路が描けたら「DC動作点を解析」で各点の電圧と部品の電流 (配線は低電圧が青、高電圧が赤) を見てみましょう。ERC で配線ミスの点検もできます".into();
        }
        "電圧が想定と違う所から疑ってみましょう。次は過渡解析 (波形)・AC解析 (周波数特性)・ノイズ解析を試せます。回路が完成したら「基板パターン」タブへ".into()
    }

    pub fn analyze_noise(&mut self) {
        self.run_noise();
    }

    pub fn set_noise_settings(&mut self, out: &str, input: &str, temp: &str) {
        self.noise_out = out.into();
        self.noise_in = input.into();
        self.noise_temp = temp.into();
    }

    /// A click at `pt` while choosing the noise output: plain click = output node,
    /// Shift+click = reference node (differential output).
    fn pick_noise_node(&mut self, pt: Pt, shift: bool) {
        let Some(name) = net_at(&self.sch, pt) else {
            self.status = "そこには接続された配線・ピンがありません。部品のピンか配線の上をクリックしてください".into();
            return;
        };
        if name == "GND" && !shift {
            self.status = "GND は電圧の基準なので出力にできません。測りたいノードをクリックしてください (Shift+クリックで基準に使えます)".into();
            return;
        }
        if shift {
            let first = self.noise_out.split(',').next().unwrap_or("").trim().to_string();
            self.noise_out = if first.is_empty() { name } else { format!("{first},{name}") };
            self.noise_marks.retain(|(_, r)| !*r);
            self.noise_marks.push((pt, true));
        } else {
            self.noise_out = name;
            self.noise_marks.clear();
            self.noise_marks.push((pt, false));
        }
        self.noise_pick = false;
        self.run_noise();
    }

    fn run_noise(&mut self) {
        let (Some(f0), Some(f1), Some(ppd), Some(temp)) =
            (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd), self.noise_temp.trim().parse::<f64>().ok())
        else {
            self.status = "開始・終了周波数、点数/デケード、温度[℃]を数値で入力してください".into();
            return;
        };
        let (out, inp) = (self.noise_out.trim().to_string(), self.noise_in.trim().to_string());
        let outcome = if out.is_empty() {
            Err(vec!["出力ノードの名前を入力してください (ネットラベル名など。差動は A,B)".to_string()])
        } else {
            match from_schematic(&self.sch) {
                Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
                Ok(c) => {
                    let (out_p, out_n) = match out.split_once(',') {
                        Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
                        None => (out.clone(), None),
                    };
                    let st = NoiseSettings {
                        sweep: AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) },
                        output: out_p,
                        output_ref: out_n,
                        input: if inp.is_empty() { None } else { Some(inp) },
                        temp_c: temp,
                    };
                    noise_analysis(&c, &st).map_err(|e| vec![e.to_string()])
                }
            }
        };
        let mut selected = BTreeSet::new();
        selected.insert("合計".to_string());
        self.status = match &outcome {
            Ok(r) => format!("ノイズ解析: 出力 {} rms ({} Hz〜{} Hz)", eng(r.out_rms, "V"), f0, f1),
            Err(e) => format!("ノイズ解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        self.noise = Some(NoiseView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi) });
        self.wave_mode = WaveMode::Noise;
    }

    fn noise_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.noise.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("ノイズ解析 (熱雑音・ショット雑音)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("縦軸: 電圧ノイズ密度 [dB, 0 dB = 1 nV/√Hz]。60 dB = 1 µV/√Hz");
        });
        let NoiseView { outcome, selected, plot: st } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                let to_db = |psd: &[f64]| -> Vec<f64> { psd.iter().map(|p| 10.0 * (p.max(1e-60) / 1e-18).log10()).collect() };
                let mut order: Vec<(String, f64)> = vec![("合計".to_string(), res.out_rms)];
                if let Some(r) = res.in_rms {
                    order.push(("入力換算".to_string(), r));
                }
                order.extend(res.contributions.iter().map(|c| (c.name.clone(), c.rms)));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.label(format!("出力合計: {}", eng(res.out_rms, "V rms")));
                        if let Some(r) = res.in_rms {
                            ui.label(format!("入力換算: {}", eng(r, "rms"))).on_hover_text("出力のノイズを回路の利得で割って入力側に直した値。入力信号がこれより小さいと、ノイズに埋もれます");
                        }
                        if let Some((top, share)) = res.dominant() {
                            let info = describe_source(&top.name);
                            ui.colored_label(Color32::from_rgb(220, 150, 40), format!("最大の原因: {} ({}) — 全体の約 {:.0}%", top.name, info.title, share * 100.0));
                            if !info.remedy.is_empty() {
                                ui.small(format!("対策: {}", info.remedy));
                            }
                        }
                        egui::CollapsingHeader::new("グラフの読み方").show(ui, |ui| {
                            ui.small("縦軸は雑音の「密度」で、1Hz の幅あたりの電圧 (dB, 0 dB = 1 nV/√Hz)。60 dB で 1 µV/√Hz。");
                            ui.small("rms 値は、グラフの帯域全体で雑音を足し合わせた実効値です。");
                            ui.small("雑音は互いに無関係に出るので、足すときは二乗して足し (電力の和)、最後に √ します。だから一番大きい原因を減らすのが近道です。");
                            ui.small("素子の名前にカーソルを合わせると、その雑音の原因と対策が出ます。");
                        });
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (name, rms) in &order {
                                let mut on = selected.contains(name);
                                let color = order.iter().filter(|(n, _)| selected.contains(n)).position(|(n, _)| n == name).map(|i| PALETTE[i % PALETTE.len()]);
                                let share = if name == "合計" || name == "入力換算" { None } else { Some(res.share(name)) };
                                let label = match share {
                                    Some(sh) => format!("{name}  {} ({:.0}%)", eng(*rms, "V"), sh * 100.0),
                                    None => format!("{name}  {}", eng(*rms, "V")),
                                };
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(label).color(c),
                                    _ => egui::RichText::new(label),
                                };
                                let resp = ui.checkbox(&mut on, text);
                                if share.is_some() {
                                    let info = describe_source(name);
                                    let kind = if info.kind == NoiseKind::Shot { "ショット雑音" } else { "熱雑音" };
                                    let msg = format!("{} [{kind}]\n{}\n対策: {}", info.title, info.why, info.remedy);
                                    resp.clone().on_hover_text(msg);
                                }
                                if resp.changed() {
                                    if on {
                                        selected.insert(name.clone());
                                    } else {
                                        selected.remove(name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(&str, Vec<f64>)> = order
                        .iter()
                        .filter(|(n, _)| selected.contains(n))
                        .map(|(n, _)| {
                            let v = match n.as_str() {
                                "合計" => to_db(&res.out_psd),
                                "入力換算" => to_db(res.in_psd.as_deref().unwrap_or(&[])),
                                _ => res.contribution(n).map(|c| to_db(&c.psd)).unwrap_or_default(),
                            };
                            (n.as_str(), v)
                        })
                        .collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から項目を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let traces: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, v))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v }).collect();
                    let sty = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, rect, egui::Id::new("noise_density"), &traces, st, 0, &sty, true);
                });
            }
        }
        if close {
            self.noise = None;
        }
    }

    /// Bottom panel: the waveform viewer for the most recent analysis.
    pub fn wave_panel(&mut self, ui: &mut egui::Ui) {
        match self.wave_mode {
            WaveMode::Tran => self.tran_wave_panel(ui),
            WaveMode::Ac => self.ac_wave_panel(ui),
            WaveMode::Noise => self.noise_wave_panel(ui),
            WaveMode::Sweep => self.sweep_wave_panel(ui),
        }
    }

    fn ac_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.ac.as_mut() else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong("AC解析 (ボード線図)");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("ゲインは 0dB = 1 V(または 1 A)。入力の電源を「AC 1」にすると、そのまま伝達関数になります");
        });
        let AcView { outcome, selected, plot: st } = view;
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order.iter().filter(|n| selected.contains(**n)).position(|n| n == name).map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(*name).color(c),
                                    _ => egui::RichText::new(*name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(name.to_string());
                                    } else {
                                        selected.remove(*name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let chosen: Vec<(&str, Vec<f64>, Vec<f64>)> = order
                        .iter()
                        .filter(|n| selected.contains(**n))
                        .filter_map(|n| res.signal(n).map(|sg| (*n, sg.db(), sg.phase_deg())))
                        .collect();
                    if chosen.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                        return;
                    }
                    let mag: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, db, _))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: db }).collect();
                    let phase: Vec<Trace> = chosen.iter().enumerate().map(|(k, (n, _, ph))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &res.freq, v: ph }).collect();
                    let h = rect.height() / 2.0;
                    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h - 2.0));
                    let bottom = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h), Vec2::new(rect.width(), h - 2.0));
                    let ms = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                    let ps = Style { unit: "°", x_unit: "Hz", x_name: "f", plain_y: true };
                    plot::draw_group(ui, top, egui::Id::new("bode_mag"), &mag, st, 0, &ms, false);
                    plot::draw_group(ui, bottom, egui::Id::new("bode_phase"), &phase, st, 1, &ps, true);
                });
            }
        }
        if close {
            self.ac = None;
        }
    }


    /// Switch the waveform viewer to the amplitude spectrum (command line `--spectrum`).
    pub fn show_spectrum(&mut self) {
        if let Some(v) = self.tran.as_mut() {
            v.spectrum = true;
        }
    }

    pub fn analyze_tran(&mut self) {
        self.run_tran();
    }

    pub fn set_tran_settings(&mut self, tstop: &str, tstep: &str) {
        self.tran_tstop = tstop.into();
        self.tran_tstep = tstep.into();
    }

    fn run_tran(&mut self) {
        let (Some(tstop), Some(tstep)) = (parse_value(&self.tran_tstop), parse_value(&self.tran_tstep)) else {
            self.status = "終了時間と時間刻みを数値で入力してください (例: 10m, 10u)".into();
            return;
        };
        let outcome = match from_schematic(&self.sch) {
            Err(errs) => Err(errs.iter().map(|e| e.to_string()).collect()),
            Ok(c) => transient(&c, &TranSettings { tstop, tstep, uic: self.tran_uic }).map_err(|e| vec![e.to_string()]),
        };
        let mut selected = BTreeSet::new();
        if let Ok(r) = &outcome {
            for sg in r.signals.iter().filter(|s| s.unit == "V" && !s.name.contains('.')).take(4) {
                selected.insert(sg.name.clone());
            }
        }
        self.status = match &outcome {
            Ok(r) => format!("過渡解析: 完了 ({} 点, 再試行 {} 回)", r.time.len(), r.retries),
            Err(e) => format!("過渡解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.tran = Some(TranView {
            outcome,
            selected,
            plot: PlotState::new(tstop),
            spectrum: false,
            spec_t0: format!("{:.6e}", tstop * 0.1),
            spec_t1: format!("{:.6e}", tstop),
            spec_plot: None,
            spec_cache: None,
        });
        self.wave_mode = WaveMode::Tran;
    }

    /// Transient waveforms.
    fn tran_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.tran.as_mut() else { return };
        let mut close = false;
        let TranView { outcome, selected, plot: st, spectrum, spec_t0, spec_t1, spec_plot, spec_cache } = view;
        ui.horizontal(|ui| {
            ui.strong("過渡解析の波形");
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.checkbox(spectrum, "スペクトル表示");
            if *spectrum {
                ui.label("解析範囲");
                ui.add(egui::TextEdit::singleline(spec_t0).desired_width(70.0));
                ui.label("〜");
                ui.add(egui::TextEdit::singleline(spec_t1).desired_width(70.0));
                ui.small("秒 / 0 dB = 1 V(ピーク値)。立ち上がり直後を除いた範囲を指定してください");
            } else {
                ui.small("ドラッグ: 移動 / ホイール: 時間軸ズーム (Ctrl: 縦軸) / ダブルクリック: 全体表示 / 統計値は表示中の範囲");
            }
        });
        match outcome {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(res) => {
                for w in &res.warnings {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), w);
                }
                ui.horizontal_top(|ui| {
                    let order: Vec<&str> = res.signals.iter().filter(|s| !s.name.contains('.')).map(|s| s.name.as_str()).collect();
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if ui.button("選択をクリア").clicked() {
                            selected.clear();
                        }
                        // mean and AC RMS (= noise / ripple amplitude) over the visible window
                        let (w0, w1) = if *spectrum {
                            (parse_value(spec_t0).unwrap_or(0.0), parse_value(spec_t1).unwrap_or(f64::MAX))
                        } else {
                            st.x
                        };
                        for name in order.iter().filter(|n| selected.contains(**n)).take(6) {
                            if let Some(sg) = res.signal(name)
                                && let Some(sx) = signal_stats(&res.time, &sg.values, w0, w1) {
                                    let u = sg.unit;
                                    ui.small(format!("{name}\n  平均 {}{u}  AC実効値 {}{u}", plot::axis_label(sx.mean, sx.mean.abs() / 1000.0), plot::axis_label(sx.rms_ac, sx.rms_ac.abs() / 1000.0)));
                                }
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for name in &order {
                                let mut on = selected.contains(*name);
                                let color = order
                                    .iter()
                                    .filter(|n| selected.contains(**n))
                                    .position(|n| n == name)
                                    .map(|i| PALETTE[i % PALETTE.len()]);
                                let text = match color {
                                    Some(c) if on => egui::RichText::new(*name).color(c),
                                    _ => egui::RichText::new(*name),
                                };
                                if ui.checkbox(&mut on, text).changed() {
                                    if on {
                                        selected.insert(name.to_string());
                                    } else {
                                        selected.remove(*name);
                                    }
                                }
                            }
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    if *spectrum {
                        let (Some(t0), Some(t1)) = (parse_value(spec_t0), parse_value(spec_t1)) else {
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "解析範囲を数値で入力してください", FontId::proportional(14.0), Color32::from_rgb(230, 60, 60));
                            return;
                        };
                        let key = format!("{t0:e}|{t1:e}|{selected:?}");
                        if spec_cache.as_ref().map(|c| &c.0) != Some(&key) {
                            let list: Vec<(String, Spectrum)> = order
                                .iter()
                                .filter(|n| selected.contains(**n))
                                .filter_map(|n| res.signal(n).and_then(|sg| fft_spectrum(&res.time, &sg.values, t0, t1)).map(|sp| (n.to_string(), sp)))
                                .collect();
                            *spec_plot = list.first().map(|(_, sp)| PlotState::log(sp.freq[0], *sp.freq.last().unwrap()));
                            *spec_cache = Some((key, list));
                        }
                        let list = &spec_cache.as_ref().unwrap().1;
                        let (Some(sp_state), false) = (spec_plot.as_mut(), list.is_empty()) else {
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から信号を選んでください (範囲が不正な場合も表示されません)", FontId::proportional(14.0), Color32::from_gray(150));
                            return;
                        };
                        let dbs: Vec<Vec<f64>> = list.iter().map(|(_, sp)| sp.amp.iter().map(|a| 20.0 * a.max(1e-12).log10()).collect()).collect();
                        let traces: Vec<Trace> = list
                            .iter()
                            .zip(&dbs)
                            .enumerate()
                            .map(|(k, ((n, sp), db))| Trace { name: n, color: PALETTE[k % PALETTE.len()], t: &sp.freq, v: db })
                            .collect();
                        let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                        plot::draw_group(ui, rect, egui::Id::new("spectrum"), &traces, sp_state, 0, &style, true);
                        return;
                    }
                    // traces per unit group, colours assigned in list order
                    let mut groups: [Vec<Trace>; 2] = [vec![], vec![]];
                    let mut k = 0;
                    for name in &order {
                        if !selected.contains(*name) {
                            continue;
                        }
                        if let Some(sg) = res.signal(name) {
                            let g = if sg.unit == "V" { 0 } else { 1 };
                            groups[g].push(Trace { name: &sg.name, color: PALETTE[k % PALETTE.len()], t: &res.time, v: &sg.values });
                            k += 1;
                        }
                    }
                    let used: Vec<usize> = (0..2).filter(|g| !groups[*g].is_empty()).collect();
                    if used.is_empty() {
                        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "左の一覧から波形を選んでください", FontId::proportional(14.0), Color32::from_gray(150));
                    }
                    let h = rect.height() / used.len().max(1) as f32;
                    for (n, g) in used.iter().enumerate() {
                        let sub = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + h * n as f32), Vec2::new(rect.width(), h - 2.0));
                        let style = Style { unit: if *g == 0 { "V" } else { "A" }, x_unit: "s", x_name: "t", plain_y: false };
                        plot::draw_group(ui, sub, egui::Id::new(("wave", *g)), &groups[*g], st, *g, &style, n + 1 == used.len());
                    }
                });
            }
        }
        if close {
            self.tran = None;
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
                    if let Some(c) = self.sch.components.iter().find(|c| c.refdes == p.refdes)
                        && let Some((_, pt)) = c.pins().into_iter().find(|(n, _)| *n == p.pin) {
                            pin_v.push((pt, v));
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
        let mut wire_v = vec![None; self.sch.wires.len()];
        let mut v_range = (0.0f64, 0.0f64);
        if let Ok(res) = &outcome {
            for (k, w) in self.sch.wires.iter().enumerate() {
                wire_v[k] = net_at(&self.sch, w.a).and_then(|n| res.voltage(&n));
            }
            let vs = res.voltages.iter().map(|(_, v)| *v);
            v_range = (vs.clone().fold(0.0, f64::min), vs.fold(0.0, f64::max));
        }
        self.dc = Some(DcView { outcome, signature: self.signature(), pin_v, wire_v, v_range });
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

    fn is_sel(&self, s: Sel) -> bool {
        self.selected == Some(s) || self.multi.contains(&s)
    }

    fn set_selection(&mut self, items: Vec<Sel>) {
        if items.len() == 1 {
            self.selected = Some(items[0]);
            self.multi.clear();
        } else {
            self.selected = None;
            self.multi = items;
        }
    }

    /// Objects touched by the box (grid coordinates). `crossing`: any overlap counts, otherwise
    /// the object must lie completely inside.
    fn objects_in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Sel> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let mut out = vec![];
        for (i, c) in self.sch.components.iter().enumerate() {
            let (x0, x1, y0, y1) = comp_extents(c);
            let pts: Vec<(f32, f32)> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .map(|&p| {
                    let w = rot_f(p, c.rot % 4);
                    (w.0 + c.pos.0 as f32, w.1 + c.pos.1 as f32)
                })
                .collect();
            let (bl, bh) = (
                (pts.iter().map(|p| p.0).fold(f32::MAX, f32::min), pts.iter().map(|p| p.1).fold(f32::MAX, f32::min)),
                (pts.iter().map(|p| p.0).fold(f32::MIN, f32::max), pts.iter().map(|p| p.1).fold(f32::MIN, f32::max)),
            );
            let hit = if crossing { bl.0 <= hi.0 && bh.0 >= lo.0 && bl.1 <= hi.1 && bh.1 >= lo.1 } else { rect_contains(lo, hi, bl) && rect_contains(lo, hi, bh) };
            if hit {
                out.push(Sel::Comp(i));
            }
        }
        for (i, w) in self.sch.wires.iter().enumerate() {
            let (pa, pb) = ((w.a.0 as f32, w.a.1 as f32), (w.b.0 as f32, w.b.1 as f32));
            let hit = if crossing { segment_hits_rect(pa, pb, lo, hi) } else { rect_contains(lo, hi, pa) && rect_contains(lo, hi, pb) };
            if hit {
                out.push(Sel::Wire(i));
            }
        }
        for (i, l) in self.sch.labels.iter().enumerate() {
            if rect_contains(lo, hi, (l.pos.0 as f32, l.pos.1 as f32)) {
                out.push(Sel::Label(i));
            }
        }
        out
    }

    /// Would a part of this kind, placed at `pos` with `rot`, overlap another part? `ignore` lists
    /// parts that are being moved themselves.
    fn would_overlap(&self, kind: Kind, ext: (f32, f32, f32, f32), pos: Pt, rot: u8, ignore: &[usize]) -> bool {
        let Some(b) = sheet_box(kind, ext, pos, rot) else { return false };
        self.sch.components.iter().enumerate().any(|(j, o)| {
            !ignore.contains(&j) && sheet_box(o.kind, comp_extents(o), o.pos, o.rot).is_some_and(|ob| boxes_overlap(b, ob))
        })
    }

    /// Rotate a part a quarter turn unless that would put it on top of another part.
    fn rotate_comp(&mut self, i: usize) {
        let c = &self.sch.components[i];
        let next = (c.rot + 1) % 4;
        if self.would_overlap(c.kind, comp_extents(c), c.pos, next, &[i]) {
            self.status = "回すと他の部品と重なるので、回せません".into();
        } else {
            self.sch.components[i].rot = next;
        }
    }

    fn start_group_drag(&mut self, anchor: Pt) {
        let mut g = GroupDrag { anchor, comps: vec![], wires: vec![], labels: vec![] };
        for s in &self.multi {
            match *s {
                Sel::Comp(i) => g.comps.push((i, self.sch.components[i].pos)),
                Sel::Wire(i) => g.wires.push((i, self.sch.wires[i].a, self.sch.wires[i].b)),
                Sel::Label(i) => g.labels.push((i, self.sch.labels[i].pos)),
            }
        }
        self.gdrag = Some(g);
    }

    fn delete_selected(&mut self) {
        if !self.multi.is_empty() {
            let mut comps: Vec<usize> = vec![];
            let (mut wires, mut labels) = (vec![], vec![]);
            for s in self.multi.drain(..) {
                match s {
                    Sel::Comp(i) => comps.push(i),
                    Sel::Wire(i) => wires.push(i),
                    Sel::Label(i) => labels.push(i),
                }
            }
            for (v, n) in [(&mut comps, 0), (&mut wires, 1), (&mut labels, 2)] {
                v.sort_unstable_by(|a, b| b.cmp(a));
                for &i in v.iter() {
                    match n {
                        0 if i < self.sch.components.len() => drop(self.sch.components.remove(i)),
                        1 if i < self.sch.wires.len() => drop(self.sch.wires.remove(i)),
                        2 if i < self.sch.labels.len() => drop(self.sch.labels.remove(i)),
                        _ => {}
                    }
                }
            }
            self.selected = None;
            return;
        }
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

    // ------------------------------------------------------------- history

    /// Record edits made since the last frame as one undo step. Waits while the mouse button is held
    /// (a drag is one step) or a text box has focus (typing a value is one step).
    fn track_history(&mut self, ui: &egui::Ui) {
        if self.sch == self.committed {
            return;
        }
        if ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        self.commit_history();
    }

    fn commit_history(&mut self) {
        if self.sch == self.committed {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.committed, self.sch.clone()));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn reset_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.committed = self.sch.clone();
    }

    pub fn undo(&mut self) {
        self.commit_history();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.sch, prev));
            self.after_history("元に戻しました");
        }
    }

    pub fn redo(&mut self) {
        self.commit_history();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.sch, next));
            self.after_history("やり直しました");
        }
    }

    fn after_history(&mut self, msg: &str) {
        self.committed = self.sch.clone();
        self.selected = None;
        self.multi.clear();
        self.boxsel = None;
        self.gdrag = None;
        self.drag = None;
        self.pending = None;
        self.problems.clear();
        self.status = msg.into();
    }

    /// With "自動で再計算" on, redo the analysis that is on screen after the circuit changed,
    /// keeping the signals and zoom the user had chosen.
    fn live_update(&mut self) {
        if !self.live {
            return;
        }
        if self.sch == self.live_snap {
            return;
        }
        self.live_snap = self.sch.clone();
        if self.dc.is_some() {
            self.run_dc();
        }
        match self.wave_mode {
            WaveMode::Tran if self.tran.is_some() => {
                let old = self.tran.take();
                self.run_tran();
                if let (Some(o), Some(n)) = (old, self.tran.as_mut())
                    && n.outcome.is_ok() && o.outcome.is_ok() {
                        n.selected = o.selected;
                        n.plot = o.plot;
                        n.spectrum = o.spectrum;
                        n.spec_t0 = o.spec_t0;
                        n.spec_t1 = o.spec_t1;
                        n.spec_plot = o.spec_plot;
                    }
            }
            WaveMode::Ac if self.ac.is_some() => {
                let old = self.ac.take();
                self.run_ac();
                if let (Some(o), Some(n)) = (old, self.ac.as_mut())
                    && n.outcome.is_ok() && o.outcome.is_ok() {
                        n.selected = o.selected;
                        n.plot = o.plot;
                    }
            }
            _ => {}
        }
    }

    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        self.track_history(ui);
        self.live_update();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, SCH_BG);
        if self.dc.is_some() {
            self.sig_cache = self.signature();
        }
        if self.fit_pending {
            self.fit_pending = false;
            self.fit_view(rect);
        }

        // in the Select tool the right button draws a crossing selection box instead of panning
        let rmb_selects = self.tool == Tool::Select && !self.noise_pick;
        if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Secondary) && !rmb_selects) {
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
        if self.noise_pick && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.noise_pick = false;
            self.status = "ノードの選択を中止しました".into();
        }
        if let Some(i) = self.rotate_req.take() {
            self.rotate_comp(i);
        }
        if !typing {
            let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            if cmd && ui.input(|i| i.key_pressed(egui::Key::Z)) {
                if shift { self.redo() } else { self.undo() }
            } else if cmd && ui.input(|i| i.key_pressed(egui::Key::Y)) {
                self.redo();
            }
            if ui.input(|i| i.key_pressed(egui::Key::R)) {
                match self.selected {
                    Some(Sel::Comp(i)) => self.rotate_comp(i),
                    _ if self.multi.iter().any(|s| matches!(s, Sel::Comp(_))) => {
                        for s in self.multi.clone() {
                            if let Sel::Comp(i) = s {
                                self.rotate_comp(i);
                            }
                        }
                    }
                    _ => self.rot = (self.rot + 1) % 4,
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                self.delete_selected();
            }
            if ui.input(|i| i.key_pressed(egui::Key::F)) {
                self.fit_pending = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending = None;
                self.selected = None;
                self.multi.clear();
                self.boxsel = None;
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

        // wires (tinted by the net voltage when a DC result is showing)
        let live_dc = self.dc.as_ref().filter(|d| d.signature == self.signature_cached() && d.outcome.is_ok());
        for (i, w) in self.sch.wires.iter().enumerate() {
            let sel = self.is_sel(Sel::Wire(i));
            let tint = live_dc.filter(|_| self.show_v).and_then(|d| d.wire_v.get(i).copied().flatten().map(|v| volt_color(v, d.v_range)));
            painter.line_segment(
                [self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32))],
                Stroke::new(if sel { 3.0 } else { 2.2 }, if sel { Color32::from_rgb(0, 80, 220) } else { tint.unwrap_or(WIRE) }),
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
            let sel = self.is_sel(Sel::Comp(i));
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
            let sel = self.is_sel(Sel::Label(i));
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
        if self.show_v
            && let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached()) {
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
        if self.show_i
            && let Some(dc) = self.dc.as_ref().filter(|d| d.signature == self.signature_cached())
            && let Ok(res) = &dc.outcome {
                let f = FontId::proportional((self.zoom * 0.5).clamp(9.0, 14.0));
                let col = Color32::from_rgb(200, 90, 0);
                for c in self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()) {
                    let Some(el) = res.element(&c.refdes) else { continue };
                    let ext = comp_extents(c);
                    let Some((x0, x1, _, y1)) = sheet_box(c.kind, ext, c.pos, c.rot) else { continue };
                    // below the part's value text
                    let at = self.to_screen(rect, ((x0 + x1) / 2.0, y1 + 1.3));
                    let galley = painter.layout_no_wrap(eng(el.current, "A"), f.clone(), col);
                    let pos = at - Vec2::new(galley.size().x / 2.0, 0.0);
                    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(1.0), 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
                    painter.galley(pos, galley, col);
                    // arrow along the part: the direction the (conventional) current flows
                    let pins = c.pins();
                    if pins.len() == 2 && el.current.abs() > 1e-12 {
                        let (p1, p2) = (self.to_screen(rect, (pins[0].1.0 as f32, pins[0].1.1 as f32)), self.to_screen(rect, (pins[1].1.0 as f32, pins[1].1.1 as f32)));
                        let (from, to) = if el.current >= 0.0 { (p1, p2) } else { (p2, p1) };
                        let dir = (to - from).normalized();
                        let tip = from + (to - from) * 0.5 + dir * 6.0;
                        let side = Vec2::new(-dir.y, dir.x) * 4.0;
                        painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir * 9.0 + side, tip - dir * 9.0 - side], col, Stroke::NONE));
                    }
                }
            }
        for pr in &self.problems {
            if let Some(a) = pr.at {
                painter.circle_stroke(self.to_screen(rect, (a.0 as f32, a.1 as f32)), 11.0, Stroke::new(2.0, Color32::from_rgb(230, 0, 200)));
            }
        }

        for (pt, is_ref) in &self.noise_marks {
            let c = self.to_screen(rect, (pt.0 as f32, pt.1 as f32));
            let col = if *is_ref { Color32::from_rgb(40, 140, 40) } else { Color32::from_rgb(220, 90, 0) };
            painter.circle_stroke(c, 10.0, Stroke::new(2.5, col));
            painter.text(c + Vec2::new(12.0, -12.0), Align2::LEFT_BOTTOM, if *is_ref { "基準" } else { "出力" }, FontId::proportional(12.0), col);
        }
        if self.noise_pick {
            let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
            painter.rect_filled(banner, 0.0, Color32::from_rgba_unmultiplied(220, 90, 0, 230));
            painter.text(
                banner.left_center() + Vec2::new(10.0, 0.0),
                Align2::LEFT_CENTER,
                "ノイズを測りたいノードをクリック (Shift+クリック: 基準ノード / Esc: 中止)",
                FontId::proportional(14.0),
                Color32::WHITE,
            );
        }

        // rubber-band selection: left drag = fully enclosed, right drag = anything touched
        if self.tool == Tool::Select && !self.noise_pick {
            let origin = ui.input(|i| i.pointer.press_origin());
            for (btn, crossing) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Secondary, true)] {
                if resp.drag_started_by(btn) && self.drag.is_none() && self.gdrag.is_none()
                    && let Some(o) = origin {
                        let g0 = self.to_grid(rect, o);
                        // a left drag that starts on a part moves it instead (below)
                        if crossing || self.comp_at(g0).is_none() {
                            self.boxsel = Some(BoxSel { start: g0, crossing });
                        }
                    }
            }
            if let Some(b) = &self.boxsel {
                let cur = ui.input(|i| i.pointer.latest_pos()).map(|p| self.to_grid(rect, p)).unwrap_or(b.start);
                let (p0, p1) = (self.to_screen(rect, b.start), self.to_screen(rect, cur));
                let r = Rect::from_two_pos(p0, p1);
                let (fill, line) = if b.crossing {
                    (Color32::from_rgba_unmultiplied(40, 160, 60, 40), Color32::from_rgb(30, 140, 50))
                } else {
                    (Color32::from_rgba_unmultiplied(40, 100, 230, 40), Color32::from_rgb(30, 80, 210))
                };
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.2, line), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (start, crossing) = (b.start, b.crossing);
                    let mut items = self.objects_in_box(start, cur, crossing);
                    if ui.input(|i| i.modifiers.shift) {
                        let mut cur_sel = self.multi.clone();
                        cur_sel.extend(self.selected);
                        for s in items {
                            if !cur_sel.contains(&s) {
                                cur_sel.push(s);
                            }
                        }
                        items = cur_sel;
                    }
                    self.status = format!("{} 個を選択 ({}範囲)", items.len(), if crossing { "接触した物を含む" } else { "完全に囲んだ" });
                    self.set_selection(items);
                    self.boxsel = None;
                }
            }
        } else {
            self.boxsel = None;
        }

        // interaction
        let Some(pos) = hover else { return };
        let g = self.to_grid(rect, pos);
        let snapped: Pt = (g.0.round() as i32, g.1.round() as i32);
        let sp = self.to_screen(rect, (snapped.0 as f32, snapped.1 as f32));
        if self.noise_pick {
            if self.pick_hover.as_ref().map(|(p, _)| *p) != Some(snapped) {
                self.pick_hover = Some((snapped, net_at(&self.sch, snapped)));
            }
            if let Some((_, name)) = &self.pick_hover {
                let (txt, col) = match name {
                    Some(n) => (format!("ノード {n}"), Color32::from_rgb(220, 90, 0)),
                    None => ("(接続なし)".to_string(), Color32::from_gray(130)),
                };
                painter.circle_stroke(sp, 7.0, Stroke::new(2.0, col));
                let g = painter.layout_no_wrap(txt, FontId::proportional(13.0), Color32::WHITE);
                let r = Rect::from_min_size(sp + Vec2::new(12.0, 10.0), g.size()).expand(3.0);
                painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(40, 40, 40, 220));
                painter.galley(r.min + Vec2::new(3.0, 3.0), g, col);
            }
        }

        match self.tool {
            _ if self.noise_pick => {}
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
        if !self.noise_pick {
            self.status = format!("({}, {})  部品 {}  配線 {}", snapped.0, snapped.1, self.sch.components.len(), self.sch.wires.len());
        }

        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && !self.noise_pick
            && let Some(i) = self.comp_at(g) {
                if self.multi.len() > 1 && self.multi.contains(&Sel::Comp(i)) {
                    self.start_group_drag(snapped);
                } else {
                    let c = &self.sch.components[i];
                    self.drag = Some((i, (snapped.0 - c.pos.0, snapped.1 - c.pos.1)));
                    self.selected = Some(Sel::Comp(i));
                    self.multi.clear();
                }
            }
        if let Some(gd) = &self.gdrag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let (dx, dy) = (snapped.0 - gd.anchor.0, snapped.1 - gd.anchor.1);
                let mv = |p: Pt| (p.0 + dx, p.1 + dy);
                // the selection moves as one piece: if any part would land on a part that is
                // not being moved, nothing moves this frame
                let moved: Vec<usize> = gd.comps.iter().map(|c| c.0).collect();
                let blocked = gd.comps.iter().any(|&(i, p)| {
                    let c = &self.sch.components[i];
                    self.would_overlap(c.kind, comp_extents(c), mv(p), c.rot, &moved)
                });
                if blocked {
                    self.status = OVERLAP_MSG.into();
                } else {
                    for &(i, p) in &gd.comps {
                        self.sch.components[i].pos = mv(p);
                    }
                    for &(i, a, b) in &gd.wires {
                        self.sch.wires[i].a = mv(a);
                        self.sch.wires[i].b = mv(b);
                    }
                    for &(i, p) in &gd.labels {
                        self.sch.labels[i].pos = mv(p);
                    }
                }
            } else {
                self.gdrag = None;
            }
        }
        if let Some((i, off)) = self.drag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                let np = (snapped.0 - off.0, snapped.1 - off.1);
                let c = &self.sch.components[i];
                if self.would_overlap(c.kind, comp_extents(c), np, c.rot, &[i]) {
                    self.status = OVERLAP_MSG.into();
                } else {
                    self.sch.components[i].pos = np;
                }
            } else {
                self.drag = None;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            self.gdrag = None;
        }

        if self.noise_pick {
            if resp.clicked_by(egui::PointerButton::Primary) {
                let shift = ui.input(|i| i.modifiers.shift);
                self.pick_noise_node(snapped, shift);
            }
        } else if resp.clicked_by(egui::PointerButton::Primary) {
            match self.tool {
                Tool::Select => {
                    self.multi.clear();
                    self.selected = self.pick(g);
                }
                Tool::Place => {
                    let lib = self.place_lib.and_then(|k| self.library.get(k)).cloned();
                    let (kind, ext) = match &lib {
                        Some(d) => (Kind::Custom, sym_extents(&d.symbol())),
                        None => (self.place, extents(self.place)),
                    };
                    if self.would_overlap(kind, ext, snapped, self.rot, &[]) {
                        self.status = OVERLAP_MSG.into();
                    } else {
                        match lib {
                            Some(d) => {
                                self.sch.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), snapped, self.rot);
                            }
                            None => {
                                self.sch.add(self.place, snapped, self.rot);
                            }
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
        // the panel is taller than a small window: let it scroll instead of hiding the bottom controls
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.side_panel_body(ui));
    }

    fn side_panel_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("回路図");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "選択").on_hover_text("部品や配線をクリックで選択、部品はドラッグで移動できます。選ぶと値(抵抗値など)を変えられます");
            ui.selectable_value(&mut self.tool, Tool::Wire, "配線").on_hover_text("端点をクリックして線を引きます。クリックするたびに折れ線が続き、ダブルクリックか右クリックで終わります");
            ui.selectable_value(&mut self.tool, Tool::Label, "ラベル").on_hover_text("同じ名前のラベルを付けた場所は、線でつながなくても同じ配線 (ネット) になります。図がすっきりします");
        });
        if self.tool == Tool::Label {
            ui.text_edit_singleline(&mut self.label_text);
        }
        ui.separator();
        ui.label("部品 (クリックして配置 / Rキーで回転)");
        for k in Kind::PALETTE {
            if ui.selectable_label(self.tool == Tool::Place && self.place_lib.is_none() && self.place == *k, k.label()).on_hover_text(kind_tip(*k)).clicked() {
                self.tool = Tool::Place;
                self.place_lib = None;
                self.place = *k;
                self.pending = None;
            }
        }
        ui.separator();
        let mut chosen = None;
        egui::CollapsingHeader::new(format!("ライブラリ部品 ({})", self.library.len())).default_open(true).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("検索"));
            let f = self.lib_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                // grouped by category, then by name
                let cat = |d: &PartDef| if d.category.is_empty() { "その他".to_string() } else { d.category.clone() };
                let mut order: Vec<usize> = (0..self.library.len()).collect();
                order.sort_by_key(|&i| (cat(&self.library[i]), self.library[i].name.to_lowercase()));
                let mut last = String::new();
                for i in order {
                    let d = &self.library[i];
                    let c = cat(d);
                    if !f.is_empty() && ![&d.name, &d.description, &c].iter().any(|t| t.to_lowercase().contains(&f)) {
                        continue;
                    }
                    if c != last {
                        ui.label(egui::RichText::new(&c).small().weak());
                        last = c;
                    }
                    let mark = if d.sim.is_some() { " ◎" } else { "" };
                    let selected = self.tool == Tool::Place && self.place_lib == Some(i);
                    if ui.selectable_label(selected, format!("  {}{mark}", d.name)).on_hover_text(&d.description).clicked() {
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
        if let Some(Sel::Comp(i)) = self.selected
            && i < self.sch.components.len() {
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
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) {
                    let (first, rest) = match c.value.trim().split_once(char::is_whitespace) {
                        Some((f, r)) => (f.to_string(), format!(" {}", r.trim())),
                        None => (c.value.trim().to_string(), String::new()),
                    };
                    if let Some(cur) = parse_value(&first).filter(|v| *v > 0.0) {
                        // re-centre on the typed value unless the slider itself wrote it
                        if self.slider.as_ref().is_none_or(|(k, _, t)| *k != i || *t != c.value) {
                            self.slider = Some((i, cur, c.value.clone()));
                        }
                        let base = self.slider.as_ref().map_or(cur, |s| s.1);
                        let mut x = (cur / base).log10().clamp(-2.0, 2.0);
                        let changed = ui
                            .horizontal(|ui| {
                                ui.label("スライダー");
                                ui.add(egui::Slider::new(&mut x, -2.0..=2.0).show_value(false)).on_hover_text(
                                    "ドラッグして値を動かします (中央 = 今の値、左右に 1/100 〜 100 倍)。
下の「自動で再計算」をオンにすると、動かしながら波形や電圧が変わるのを見られます",
                                ).changed()
                            })
                            .inner;
                        if changed {
                            let v = format_value(base * 10f64.powf(x));
                            c.value = format!("{v}{rest}");
                            self.slider = Some((i, base, c.value.clone()));
                        }
                    }
                }
                if c.kind == Kind::VSource {
                    // read the form again when the text changed or another source was selected
                    let stale = self.src_form.as_ref().is_none_or(|(k, t, _)| *k != i || *t != c.value);
                    if stale {
                        self.src_form = Some((i, c.value.clone(), SourceForm::parse(&c.value)));
                    }
                    if let Some((_, text, form)) = self.src_form.as_mut() {
                        ui.separator();
                        if crate::source_form::show(ui, form) {
                            match form.to_text() {
                                Ok(t) => {
                                    c.value = t.clone();
                                    *text = t;
                                }
                                Err(msg) => {
                                    ui.colored_label(Color32::from_rgb(230, 60, 60), msg);
                                }
                            }
                        } else if let Err(msg) = form.to_text() {
                            ui.colored_label(Color32::from_rgb(230, 60, 60), msg);
                        }
                    }
                    ui.collapsing("値の書き方 (テキストで直接入力する場合)", |ui| {
                        ui.small("例: 5 / SIN(0 1 1k) / PULSE(0 5 100u 1u 1u 400u 1m) / PWL(0 0 1m 5) / 5 AC 1");
                        ui.small("ノイズ: NOISE(実効値 最高周波数 [シード]) 例: 5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k) (+ の前後は空白)");
                    });
                }
                if c.kind == Kind::Capacitor {
                    ui.small("例: 100n / 100n ESR=20m ESL=5n (等価直列抵抗・インダクタンス)");
                }
                if c.kind == Kind::Inductor {
                    ui.small("例: 10u / 10u DCR=0.1 (巻線抵抗)");
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
                    self.rotate_req = Some(i);
                }
            }
        if self.selected.is_some() && ui.button("削除 (Delete)").clicked() {
            self.delete_selected();
        }
        ui.separator();
        if ui.button("ERC (電気ルールチェック)").on_hover_text("配線ミスの点検。ピンがどこにもつながっていない、部品番号がダブっている、などを探して赤紫の丸で示します。シミュレーションや基板に進む前にやっておくと安心です").clicked() {
            self.problems = erc(&self.sch);
            self.status = format!("ERC: 問題 {} 件", self.problems.len());
        }
        for p in &self.problems {
            ui.colored_label(Color32::from_rgb(230, 0, 200), &p.message);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("DC動作点を解析").on_hover_text("時間が経って落ち着いたときの各点の電圧と電流を求めます。回路図の各ピンの横に電圧が出るので、「思った電圧になっているか」の確認に一番よく使います").clicked() {
                self.run_dc();
            }
            ui.checkbox(&mut self.show_v, "電圧表示").on_hover_text("DC解析の結果を回路図に重ねて表示します。配線は電圧が低いほど青、高いほど赤に色分けされます");
            ui.checkbox(&mut self.show_i, "電流表示").on_hover_text("各部品を流れる電流を数値と矢印 (電流の向き) で表示します。「どこに電流が流れているか」の理解に役立ちます");
        });
        ui.checkbox(&mut self.live, "自動で再計算").on_hover_text(
            "回路を変えるたびに、開いている解析 (DC / 過渡 / AC) をやり直します。
部品の「スライダー」と組み合わせると、値を動かしながら結果がどう変わるかを実験できます。
重い回路では動きがもたつくのでオフにしてください",
        );
        ui.separator();
        ui.label("過渡解析");
        ui.horizontal(|ui| {
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstop).desired_width(56.0)).on_hover_text("どこまで時間を進めるか。例: 10m = 10ミリ秒。見たい波形が数周期入る長さにします");
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.tran_tstep).desired_width(56.0)).on_hover_text("計算の最大の時間刻み。波形が角ばって見えるときは小さくします (終了時間の 1/1000 くらいが目安)");
        });
        ui.horizontal(|ui| {
            if ui.button("過渡解析を実行").on_hover_text("時間とともに電圧・電流がどう変わるかを計算し、オシロスコープのように波形を表示します。充放電や発振、パルスの応答を見るのに使います").clicked() {
                self.run_tran();
            }
            ui.checkbox(&mut self.tran_uic, "ゼロ状態から").on_hover_text("コンデンサの電圧などを 0 の状態から始めます。電源を入れた直後の立ち上がりを見たいときに使います");
        });
        ui.separator();
        ui.label("AC解析 (周波数特性)");
        ui.horizontal(|ui| {
            ui.label("開始");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstart).desired_width(48.0)).on_hover_text("測り始める周波数 (Hz)。例: 10");
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.ac_fstop).desired_width(48.0)).on_hover_text("測り終わる周波数 (Hz)。例: 1Meg = 1MHz");
            ui.label("点/dec");
            ui.add(egui::TextEdit::singleline(&mut self.ac_ppd).desired_width(30.0)).on_hover_text("周波数が10倍になる間に何点測るか。多いほど滑らかですが、20 もあれば十分です");
        });
        if ui.button("AC解析を実行").on_hover_text("入力の周波数を変えたとき、出力の大きさと位相がどう変わるか (周波数特性) を求めます。フィルタやアンプの帯域を調べるボード線図が出ます").clicked() {
            self.run_ac();
        }
        ui.small("入力にする電源の値を「AC 1」にしてください");
        ui.separator();
        ui.label("DCスイープ (入力→出力の特性)");
        ui.horizontal(|ui| {
            ui.label("電源");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_src).desired_width(36.0).hint_text("V2"))
                .on_hover_text("電圧を変える電源の番号 (例 V2)。インバーターなら入力にあたる電源です。値を少しずつ変えながら、そのつど動作点を求めます");
            ui.label("開始");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_start).desired_width(34.0)).on_hover_text("スイープの開始電圧 (V)");
            ui.label("終了");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_stop).desired_width(34.0)).on_hover_text("スイープの終了電圧 (V)。ロジックなら電源電圧まで");
            ui.label("刻み");
            ui.add(egui::TextEdit::singleline(&mut self.sweep_step).desired_width(40.0)).on_hover_text("電圧を変える間隔 (V)。切り替わりをなめらかに見るには、電源電圧の 1/100 くらい");
        });
        if ui.button("DCスイープを実行").on_hover_text("入力電圧を変えながら出力を測り、伝達特性のグラフを描きます。インバーターなら VOH / VOL / 切替電圧 / ノイズマージンも自動で計算します").clicked() {
            self.run_sweep();
        }
        ui.separator();
        ui.label("ノイズ解析 (熱雑音・ショット雑音)");
        ui.horizontal(|ui| {
            ui.label("出力");
            ui.add(egui::TextEdit::singleline(&mut self.noise_out).desired_width(56.0).hint_text("OUT / A,B"))
                .on_hover_text("ノイズを測るノードの名前。「A,B」と書くと A と B の電位差を測ります。\n名前が分からないときは右の「回路図で選ぶ」を使うと、クリックするだけで入力できます");
            ui.label("入力電源");
            ui.add(egui::TextEdit::singleline(&mut self.noise_in).desired_width(36.0).hint_text("V1"))
                .on_hover_text("回路の入力にあたる電源の番号 (例 V1)。入れると、ノイズを入力側に換算した値も出ます。\n「このアンプは入力で何 µV のノイズがあるのと同じか」が分かります。空欄でも構いません");
            ui.label("℃");
            ui.add(egui::TextEdit::singleline(&mut self.noise_temp).desired_width(30.0)).on_hover_text("回路の温度 (摂氏)。熱雑音は絶対温度に比例するので、温度を上げると増えます");
        });
        ui.horizontal(|ui| {
            let label = if self.noise_pick { "選択中… (Esc で中止)" } else { "回路図で選ぶ" };
            if ui.selectable_label(self.noise_pick, label).on_hover_text("押してから回路図のピンや配線をクリックすると、そのノードが出力になり、すぐ解析します。\nShift+クリックで基準ノード (差動出力)").clicked() {
                self.noise_pick = !self.noise_pick;
                self.pick_hover = None;
                if self.noise_pick {
                    self.status = "回路図の測りたいノードをクリックしてください".into();
                }
            }
            if ui.button("ノイズ解析を実行").on_hover_text("抵抗の熱雑音や、ダイオード・トランジスタのショット雑音が出力にどれだけ現れるかを周波数ごとに計算します。").clicked() {
                self.run_noise();
            }
        });
        ui.small("周波数範囲・点数は AC解析の欄を共用。入力電源を指定すると入力換算も表示");
        ui.separator();
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
        if ui.button("SPICEネットリストを書き出し…").clicked()
            && let Some(p) = rfd::FileDialog::new().add_filter("SPICE", &["cir"]).set_file_name("circuit.cir").save_file() {
                self.status = match std::fs::write(&p, to_spice(&self.sch, "circuitCad")) {
                    Ok(()) => format!("書き出しました: {}", p.display()),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
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
        ui.small("左クリック: 配置 / 配線 (斜めは自動でL字)\n右クリック・ダブルクリック: 配線を終了\n選択ツール: 左ドラッグ = 完全に囲んだものを選択 / 右ドラッグ = 触れたものも選択 (Shiftで追加)\n中ドラッグ: 移動 (配線中などは右ドラッグも可) / ホイール: 拡大縮小");
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
                self.reset_history();
                self.path = Some(p.clone());
                self.selected = None;
                self.pending = None;
                self.problems.clear();
                self.dc = None;
                self.fit_pending = true;
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

#[cfg(test)]
mod select_tests {
    use super::*;

    /// R1 at (0,0) and R2 at (10,0), a wire between them and a label near R2.
    fn editor() -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::Resistor, (0, 0), 0);
        e.sch.add(Kind::Resistor, (10, 0), 0);
        e.sch.wires.push(Wire { a: (2, 0), b: (8, 0) });
        e.sch.labels.push(Label { pos: (10, -4), text: "X".into() });
        e
    }

    #[test]
    fn undo_and_redo_step_through_committed_edits() {
        let mut e = editor();
        e.reset_history();
        e.sch.wires.push(Wire { a: (0, 5), b: (4, 5) });
        e.commit_history();
        e.sch.labels.clear();
        e.commit_history();
        assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 0));
        e.undo();
        assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 1));
        e.undo();
        assert_eq!(e.sch.wires.len(), 1);
        e.undo(); // nothing left
        assert_eq!(e.sch.wires.len(), 1);
        e.redo();
        e.redo();
        assert_eq!((e.sch.wires.len(), e.sch.labels.len()), (2, 0));
        // a new edit discards the redo history
        e.undo();
        e.sch.wires.clear();
        e.commit_history();
        e.redo();
        assert_eq!(e.sch.wires.len(), 0);
    }

    #[test]
    fn uncommitted_edit_is_undone_in_one_step() {
        let mut e = editor();
        e.reset_history();
        e.sch.components[0].value = "47k".into();
        e.undo(); // commits the pending edit first, then reverts it
        assert_ne!(e.sch.components[0].value, "47k");
        e.redo();
        assert_eq!(e.sch.components[0].value, "47k");
    }

    #[test]
    fn live_update_reruns_the_open_analysis_after_an_edit() {
        let mut e = editor();
        e.run_dc();
        e.live = true;
        e.live_update(); // first call syncs the snapshot
        let before = e.dc.as_ref().unwrap().signature.clone();
        e.sch.components[0].value = "2.2k".into();
        assert_ne!(e.dc.as_ref().unwrap().signature, e.signature(), "stale before the live update");
        e.live_update();
        assert_eq!(e.dc.as_ref().unwrap().signature, e.signature());
        assert_ne!(e.dc.as_ref().unwrap().signature, before);
        e.live = false;
        e.sch.components[0].value = "3.3k".into();
        e.live_update();
        assert_ne!(e.dc.as_ref().unwrap().signature, e.signature(), "off: nothing is recomputed");
    }

    #[test]
    fn dc_result_colours_every_wire_and_reports_part_currents() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/led.schjson");
        let mut e = editor();
        e.sch = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        e.run_dc();
        let dc = e.dc.as_ref().unwrap();
        let res = dc.outcome.as_ref().expect("the LED example converges");
        assert_eq!(dc.wire_v.len(), e.sch.wires.len());
        assert!(dc.wire_v.iter().all(|v| v.is_some()), "every wire belongs to a net with a voltage");
        assert!(dc.v_range.1 > dc.v_range.0);
        assert!(e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).all(|c| res.element(&c.refdes).is_some()));
        // blue at the bottom of the range, red at the top
        let (lo, hi) = (volt_color(0.0, (0.0, 5.0)), volt_color(5.0, (0.0, 5.0)));
        assert!(lo.b() > lo.r() && hi.r() > hi.b());
    }

    #[test]
    fn parts_may_touch_but_not_overlap_and_ground_symbols_are_exempt() {
        let e = editor(); // R1 at (0,0), R2 at (10,0); a resistor is 4 wide and 2 tall
        let r = |pos, rot| e.would_overlap(Kind::Resistor, extents(Kind::Resistor), pos, rot, &[]);
        assert!(r((0, 0), 0), "exactly on top of R1");
        assert!(r((3, 0), 0), "half on R1");
        assert!(r((0, 1), 0), "shifted one row");
        assert!(!r((4, 0), 0), "pins meeting end to end is the normal way to join parts");
        assert!(!r((0, 2), 0), "stacked with the edges touching");
        assert!(r((4, 0), 1) == false && r((3, 0), 1) == false, "a vertical resistor beside R1 fits");
        assert!(r((1, 0), 1), "a vertical resistor across R1 does not");
        // moving a part does not collide with itself
        assert!(!e.would_overlap(Kind::Resistor, extents(Kind::Resistor), (0, 0), 0, &[0]));
        // a ground symbol sits right on R1's pin
        assert!(!e.would_overlap(Kind::Ground, extents(Kind::Ground), (2, 0), 0, &[]));
        assert!(!e.would_overlap(Kind::Ground, extents(Kind::Ground), (0, 0), 0, &[]));
    }

    #[test]
    fn rotating_into_a_neighbour_is_refused() {
        let mut e = editor(); // R1 at (0,0) fills x -2..2, y -1..1
        e.sch.add(Kind::Resistor, (0, 2), 0); // R3 right above it, edges touching
        e.rotate_comp(2); // upright it would span y 0..4 and cut into R1
        assert_eq!(e.sch.components[2].rot, 0);
        e.sch.components[2].pos = (0, 5); // clear of everything
        e.rotate_comp(2);
        assert_eq!(e.sch.components[2].rot, 1);
    }

    #[test]
    fn window_box_needs_full_enclosure_and_crossing_box_accepts_a_touch() {
        let e = editor();
        // covers R1 completely and only the left part of R2 and of the wire's far end
        let (a, b) = ((-4.0, -3.0), (9.0, 3.0));
        let window = e.objects_in_box(a, b, false);
        assert!(window.contains(&Sel::Comp(0)));
        assert!(!window.contains(&Sel::Comp(1)), "R2 is only partly inside");
        assert!(window.contains(&Sel::Wire(0)), "the wire (2..8) is fully inside");
        assert!(!window.contains(&Sel::Label(0)));
        let crossing = e.objects_in_box(a, b, true);
        assert!(crossing.contains(&Sel::Comp(0)) && crossing.contains(&Sel::Comp(1)));
        // a box that only grazes the wire in the middle
        let graze = e.objects_in_box((4.0, -1.0), (5.0, 1.0), true);
        assert_eq!(graze, vec![Sel::Wire(0)]);
        assert!(e.objects_in_box((4.0, -1.0), (5.0, 1.0), false).is_empty());
    }

    #[test]
    fn selecting_many_then_deleting_removes_exactly_those() {
        let mut e = editor();
        let items = e.objects_in_box((-4.0, -3.0), (9.0, 3.0), true);
        e.set_selection(items);
        assert!(e.selected.is_none() && e.multi.len() >= 3);
        e.delete_selected();
        assert!(e.sch.components.is_empty() && e.sch.wires.is_empty());
        assert_eq!(e.sch.labels.len(), 1, "the label was outside the box");
        // a single hit goes to the normal selection
        let mut e = editor();
        e.set_selection(vec![Sel::Label(0)]);
        assert_eq!(e.selected, Some(Sel::Label(0)));
        assert!(e.multi.is_empty());
    }
}

#[cfg(test)]
mod example_overlap_tests {
    use super::*;

    #[test]
    fn shipped_example_circuits_have_no_overlapping_parts() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut found = vec![];
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "schjson") {
                let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                let boxes: Vec<_> = sch.components.iter().map(|c| (c, sheet_box(c.kind, comp_extents(c), c.pos, c.rot))).collect();
                for (i, (a, ab)) in boxes.iter().enumerate() {
                    for (b, bb) in &boxes[i + 1..] {
                        if let (Some(x), Some(y)) = (ab, bb)
                            && boxes_overlap(*x, *y)
                        {
                            found.push(format!("{}: {} / {}", e.file_name().to_string_lossy(), a.refdes, b.refdes));
                        }
                    }
                }
            }
        }
        assert!(found.is_empty(), "overlapping parts in the examples: {found:?}");
    }
}
