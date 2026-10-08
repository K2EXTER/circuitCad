//! A small waveform plot: linked x axis (linear time or logarithmic frequency), one y range
//! per group, pan / zoom / readout.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};

pub struct Trace<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub t: &'a [f64],
    pub v: &'a [f64],
}

#[derive(Clone, Debug)]
pub struct PlotState {
    pub x: (f64, f64),
    pub full_x: (f64, f64),
    /// Logarithmic x axis (frequency sweeps); `x` must then be positive.
    pub log_x: bool,
    /// One y range per group (voltage, current / magnitude, phase).
    pub y: [(f64, f64); 2],
    pub auto_y: [bool; 2],
}

impl PlotState {
    pub fn new(tstop: f64) -> Self {
        Self { x: (0.0, tstop), full_x: (0.0, tstop), log_x: false, y: [(-1.0, 1.0); 2], auto_y: [true; 2] }
    }

    pub fn log(f0: f64, f1: f64) -> Self {
        Self { x: (f0, f1), full_x: (f0, f1), log_x: true, y: [(-1.0, 1.0); 2], auto_y: [true; 2] }
    }
}

/// How one group of traces is labelled.
pub struct Style<'a> {
    /// Unit appended to y tick labels.
    pub unit: &'a str,
    /// Unit appended to x tick labels ("s", "Hz").
    pub x_unit: &'a str,
    /// Name of the x quantity in the readout ("t", "f").
    pub x_name: &'a str,
    /// No SI prefixes on y labels (dB, degrees).
    pub plain_y: bool,
}

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(240, 200, 60),
    Color32::from_rgb(70, 200, 230),
    Color32::from_rgb(230, 90, 200),
    Color32::from_rgb(110, 220, 110),
    Color32::from_rgb(250, 140, 60),
    Color32::from_rgb(140, 160, 255),
    Color32::from_rgb(255, 130, 140),
    Color32::from_rgb(225, 225, 225),
];

/// Engineering-notation label for an axis value, with decimals chosen from the tick step.
pub fn axis_label(v: f64, step: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    // prefix follows the value (1.5µ, 500n), decimals follow the tick step
    let exp3 = ((v.abs().log10() / 3.0).floor() as i32 * 3).clamp(-12, 9);
    let prefix = match exp3 {
        -12 => "p",
        -9 => "n",
        -6 => "µ",
        -3 => "m",
        0 => "",
        3 => "k",
        6 => "M",
        _ => "G",
    };
    let decimals = (-(step.abs().log10() - exp3 as f64) - 1e-9).ceil().max(0.0) as usize;
    format!("{:.*}{prefix}", decimals, v / 10f64.powi(exp3))
}

/// Plain number with just enough decimals for the tick step.
pub fn plain_label(v: f64, step: f64) -> String {
    let decimals = (-step.abs().log10() - 1e-9).ceil().max(0.0) as usize;
    format!("{:.*}", decimals, if v == 0.0 { 0.0 } else { v })
}

/// Round tick positions covering [lo, hi] with roughly `target` ticks. Returns (ticks, step).
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, f64) {
    let range = (hi - lo).abs().max(1e-300);
    let raw = range / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let mut v = vec![];
    let mut t = (lo / step).ceil() * step;
    while t <= hi + step * 1e-9 && v.len() < 200 {
        v.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    (v, step)
}

fn interp(t: &[f64], v: &[f64], x: f64) -> Option<f64> {
    if t.is_empty() || x < t[0] || x > t[t.len() - 1] {
        return None;
    }
    if t.len() == 1 {
        return Some(v[0]);
    }
    let i = t.partition_point(|&a| a < x).clamp(1, t.len() - 1);
    let (t0, t1) = (t[i - 1], t[i]);
    Some(if t1 > t0 { v[i - 1] + (v[i] - v[i - 1]) * (x - t0) / (t1 - t0) } else { v[i] })
}

/// Draw one group of traces in `rect`.
pub fn draw_group(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    traces: &[Trace],
    st: &mut PlotState,
    group: usize,
    style: &Style,
    x_labels: bool,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
    let pa = Rect::from_min_max(
        Pos2::new(rect.left() + 64.0, rect.top() + 6.0),
        Pos2::new(rect.right() - 26.0, rect.bottom() - if x_labels { 22.0 } else { 6.0 }),
    );
    if pa.width() < 20.0 || pa.height() < 20.0 {
        return;
    }
    let resp = ui.interact(pa, id, Sense::click_and_drag());
    let log = st.log_x;
    // x in "axis space": log10(f) on a log axis, the value itself otherwise
    let to_ax = |t: f64| if log { t.max(1e-300).log10() } else { t };
    let from_ax = |a: f64| if log { 10f64.powf(a) } else { a };

    // autoscale y to the visible part of the data
    if st.auto_y[group] {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for tr in traces {
            let a = tr.t.partition_point(|&t| t < st.x.0).saturating_sub(1);
            let b = (tr.t.partition_point(|&t| t <= st.x.1) + 1).min(tr.t.len());
            for &y in &tr.v[a..b.max(a)] {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
        if lo > hi {
            (lo, hi) = (-1.0, 1.0);
        }
        let pad = if hi - lo < 1e-12 { 1.0 } else { (hi - lo) * 0.06 };
        st.y[group] = (lo - pad, hi + pad);
    }

    // interaction: drag = pan, wheel = zoom x (Ctrl: zoom y), double click = fit
    let (w, h) = (pa.width() as f64, pa.height() as f64);
    if resp.dragged() {
        let d = resp.drag_delta();
        let (a0, a1) = (to_ax(st.x.0), to_ax(st.x.1));
        let dx = -(d.x as f64) / w * (a1 - a0);
        st.x = (from_ax(a0 + dx), from_ax(a1 + dx));
        if d.y.abs() > 0.0 {
            let dy = d.y as f64 / h * (st.y[group].1 - st.y[group].0);
            st.y[group] = (st.y[group].0 + dy, st.y[group].1 + dy);
            st.auto_y[group] = false;
        }
    }
    if resp.hovered() {
        let (scroll, ctrl) = ui.input(|i| (i.smooth_scroll_delta.y, i.modifiers.ctrl));
        if scroll != 0.0 {
            let f = (1.0 - scroll as f64 * 0.002).clamp(0.5, 2.0);
            let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(pa.center());
            if ctrl {
                let c = st.y[group].1 - (p.y - pa.top()) as f64 / h * (st.y[group].1 - st.y[group].0);
                st.y[group] = (c + (st.y[group].0 - c) * f, c + (st.y[group].1 - c) * f);
                st.auto_y[group] = false;
            } else {
                let (a0, a1) = (to_ax(st.x.0), to_ax(st.x.1));
                let c = a0 + (p.x - pa.left()) as f64 / w * (a1 - a0);
                st.x = (from_ax(c + (a0 - c) * f), from_ax(c + (a1 - c) * f));
            }
        }
    }
    if resp.double_clicked() {
        st.x = st.full_x;
        st.auto_y[group] = true;
    }

    let (x0, x1) = st.x;
    let (a0, a1) = (to_ax(x0), to_ax(x1));
    let (y0, y1) = st.y[group];
    let sx = |t: f64| pa.left() + ((to_ax(t) - a0) / (a1 - a0)) as f32 * pa.width();
    let sy = |v: f64| pa.bottom() - ((v - y0) / (y1 - y0)) as f32 * pa.height();

    // grid + labels
    let grid = Stroke::new(1.0, Color32::from_gray(45));
    let faint = Stroke::new(1.0, Color32::from_gray(30));
    let label = FontId::proportional(11.0);
    let label_col = Color32::from_gray(170);
    let x_label = |painter: &egui::Painter, t: f64, text: String| {
        if x_labels {
            painter.text(Pos2::new(sx(t), pa.bottom() + 3.0), Align2::CENTER_TOP, text, label.clone(), label_col);
        }
    };
    if log {
        let (k0, k1) = (a0.floor() as i32, a1.ceil() as i32);
        for k in k0..=k1 {
            let base = 10f64.powi(k);
            for m in 2..10 {
                let t = base * m as f64;
                if t > x0 && t < x1 {
                    painter.line_segment([Pos2::new(sx(t), pa.top()), Pos2::new(sx(t), pa.bottom())], faint);
                }
            }
            if base >= x0 * (1.0 - 1e-9) && base <= x1 * (1.0 + 1e-9) {
                painter.line_segment([Pos2::new(sx(base), pa.top()), Pos2::new(sx(base), pa.bottom())], grid);
                x_label(&painter, base, format!("{}{}", axis_label(base, base), style.x_unit));
            }
        }
    } else {
        let (xt, xs) = nice_ticks(x0, x1, (pa.width() / 90.0) as usize);
        for t in &xt {
            painter.line_segment([Pos2::new(sx(*t), pa.top()), Pos2::new(sx(*t), pa.bottom())], grid);
            x_label(&painter, *t, format!("{}{}", axis_label(*t, xs), style.x_unit));
        }
    }
    let (yt, ys) = nice_ticks(y0, y1, (pa.height() / 40.0) as usize);
    let ylab = |v: f64, step: f64| if style.plain_y { plain_label(v, step) } else { axis_label(v, step) };
    for v in &yt {
        painter.line_segment([Pos2::new(pa.left(), sy(*v)), Pos2::new(pa.right(), sy(*v))], if *v == 0.0 { Stroke::new(1.0, Color32::from_gray(90)) } else { grid });
        painter.text(Pos2::new(pa.left() - 5.0, sy(*v)), Align2::RIGHT_CENTER, format!("{}{}", ylab(*v, ys), style.unit), label.clone(), label_col);
    }
    painter.rect_stroke(pa, 0.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);

    // traces (min/max envelope per pixel column when zoomed out)
    let clip = painter.with_clip_rect(pa);
    for tr in traces {
        let a = tr.t.partition_point(|&t| t < x0).saturating_sub(1);
        let b = (tr.t.partition_point(|&t| t <= x1) + 1).min(tr.t.len());
        if b <= a + 1 {
            continue;
        }
        let stroke = Stroke::new(1.5, tr.color);
        let n = b - a;
        if n as f32 <= pa.width() * 2.0 {
            let pts: Vec<Pos2> = (a..b).map(|i| Pos2::new(sx(tr.t[i]), sy(tr.v[i]))).collect();
            clip.add(egui::Shape::line(pts, stroke));
        } else {
            let mut pts: Vec<Pos2> = vec![];
            let cols = pa.width().ceil() as usize;
            let per = n.div_ceil(cols).max(1);
            let mut i = a;
            while i < b {
                let j = (i + per).min(b);
                let seg = &tr.v[i..j];
                let (mn, mx) = seg.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                let x = sx(tr.t[i]);
                let (first, last) = (seg[0], seg[seg.len() - 1]);
                for v in [first, mn, mx, last] {
                    pts.push(Pos2::new(x, sy(v)));
                }
                i = j;
            }
            clip.add(egui::Shape::line(pts, stroke));
        }
    }

    // readout
    if let Some(p) = resp.hover_pos() {
        let t = from_ax(a0 + (p.x - pa.left()) as f64 / w * (a1 - a0));
        clip.line_segment([Pos2::new(p.x, pa.top()), Pos2::new(p.x, pa.bottom())], Stroke::new(1.0, Color32::from_gray(140)));
        let mut y = pa.top() + 4.0;
        let head = format!("{} = {}{}", style.x_name, axis_label(t, t.abs() / 1000.0), style.x_unit);
        clip.text(Pos2::new(pa.left() + 6.0, y), Align2::LEFT_TOP, head, label.clone(), Color32::from_gray(210));
        for tr in traces {
            y += 14.0;
            if let Some(v) = interp(tr.t, tr.v, t) {
                clip.circle_filled(Pos2::new(p.x, sy(v)), 3.0, tr.color);
                let step = (y1 - y0) / 1000.0;
                let txt = format!("{} = {}{}", tr.name, if style.plain_y { plain_label(v, step) } else { axis_label(v, step) }, style.unit);
                clip.text(Pos2::new(pa.left() + 6.0, y), Align2::LEFT_TOP, txt, label.clone(), tr.color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round() {
        let (t, step) = nice_ticks(0.0, 10e-3, 5);
        assert_eq!(step, 2e-3);
        assert_eq!(t.len(), 6);
        assert_eq!(t[0], 0.0);
        let (t, step) = nice_ticks(-1.2, 6.3, 6);
        assert_eq!(step, 2.0);
        assert!(t.contains(&0.0) && t.contains(&6.0));
    }

    #[test]
    fn labels_use_engineering_prefixes() {
        assert_eq!(axis_label(2e-3, 2e-3), "2m");
        assert_eq!(axis_label(1.5e-6, 0.5e-6), "1.5µ");
        assert_eq!(axis_label(5.0, 1.0), "5");
        assert_eq!(axis_label(2500.0, 500.0), "2.5k");
        assert_eq!(axis_label(0.0, 1.0), "0");
        assert_eq!(axis_label(1e6, 1e6), "1M");
        assert_eq!(axis_label(100.0, 100.0), "100");
    }

    #[test]
    fn plain_labels_for_db_and_degrees() {
        assert_eq!(plain_label(-20.0, 20.0), "-20");
        assert_eq!(plain_label(2.5, 0.5), "2.5");
        assert_eq!(plain_label(-135.0, 45.0), "-135");
        assert_eq!(plain_label(0.0, 10.0), "0");
    }

    #[test]
    fn interpolation() {
        let (t, v) = ([0.0, 1.0, 3.0], [0.0, 10.0, 30.0]);
        assert_eq!(interp(&t, &v, 0.5), Some(5.0));
        assert_eq!(interp(&t, &v, 2.0), Some(20.0));
        assert_eq!(interp(&t, &v, 4.0), None);
    }
}
