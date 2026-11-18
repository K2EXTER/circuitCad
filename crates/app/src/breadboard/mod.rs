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
use sch_core::breadboard::{auto_layout, board_nets, check, moved, net_at, rotated, with_wire, without_wire, wire_end_moved, BoardNets, Hole, Layout, Look, Problem, Rail};
use std::collections::BTreeSet;

mod live;
use live::Live;

/// What the person is dragging: the part, and the hole the pointer started on.
struct Drag {
    refdes: String,
    from: Hole,
}

/// What a click on the board does.
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
    /// Switches to move once the circuit starts running (`--proto-flip SW1,SW2`, for screenshots).
    pub flip_on_start: Vec<String>,
}

/// The DC point of the board's circuit: the current through each part, and the parts past (or near) their ratings.
pub type BoardDc = (Vec<(String, f64)>, Vec<sim_core::RatingIssue>);

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
        let l = self.layout(sch);
        crate::widgets::section_heading(ui, i18n::tr("ブレッドボード"));
        ui.label(spaced("回路図の部品を、ブレッドボードのどの穴に差すかを並べました。縦に並んだ 5 つの穴 (a〜e、f〜j) は中でつながっていて、上下の赤と青の列は電源とGNDです。部品をドラッグすると差し替えられます。").color(NOTE));
        let problems = check(&sch.sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            ui.colored_label(BAD, i18n::trf!("回路図と違う所が {} 件あります", problems.len()));
            for p in &problems {
                ui.label(egui::RichText::new(format!("・{}", i18n::tr_dyn(&p.message()))).color(BAD));
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (t, name, tip) in [
                (Tool::Move, "動かす", "部品やジャンパー線の端をドラッグして差し替えます。選んだ部品は R で回り、選んだ線は Delete で消えます"),
                (Tool::Wire, "ジャンパー線", "穴をクリックして、つなぐ先の穴をクリックすると、線を差します。Esc でやめます"),
                (Tool::Meter, "テスター", "1 回目のクリックで赤い棒、2 回目で黒い棒を穴に当てます。2 つの穴の間の直流の電圧を読みます (黒い棒はふつう GND に)"),
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
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("bb_parts").num_columns(2).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                for p in &l.parts {
                    let picked = self.picked.as_deref() == Some(p.refdes.as_str());
                    if ui.selectable_label(picked, egui::RichText::new(&p.refdes).strong()).on_hover_text(i18n::tr("選ぶと、ブレッドボードの上でこの部品が光ります")).clicked() {
                        self.picked = if picked { None } else { Some(p.refdes.clone()) };
                    }
                    ui.label(p.leads.iter().map(|(pin, h)| format!("{pin}:{}", h.name())).collect::<Vec<_>>().join("  "));
                    ui.end_row();
                }
            });
            if !l.off_board.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ブレッドボードの外 (電源・電池・信号源)")).strong()).on_hover_text(i18n::tr("実験用の電源・電池・発振器などは、ブレッドボードの外に置いて、線でつなぎます"));
                for o in &l.off_board {
                    ui.label(format!("{}  {}", o.refdes, o.pins.iter().map(|(pin, net, h)| format!("{pin} ({net}) → {}", h.name())).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// The parts past (or near) their ratings on the board's circuit with the power on.
    fn issues(&self) -> Vec<sim_core::RatingIssue> {
        match &self.board {
            Some(Board { dc: Ok((_, r)), .. }) if self.powered => r.clone(),
            _ => vec![],
        }
    }

    /// With the power on: the circuit as the board joins it is worked out (again when the schematic or the layout changes), its DC
    /// point found, and it runs in time; each frame the time goes on, and the panel shows the time, the speed and the switches.
    fn live_frame(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout, matches: bool) {
        if !self.powered {
            (self.live, self.board, self.run_key, self.live_failed) = (None, None, None, None);
            self.switch_down = false;
            return;
        }
        if self.run_key.as_ref().is_none_or(|(r, k)| *r != sch.rev || k != l) {
            self.run_key = Some((sch.rev, l.clone()));
            let speed = self.live.as_ref().map_or(1.0, |v| v.speed);
            let on: Vec<String> = self.live.as_ref().map(|v| v.switches.iter().filter(|s| s.on && !s.push).map(|s| s.refdes.clone()).collect()).unwrap_or_default();
            self.live = None;
            self.live_failed = None;
            let nets = board_nets(&sch.sch, l);
            let dc = sch.board_dc(&nets);
            match sch.board_circuit(&nets) {
                Ok(c) => {
                    let leds: Vec<String> = sch.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
                    let mut lv = Live::start(c, &leds, sch.live_switches());
                    lv.speed = speed;
                    // the slide switches stay where they were when the board is changed
                    for r in on.into_iter().chain(std::mem::take(&mut self.flip_on_start)) {
                        if let Some(k) = lv.switch_index(&r) {
                            lv.press(k);
                        }
                    }
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
        let problems = check(&sch.sch, &l);
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
            let value = sch.sch.components.iter().find(|c| c.refdes == o.refdes).map(|c| c.value.clone()).unwrap_or_default();
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
            let rect = draw_part(&painter, p, comp, to, pitch, &font, lit);
            if response.hover_pos().is_some_and(|h| rect.contains(h)) {
                hovered_part = Some((p.refdes.clone(), comp.map(|c| c.value.clone()).unwrap_or_default()));
            }
        }
        if let (Some(lv), Some(b)) = (running, &self.board) {
            draw_running(&painter, sch, &l, &b.nets, lv, to, pitch, &font, ui.input(|i| i.time));
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
                response.clone().on_hover_text(if switch { i18n::trf!("{r} {v}\nクリックで切り替え (押しボタンは押している間だけ入る)") } else { i18n::trf!("{r} {v}\nドラッグで差し替え") });
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
        "縦の 5 つの穴は中でつながっています。ポインタを穴に合わせると、つながっている穴が光ります。部品をドラッグすると差し替えられ、回路図と違うつなぎ方は赤い丸と左の一覧で分かります".into()
    }
}

/// The marks of the running circuit over the parts: where each switch is, a motor's speed, a buzzer that sounds.
#[allow(clippy::too_many_arguments)]
fn draw_running(painter: &egui::Painter, sch: &SchEditor, l: &Layout, nets: &BoardNets, lv: &Live, to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, now: f64) {
    let volt = |p: &sch_core::breadboard::Placed, pin: &str| -> Option<f64> {
        let h = p.leads.iter().find(|(q, _)| q.eq_ignore_ascii_case(pin))?.1;
        let n = nets.at(h)?;
        if is_ground(n) { Some(0.0) } else { lv.voltage(n) }
    };
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
            painter.text(c + Vec2::new(pitch * 1.1, 0.0), egui::Align2::LEFT_CENTER, format!("{:.0} rpm", rpm), font.clone(), Color32::from_gray(230));
        }
        if model.contains("BUZZER") {
            let v = match (p.leads.first(), p.leads.get(1)) {
                (Some((a, _)), Some((b, _))) => volt(p, a).zip(volt(p, b)).map(|(x, y)| (x - y).abs()),
                _ => None,
            };
            if v.is_some_and(|v| v > 1.0) {
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
