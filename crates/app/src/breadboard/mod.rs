//! The prototype tab ("試作", candidate B6 ①): the schematic laid out on a solderless breadboard. The layout itself (which hole each
//! lead goes in, the jumpers, the supplies off the board) and the check against the schematic are `sch_core::breadboard`; this draws
//! the board, lights up the strip under the pointer, lets a part be dragged to other holes, and lists what the holes join that the
//! schematic does not (or the other way round). Tools: move (parts and jumper ends, R turns a part, Delete removes a jumper), jumper
//! (two holes), the meter (red and black probes on two holes: the DC voltage between them, B6 ③), the scope (a probe on a hole: the
//! transient waveform of its net) and the power switch (LEDs light by their current; parts past their ratings show as burnt, with why,
//! B6 ④; with the power on the circuit runs in time, `live.rs`, B6 ②: switches are clicked, the meter and scope read it now).
//! The layout is the sheet's (saved with it once the person moved something or asked for
//! a layout, `SheetSettings::breadboard`); until then it is laid out afresh from the schematic. Notes: `docs/code-notes/app/breadboard.md`.
//!
//! The files: this one has the view's state and the layout shown; `panel.rs` the left panel, `canvas.rs` the board drawn and
//! worked on, `draw.rs` the parts and wires drawn, `power.rs` the board with the power on, `instruments.rs` the meter and the scope,
//! `strip_view.rs` the stripboard and perfboard, `live.rs` the circuit running in time, `tests.rs` the tests.

use crate::learn::{spaced, NOTE};
use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use sch_core::breadboard::{auto_layout, board_nets, check, moved, unjoined, net_at, rotated, with_wire, without_wire, wire_end_moved, BoardNets, Hole, Layout, Look, Problem, Rail};
use std::collections::BTreeSet;

mod live;
mod flow_dots;
pub(crate) mod sketch_ui;
mod code_edit;
mod blocks_ui;
mod build_steps;
mod looks;
mod strip_view;
mod mate_ui;
pub use strip_view::{BoardKind, StripTool};
pub use mate_ui::mount_choice_ui;
#[cfg(test)]
pub use sketch_ui::BLINK;
pub(crate) mod sound;
use live::Live;
mod canvas;
mod draw;
mod instruments;
mod panel;
mod zoom;
mod power;
use draw::*;
use power::*;

/// What the person is dragging: the part, and the hole the pointer started on.
struct Drag {
    refdes: String,
    from: Hole,
}

/// What a click on the board does.
/// The range of the meter (B6 ③): how a real tester is set before its probes go on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MeterRange {
    /// DC voltage between the probes (across a part, the black probe usually on GND).
    #[default]
    Volts,
    /// Current: the meter goes in series, its probes on the two ends of a gap (`sim_core::meter::current`).
    Amps,
    /// Resistance with the power off (`sim_core::meter::resistance`).
    Ohms,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Drag parts and jumper ends; R turns the picked part, Delete removes the picked jumper.
    #[default]
    Move,
    /// Click a hole, then another: a jumper between them.
    Wire,
    /// The meter: the red probe on one hole, the black on another; the DC voltage between them.
    Meter,
    /// The scope's probe on a hole: the transient waveform of its net.
    Scope,
}

#[derive(Default)]
pub struct BreadboardView {
    /// Breadboard, stripboard or perfboard (B5).
    pub kind: BoardKind,
    /// The schematic in a small window beside the board (`SchEditor::preview`).
    pub show_schematic: bool,
    /// The zoom and the shift of each kind's picture (`zoom.rs`), by `BoardKind` in order.
    views: [zoom::ZoomPan; 3],
    /// The pointer was over a knob of the running circuit last frame: the wheel turns it, not the zoom.
    wheel_on_knob: bool,
    /// The breadboard's parts picked together (two or more, as on the schematic; one alone is `picked`), and a box being drawn to pick
    /// them: where the press was and whether it picks what it touches (the right button) or what it encloses (the left).
    chosen: Vec<String>,
    pick_box: Option<(Pos2, bool)>,
    /// Where the breadboard's origin was drawn and its pitch, last frame (for the tests' pointer).
    bread_drawn: Option<(Pos2, f32)>,
    /// The stripboard / perfboard view's tools, drag and caches (`strip_view.rs`).
    pub strip: strip_view::StripState,
    /// "組む順と通電": the build one section at a time and the check after each (`build_steps.rs`).
    pub build: build_steps::BuildState,
    /// "回路図の上で動かす" was pressed: the app starts the digital live mode on the schematic tab (`schematic/live_logic.rs`).
    pub want_live_logic: bool,
    /// The layout made from the schematic when none is saved, and the schematic's edit count it was made at.
    auto: Option<(u64, usize, Layout)>,
    drag: Option<Drag>,
    /// A jumper end being dragged: (jumper, end 0 or 1).
    wire_drag: Option<(usize, usize)>,
    /// The part picked on the board or in the list.
    pub picked: Option<String>,
    /// The jumper picked (Delete removes it).
    picked_wire: Option<usize>,
    pub tool: Tool,
    /// The first hole of a jumper being drawn.
    wire_start: Option<Hole>,
    /// The meter's probes and the scope's.
    pub red: Option<Hole>,
    pub black: Option<Hole>,
    pub meter_range: MeterRange,
    /// The last current or resistance reading and what it was read for (the range, the probes' nets, the power, the parts burnt,
    /// the schematic's edit count): a DC solve, not repeated every frame.
    meter_cache: Option<(String, Result<String, String>)>,
    /// The board checked against the schematic, by the schematic's edit count and the layout (`board_check`): the panel and the
    /// board both show it, so it was worked out twice a frame.
    checked: Option<(u64, Layout, Vec<Problem>)>,
    pub scope: Option<Hole>,
    /// The supply is switched on: LEDs light, parts past their ratings burn.
    pub powered: bool,
    /// The DC point was worked out once for the meter or the power switch (not again every frame when it fails).
    dc_asked: bool,
    /// With the power on: the circuit as the board joins it (worked out for the schematic's edit count and the layout in `run_key`),
    /// its DC point (the parts' currents and the parts past their ratings) or why it has none, and the circuit running in time
    /// (B6 ②) or why it could not start.
    run_key: Option<(u64, RunBoard)>,
    board: Option<Board>,
    live: Option<Live>,
    live_failed: Option<String>,
    /// The mouse went down on a switch (not a drag or a click of the tool).
    switch_down: bool,
    /// The rating check of the circuit while it runs.
    watch: Option<Watch>,
    /// The parts burnt and how they failed (open or short, as such a part usually dies): the circuit runs on with them broken,
    /// until the power is switched off or the board is changed. `restart`: start the run again with what is kept (a part has just
    /// burnt, the temperature was changed).
    broken: Vec<(String, sim_core::Fault)>,
    restart: bool,
    /// The sketches the run was started with (`sketch_ui::sketches_key`): a change of one starts it again.
    sketch_key: u64,
    /// The sketches built by arduino-cli for the real chip (W5 (b)), by their text (`sketch_ui::text_key`): None while it builds.
    real_builds: std::collections::HashMap<u64, sketch_ui::RealBuild>,
    /// The line being typed in each board's serial monitor (sent to the real chip with Enter or 送る).
    serial_input: std::collections::HashMap<String, String>,
    /// The board whose sketch is open in its own window (a bigger editor with line numbers), if any.
    pub sketch_window: Option<String>,
    /// Each board's sketch as it was before its last changes, for 元に戻す / やり直す (`sketch_ui::SketchHistory`; the schematic's
    /// undo does not hold the sketches).
    sketch_history: std::collections::HashMap<String, sketch_ui::SketchHistory>,
    /// The knobs turned (part → volts) and the temperature of the room (℃, None = the schematic's): kept when the run starts again.
    knob_values: std::collections::BTreeMap<String, f64>,
    /// A potentiometer being turned on the board: its knob, where the pointer went down and the knob's position then.
    knob_drag: Option<(usize, Pos2, f64)>,
    /// Where each part was drawn last frame: a potentiometer is turned by its drawn knob (in the middle gap, away from its
    /// leads), and the tests click on the board here.
    drawn: Vec<(String, Rect)>,
    temp_c: Option<f64>,
    /// The air's humidity the SHT31 modules measure (%RH, None = 50 %): kept when the run starts again.
    humidity: Option<f64>,
    /// Switches to move once the circuit starts running (`--proto-flip SW1,SW2`, for screenshots).
    pub flip_on_start: Vec<String>,
    /// The buzzers' tone through the speakers, and whether the learner turned it off.
    beeper: sound::Beeper,
    sound_off: bool,
    /// The other boards of the project plugged into this one (L4): set by the app when the board in the editors or the links change
    /// (`App::update_mates`), with what the last check found.
    pub mates: Vec<MateBoard>,
    /// (the board in the editors, the links) the mates were made for.
    pub mates_key: Option<(usize, usize, usize)>,
    /// `--proto-mate`: check the connectors against every plugged board as soon as the panel shows (for screenshots).
    pub mate_on_start: bool,
}

/// Another board of the project joined to this one by connectors, for matching the holes of the connectors (L4).
#[derive(Clone)]
pub struct MateBoard {
    /// The other board's name.
    pub name: String,
    /// The connector pairs: `a` the other board's part, `b` this board's, with the pins joined.
    pub links: Vec<sch_core::stripboard::MateLink>,
    /// The other board's schematic and its perfboard layout as saved (None: laid out by itself, made when the check runs).
    pub sch: sch_core::Schematic,
    pub layout: Option<sch_core::stripboard::StripLayout>,
    pub opt: sch_core::stripboard::StripOptions,
    /// What the last check found (on this board's layout then).
    pub found: Option<sch_core::stripboard::Mate>,
}

/// The DC point of the board's circuit: the current through each part, and the parts past (or near) their ratings.
pub type BoardDc = (Vec<(String, f64)>, Vec<sim_core::RatingIssue>);

/// Points of pointer travel (right or up) that turn a potentiometer on the board from one end to the other.
const KNOB_DRAG: f64 = 200.0;
/// How far one notch of the wheel turns it.
const KNOB_NOTCH: f64 = 0.05;

/// How long (seconds of the circuit's time) a part must stay past its rating to burn: a moment's inrush does not burn it.
const BURN_HOLD: f64 = 0.02;
/// The time constant (seconds of the circuit's time) of the average the currents and powers are checked by: a part heats up by
/// the mean of what it carries (a resistor on a sine is not burnt by its peaks), while a voltage breaks a part down at once.
const HEAT_TAU: f64 = 0.1;

/// The rating check of the running circuit: the schematic with the library's ratings, the parts burnt (they stay burnt until the
/// power is switched off, as a real one does), the parts near their ratings now, and since when each part has been past its rating.
struct Watch {
    rated: sch_core::Schematic,
    burnt: Vec<sim_core::RatingIssue>,
    hot: Vec<sim_core::RatingIssue>,
    over_since: std::collections::BTreeMap<String, f64>,
    /// Each element's current and power averaged over `HEAT_TAU`, and the time of the last check.
    avg: std::collections::BTreeMap<String, (f64, f64)>,
    last: f64,
}

impl Watch {
    fn new(rated: sch_core::Schematic) -> Watch {
        Watch { rated, burnt: vec![], hot: vec![], over_since: Default::default(), avg: Default::default(), last: 0.0 }
    }

    /// Checks the running circuit now. `start`: the operating point the run starts from (a steady state: past the rating there
    /// burns at once).
    fn check(&mut self, sch: &SchEditor, lv: &Live, start: bool) {
        let Some(mut snap) = lv.snapshot() else { return };
        let t = lv.time();
        // the currents and powers checked are their averages (the voltages are the ones now)
        let keep = if start { 0.0 } else { (-(t - self.last).max(0.0) / HEAT_TAU).exp() };
        self.last = t;
        for e in &mut snap.elements {
            let a = self.avg.entry(e.name.clone()).or_insert((e.current, e.power));
            *a = (keep * a.0 + (1.0 - keep) * e.current, keep * a.1 + (1.0 - keep) * e.power);
            (e.current, e.power) = *a;
        }
        let now = sch.board_ratings(&self.rated, &snap);
        let over = |r: &sim_core::RatingIssue| r.level == sim_core::RatingLevel::Over;
        self.over_since.retain(|k, _| now.iter().any(|r| &r.refdes == k && over(r)));
        for r in now.iter().filter(|r| over(r)) {
            if self.burnt.iter().any(|b| b.refdes == r.refdes) {
                continue;
            }
            let since = *self.over_since.entry(r.refdes.clone()).or_insert(t);
            if start || t - since >= BURN_HOLD {
                self.burnt.push(r.clone());
            }
        }
        self.hot = now.into_iter().filter(|r| !over(r) && !self.burnt.iter().any(|b| b.refdes == r.refdes)).collect();
    }
}

/// The breadboard's own circuit with the power on.
struct Board {
    nets: BoardNets,
    dc: Result<BoardDc, String>,
}

const BOARD: Color32 = Color32::from_rgb(236, 234, 226);
const HOLE: Color32 = Color32::from_rgb(70, 70, 74);
const RED: Color32 = Color32::from_rgb(214, 56, 50);
const BLUE: Color32 = Color32::from_rgb(52, 98, 196);
const LIT: Color32 = crate::theme::HIGHLIGHT;
const BAD: Color32 = crate::theme::ERROR;

/// Where a hole is drawn, in pitches: x from the left edge (four pitches left of the first column for the supplies off the board),
/// y from the top rail.
fn at(h: Hole) -> Vec2 {
    let x = 5.0 + h.col() as f32;
    let y = match h {
        Hole::Rail { rail: Rail::TopPlus, .. } => 0.0,
        Hole::Rail { rail: Rail::TopMinus, .. } => 1.0,
        Hole::Strip { row, .. } if row < 5 => 2.6 + row as f32,
        Hole::Strip { row, .. } => 4.4 + row as f32,
        Hole::Rail { rail: Rail::BottomMinus, .. } => 15.0,
        Hole::Rail { rail: Rail::BottomPlus, .. } => 16.0,
    };
    Vec2::new(x, y)
}

const HEIGHT: f32 = 16.0;

fn rail_colour(r: Rail) -> Color32 {
    match r {
        Rail::TopPlus | Rail::BottomPlus => RED,
        _ => BLUE,
    }
}

/// The colour of a jumper: red for the supply, black for ground, else one of a few by the net's name.
fn wire_colour(net: &str, l: &Layout) -> Color32 {
    let on_rail = |k: &str| l.rails.get(k).is_some_and(|n| n == net);
    if on_rail("TopMinus") || on_rail("BottomMinus") {
        return Color32::from_gray(30);
    }
    if on_rail("TopPlus") || on_rail("BottomPlus") {
        return RED;
    }
    const OTHERS: [Color32; 5] = [Color32::from_rgb(40, 140, 60), Color32::from_rgb(230, 170, 30), Color32::from_rgb(40, 110, 200), Color32::from_rgb(150, 70, 170), Color32::from_rgb(230, 110, 40)];
    let h = net.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    OTHERS[h % OTHERS.len()]
}

/// The colour bands of a resistor value (two digits, the multiplier, gold for 5 %).
fn bands(value: &str) -> Option<[Color32; 4]> {
    const C: [Color32; 10] = [
        Color32::from_rgb(20, 20, 20),
        Color32::from_rgb(120, 70, 30),
        Color32::from_rgb(210, 40, 40),
        Color32::from_rgb(240, 130, 30),
        Color32::from_rgb(240, 220, 40),
        Color32::from_rgb(40, 160, 60),
        Color32::from_rgb(40, 90, 210),
        Color32::from_rgb(140, 60, 180),
        Color32::from_rgb(130, 130, 130),
        Color32::from_rgb(250, 250, 250),
    ];
    let v = sim_core::parse_value(value).filter(|v| *v >= 1.0 && v.is_finite())?;
    let exp = v.log10().floor() as i32 - 1;
    let digits = (v / 10f64.powi(exp)).round() as i32;
    let (a, b) = ((digits / 10).clamp(0, 9) as usize, (digits % 10).clamp(0, 9) as usize);
    let m = exp.clamp(0, 9) as usize;
    Some([C[a], C[b], C[m], Color32::from_rgb(200, 160, 60)])
}

impl BreadboardView {
    /// The layout shown: the sheet's, else one made from the schematic (made again when the schematic changes).
    fn layout(&mut self, sch: &SchEditor) -> Layout {
        if let Some(l) = &sch.breadboard {
            return l.clone();
        }
        let key = (sch.rev, sch.sch.components.len());
        if !self.auto.as_ref().is_some_and(|(r, n, _)| (*r, *n) == key) {
            self.auto = Some((key.0, key.1, auto_layout(&sch.sch)));
        }
        self.auto.as_ref().map(|(_, _, l)| l.clone()).unwrap_or_default()
    }

    /// The status line's hint.
    pub fn hint(&self) -> String {
        if self.kind != BoardKind::Breadboard {
            return "部品・線の端は穴ごとにドラッグで動かせます。何も無い所からドラッグで囲むと部品を選べ (右ドラッグは触れた物も、Shift で足す)、選んだ部品はまとめて動かせます。動かすたびに、左で回路図と照らし合わせます".into();
        }
        "縦の 5 つの穴は中でつながっています。ポインタを穴に合わせると、つながっている穴が光ります。部品をドラッグすると差し替えられ、回路図と違うつなぎ方は赤い丸と左の一覧で分かります".into()
    }
}

/// A probe on a hole: the hole's name and the net there (None: joined to nothing).
pub(super) type Probe = (String, Option<String>);

/// The board a circuit runs on with the power on: a breadboard, or a stripboard or perfboard (B5d).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum RunBoard {
    Bread(Layout),
    Strip(sch_core::stripboard::StripLayout),
}

impl RunBoard {
    /// The circuit's nets as the board joins its leads.
    fn nets(&self, s: &sch_core::Schematic) -> BoardNets {
        match self {
            RunBoard::Bread(l) => board_nets(s, l),
            RunBoard::Strip(l) => sch_core::stripboard::strip_board_nets(s, l),
        }
    }

    /// The parts off the board (the supplies, the sources).
    fn off(&self) -> Vec<String> {
        match self {
            RunBoard::Bread(l) => l.off_board.iter().map(|o| o.refdes.clone()).collect(),
            RunBoard::Strip(l) => l.off.iter().map(|(r, _)| r.clone()).collect(),
        }
    }

    /// The parts on the board and their leads' pins.
    fn on(&self) -> Vec<(String, Vec<String>)> {
        match self {
            RunBoard::Bread(l) => l.parts.iter().map(|p| (p.refdes.clone(), p.leads.iter().map(|(pin, _)| pin.clone()).collect())).collect(),
            RunBoard::Strip(l) => l.parts.iter().map(|p| (p.refdes.clone(), p.leads.iter().map(|(pin, _)| pin.clone()).collect())).collect(),
        }
    }
}

fn is_ground(n: &str) -> bool {
    matches!(n.to_ascii_uppercase().as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND")
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// A hole's name as shown: the rail names ("上+") read in the UI language ("T+").
pub(crate) fn hole_text(h: Hole) -> String {
    match h {
        Hole::Rail { rail, col } => format!("{}{}", i18n::tr(sch_core::breadboard::rail_name(rail)), col + 1),
        _ => h.name(),
    }
}

#[cfg(test)]
mod tests;

impl BreadboardView {
    /// The zoom and shift of the picture of the board kind shown.
    fn view(&mut self) -> &mut zoom::ZoomPan {
        let k = match self.kind {
            BoardKind::Breadboard => 0,
            BoardKind::Stripboard => 1,
            BoardKind::Perfboard => 2,
        };
        &mut self.views[k]
    }

    /// The breadboard's parts picked: two or more are picked together, one alone is the picked part.
    fn set_bread_pick(&mut self, mut parts: Vec<String>) {
        if parts.len() >= 2 {
            self.picked = None;
            self.chosen = parts;
        } else {
            self.chosen.clear();
            self.picked = parts.pop();
        }
    }
}
