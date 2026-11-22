//! The schematic as the browser draws it: wires coloured by their DC voltage, parts as simple outlines (the app's symbols come later,
//! shared through a crate of their own), dragged with a finger or the mouse and zoomed with a pinch or the wheel.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use sch_core::{net_at, Kind, Pt, Schematic};

/// The circuit on screen and what the DC solve said about it.
pub struct Sheet {
    pub sch: Schematic,
    /// The DC voltage of each wire's net (`None`: not solved, or the net has no voltage).
    wire_v: Vec<Option<f64>>,
    /// One voltage per net, written once beside the net's first wire.
    net_marks: Vec<(Pt, Pt, f64)>,
    v_range: (f64, f64),
    /// Every net's DC voltage, by name (empty: not solved).
    volts: Vec<(String, f64)>,
    pub status: String,
    pub ok: bool,
    /// Screen pixels per grid step, and where the grid's (0, 0) is relative to the canvas's top left.
    scale: f32,
    offset: Vec2,
    fitted: bool,
    /// The canvas size the last fit was for: a new size (the page loaded, a phone turned) frames the circuit again.
    fitted_for: Vec2,
    pub selected: Option<usize>,
}

impl Sheet {
    pub fn new(sch: Schematic) -> Self {
        let mut s = Sheet { sch, wire_v: vec![], net_marks: vec![], v_range: (0.0, 0.0), volts: vec![], status: String::new(), ok: false, scale: 12.0, offset: Vec2::ZERO, fitted: false, fitted_for: Vec2::ZERO, selected: None };
        s.solve();
        s
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
                let mut seen: Vec<String> = vec![];
                for (k, w) in self.sch.wires.iter().enumerate() {
                    let Some(net) = net_at(&self.sch, w.a) else { continue };
                    let v = r.voltage(&net);
                    self.wire_v[k] = v;
                    if let Some(v) = v
                        && !seen.contains(&net)
                    {
                        seen.push(net);
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

    /// The DC voltage at a grid point (a pin), when the sheet is solved and something is connected there.
    pub fn pin_voltage(&self, at: Pt) -> Option<f64> {
        let net = net_at(&self.sch, at)?;
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
        self.fitted_for = area;
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        if !self.fitted || (rect.size() - self.fitted_for).length() > 1.0 {
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
        painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 26, 30));

        for (k, w) in self.sch.wires.iter().enumerate() {
            painter.line_segment([to_screen(w.a), to_screen(w.b)], Stroke::new((s * 0.18).max(2.0), self.color_of(self.wire_v[k])));
        }
        let text = FontId::proportional((s * 0.9).clamp(10.0, 18.0));
        for (k, c) in self.sch.components.iter().enumerate() {
            let pins: Vec<Pos2> = c.pins().into_iter().map(|(_, p)| to_screen(p)).collect();
            let body = if self.selected == Some(k) { Color32::from_rgb(255, 210, 90) } else { Color32::from_gray(190) };
            let stroke = Stroke::new((s * 0.12).max(1.5), body);
            let label_at = draw_part(&painter, c.kind, &pins, to_screen(c.pos), s, stroke);
            let name = if c.kind.is_power_symbol() { c.value.clone() } else { format!("{} {}", c.refdes, c.value) };
            painter.text(label_at, Align2::LEFT_CENTER, name, text.clone(), Color32::from_gray(220));
        }
        for l in &self.sch.labels {
            painter.text(to_screen(l.pos) + Vec2::new(0.0, -s * 0.4), Align2::LEFT_BOTTOM, &l.text, text.clone(), Color32::from_rgb(140, 200, 255));
        }
        if self.ok {
            for (a, b, v) in &self.net_marks {
                let at = to_screen(*a).lerp(to_screen(*b), 0.5) + Vec2::new(s * 0.3, s * 0.3);
                painter.text(at, Align2::LEFT_TOP, sim_volts(*v), text.clone(), self.color_of(Some(*v)));
            }
        }

        // a tap (or click) picks the part nearest to it
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            self.selected = self
                .sch
                .components
                .iter()
                .enumerate()
                .map(|(k, c)| (k, to_screen(c.pos).distance(at).min(c.pins().into_iter().map(|(_, p)| to_screen(p).distance(at)).fold(f32::MAX, f32::min))))
                .filter(|(_, d)| *d < s * 3.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k);
        }
    }
}

/// A part as a plain outline (two leads: a lead to each pin and a box between them; one pin: a power or ground mark; more: a box
/// around the pins). Returns where its name is written.
fn draw_part(painter: &egui::Painter, kind: Kind, pins: &[Pos2], pos: Pos2, s: f32, stroke: Stroke) -> Pos2 {
    match pins {
        [p] => {
            if kind == Kind::Ground {
                // the bars go on the side of the part's position (away from the wire)
                let d = if pos.y >= p.y { 1.0 } else { -1.0 };
                for (i, half) in [0.9, 0.55, 0.2].iter().enumerate() {
                    let y = p.y + d * (0.3 + i as f32 * 0.3) * s;
                    painter.line_segment([Pos2::new(p.x - half * s, y), Pos2::new(p.x + half * s, y)], stroke);
                }
                painter.line_segment([*p, Pos2::new(p.x, p.y + d * 0.3 * s)], stroke);
                *p + Vec2::new(s * 1.2, d * s * 0.6)
            } else {
                painter.line_segment([Pos2::new(p.x - 0.8 * s, p.y), Pos2::new(p.x + 0.8 * s, p.y)], stroke);
                *p + Vec2::new(s * 1.0, -s * 0.5)
            }
        }
        [a, b] => {
            let (mid, along) = (a.lerp(*b, 0.5), (*b - *a));
            let len = along.length().max(1.0);
            let dir = along / len;
            let normal = Vec2::new(-dir.y, dir.x);
            let half = (len * 0.25).min(1.5 * s);
            let (p, q) = (mid - dir * half, mid + dir * half);
            painter.line_segment([*a, p], stroke);
            painter.line_segment([q, *b], stroke);
            let w = 0.5 * s;
            let corners = vec![p + normal * w, q + normal * w, q - normal * w, p - normal * w];
            let fill = if matches!(kind, Kind::VSource | Kind::ISource) { Color32::from_rgb(60, 50, 30) } else { Color32::from_rgb(40, 44, 52) };
            painter.add(egui::Shape::convex_polygon(corners, fill, stroke));
            // the name beside the box, on the side away from the leads
            mid + if dir.x.abs() > dir.y.abs() { Vec2::new(-half, -s * 1.2) } else { Vec2::new(s * 0.9, 0.0) }
        }
        _ => {
            let r = pins.iter().fold(Rect::from_center_size(pos, Vec2::splat(s)), |r, p| r.union(Rect::from_center_size(*p, Vec2::ZERO))).shrink(s * 0.5);
            painter.rect(r, 2.0, Color32::from_rgb(40, 44, 52), stroke, egui::StrokeKind::Middle);
            Pos2::new(r.left(), r.top() - s * 0.6)
        }
    }
}

/// `1.23 V`, `-5 V`, `330 mV`.
pub fn sim_volts(v: f64) -> String {
    if v.abs() >= 1.0 || v == 0.0 { format!("{:.3} V", v).replace(".000", "") } else { format!("{:.0} mV", v * 1000.0) }
}
