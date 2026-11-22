//! The schematic as the browser draws it, with the app's symbols (`sch_core::symbol_shapes`), dragged with a finger or the mouse
//! and zoomed with a pinch or the wheel. Two ways of showing a circuit: the wires coloured by their DC voltage, or a digital
//! circuit running (`sim_core::live_logic`, the app's "デジタル回路を動かしながら見る"): the switches are tapped, the nets are
//! High / Low, the LEDs light.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Stroke, Vec2};
use sch_core::glyphs::{body, body_glyph, seg7_segments};
use sch_core::symbol_shapes::{comp_extents, component_shapes, rot_f, sheet_box, Sh};
use sch_core::{net_at, Component, Kind, PinSide, Pt, Schematic};
use sim_core::live_logic::{InputKind, LiveLogic, GLITCH_SHOW};
use sim_core::logic_level::Level;

const HIGH: Color32 = Color32::from_rgb(255, 96, 80);
const LOW: Color32 = Color32::from_rgb(70, 140, 255);
const MID: Color32 = Color32::from_rgb(235, 200, 60);
const LED_ON: Color32 = Color32::from_rgb(255, 70, 40);
const BG: Color32 = Color32::from_rgb(24, 26, 30);

pub fn level_colour(l: Level) -> Color32 {
    match l {
        Level::High => HIGH,
        Level::Low => LOW,
        Level::Mid => MID,
    }
}

/// The circuit on screen and what the DC solve (or the running digital circuit) says about it.
pub struct Sheet {
    pub sch: Schematic,
    /// The DC voltage of each wire's net (`None`: not solved, or the net has no voltage).
    wire_v: Vec<Option<f64>>,
    /// One voltage per net, written once beside the net's longest wire.
    net_marks: Vec<(Pt, Pt, f64)>,
    v_range: (f64, f64),
    /// Every net's DC voltage, by name (empty: not solved).
    volts: Vec<(String, f64)>,
    pub status: String,
    pub ok: bool,
    /// The digital circuit running, for a sample that opens that way.
    pub live: Option<LiveLogic>,
    /// The clock of egui at the last frame (the running circuit's time follows it).
    last_frame: Option<f64>,
    /// Screen pixels per grid step, and where the grid's (0, 0) is relative to the canvas's top left.
    scale: f32,
    offset: Vec2,
    fitted: bool,
    /// The size of the whole screen at the last fit: a new size (the page loaded, a phone turned) frames the circuit again. Not the
    /// canvas's own size: that changes when the bands above and below it grow (a part picked, the truth table filling), and the
    /// learner's zoom must not jump back then.
    fitted_for: Vec2,
    pub selected: Option<usize>,
}

impl Sheet {
    /// The circuit solved for DC, or (`live`) running as a digital circuit.
    pub fn new(sch: Schematic, live: bool) -> Self {
        let mut s = Sheet {
            sch,
            wire_v: vec![],
            net_marks: vec![],
            v_range: (0.0, 0.0),
            volts: vec![],
            status: String::new(),
            ok: false,
            live: None,
            last_frame: None,
            scale: 12.0,
            offset: Vec2::ZERO,
            fitted: false,
            fitted_for: Vec2::ZERO,
            selected: None,
        };
        if live {
            s.start_live();
        } else {
            s.solve();
        }
        s
    }

    fn start_live(&mut self) {
        match sim_core::from_schematic(&self.sch) {
            Ok(c) => {
                let ll = LiveLogic::new(&self.sch, c, String::new());
                self.ok = ll.error.is_none();
                self.status = match &ll.error {
                    Some(e) => e.clone(),
                    None => i18n::tr("スイッチをタップすると 0 と 1 が切り替わります。線の色が High (赤) と Low (青) です").to_string(),
                };
                self.live = Some(ll);
            }
            Err(e) => self.status = format!("{} {}", i18n::tr("動かせませんでした:"), e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(" / ")),
        }
    }

    /// The DC operating point, put on the wires.
    pub fn solve(&mut self) {
        self.wire_v = vec![None; self.sch.wires.len()];
        self.net_marks.clear();
        self.volts.clear();
        let result = sim_core::from_schematic(&self.sch).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(" / ")).and_then(|c| sim_core::dc_op(&c).map_err(|e| e.to_string()));
        match result {
            Ok(r) => {
                self.volts = r.voltages.clone();
                // the voltage is written once per net, beside its longest wire (a short one ends at a label or a pin, whose text it would cover)
                let mut longest: Vec<(String, i32, usize)> = vec![];
                for (k, w) in self.sch.wires.iter().enumerate() {
                    let Some(net) = net_at(&self.sch, w.a) else { continue };
                    self.wire_v[k] = r.voltage(&net);
                    let len = (w.a.0 - w.b.0).abs() + (w.a.1 - w.b.1).abs();
                    match longest.iter_mut().find(|(n, _, _)| *n == net) {
                        Some(best) if best.1 < len => *best = (net, len, k),
                        Some(_) => {}
                        None => longest.push((net, len, k)),
                    }
                }
                for (_, _, k) in longest {
                    if let Some(v) = self.wire_v[k] {
                        let w = &self.sch.wires[k];
                        self.net_marks.push((w.a, w.b, v));
                    }
                }
                let vs = self.wire_v.iter().flatten().copied();
                self.v_range = (vs.clone().fold(0.0, f64::min), vs.fold(0.0, f64::max));
                self.ok = true;
                self.status = i18n::tr("DC動作点を計算しました。電線の色が電圧です (青 = 低い、赤 = 高い)").to_string();
            }
            Err(e) => {
                self.ok = false;
                self.status = format!("{} {e}", i18n::tr("計算できませんでした:"));
            }
        }
    }

    /// The voltage at a grid point (a pin), when the sheet is solved (or running) and something is connected there.
    pub fn pin_voltage(&self, at: Pt) -> Option<f64> {
        let net = net_at(&self.sch, at)?;
        if let Some(ll) = &self.live {
            return ll.volts.get(&net).copied();
        }
        self.volts.iter().find(|(n, _)| n.eq_ignore_ascii_case(&net)).map(|(_, v)| *v)
    }

    /// Frame the whole circuit again on the next frame.
    pub fn refit(&mut self) {
        self.fitted = false;
    }

    fn color_of(&self, v: Option<f64>) -> Color32 {
        let Some(v) = v else { return Color32::from_gray(150) };
        let (lo, hi) = self.v_range;
        let t = if hi - lo < 1e-9 { 0.0 } else { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 };
        // blue (low) -> green -> red (high)
        let (r, g, b) = if t < 0.5 { (0.0, t * 2.0, 1.0 - t * 2.0) } else { ((t - 0.5) * 2.0, 1.0 - (t - 0.5) * 2.0, 0.0) };
        Color32::from_rgb((70.0 + 185.0 * r) as u8, (90.0 + 150.0 * g) as u8, (90.0 + 165.0 * b) as u8)
    }

    /// The colour of wire `k`: its level while the circuit runs, else its DC voltage.
    fn wire_colour(&self, k: usize) -> Color32 {
        match &self.live {
            Some(ll) => ll.wire_level(k).map_or(Color32::from_gray(150), level_colour),
            None => self.color_of(self.wire_v[k]),
        }
    }

    /// Every point the drawing uses: what "fit to the screen" frames.
    fn points(&self) -> Vec<Pt> {
        let mut pts: Vec<Pt> = self.sch.wires.iter().flat_map(|w| [w.a, w.b]).collect();
        pts.extend(self.sch.components.iter().flat_map(|c| c.pins().into_iter().map(|(_, p)| p).chain([c.pos])));
        pts.extend(self.sch.labels.iter().map(|l| l.pos));
        pts
    }

    pub fn fit(&mut self, area: Vec2) {
        let pts = self.points();
        if pts.is_empty() {
            return;
        }
        // more room on the right: the names are written to the right of the parts
        let (x0, x1) = (pts.iter().map(|p| p.0).min().unwrap() - 3, pts.iter().map(|p| p.0).max().unwrap() + 7);
        let (y0, y1) = (pts.iter().map(|p| p.1).min().unwrap() - 3, pts.iter().map(|p| p.1).max().unwrap() + 3);
        let (w, h) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
        self.scale = (area.x / w).min(area.y / h).clamp(3.0, 80.0);
        let center = Vec2::new((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
        self.offset = area / 2.0 - center * self.scale;
        self.fitted = true;
    }

    /// The running circuit's time follows the screen's; it asks for the next frame while something is moving.
    fn tick(&mut self, ui: &egui::Ui) {
        let Some(ll) = self.live.as_mut() else { return };
        let now = ui.input(|i| i.time);
        let dt = self.last_frame.map_or(0.0, |t| now - t);
        self.last_frame = Some(now);
        ll.tick(dt);
        if ll.running || ll.burst > 0 || ll.held.is_some() || !ll.settled() {
            ui.ctx().request_repaint();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.tick(ui);
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        let screen = ui.ctx().content_rect().size();
        if !self.fitted || (screen - self.fitted_for).length() > 1.0 {
            self.fitted_for = screen;
            self.fit(rect.size());
        }
        // pan with a drag, zoom with a pinch / ctrl + wheel / plain wheel, around the pointer
        if resp.dragged() {
            self.offset += resp.drag_delta();
        }
        if resp.hovered() {
            let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            let zoom = if (zoom - 1.0).abs() > 1e-4 { zoom } else { (scroll.y / 300.0).exp() };
            if (zoom - 1.0).abs() > 1e-4 {
                let at = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()) - rect.min;
                let new_scale = (self.scale * zoom).clamp(3.0, 80.0);
                self.offset = at - (at - self.offset) * (new_scale / self.scale);
                self.scale = new_scale;
            }
        }
        let origin = rect.min.to_vec2() + self.offset;
        let s = self.scale;
        let to_screen = |p: Pt| Pos2::new(p.0 as f32 * s, p.1 as f32 * s) + origin;
        painter.rect_filled(rect, 0.0, BG);

        for (k, w) in self.sch.wires.iter().enumerate() {
            painter.line_segment([to_screen(w.a), to_screen(w.b)], Stroke::new((s * 0.18).max(2.0), self.wire_colour(k)));
        }
        let text = FontId::proportional((s * 0.9).clamp(10.0, 18.0));
        for (k, c) in self.sch.components.iter().enumerate() {
            let body = if self.selected == Some(k) { Color32::from_rgb(255, 210, 90) } else { Color32::from_gray(190) };
            let label_at = draw_part(&painter, c, s, origin, Stroke::new((s * 0.12).max(1.5), body));
            let name = if c.kind.is_power_symbol() { c.value.clone() } else { format!("{} {}", c.refdes, c.value) };
            painter.text(label_at, Align2::LEFT_CENTER, name, text.clone(), Color32::from_gray(220));
        }
        for l in &self.sch.labels {
            painter.text(to_screen(l.pos) + Vec2::new(0.0, -s * 0.4), Align2::LEFT_BOTTOM, &l.text, text.clone(), Color32::from_rgb(140, 200, 255));
        }
        if let Some(ll) = &self.live {
            draw_live(&painter, &self.sch, ll, s, origin);
        } else if self.ok {
            for (a, b, v) in &self.net_marks {
                // left of an upright wire, below a lying one: the names of labels and parts are written to the right and above
                let mid = to_screen(*a).lerp(to_screen(*b), 0.5);
                let (at, anchor) = if a.0 == b.0 { (mid - Vec2::new(s * 0.4, 0.0), Align2::RIGHT_CENTER) } else { (mid + Vec2::new(0.0, s * 0.4), Align2::CENTER_TOP) };
                painter.text(at, anchor, sim_volts(*v), text.clone(), self.color_of(Some(*v)));
            }
        }

        let nearest = |at: Pos2, only: &dyn Fn(&Component) -> bool| {
            self.sch
                .components
                .iter()
                .enumerate()
                .filter(|(_, c)| only(c))
                .map(|(k, c)| (k, to_screen(c.pos).distance(at).min(c.pins().into_iter().map(|(_, p)| to_screen(p).distance(at)).fold(f32::MAX, f32::min))))
                .filter(|(_, d)| *d < s * 3.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k)
        };
        // while the circuit runs, a press on a switch works it (a push button stays pressed while the finger is down)
        if let Some(ll) = self.live.as_mut() {
            if ui.input(|i| i.pointer.any_released()) {
                ll.release_held();
            }
            if resp.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.primary_pressed())
                && let Some(at) = ui.input(|i| i.pointer.interact_pos())
                && let Some(k) = nearest(at, &|c| ll.input_index(&c.refdes).is_some())
                && let Some(i) = ll.input_index(&self.sch.components[k].refdes)
            {
                ll.mouse_on_input(i, true, false);
                ui.ctx().request_repaint();
                return;
            }
        }
        // a tap (or click) elsewhere picks the part nearest to it
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            self.selected = nearest(at, &|_| true);
        }
    }
}

/// A part with its symbol (`sch_core::symbol_shapes`, the same lines the app draws), and the names and numbers of the pins of a
/// box. Returns where its name is written: to the right of the part when it stands upright, above it when it lies.
fn draw_part(painter: &egui::Painter, c: &Component, s: f32, origin: Vec2, stroke: Stroke) -> Pos2 {
    let at = |x: f32, y: f32| {
        let (rx, ry) = rot_f((x, y), c.rot);
        Pos2::new((c.pos.0 as f32 + rx) * s, (c.pos.1 as f32 + ry) * s) + origin
    };
    for shape in component_shapes(c) {
        match shape {
            Sh::L(a, b, x, y) => {
                painter.line_segment([at(a, b), at(x, y)], stroke);
            }
            Sh::C(x, y, r) => {
                painter.circle_stroke(at(x, y), r * s, stroke);
            }
        }
    }
    if let Some(sym) = &c.custom {
        let boxed = body_glyph(sym).and_then(|g| body(g, &sym.layout())).is_none() && !sym.is_two_lead_glyph();
        // the names inside the box and the numbers beside the leads, as the app writes them; only when they can be read
        if boxed && s >= 7.0 {
            let (name_font, number_font) = (FontId::proportional((s * 0.55).clamp(8.0, 14.0)), FontId::proportional((s * 0.45).clamp(7.0, 12.0)));
            for p in &sym.layout().pins {
                let (tx, ty, ex, ey) = (p.tip.0 as f32, p.tip.1 as f32, p.edge.0 as f32, p.edge.1 as f32);
                let inward = match p.side {
                    PinSide::Left => (1.0, 0.0),
                    PinSide::Right => (-1.0, 0.0),
                    PinSide::Top => (0.0, 1.0),
                    PinSide::Bottom => (0.0, -1.0),
                };
                let r = rot_f(inward, c.rot);
                let anchor = if r.0 > 0.5 {
                    Align2::LEFT_CENTER
                } else if r.0 < -0.5 {
                    Align2::RIGHT_CENTER
                } else if r.1 > 0.5 {
                    Align2::CENTER_TOP
                } else {
                    Align2::CENTER_BOTTOM
                };
                painter.text(at(ex + inward.0 * 0.3, ey + inward.1 * 0.3), anchor, &p.name, name_font.clone(), Color32::from_gray(170));
                let off = if matches!(p.side, PinSide::Left | PinSide::Right) { (0.0, -0.45) } else { (0.45, 0.0) };
                painter.text(at((tx + ex) / 2.0 + off.0, (ty + ey) / 2.0 + off.1), Align2::CENTER_CENTER, &p.number, number_font.clone(), Color32::from_gray(130));
            }
        }
    }
    match sheet_box(c.kind, comp_extents(c), c.pos, c.rot) {
        Some((x0, x1, y0, y1)) if x1 - x0 < y1 - y0 => Pos2::new(x1 * s + s * 0.3, (y0 + y1) / 2.0 * s) + origin,
        Some((x0, _, y0, _)) => Pos2::new(x0 * s, y0 * s - s * 0.5) + origin,
        // a ground or power mark: beside its pin
        None => {
            let (_, pin) = c.pins()[0];
            Pos2::new(pin.0 as f32 * s + s * 1.1, pin.1 as f32 * s + if c.kind == Kind::Ground { s * 0.8 } else { -s * 0.5 }) + origin
        }
    }
}

/// The marks of the running circuit over the parts, as the app draws them: the lit LEDs and segments, the glitches, what the
/// flip-flops hold, where each switch is, and 0 / 1 beside the pins of the logic parts.
fn draw_live(painter: &egui::Painter, sch: &Schematic, ll: &LiveLogic, z: f32, origin: Vec2) {
    let to_screen = |p: (f32, f32)| Pos2::new(p.0 * z, p.1 * z) + origin;
    let part = |refdes: &str| sch.components.iter().find(|c| c.refdes == refdes);
    for l in &ll.leds {
        if let Some(c) = part(&l.refdes).filter(|_| l.lit) {
            let p = to_screen((c.pos.0 as f32, c.pos.1 as f32));
            painter.circle_filled(p, 1.6 * z, LED_ON.gamma_multiply(0.35));
            painter.circle_filled(p, 0.8 * z, LED_ON);
        }
    }
    for g in &ll.glitches {
        let fade = (1.0 - (ll.wall - g.seen) / GLITCH_SHOW).clamp(0.0, 1.0) as f32;
        let colour = Color32::from_rgb(255, 80, 80).gamma_multiply(0.25 + 0.5 * fade);
        for (w, net) in sch.wires.iter().zip(&ll.wire_net) {
            if net.as_deref() == Some(g.net.as_str()) {
                painter.line_segment([to_screen((w.a.0 as f32, w.a.1 as f32)), to_screen((w.b.0 as f32, w.b.1 as f32))], Stroke::new(0.6 * z, colour));
            }
        }
    }
    for s in &ll.seg7s {
        let Some(c) = part(&s.refdes) else { continue };
        let at = |p: (f32, f32)| {
            let r = rot_f(p, c.rot);
            to_screen((c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1))
        };
        for (k, (a, b)) in seg7_segments().into_iter().enumerate() {
            if s.lit[k] {
                painter.line_segment([at(a), at(b)], Stroke::new(0.35 * z, LED_ON.gamma_multiply(0.35)));
                painter.line_segment([at(a), at(b)], Stroke::new(0.18 * z, LED_ON));
            }
        }
    }
    let small = FontId::proportional((0.8 * z).clamp(9.0, 14.0));
    for (refdes, bits) in &ll.memory {
        let Some(c) = part(refdes) else { continue };
        let Some(sym) = c.custom.as_ref() else { continue };
        let top = sym.layout().hh as f32 + 0.4;
        let text: String = bits.iter().rev().map(|b| if *b { '1' } else { '0' }).collect();
        let value: u32 = bits.iter().enumerate().map(|(k, b)| (*b as u32) << k).sum();
        let label = if bits.len() >= 3 { i18n::trf!("記憶 {text} (= {value})") } else { i18n::trf!("記憶 {text}") };
        let p = to_screen((c.pos.0 as f32 + 0.5, c.pos.1 as f32 - top));
        let galley = painter.layout_no_wrap(label, small.clone(), LED_ON);
        let at = p - Vec2::new(0.0, galley.size().y);
        painter.rect_filled(egui::Rect::from_min_size(at, galley.size()).expand(2.0), 2.0, BG);
        painter.galley(at, galley, Color32::PLACEHOLDER);
    }
    for inp in &ll.inputs {
        let Some(c) = part(&inp.refdes) else { continue };
        let pins = c.pins();
        let pt = |n: &str| pins.iter().find(|p| p.0 == n).map(|p| to_screen((p.1.0 as f32, p.1.1 as f32)));
        let colour = inp.net.as_deref().and_then(|n| ll.level(n)).map_or(Color32::from_gray(190), level_colour);
        match inp.kind {
            InputKind::Slide => {
                // the lever from COM to the contact it rests on
                if let (Some(com), Some(to)) = (pt("2"), pt(if inp.on { "3" } else { "1" })) {
                    painter.line_segment([com + (to - com) * 0.3, com + (to - com) * 0.7], Stroke::new(0.35 * z, colour));
                }
            }
            InputKind::Push => {
                let p = to_screen((c.pos.0 as f32, c.pos.1 as f32));
                if inp.on {
                    painter.circle_filled(p, 0.5 * z, colour);
                } else {
                    painter.circle_stroke(p, 0.5 * z, Stroke::new(1.5, colour));
                }
            }
        }
    }
    if z >= 8.0 {
        let font = FontId::monospace((0.9 * z).clamp(9.0, 16.0));
        for (ci, pt, net) in &ll.pin_nets {
            let Some(lv) = ll.level(net) else { continue };
            let c = &sch.components[*ci];
            let p = to_screen((pt.0 as f32, pt.1 as f32));
            let away = if pt.0 < c.pos.0 { -1.0 } else { 1.0 };
            // between Low and High: the voltage itself (an RC charging, a divided level), which is neither 0 nor 1
            let text = match lv {
                Level::High => "1".to_string(),
                Level::Low => "0".to_string(),
                Level::Mid => ll.volts.get(net).map_or("X".to_string(), |v| format!("{v:.1}V")),
            };
            painter.text(p + Vec2::new(away * 0.1 * z, -0.6 * z), Align2::CENTER_BOTTOM, text, font.clone(), level_colour(lv));
        }
    }
}

/// `1.23 V`, `-5 V`, `330 mV`.
pub fn sim_volts(v: f64) -> String {
    if v.abs() >= 1.0 || v == 0.0 { format!("{:.3} V", v).replace(".000", "") } else { format!("{:.0} mV", v * 1000.0) }
}
