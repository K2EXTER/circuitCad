//! The digital view of a transient run: each voltage as a logic lane (High / Low / between the thresholds) and nets that
//! differ only in a trailing number (`D0`..`D7`) as one bus lane with its value, like a logic analyser. Shares the time axis,
//! zoom and cursors (`PlotState`) with the analog plot.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};

use crate::plot::PlotState;

/// A level read with the CMOS thresholds: below VIL, above VIH, or between them (undefined).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
    Mid,
}

/// The input thresholds of a CMOS logic family (74HC): 30 % and 70 % of the supply.
pub fn thresholds(vcc: f64) -> (f64, f64) {
    (0.3 * vcc, 0.7 * vcc)
}

pub fn level(v: f64, (vil, vih): (f64, f64)) -> Level {
    if v <= vil {
        Level::Low
    } else if v >= vih {
        Level::High
    } else {
        Level::Mid
    }
}

/// One row of the view: a single net, or a bus (bit names with the most significant first).
#[derive(Clone, Debug, PartialEq)]
pub enum Lane {
    Bit(String),
    Bus { name: String, bits: Vec<String> },
}

/// The net inside a signal name: `V(D0)` → `D0`.
pub fn net_of(signal: &str) -> &str {
    signal.strip_prefix("V(").and_then(|s| s.strip_suffix(')')).unwrap_or(signal)
}

/// A net name split into a prefix and a trailing index: a number (`Q12` → (`Q`, 12)), or one letter A to H as the logic ICs
/// name their outputs (`QA` → (`Q`, 0): QA is the least significant bit of a 74HC161 or 595). None for a name that is only
/// a number; a letter needs a prefix of two letters or less after which it stands (`QA`, `OUTB` is not one).
fn split_index(net: &str) -> Option<(&str, u32, bool)> {
    let digits = net.len() - net.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        if digits == net.len() || digits > 3 {
            return None;
        }
        let (p, n) = net.split_at(net.len() - digits);
        return Some((p, n.parse().ok()?, false));
    }
    let last = net.chars().last()?;
    let p = &net[..net.len() - 1];
    (('A'..='H').contains(&last) && (1..=2).contains(&p.len()) && p.chars().all(|c| c.is_ascii_uppercase())).then(|| (p, last as u32 - 'A' as u32, true))
}

/// Nets that may form a bus: prefix, by letter, (index, signal).
type Group = (String, bool, Vec<(u32, String)>);

/// The lanes for the chosen signals, in their order: two or more nets with the same prefix and a trailing number become one
/// bus at the place of the first (bits ordered by the number, the largest as the most significant bit); a bus is at most 32
/// bits wide (wider groups stay as single lanes).
pub fn lanes(signals: &[&str]) -> Vec<Lane> {
    // (prefix, by letter, bits): numbered and lettered nets with the same prefix are different buses
    let mut groups: Vec<Group> = vec![];
    for s in signals {
        if let Some((p, n, letter)) = split_index(net_of(s)) {
            match groups.iter_mut().find(|(gp, gl, _)| gp == p && *gl == letter) {
                Some((_, _, v)) => v.push((n, s.to_string())),
                None => groups.push((p.to_string(), letter, vec![(n, s.to_string())])),
            }
        }
    }
    let bus_of = |s: &str| {
        split_index(net_of(s)).and_then(|(p, _, letter)| groups.iter().find(|(gp, gl, v)| gp == p && *gl == letter && (2..=32).contains(&v.len())))
    };
    let mut out = vec![];
    let mut done: Vec<(String, bool)> = vec![];
    for s in signals {
        match bus_of(s) {
            Some((p, letter, v)) => {
                if done.contains(&(p.clone(), *letter)) {
                    continue;
                }
                done.push((p.clone(), *letter));
                let mut v = v.clone();
                v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
                v.dedup_by_key(|(n, _)| *n);
                let (lo, hi) = (v.last().map_or(0, |b| b.0), v[0].0);
                let idx = |n: u32| if *letter { char::from_u32('A' as u32 + n).unwrap_or('?').to_string() } else { n.to_string() };
                out.push(Lane::Bus { name: format!("{p}[{}..{}]", idx(lo), idx(hi)), bits: v.into_iter().map(|(_, s)| s).collect() });
            }
            None => out.push(Lane::Bit(s.to_string())),
        }
    }
    out
}

/// The rows drawn: each lane, and under an opened bus its bits (indented). The bool is "a bit of the bus above".
pub fn rows(lanes: &[Lane], open: &[String]) -> Vec<(Lane, bool)> {
    let mut out = vec![];
    for l in lanes {
        out.push((l.clone(), false));
        if let Lane::Bus { name, bits } = l
            && open.contains(name)
        {
            out.extend(bits.iter().map(|b| (Lane::Bit(b.clone()), true)));
        }
    }
    out
}

/// Runs of the same value: (index of the first sample, value). `value_at(i)` is the value of sample `i`.
pub fn runs<T: PartialEq + Copy>(n: usize, value_at: impl Fn(usize) -> T) -> Vec<(usize, T)> {
    let mut out: Vec<(usize, T)> = vec![];
    for i in 0..n {
        let v = value_at(i);
        if out.last().is_none_or(|(_, last)| *last != v) {
            out.push((i, v));
        }
    }
    out
}

/// A bus's value at one sample, from its bits (most significant first); none if a bit is between the thresholds.
pub fn bus_value(bits: &[&[f64]], i: usize, th: (f64, f64)) -> Option<u64> {
    let mut v = 0u64;
    for b in bits {
        v = (v << 1)
            | match level(*b.get(i)?, th) {
                Level::High => 1,
                Level::Low => 0,
                Level::Mid => return None,
            };
    }
    Some(v)
}

/// How a bus value is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Radix {
    #[default]
    Dec,
    Hex,
    Bin,
}

pub fn format_bus(v: Option<u64>, width: usize, radix: Radix) -> String {
    let Some(v) = v else { return "X".into() };
    match radix {
        Radix::Dec => v.to_string(),
        Radix::Hex => format!("{v:0w$X}h", w = width.div_ceil(4)),
        Radix::Bin => format!("{v:0width$b}"),
    }
}

/// The supply the thresholds are taken from when none is typed: the highest voltage of the chosen signals, which in a logic
/// circuit is the supply (a High output sits on it).
pub fn guess_vcc(values: &[&[f64]]) -> f64 {
    let m = values.iter().flat_map(|v| v.iter()).fold(0.0f64, |m, v| m.max(*v));
    if m > 0.0 { m } else { 5.0 }
}

const LANE_H: f32 = 26.0;
const MARK_H: f32 = 16.0;
const NAME_W: f32 = 132.0;
const HIGH: Color32 = Color32::from_rgb(110, 220, 120);
const LOW: Color32 = Color32::from_rgb(70, 150, 80);
const MID: Color32 = Color32::from_rgb(235, 90, 80);
const BUS: Color32 = Color32::from_rgb(120, 190, 240);

/// The lanes drawn into `rect`, with the time axis of `st` (drag = pan, wheel = zoom, click = cursor, double click = all).
/// `signals` are the values over `time` by signal name. The cursor A's values are written after the names.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    time: &[f64],
    rows: &[(Lane, bool)],
    open: &mut Vec<String>,
    signals: &[(&str, &[f64])],
    th: (f64, f64),
    radix: Radix,
    marks: &[(f64, String)],
    st: &mut PlotState,
) {
    let signal = |name: &str| signals.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 22, 18));
    // the timing violations get a strip of their own above the lanes
    let strip = if marks.is_empty() { 0.0 } else { MARK_H };
    let pa = Rect::from_min_max(Pos2::new(rect.left() + NAME_W, rect.top() + 4.0 + strip), Pos2::new(rect.right() - 26.0, rect.bottom() - 22.0));
    if pa.width() < 20.0 || pa.height() < 10.0 || time.len() < 2 {
        return;
    }
    let resp = ui.interact(pa, id, Sense::click_and_drag());
    let w = pa.width() as f64;
    if resp.dragged() {
        let dx = -(resp.drag_delta().x as f64) / w * (st.x.1 - st.x.0);
        st.x = (st.x.0 + dx, st.x.1 + dx);
    }
    if resp.hovered() {
        let scroll = crate::prefs::zoom_scroll(ui);
        if scroll != 0.0 {
            let f = (1.0 - scroll as f64 * 0.002).clamp(0.5, 2.0);
            let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(pa.center());
            let c = st.x.0 + (p.x - pa.left()) as f64 / w * (st.x.1 - st.x.0);
            st.x = (c + (st.x.0 - c) * f, c + (st.x.1 - c) * f);
        }
    }
    if resp.double_clicked() {
        st.x = st.full_x;
        st.cursors = [None; 2];
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let t = st.x.0 + (p.x - pa.left()) as f64 / w * (st.x.1 - st.x.0);
        crate::plot::place_cursor(&mut st.cursors, t, false);
    }
    let (x0, x1) = st.x;
    let sx = |t: f64| pa.left() + ((t - x0) / (x1 - x0)) as f32 * pa.width();
    let font = FontId::proportional(11.0);
    let label_col = Color32::from_gray(170);
    // time grid and labels
    let (xt, xs) = crate::plot::nice_ticks(x0, x1, (pa.width() / 90.0) as usize);
    for t in &xt {
        painter.line_segment([Pos2::new(sx(*t), pa.top()), Pos2::new(sx(*t), pa.bottom())], Stroke::new(1.0, Color32::from_gray(38)));
        painter.text(Pos2::new(sx(*t), pa.bottom() + 3.0), Align2::CENTER_TOP, format!("{}s", crate::plot::axis_label(*t, xs)), font.clone(), label_col);
    }
    let clip = painter.with_clip_rect(pa);
    // the samples on screen (one either side, so a run that starts off screen is drawn from the edge)
    let a = time.partition_point(|&t| t < x0).saturating_sub(1);
    let b = (time.partition_point(|&t| t <= x1) + 1).min(time.len());
    let cursor_a = st.cursors[0].map(|t| time.partition_point(|&s| s <= t).saturating_sub(1));
    // a few lanes are drawn taller, to use the place
    let lane_h = ((pa.height() - 4.0) / rows.len().max(1) as f32).clamp(LANE_H, 44.0);
    for (k, (lane, inner)) in rows.iter().enumerate() {
        let top = pa.top() + k as f32 * lane_h + 4.0;
        if top > pa.bottom() {
            break;
        }
        let (hi_y, lo_y) = (top + (lane_h - LANE_H) * 0.25, top + lane_h - 8.0 - (lane_h - LANE_H) * 0.25);
        let mid_y = (hi_y + lo_y) * 0.5;
        let name_pos = Pos2::new(rect.left() + if *inner { 18.0 } else { 6.0 }, mid_y);
        // a bus name opens and closes its bits
        if let Lane::Bus { name, .. } = lane {
            let r = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(pa.left(), top + lane_h - 4.0));
            let resp = ui.interact(r, id.with(("bus", name.as_str())), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                if let Some(i) = open.iter().position(|n| n == name) {
                    open.remove(i);
                } else {
                    open.push(name.clone());
                }
            }
            resp.on_hover_text(i18n::tr("クリックで、このバスの 1 本ずつの線を下に開きます (もう一度で閉じる)"));
        }
        match lane {
            Lane::Bit(s) => {
                let Some(v) = signal(s) else { continue };
                let lv = |i: usize| level(v[i], th);
                let mut text = net_of(s).to_string();
                if let Some(i) = cursor_a {
                    text += match lv(i) {
                        Level::High => " = 1",
                        Level::Low => " = 0",
                        Level::Mid => " = X",
                    };
                }
                painter.text(name_pos, Align2::LEFT_CENTER, text, font.clone(), HIGH);
                let rs = runs(b - a, |i| lv(a + i));
                let y_of = |l: Level| match l {
                    Level::High => hi_y,
                    Level::Low => lo_y,
                    Level::Mid => mid_y,
                };
                let col_of = |l: Level| match l {
                    Level::High => HIGH,
                    Level::Low => LOW,
                    Level::Mid => MID,
                };
                for (j, (i0, l)) in rs.iter().enumerate() {
                    let xa = sx(time[a + i0].max(x0));
                    let xb = rs.get(j + 1).map_or(sx(time[b - 1].min(x1)), |(i1, _)| sx(time[a + i1]));
                    if l == &Level::High {
                        clip.rect_filled(Rect::from_min_max(Pos2::new(xa, hi_y), Pos2::new(xb.max(xa + 1.0), lo_y)), 0.0, Color32::from_rgba_unmultiplied(110, 220, 120, 28));
                    }
                    clip.line_segment([Pos2::new(xa, y_of(*l)), Pos2::new(xb, y_of(*l))], Stroke::new(1.6, col_of(*l)));
                    if j > 0 {
                        clip.line_segment([Pos2::new(xa, hi_y), Pos2::new(xa, lo_y)], Stroke::new(1.0, Color32::from_gray(120)));
                    }
                }
            }
            Lane::Bus { name, bits } => {
                let vals: Vec<&[f64]> = bits.iter().filter_map(|b| signal(b)).collect();
                if vals.len() != bits.len() {
                    continue;
                }
                let mut text = format!("{} {name}", if open.contains(name) { "▼" } else { "▶" });
                if let Some(i) = cursor_a {
                    text += &format!(" = {}", format_bus(bus_value(&vals, i, th), bits.len(), radix));
                }
                painter.text(name_pos, Align2::LEFT_CENTER, text, font.clone(), BUS);
                let rs = runs(b - a, |i| bus_value(&vals, a + i, th));
                for (j, (i0, val)) in rs.iter().enumerate() {
                    let xa = sx(time[a + i0].max(x0));
                    let xb = rs.get(j + 1).map_or(sx(time[b - 1].min(x1)), |(i1, _)| sx(time[a + i1]));
                    let col = if val.is_some() { BUS } else { MID };
                    // the usual bus picture: two lines that cross where the value changes
                    let e = ((xb - xa) * 0.5).min(3.0);
                    let pts = vec![
                        Pos2::new(xa, mid_y),
                        Pos2::new(xa + e, hi_y),
                        Pos2::new(xb - e, hi_y),
                        Pos2::new(xb, mid_y),
                        Pos2::new(xb - e, lo_y),
                        Pos2::new(xa + e, lo_y),
                        Pos2::new(xa, mid_y),
                    ];
                    clip.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
                    let label = format_bus(*val, bits.len(), radix);
                    if (xb - xa) > label.chars().count() as f32 * 6.5 + 8.0 {
                        clip.text(Pos2::new((xa + xb) * 0.5, mid_y), Align2::CENTER_CENTER, label, font.clone(), Color32::from_gray(225));
                    }
                }
            }
        }
        painter.line_segment([Pos2::new(rect.left(), top + lane_h - 4.0), Pos2::new(pa.right(), top + lane_h - 4.0)], Stroke::new(1.0, Color32::from_gray(32)));
    }
    painter.rect_stroke(pa, 0.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
    if !marks.is_empty() {
        draw_marks(ui, &painter, rect, pa, id, marks, &sx, (x0, x1), &font);
    }
    // cursors and their difference
    for (k, c) in st.cursors.iter().enumerate() {
        let Some(t) = *c else { continue };
        if t < x0 || t > x1 {
            continue;
        }
        let col = crate::plot::CURSOR_COLORS[k];
        clip.line_segment([Pos2::new(sx(t), pa.top()), Pos2::new(sx(t), pa.bottom())], Stroke::new(1.5, col));
        clip.text(Pos2::new(sx(t) + 3.0, pa.bottom() - 3.0), Align2::LEFT_BOTTOM, if k == 0 { "A" } else { "B" }, FontId::proportional(12.0), col);
    }
    if let [Some(ta), Some(tb)] = st.cursors {
        let d = (tb - ta).abs();
        let mut text = format!("B − A = {}s", crate::plot::axis_label(d, d / 1000.0));
        if d > 0.0 {
            text += &format!(" ({}Hz)", crate::plot::axis_label(1.0 / d, 1.0 / d / 1000.0));
        }
        clip.text(Pos2::new(pa.right() - 6.0, pa.top() + 4.0), Align2::RIGHT_TOP, text, font.clone(), Color32::from_gray(220));
    }
    if let Some(p) = resp.hover_pos() {
        clip.line_segment([Pos2::new(p.x, pa.top()), Pos2::new(p.x, pa.bottom())], Stroke::new(1.0, Color32::from_gray(140)));
        let t = x0 + (p.x - pa.left()) as f64 / w * (x1 - x0);
        clip.text(Pos2::new(p.x + 4.0, pa.bottom() - 4.0), Align2::LEFT_BOTTOM, format!("t = {}s", crate::plot::axis_label(t, t.abs() / 1000.0)), font, Color32::from_gray(210));
    }
}

/// The timing violations above the lanes: a red mark at each time (a faint line down through the lanes), the text of the
/// nearest under the pointer.
#[allow(clippy::too_many_arguments)]
fn draw_marks(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, pa: Rect, id: egui::Id, marks: &[(f64, String)], sx: &dyn Fn(f64) -> f32, (x0, x1): (f64, f64), font: &FontId) {
    let top = pa.top() - MARK_H;
    painter.text(Pos2::new(rect.left() + 6.0, top + MARK_H * 0.5), Align2::LEFT_CENTER, i18n::tr("タイミング違反"), font.clone(), MID);
    let band = Rect::from_min_max(Pos2::new(pa.left(), top), Pos2::new(pa.right(), pa.top()));
    let clip = painter.with_clip_rect(Rect::from_min_max(band.min, pa.max));
    let mut last_x = f32::NEG_INFINITY;
    for (t, _) in marks.iter().filter(|(t, _)| *t >= x0 && *t <= x1) {
        let x = sx(*t);
        clip.line_segment([Pos2::new(x, pa.top()), Pos2::new(x, pa.bottom())], Stroke::new(1.0, Color32::from_rgba_unmultiplied(235, 90, 80, 70)));
        // marks closer than a few pixels are one triangle
        if x - last_x >= 4.0 {
            let y = top + 2.0;
            clip.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 4.0, y), Pos2::new(x + 4.0, y), Pos2::new(x, y + MARK_H - 5.0)], MID, Stroke::NONE));
            last_x = x;
        }
    }
    let resp = ui.interact(band, id.with("marks"), Sense::hover());
    if let Some(p) = resp.hover_pos() {
        let near = marks.iter().filter(|(t, _)| *t >= x0 && *t <= x1).min_by(|a, b| (sx(a.0) - p.x).abs().total_cmp(&(sx(b.0) - p.x).abs()));
        if let Some((_, text)) = near.filter(|(t, _)| (sx(*t) - p.x).abs() < 8.0) {
            resp.on_hover_text(text.as_str());
        }
    }
}

/// How high the lanes are drawn (the plot area and the time labels).
pub fn height(rows: usize) -> f32 {
    rows as f32 * LANE_H + 34.0 + MARK_H
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_nets_become_one_bus_with_the_largest_number_first() {
        let l = lanes(&["V(CLK)", "V(Q0)", "V(D1)", "V(Q2)", "V(Q1)", "V(D0)", "V(Q3)", "V(RST)", "V(8)"]);
        assert_eq!(
            l,
            vec![
                Lane::Bit("V(CLK)".into()),
                Lane::Bus { name: "Q[0..3]".into(), bits: vec!["V(Q3)".into(), "V(Q2)".into(), "V(Q1)".into(), "V(Q0)".into()] },
                Lane::Bus { name: "D[0..1]".into(), bits: vec!["V(D1)".into(), "V(D0)".into()] },
                Lane::Bit("V(RST)".into()),
                Lane::Bit("V(8)".into()),
            ]
        );
        // the logic ICs' lettered outputs: QA is the least significant bit
        assert_eq!(
            lanes(&["V(QA)", "V(QB)", "V(QC)", "V(QD)", "V(CLK)", "V(OUTA)"]),
            vec![
                Lane::Bus { name: "Q[A..D]".into(), bits: vec!["V(QD)".into(), "V(QC)".into(), "V(QB)".into(), "V(QA)".into()] },
                Lane::Bit("V(CLK)".into()),
                Lane::Bit("V(OUTA)".into()),
            ]
        );
        // a lone numbered net stays a single lane
        assert_eq!(lanes(&["V(Q0)", "V(OUT)"]), vec![Lane::Bit("V(Q0)".into()), Lane::Bit("V(OUT)".into())]);
    }

    #[test]
    fn a_bus_reads_its_bits_with_the_thresholds_and_is_unknown_between_them() {
        let th = thresholds(5.0);
        assert_eq!(th, (1.5, 3.5));
        let (q1, q0) = ([5.0, 5.0, 2.5, 0.0], [0.0, 5.0, 5.0, 0.2]);
        let bits: [&[f64]; 2] = [&q1, &q0];
        let v: Vec<Option<u64>> = (0..4).map(|i| bus_value(&bits, i, th)).collect();
        assert_eq!(v, vec![Some(2), Some(3), None, Some(0)]);
        assert_eq!(format_bus(Some(10), 4, Radix::Dec), "10");
        assert_eq!(format_bus(Some(10), 8, Radix::Hex), "0Ah");
        assert_eq!(format_bus(Some(5), 4, Radix::Bin), "0101");
        assert_eq!(format_bus(None, 4, Radix::Hex), "X");
    }

    #[test]
    fn an_opened_bus_lists_its_bits_under_it() {
        let l = lanes(&["V(CLK)", "V(Q0)", "V(Q1)"]);
        assert_eq!(rows(&l, &[]).len(), 2);
        let r = rows(&l, &["Q[0..1]".to_string()]);
        assert_eq!(r.iter().map(|(l, inner)| (l.clone(), *inner)).collect::<Vec<_>>()[2], (Lane::Bit("V(Q1)".into()), true));
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn runs_keep_the_first_sample_of_each_value() {
        let v = [0, 0, 1, 1, 1, 0];
        assert_eq!(runs(v.len(), |i| v[i]), vec![(0, 0), (2, 1), (5, 0)]);
        assert_eq!(guess_vcc(&[&[0.0, 3.3, 3.29]]), 3.3);
        assert_eq!(guess_vcc(&[]), 5.0);
    }
}
