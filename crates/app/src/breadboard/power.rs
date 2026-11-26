//! The board with the power on: the ratings, the live run (`live.rs`), the buzzers, the LEDs lit and the knobs turned.

use super::*;

impl BreadboardView {
    /// With the power on: the circuit as the board joins it is worked out (again when the schematic or the layout changes), its DC
    /// point found, and it runs in time; each frame the time goes on, and the panel shows the time, the speed and the switches.
    pub(super) fn live_frame(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &RunBoard, matches: bool) {
        self.run_frame(ui, sch, l, matches);
        let step = self.buzz_step(sch, l);
        self.beeper.set(step);
    }

    /// Stops the buzzers' tone (the tab is left).
    pub fn silence(&mut self) {
        self.beeper.set(None);
    }

    /// The buzzers on the board with the voltage from their + to their − now (not the burnt ones).
    pub(super) fn buzzers(&self, sch: &SchEditor, l: &RunBoard) -> Vec<(String, f64)> {
        let (Some(lv), Some(b)) = (self.live.as_ref().filter(|v| self.powered && v.error.is_none()), self.board.as_ref()) else { return vec![] };
        buzzer_volts(sch, l, &b.nets, lv).into_iter().filter(|(r, _)| !self.broken.iter().any(|(e, _)| element_of(e, r))).collect()
    }

    /// How loud the tone is: the loudest buzzer sounding, or None (none sounds, or the sound is off).
    /// The board's differences from the schematic (`check`), kept while the schematic and the layout stay the same.
    pub(super) fn board_check(&mut self, sch: &SchEditor, l: &Layout) -> Vec<Problem> {
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

    pub(super) fn buzz_step(&self, sch: &SchEditor, l: &RunBoard) -> Option<u8> {
        if self.sound_off {
            return None;
        }
        self.buzzers(sch, l).into_iter().filter_map(|(_, v)| sound::loudness(v)).max()
    }

    pub(super) fn run_frame(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &RunBoard, matches: bool) {
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
            let nets = l.nets(&sch.sch);
            let dc = sch.board_dc(&nets, self.temp_c);
            // the parts burnt so far, broken in the circuit
            let built = sch.board_circuit(&nets, self.temp_c).map(|c| self.broken.iter().fold(c, |c, (r, f)| sim_core::with_fault(&c, r, *f).unwrap_or(c)));
            match built {
                Ok(c) => {
                    let leds: Vec<String> = sch.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
                    // the knobs: the DC voltage sources off the board (a bench supply, the light or the signal on a sensor's input)
                    // and the potentiometers on the board
                    let kind_of = |r: &str| sch.sch.components.iter().find(|c| c.refdes == r).map(|c| c.kind);
                    let mut knobs: Vec<String> = l.off().into_iter().filter(|r| kind_of(r) == Some(sch_core::Kind::VSource)).collect();
                    knobs.extend(l.on().into_iter().map(|(r, _)| r).filter(|r| kind_of(r) == Some(sch_core::Kind::Pot)));
                    let mut lv = Live::start(c, &leds, sch.live_switches(), &knobs);
                    lv.speed = speed;
                    // the microcontroller boards with their sketches (B6 ⑤)
                    for refdes in sketch_ui::boards(&sch.sch) {
                        let pins = sketch_ui::pin_nets(&sch.sch, &refdes, &nets);
                        if sch.sketch_real.contains(&refdes) {
                            // the real chip (W5 (b)): it starts when arduino-cli has built its program (`give_real_programs`)
                            lv.add_real_mcu(refdes.clone(), pins);
                        } else {
                            lv.add_mcu(refdes.clone(), sim_core::sketch::compile(&sketch_ui::sketch_of(sch, &refdes)), pins);
                        }
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
            ui.label(egui::RichText::new(i18n::tr("回路図と違う差し方のまま動かしています (計算はブレッドボードに差したとおり)")).color(crate::theme::warn()))
                .on_hover_text(i18n::tr("実物と同じで、違う差し方なら違う動きをします。ショートした所には大きな電流が流れ、外れた所には流れません"));
        }
        if let Some(e) = &self.live_failed {
            ui.colored_label(crate::theme::warn(), i18n::trf!("回路を動かせませんでした: {}", i18n::tr_dyn(e)));
            return;
        }
        // a real chip whose program has just been built starts now
        self.give_real_programs(ui.ctx(), sch);
        let Some(lv) = self.live.as_mut() else { return };
        if let Some(e) = &lv.error {
            ui.colored_label(crate::theme::warn(), i18n::tr_dyn(e));
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
            ui.label(egui::RichText::new(i18n::trf!("動いています  {} s", format!("{:.2}", lv.time()))).color(crate::theme::ok()));
            ui.label(i18n::tr("速さ")).on_hover_text(i18n::tr("回路の時間の進み方です。速い回路 (kHz の発振など) は、遅くすると目で追えます"));
            egui::ComboBox::from_id_salt("bb_speed").selected_text(i18n::tr_dyn(live::SPEEDS.iter().find(|(v, _)| *v == lv.speed).map_or("実時間", |(_, n)| *n))).show_ui(ui, |ui| {
                for (v, name) in live::SPEEDS {
                    ui.selectable_value(&mut lv.speed, v, i18n::tr_dyn(name));
                }
            });
        });
        if l.on().iter().any(|(r, _)| is_buzzer(sch, r)) {
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
            ui.label(egui::RichText::new(i18n::tr("計算が画面の時間に追いついていません (この回路は実時間では重すぎます)。速さを遅くします")).color(crate::theme::warn()));
        }
        if !lv.switches.is_empty() {
            ui.label(egui::RichText::new(i18n::tr("スイッチ・押しボタンは、ブレッドボードの上でクリックします (押しボタンは押している間だけ入ります)")).color(NOTE));
            ui.horizontal_wrapped(|ui| {
                for s in &lv.switches {
                    let state = if s.on { i18n::tr("入") } else { i18n::tr("切") };
                    ui.label(egui::RichText::new(format!("{} {state}", s.refdes)).color(if s.on { crate::theme::ok() } else { NOTE }));
                }
            });
        }
    }
}

/// The marks of the running circuit over the parts: where each switch is, a motor's speed, a buzzer that sounds.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_running(painter: &egui::Painter, sch: &SchEditor, l: &Layout, nets: &BoardNets, lv: &Live, sounding: &[(String, f64)], to: impl Fn(Hole) -> Pos2 + Copy, pitch: f32, font: &egui::FontId, now: f64) {
    let volt = |p: &sch_core::breadboard::Placed, pin: &str| pin_volts(p, pin, nets, lv);
    for p in &l.parts {
        let rect = part_rect(p, to, pitch);
        if let Some(s) = lv.switch_index(&p.refdes).map(|k| &lv.switches[k]) {
            let (c, text) = match (s.push, s.on) {
                (true, true) => (crate::theme::ok(), i18n::tr("押している")),
                (true, false) => (Color32::from_gray(90), i18n::tr("押す")),
                (false, true) => (crate::theme::ok(), i18n::tr("入")),
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
pub(super) fn knob_dragged(v0: f64, from: Pos2, to: Pos2) -> f64 {
    let d = (to.x - from.x) as f64 + (from.y - to.y) as f64;
    (v0 + d / KNOB_DRAG).clamp(0.0, 1.0)
}

/// The voltage at a part's pin: its lead's hole, or (a simulation-only pin like a motor's speed) the net the board gives the pin.
pub(super) fn pin_volts(p: &sch_core::breadboard::Placed, pin: &str, nets: &BoardNets, lv: &Live) -> Option<f64> {
    let n = match p.leads.iter().find(|(q, _)| q.eq_ignore_ascii_case(pin)) {
        Some((_, h)) => nets.at(*h)?,
        None => nets.pins.iter().find(|((r, q), _)| *r == p.refdes && q.eq_ignore_ascii_case(pin))?.1.as_str(),
    };
    if is_ground(n) { Some(0.0) } else { lv.voltage(n) }
}

/// A library buzzer (its model's name says so).
pub(super) fn is_buzzer(sch: &SchEditor, refdes: &str) -> bool {
    sch.sch.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref()).is_some_and(|c| c.name.to_ascii_uppercase().contains("BUZZER"))
}

/// Each buzzer on the board and the voltage of its first lead (+) above its second (−) now.
pub(super) fn buzzer_volts(sch: &SchEditor, l: &RunBoard, nets: &BoardNets, lv: &Live) -> Vec<(String, f64)> {
    let volts = |r: &str, pin: &str| {
        let n = nets.pins.get(&(r.to_string(), pin.to_string()))?;
        if is_ground(n) { Some(0.0) } else { lv.voltage(n) }
    };
    l.on()
        .into_iter()
        .filter(|(r, _)| is_buzzer(sch, r))
        .filter_map(|(r, pins)| {
            let v = volts(&r, pins.first()?)? - volts(&r, pins.get(1)?)?;
            Some((r, v))
        })
        .collect()
}
