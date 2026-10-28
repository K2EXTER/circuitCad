//! Schematic editor: place parts, wire them together, run ERC, export netlist.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use crate::part_form::PartForm;
use part_lib::PartDef;
use crate::plot::{self, PlotState, Style, Trace, PALETTE};
use sim_core::{diagnose, stability_margins, minus_3db, wave_measure, 
    ac_analysis, dc_sweep, describe_source, inverter_metrics, noise_analysis, DcSweepResult, DcSweepSettings, DcSweepSignal, NoiseKind, NoiseResult, NoiseSettings, dc_op, from_schematic, format_value, monte_carlo, temp_sweep, Circuit, step_metrics, settling_times, McMeasure, McResult, McSettings, TranMetric, T_NOM_C, parse_value, spectrum as fft_spectrum, stats as signal_stats, transient, AcResult, AcSettings, AcSweep, DcResult, Spectrum,
    TranResult, TranSettings,
};
use std::collections::BTreeSet;
use crate::source_form::SourceForm;
use sch_core::{erc, extract, net_at, on_segment, to_spice, Component, CustomSymbol, Kind, Label, PinSide, Problem, Pt, Schematic, Severity, Wire};
use std::path::{Path, PathBuf};

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
    /// Counts every committed edit and every replaced sheet; `saved_rev` is its value when the sheet was last saved or opened.
    pub rev: u64,
    saved_rev: u64,
    /// Value slider: (component, the value the slider is centred on, the text it last wrote).
    slider: Option<(usize, f64, String)>,
    /// How far the value slider reaches from the centre, in decades (0.3 = 1/2..2x, 1 = 1/10..10x, 2 = 1/100..100x).
    slider_span: f64,
    /// Re-run the open analysis whenever the circuit changes.
    live: bool,
    live_snap: Schematic,
    pub path: Option<PathBuf>,
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
    /// What Ctrl+C / Ctrl+X copied (kept when another circuit is opened, so it can be pasted there).
    clipboard: sch_core::Fragment,
    /// A rotation asked for by the property panel, applied once the panel's borrow has ended.
    rotate_req: Option<usize>,
    /// The selection was put away when the Place tool was chosen (reset when another tool is chosen).
    place_cleared: bool,
    drag: Option<(usize, Pt)>,
    /// Wires and pin places from the start of the part drag in progress (wires follow the moved parts).
    stretch: Option<stretch::StretchBase>,
    zoom: f32, // pixels per grid step
    pan: Vec2, // grid position at the screen centre
    problems: Vec<Problem>,
    /// The ERC finding whose explanation is open.
    erc_focus: Option<usize>,
    /// An ERC check has been run since the last change of the list.
    erc_ran: bool,
    pub status: String,
    /// Set by the small "操作" buttons; the app opens the guide on that page.
    pub help_request: Option<crate::help::HelpTab>,
    pub library: Vec<PartDef>,
    lib_errors: Vec<String>,
    lib_dir: PathBuf,
    lib_filter: String,
    /// Library category shown (empty = all).
    lib_cat: String,
    /// Only parts that have a simulation model.
    lib_sim_only: bool,
    /// Favorite and recently placed library parts (saved next to the library).
    lib_prefs: library::LibPrefs,
    /// The part list takes more height.
    lib_tall: bool,
    /// The quick-add box (key A), when open.
    quick: Option<quick::QuickAdd>,
    place_lib: Option<usize>,
    form: Option<PartForm>,
    /// The "import from KiCad" window (symbol list and optional footprint).
    kicad: Option<kicad_import::KicadImport>,
    dc: Option<DcView>,
    show_v: bool,
    show_i: bool,
    sig_cache: String,
    /// The DC currents of the nets, with the signature of the circuit they were worked out for: the board's net window asks for
    /// them each time it opens, and the DC solve of a big circuit takes seconds.
    dc_currents_cache: std::cell::RefCell<Option<(String, Result<Vec<(String, f64)>, String>)>>,
    /// `rev` when the cache above was filled (so the board can ask cheaply whether the answer is at hand).
    dc_currents_rev: std::cell::Cell<Option<u64>>,
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
    /// Copper resistance of the routed board in series with pads (post-layout simulation); empty = ideal wires.
    pub wiring: Vec<sim_core::WiringR>,
    /// Magnetic coupling between the wiring inductors (with `wiring`).
    pub couplings: Vec<sim_core::WiringK>,
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
    panel_tab: PanelTab,
    mc_metric: TranMetric,
    /// The running lesson and the circuit it started from.
    lesson: Option<ActiveLesson>,
    /// The "作ってみよう" challenge being worked on.
    challenge: Option<challenge::ActiveChallenge>,
    /// A left drag with Alt held pans the view (until the button is released).
    alt_pan: bool,
    /// Floating settings window opened by right-clicking a part: (component index, where, serial for a fresh window position).
    /// Solver accuracy settings and whether their window is open.
    sim_opts: sim_core::SimOptions,
    pub show_sim_opts: bool,
    part_popup: Option<(usize, egui::Pos2, u32)>,
    popup_serial: u32,
    lesson_base: Schematic,
    /// "値の決め方" window: open flag, which calculator, its input texts.
    wiz_open: bool,
    /// The window of the design's requirements (`requirements.rs`), the text boxes of its table, and the sentences about names the circuit lacks.
    req_open: bool,
    req_buf: requirements::Buffers,
    req_problems: Vec<String>,
    wiz_kind: usize,
    wiz_in: Vec<String>,
    /// Analysis sections of the "解析" tab: a generation counter (bumped by the buttons to reset them) and how they start: 0 = the basics open, 1 = all open, 2 = all closed.
    sec_gen: u32,
    sec_mode: u8,
    fault_part: String,
    /// Pin to break ("" = break the whole part), how (0 open, 1 to ground, 2 bridge to `fault_bridge`).
    fault_pin: String,
    fault_pin_how: usize,
    fault_bridge: String,
    /// Compare in the time domain or AC, or at DC, and the node watched in the time domain.
    fault_domain: fault::FaultDomain,
    fault_node: String,
    fault_wave: Option<fault::FaultWaveView>,
    /// Targets for the design (pass / fail table); kept for the session.
    specs: Vec<spec_table::SpecRow>,
    /// The spec table as last saved or opened (a changed table counts as an unsaved change).
    saved_specs: Vec<spec_table::SpecItem>,
    fault_kind: usize,
    fault_view: Option<fault::FaultView>,
    fault_scan: Vec<fault::ScanRow>,
    /// The scan of every part also tries the faults that hardly ever happen.
    scan_rare: bool,
    /// The "break every part" scan while it runs on its own thread.
    scan_run: Option<fault::ScanRun>,
    fault_chain: Option<Result<sim_core::ChainReport, Vec<String>>>,
    sweep_src: String,
    param: Option<param::ParamView>,
    param_part: String,
    param_from: String,
    param_to: String,
    param_n: String,
    param_node: String,
    param_log: bool,
    param_ac: bool,
    sweep_start: String,
    sweep_stop: String,
    sweep_step: String,
    noise_pick: bool,
    /// The node picker is choosing a waveform to add to the viewer (not the noise output).
    pick_probe: bool,
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
    Param,
    Fault,
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

/// The two pages of the left panel: drawing (tools, parts, settings of the selected part, ERC)
/// and analysis (running simulations and reading the numbers).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelTab {
    Draw,
    Analyze,
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

/// A kept copy of earlier waveforms, drawn dimmed behind the new ones so a change can be compared with "before".
#[derive(Clone)]
struct Ghost {
    x: Vec<f64>,
    sigs: Vec<GhostSig>,
}

#[derive(Clone)]
struct GhostSig {
    /// Signal name in the result (matches the live trace it is drawn behind).
    base: String,
    /// Name shown in the readout.
    name: String,
    /// Plot group (0 = voltage / magnitude, 1 = current / phase).
    group: usize,
    v: Vec<f64>,
}

/// A finished AC sweep (Bode plot).
struct AcView {
    outcome: Result<AcResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
    ghost: Option<Ghost>,
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
    ghost: Option<Ghost>,
    /// Signals picked in the power box: input V, input I, output V, output I.
    power: [String; 4],
    /// Ripple current and life of the electrolytic capacitors that give data sheet figures.
    parts: Vec<sim_core::RippleReport>,
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
    /// Set when this is a what-if result with a broken part ("R1 を断線"), not the real circuit.
    fault: Option<String>,
    /// Parts running past what their package / type usually allows.
    ratings: Vec<sim_core::RatingIssue>,
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
        let lib_prefs = library::LibPrefs::load(&lib_dir);
        Self {
            library,
            lib_prefs,
            lib_tall: false,
            quick: None,
            lib_errors,
            lib_dir,
            lib_filter: String::new(),
            lib_cat: String::new(),
            lib_sim_only: false,
            place_lib: None,
            form: None,
            kicad: None,
            dc: None,
            show_i: true,
            show_v: true,
            sig_cache: String::new(),
            dc_currents_cache: std::cell::RefCell::new(None),
            dc_currents_rev: std::cell::Cell::new(None),
            tran: None,
            tran_tstop: "10m".into(),
            tran_tstep: String::new(),
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
            wiring: vec![],
            couplings: vec![],
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
            panel_tab: PanelTab::Draw,
            mc_metric: TranMetric::RiseTime,
            lesson: None,
            challenge: None,
            alt_pan: false,
            part_popup: None,
            sim_opts: sim_core::SimOptions::default(),
            show_sim_opts: false,
            popup_serial: 0,
            lesson_base: Schematic::default(),
            wiz_open: false,
            req_open: false,
            req_buf: Default::default(),
            req_problems: vec![],
            wiz_kind: 0,
            wiz_in: vec![],
            sec_gen: 0,
            sec_mode: 0,
            fault_part: String::new(),
            fault_pin: String::new(),
            fault_pin_how: 0,
            fault_bridge: String::new(),
            fault_domain: fault::FaultDomain::Dc,
            fault_node: String::new(),
            fault_wave: None,
            specs: vec![],
            saved_specs: vec![],
            fault_kind: 0,
            fault_view: None,
            fault_scan: vec![],
            scan_rare: false,
            scan_run: None,
            fault_chain: None,
            sweep_src: String::new(),
            param: None,
            param_part: String::new(),
            param_from: "1k".into(),
            param_to: "100k".into(),
            param_n: "5".into(),
            param_node: String::new(),
            param_log: true,
            param_ac: true,
            sweep_start: "0".into(),
            sweep_stop: "5".into(),
            sweep_step: "0.05".into(),
            noise_pick: false,
            pick_probe: false,
            pick_hover: None,
            noise_marks: vec![],
            wave_mode: WaveMode::Tran,
            fit_pending: false,
            sch: Schematic::default(),
            undo: vec![],
            redo: vec![],
            committed: Schematic::default(),
            rev: 0,
            saved_rev: 0,
            slider: None,
            slider_span: 1.0,
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
            clipboard: Default::default(),
            rotate_req: None,
            place_cleared: false,
            drag: None,
            stretch: None,
            zoom: 22.0,
            pan: Vec2::new(10.0, 6.0),
            problems: vec![],
            erc_focus: None,
            erc_ran: false,
            status: "回路図: 部品を選んで配置し、配線ツールで線をつなぎます".into(),
            help_request: None,
        }
    }
}


const SCH_BG: Color32 = Color32::from_rgb(250, 248, 235);
const SYM: Color32 = Color32::from_rgb(120, 20, 20);
const WIRE: Color32 = Color32::from_rgb(0, 110, 30);

// The editor is one type spread over several files; the child modules see its private fields.
use lessons::ActiveLesson;
mod quick;
#[cfg(test)]
pub use challenge::challenge_exists;
pub use spec_table::{take_specs, with_specs, SpecItem};
pub use challenge::{challenge_of_lesson, progress_key as challenge_progress_key};
pub use lessons::{lesson_ids, lesson_titles};
use symbols::{boxes_overlap, comp_extents, extents, rot_f, sheet_box, sym_extents, OVERLAP_MSG};
mod symbols;
mod glyphs;
pub(crate) use glyphs::{symbol_lines, symbol_pin_labels};
mod wave;
mod ac;
mod analysis_panel;
mod sweep;
mod challenge;
mod lessons;
mod noise;
mod variation;
mod tran;
mod dc;
mod library;
mod kicad_import;
mod edit;
mod clipboard;
mod stretch;
mod erc_skip;
mod net_hints;
mod snippet;
mod sheets_ui;
mod history;
mod hint;
mod canvas;
mod draw;
mod interact;
mod panel;
mod fault;
mod spec_table;
mod power_ui;
mod requirements;
mod wizard;
mod param;
mod settings;
mod file;
#[cfg(test)]
mod select_tests;
#[cfg(test)]
mod variation_tests;
#[cfg(test)]
mod lessons_tests;
#[cfg(test)]
mod example_overlap_tests;

#[cfg(test)]
mod example_sim_tests;
#[cfg(test)]
mod sensor_example_tests;
