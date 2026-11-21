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
    /// Bus lines (see `sch-core/bus.rs`).
    Bus,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sel {
    Comp(usize),
    Wire(usize),
    Label(usize),
    Bus(usize),
    /// A port of the sheet (the entry of a child sheet).
    Port(usize),
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
    ports: Vec<(usize, Pt)>,
    buses: Vec<(usize, Pt, Pt)>,
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
    /// Re-run the open analysis whenever the circuit changes.
    live: bool,
    live_snap: Schematic,
    /// Keep the curves a re-run replaces as the faint "before" curves.
    live_ghost: bool,
    /// How long the last automatic re-run took (s), when it ended, and whether a change is waiting for the next one.
    live_cost: f64,
    live_last: Option<std::time::Instant>,
    live_waiting: bool,
    pub path: Option<PathBuf>,
    tool: Tool,
    place: Kind,
    rot: u8,
    /// The part the placing rotation was set for: choosing another part starts it upright again (a resistor turned with R left the
    /// next GND symbol on its side, and a part put down turned the wrong way joins the wrong pins).
    rot_for: Option<(Kind, Option<usize>)>,
    label_text: String,
    /// The label tool puts a port of the sheet (a pin of the sheet symbol) of this direction, not a net label.
    label_port: Option<sch_core::PortDir>,
    /// The sheets entered through a sheet symbol, outermost first: "back to the parent" goes up one.
    sheet_trail: Vec<usize>,
    /// Count the number at the end of the label text up after each label placed (D0, D1, ...).
    label_step: bool,
    /// With a bus label as the text (D[0..7]): the member the next label stands for, and the text it was counted for.
    label_k: usize,
    label_k_text: String,
    pending: Option<Pt>,
    selected: Option<Sel>,
    /// What was picked in the board or the 3D view (`crate::link`), shown on the sheet; and the wires of its net, with the key they were found for.
    pub linked: crate::link::Focus,
    linked_cache: std::cell::RefCell<((String, u64, usize), Vec<usize>)>,
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
    /// Keeps a message on the status line for a while: the pointer position written over it every frame hid it at once
    /// (a click that could not place a part said why for one frame only).
    status_hold: canvas::StatusHold,
    /// Set by the small "操作" buttons; the app opens the guide on that page.
    pub help_request: Option<crate::help::HelpTab>,
    /// Shared, not copied (every new editor and every test editor had a copy of the ~15 000 parts); replaced as a whole on reload.
    pub library: std::sync::Arc<Vec<PartDef>>,
    /// Which of the two part lists in the panel is open: only one at a time, so the open one gets the height.
    basic_open: bool,
    /// Whether the selected part's settings band in the panel is open.
    props_open: bool,
    /// The same band in the analysis tab (folded at first, so that it does not push the analyses down).
    props_open_analyze: bool,
    /// Counts the times the library was read again (the board compares its parts with the library when this changes).
    pub lib_rev: u64,
    /// The quick-add box (key A), when open.
    quick: Option<quick::QuickAdd>,
    place_lib: Option<usize>,
    /// The value typed in the library card when the chosen part belongs to a series (`part_lib::series`): "4.7k".
    series_value: String,
    form: Option<PartForm>,
    /// The "import from KiCad" window (symbol list and optional footprint).
    kicad: Option<kicad_import::KicadImport>,
    dc: Option<DcView>,
    show_v: bool,
    show_i: bool,
    /// How the wires are coloured by voltage (the legend's switches) and the menu of a right-clicked net.
    vscale: VoltScale,
    net_menu: Option<(String, Pos2)>,
    /// The analysis run last, which F5 and the band over an out-of-date result run again.
    last_run: LastRun,
    /// The analyses pinned (★) to the top of the analysis tab (kept in `settings.json`), and whether that changed this frame.
    pub analysis_pins: Vec<String>,
    pub analysis_pins_changed: bool,
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
    z: Option<impedance::ZView>,
    z_node: String,
    z_ref: String,
    z_remove: String,
    /// Waiting for a click on the canvas to choose the net of the impedance analysis (shares `noise_pick` for the canvas).
    z_pick: bool,
    /// The poles-and-zeros box: output net, input net, waiting for a click (shares `noise_pick`), and the last result for the list.
    pz_out: String,
    pz_in: String,
    pz_pick: bool,
    pz: Option<polezero::PzSummary>,
    /// The loop gain box: the part to cut, the net on its output side, and waiting for a click to choose them (shares `noise_pick`).
    loop_part: String,
    /// How a loop gain result is drawn (Bode / Nyquist / Nichols), the squeeze of the Nyquist chart and its pan and zoom.
    loop_chart: loopgain::LoopChart,
    loop_squeeze: bool,
    loop_xy: crate::plot::xy::XyView,
    /// The root locus: the last result and the number of values (the part and its range are the parameter sweep's).
    locus: Option<rootlocus::LocusView>,
    locus_n: String,
    psrr_source: String,
    psrr_out: String,
    /// CMRR: the sources on the + and − inputs, the output net and the resistor tolerance (%) of the spread.
    cmrr_plus: String,
    cmrr_minus: String,
    cmrr_out: String,
    cmrr_tol: String,
    loop_from: String,
    loop_pick: bool,
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
    /// The board copper joined to each part's pins (with `wiring`), by reference: (conductance W/K beside the part's θ,
    /// mm² beyond its pads). `pcb_core::heat_copper`.
    pub heat_copper: std::collections::BTreeMap<String, (f64, f64)>,
    /// The board carried over for the heat runs: its temperature grid replaces the θ of the parts that give off heat (`board_heat.rs`).
    pub(crate) board_heat: Option<board_heat::BoardHeat>,
    /// The parts the last heat run took from the board's grid.
    pub(crate) board_heat_parts: Vec<String>,
    /// The analysis tab's "外す": take the board's wiring and heat away (the App does it).
    pub(crate) post_layout_request: Option<bool>,
    temp_from: String,
    temp_to: String,
    temp_step: String,
    mc: Option<McView>,
    sens: Option<sensitivity::SensView>,
    panel_tab: PanelTab,
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
    /// The fault that the repair lesson put into its circuit (an index of its list).
    debug_fault: Option<u64>,
    /// Why the lesson step on hand is not ticked (written by the step's condition each time it is checked).
    lesson_unmet: std::cell::RefCell<Option<lessons::Unmet>>,
    /// A word of the lesson window that the learner clicked: the glossary opens on it (taken by the app).
    pub glossary_request: Option<&'static str>,
    /// "値の決め方" window: open flag, which calculator, its input texts.
    wiz_open: bool,
    /// The window of the title block (`title_block.rs`).
    title_open: bool,
    /// What an empty title block's "作者" gets when its window opens (the settings' 図枠の作者).
    pub default_author: String,
    /// Wires that cross without joining are drawn with a half-circle hop (the settings' 交差を飛び越しで描く).
    pub wire_hops: bool,
    wiz_kind: usize,
    wiz_in: Vec<String>,
    /// Analysis sections of the "解析" tab: a generation counter (bumped by the buttons to reset them) and how they start: 0 = the basics open, 1 = all open, 2 = all closed.
    sec_gen: u32,
    /// The one analysis section opened by name (command line); None = the usual open / closed.
    focus_section: Option<&'static str>,
    /// The breadboard view as the person left it (saved with the sheet); None = laid out by itself (`app/src/breadboard/`).
    pub(crate) breadboard: Option<sch_core::breadboard::Layout>,
    /// The stripboard / perfboard layout changed by hand (saved with the sheet); None = laid out by itself (`breadboard/strip_view.rs`).
    pub(crate) stripboard: Option<sch_core::stripboard::StripLayout>,
    /// The stripboard / perfboard's size chosen, width × height in mm (saved with the sheet); None = as big as the layout takes.
    pub(crate) strip_size: Option<(f32, f32)>,
    /// A perfboard's 0805 / 1206 chips go on the copper side (`StripOptions::chips_back`).
    pub(crate) strip_chips_back: bool,
    /// The Arduino sketches of the microcontroller boards (refdes → text), saved with the sheet (`tolerance.rs`; B6 ⑤).
    pub(crate) sketches: std::collections::BTreeMap<String, String>,
    /// The roles of the part picked, as the bubble shows them, and the part whose bubble was closed (`roles_ui.rs`, B11).
    roles_cache: roles_ui::RolesCache,
    roles_hidden: Option<String>,
    /// The transient viewer's table to open on (the command line's `--tran-tab`).
    pub(crate) tran_tab_request: Option<usize>,
    /// What the simulation leaves out (`sim_core::model_coverage`), kept for the schematic revision it was made for.
    coverage_cache: Option<(u64, std::rc::Rc<sim_core::ModelCoverage>)>,
    /// The text typed in the analysis tab's search box (empty: every analysis).
    analysis_filter: String,
    /// Windows with the long result lists (results table, net list)
    tables: result_tables::State,
    /// The placed parts older than the library and the window to update them (`lib_sync.rs`).
    lib_sync: lib_sync::LibSync,
    /// The inputs, reference and gain chosen for each ADC in "ADC の読み" (`adc_ui.rs`).
    adc_setups: adc_ui::AdcSetups,
    /// The window of what the ADCs measure (`adc_measure_ui.rs`).
    adc_measure_ui: adc_measure_ui::AdcMeasureUi,
    /// The window of a meter's log over time (`time_log.rs`).
    time_log_ui: time_log::TimeLogUi,
    /// Targets for the design (pass / fail table); kept for the session.
    specs: Vec<spec_table::SpecRow>,
    /// Worst cases computed for the spec table, by what they were computed from (circuit, tolerances, node): at most a few.
    worst_spec_cache: Vec<(String, Result<sim_core::WorstCase, String>)>,
    /// The spec table as last saved or opened (a changed table counts as an unsaved change).
    saved_specs: Vec<spec_table::SpecItem>,
    /// Bench recordings drawn over the transient / AC plots (saved with the sheet).
    bench: bench_data::BenchData,
    saved_bench: bench_data::BenchData,
    /// The tolerance boxes as last saved or opened (they are saved with the sheet).
    saved_tol: tolerance::SheetSettings,
    /// The "実測を重ねる" box: the pasted text and its settings.
    bench_ui: bench_data::BenchUi,
    /// "症状から原因を探す": the readings and the solved faults.
    diag: troubleshoot::DiagState,
    /// Repair practice with the virtual meter (the "修理の道" lessons): the hidden fault and the readings.
    repair: Option<repair_practice::RepairPractice>,
    /// "デジタル回路を動かしながら見る": the learning mode over the canvas while it is on (`live_logic.rs`).
    live_logic: Option<live_logic::LiveLogic>,
    sweep_src: String,
    param: Option<param::ParamView>,
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
    /// A running lesson keeps the circuit fitted left of its window while the canvas changes size (the left panel gets wider on the analysis page): the canvas size, zoom and pan of the last fit. Cleared when the learner moves the view.
    lesson_fit: Option<(Vec2, f32, Vec2)>,
    /// The inputs and results of the fault section ("壊れたら"). (`fault.rs`)
    fault_ui: fault::FaultUi,
    /// The inputs of the variation (Monte Carlo) and worst-case runs. (`variation.rs`)
    mc_in: variation::McInputs,
    /// The window of the design's requirements and what it suggests. (`requirements.rs`)
    pub(crate) req: requirements::ReqUi,
    /// The parts library as read from its folder, and the state of the library list. (`library.rs`)
    lib: library::LibUi,
    /// The inputs of the parameter sweep (also the part and range of the root locus). (`param.rs`)
    param_in: param::ParamInputs,
    /// The heat sections: self-heating, temperature over time and the sweep of the surrounding temperature. (`selfheat.rs`)
    pub(crate) heat: selfheat::HeatUi,
    /// "電池の持ち時間": the stretch to average and the condition to keep working, as typed (saved with the sheet), and the last answer. (`battery.rs`)
    bat: battery::BatteryInputs,
    /// "電源の予算表": the rails of the last run, as a tree. (`budget.rs`)
    budget: Option<Result<Vec<sim_core::budget::Rail>, String>>,
    /// "突入電流": the boxes and the last answer. (`inrush.rs`)
    inrush: inrush::InrushUi,
}

#[derive(Clone, Copy, PartialEq)]
enum WaveMode {
    Tran,
    Ac,
    Noise,
    Impedance,
    Sweep,
    Mc,
    Sens,
    Param,
    Fault,
    Locus,
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
    /// The worst case of the same circuit and tolerances (every part at an end of its tolerance).
    worst: Option<sim_core::WorstCase>,
}

/// A finished noise analysis: output density plot plus the per-element breakdown.
struct NoiseView {
    outcome: Result<NoiseResult, Vec<String>>,
    selected: BTreeSet<String>,
    plot: PlotState,
    /// The circuit the result is for (the lessons check that it is not old).
    signature: String,
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
    /// The curve is a loop gain (`run_loop`), not an AC sweep: a recompute redoes that.
    loop_run: bool,
    /// The curves are a transfer function and its fit (`run_pz`): a recompute redoes that.
    pz_run: bool,
    /// The curves answer a ripple on one supply (`run_psrr`): a recompute redoes that.
    psrr_run: bool,
    /// The curves are a common-mode rejection (`run_cmrr`): a recompute redoes that.
    cmrr_run: bool,
    /// Figures shown above the plot (the rejection of a `run_psrr` / `run_cmrr`).
    notes: Vec<String>,
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
    /// Base frequency typed for the distortion box (empty = found from the waveform).
    thd_f0: String,
    ghost: Option<Ghost>,
    /// Signals picked in the power box: input V, input I, output V, output I.
    power: [String; 4],
    /// Ripple current and life of the electrolytic capacitors that give data sheet figures.
    parts: Vec<sim_core::RippleReport>,
    /// The schematic this result was computed from.
    signature: String,
    /// The digital view (logic lanes and buses, `logic_plot`) instead of the analog curves; the supply its thresholds are taken
    /// from (empty = the highest voltage shown) and how a bus value is written.
    logic: bool,
    logic_vcc: String,
    logic_radix: crate::logic_plot::Radix,
}

/// A finished DC analysis, remembered with the schematic it was computed from.
struct DcView {
    outcome: Result<DcResult, Vec<String>>,
    signature: String,
    /// Voltage at every pin position (the sheet shows `marks`; this is read by the tests' `pin_voltage`).
    #[cfg_attr(not(test), allow(dead_code))]
    pin_v: Vec<(Pt, f64)>,
    /// The voltages written on the sheet: one per net, ground left out, none on a sheet with no voltage at all (`dc.rs voltage_marks`).
    marks: Vec<(Pt, f64)>,
    /// Voltage of each wire's net (same order as `Schematic::wires`).
    wire_v: Vec<Option<f64>>,
    /// The two ends of each wire when the result was made, to find a wire's voltage after the wires change.
    wire_ends: Vec<(Pt, Pt)>,
    /// Range of the wire voltages (not the nodes inside the parts' models); the colours take theirs from the wires shown
    /// (`vscale.rs`), the tests read this.
    #[cfg_attr(not(test), allow(dead_code))]
    v_range: (f64, f64),
    /// Set when this is a what-if result with a broken part ("R1 を断線"), not the real circuit.
    fault: Option<String>,
    /// Parts running past what their package / type usually allows.
    ratings: Vec<sim_core::RatingIssue>,
}

/// The analyses F5 can run again (the ones that need nothing picked first).
#[derive(Clone, Copy, PartialEq, Debug)]
enum LastRun {
    Dc,
    Tran,
    Ac,
    Noise,
    Sweep,
    Impedance,
    Loop,
    PoleZero,
    Psrr,
    Cmrr,
    Param,
    Locus,
}

impl SchEditor {
    /// Runs the analysis run last again (F5, and the band over an out-of-date result).
    pub fn rerun_last(&mut self) {
        match self.last_run {
            LastRun::Dc => self.analyze_dc(),
            LastRun::Tran => self.analyze_tran(),
            LastRun::Ac => self.analyze_ac(),
            LastRun::Noise => self.analyze_noise(),
            LastRun::Sweep => self.analyze_sweep(),
            LastRun::Impedance => self.analyze_impedance(),
            LastRun::Loop => self.analyze_loop(),
            LastRun::PoleZero => self.analyze_pz(),
            LastRun::Psrr => self.analyze_psrr(),
            LastRun::Cmrr => self.analyze_cmrr(),
            LastRun::Param => self.analyze_param(),
            LastRun::Locus => self.analyze_locus(),
        }
    }

    /// The name of the analysis F5 runs, for the palette and the hover.
    pub fn last_run_name(&self) -> &'static str {
        match self.last_run {
            LastRun::Dc => "DC動作点",
            LastRun::Tran => "過渡解析",
            LastRun::Ac => "AC解析",
            LastRun::Noise => "ノイズ解析",
            LastRun::Sweep => "DCスイープ",
            LastRun::Impedance => "インピーダンス",
            LastRun::Loop => "ループゲイン",
            LastRun::PoleZero => "極と零",
            LastRun::Psrr => "電源除去比",
            LastRun::Cmrr => "同相除去比",
            LastRun::Param => "パラメータスイープ",
            LastRun::Locus => "根軌跡",
        }
    }
}

/// How the wires are coloured by their voltage: the two switches of the legend in the canvas corner.
#[derive(Clone, Default, PartialEq, Debug)]
pub(crate) struct VoltScale {
    /// Steps of ten (1 mV, 10 mV, 100 mV, 1 V, 10 V) instead of steps in proportion to the largest voltage shown.
    pub decades: bool,
    /// The net the voltages are measured from (like moving a scope's ground clip); `None` = ground.
    pub reference: Option<String>,
}

/// Below this (in proportion: a hundredth of the range; in decades: 1 mV) a wire counts as 0 V and is gray.
const DECADE_FLOOR: f64 = 1e-3;

/// The step of a voltage, already taken from the reference: 0 = about 0 V, 1..=5 above, -1..=-5 below. In proportion the
/// steps split the largest voltage shown on each side (`lo` ≤ 0 ≤ `hi`); in decades a step is a power of ten from 1 mV.
fn volt_step(v: f64, (lo, hi): (f64, f64), decades: bool) -> i32 {
    if decades {
        let a = v.abs();
        if a < DECADE_FLOOR {
            return 0;
        }
        let k = ((a / DECADE_FLOOR).log10().floor() as i32 + 1).clamp(1, 5);
        return if v > 0.0 { k } else { -k };
    }
    let span = hi.max(-lo);
    if span < 1e-9 || v.abs() < span * 0.01 {
        return 0;
    }
    let side = if v > 0.0 { hi } else { -lo };
    let k = ((v.abs() / side.max(1e-12)) * 5.0).ceil().clamp(1.0, 5.0) as i32;
    if v > 0.0 { k } else { -k }
}

fn step_colour(step: i32) -> Color32 {
    let p = crate::canvas_theme::palette();
    match step {
        0 => p.zero,
        s if s > 0 => p.above[(s - 1) as usize],
        s => p.below[(-s - 1) as usize],
    }
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
    // four significant digits, so a column of values reads the same width (12.00 V, 1.948 V, 330.0 mV)
    if a == 0.0 {
        return format!("0 {unit}");
    }
    let s = v * scale;
    let decimals = match s.abs() {
        x if x >= 100.0 => 1,
        x if x >= 10.0 => 2,
        _ => 3,
    };
    format!("{s:.decimals$} {prefix}{unit}")
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
        Kind::Pot => "ボリューム (可変抵抗): つまみで抵抗の比が変わります。両端を電源と GND に、真ん中の足から好きな電圧を取り出せます (分圧)",
        Kind::ISource => "電流源: 決めた電流を流します (シミュレーション専用で、基板には載りません)。負荷の急変の試験や、バイアス電流・光で流れる電流の代わりに使います。矢印の向きに流れ出します",
        Kind::Npn => "NPN トランジスタ: ベースに小さい電流を流すと、コレクタ→エミッタに大きな電流が流れます",
        Kind::Ground => "GND: 電圧の基準 (0 V)。回路には必ず 1 つ以上必要です",
        Kind::Power => "電源ラベル: 同じ名前 (VCC など) の場所を線なしでつなぎます",
        _ => "",
    }
}

/// The parts library of a new editor: the user's library folder (the bundled parts are copied there the first time).
#[cfg(not(test))]
fn startup_library() -> (std::path::PathBuf, std::sync::Arc<Vec<part_lib::PartDef>>, Vec<String>) {
    let lib_dir = part_lib::default_dir();
    let _ = part_lib::install_examples(&lib_dir);
    let (library, lib_errors) = part_lib::load_library(&lib_dir);
    (lib_dir, std::sync::Arc::new(library), lib_errors)
}

/// In the tests: the bundled parts only, in a folder of the tests' own (emptied at the first editor of the run), read once. Every
/// editor reading the user's library took about 0.1 s (hundreds of tests make editors), and the user's own parts could change
/// what a test sees; a part a test saves no longer lands in the user's library.
#[cfg(test)]
fn startup_library() -> (std::path::PathBuf, std::sync::Arc<Vec<part_lib::PartDef>>, Vec<String>) {
    static LIB: std::sync::OnceLock<(std::path::PathBuf, std::sync::Arc<Vec<part_lib::PartDef>>, Vec<String>)> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        let dir = std::env::temp_dir().join("circuitCad-app-test-lib");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = part_lib::install_examples(&dir);
        let (library, lib_errors) = part_lib::load_library(&dir);
        (dir, std::sync::Arc::new(library), lib_errors)
    })
    .clone()
}

impl Default for SchEditor {
    fn default() -> Self {
        let (lib_dir, library, lib_errors) = startup_library();
        let lib_prefs = library::LibPrefs::load(&lib_dir);
        Self {
            library,
            basic_open: true,
            props_open: true,
            props_open_analyze: false,
            lib_rev: 0,
            quick: None,
            place_lib: None,
            series_value: String::new(),
            form: None,
            kicad: None,
            dc: None,
            show_i: true,
            show_v: true,
            vscale: VoltScale::default(),
            net_menu: None,
            last_run: LastRun::Dc,
            analysis_pins: crate::prefs::Prefs::default().analysis_pins,
            analysis_pins_changed: false,
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
            z: None,
            z_node: String::new(),
            z_ref: String::new(),
            z_remove: String::new(),
            z_pick: false,
            pz_out: String::new(),
            pz_in: String::new(),
            pz_pick: false,
            pz: None,
            loop_part: String::new(),
            loop_chart: loopgain::LoopChart::Bode,
            loop_squeeze: true,
            loop_xy: crate::plot::xy::XyView::new(true),
            locus: None,
            locus_n: "15".into(),
            psrr_source: String::new(),
            psrr_out: String::new(),
            cmrr_plus: String::new(),
            cmrr_minus: String::new(),
            cmrr_out: String::new(),
            cmrr_tol: "1".into(),
            loop_from: String::new(),
            loop_pick: false,
            src_form: None,
            sweep: None,
            temp_c: "27".into(),
            wiring: vec![],
            heat_copper: Default::default(),
            board_heat: None,
            board_heat_parts: vec![],
            post_layout_request: None,
            couplings: vec![],
            temp_from: "-20".into(),
            temp_to: "80".into(),
            temp_step: "5".into(),
            mc: None,
            sens: None,
            panel_tab: PanelTab::Draw,
            lesson: None,
            challenge: None,
            alt_pan: false,
            part_popup: None,
            sim_opts: sim_core::SimOptions::default(),
            show_sim_opts: false,
            popup_serial: 0,
            lesson_base: Schematic::default(),
            debug_fault: None,
            lesson_unmet: Default::default(),
            glossary_request: None,
            wiz_open: false,
            title_open: false,
            default_author: String::new(),
            wire_hops: true,
            wiz_kind: 0,
            wiz_in: vec![],
            sec_gen: 0,
            focus_section: None,
            breadboard: None,
            stripboard: None,
            strip_size: None,
            strip_chips_back: false,
            sketches: Default::default(),
            roles_cache: Default::default(),
            roles_hidden: None,
            tran_tab_request: None,
            coverage_cache: None,
            analysis_filter: String::new(),
            tables: Default::default(),
            lib_sync: Default::default(),
            adc_setups: Default::default(),
            adc_measure_ui: Default::default(),
            time_log_ui: Default::default(),
            specs: vec![],
            worst_spec_cache: vec![],
            saved_specs: vec![],
            bench: Default::default(),
            saved_bench: Default::default(),
            saved_tol: Default::default(),
            bench_ui: Default::default(),
            diag: Default::default(),
            repair: None,
            live_logic: None,
            sweep_src: String::new(),
            param: None,
            sweep_start: "0".into(),
            sweep_stop: "5".into(),
            sweep_step: "0.05".into(),
            noise_pick: false,
            pick_probe: false,
            pick_hover: None,
            noise_marks: vec![],
            wave_mode: WaveMode::Tran,
            fit_pending: false,
            lesson_fit: None,
            sch: Schematic::default(),
            undo: vec![],
            redo: vec![],
            committed: Schematic::default(),
            rev: 0,
            saved_rev: 0,
            slider: None,
            live: false,
            live_snap: Schematic::default(),
            live_ghost: true,
            live_cost: 0.0,
            live_last: None,
            live_waiting: false,
            path: None,
            tool: Tool::Place,
            place: Kind::Resistor,
            rot: 0,
            rot_for: None,
            label_text: "SIG".into(),
            label_port: None,
            sheet_trail: vec![],
            label_step: false,
            label_k: 0,
            label_k_text: String::new(),
            pending: None,
            selected: None,
            linked: Default::default(),
            linked_cache: Default::default(),
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
            status_hold: canvas::StatusHold::default(),
            help_request: None,
            fault_ui: Default::default(),
            mc_in: Default::default(),
            req: Default::default(),
            lib: library::LibUi::new(lib_dir, lib_errors, lib_prefs),
            param_in: Default::default(),
            heat: Default::default(),
            bat: Default::default(),
            budget: None,
            inrush: Default::default(),
        }
    }
}



// The editor is one type spread over several files; the child modules see its private fields.
use lessons::ActiveLesson;
mod hierarchy_ui;
mod linked;
mod preview;
mod quick;
#[cfg(test)]
pub use challenge::challenge_exists;
pub use spec_table::{take_specs, with_specs, SpecItem};
pub use bench_data::{allowance, judge, parse_reading, take_bench, with_bench, Agree, BenchData};
pub use tolerance::{take_settings, with_settings, SheetSettings};
pub use challenge::{challenge_of_lesson, progress_key as challenge_progress_key};
pub use lessons::{lesson_ids, lesson_texts, lesson_titles};
use symbols::{boxes_overlap, comp_extents, extents, rot_f, sheet_box, sym_extents, OVERLAP_MSG};
mod symbols;
mod glyphs;
pub(crate) use glyphs::{glyph_fits, symbol_lines, symbol_pin_labels};
mod wave;
mod ac;
mod analysis_panel;
mod adc_ui;
mod lib_sync;
mod adc_measure_ui;
mod time_log;
mod pindefs_ui;
mod result_tables;
mod sweep;
mod challenge;
mod lessons;
mod noise;
mod loopgain;
mod impedance;
mod polezero;
mod rootlocus;
mod variation;
mod sensitivity;
mod tolerance;
mod psrr;
mod cmrr;
mod troubleshoot;
mod repair_practice;
mod live_logic;
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
mod filter_place;
mod sheets_ui;
mod history;
mod hint;
mod canvas;
mod draw;
mod vscale;
mod acts;
mod pdf;
mod svg;
mod title_block;
mod interact;
pub(crate) mod panel;
pub(crate) use panel::analysis_sections;
mod coverage;
mod selfheat;
mod board_heat;
mod ambient_ui;
pub(crate) use board_heat::BoardHeat;
mod selfheat_tran;
mod fault;
mod spec_table;
mod bench_data;
mod bench_marks;
mod power_ui;
mod audio_ui;
mod power_on;
mod battery;
mod budget;
mod roles_ui;
mod inrush;
mod for_bench;
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
