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
    /// Measurement cursors A and B (x positions, shared by every group of the viewer).
    pub cursors: [Option<f64>; 2],
    /// Text boxes of the scale bar (x from / to, y from / to per group); kept here so typing is not overwritten each frame.
    pub x_text: [String; 2],
    pub y_text: [[String; 2]; 2],
}

impl PlotState {
    pub fn new(tstop: f64) -> Self {
        Self::linear(0.0, tstop)
    }

    pub fn linear(lo: f64, hi: f64) -> Self {
        Self { x: (lo, hi), full_x: (lo, hi), log_x: false, y: [(-1.0, 1.0); 2], auto_y: [true; 2], cursors: [None; 2], x_text: Default::default(), y_text: Default::default() }
    }

    pub fn log(f0: f64, f1: f64) -> Self {
        Self { log_x: true, ..Self::linear(f0, f1) }
    }
}

/// Scales the range `r` by `f` keeping the point `c` where it is (the pointer stays over the same value).
fn zoom_about(r: (f64, f64), f: f64, c: f64) -> (f64, f64) {
    (c + (r.0 - c) * f, c + (r.1 - c) * f)
}

/// One pair of "from / to" text boxes. Typing a number and pressing Enter (or leaving the box)
/// returns the new range; otherwise the boxes follow the real range.
fn range_boxes(ui: &mut egui::Ui, text: &mut [String; 2], cur: (f64, f64), unit: &str, tip: &str) -> Option<(f64, f64)> {
    let mut out = None;
    let mut edited = [false; 2];
    for k in 0..2 {
        if k == 1 {
            ui.label("〜");
        }
        let resp = ui.add(egui::TextEdit::singleline(&mut text[k]).desired_width(66.0)).on_hover_text(tip);
        edited[k] = resp.lost_focus();
        if !resp.has_focus() && !resp.lost_focus() {
            text[k] = sim_core::format_value(if k == 0 { cur.0 } else { cur.1 });
        }
    }
    if edited[0] || edited[1] {
        if let (Some(a), Some(b)) = (sim_core::parse_value(&text[0]), sim_core::parse_value(&text[1]))
            && a < b
        {
            out = Some((a, b));
        }
        // a bad entry falls back to the real range on the next frame
        text[0] = sim_core::format_value(cur.0);
        text[1] = sim_core::format_value(cur.1);
    }
    ui.label(unit);
    out
}

/// The visible range of every axis as editable numbers, plus "全体" (x) and an auto-scale switch (y).
/// `groups` = (group index, unit). Zooming itself is the wheel over the axis numbers (see `draw_group`).
pub fn scale_bar(ui: &mut egui::Ui, st: &mut PlotState, x_unit: &str, groups: &[(usize, &str)]) {
    ui.horizontal_wrapped(|ui| {
        if let Some(r) = range_boxes(ui, &mut st.x_text, st.x, x_unit, "横軸の表示範囲の始まり / 終わり。数字 (10m, 2u など) を入れて Enter で反映します。横軸の数字の上でホイールを回しても拡大縮小できます") {
            st.x = r;
        }
        if ui.small_button("全体").on_hover_text("横軸を最初の範囲 (全体) に戻します").clicked() {
            st.x = st.full_x;
        }
        for (g, unit) in groups {
            let g = *g;
            ui.separator();
            if let Some(r) = range_boxes(ui, &mut st.y_text[g], st.y[g], unit, "縦軸の下端 / 上端。数字 (-1, 500m など) を入れて Enter で反映します。縦軸の数字の上でホイールを回しても拡大縮小できます") {
                st.y[g] = r;
                st.auto_y[g] = false;
            }
            ui.checkbox(&mut st.auto_y[g], "自動").on_hover_text("オンのあいだ、いま見えている範囲に合わせて縦軸を自動で決めます。手で動かすと自動でオフになります");
        }
    });
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

/// The dimmed colour of a kept ("before") trace drawn behind the live one of the same colour.
pub fn ghost_color(c: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 100)
}

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

pub const CURSOR_COLORS: [Color32; 2] = [Color32::from_rgb(255, 235, 120), Color32::from_rgb(120, 235, 255)];

/// A click puts cursor A, then B; after that it moves whichever cursor is nearer (on the axis the plot uses).
fn place_cursor(c: &mut [Option<f64>; 2], t: f64, log: bool) {
    let ax = |x: f64| if log { x.max(1e-300).log10() } else { x };
    match (c[0], c[1]) {
        (None, _) => c[0] = Some(t),
        (_, None) => c[1] = Some(t),
        (Some(a), Some(b)) => {
            if (ax(t) - ax(a)).abs() <= (ax(t) - ax(b)).abs() {
                c[0] = Some(t)
            } else {
                c[1] = Some(t)
            }
        }
    }
}

/// Text lines for the cursor box: positions, their difference, and every trace at both cursors.
/// The first element says which trace (by index) a line belongs to, for its colour.
pub fn cursor_report(traces: &[Trace], c: &[Option<f64>; 2], style: &Style, log: bool) -> Vec<(Option<usize>, String)> {
    let mut out = vec![];
    if c[0].is_none() && c[1].is_none() {
        return out;
    }
    let xs = |t: f64| format!("{}{}", axis_label(t, t.abs() / 100.0), style.x_unit);
    let ys = |v: f64, step: f64| format!("{}{}", if style.plain_y { plain_label(v, step) } else { axis_label(v, step) }, style.unit);
    for (k, name) in ["A", "B"].iter().enumerate() {
        if let Some(t) = c[k] {
            out.push((None, format!("{name}: {} = {}", style.x_name, xs(t))));
        }
    }
    if let (Some(a), Some(b)) = (c[0], c[1]) {
        if log {
            if a > 0.0 && b > 0.0 {
                out.push((None, format!("B/A = {:.3} 倍 ({:.2} オクターブ)", b / a, (b / a).log2())));
            }
        } else {
            let d = b - a;
            let mut l = format!("Δ{} = {}", style.x_name, xs(d));
            if d != 0.0 && style.x_unit == "s" {
                l += &format!("  (1/Δ{} = {})", style.x_name, format_args!("{}Hz", axis_label(1.0 / d.abs(), 1.0 / d.abs() / 100.0)));
            }
            out.push((None, l));
        }
    }
    for (i, tr) in traces.iter().enumerate() {
        let (va, vb) = (c[0].and_then(|t| interp(tr.t, tr.v, t)), c[1].and_then(|t| interp(tr.t, tr.v, t)));
        let step = |v: f64| v.abs() / 100.0;
        let mut l = tr.name.to_string();
        if let Some(v) = va {
            l += &format!("  A={}", ys(v, step(v)));
        }
        if let Some(v) = vb {
            l += &format!("  B={}", ys(v, step(v)));
        }
        if let (Some(a), Some(b)) = (va, vb) {
            l += &format!("  Δ={}", ys(b - a, step(b - a)));
        }
        if va.is_some() || vb.is_some() {
            out.push((Some(i), l));
        }
    }
    out
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

    // wheel over the axis numbers: the y numbers zoom the y axis, the x numbers zoom the x axis (about the pointer)
    {
        let ystrip = Rect::from_min_max(Pos2::new(rect.left(), pa.top()), Pos2::new(pa.left(), pa.bottom()));
        let xstrip = Rect::from_min_max(Pos2::new(pa.left(), pa.bottom()), Pos2::new(pa.right(), rect.bottom()));
        let yr = ui.interact(ystrip, id.with("y_axis"), Sense::hover()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
        let xr = x_labels.then(|| ui.interact(xstrip, id.with("x_axis"), Sense::hover()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal));
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let f = (1.0 - scroll as f64 * 0.002).clamp(0.5, 2.0);
            let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(pa.center());
            if yr.hovered() {
                // the y range as shown this frame (auto-scaled groups have not been refreshed yet)
                let (h0, h1) = (pa.height() as f64, st.y[group]);
                let c = h1.1 - ((p.y - pa.top()) as f64 / h0).clamp(0.0, 1.0) * (h1.1 - h1.0);
                st.y[group] = zoom_about(st.y[group], f, c);
                st.auto_y[group] = false;
            } else if xr.as_ref().is_some_and(|r| r.hovered()) {
                let (a0, a1) = (to_ax(st.x.0), to_ax(st.x.1));
                let c = a0 + ((p.x - pa.left()) as f64 / pa.width() as f64).clamp(0.0, 1.0) * (a1 - a0);
                let (n0, n1) = zoom_about((a0, a1), f, c);
                st.x = (from_ax(n0), from_ax(n1));
            }
        }
    }

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
        st.cursors = [None; 2];
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (a0, a1) = (to_ax(st.x.0), to_ax(st.x.1));
        let t = from_ax(a0 + (p.x - pa.left()) as f64 / w * (a1 - a0));
        place_cursor(&mut st.cursors, t, log);
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
            // too many points: keep first / min / max / last of every pixel column
            // (grouped by screen position, so this is also right on a logarithmic axis)
            let mut pts: Vec<Pos2> = vec![];
            let mut cur: Option<(i64, f32, f64, f64, f64, f64)> = None; // column, x, first, min, max, last
            let flush = |pts: &mut Vec<Pos2>, c: (i64, f32, f64, f64, f64, f64)| {
                for v in [c.2, c.3, c.4, c.5] {
                    pts.push(Pos2::new(c.1, sy(v)));
                }
            };
            for i in a..b {
                let px = sx(tr.t[i]);
                let col = px.floor() as i64;
                let y = tr.v[i];
                match &mut cur {
                    Some((c, _, _, mn, mx, last)) if *c == col => {
                        *mn = mn.min(y);
                        *mx = mx.max(y);
                        *last = y;
                    }
                    _ => {
                        if let Some(done) = cur.take() {
                            flush(&mut pts, done);
                        }
                        cur = Some((col, px, y, y, y, y));
                    }
                }
            }
            if let Some(done) = cur {
                flush(&mut pts, done);
            }
            clip.add(egui::Shape::line(pts, stroke));
        }
    }

    // measurement cursors
    for (k, c) in st.cursors.iter().enumerate() {
        let Some(t) = *c else { continue };
        if t < x0 || t > x1 {
            continue;
        }
        let col = CURSOR_COLORS[k];
        let x = sx(t);
        clip.line_segment([Pos2::new(x, pa.top()), Pos2::new(x, pa.bottom())], Stroke::new(1.5, col));
        clip.text(Pos2::new(x + 3.0, pa.bottom() - 3.0), Align2::LEFT_BOTTOM, if k == 0 { "A" } else { "B" }, FontId::proportional(12.0), col);
        for tr in traces {
            if let Some(v) = interp(tr.t, tr.v, t) {
                clip.circle_stroke(Pos2::new(x, sy(v)), 4.0, Stroke::new(1.5, tr.color));
            }
        }
    }
    let report = cursor_report(traces, &st.cursors, style, log);
    if !report.is_empty() {
        let mut y = pa.top() + 4.0;
        let width = report.iter().map(|(_, l)| l.chars().count()).max().unwrap_or(0) as f32 * 6.4 + 12.0;
        let boxr = Rect::from_min_size(Pos2::new(pa.right() - width - 4.0, y - 2.0), egui::vec2(width, report.len() as f32 * 14.0 + 4.0));
        clip.rect_filled(boxr, 3.0, Color32::from_rgba_unmultiplied(10, 12, 10, 215));
        for (who, line) in &report {
            let col = who.map_or(Color32::from_gray(225), |i| traces[i].color);
            clip.text(Pos2::new(pa.right() - 10.0, y), Align2::RIGHT_TOP, line, label.clone(), col);
            y += 14.0;
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
    fn zooming_about_a_point_keeps_that_point_fixed() {
        let r = zoom_about((0.0, 10.0), 0.5, 2.0);
        assert!((r.0 - 1.0).abs() < 1e-9 && (r.1 - 6.0).abs() < 1e-9, "{r:?}");
        // the point itself maps to the same fraction of the range before and after
        let (a, b) = (0.0, 10.0);
        let (c, f) = (2.0, 3.0);
        let (n0, n1) = zoom_about((a, b), f, c);
        assert!(((c - a) / (b - a) - (c - n0) / (n1 - n0)).abs() < 1e-9);
    }

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
    fn cursors_fill_a_then_b_then_move_the_nearer() {
        let mut c = [None; 2];
        place_cursor(&mut c, 1.0, false);
        place_cursor(&mut c, 5.0, false);
        assert_eq!(c, [Some(1.0), Some(5.0)]);
        place_cursor(&mut c, 4.0, false);
        assert_eq!(c, [Some(1.0), Some(4.0)]);
        place_cursor(&mut c, 0.0, false);
        assert_eq!(c, [Some(0.0), Some(4.0)]);
        // on a log axis 100 is nearer to 10 than to 10000 in decades
        let mut l = [Some(10.0), Some(10000.0)];
        place_cursor(&mut l, 100.0, true);
        assert_eq!(l, [Some(100.0), Some(10000.0)]);
    }

    #[test]
    fn cursor_report_gives_differences_and_period() {
        let (t, v) = ([0.0, 1e-3, 2e-3], [0.0, 1.0, 3.0]);
        let tr = [Trace { name: "V(out)", color: Color32::WHITE, t: &t, v: &v }];
        let style = Style { unit: "V", x_unit: "s", x_name: "t", plain_y: false };
        assert!(cursor_report(&tr, &[None, None], &style, false).is_empty());
        let r = cursor_report(&tr, &[Some(0.0), Some(2e-3)], &style, false);
        let text: Vec<&str> = r.iter().map(|(_, l)| l.as_str()).collect();
        assert!(text.iter().any(|l| l.starts_with("Δt = 2.0") && l.contains("500Hz")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("A=0") && l.contains("B=3") && l.contains("Δ=3")), "{text:?}");
        let f = cursor_report(&tr, &[Some(100.0), Some(200.0)], &Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true }, true);
        assert!(f.iter().any(|(_, l)| l.contains("2.000 倍") && l.contains("1.00 オクターブ")));
    }

    #[test]
    fn ghost_colour_keeps_the_hue_but_is_see_through() {
        let g = ghost_color(Color32::from_rgb(240, 200, 60));
        assert!(g.a() < 255 && g.a() > 0);
        assert!(g.r() > g.b(), "{g:?}");
    }

    #[test]
    fn interpolation() {
        let (t, v) = ([0.0, 1.0, 3.0], [0.0, 10.0, 30.0]);
        assert_eq!(interp(&t, &v, 0.5), Some(5.0));
        assert_eq!(interp(&t, &v, 2.0), Some(20.0));
        assert_eq!(interp(&t, &v, 4.0), None);
    }
}
