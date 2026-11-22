//! "デジタル回路を動かしながら見る" (候補 B8): a learning mode laid over the schematic. The switches and push buttons are
//! clicked to 0 / 1, the pulse sources become a clock the learner drives (speed, pause, one clock, one event), the nets are
//! painted High / Low / X, the pins of the logic parts carry 0 / 1, the LEDs light, a logic analyser runs to the right, and a
//! truth table fills as the inputs are tried. Slow motion stretches the gate delays (nanoseconds) to seconds.
//!
//! What runs the circuit (the inputs, the clocks, the levels, the LEDs, the truth table) is `sim_core::live_logic`, shared with
//! the browser version; this file draws it over the canvas and keeps what only the screen needs (the logic analyser's view).
//! The design side of the editor is not touched: while the mode is on, the canvas only pans, zooms and clicks the inputs, and a
//! change of the circuit ends the mode.

use super::*;
use crate::logic_plot::{self, Level};
use i18n::{tr, trf};
pub(super) use sim_core::live_logic::{switch_of, InputKind};
use sim_core::live_logic::{Truth, GLITCH_SHOW, HISTORY, HZ_RANGE};

/// The mode as the app shows it: the running circuit, and the logic analyser's view of it.
pub(crate) struct LiveLogic {
    core: sim_core::live_logic::LiveLogic,
    plot: crate::plot::PlotState,
    open_buses: Vec<String>,
    /// The clock of egui at the last frame.
    last_frame: Option<f64>,
}

impl std::ops::Deref for LiveLogic {
    type Target = sim_core::live_logic::LiveLogic;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl std::ops::DerefMut for LiveLogic {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl LiveLogic {
    fn new(sch: &Schematic, circuit: sim_core::Circuit, signature: String) -> LiveLogic {
        LiveLogic { core: sim_core::live_logic::LiveLogic::new(sch, circuit, signature), plot: crate::plot::PlotState::new(HISTORY), open_buses: vec![], last_frame: None }
    }
}

/// A flip-flop's timing violation, in words.
pub(crate) fn race_text(v: &sim_core::TimingViolation) -> String {
    let what = match v.kind {
        sim_core::TimingKind::Setup => tr("セットアップ時間の違反 (クロックの直前にデータが変わった)"),
        sim_core::TimingKind::Hold => tr("ホールド時間の違反 (クロックの直後にデータが変わった)"),
        sim_core::TimingKind::PulseWidth => tr("パルス幅の違反 (クロックが短すぎる)"),
    };
    trf!("{}: {} ({} 回)", v.cell, what, v.count)
}

fn level_colour(l: Level) -> Color32 {
    use crate::canvas_theme::{col, Role};
    match l {
        Level::High => col(Role::LogicHigh),
        Level::Low => col(Role::LogicLow),
        Level::Mid => col(Role::LogicX),
    }
}

impl SchEditor {
    /// Starts "デジタル回路を動かしながら見る" on the circuit on the canvas (the lessons, the samples and the 学ぶ menu).
    pub fn start_live_logic(&mut self) {
        match self.sim_circuit() {
            Ok(c) => {
                let ll = LiveLogic::new(&self.sch, c, self.signature());
                self.status = match &ll.error {
                    Some(e) => e.clone(),
                    None => tr("デジタル回路を動かしています: スイッチをクリックして 0 と 1 を切り替えます。編集に戻るには、上の帯の「終わる」を押します").to_string(),
                };
                self.live_logic = Some(ll);
            }
            Err(errs) => self.status = trf!("動かせませんでした: {}", errs.first().cloned().unwrap_or_default()),
        }
    }

    pub fn stop_live_logic(&mut self) {
        self.live_logic = None;
    }

    pub fn live_logic_active(&self) -> bool {
        self.live_logic.is_some()
    }

    /// For the command line (screenshots): flips the switches named (`SW1,SW2`), and slow motion.
    pub fn live_logic_args(&mut self, flip: &str, slow: bool) {
        if let Some(ll) = self.live_logic.as_mut() {
            for r in flip.split(',').filter(|r| !r.is_empty()) {
                if let Some(i) = ll.input_index(r) {
                    let on = !ll.inputs[i].on;
                    ll.set_input(i, on);
                }
            }
            ll.slow = slow;
            ll.tick(0.05);
        }
    }

    /// The colour of wire `i` while the mode is on.
    pub(super) fn live_wire_colour(&self, i: usize) -> Option<Color32> {
        self.live_logic.as_ref()?.wire_level(i).map(level_colour)
    }

    /// Each frame, before the canvas is drawn: the time goes on; a change of the circuit ends the mode.
    pub(super) fn live_logic_frame(&mut self, ui: &egui::Ui) {
        let Some(ll) = self.live_logic.as_mut() else { return };
        let now = ui.input(|i| i.time);
        let dt = ll.last_frame.map_or(0.0, |t| now - t);
        ll.last_frame = Some(now);
        ll.tick(dt);
        if ll.running || ll.burst > 0 || ll.held.is_some() || !ll.settled() {
            ui.ctx().request_repaint();
        }
        if self.live_logic.as_ref().is_some_and(|ll| ll.signature != self.signature()) {
            self.live_logic = None;
            self.status = tr("回路が変わったので、デジタル回路を動かすのを終えました。もう一度動かすには、学ぶメニューから始めます").to_string();
        }
    }

    /// The marks of the mode over the parts: the lit LEDs, where each switch is, 0 / 1 beside the pins of the logic parts.
    pub(super) fn draw_live_logic(&self, painter: &egui::Painter, rect: Rect) {
        let Some(ll) = self.live_logic.as_ref() else { return };
        let z = self.zoom;
        for l in &ll.leds {
            let Some(c) = self.sch.components.iter().find(|c| c.refdes == l.refdes) else { continue };
            if l.lit {
                let p = self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32));
                let on = crate::canvas_theme::col(crate::canvas_theme::Role::LedOn);
                painter.circle_filled(p, 1.6 * z, on.gamma_multiply(0.35));
                painter.circle_filled(p, 0.8 * z, on);
            }
        }
        // a glitch: the wires of its net glow for a while, fading
        for g in &ll.glitches {
            let fade = (1.0 - (ll.wall - g.seen) / GLITCH_SHOW).clamp(0.0, 1.0) as f32;
            let colour = crate::canvas_theme::col(crate::canvas_theme::Role::Error).gamma_multiply(0.25 + 0.5 * fade);
            for (w, net) in self.sch.wires.iter().zip(&ll.wire_net) {
                if net.as_deref() == Some(g.net.as_str()) {
                    let (a, b) = (self.to_screen(rect, (w.a.0 as f32, w.a.1 as f32)), self.to_screen(rect, (w.b.0 as f32, w.b.1 as f32)));
                    painter.line_segment([a, b], Stroke::new(0.6 * z, colour));
                }
            }
        }
        for s in &ll.seg7s {
            let Some(c) = self.sch.components.iter().find(|c| c.refdes == s.refdes) else { continue };
            let on = crate::canvas_theme::col(crate::canvas_theme::Role::LedOn);
            let at = |p: (f32, f32)| {
                let r = super::symbols::rot_f(p, c.rot);
                self.to_screen(rect, (c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1))
            };
            for (k, (a, b)) in super::glyphs::seg7_segments().into_iter().enumerate() {
                if s.lit[k] {
                    painter.line_segment([at(a), at(b)], Stroke::new(0.35 * z, on.gamma_multiply(0.35)));
                    painter.line_segment([at(a), at(b)], Stroke::new(0.18 * z, on));
                }
            }
        }
        // what the flip-flops of each part hold: the bits (the first cell rightmost), and their number for three or more
        let small = egui::FontId::proportional((0.8 * z).clamp(9.0, 14.0));
        for (part, bits) in &ll.memory {
            let Some(c) = self.sch.components.iter().find(|c| c.refdes == *part) else { continue };
            let Some(sym) = c.custom.as_ref() else { continue };
            let top = sym.layout().hh as f32 + 0.4;
            let text: String = bits.iter().rev().map(|b| if *b { '1' } else { '0' }).collect();
            let value: u32 = bits.iter().enumerate().map(|(k, b)| (*b as u32) << k).sum();
            let label = if bits.len() >= 3 { trf!("記憶 {text} (= {value})") } else { trf!("記憶 {text}") };
            // above the box, right of its middle (clear of a supply lead at the top), on a patch of the canvas's own colour
            let p = self.to_screen(rect, (c.pos.0 as f32 + 0.5, c.pos.1 as f32 - top));
            let galley = painter.layout_no_wrap(label, small.clone(), crate::canvas_theme::col(crate::canvas_theme::Role::LedOn));
            let at = p - egui::vec2(0.0, galley.size().y);
            painter.rect_filled(egui::Rect::from_min_size(at, galley.size()).expand(2.0), 2.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
            painter.galley(at, galley, Color32::PLACEHOLDER);
        }
        for inp in &ll.inputs {
            let Some(c) = self.sch.components.iter().find(|c| c.refdes == inp.refdes) else { continue };
            let pins: Vec<(String, Pt)> = c.pins();
            let pt = |n: &str| pins.iter().find(|p| p.0 == n).map(|p| self.to_screen(rect, (p.1.0 as f32, p.1.1 as f32)));
            let colour = inp.net.as_deref().and_then(|n| ll.level(n)).map_or(crate::canvas_theme::col(crate::canvas_theme::Role::Part), level_colour);
            match inp.kind {
                InputKind::Slide => {
                    // the lever from COM to the contact it rests on
                    if let (Some(com), Some(to)) = (pt("2"), pt(if inp.on { "3" } else { "1" })) {
                        let a = com + (to - com) * 0.3;
                        let b = com + (to - com) * 0.7;
                        painter.line_segment([a, b], Stroke::new(0.35 * z, colour));
                    }
                }
                InputKind::Push => {
                    let p = self.to_screen(rect, (c.pos.0 as f32, c.pos.1 as f32));
                    if inp.on {
                        painter.circle_filled(p, 0.5 * z, colour);
                    } else {
                        painter.circle_stroke(p, 0.5 * z, Stroke::new(1.5, colour));
                    }
                }
            }
        }
        if z >= 8.0 {
            let font = egui::FontId::monospace((0.9 * z).clamp(9.0, 16.0));
            for (ci, pt, net) in &ll.pin_nets {
                let Some(lv) = ll.level(net) else { continue };
                let c = &self.sch.components[*ci];
                let p = self.to_screen(rect, (pt.0 as f32, pt.1 as f32));
                // outside the body: away from the centre of the part
                let away = if pt.0 < c.pos.0 { -1.0 } else { 1.0 };
                // between Low and High: the voltage itself (an RC charging, a divided level), which is neither 0 nor 1
                let text = match lv {
                    Level::High => "1".to_string(),
                    Level::Low => "0".to_string(),
                    Level::Mid => ll.volts.get(net).map_or("X".to_string(), |v| format!("{v:.1}V")),
                };
                painter.text(p + egui::vec2(0.0, -0.6 * z) + egui::vec2(away * 0.1 * z, 0.0), egui::Align2::CENTER_BOTTOM, text, font.clone(), level_colour(lv));
            }
        }
    }

    /// The mouse on the canvas while the mode is on: a press on a switch flips it (a push button stays pressed while held).
    pub(super) fn live_logic_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
        let Some(ll) = self.live_logic.as_mut() else { return };
        let released = ui.input(|i| i.pointer.any_released());
        if released {
            ll.release_held();
        }
        let Some(pos) = resp.hover_pos() else { return };
        let g = self.to_grid(rect, pos);
        let over = self.comp_at(g).and_then(|ci| self.live_logic.as_ref()?.input_index(&self.sch.components[ci].refdes));
        if over.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if let (Some(i), true) = (over, ui.input(|i| i.pointer.primary_pressed()))
            && let Some(ll) = self.live_logic.as_mut()
        {
            ll.mouse_on_input(i, true, ui.input(|i| i.modifiers.shift));
        }
    }

    /// The band of the mode over the top of the canvas, and the logic analyser and truth table along its bottom.
    pub(super) fn live_logic_ui(&mut self, ui: &egui::Ui, rect: Rect) {
        let Some(ll) = self.live_logic.as_mut() else { return };
        let mut stop = false;
        egui::Area::new(egui::Id::new("live_logic_band")).fixed_pos(rect.left_top() + egui::vec2(8.0, 8.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr("デジタル回路を動かす")).strong());
                    let (icon, tip) = if ll.running { ("⏸", tr("一時停止: 時間を止めます。止めている間は「1 クロック」「1 イベント」で少しずつ進めます")) } else { ("▶", tr("動かす: 時間を進めます。クロックはつまみの速さで変わります")) };
                    if ui.button(icon).on_hover_text(tip).clicked() {
                        ll.running = !ll.running;
                    }
                    if ui.add_enabled(!ll.clocks.is_empty(), egui::Button::new(tr("1 クロック"))).on_hover_text(tr("クロックを 1 回 (上がって下がる) だけ進めて、一時停止します。カウンタが 1 つ進むのを確かめられます")).on_disabled_hover_text(tr("この回路にはクロック (パルスの電源) がありません")).clicked() {
                        ll.step_clock();
                    }
                    if ui.button(tr("1 イベント")).on_hover_text(tr("次にゲートの出力が変わる時刻まで進めて、一時停止します。ゲートの遅れ (数 ns) の順に、信号が 1 段ずつ伝わるのを見られます")).clicked() {
                        ll.running = false;
                        ll.step_event();
                    }
                    ui.separator();
                    ui.label(tr("クロック"));
                    ui.add_enabled(!ll.clocks.is_empty(), egui::Slider::new(&mut ll.hz, HZ_RANGE.0..=HZ_RANGE.1).logarithmic(true).suffix(" Hz").max_decimals(1))
                        .on_hover_text(tr("クロックの速さ (1 秒に何回)。目で追える速さにしてあります。実物の回路は MHz で動きます"));
                    ui.checkbox(&mut ll.slow, tr("ゆっくり")).on_hover_text(tr("ゲートの遅れ (10ns ほど) を 1 秒ほどに引き延ばします。入力を変えると、信号がゲートを 1 段ずつ伝わり、一瞬だけ違う値 (グリッチ) が出るのも見えます"));
                    ui.separator();
                    let t = ll.time();
                    let state = if let Some(e) = &ll.error {
                        e.clone()
                    } else if !ll.settled() {
                        trf!("伝わっています (時刻 {})", sim_core::format_time(t))
                    } else if ll.running {
                        trf!("時刻 {}", sim_core::format_time(t))
                    } else {
                        trf!("一時停止中 (時刻 {})", sim_core::format_time(t))
                    };
                    ui.label(state);
                    if ui.button(tr("終わる")).on_hover_text(tr("動かすのをやめて、回路図の編集に戻ります")).clicked() {
                        stop = true;
                    }
                });
                if let Some(g) = ll.glitches.last() {
                    let level = if g.high { "High" } else { "Low" };
                    ui.label(egui::RichText::new(trf!("グリッチ: {} が {} だけ {} になりました (線が赤く光っています)", g.net, sim_core::format_time(g.width), level)).color(crate::theme::WARN))
                        .on_hover_text(tr("ゲートの入力が少しずれて変わると、出力に一瞬だけ違う値が出ます (グリッチ)。この線をクロックやクリアにつなぐと、数えたり消えたりしてしまいます。「ゆっくり」で、どの順に変わったかを見られます"));
                }
                for r in ll.races.iter().take(3) {
                    ui.label(egui::RichText::new(trf!("競合: {}", race_text(r))).color(crate::theme::ERROR))
                        .on_hover_text(tr("フリップフロップは、クロックの前後の決まった時間はデータが変わらないことを求めます。破ると、0 と 1 のどちらを覚えるか決まりません (実物では、どちらになるか分かりません)"));
                }
            });
        });
        // the logic analyser and the truth table
        let lanes_n = logic_plot::lanes(&ll.watched.iter().map(|n| format!("V({n})")).collect::<Vec<_>>().iter().map(|s| s.as_str()).collect::<Vec<_>>()).len();
        let table_h = ll.truth.as_ref().map_or(0.0, |t| 22.0 * ((1usize << t.ins.len()) as f32 + 2.0));
        let h = (logic_plot::height(lanes_n.max(2)) + 16.0).max(table_h).min(rect.height() * 0.45);
        let bottom = Rect::from_min_max(egui::pos2(rect.left() + 8.0, rect.bottom() - h - 8.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0));
        egui::Area::new(egui::Id::new("live_logic_bottom")).fixed_pos(bottom.min).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(bottom.width() - 16.0);
                ui.horizontal(|ui| {
                    let table_w = if ll.truth.is_some() { 60.0 + 34.0 * (ll.truth.as_ref().map_or(0, |t| t.ins.len() + t.outs.len()) as f32) } else { 0.0 };
                    let plot_w = (bottom.width() - 32.0 - table_w).max(120.0);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(plot_w, h - 16.0), Sense::hover());
                    let LiveLogic { core, plot, open_buses, .. } = &mut *ll;
                    let names: Vec<String> = core.watched.iter().map(|n| format!("V({n})")).collect();
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    let lanes = logic_plot::lanes(&refs);
                    let rows = logic_plot::rows(&lanes, open_buses);
                    let signals: Vec<(&str, &[f64])> = names.iter().zip(&core.hist_v).map(|(n, v)| (n.as_str(), v.as_slice())).collect();
                    // the window follows the present moment, like a logic analyser that keeps scrolling
                    plot.x = ((core.wall - 10.0).max(0.0), core.wall.max(10.0));
                    plot.full_x = plot.x;
                    logic_plot::draw(ui, r, egui::Id::new("live_logic_plot"), &core.hist_t, &rows, open_buses, &signals, logic_plot::thresholds(core.vdd), logic_plot::Radix::Hex, &[], plot);
                    if let Some(t) = &core.truth {
                        ui.vertical(|ui| truth_table(ui, t));
                    }
                });
            });
        });
        if stop {
            self.stop_live_logic();
        }
    }
}

/// The truth table, a row per combination; the rows not tried yet are blank, a cell that has shown both 0 and 1 is "記憶".
fn truth_table(ui: &mut egui::Ui, t: &Truth) {
    let n = t.ins.len();
    let head = if t.complete() { tr("真理値表 (全部埋まりました)").to_string() } else { trf!("真理値表 ({}/{} 行)", t.rows.len(), 1 << n) };
    ui.label(egui::RichText::new(head).strong()).on_hover_text(tr("入力の組み合わせを全部試すと、表が完成します。同じ入力で前と違う出力になった所は「記憶」(前の状態で決まる) と出ます"));
    egui::Grid::new("live_truth").striped(true).spacing(egui::vec2(10.0, 0.0)).show(ui, |ui| {
        for name in t.ins.iter().chain(&t.outs) {
            ui.label(egui::RichText::new(name).monospace().strong());
        }
        ui.end_row();
        for key in 0..(1u32 << n) {
            for b in (0..n).rev() {
                ui.label(egui::RichText::new(if key >> b & 1 == 1 { "1" } else { "0" }).monospace());
            }
            match t.rows.get(&key) {
                Some(outs) => {
                    for o in outs {
                        let (s, c) = match o {
                            Some(true) => ("1", level_colour(Level::High)),
                            Some(false) => ("0", level_colour(Level::Low)),
                            None => ("記憶", level_colour(Level::Mid)),
                        };
                        ui.label(egui::RichText::new(tr(s)).monospace().color(c));
                    }
                }
                None => {
                    for _ in &t.outs {
                        ui.label("·");
                    }
                }
            }
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor starts the mode on the sample it has open, and a switch set through it lights the LED it drives (the circuit
    /// itself is tested in `sim_core::live_logic`).
    #[test]
    fn the_editor_runs_the_half_adder() {
        let mut e = SchEditor::default();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        e.load_path(root.join("half_adder.schjson"));
        e.start_live_logic();
        let ll = e.live_logic.as_mut().expect("the mode is on");
        assert!(ll.error.is_none(), "{}", e.status);
        ll.set_input(0, true);
        ll.tick(0.05);
        assert_eq!(ll.leds.iter().map(|l| l.lit).collect::<Vec<_>>(), [true, false], "A=1 B=0: S on, C off");
    }
}
