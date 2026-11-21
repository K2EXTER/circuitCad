//! The prototype tab ("試作", candidate B6 ①): the schematic laid out on a solderless breadboard. The layout itself (which hole each
//! lead goes in, the jumpers, the supplies off the board) and the check against the schematic are `sch_core::breadboard`; this draws
//! the board, lights up the strip under the pointer, lets a part be dragged to other holes, and lists what the holes join that the
//! schematic does not (or the other way round). Tools: move (parts and jumper ends, R turns a part, Delete removes a jumper), jumper
//! (two holes), the meter (red and black probes on two holes: the DC voltage between them, B6 ③), the scope (a probe on a hole: the
//! transient waveform of its net) and the power switch (LEDs light by their current; parts past their ratings show as burnt, with why,
//! B6 ④; with the power on the circuit runs in time, `live.rs`, B6 ②: switches are clicked, the meter and scope read it now).
//! The layout is the sheet's (saved with it once the person moved something or asked for
//! a layout, `SheetSettings::breadboard`); until then it is laid out afresh from the schematic. Notes: `docs/code-notes/app/breadboard.md`.

use crate::learn::{spaced, NOTE};
use crate::schematic::SchEditor;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use sch_core::breadboard::{auto_layout, board_nets, check, moved, unjoined, net_at, rotated, with_wire, without_wire, wire_end_moved, BoardNets, Hole, Layout, Look, Problem, Rail};
use std::collections::BTreeSet;

mod live;
pub(crate) mod sketch_ui;
mod strip_view;
pub use strip_view::BoardKind;
#[cfg(test)]
pub use sketch_ui::BLINK;
pub(crate) mod sound;
use live::Live;

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
    /// The stripboard / perfboard view's tools, drag and caches (`strip_view.rs`).
    pub strip: strip_view::StripState,
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
    run_key: Option<(u64, Layout)>,
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
    /// The knobs turned (part → volts) and the temperature of the room (℃, None = the schematic's): kept when the run starts again.
    knob_values: std::collections::BTreeMap<String, f64>,
    /// A potentiometer being turned on the board: its knob, where the pointer went down and the knob's position then.
    knob_drag: Option<(usize, Pos2, f64)>,
    /// Where each part was drawn last frame: a potentiometer is turned by its drawn knob (in the middle gap, away from its
    /// leads), and the tests click on the board here.
    drawn: Vec<(String, Rect)>,
    temp_c: Option<f64>,
    /// Switches to move once the circuit starts running (`--proto-flip SW1,SW2`, for screenshots).
    pub flip_on_start: Vec<String>,
    /// The buzzers' tone through the speakers, and whether the learner turned it off.
    beeper: sound::Beeper,
    sound_off: bool,
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

    /// The left panel: what this is, the differences from the schematic, the parts and where their leads go.
    pub fn panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        ui.horizontal(|ui| {
            for (k, name, tip) in [
                (BoardKind::Breadboard, "ブレッドボード", "はんだ不要の差し込み式の板。試しに組んで、電源を入れて動かせます"),
                (BoardKind::Stripboard, "ストリップ基板", "銅の帯が並んだ穴あき基板。帯を切る所と、表を通す線の並べ方を出します"),
                (BoardKind::Perfboard, "ユニバーサル基板", "穴だけの基板。裏で穴をつなぐすずめっき線と、表を通す線の並べ方を出します"),
            ] {
                if ui.selectable_label(self.kind == k, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.kind = k;
                    if k != BoardKind::Breadboard {
                        self.powered = false;
                    }
                }
            }
        });
        ui.checkbox(&mut self.show_schematic, i18n::tr("回路図を小窓で見る")).on_hover_text(i18n::tr(
            "回路図を小さな窓に出します (動かせる・大きさを変えられる)。窓の中の部品をクリックすると、基板の上でその部品が光り、基板で選んだ部品は窓の中で囲まれます。差し方を回路図と見比べながら組めます",
        ));
        if self.kind != BoardKind::Breadboard {
            self.strip_panel(ui, sch);
            return;
        }
        let l = self.layout(sch);
        crate::widgets::section_heading(ui, i18n::tr("ブレッドボード"));
        ui.label(spaced("回路図の部品を、ブレッドボードのどの穴に差すかを並べました。縦に並んだ 5 つの穴 (a〜e、f〜j) は中でつながっていて、上下の赤と青の列は電源とGNDです。部品をドラッグすると差し替えられます。").color(NOTE));
        let problems = self.board_check(sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            ui.colored_label(BAD, i18n::trf!("回路図と違う所が {} 件あります", problems.len()));
            for p in &problems {
                ui.label(egui::RichText::new(i18n::trf!("・{}", i18n::tr_dyn(&p.message()))).color(BAD));
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (t, name, tip) in [
                (Tool::Move, "動かす", "部品やジャンパー線の端をドラッグして差し替えます。選んだ部品は R で回り、選んだ線は Delete で消えます"),
                (Tool::Wire, "ジャンパー線", "穴をクリックして、つなぐ先の穴をクリックすると、線を差します。Esc でやめます"),
                (Tool::Meter, "テスター", "1 回目のクリックで赤い棒、2 回目で黒い棒を穴に当てます。レンジは 電圧 (並列に当てる)・電流 (線を外して直列に入れる)・抵抗 (電源を切って測る) から選びます"),
                (Tool::Scope, "オシロ", "探り針を穴に当てると、そのネットの電圧の時間の変化 (過渡解析) が左に出ます"),
            ] {
                if ui.selectable_label(self.tool == t, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.tool = t;
                    self.wire_start = None;
                    self.dc_asked = false;
                }
            }
        });
        let power_label = if self.powered { i18n::tr("電源を切る") } else { i18n::tr("電源を入れる") };
        if ui.button(power_label).on_hover_text(i18n::tr("電源を入れると、回路が時間とともに動き出します (過渡解析を実時間で進め続けます)。計算するのはブレッドボードに差したとおりの回路なので、回路図と違う差し方なら、そのとおりに動いたり壊れたりします。LED は流れる電流で光り、スイッチと押しボタンはクリックで切り替わります。定格を超えた部品は焦げた印と理由が出ます (実物なら壊れるところ)")).clicked() {
            self.powered = !self.powered;
            self.dc_asked = false;
        }
        if ui
            .button(i18n::tr("デジタル回路を回路図の上で動かす"))
            .on_hover_text(i18n::tr("回路図のタブに移って、デジタル回路を動かしながら見ます。配線が High / Low の色になり、真理値表・ロジックアナライザ・フリップフロップの記憶・7 セグメントの表示・グリッチの印が出ます (ブレッドボードの絵より、信号の流れが追いやすい形)"))
            .clicked()
        {
            self.want_live_logic = true;
        }
        self.sketch_ui(ui, sch);
        // the meter with the power off reads the schematic's DC point (what the board will read once it is switched on)
        if !self.powered && self.tool == Tool::Meter && sch.bench_voltages().is_none() && !self.dc_asked && problems.is_empty() {
            self.dc_asked = true;
            sch.analyze_dc();
        }
        self.live_frame(ui, sch, &l, problems.is_empty());
        match self.tool {
            Tool::Meter => self.meter_ui(ui, sch, &l, problems.is_empty()),
            Tool::Scope => self.scope_ui(ui, sch, &l),
            Tool::Wire => {
                ui.label(egui::RichText::new(i18n::tr("穴をクリックして、つなぐ先の穴をクリックします")).color(NOTE));
            }
            Tool::Move => {}
        }
        if self.powered {
            let issues = self.issues();
            if let Some(Board { dc: Err(e), .. }) = &self.board {
                ui.colored_label(crate::theme::WARN, i18n::trf!("電源を入れたときの直流の様子が求まりませんでした: {}", i18n::tr_dyn(e)));
            } else if issues.is_empty() {
                ui.colored_label(crate::theme::OK, i18n::tr("電源を入れました。定格を超えた部品はありません"));
            } else {
                for r in &issues {
                    let (col, head) = if r.level == sim_core::RatingLevel::Over { (BAD, i18n::tr("壊れた")) } else { (crate::theme::WARN, i18n::tr("熱い・余裕が少ない")) };
                    ui.label(egui::RichText::new(format!("{} {}: {}", r.refdes, head, i18n::tr_dyn(&r.message))).color(col));
                    if let Some((_, f)) = self.broken.iter().find(|(b, _)| element_of(b, &r.refdes)) {
                        ui.label(egui::RichText::new(i18n::trf!("　{} は {} になり、回路からはそのように計算しています", r.refdes, i18n::tr_dyn(&f.label()))).color(BAD));
                    }
                    ui.label(egui::RichText::new(format!("　→ {}", i18n::tr_dyn(&r.hint))).color(NOTE));
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("自動で並べ直す")).on_hover_text(i18n::tr("回路図から、部品の差し方を並べ直します (IC は中央の溝をまたぎ、電源とGNDは上下の列、同じネットの足は同じ縦の列)。手で動かした分は消えます")).clicked() {
                sch.breadboard = Some(auto_layout(&sch.sch));
            }
            if sch.breadboard.is_none() {
                ui.label(egui::RichText::new(i18n::tr("(自動で並べた形)")).color(NOTE)).on_hover_text(i18n::tr("まだ手で動かしていないので、回路図を変えると並べ直されます。部品を動かすと、その並べ方が回路図のファイルに入ります"));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("部品と差す穴")).strong());
        // one part a row, its holes wrapped within the panel (a 30-pin board on one line widened the panel past the breadboard)
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (k, p) in l.parts.iter().enumerate() {
                let picked = self.picked.as_deref() == Some(p.refdes.as_str());
                let row = |ui: &mut egui::Ui, picked: bool| {
                    ui.horizontal_top(|ui| {
                        let clicked = ui.add_sized([44.0, 18.0], egui::Button::selectable(picked, egui::RichText::new(&p.refdes).strong())).on_hover_text(i18n::tr("選ぶと、ブレッドボードの上でこの部品が光ります")).clicked();
                        let text = p.leads.iter().map(|(pin, h)| format!("{pin}:{}", h.name())).collect::<Vec<_>>().join("  ");
                        ui.add(egui::Label::new(text).wrap());
                        clicked
                    })
                    .inner
                };
                let clicked = if k % 2 == 1 {
                    egui::Frame::NONE.fill(ui.visuals().faint_bg_color).show(ui, |ui| row(ui, picked)).inner
                } else {
                    row(ui, picked)
                };
                if clicked {
                    self.picked = if picked { None } else { Some(p.refdes.clone()) };
                }
            }
            if !l.off_board.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ブレッドボードの外 (電源・電池・信号源)")).strong()).on_hover_text(i18n::tr("実験用の電源・電池・発振器などは、ブレッドボードの外に置いて、線でつなぎます"));
                for o in &l.off_board {
                    ui.label(format!("{}  {}", o.refdes, o.pins.iter().map(|(pin, net, h)| format!("{pin} ({net}) → {}", h.name())).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// The parts past (or near) their ratings on the board's circuit with the power on: while it runs, the parts burnt so far and
    /// the ones near their ratings now; else at its DC point.
    fn issues(&self) -> Vec<sim_core::RatingIssue> {
        if let Some(w) = self.watch.as_ref().filter(|_| self.powered) {
            return w.burnt.iter().chain(&w.hot).cloned().collect();
        }
        match &self.board {
            Some(Board { dc: Ok((_, r)), .. }) if self.powered => r.clone(),
            _ => vec![],
        }
    }

    /// With the power on: the circuit as the board joins it is worked out (again when the schematic or the layout changes), its DC
    /// point found, and it runs in time; each frame the time goes on, and the panel shows the time, the speed and the switches.
    fn live_frame(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout, matches: bool) {
        self.run_frame(ui, sch, l, matches);
        let step = self.buzz_step(sch, l);
        self.beeper.set(step);
    }

    /// Stops the buzzers' tone (the tab is left).
    pub fn silence(&mut self) {
        self.beeper.set(None);
    }

    /// The buzzers on the board with the voltage from their + to their − now (not the burnt ones).
    fn buzzers(&self, sch: &SchEditor, l: &Layout) -> Vec<(String, f64)> {
        let (Some(lv), Some(b)) = (self.live.as_ref().filter(|v| self.powered && v.error.is_none()), self.board.as_ref()) else { return vec![] };
        buzzer_volts(sch, l, &b.nets, lv).into_iter().filter(|(r, _)| !self.broken.iter().any(|(e, _)| element_of(e, r))).collect()
    }

    /// How loud the tone is: the loudest buzzer sounding, or None (none sounds, or the sound is off).
    /// The board's differences from the schematic (`check`), kept while the schematic and the layout stay the same.
    fn board_check(&mut self, sch: &SchEditor, l: &Layout) -> Vec<Problem> {
        if let Some((r, k, p)) = &self.checked
            && *r == sch.rev
            && k == l
            && sch.settled()
        {
            return p.clone();
        }
        let p = check(&sch.sch, l);
        self.checked = Some((sch.rev, l.clone(), p.clone()));
        p
    }

    fn buzz_step(&self, sch: &SchEditor, l: &Layout) -> Option<u8> {
        if self.sound_off {
            return None;
        }
        self.buzzers(sch, l).into_iter().filter_map(|(_, v)| sound::loudness(v)).max()
    }

    fn run_frame(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout, matches: bool) {
        if !self.powered {
            (self.live, self.board, self.run_key, self.live_failed, self.watch) = (None, None, None, None, None);
            self.knob_values.clear();
            self.broken.clear();
            self.switch_down = false;
            return;
        }
        let board_changed = self.run_key.as_ref().is_none_or(|(r, k)| *r != sch.rev || k != l);
        let sketches = sketch_ui::sketches_key(sch);
        if board_changed || self.restart || sketches != self.sketch_key {
            self.sketch_key = sketches;
            self.run_key = Some((sch.rev, l.clone()));
            self.restart = false;
            // a new board (or schematic) has new parts: nothing on it is burnt yet
            let kept_burnt = if board_changed { vec![] } else { self.watch.as_ref().map(|w| w.burnt.clone()).unwrap_or_default() };
            if board_changed {
                self.broken.clear();
            }
            let speed = self.live.as_ref().map_or(1.0, |v| v.speed);
            let on: Vec<String> = self.live.as_ref().map(|v| v.switches.iter().filter(|s| s.on && !s.push).map(|s| s.refdes.clone()).collect()).unwrap_or_default();
            self.live = None;
            self.live_failed = None;
            self.watch = None;
            let nets = board_nets(&sch.sch, l);
            let dc = sch.board_dc(&nets, self.temp_c);
            // the parts burnt so far, broken in the circuit
            let built = sch.board_circuit(&nets, self.temp_c).map(|c| self.broken.iter().fold(c, |c, (r, f)| sim_core::with_fault(&c, r, *f).unwrap_or(c)));
            match built {
                Ok(c) => {
                    let leds: Vec<String> = sch.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
                    // the knobs: the DC voltage sources off the board (a bench supply, the light or the signal on a sensor's input)
                    // and the potentiometers on the board
                    let mut knobs: Vec<String> = l.off_board.iter().filter(|o| sch.sch.components.iter().any(|c| c.refdes == o.refdes && c.kind == sch_core::Kind::VSource)).map(|o| o.refdes.clone()).collect();
                    knobs.extend(l.parts.iter().filter(|p| sch.sch.components.iter().any(|c| c.refdes == p.refdes && c.kind == sch_core::Kind::Pot)).map(|p| p.refdes.clone()));
                    let mut lv = Live::start(c, &leds, sch.live_switches(), &knobs);
                    lv.speed = speed;
                    // the microcontroller boards with their sketches (B6 ⑤)
                    for refdes in sketch_ui::boards(&sch.sch) {
                        let pins = sketch_ui::pin_nets(&sch.sch, &refdes, &nets);
                        lv.add_mcu(refdes.clone(), sim_core::sketch::compile(&sketch_ui::sketch_of(sch, &refdes)), pins);
                    }
                    for (r, v) in &self.knob_values {
                        if let Some(k) = lv.knob_index(r) {
                            lv.set_knob(k, *v);
                        }
                    }
                    // the slide switches stay where they were when the board is changed
                    for r in on.into_iter().chain(std::mem::take(&mut self.flip_on_start)) {
                        if let Some(k) = lv.switch_index(&r) {
                            lv.press(k);
                        }
                    }
                    let mut w = Watch::new(sch.rated_schematic());
                    w.burnt = kept_burnt;
                    w.check(sch, &lv, true);
                    self.watch = Some(w);
                    self.live = Some(lv);
                }
                Err(e) => self.live_failed = Some(e),
            }
            self.board = Some(Board { nets, dc });
        }
        if !matches {
            ui.label(egui::RichText::new(i18n::tr("回路図と違う差し方のまま動かしています (計算はブレッドボードに差したとおり)")).color(crate::theme::WARN))
                .on_hover_text(i18n::tr("実物と同じで、違う差し方なら違う動きをします。ショートした所には大きな電流が流れ、外れた所には流れません"));
        }
        if let Some(e) = &self.live_failed {
            ui.colored_label(crate::theme::WARN, i18n::trf!("回路を動かせませんでした: {}", i18n::tr_dyn(e)));
            return;
        }
        let Some(lv) = self.live.as_mut() else { return };
        if let Some(e) = &lv.error {
            ui.colored_label(crate::theme::WARN, i18n::tr_dyn(e));
            return;
        }
        lv.frame(ui.input(|i| i.time));
        if let Some(w) = self.watch.as_mut() {
            w.check(sch, lv, false);
            // a part that has just burnt fails in the circuit (an LED or a resistor opens, a transistor shorts): run on without it
            for b in &w.burnt {
                if self.broken.iter().any(|(r, _)| element_of(r, &b.refdes)) {
                    continue;
                }
                // the part's own element, or the hottest one inside it (the piece of a potentiometer's track the current runs through)
                let el = lv.element(&b.refdes).map(|e| e.name().to_string()).or_else(|| {
                    let snap = lv.snapshot()?;
                    snap.elements.iter().filter(|e| element_of(&e.name, &b.refdes)).max_by(|x, y| x.power.total_cmp(&y.power)).map(|e| e.name.clone())
                });
                let fault = el.as_ref().and_then(|n| sch.sch.components.iter().find(|c| c.refdes == b.refdes).and_then(|comp| lv.element(n).and_then(|e| sim_core::burn_failure(comp, e))));
                if let (Some(n), Some(f)) = (el, fault) {
                    self.broken.push((n, f));
                    self.restart = true;
                }
            }
        }
        ui.ctx().request_repaint();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(i18n::trf!("動いています  {} s", format!("{:.2}", lv.time()))).color(crate::theme::OK));
            ui.label(i18n::tr("速さ")).on_hover_text(i18n::tr("回路の時間の進み方です。速い回路 (kHz の発振など) は、遅くすると目で追えます"));
            egui::ComboBox::from_id_salt("bb_speed").selected_text(i18n::tr_dyn(live::SPEEDS.iter().find(|(v, _)| *v == lv.speed).map_or("実時間", |(_, n)| *n))).show_ui(ui, |ui| {
                for (v, name) in live::SPEEDS {
                    ui.selectable_value(&mut lv.speed, v, i18n::tr_dyn(name));
                }
            });
        });
        if l.parts.iter().any(|p| is_buzzer(sch, &p.refdes)) {
            let mut on = !self.sound_off;
            if ui
                .checkbox(&mut on, i18n::tr("ブザーの音を鳴らす"))
                .on_hover_text(i18n::trf!("鳴っているブザーの音 (約 {} Hz) をスピーカーから出します。ブザーの + が − より約 {} V 以上高いと鳴り、電圧が高いほど大きくなります。逆向きでは鳴りません", sound::BUZZ_HZ, sound::BUZZ_FROM))
                .changed()
            {
                self.sound_off = !on;
            }
        }
        if !lv.knobs.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("つまみ (電源・信号源の電圧、ボリューム)")).strong())
                .on_hover_text(i18n::tr("ブレッドボードの外の電源や信号源の電圧と、ボリュームのつまみを、動かしたまま変えます。実験用の電源のつまみを回す・センサーに当てる光を強くする・ボリュームを回す、の代わりです"));
            for k in 0..lv.knobs.len() {
                let (name, mut v, range) = (lv.knobs[k].refdes.clone(), lv.knobs[k].value, lv.knobs[k].range());
                let (nominal, pot) = (lv.knobs[k].nominal, lv.knobs[k].pot.is_some());
                ui.horizontal(|ui| {
                    ui.label(&name);
                    let r = if pot {
                        // the position as percent of the turn
                        let mut pct = v * 100.0;
                        let r = ui
                            .add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %").max_decimals(0))
                            .on_hover_text(i18n::trf!("ボリュームのつまみ: 0 % で 2 番の足が 1 番の側、100 % で 3 番の側です。回路図では {} % です", (nominal * 100.0).round()));
                        v = pct / 100.0;
                        r
                    } else {
                        ui.add(egui::Slider::new(&mut v, range).suffix(" V").max_decimals(2)).on_hover_text(i18n::trf!("回路図の値は {} V です", sim_core::format_value(nominal)))
                    };
                    if r.changed() {
                        lv.set_knob(k, v);
                        self.knob_values.insert(name.clone(), v);
                    }
                    if v != nominal && ui.small_button(i18n::tr("戻す")).on_hover_text(i18n::tr("回路図の値に戻します")).clicked() {
                        lv.set_knob(k, nominal);
                        self.knob_values.remove(&name);
                    }
                });
            }
        }
        // the room's temperature: thermistors and temperature sensors follow it (the run starts again at the new temperature)
        let mut t = self.temp_c.unwrap_or_else(|| sch.board_temp());
        let mut restart = false;
        ui.horizontal(|ui| {
            ui.label(i18n::tr("周りの温度"));
            let r = ui.add(egui::Slider::new(&mut t, -20.0..=100.0).suffix(" ℃").max_decimals(0)).on_hover_text(i18n::tr("サーミスタ・温度センサーなど、温度で値が変わる部品が変わります。放すと、その温度で計算し直します (回路の時間は 0 から)"));
            if r.changed() {
                self.temp_c = Some(t);
                restart = r.drag_stopped() || !r.dragged();
            }
        });
        if restart {
            self.restart = true;
        }
        if lv.behind {
            ui.label(egui::RichText::new(i18n::tr("計算が画面の時間に追いついていません (この回路は実時間では重すぎます)。速さを遅くします")).color(crate::theme::WARN));
        }
        if !lv.switches.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("スイッチ・押しボタンは、ブレッドボードの上でクリックします (押しボタンは押している間だけ入ります)")).color(NOTE));
            ui.horizontal_wrapped(|ui| {
                for s in &lv.switches {
                    let state = if s.on { i18n::tr("入") } else { i18n::tr("切") };
                    ui.label(egui::RichText::new(format!("{} {state}", s.refdes)).color(if s.on { crate::theme::OK } else { NOTE }));
                }
            });
        }
    }

    /// The meter's reading: the DC voltage of the red probe's net against the black's.
    fn meter_ui(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout, matches: bool) {
        ui.horizontal(|ui| {
            for (r, name, tip) in [
                (MeterRange::Volts, "電圧 V", "2 本の棒の間の直流の電圧を読みます。部品の両端に当てます (並列)。黒い棒はふつう GND に"),
                (MeterRange::Amps, "電流 A", "テスターに電流を通して読みます。回路の線を 1 か所外し、その両端に棒を当てます (直列)。電源の + と − に当てると、テスターがショートになってヒューズが切れます"),
                (MeterRange::Ohms, "抵抗 Ω", "電源を切って、2 本の棒の間の抵抗を読みます。テスターが小さな電流を流して測るので、電源が入っていると正しく読めません。部品を回路につないだままだと、まわりの部品も一緒に測ります"),
            ] {
                if ui.selectable_label(self.meter_range == r, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.meter_range = r;
                }
            }
        });
        if self.meter_range != MeterRange::Volts {
            self.meter_other_range(ui, sch, l);
            return;
        }
        let (Some(red), Some(black)) = (self.red, self.black) else {
            ui.label(egui::RichText::new(if self.red.is_none() { i18n::tr("赤い棒を当てる穴をクリックします") } else { i18n::tr("黒い棒を当てる穴をクリックします (ふつうは GND の青い列)") }).color(NOTE));
            return;
        };
        let live = self.live.as_ref().filter(|v| v.error.is_none());
        if !matches && live.is_none() {
            ui.colored_label(crate::theme::WARN, i18n::tr("回路図と違う差し方なので、電源を入れてから読みます (差したとおりの回路を計算します)"));
            return;
        }
        let (na, nb) = (net_at(&sch.sch, l, red), net_at(&sch.sch, l, black));
        let volts = sch.bench_voltages();
        let v_of = |n: &Option<String>| -> Option<f64> {
            let n = n.as_ref()?;
            if is_ground(n) {
                return Some(0.0);
            }
            if let Some(v) = live.and_then(|lv| lv.voltage(n)) {
                return Some(v);
            }
            volts.as_ref()?.iter().find(|(m, _)| m.eq_ignore_ascii_case(n)).map(|(_, v)| *v)
        };
        let name = |h: Hole, n: &Option<String>| format!("{} ({})", h.name(), n.clone().unwrap_or_else(|| i18n::tr("どこにもつながっていない").to_string()));
        ui.label(i18n::trf!("赤: {}　黒: {}", name(red, &na), name(black, &nb)));
        match (v_of(&na), v_of(&nb)) {
            (Some(a), Some(b)) => {
                ui.label(egui::RichText::new(format!("{} V", sim_core::format_value(a - b))).size(22.0).strong().color(crate::theme::OK));
            }
            _ if na.is_none() || nb.is_none() => {
                ui.colored_label(crate::theme::WARN, i18n::tr("棒の片方が、どこにもつながっていない穴です (表示は定まりません)"));
            }
            _ => {
                ui.label(egui::RichText::new(i18n::tr("DC 動作点が求まっていません (回路図の解析で確かめます)")).color(NOTE));
            }
        }
    }

    /// The current and resistance ranges: worked out on the circuit as the board joins it (the parts burnt so far broken in it).
    fn meter_other_range(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout) {
        let (Some(red), Some(black)) = (self.red, self.black) else {
            ui.label(egui::RichText::new(if self.red.is_none() { i18n::tr("赤い棒を当てる穴をクリックします") } else { i18n::tr("黒い棒を当てる穴をクリックします") }).color(NOTE));
            return;
        };
        let (na, nb) = (net_at(&sch.sch, l, red), net_at(&sch.sch, l, black));
        let name = |h: Hole, n: &Option<String>| format!("{} ({})", h.name(), n.clone().unwrap_or_else(|| i18n::tr("どこにもつながっていない").to_string()));
        ui.label(i18n::trf!("赤: {}　黒: {}", name(red, &na), name(black, &nb)));
        let range = self.meter_range;
        if range == MeterRange::Ohms && self.powered {
            ui.colored_label(crate::theme::WARN, i18n::tr("電源が入っています。抵抗は電源を切ってから測ります (テスターが自分で流す小さな電流に、回路の電流が混ざって正しく読めません。実物では、テスターを傷めることもあります)"));
            return;
        }
        if range == MeterRange::Amps && !self.powered {
            ui.label(egui::RichText::new(i18n::tr("電流は、電源を入れてから測ります (切っているあいだは 0 A)")).color(NOTE));
            return;
        }
        let (Some(a), Some(b)) = (na, nb) else {
            ui.colored_label(crate::theme::WARN, i18n::tr("棒の片方が、どこにもつながっていない穴です"));
            return;
        };
        let key = format!("{range:?}|{a}|{b}|{}|{:?}|{}|{:?}", self.powered, self.broken, sch.rev, self.temp_c);
        if self.meter_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let nets = board_nets(&sch.sch, l);
            let built = sch.board_circuit(&nets, self.temp_c).map(|c| self.broken.iter().fold(c, |c, (r, f)| sim_core::with_fault(&c, r, *f).unwrap_or(c)));
            let reading = built.and_then(|c| match range {
                MeterRange::Ohms => sim_core::meter::resistance(&c, &a, &b).map(|r| match r {
                    Some(r) => format!("{}Ω", sim_core::format_value(r)),
                    None => "OL".to_string(),
                }),
                _ => sim_core::meter::current(&c, &a, &b).map(|x| match x {
                    sim_core::meter::Amps::Reading(i) => format!("{}A", sim_core::format_value(i)),
                    sim_core::meter::Amps::SameNet => "SAME".to_string(),
                    sim_core::meter::Amps::FuseBlown(i) => format!("FUSE {}", sim_core::format_value(i.abs())),
                }),
            });
            self.meter_cache = Some((key, reading));
        }
        let Some((_, reading)) = &self.meter_cache else { return };
        match reading {
            Err(e) => {
                ui.colored_label(crate::theme::ERROR, i18n::tr_dyn(e));
            }
            Ok(r) if r == "SAME" => {
                ui.label(egui::RichText::new("0 A").size(22.0).strong().color(crate::theme::OK));
                ui.label(egui::RichText::new(i18n::tr("2 本の棒が同じ線 (ネット) に当たっています。電流計は直列に入れます: 測りたい線を 1 か所外し (ジャンパー線を抜く・部品の足を別の列へ)、空いた 2 つの穴に赤と黒を当てます")).color(NOTE));
            }
            Ok(r) if r.starts_with("FUSE") => {
                ui.label(egui::RichText::new(i18n::tr("ヒューズが切れました")).size(18.0).strong().color(crate::theme::ERROR));
                ui.label(egui::RichText::new(i18n::trf!("テスターの中を {}A 流れようとしました。電流のレンジのテスターは、中がほとんど 0 Ω の線です。電源の + と − (並列) に当てるとショートになります。電流は、線を外した所に直列に入れて測ります", &r[5..])).color(crate::theme::WARN));
            }
            Ok(r) if r == "OL" => {
                ui.label(egui::RichText::new("OL").size(22.0).strong().color(crate::theme::OK));
                ui.label(egui::RichText::new(i18n::tr("つながっていない (開いている) か、とても大きな抵抗です。コンデンサは直流を通さないので OL です")).color(NOTE));
            }
            Ok(r) => {
                ui.label(egui::RichText::new(r.as_str()).size(22.0).strong().color(crate::theme::OK));
                let note = if range == MeterRange::Ohms {
                    i18n::tr("回路につないだまま測ると、並んだ部品も一緒に測ります (並列なら小さく出ます)。部品だけの値は、片方の足を外して測ります。ダイオード・LED は向きと流す電流で読みが変わります")
                } else {
                    i18n::tr("赤い棒から入って黒い棒へ抜ける向きが + です。テスターの 0.1Ω が入るので、ごく小さい抵抗の回路では少し減ります")
                };
                ui.label(egui::RichText::new(note).color(NOTE));
            }
        }
    }

    /// The scope: the transient waveform of the probe's net.
    fn scope_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, l: &Layout) {
        let Some(h) = self.scope else {
            ui.label(egui::RichText::new(i18n::tr("探り針を当てる穴をクリックします")).color(NOTE));
            return;
        };
        let Some(net) = net_at(&sch.sch, l, h) else {
            ui.colored_label(crate::theme::WARN, i18n::trf!("{} は、どこにもつながっていない穴です", h.name()));
            return;
        };
        ui.label(i18n::trf!("探り針: {} ({net})", h.name()));
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            lv.set_probe(Some(net.clone()));
            let (t, v) = lv.trace();
            ui.label(egui::RichText::new(i18n::tr("動いている波形 (画面の直近 3 秒)")).color(NOTE));
            if t.len() > 1 {
                scope_plot(ui, &t, &v);
            }
            return;
        }
        let signal = if is_ground(&net) { None } else { sch.tran_signal(&format!("V({net})")) };
        if ui.button(i18n::tr("過渡解析を実行")).on_hover_text(i18n::tr("回路図の過渡解析の欄の時間で計算し直します")).clicked() {
            sch.analyze_tran();
        }
        let Some((t, v)) = signal else {
            ui.label(egui::RichText::new(if is_ground(&net) { i18n::tr("GND は 0 V です") } else { i18n::tr("過渡解析の結果がありません。「過渡解析を実行」を押します") }).color(NOTE));
            return;
        };
        scope_plot(ui, &t, &v);
    }

    /// The board. `focus`: the part picked in the schematic, lit here too.
    pub fn canvas(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, focus: Option<&str>) {
        if self.kind != BoardKind::Breadboard {
            self.strip_canvas(ui, sch);
            return;
        }
        let mut l = self.layout(sch);
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        if l.parts.is_empty() && l.off_board.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("回路図に部品がありません。回路図の画面で部品を置いて配線すると、ここにブレッドボードの差し方が出ます"), egui::FontId::proportional(16.0), Color32::from_gray(200));
            return;
        }
        let width = 5.0 + l.cols as f32 + 1.0;
        let pitch = ((rect.width() - 40.0) / width).min((rect.height() - 60.0) / (HEIGHT + 2.0)).clamp(6.0, 28.0);
        let origin = Pos2::new(rect.left() + (rect.width() - width * pitch) / 2.0, rect.top() + 40.0 + pitch);
        let to = |h: Hole| origin + at(h) * pitch;
        let cols = l.cols;
        let hole_at = |p: Pos2| -> Option<Hole> {
            let v = (p - origin) / pitch;
            let col = (v.x - 5.0).round();
            if col < 0.0 || col >= cols as f32 {
                return None;
            }
            let col = col as u16;
            let cands = [Hole::Rail { rail: Rail::TopPlus, col }, Hole::Rail { rail: Rail::TopMinus, col }, Hole::Rail { rail: Rail::BottomMinus, col }, Hole::Rail { rail: Rail::BottomPlus, col }];
            cands.into_iter().chain((0..10).map(|row| Hole::Strip { col, row })).find(|h| (at(*h).y - v.y).abs() < 0.5)
        };
        // a jumper end near the pointer (they lie over the parts' leads)
        let wire_end_at = |p: Pos2| -> Option<(usize, usize)> {
            l.wires.iter().enumerate().flat_map(|(k, w)| [(k, 0, w.a), (k, 1, w.b)]).find(|(_, _, h)| to(*h).distance(p) < pitch * 0.45).map(|(k, e, _)| (k, e))
        };
        // with the circuit running, a press on a switch moves it (a push button closes while held) instead of the tool's click
        let mut on_switch = self.switch_down;
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            if response.hovered()
                && ui.input(|i| i.pointer.primary_pressed())
                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                && let Some(k) = l.parts.iter().filter(|x| part_rect(x, to, pitch).expand(pitch * 0.4).contains(p)).find_map(|x| lv.switch_index(&x.refdes))
            {
                lv.press(k);
                self.switch_down = true;
                on_switch = true;
            }
            if ui.input(|i| i.pointer.primary_released()) {
                lv.release();
                self.switch_down = false;
            }
        }
        // with the circuit running, a potentiometer is turned on the board: dragged right or up turns it towards pin 3, the wheel
        // a notch at a time
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            // the leads, or the knob as it was drawn (the hover help on the knob says to drag or wheel it)
            let drawn = &self.drawn;
            let pot_at = |lv: &Live, p: Pos2| {
                l.parts
                    .iter()
                    .filter(|x| part_rect(x, to, pitch).expand(pitch * 0.4).contains(p) || drawn.iter().any(|(r, d)| *r == x.refdes && d.contains(p)))
                    .find_map(|x| lv.knob_index(&x.refdes).filter(|k| lv.knobs[*k].pot.is_some()))
            };
            if !on_switch
                && response.hovered()
                && ui.input(|i| i.pointer.primary_pressed())
                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                && let Some(k) = pot_at(lv, p)
            {
                self.knob_drag = Some((k, p, lv.knobs[k].value));
            }
            let mut turn = None;
            if let Some((k, from, v0)) = self.knob_drag {
                on_switch = true;
                if let Some(p) = ui.ctx().pointer_latest_pos() {
                    turn = Some((k, knob_dragged(v0, from, p)));
                }
                if ui.input(|i| i.pointer.primary_released()) {
                    self.knob_drag = None;
                }
            } else if let Some(p) = response.hover_pos()
                && let Some(k) = pot_at(lv, p)
            {
                // one step for each wheel event (a notch of a mouse wheel)
                let notches: f32 = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y.signum()) } else { None }).sum());
                if notches != 0.0 {
                    turn = Some((k, (lv.knobs[k].value + KNOB_NOTCH * notches as f64).clamp(0.0, 1.0)));
                }
            }
            if let Some((k, v)) = turn
                && (v - lv.knobs[k].value).abs() > 1e-9
            {
                lv.set_knob(k, v);
                self.knob_values.insert(lv.knobs[k].refdes.clone(), v);
            }
        } else {
            self.knob_drag = None;
        }
        // dragging (the move tool): a jumper's end, or the part, follows the pointer by whole holes; let go to keep it there
        if self.tool == Tool::Move
            && !on_switch
            && response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            if let Some(we) = wire_end_at(p) {
                self.wire_drag = Some(we);
                self.picked_wire = Some(we.0);
            } else if let Some(h) = hole_at(p)
                && let Some(part) = l.parts.iter().find(|x| part_rect(x, to, pitch).expand(pitch * 0.6).contains(p))
            {
                self.drag = Some(Drag { refdes: part.refdes.clone(), from: h });
                self.picked = Some(part.refdes.clone());
            }
        }
        if let Some((k, end)) = self.wire_drag
            && let Some(p) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            && let Some(h) = hole_at(p)
        {
            let m = wire_end_moved(&sch.sch, &l, k, end, h);
            if response.drag_stopped() {
                sch.breadboard = Some(m.clone());
                self.wire_drag = None;
            }
            l = m;
        }
        // the keys of the move tool: R turns the picked part, Delete removes the picked jumper
        if self.tool == Tool::Move && response.hovered() {
            if ui.input(|i| i.key_pressed(egui::Key::R))
                && let Some(r) = &self.picked
            {
                match rotated(&l, r) {
                    Some(m) => {
                        sch.breadboard = Some(m.clone());
                        l = m;
                    }
                    None => sch.status = format!("{r} は、ここでは回せません (IC・電源の列に差した足・ブレッドボードからはみ出す向き)"),
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                && let Some(k) = self.picked_wire.take()
            {
                let m = without_wire(&l, k);
                sch.breadboard = Some(m.clone());
                l = m;
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.wire_start = None;
        }
        let mut dragging = false;
        if let Some(d) = &self.drag
            && let Some(p) = response.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            && let Some(h) = hole_at(p)
        {
            let dcol = h.col() as i32 - d.from.col() as i32;
            // rows count between strip holes (a step across the groove is one row); a rail lead only moves along its rail
            let drow = match (d.from, h) {
                (Hole::Strip { row: a, .. }, Hole::Strip { row: b, .. }) => b as i32 - a as i32,
                _ => 0,
            };
            if let Some(m) = moved(&l, &d.refdes, dcol, drow) {
                if response.drag_stopped() {
                    sch.breadboard = Some(m.clone());
                } else {
                    dragging = true;
                }
                l = m;
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            self.wire_drag = None;
        }
        // clicks of the other tools
        if response.clicked()
            && !on_switch
            && let Some(h) = response.interact_pointer_pos().and_then(hole_at)
        {
            match self.tool {
                Tool::Wire => match self.wire_start.take() {
                    None => self.wire_start = Some(h),
                    Some(a) if a != h => {
                        let m = with_wire(&sch.sch, &l, a, h);
                        sch.breadboard = Some(m.clone());
                        l = m;
                    }
                    Some(_) => {}
                },
                Tool::Meter => {
                    if self.red.is_none() || self.black.is_some() {
                        (self.red, self.black) = (Some(h), None);
                    } else {
                        self.black = Some(h);
                    }
                }
                Tool::Scope => self.scope = Some(h),
                Tool::Move => {}
            }
        }
        let problems = self.board_check(sch, &l);
        // the leads named in a problem, drawn with a red ring
        let mut wrong: BTreeSet<Hole> = BTreeSet::new();
        for p in &problems {
            let names: Vec<String> = match p {
                Problem::Short { at, .. } => at.clone(),
                Problem::Open { apart, .. } => apart.iter().flat_map(|a| a.split(',').map(String::from).collect::<Vec<_>>()).collect(),
                Problem::SameHole { leads, .. } => leads.clone(),
                Problem::Missing { .. } => vec![],
            };
            for n in names {
                let Some((r, pin)) = n.split_once('.') else { continue };
                for part in l.parts.iter().filter(|x| x.refdes == r) {
                    wrong.extend(part.leads.iter().filter(|(p2, _)| p2 == pin).map(|(_, h)| *h));
                }
            }
        }
        // the board
        let board = Rect::from_min_max(origin + Vec2::new(4.3, -0.8) * pitch, origin + Vec2::new(width - 0.3, HEIGHT + 0.8) * pitch);
        painter.rect_filled(board, pitch * 0.4, BOARD);
        painter.rect_filled(Rect::from_min_max(origin + Vec2::new(4.6, 7.6) * pitch, origin + Vec2::new(width - 0.6, 9.4) * pitch), 0.0, Color32::from_rgb(214, 212, 204));
        for (y, c) in [(-0.5, RED), (1.5, BLUE), (14.5, BLUE), (16.5, RED)] {
            painter.line_segment([origin + Vec2::new(4.6, y) * pitch, origin + Vec2::new(width - 0.6, y) * pitch], Stroke::new(1.5, c));
        }
        // the strip (or rail) under the pointer, lit
        let hover = response.hover_pos().and_then(hole_at);
        let lit_node = hover.map(|h| h.node());
        let small = egui::FontId::proportional((pitch * 0.55).clamp(8.0, 12.0));
        for row in 0..10u8 {
            let y = at(Hole::Strip { col: 0, row }).y;
            painter.text(origin + Vec2::new(4.6, y) * pitch, egui::Align2::CENTER_CENTER, ((b'a' + row) as char).to_string(), small.clone(), Color32::from_gray(120));
        }
        for col in 0..l.cols {
            if col % 5 == 4 || col == 0 {
                painter.text(origin + Vec2::new(5.0 + col as f32, 2.0) * pitch, egui::Align2::CENTER_CENTER, (col + 1).to_string(), small.clone(), Color32::from_gray(120));
            }
            let holes = (0..10).map(|row| Hole::Strip { col, row }).chain([Rail::TopPlus, Rail::TopMinus, Rail::BottomMinus, Rail::BottomPlus].map(|rail| Hole::Rail { rail, col }));
            for h in holes {
                let p = to(h);
                let lit = lit_node == Some(h.node());
                if lit {
                    painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.8)), 2.0, LIT.gamma_multiply(0.6));
                }
                painter.rect_filled(Rect::from_center_size(p, Vec2::splat(pitch * 0.32)), 1.0, if lit { Color32::from_gray(20) } else { HOLE });
            }
        }
        for (key, net) in &l.rails {
            let (y, rail) = match key.as_str() {
                "TopPlus" => (0.0, Rail::TopPlus),
                "TopMinus" => (1.0, Rail::TopMinus),
                "BottomMinus" => (15.0, Rail::BottomMinus),
                _ => (16.0, Rail::BottomPlus),
            };
            painter.text(origin + Vec2::new(width - 0.1, y) * pitch, egui::Align2::LEFT_CENTER, net, small.clone(), rail_colour(rail));
        }
        // the wires, then the parts on top
        for (k, w) in l.wires.iter().enumerate() {
            if self.picked_wire == Some(k) {
                draw_wire(&painter, to(w.a), to(w.b), LIT, pitch * 1.8);
            }
            draw_wire(&painter, to(w.a), to(w.b), wire_colour(&w.net, &l), pitch);
        }
        if let (Some(a), Some(p)) = (self.wire_start, response.hover_pos()) {
            draw_wire(&painter, to(a), p, Color32::from_rgb(40, 140, 60).gamma_multiply(0.6), pitch);
        }
        let font = egui::FontId::proportional((pitch * 0.6).clamp(9.0, 14.0));
        for (k, o) in l.off_board.iter().enumerate() {
            let center = origin + Vec2::new(2.0, 3.0 + 4.0 * k as f32) * pitch;
            let r = Rect::from_center_size(center, Vec2::new(3.2, 2.4) * pitch);
            painter.rect(r, 3.0, Color32::from_rgb(60, 64, 72), Stroke::new(1.0, Color32::from_gray(160)), egui::StrokeKind::Middle);
            // a knob turned while it runs shows its value now
            let knob = self.live.as_ref().filter(|_| self.powered).and_then(|v| v.knob_index(&o.refdes).map(|k| v.knobs[k].value));
            let value = match knob {
                Some(v) => format!("{} V", sim_core::format_value(v)),
                None => sch.sch.components.iter().find(|c| c.refdes == o.refdes).map(|c| c.value.clone()).unwrap_or_default(),
            };
            painter.text(center - Vec2::new(0.0, 0.4 * pitch), egui::Align2::CENTER_CENTER, &o.refdes, font.clone(), Color32::WHITE);
            painter.text(center + Vec2::new(0.0, 0.5 * pitch), egui::Align2::CENTER_CENTER, short(&value, 10), small.clone(), Color32::from_gray(200));
            for (i, (pin, net, h)) in o.pins.iter().enumerate() {
                let from = r.right_center() + Vec2::new(0.0, (i as f32 - 0.5) * 0.8 * pitch);
                draw_wire(&painter, from, to(*h), wire_colour(net, &l), pitch);
                painter.text(from + Vec2::new(-0.2 * pitch, 0.0), egui::Align2::RIGHT_CENTER, pin, small.clone(), Color32::WHITE);
            }
        }
        let focus = focus.map(String::from);
        let mut hovered_part = None;
        // with the power on: the LEDs' currents and the parts past their ratings
        let on = self.powered;
        let running = self.live.as_ref().filter(|v| on && v.error.is_none());
        let currents: Vec<(String, f64)> = match (running, &self.board) {
            (Some(lv), _) => l.parts.iter().filter_map(|p| lv.current(&p.refdes).map(|i| (p.refdes.clone(), i))).collect(),
            (None, Some(Board { dc: Ok((c, _)), .. })) if on => c.clone(),
            _ => vec![],
        };
        let issues = self.issues();
        for p in l.parts.iter().filter(|p| sch.sch.components.iter().any(|c| c.refdes == p.refdes && c.kind == sch_core::Kind::Led)) {
            let i = currents.iter().find(|(n, _)| *n == p.refdes).map_or(0.0, |(_, i)| *i);
            // the eye sees an LED bright at a few mA: the glow follows the square root of the current (1 at 20 mA)
            let glow = (i.max(0.0) / 0.02).sqrt().clamp(0.0, 1.3) as f32;
            if glow > 0.05 && !issues.iter().any(|r| r.refdes == p.refdes && r.level == sim_core::RatingLevel::Over) {
                let c = part_rect(p, to, pitch).center();
                for k in 0..4 {
                    painter.circle_filled(c, pitch * (1.0 + 0.6 * k as f32) * glow.max(0.5), Color32::from_rgb(255, 80, 60).gamma_multiply(0.25 * glow.min(1.0)));
                }
            }
        }
        for p in &l.parts {
            let lit = self.picked.as_deref() == Some(p.refdes.as_str()) || focus.as_deref() == Some(p.refdes.as_str()) || self.drag.as_ref().is_some_and(|d| d.refdes == p.refdes);
            let comp = sch.sch.components.iter().find(|c| c.refdes == p.refdes);
            // a potentiometer turned while it runs shows its knob where it is now
            let turned = comp.filter(|c| c.kind == sch_core::Kind::Pot).and_then(|c| {
                let lv = running?;
                let v = lv.knobs.get(lv.knob_index(&c.refdes)?)?.value;
                let mut c = c.clone();
                c.value = sch_core::with_pot_position(&c.value, v);
                Some(c)
            });
            let comp = turned.as_ref().or(comp);
            let rect = draw_part(&painter, p, comp, to, pitch, &font, lit);
            self.drawn.retain(|(r, _)| *r != p.refdes);
            self.drawn.push((p.refdes.clone(), rect));
            if response.hover_pos().is_some_and(|h| rect.contains(h)) {
                hovered_part = Some((p.refdes.clone(), comp.map(|c| c.value.clone()).unwrap_or_default()));
            }
        }
        if let (Some(lv), Some(b)) = (running, &self.board) {
            let sounding = self.buzzers(sch, &l);
            draw_running(&painter, sch, &l, &b.nets, lv, &sounding, to, pitch, &font, ui.input(|i| i.time));
        }
        for r in &issues {
            let Some(p) = l.parts.iter().find(|p| p.refdes == r.refdes) else { continue };
            let rect = part_rect(p, to, pitch);
            if r.level == sim_core::RatingLevel::Over {
                // burnt: soot and a cross
                for k in 0..3 {
                    painter.circle_filled(rect.center() + Vec2::new((k as f32 - 1.0) * pitch * 0.4, -pitch * 0.5 - k as f32 * pitch * 0.2), pitch * (0.5 + 0.15 * k as f32), Color32::from_gray(60).gamma_multiply(0.5));
                }
                let d = pitch * 0.5;
                for (a, b) in [(Vec2::new(-d, -d), Vec2::new(d, d)), (Vec2::new(-d, d), Vec2::new(d, -d))] {
                    painter.line_segment([rect.center() + a, rect.center() + b], Stroke::new(3.0, BAD));
                }
            } else {
                painter.rect_stroke(rect.expand(pitch * 0.2), 3.0, Stroke::new(2.0, crate::theme::WARN), egui::StrokeKind::Middle);
            }
        }
        for h in &wrong {
            painter.circle_stroke(to(*h), pitch * 0.55, Stroke::new(2.5, BAD));
        }
        // what is still to be joined: a dashed line between the nearest leads of a net's separate groups (as a board's ratsnest)
        for (_, a, b) in unjoined(&sch.sch, &l) {
            rats_line(&painter, to(a), to(b), pitch);
        }
        // the probes
        for (h, c, label) in [(self.red, RED, "+"), (self.black, Color32::from_gray(20), "−")] {
            if let (Some(h), Tool::Meter) = (h, self.tool) {
                let p = to(h);
                painter.line_segment([p, p + Vec2::new(pitch * 0.8, -pitch * 1.6)], Stroke::new(pitch * 0.25, c));
                painter.circle_filled(p, pitch * 0.3, c);
                painter.text(p + Vec2::new(pitch * 0.9, -pitch * 1.8), egui::Align2::CENTER_CENTER, label, font.clone(), c);
            }
        }
        if let (Some(h), Tool::Scope) = (self.scope, self.tool) {
            let p = to(h);
            painter.line_segment([p, p + Vec2::new(-pitch * 0.8, -pitch * 1.6)], Stroke::new(pitch * 0.25, LIT));
            painter.circle_filled(p, pitch * 0.3, LIT);
        }
        // what is under the pointer
        if !dragging {
            if let Some((r, v)) = &hovered_part {
                let switch = running.is_some_and(|lv| lv.switch_index(r).is_some());
                let knob = running.is_some_and(|lv| lv.knob_index(r).is_some());
                response.clone().on_hover_text(if switch {
                    i18n::trf!("{r} {v}\nクリックで切り替え (押しボタンは押している間だけ入る)")
                } else if knob {
                    i18n::trf!("{r} {v}\n右か上へドラッグ・ホイールで回します (左の「つまみ」でも)")
                } else {
                    i18n::trf!("{r} {v}\nドラッグで差し替え")
                });
            } else if let Some(h) = hover {
                let here: Vec<String> = l.parts.iter().flat_map(|p| p.leads.iter().filter(|(_, lh)| lh.node() == h.node()).map(move |(pin, _)| format!("{}.{pin}", p.refdes))).collect();
                let tip = if here.is_empty() { h.name() } else { format!("{}: {}", h.name(), here.join(", ")) };
                response.clone().on_hover_text(tip);
            }
        }
        if response.clicked() && self.tool == Tool::Move {
            self.picked = hovered_part.map(|(r, _)| r);
            // a click on a jumper picks it (Delete removes it)
            self.picked_wire = response.interact_pointer_pos().and_then(|p| l.wires.iter().position(|w| wire_points(to(w.a), to(w.b), pitch).iter().any(|q| q.distance(p) < pitch * 0.35)));
        }
        let legend = match self.tool {
            Tool::Move => i18n::tr("黄: ポインタの下の穴とつながっている穴　赤い丸: 回路図と違う足　部品・線の端はドラッグで差し替え (R で回す・Delete で線を消す)"),
            Tool::Wire => i18n::tr("ジャンパー線: 穴をクリックして、つなぐ先の穴をクリック (Esc でやめる)"),
            Tool::Meter => i18n::tr("テスター: 赤い棒 → 黒い棒の順に穴をクリック。電圧は左に出ます"),
            Tool::Scope => i18n::tr("オシロ: 探り針を当てる穴をクリック。波形は左に出ます"),
        };
        painter.text(rect.left_top() + Vec2::new(12.0, 10.0), egui::Align2::LEFT_TOP, legend, egui::FontId::proportional(13.0), Color32::from_gray(210));
    }

    /// The status line's hint.
    pub fn hint(&self) -> String {
        if self.kind != BoardKind::Breadboard {
            return "部品・線の端は穴ごとにドラッグで動かせます。何も無い所から四角で囲むと、中をまとめて動かせます。動かすたびに、左で回路図と照らし合わせます".into();
        }
        "縦の 5 つの穴は中でつながっています。ポインタを穴に合わせると、つながっている穴が光ります。部品をドラッグすると差し替えられ、回路図と違うつなぎ方は赤い丸と左の一覧で分かります".into()
    }
}

/// A ratsnest line: a net's two leads that the board leaves apart, joined by a dashed line with a ring at each end.
pub(super) fn rats_line(painter: &egui::Painter, a: Pos2, b: Pos2, pitch: f32) {
    let c = Color32::from_rgb(255, 214, 60);
    painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new((pitch * 0.12).max(1.2), c), pitch * 0.5, pitch * 0.3));
    for p in [a, b] {
        painter.circle_stroke(p, pitch * 0.32, Stroke::new(1.5, c));
    }
}

/// The marks of the running circuit over the parts: where each switch is, a motor's speed, a buzzer that sounds.
#[allow(clippy::too_many_arguments)]
fn draw_running(painter: &egui::Painter, sch: &SchEditor, l: &Layout, nets: &BoardNets, lv: &Live, sounding: &[(String, f64)], to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, now: f64) {
    let volt = |p: &sch_core::breadboard::Placed, pin: &str| pin_volts(p, pin, nets, lv);
    for p in &l.parts {
        let rect = part_rect(p, to, pitch);
        if let Some(s) = lv.switch_index(&p.refdes).map(|k| &lv.switches[k]) {
            let (c, text) = match (s.push, s.on) {
                (true, true) => (crate::theme::OK, i18n::tr("押している")),
                (true, false) => (Color32::from_gray(90), i18n::tr("押す")),
                (false, true) => (crate::theme::OK, i18n::tr("入")),
                (false, false) => (Color32::from_gray(90), i18n::tr("切")),
            };
            painter.rect_stroke(rect.expand(pitch * 0.25), 3.0, Stroke::new(2.5, c), egui::StrokeKind::Middle);
            // the state on a tag above the part (jumpers may run there)
            let galley = painter.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
            let tag = Rect::from_center_size(rect.center_top() - Vec2::new(0.0, pitch * 0.9), galley.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(tag, 4.0, c);
            painter.galley(tag.center() - galley.size() / 2.0, galley, Color32::WHITE);
            continue;
        }
        let model = sch.sch.components.iter().find(|c| c.refdes == p.refdes).and_then(|c| c.custom.as_ref()).map(|c| c.name.to_ascii_uppercase()).unwrap_or_default();
        if model.contains("MOTOR")
            && let Some(krpm) = volt(p, "SPD")
        {
            // the speed pin is 1 V per 1000 rpm; the spoke turns at a speed the eye can follow
            let rpm = krpm * 1000.0;
            let c = rect.center() - Vec2::new(0.0, pitch * 1.6);
            let turns = now * (rpm / 600.0).clamp(-3.0, 3.0);
            painter.circle(c, pitch * 0.9, Color32::from_rgb(70, 74, 82), Stroke::new(1.0, Color32::from_gray(160)));
            let a = (turns * std::f64::consts::TAU) as f32;
            painter.line_segment([c, c + Vec2::angled(a) * pitch * 0.8], Stroke::new(3.0, Color32::from_rgb(240, 200, 60)));
            painter.text(c + Vec2::new(pitch * 1.1, 0.0), egui::Align2::LEFT_CENTER, format!("{:.0} rpm", rpm), font.clone(), Color32::from_gray(40));
        }
        if model.contains("BUZZER") && sounding.iter().any(|(r, v)| *r == p.refdes && sound::loudness(*v).is_some()) {
            // sound waves, pulsing
            let c = rect.center();
            let k = (now * 4.0).fract() as f32;
            for i in 0..3 {
                let r = pitch * (0.8 + 0.5 * (i as f32 + k));
                painter.circle_stroke(c, r, Stroke::new(1.5, Color32::from_rgb(250, 210, 80).gamma_multiply(1.0 - (i as f32 + k) / 3.0)));
            }
            painter.text(c - Vec2::new(0.0, pitch * 1.4), egui::Align2::CENTER_BOTTOM, "♪", font.clone(), Color32::from_rgb(250, 210, 80));
        }
    }
}

/// A potentiometer's position after the pointer moved from `from` to `to` while it was held (started at `v0`): right or up turns it
/// towards pin 3 (clockwise, as the knob is drawn), a full turn over `KNOB_DRAG` points.
fn knob_dragged(v0: f64, from: Pos2, to: Pos2) -> f64 {
    let d = (to.x - from.x) as f64 + (from.y - to.y) as f64;
    (v0 + d / KNOB_DRAG).clamp(0.0, 1.0)
}

/// The voltage at a part's pin: its lead's hole, or (a simulation-only pin like a motor's speed) the net the board gives the pin.
fn pin_volts(p: &sch_core::breadboard::Placed, pin: &str, nets: &BoardNets, lv: &Live) -> Option<f64> {
    let n = match p.leads.iter().find(|(q, _)| q.eq_ignore_ascii_case(pin)) {
        Some((_, h)) => nets.at(*h)?,
        None => nets.pins.iter().find(|((r, q), _)| *r == p.refdes && q.eq_ignore_ascii_case(pin))?.1.as_str(),
    };
    if is_ground(n) { Some(0.0) } else { lv.voltage(n) }
}

/// A library buzzer (its model's name says so).
fn is_buzzer(sch: &SchEditor, refdes: &str) -> bool {
    sch.sch.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref()).is_some_and(|c| c.name.to_ascii_uppercase().contains("BUZZER"))
}

/// Each buzzer on the board and the voltage of its first lead (+) above its second (−) now.
fn buzzer_volts(sch: &SchEditor, l: &Layout, nets: &BoardNets, lv: &Live) -> Vec<(String, f64)> {
    l.parts
        .iter()
        .filter(|p| is_buzzer(sch, &p.refdes))
        .filter_map(|p| {
            let (a, b) = (&p.leads.first()?.0, &p.leads.get(1)?.0);
            Some((p.refdes.clone(), pin_volts(p, a, nets, lv)? - pin_volts(p, b, nets, lv)?))
        })
        .collect()
}

fn is_ground(n: &str) -> bool {
    matches!(n.to_ascii_uppercase().as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND")
}

/// A small scope screen: the waveform, its highest and lowest value and the time span.
fn scope_plot(ui: &mut egui::Ui, t: &[f64], v: &[f64]) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(380.0), 170.0), egui::Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 4.0, Color32::from_rgb(16, 26, 20));
    for k in 1..5 {
        let y = r.top() + r.height() * k as f32 / 5.0;
        painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(0.5, Color32::from_rgb(40, 70, 50)));
        let x = r.left() + r.width() * k as f32 / 5.0;
        painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(0.5, Color32::from_rgb(40, 70, 50)));
    }
    let (Some(&t0), Some(&t1)) = (t.first(), t.last()) else { return };
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    let pad = ((hi - lo) * 0.1).max(1e-3);
    let (lo, hi) = (lo - pad, hi + pad);
    let pts: Vec<Pos2> = t.iter().zip(v).map(|(t, v)| Pos2::new(r.left() + ((t - t0) / (t1 - t0).max(1e-30)) as f32 * r.width(), r.bottom() - ((v - lo) / (hi - lo)) as f32 * r.height())).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.8, Color32::from_rgb(120, 240, 140))));
    let f = egui::FontId::proportional(12.0);
    painter.text(r.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, format!("{}V", sim_core::format_value(hi - pad)), f.clone(), Color32::from_gray(220));
    painter.text(r.left_bottom() + Vec2::new(4.0, -2.0), egui::Align2::LEFT_BOTTOM, format!("{}V", sim_core::format_value(lo + pad)), f.clone(), Color32::from_gray(220));
    painter.text(r.right_bottom() + Vec2::new(-4.0, -2.0), egui::Align2::RIGHT_BOTTOM, format!("{}s", sim_core::format_value(t1 - t0)), f, Color32::from_gray(220));
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// A jumper wire: a gentle arc between two holes, with its ends.
fn wire_points(a: Pos2, b: Pos2, pitch: f32) -> Vec<Pos2> {
    let mid = a + (b - a) / 2.0 - Vec2::new(0.0, ((b - a).length() * 0.15).min(pitch * 1.5));
    (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            let u = 1.0 - t;
            Pos2::new(u * u * a.x + 2.0 * u * t * mid.x + t * t * b.x, u * u * a.y + 2.0 * u * t * mid.y + t * t * b.y)
        })
        .collect()
}

fn draw_wire(painter: &egui::Painter, a: Pos2, b: Pos2, c: Color32, pitch: f32) {
    let pts = wire_points(a, b, pitch);
    painter.add(egui::Shape::line(pts, Stroke::new((pitch * 0.22).max(2.0), c)));
    for p in [a, b] {
        painter.circle_filled(p, pitch * 0.16, Color32::from_gray(200));
    }
}

fn part_rect(p: &sch_core::breadboard::Placed, to: impl Fn(Hole) -> Pos2, pitch: f32) -> Rect {
    let mut r = Rect::NOTHING;
    for (_, h) in &p.leads {
        r = r.union(Rect::from_center_size(to(*h), Vec2::splat(pitch * 0.9)));
    }
    r
}

/// Whether circuit element `element` belongs to part `refdes`: the part's own element, or one inside it (`VR1.A`).
fn element_of(element: &str, refdes: &str) -> bool {
    element.strip_prefix(refdes).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Draws a part and returns the area it takes (for the pointer).
fn draw_part(painter: &egui::Painter, p: &sch_core::breadboard::Placed, comp: Option<&sch_core::Component>, to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, lit: bool) -> Rect {
    let outline = if lit { Stroke::new(2.5, LIT) } else { Stroke::new(1.0, Color32::from_gray(40)) };
    let kind = comp.map(|c| c.kind);
    let value = comp.map(|c| c.value.as_str()).unwrap_or("");
    let lead = Stroke::new((pitch * 0.12).max(1.5), Color32::from_gray(150));
    match p.look {
        Look::Dip => {
            let r = part_rect(p, to, pitch).shrink(pitch * 0.15);
            let body = Rect::from_min_max(Pos2::new(r.left(), r.top() + pitch * 0.25), Pos2::new(r.right(), r.bottom() - pitch * 0.25));
            for (_, h) in &p.leads {
                let c = to(*h);
                painter.rect_filled(Rect::from_center_size(c, Vec2::splat(pitch * 0.35)), 1.0, Color32::from_gray(190));
            }
            painter.rect(body, 2.0, Color32::from_rgb(34, 34, 38), outline, egui::StrokeKind::Middle);
            painter.circle_filled(Pos2::new(body.left() + pitch * 0.15, body.center().y), pitch * 0.3, BOARD.gamma_multiply(0.6));
            painter.text(body.center(), egui::Align2::CENTER_CENTER, format!("{} {}", p.refdes, short(value, 12)), font.clone(), Color32::WHITE);
            r
        }
        Look::Row => {
            let r = part_rect(p, to, pitch);
            // the body over the groove, below the leads in row e
            let body = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - pitch * 0.2), Pos2::new(r.max.x, r.max.y + pitch * 1.1));
            for (_, h) in &p.leads {
                painter.line_segment([to(*h), Pos2::new(to(*h).x, body.top())], lead);
            }
            if kind == Some(sch_core::Kind::Pot) {
                // a small blue trimmer: the square body, the round knob and its slot pointing where it is turned (from 7 to 5 o'clock)
                let body = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - pitch * 0.2), Pos2::new(r.max.x, r.max.y + pitch * 1.1));
                painter.rect(body, pitch * 0.2, Color32::from_rgb(40, 90, 170), outline, egui::StrokeKind::Middle);
                let c = body.center();
                let rad = body.width().min(body.height()) * 0.42;
                painter.circle(c, rad, Color32::from_gray(235), Stroke::new(1.0, Color32::from_gray(60)));
                let pos = sch_core::pot_position(value) as f32;
                // 0 % at the lower left (pin 1's side), 100 % at the lower right; screen angles run clockwise
                let a = (135.0 + 270.0 * pos).to_radians();
                let d = Vec2::new(a.cos(), a.sin()) * rad * 0.85;
                painter.line_segment([c - d * 0.3, c + d], Stroke::new((pitch * 0.15).max(2.0), Color32::from_gray(40)));
                painter.text(Pos2::new(c.x, body.bottom() + pitch * 0.35), egui::Align2::CENTER_CENTER, format!("{} {:.0}%", p.refdes, pos * 100.0), font.clone(), Color32::from_gray(30));
                return r.union(body);
            }
            let fill = if matches!(kind, Some(sch_core::Kind::Header(_))) { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(40, 40, 44) };
            painter.rect(body, pitch * 0.25, fill, outline, egui::StrokeKind::Middle);
            painter.text(body.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
            r.union(body)
        }
        Look::TwoLead => {
            let (Some((_, a)), Some((_, b))) = (p.leads.first(), p.leads.get(1)) else { return Rect::NOTHING };
            let (pa, pb) = (to(*a), to(*b));
            painter.line_segment([pa, pb], lead);
            let mid = pa + (pb - pa) / 2.0;
            let dir = (pb - pa).normalized();
            let len = ((pb - pa).length() * 0.55).clamp(pitch * 0.8, pitch * 2.2);
            let body = Rect::from_center_size(mid, if dir.x.abs() >= dir.y.abs() { Vec2::new(len, pitch * 0.55) } else { Vec2::new(pitch * 0.55, len) });
            match kind {
                Some(sch_core::Kind::Resistor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(222, 196, 150), outline, egui::StrokeKind::Middle);
                    if let Some(bs) = bands(value) {
                        for (i, c) in bs.iter().enumerate() {
                            let t = 0.25 + 0.17 * i as f32 - 0.5;
                            let at = body.center() + if dir.x.abs() >= dir.y.abs() { Vec2::new(t * body.width() * dir.x.signum(), 0.0) } else { Vec2::new(0.0, t * body.height() * dir.y.signum()) };
                            let size = if dir.x.abs() >= dir.y.abs() { Vec2::new(pitch * 0.12, body.height()) } else { Vec2::new(body.width(), pitch * 0.12) };
                            painter.rect_filled(Rect::from_center_size(at, size), 0.0, *c);
                        }
                    }
                }
                Some(sch_core::Kind::Led) => {
                    painter.circle(mid, pitch * 0.55, Color32::from_rgb(230, 60, 60).gamma_multiply(0.85), outline);
                    // the cathode's flat side
                    let k = to(*b);
                    let n = (k - mid).normalized() * pitch * 0.45;
                    painter.line_segment([mid + n + n.rot90() * 0.9, mid + n - n.rot90() * 0.9], Stroke::new(2.0, Color32::from_gray(40)));
                    painter.text(mid, egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                    return body.union(Rect::from_center_size(mid, Vec2::splat(pitch * 1.2)));
                }
                Some(sch_core::Kind::Diode) => {
                    painter.rect(body, pitch * 0.15, Color32::from_rgb(30, 30, 30), outline, egui::StrokeKind::Middle);
                    // the band at the cathode
                    let k = body.center() + (to(*b) - mid).normalized() * (len * 0.35);
                    let size = if dir.x.abs() >= dir.y.abs() { Vec2::new(pitch * 0.14, body.height()) } else { Vec2::new(body.width(), pitch * 0.14) };
                    painter.rect_filled(Rect::from_center_size(k, size), 0.0, Color32::from_gray(210));
                }
                Some(sch_core::Kind::Capacitor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(90, 140, 210), outline, egui::StrokeKind::Middle);
                }
                Some(sch_core::Kind::Inductor) => {
                    painter.rect(body, pitch * 0.25, Color32::from_rgb(80, 150, 90), outline, egui::StrokeKind::Middle);
                }
                _ => {
                    painter.rect(body, pitch * 0.2, Color32::from_rgb(60, 60, 70), outline, egui::StrokeKind::Middle);
                }
            }
            let label_at = body.center() + if dir.x.abs() >= dir.y.abs() { Vec2::new(0.0, -pitch * 0.75) } else { Vec2::new(pitch * 0.9, 0.0) };
            painter.text(label_at, egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::from_gray(30));
            body.expand(pitch * 0.2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Mode};

    #[test]
    fn the_copper_side_is_the_board_mirrored_and_a_lead_clicked_there_picks_its_part() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.kind = BoardKind::Stripboard;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        let l = app.breadboard.strip_shown(&app.sch);
        let (front, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
        // F over the board turns it over
        let middle = front + Vec2::new(l.cols as f32 / 2.0, l.rows as f32 / 2.0) * pitch;
        let key = egui::Event::Key { key: egui::Key::F, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(middle)]);
        run(&mut app, vec![key]);
        assert!(app.breadboard.strip.back, "F turned the board over");
        run(&mut app, vec![]);
        let (origin, _) = app.breadboard.strip.drawn.unwrap();
        // U1's pin 1, seen from below: its column counted from the right
        let pin1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap().leads.iter().find(|(p, _)| p == "1").unwrap().1;
        let at = origin + Vec2::new((l.cols - 1 - pin1.1) as f32, pin1.0 as f32) * pitch;
        let button = |pressed: bool| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let before = app.sch.stripboard.clone();
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![button(true)]);
        run(&mut app, vec![button(false)]);
        assert_eq!(app.breadboard.picked.as_deref(), Some("U1"), "the lead clicked is U1's pin 1");
        assert_eq!(app.sch.stripboard, before, "nothing is edited on the copper side");
    }

    #[test]
    fn the_checks_kept_between_frames_follow_an_edit_to_the_schematic() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        let (l, strip) = (auto_layout(&app.sch.sch), sch_core::stripboard::strip_layout(&app.sch.sch));
        let bb = |app: &mut App| app.breadboard.board_check(&app.sch, &l).len();
        let rules = |app: &mut App| app.breadboard.strip_checks(&app.sch, &strip).rules.len();
        let (bb0, rules0) = (bb(&mut app), rules(&mut app));
        assert_eq!(bb0, 0, "the breadboard as laid out matches");
        // edits not yet an undo step (the schematic tab commits them later): the edit count is the same, the kept checks are not used
        app.sch.sch.components.iter_mut().find(|c| c.refdes == "R1").unwrap().refdes = "R9".into();
        app.sch.sch.requirements.apart.push(design_spec::KeepApart { name: "test".into(), a: vec!["R2".into()], b: vec!["D1".into()], min_dist: 1000.0, note: String::new() });
        assert!(bb(&mut app) > 0, "R9 is not on the breadboard");
        assert_eq!(rules(&mut app), rules0 + 1, "the rule added is checked");
        app.sch.commit_history_for_test();
        assert!(bb(&mut app) > 0 && rules(&mut app) == rules0 + 1, "the same once it is an undo step");
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        assert!(bb(&mut app) == 0 && rules(&mut app) == rules0, "checked again for the sheet opened");
    }

    fn power_on_app(sample: &str) -> (App, impl FnMut(&mut App)) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join(sample));
        app.mode = Mode::Proto;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let run = move |app: &mut App| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        (app, run)
    }

    fn burnt(app: &App) -> Vec<String> {
        app.breadboard.issues().iter().filter(|r| r.level == sim_core::RatingLevel::Over).map(|r| r.refdes.clone()).collect()
    }

    /// The power switch works the board's DC point out by itself and the circuit runs; the meter reads the supply; an LED whose
    /// resistor is far too small burns.
    #[test]
    fn switching_on_burns_an_led_without_enough_resistance_and_the_meter_reads() {
        let (mut app, mut run) = power_on_app("ne555_blinker.schjson");
        app.breadboard.powered = true;
        run(&mut app);
        assert!(matches!(&app.breadboard.board, Some(Board { dc: Ok(_), .. })), "switching on worked the DC point out");
        assert!(app.breadboard.live.as_ref().is_some_and(|v| v.error.is_none()), "and the circuit runs");
        // the meter on the supply rail against ground reads the supply
        let l = auto_layout(&app.sch.sch);
        app.breadboard.tool = Tool::Meter;
        app.breadboard.red = Some(Hole::Rail { rail: Rail::TopPlus, col: 10 });
        app.breadboard.black = Some(Hole::Rail { rail: Rail::TopMinus, col: 10 });
        run(&mut app);
        assert_eq!(net_at(&app.sch.sch, &l, app.breadboard.red.unwrap()).as_deref(), Some("VCC"));
        assert!(app.breadboard.live.as_ref().and_then(|v| v.voltage("VCC")).is_some_and(|v| v > 4.0));
        // the power-on sample's LED resistor (R1, 330 ohm from 5 V) made 1 ohm: the LED is past its rating and shows as burnt
        let (mut app, mut run) = power_on_app("power_on.schjson");
        let led_r = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
        app.sch.sch.components[led_r].value = "1".into();
        app.breadboard.powered = true;
        run(&mut app);
        assert!(burnt(&app).iter().any(|r| r.starts_with('D')), "{:?}", burnt(&app));
    }

    /// The meter's other ranges: resistance with the power off (R1's two leads read R1 with what is beside it in the circuit, an
    /// empty strip reads OL), and refuses with the power on; current with the power on, which blows the fuse across the supply rails
    /// and reads nothing with both probes on one strip.
    #[test]
    fn the_meter_reads_ohms_with_the_power_off_and_amps_in_series() {
        let (mut app, mut run) = power_on_app("power_on.schjson");
        let l = auto_layout(&app.sch.sch);
        let r1 = l.parts.iter().find(|p| p.refdes == "R1").unwrap();
        let reading = |app: &crate::App| app.breadboard.meter_cache.as_ref().map(|(_, r)| r.clone());
        app.breadboard.tool = Tool::Meter;
        app.breadboard.meter_range = MeterRange::Ohms;
        (app.breadboard.red, app.breadboard.black) = (Some(r1.leads[0].1), Some(r1.leads[1].1));
        run(&mut app);
        let r = reading(&app).unwrap().unwrap();
        let ohms = sim_core::parse_value(r.trim_end_matches('Ω')).unwrap_or_else(|| panic!("{r}"));
        assert!(ohms > 10.0 && ohms <= 331.0, "R1 is 330 ohm, with the circuit beside it no more: {r}");
        // the power on: the resistance range refuses (the reading is not worked out)
        app.breadboard.powered = true;
        app.breadboard.meter_cache = None;
        run(&mut app);
        assert!(reading(&app).is_none());
        // current across the supply rails: the fuse
        app.breadboard.meter_range = MeterRange::Amps;
        let plus = l.parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1)).find(|h| net_at(&app.sch.sch, &l, *h).as_deref() == Some("VIN"));
        let gnd = Hole::Rail { rail: Rail::TopMinus, col: 3 };
        let plus = plus.expect("a lead on VIN");
        (app.breadboard.red, app.breadboard.black) = (Some(plus), Some(gnd));
        run(&mut app);
        assert!(reading(&app).unwrap().unwrap().starts_with("FUSE"), "{:?}", reading(&app));
        // both probes on R1's first strip: nothing flows through the meter
        let same = r1.leads[0].1;
        (app.breadboard.red, app.breadboard.black) = (Some(same), Some(same));
        run(&mut app);
        assert_eq!(reading(&app).unwrap().unwrap(), "SAME");
    }

    /// The board is what is simulated: a jumper across the LED's resistor (not in the schematic) burns the LED, though the
    /// schematic is fine; taking the jumper away again, it lives.
    #[test]
    fn a_jumper_across_the_resistor_burns_the_led_the_schematic_does_not() {
        let (mut app, mut run) = power_on_app("power_on.schjson");
        app.breadboard.powered = true;
        run(&mut app);
        assert!(burnt(&app).is_empty(), "as drawn nothing burns: {:?}", burnt(&app));
        let l = auto_layout(&app.sch.sch);
        let r1 = l.parts.iter().find(|p| p.refdes == "R1").unwrap();
        let (a, b) = (r1.leads[0].1, r1.leads[1].1);
        // a free hole in the strip (or rail) of each lead
        let free = |h: Hole| -> Hole {
            let taken: BTreeSet<Hole> = l.parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1)).chain(l.wires.iter().flat_map(|w| [w.a, w.b])).collect();
            let cands: Vec<Hole> = match h {
                Hole::Strip { col, row } => (if row < 5 { 0..5 } else { 5..10 }).map(|r| Hole::Strip { col, row: r }).collect(),
                Hole::Rail { rail, .. } => (0..l.cols).map(|c| Hole::Rail { rail, col: c }).collect(),
            };
            cands.into_iter().find(|x| !taken.contains(x)).expect("a free hole")
        };
        app.sch.breadboard = Some(with_wire(&app.sch.sch, &l, free(a), free(b)));
        run(&mut app);
        assert!(!check(&app.sch.sch, app.sch.breadboard.as_ref().unwrap()).is_empty(), "the board differs from the schematic");
        assert!(burnt(&app).iter().any(|r| r.starts_with('D')), "the LED burns: {:?}", burnt(&app));
        assert!(app.breadboard.live.as_ref().is_some_and(|v| v.error.is_none()), "and the mis-wired board still runs");
        app.sch.breadboard = Some(l.clone());
        run(&mut app);
        assert!(burnt(&app).is_empty(), "{:?}", burnt(&app));
        // a jumper from the + rail to the − rail: the supply is shorted, and says so
        let (plus, minus) = (Hole::Rail { rail: Rail::TopPlus, col: l.cols - 1 }, Hole::Rail { rail: Rail::TopMinus, col: l.cols - 1 });
        app.sch.breadboard = Some(with_wire(&app.sch.sch, &l, plus, minus));
        run(&mut app);
        let short: Vec<String> = app.breadboard.issues().iter().filter(|r| r.message.contains("ショート")).map(|r| r.refdes.clone()).collect();
        assert!(short.iter().any(|r| r.starts_with('V')), "{:?}", app.breadboard.issues());
    }

    /// The room's temperature slider: warmer, the thermistors' dividers move (the run starts again at the new temperature).
    #[test]
    fn a_warmer_room_moves_the_thermistor_dividers() {
        let (mut app, mut run) = power_on_app("thermistor_basics.schjson");
        app.breadboard.powered = true;
        run(&mut app);
        let nets: BTreeSet<String> = board_nets(&app.sch.sch, &auto_layout(&app.sch.sch)).pins.into_values().collect();
        let read = |app: &App| -> Vec<f64> { nets.iter().map(|n| app.breadboard.live.as_ref().and_then(|v| v.voltage(n)).unwrap_or(0.0)).collect() };
        let cool = read(&app);
        app.breadboard.temp_c = Some(80.0);
        app.breadboard.run_key = None;
        run(&mut app);
        let warm = read(&app);
        assert!(cool.iter().zip(&warm).any(|(a, b)| (a - b).abs() > 0.2), "{cool:?} -> {warm:?}");
    }

    /// The rating check follows the running circuit: the supply's knob turned up past what the LED's resistor allows burns the LED
    /// once it has stayed there a moment; turned back down, the LED stays burnt until the power is switched off.
    #[test]
    fn turning_the_supply_up_burns_the_led_while_it_runs() {
        let (mut app, mut run) = power_on_app("led.schjson");
        let r1 = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
        app.sch.sch.components[r1].value = "180".into();
        // a 1 W resistor: the LED is the part pushed past its rating (an 0805 one would burn open first and save the LED)
        app.sch.sch.components[r1].footprint = "R_2512".into();
        app.breadboard.powered = true;
        run(&mut app);
        assert!(burnt(&app).is_empty(), "about 16 mA at 5 V: {:?}", app.breadboard.issues());
        let turn = |app: &mut App, v: f64| {
            let lv = app.breadboard.live.as_mut().unwrap();
            let k = lv.knob_index("V1").expect("the supply is a knob");
            lv.set_knob(k, v);
        };
        let step = |app: &mut App, run: &mut dyn FnMut(&mut App)| {
            app.breadboard.live.as_mut().unwrap().advance(0.015);
            run(app);
        };
        turn(&mut app, 10.0);
        step(&mut app, &mut run);
        assert!(burnt(&app).is_empty(), "a moment past the rating does not burn it yet");
        for _ in 0..20 {
            step(&mut app, &mut run);
        }
        assert!(burnt(&app).contains(&"D1".to_string()), "{:?}", app.breadboard.issues());
        // the burnt LED opens: the run starts again with it broken, and no current flows through it any more
        run(&mut app);
        assert!(app.breadboard.broken.iter().any(|(r, f)| r == "D1" && *f == sim_core::Fault::Open), "{:?}", app.breadboard.broken);
        step(&mut app, &mut run);
        let i = app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0);
        assert!(i.abs() < 1e-6, "a burnt LED is dark: {i}");
        turn(&mut app, 5.0);
        step(&mut app, &mut run);
        assert!(burnt(&app).contains(&"D1".to_string()), "a burnt LED stays burnt");
        app.breadboard.powered = false;
        run(&mut app);
        app.breadboard.powered = true;
        run(&mut app);
        assert!(burnt(&app).is_empty(), "switched off and on with the knob back: a new start");
        assert!(app.breadboard.broken.is_empty());
    }

    /// With the small (0805) resistor, turning the supply up burns the resistor first: it opens, and the LED behind it survives.
    #[test]
    fn a_resistor_that_burns_open_saves_its_led() {
        let (mut app, mut run) = power_on_app("led.schjson");
        let r1 = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
        app.sch.sch.components[r1].value = "180".into();
        app.breadboard.powered = true;
        run(&mut app);
        {
            let lv = app.breadboard.live.as_mut().unwrap();
            let k = lv.knob_index("V1").unwrap();
            lv.set_knob(k, 10.0);
        }
        for _ in 0..30 {
            app.breadboard.live.as_mut().unwrap().advance(0.015);
            run(&mut app);
        }
        assert!(app.breadboard.broken.iter().any(|(r, f)| r == "R1" && *f == sim_core::Fault::Open), "{:?}", app.breadboard.broken);
        assert!(!burnt(&app).contains(&"D1".to_string()), "{:?}", app.breadboard.issues());
        assert!(app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0).abs() < 1e-6, "no current through the open resistor");
    }

    /// The potentiometer is a knob while it runs: turned towards pin 1 (5 V) the transistor lights the LED, towards pin 3 (GND)
    /// it goes dark.
    /// The buzzer sounds while its push button is held, louder with more voltage, not at all on too little or with the sound off.
    #[test]
    fn the_buzzer_sounds_while_its_button_is_held() {
        let (mut app, mut run) = power_on_app("buzzer_button.schjson");
        app.breadboard.powered = true;
        run(&mut app);
        let step = |app: &App| {
            let l = auto_layout(&app.sch.sch);
            app.breadboard.buzz_step(&app.sch, &l)
        };
        assert_eq!(step(&app), None, "silent until the button is pressed");
        let lv = app.breadboard.live.as_mut().unwrap();
        let k = lv.switch_index("SW1").unwrap();
        lv.press(k);
        lv.advance(0.01);
        run(&mut app);
        let l = auto_layout(&app.sch.sch);
        let v = app.breadboard.buzzers(&app.sch, &l);
        assert!(v.iter().any(|(r, v)| r == "BZ1" && *v > 4.5), "{v:?}");
        assert_eq!(step(&app), Some(4), "loud at 5 V");
        assert!(app.breadboard.beeper.is_playing());
        // the supply turned down to 2.7 V: quieter; to 2 V: silent
        let lv = app.breadboard.live.as_mut().unwrap();
        let s = lv.knob_index("V1").unwrap();
        lv.set_knob(s, 2.7);
        lv.advance(0.01);
        assert_eq!(step(&app), Some(1));
        let lv = app.breadboard.live.as_mut().unwrap();
        lv.set_knob(s, 2.0);
        lv.advance(0.01);
        assert_eq!(step(&app), None);
        let lv = app.breadboard.live.as_mut().unwrap();
        lv.set_knob(s, 5.0);
        lv.advance(0.01);
        app.breadboard.sound_off = true;
        assert_eq!(step(&app), None, "the learner turned the sound off");
        app.breadboard.sound_off = false;
        // released: silent again; leaving the tab stops the tone
        app.breadboard.live.as_mut().unwrap().release();
        app.breadboard.live.as_mut().unwrap().advance(0.01);
        assert_eq!(step(&app), None);
        app.breadboard.live.as_mut().unwrap().press(k);
        app.breadboard.live.as_mut().unwrap().advance(0.01);
        run(&mut app);
        assert!(app.breadboard.beeper.is_playing());
        app.mode = Mode::Schematic;
        run(&mut app);
        assert!(!app.breadboard.beeper.is_playing());
    }

    /// The pot is turned on the board itself: a drag to the right or up turns it on, a wheel notch by 5 %.
    #[test]
    fn the_pot_turns_by_a_drag_on_the_board() {
        assert!((knob_dragged(0.3, Pos2::new(100.0, 100.0), Pos2::new(140.0, 100.0)) - 0.5).abs() < 1e-9, "40 points right: a fifth of a turn");
        assert!((knob_dragged(0.3, Pos2::new(100.0, 100.0), Pos2::new(100.0, 160.0)) - 0.0).abs() < 1e-9, "down past the end stops at 0");
        assert_eq!(knob_dragged(0.9, Pos2::new(0.0, 0.0), Pos2::new(0.0, -100.0)), 1.0);
        // on the running sample, with the pointer: press on VR1 (at 30 %), drag 60 points right, let go, then a wheel notch up
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("pot_dimmer.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.powered = true;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        // on the drawn knob (its lower edge, in the middle gap): away from the leads, where the hover help says to turn it
        let drawn = app.breadboard.drawn.iter().find(|(r, _)| r == "VR1").unwrap().1;
        let at = Pos2::new(drawn.center().x, drawn.bottom() - 3.0);
        let knob = |app: &App| app.breadboard.live.as_ref().and_then(|v| v.knobs.get(v.knob_index("VR1")?)).unwrap().value;
        assert!((knob(&app) - 0.3).abs() < 1e-9);
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![button(at, true)]);
        let to = at + Vec2::new(60.0, 0.0);
        run(&mut app, vec![egui::Event::PointerMoved(to)]);
        run(&mut app, vec![button(to, false)]);
        assert!((knob(&app) - 0.6).abs() < 1e-6, "dragged to {}", knob(&app));
        assert_eq!(app.sch.sch.components.iter().find(|c| c.refdes == "VR1").unwrap().value, "10k POS=30", "the schematic keeps its value");
        assert!(app.sch.breadboard.is_none(), "turning the knob does not move the part");
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: Vec2::new(0.0, 1.0), phase: egui::TouchPhase::Move, modifiers: Default::default() }]);
        assert!((knob(&app) - 0.65).abs() < 1e-6, "a notch up: {}", knob(&app));
    }

    /// On the stripboard, a part dragged by the pointer moves by whole holes and the layout is kept with the sheet; the check then
    /// finds what the move broke; a click of the cut tool cuts a strip; Ctrl+Z and Ctrl+Y undo and redo them; "自動で並べ直す" (None)
    /// gives the automatic layout back.
    #[test]
    fn a_part_dragged_on_the_stripboard_stays_and_is_checked() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.kind = BoardKind::Stripboard;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        let auto = sch_core::stripboard::strip_layout(&app.sch.sch);
        let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
        let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        // a standing two-lead part, pressed on its middle (between its leads, away from the links' ends)
        let part = auto.parts.iter().find(|p| p.leads.len() == 2 && p.leads[0].1 .1 == p.leads[1].1 .1 && p.leads[1].1 .0.abs_diff(p.leads[0].1 .0) >= 2).expect("a standing part");
        let (a, b) = (hole(part.leads[0].1), hole(part.leads[1].1));
        let at = a + (b - a) * 0.5;
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![button(at, true)]);
        let to = at + Vec2::new(pitch * 2.0, 0.0);
        run(&mut app, vec![egui::Event::PointerMoved(to)]);
        run(&mut app, vec![button(to, false)]);
        let saved = app.sch.stripboard.clone().expect("the moved layout is kept");
        let moved = saved.parts.iter().find(|p| p.refdes == part.refdes).unwrap();
        assert_eq!(moved.leads[0].1, (part.leads[0].1 .0, part.leads[0].1 .1 + 2), "two holes right");
        let saved_text = crate::schematic::SheetSettings { stripboard: Some(saved.clone()), ..Default::default() };
        let mut v = serde_json::json!({});
        crate::schematic::with_settings(&mut v, &saved_text);
        assert_eq!(crate::schematic::take_settings(&mut v).stripboard, Some(saved.clone()), "saved with the sheet");
        // the cut tool: a click on a free hole of the bottom row cuts it
        app.breadboard.strip.tool = strip_view::StripTool::Cut;
        let free = (saved.rows - 1, saved.cols - 1);
        run(&mut app, vec![egui::Event::PointerMoved(hole(free))]);
        run(&mut app, vec![button(hole(free), true)]);
        run(&mut app, vec![button(hole(free), false)]);
        assert!(app.sch.stripboard.as_ref().unwrap().cuts.contains(&free));
        // Ctrl+Z takes the cut back, then the move (the automatic layout again); Ctrl+Y does the move again
        let key = |key: egui::Key| vec![egui::Event::ModifiersChanged(egui::Modifiers::COMMAND), egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }];
        run(&mut app, key(egui::Key::Z));
        assert_eq!(app.sch.stripboard.as_ref(), Some(&saved), "the cut is undone");
        run(&mut app, key(egui::Key::Z));
        assert_eq!(app.sch.stripboard, None, "the move is undone");
        run(&mut app, key(egui::Key::Z));
        assert_eq!(app.sch.stripboard, None, "nothing more to undo");
        run(&mut app, key(egui::Key::Y));
        assert_eq!(app.sch.stripboard.as_ref(), Some(&saved), "the move is done again");
        app.sch.stripboard = None;
        run(&mut app, vec![]);
        assert_eq!(app.breadboard.strip_shown(&app.sch), auto);
    }

    /// On the perfboard, L locks the picked part (kept with the sheet, the padlock's mark in the list); "固定した部品のまわりを並べ直す"
    /// then lays the others out again round it: the locked part in the same holes, the lock kept, the board as the schematic joins it.
    #[test]
    fn a_part_locked_on_the_perfboard_stays_when_the_rest_is_laid_out_again() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.kind = BoardKind::Perfboard;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        let auto = app.breadboard.strip_shown(&app.sch);
        let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
        let u1 = auto.parts.iter().find(|p| p.refdes == "U1").unwrap().clone();
        let pts: Vec<Pos2> = u1.leads.iter().map(|(_, a)| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch).collect();
        let at = egui::Rect::from_points(&pts).center();
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![button(at, true)]);
        run(&mut app, vec![button(at, false)]);
        assert_eq!(app.breadboard.picked.as_deref(), Some("U1"));
        run(&mut app, vec![egui::Event::Key { key: egui::Key::L, physical_key: Some(egui::Key::L), pressed: true, repeat: false, modifiers: Default::default() }]);
        let locked = app.sch.stripboard.clone().expect("the lock is kept with the sheet");
        assert!(locked.locked.contains("U1"));
        app.breadboard.strip_relayout(&app.sch, &locked);
        run(&mut app, vec![]);
        let again = app.sch.stripboard.clone().unwrap();
        assert_eq!(again.parts.iter().find(|p| p.refdes == "U1").unwrap().leads, u1.leads, "U1 stays");
        assert!(again.locked.contains("U1"), "still locked");
        assert!(sch_core::stripboard::strip_check(&app.sch.sch, &again).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &again));
    }

    /// On the perfboard, a section's name dragged two rows down: the box is put there (kept with the sheet, marked as put by hand),
    /// the others are packed round it and the board still joins what the schematic joins.
    #[test]
    fn a_section_box_dragged_by_its_name_stays_where_it_is_put() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        app.sch.sch.requirements.groups = vec![group("タイマ", &["U1", "R1", "R2", "C1", "C2"]), group("表示", &["R3", "D1"])];
        app.mode = Mode::Proto;
        app.breadboard.kind = BoardKind::Perfboard;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        let auto = app.breadboard.strip_shown(&app.sch);
        let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
        let (name, a, b) = auto.sections.iter().find(|x| x.0 == "表示").expect("the section's box").clone();
        let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        let at = egui::Rect::from_two_pos(hole(a), hole(b)).expand(pitch * 0.45).left_top() + Vec2::new(pitch * 0.8, -pitch * 0.4);
        let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(&mut app, vec![egui::Event::PointerMoved(at)]);
        run(&mut app, vec![button(at, true)]);
        let to = at + Vec2::new(0.0, pitch * 2.0);
        run(&mut app, vec![egui::Event::PointerMoved(to)]);
        run(&mut app, vec![button(to, false)]);
        run(&mut app, vec![]);
        let put = app.sch.stripboard.clone().expect("the layout round the box is kept with the sheet");
        assert!(put.pinned.contains(&name), "marked as put by hand");
        assert_eq!(put.sections.iter().find(|x| x.0 == name).unwrap().1, (a.0 + 2, a.1), "two rows down");
        assert!(sch_core::stripboard::strip_check(&app.sch.sch, &put).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &put));
    }

    /// On the perfboard, the bare wire tool: a click on a hole and another on its row draws a wire on the copper side, one off its row
    /// and column draws nothing, and a right click on the wire takes it away.
    #[test]
    fn a_bare_wire_is_drawn_and_taken_away_on_the_perfboard() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("seg7_counter.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.kind = BoardKind::Perfboard;
        app.breadboard.strip.tool = strip_view::StripTool::Bare;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
        let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        let click = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), at: sch_core::stripboard::At, button: egui::PointerButton| {
            let p = hole(at);
            run(app, vec![egui::Event::PointerMoved(p)]);
            run(app, vec![egui::Event::PointerButton { pos: p, button, pressed: true, modifiers: Default::default() }]);
            run(app, vec![egui::Event::PointerButton { pos: p, button, pressed: false, modifiers: Default::default() }]);
        };
        // the perfboard's own layout (no strips, its wires already drawn); three free holes in a row that no wire runs over
        let auto = app.breadboard.strip_shown(&app.sch);
        assert!(auto.perf, "the perfboard has a layout of its own");
        let used: Vec<sch_core::stripboard::At> = auto.used().into_iter().map(|(a, _)| a).collect();
        let on_wire = |h: sch_core::stripboard::At| auto.bare.iter().any(|(p, q)| (p.0 == q.0 && h.0 == p.0 && h.1 >= p.1.min(q.1) && h.1 <= p.1.max(q.1)) || (p.1 == q.1 && h.1 == p.1 && h.0 >= p.0.min(q.0) && h.0 <= p.0.max(q.0)));
        let free = |h: sch_core::stripboard::At| !used.contains(&h) && !on_wire(h);
        let (r, c0) = (1..auto.rows).flat_map(|r| (0..auto.cols.saturating_sub(2)).map(move |c| (r, c))).find(|(r, c)| (0..3).all(|i| free((*r, c + i)))).expect("a free run");
        let (a, b, off) = ((r, c0), (r, c0 + 2), (r - 1, c0 + 1));
        click(&mut app, &mut run, a, egui::PointerButton::Primary);
        click(&mut app, &mut run, off, egui::PointerButton::Primary);
        assert!(app.sch.stripboard.is_none(), "not on the start's row or column: nothing drawn");
        click(&mut app, &mut run, b, egui::PointerButton::Primary);
        let drawn = app.sch.stripboard.clone().expect("kept with the sheet");
        assert!(drawn.perf && drawn.bare.len() == auto.bare.len() + 1 && drawn.bare.last() == Some(&(a, b)), "{:?}", drawn.bare.last());
        click(&mut app, &mut run, (r, c0 + 1), egui::PointerButton::Secondary);
        assert_eq!(app.sch.stripboard.as_ref().map(|l| l.bare.len()), Some(auto.bare.len()), "a right click on it takes it away");
        // the stripboard does not show a layout kept for the perfboard
        app.breadboard.kind = BoardKind::Stripboard;
        assert!(!app.breadboard.strip_shown(&app.sch).perf);
    }

    /// The schematic's small window on the proto tab: a click on a part in it picks that part on the board (and the board's pick is
    /// what the window rings); a part moved off its column on the breadboard leaves lines still to join.
    #[test]
    fn the_schematic_window_picks_parts_on_the_board() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        app.breadboard.show_schematic = true;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        run(&mut app, vec![]);
        // the window's place on the screen
        let area = ctx.memory(|m| m.area_rect(egui::Id::new("proto_schematic"))).expect("the window is shown");
        assert!(app.breadboard.show_schematic);
        // click on each part's place until one is picked: the window maps the click back to a part of the schematic
        let mut picked = None;
        'scan: for fy in [0.3f32, 0.4, 0.5, 0.6, 0.7] {
            for fx in [0.2f32, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
                let p = area.min + egui::vec2(area.width() * fx, area.height() * fy);
                let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
                run(&mut app, vec![egui::Event::PointerMoved(p)]);
                run(&mut app, vec![button(true)]);
                run(&mut app, vec![button(false)]);
                if let Some(r) = app.breadboard.picked.clone() {
                    picked = Some(r);
                    break 'scan;
                }
            }
        }
        let r = picked.expect("a click on a part of the schematic picks it");
        assert!(app.sch.sch.components.iter().any(|c| c.refdes == r));
        assert_eq!(app.sch.linked.part.as_deref(), Some(r.as_str()), "the schematic rings the part picked");
    }

    #[test]
    fn turning_the_pot_dims_the_led() {
        let (mut app, mut run) = power_on_app("pot_dimmer.schjson");
        app.breadboard.powered = true;
        run(&mut app);
        let mut led_at = |app: &mut App, pos: f64| {
            let lv = app.breadboard.live.as_mut().unwrap();
            let k = lv.knob_index("VR1").expect("the pot is a knob");
            assert_eq!(lv.knobs[k].range(), 0.0..=1.0);
            lv.set_knob(k, pos);
            lv.advance(0.02);
            run(app);
            app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0)
        };
        let full = led_at(&mut app, 0.0);
        assert!((0.012..0.02).contains(&full), "about 16 mA with the wiper on 5 V: {full}");
        let some = led_at(&mut app, 0.3);
        assert!((0.003..0.01).contains(&some), "dimmer at 30 %: {some}");
        assert!(led_at(&mut app, 1.0).abs() < 1e-4, "dark with the wiper on GND");
        assert!(burnt(&app).is_empty(), "{:?}", app.breadboard.issues());
    }

    /// The LED on the wiper itself (its resistor moved from the emitter to the wiper): turned near pin 1, its current runs through
    /// a short piece of the track, which burns open, and the LED goes dark.
    #[test]
    fn an_led_on_the_wiper_burns_the_pot() {
        let (mut app, mut run) = power_on_app("pot_dimmer.schjson");
        // the label on R1's pin 1 sits at the end of the short wire from the pin
        let r1 = app.sch.sch.components.iter().find(|c| c.refdes == "R1").unwrap().pins().into_iter().find(|(n, _)| n == "1").unwrap().1;
        let end = app.sch.sch.wires.iter().find_map(|w| (w.a == r1).then_some(w.b)).unwrap();
        app.sch.sch.labels.iter_mut().find(|l| l.pos == end).unwrap().text = "W".into();
        app.breadboard.powered = true;
        run(&mut app);
        {
            let lv = app.breadboard.live.as_mut().unwrap();
            let k = lv.knob_index("VR1").unwrap();
            lv.set_knob(k, 0.05);
        }
        for _ in 0..30 {
            app.breadboard.live.as_mut().unwrap().advance(0.015);
            run(&mut app);
        }
        assert!(burnt(&app).contains(&"VR1".to_string()), "{:?}", app.breadboard.issues());
        assert!(app.breadboard.broken.iter().any(|(r, f)| r == "VR1.A" && *f == sim_core::Fault::Open), "the piece from pin 1 to the wiper: {:?}", app.breadboard.broken);
        assert!(app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0).abs() < 1e-6, "no current past the burnt track");
    }

    #[test]
    fn resistor_bands_read_the_value() {
        let b = bands("4.7k").unwrap();
        // yellow, violet, red (x100), gold
        assert_eq!(b[0], Color32::from_rgb(240, 220, 40));
        assert_eq!(b[1], Color32::from_rgb(140, 60, 180));
        assert_eq!(b[2], Color32::from_rgb(210, 40, 40));
        assert_eq!(bands("10").unwrap()[2], Color32::from_rgb(20, 20, 20), "10 = brown black black");
        assert!(bands("x").is_none());
    }

    #[test]
    fn the_tab_draws_a_sample_and_a_drag_is_kept_with_the_sheet() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.mode = Mode::Proto;
        let ctx = egui::Context::default();
        let mut f = eframe::Frame::_new_kittest();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let mut run = |app: &mut App, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
            out.textures_delta.clear();
        };
        run(&mut app, vec![]);
        assert!(app.sch.breadboard.is_none(), "looking does not change the sheet");
        assert!(!app.sch.dirty());
        // the layout kept with the sheet once it is asked for, and read back from the file
        app.sch.breadboard = Some(auto_layout(&app.sch.sch));
        assert!(app.sch.dirty());
        let mut v = serde_json::json!({});
        crate::schematic::with_settings(&mut v, &app.sch.sheet_settings());
        let back = crate::schematic::take_settings(&mut v);
        assert_eq!(back.breadboard, app.sch.breadboard);
        run(&mut app, vec![]);
        // every tool draws
        for t in [Tool::Wire, Tool::Meter, Tool::Scope, Tool::Move] {
            app.breadboard.tool = t;
            run(&mut app, vec![]);
        }
        // an empty schematic says what to do
        let mut empty = App::default();
        empty.mode = Mode::Proto;
        run(&mut empty, vec![]);
    }
}
