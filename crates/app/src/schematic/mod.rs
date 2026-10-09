//! Schematic editor: place parts, wire them together, run ERC, export netlist.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use crate::part_form::PartForm;
use part_lib::PartDef;
use crate::plot::{self, PlotState, Style, Trace, PALETTE};
use sim_core::{
    ac_analysis, dc_sweep, describe_source, inverter_metrics, noise_analysis, DcSweepResult, DcSweepSettings, DcSweepSignal, NoiseKind, NoiseResult, NoiseSettings, dc_op, from_schematic, format_value, monte_carlo, temp_sweep, Circuit, step_metrics, McMeasure, McResult, McSettings, TranMetric, T_NOM_C, parse_value, spectrum as fft_spectrum, stats as signal_stats, transient, AcResult, AcSettings, AcSweep, DcResult, Spectrum,
    TranResult, TranSettings,
};
use std::collections::BTreeSet;
use crate::source_form::SourceForm;
use sch_core::{erc, extract, net_at, on_segment, to_spice, Component, CustomSymbol, Kind, Label, PinSide, Problem, Pt, Schematic, Severity, Wire};
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
    /// The ERC finding whose explanation is open.
    erc_focus: Option<usize>,
    /// An ERC check has been run since the last change of the list.
    erc_ran: bool,
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
    /// Circuit temperature (℃) used by the DC / transient / AC / sweep analyses.
    temp_c: String,
    temp_from: String,
    temp_to: String,
    temp_step: String,
    mc: Option<McView>,
    mc_out: String,
    mc_runs: String,
    mc_tol_r: String,
    mc_tol_c: String,
    mc_tol_l: String,
    mc_seed: u64,
    /// What the Monte Carlo run measures.
    mc_kind: McKind,
    mc_metric: TranMetric,
    /// The running lesson and the circuit it started from.
    lesson: Option<ActiveLesson>,
    lesson_base: Schematic,
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
    Mc,
}

/// A finished DC sweep: transfer curves with the signals to draw.
struct SweepView {
    outcome: Result<DcSweepResult, Vec<String>>,
    selected: BTreeSet<String>,
    /// Draw the line output = input (its crossing with the curve is the switching threshold).
    show_diag: bool,
    plot: PlotState,
    /// The x axis is the circuit temperature (℃) instead of a source voltage.
    temp: bool,
}

/// Quantity a Monte Carlo run measures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum McKind {
    /// DC voltage of a node.
    Dc,
    /// -3 dB frequency (AC).
    Ac,
    /// Step-response figure (transient).
    Tran,
}

/// A finished Monte Carlo run.
struct McView {
    outcome: Result<McResult, Vec<String>>,
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
    /// The schematic this result was computed from.
    signature: String,
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
    /// The schematic this result was computed from.
    signature: String,
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
            temp_c: "27".into(),
            temp_from: "-20".into(),
            temp_to: "80".into(),
            temp_step: "5".into(),
            mc: None,
            mc_out: String::new(),
            mc_runs: "300".into(),
            mc_tol_r: "5".into(),
            mc_tol_c: "10".into(),
            mc_tol_l: "10".into(),
            mc_seed: 1,
            mc_kind: McKind::Dc,
            mc_metric: TranMetric::RiseTime,
            lesson: None,
            lesson_base: Schematic::default(),
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
            erc_focus: None,
            erc_ran: false,
            status: "回路図: 部品を選んで配置し、配線ツールで線をつなぎます".into(),
        }
    }
}


const SCH_BG: Color32 = Color32::from_rgb(250, 248, 235);
const SYM: Color32 = Color32::from_rgb(120, 20, 20);
const WIRE: Color32 = Color32::from_rgb(0, 110, 30);

// The editor is one type spread over several files; the child modules see its private fields.
use lessons::ActiveLesson;
pub use lessons::lesson_titles;
use symbols::{boxes_overlap, comp_extents, extents, rot_f, sheet_box, sym_extents, OVERLAP_MSG};
mod symbols;
mod wave;
mod ac;
mod analysis_panel;
mod sweep;
mod lessons;
mod noise;
mod variation;
mod tran;
mod dc;
mod library;
mod edit;
mod history;
mod hint;
mod canvas;
mod draw;
mod interact;
mod panel;
mod file;
#[cfg(test)]
mod select_tests;
#[cfg(test)]
mod variation_tests;
#[cfg(test)]
mod lessons_tests;
#[cfg(test)]
mod example_overlap_tests;
