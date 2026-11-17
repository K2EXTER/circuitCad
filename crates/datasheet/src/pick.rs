//! Which curve of a graph the part form wants, found without a click: every curve in the plot is followed
//! (`graph::trace_curve`), and the one next to the label that names it is taken ("VBE(sat) @ IC/IB = 10" on a
//! graph of three voltages, "25°C" among curves for several temperatures). A graph with one curve gives that one.

use crate::axes::ReadAxes;
use crate::figures::Curve;
use crate::text::{Rect, Word, normalize};

/// The curve found, and the label it was taken by (empty for a graph with one curve).
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    One { path: Vec<(f32, f32)>, by: String },
    /// Several curves and no label tells which: the person clicks one. The curves found (page points), to be shown as
    /// the ones to pick from.
    Several(Vec<Vec<(f32, f32)>>),
    None,
}

/// The plot's box: from the first to the last tick of each axis.
fn plot(axes: &ReadAxes) -> Option<Rect> {
    let span = |t: &[(f32, String)]| {
        let lo = t.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let hi = t.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        (hi - lo > 10.0).then_some((lo, hi))
    };
    let (x0, x1) = span(&axes.x.ticks)?;
    let (y0, y1) = span(&axes.y.ticks)?;
    Some(Rect { x0, y0, x1, y1 })
}

/// Two traces are the same curve: where both have points, they lie within 1.5 points of each other (half of them).
fn same(a: &[(f32, f32)], b: &[(f32, f32)]) -> bool {
    let near: Vec<f32> = a.iter().step_by(4).filter_map(|p| b.iter().filter(|q| (q.0 - p.0).abs() < 0.6).map(|q| (q.1 - p.1).abs()).min_by(|x, y| x.total_cmp(y))).collect();
    near.len() >= 4 && near.iter().filter(|d| **d < 1.5).count() * 2 > near.len()
}

/// The labels printed in the plot that name a curve the form wants, the most telling first: the field's own names
/// (`names`: "cob" for the output capacitance), one of the three voltages of an "On Voltages" graph, the single pulse
/// of a thermal response; then "25°C" (the data sheet's typical curve).
fn wanted(curve: Curve, names: &[&str], text: &str) -> Option<u8> {
    let t: String = normalize(text).chars().filter(|c| !c.is_whitespace()).collect();
    let has = |k: &str| t.contains(k);
    let own = names.iter().any(|n| {
        // a name as a word of its own ("Cob" in "Cob", not in "Cobalt")
        t.match_indices(n).any(|(i, _)| !t[i + n.len()..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
    }) || match curve {
        Curve::VbeSat => has("be(sat)") || has("besat"),
        Curve::VceSat => has("ce(sat)") || has("cesat"),
        Curve::VbeOn => has("be(on)") || (has("vbe") && !has("sat") && (has("@vce") || has("vce="))),
        Curve::ThermalImpedance => has("singlepulse") || has("single"),
        _ => false,
    };
    if own {
        return Some(0);
    }
    // 25 °C: not 125 or −25
    let room = t.match_indices("25").any(|(i, _)| {
        let before = t[..i].chars().last();
        let after = &t[i + 2..];
        !before.is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.') && ["°", "˚", "º", "degc", "c"].iter().any(|u| after.starts_with(u)) && !after.starts_with("cm")
    });
    room.then_some(1)
}

/// A label that names one curve of several: a temperature ("TJ = +125°C", "-55 °C").
fn names_a_curve(text: &str) -> bool {
    let t: String = normalize(text).chars().filter(|c| !c.is_whitespace()).collect();
    t.char_indices().any(|(i, c)| c.is_ascii_digit() && ["°c", "˚c", "ºc"].iter().any(|u| t[i + c.len_utf8()..].starts_with(u)))
}

/// The lines of words inside a box, each as its text and its box.
fn lines_in(words: &[Word], area: Rect) -> Vec<(String, Rect)> {
    let mut inside: Vec<&Word> = words.iter().filter(|w| w.rect.x0 >= area.x0 && w.rect.x1 <= area.x1 && w.rect.y0 >= area.y0 && w.rect.y1 <= area.y1).collect();
    inside.sort_by(|a, b| a.baseline().total_cmp(&b.baseline()).then(a.rect.x0.total_cmp(&b.rect.x0)));
    let mut out: Vec<(String, Rect, f32)> = vec![];
    for w in inside {
        match out.last_mut() {
            // the same line, and close (two labels side by side are two labels)
            Some((t, r, base)) if (*base - w.baseline()).abs() < 2.0 && w.rect.x0 - r.x1 < 2.0 * w.em().max(4.0) => {
                t.push(' ');
                t.push_str(&w.text);
                *r = Rect { x0: r.x0.min(w.rect.x0), y0: r.y0.min(w.rect.y0), x1: r.x1.max(w.rect.x1), y1: r.y1.max(w.rect.y1) };
            }
            _ => out.push((w.text.clone(), w.rect, w.baseline())),
        }
    }
    out.into_iter().map(|(t, r, _)| (t, r)).collect()
}

/// The distance from a label to a curve (to its nearest point).
fn distance(label: Rect, path: &[(f32, f32)]) -> f32 {
    path.iter()
        .map(|&(x, y)| {
            let dx = (label.x0 - x).max(x - label.x1).max(0.0);
            let dy = (label.y0 - y).max(y - label.y1).max(0.0);
            (dx * dx + dy * dy).sqrt()
        })
        .fold(f32::MAX, f32::min)
}

/// The part of a trace before it turns sharply (more than 50° within a few points): a curve is smooth, and a trace that
/// kinks has jumped onto another curve or a line where curves cross. The longest smooth stretch is kept.
fn smooth_part(path: &[(f32, f32)]) -> Vec<(f32, f32)> {
    const STEP: usize = 6; // 3 points at half-point steps
    if path.len() < 3 * STEP {
        return path.to_vec();
    }
    let dir = |i: usize| (path[i + STEP].1 - path[i].1).atan2(path[i + STEP].0 - path[i].0);
    let mut cuts = vec![0];
    let mut i = STEP;
    while i + STEP < path.len() {
        let turn = (dir(i) - dir(i - STEP)).abs();
        if turn.min(2.0 * std::f32::consts::PI - turn) > 50f32.to_radians() {
            cuts.push(i);
            i += STEP;
        }
        i += 1;
    }
    cuts.push(path.len());
    let best = cuts.windows(2).max_by_key(|w| w[1] - w[0]).map_or(0..path.len(), |w| w[0]..w[1]);
    path[best].to_vec()
}

/// Pieces of traces joined where one carries on another: the next piece starts less than 25 points to the right of
/// where one ends, on the line the end was going along (within 2 points and a fifth of the gap). The nearest such
/// piece is taken, so curves running side by side stay apart.
fn joined(mut pieces: Vec<Vec<(f32, f32)>>) -> Vec<Vec<(f32, f32)>> {
    pieces.sort_by(|a, b| a[0].0.total_cmp(&b[0].0));
    let mut out: Vec<Vec<(f32, f32)>> = vec![];
    let mut used = vec![false; pieces.len()];
    for i in 0..pieces.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut path = pieces[i].clone();
        loop {
            let end = *path.last().unwrap();
            // the slope over the last 3 points of the trace
            let back = path.iter().rev().find(|p| end.0 - p.0 >= 3.0).copied();
            let slope = back.map_or(0.0, |p| (end.1 - p.1) / (end.0 - p.0));
            let next = (0..pieces.len())
                .filter(|&j| !used[j])
                .filter_map(|j| {
                    let s = pieces[j][0];
                    let gap = s.0 - end.0;
                    let off = (s.1 - (end.1 + slope * gap)).abs();
                    (gap > -0.5 && gap < 25.0 && off < 2.0 + 0.2 * gap.max(0.0)).then_some((off + gap * 0.1, j))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, j)) = next else { break };
            used[j] = true;
            path.extend(pieces[j].iter().copied());
        }
        out.push(path);
    }
    out
}

/// The way a curve of this kind goes, in its value against the horizontal value: up (a diode's current with its
/// voltage), down (a capacitance with the reverse voltage), or either (hFE and CTR rise and fall).
fn rising(curve: Curve) -> Option<bool> {
    match curve {
        Curve::Forward | Curve::LightCurrent | Curve::VbeOn | Curve::VbeSat | Curve::VceSat | Curve::ThermalImpedance => Some(true),
        Curve::Capacitance => Some(false),
        Curve::DcGain | Curve::Ctr => None,
    }
}

/// The trace goes the way its kind of curve goes (allowing 3 points of wobble against it): one that does not has
/// mixed curves up.
fn goes_its_way(path: &[(f32, f32)], curve: Curve, axes: &ReadAxes) -> bool {
    let Some(up) = rising(curve) else { return true };
    // the value grows towards the top of the page (or, rarely, down)
    let (top, bottom) = (axes.y.ticks.iter().min_by(|a, b| a.0.total_cmp(&b.0)), axes.y.ticks.iter().max_by(|a, b| a.0.total_cmp(&b.0)));
    let grows_up = match (top.and_then(|t| crate::graph::tick_value(&t.1)), bottom.and_then(|t| crate::graph::tick_value(&t.1))) {
        (Some(t), Some(b)) => t > b,
        _ => true,
    };
    // the page's y should fall along the trace when the value rises upward
    let falls = up == grows_up;
    let against: f32 = path.windows(2).map(|w| if falls { (w[1].1 - w[0].1).max(0.0) } else { (w[0].1 - w[1].1).max(0.0) }).sum();
    against < 3.0
}

/// The curve of the graph whose axes are `axes` that the form's field wants (`curve`, and the names its label may
/// have: `["cob"]`). `img` is the page drawn at `px_per_pt`.
pub fn pick(img: &crate::PageImage, px_per_pt: f32, words: &[Word], axes: &ReadAxes, curve: Curve, names: &[&str]) -> Pick {
    let Some(b) = plot(axes) else { return Pick::None };
    let skip: Vec<Rect> = words.iter().map(|w| w.rect).collect();
    // every curve: crossed by a few vertical lines through the plot, each followed to its ends
    let mut traces: Vec<Vec<(f32, f32)>> = vec![];
    for f in [0.2, 0.4, 0.6, 0.8] {
        let x = b.x0 + f * (b.x1 - b.x0);
        for y in crate::graph::curve_crossings(img, px_per_pt, x, b.y0 + 1.0, b.y1 - 1.0, &skip) {
            if traces.iter().any(|t| t.iter().any(|p| (p.0 - x).abs() < 0.6 && (p.1 - y).abs() < 1.5)) {
                continue;
            }
            let path: Vec<(f32, f32)> = crate::graph::trace_curve(img, px_per_pt, (x, y), &skip).into_iter().filter(|p| p.0 >= b.x0 - 1.0 && p.0 <= b.x1 + 1.0 && p.1 >= b.y0 - 1.0 && p.1 <= b.y1 + 1.0).collect();
            let path = smooth_part(&path);
            if path.len() >= 2 && !traces.iter().any(|t| same(t, &path)) {
                traces.push(path);
            }
        }
    }
    // a curve broken where it crosses a black grid line (Vishay's 1N5400) is put back together; then only curves, not
    // short marks (an arrow, a bit of a label)
    let mut traces: Vec<Vec<(f32, f32)>> = joined(traces).into_iter().filter(|p| p.last().zip(p.first()).map_or(0.0, |(l, f)| l.0 - f.0) > 0.15 * (b.x1 - b.x0)).collect();
    // the labels in the plot (and a little around it) that name the wanted curve, the most telling kind first
    let around = Rect { x0: b.x0 - 5.0, y0: b.y0 - 5.0, x1: b.x1 + 5.0, y1: b.y1 + 5.0 };
    let mut labels: Vec<(u8, String, Rect)> = lines_in(words, around).into_iter().filter_map(|(t, r)| wanted(curve, names, &t).map(|k| (k, t, r))).collect();
    labels.sort_by_key(|l| l.0);
    match traces.len() {
        0 => return Pick::None,
        // one curve found: that one, unless the field's own label is printed away from it (the other curves of the
        // graph were not found)
        1 if !labels.iter().any(|l| l.0 == 0 && distance(l.2, &traces[0]) >= 25.0) => {
            return if goes_its_way(&traces[0], curve, axes) { Pick::One { path: traces.remove(0), by: String::new() } } else { Pick::Several(traces) };
        }
        _ => {}
    }
    let Some(kind) = labels.first().map(|l| l.0) else { return Pick::Several(traces) };
    // a line going the wrong way for the curve (a label's leader line) is not what a label names
    let all = traces.clone();
    traces.retain(|t| goes_its_way(t, curve, axes));
    // the curve nearest to one of the labels of that kind; a label far from every curve (a note in a corner) tells nothing
    // and, for a temperature, not a curve another temperature's label is nearer to ("+25°C" printed on its curve where that curve was not traced
    // takes the +125 °C curve passing by, which its own "+125°C" is nearer to: 2N3903's Figure 15)
    let others: Vec<Rect> = lines_in(words, around).into_iter().filter(|(t, _)| names_a_curve(t)).map(|l| l.1).collect();
    let best = labels
        .iter()
        .filter(|l| l.0 == kind)
        .flat_map(|l| traces.iter().enumerate().map(move |(i, t)| (distance(l.2, t), i, l.1.clone(), l.2)))
        .filter(|(d, i, by, r)| !names_a_curve(by) || !others.iter().any(|o| o != r && distance(*o, &traces[*i]) < *d - 1.0))
        .map(|(d, i, by, _)| (d, i, by))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match best {
        Some((d, i, by)) if d < 25.0 => Pick::One { path: traces.swap_remove(i), by },
        _ => Pick::Several(all),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axes::ReadAxis;

    /// A 200 x 120 point page at 2 pixels per point with straight black curves y = a + s x across a plot from x 20 to
    /// 180 and y 10 to 110.
    fn page(curves: &[(f32, f32)]) -> crate::PageImage {
        let k = 2.0f32;
        let (w, h) = ((200.0 * k) as usize, (120.0 * k) as usize);
        let mut rgba = vec![255u8; w * h * 4];
        for &(a, s) in curves {
            let mut x = 20.0f32;
            while x <= 180.0 {
                let y = a + s * (x - 20.0);
                if (10.0..=110.0).contains(&y) {
                    for dy in 0..2 {
                        let (px, py) = ((x * k) as usize, (y * k) as usize + dy);
                        let i = (py * w + px) * 4;
                        rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
                    }
                }
                x += 0.1;
            }
        }
        crate::PageImage { width: w, height: h, rgba }
    }

    fn axes() -> ReadAxes {
        // values grow to the right and up the page
        let t = |a: f32, b: f32| vec![(a, "0".to_string()), ((a + b) / 2.0, "1".to_string()), (b, "2".to_string())];
        ReadAxes { x: ReadAxis { ticks: t(20.0, 180.0), log: false, prefix: 1.0 }, y: ReadAxis { ticks: t(110.0, 10.0), log: false, prefix: 1.0 } }
    }

    fn label(text: &str, x: f32, y: f32) -> Word {
        Word::for_test(text, x, y - 3.0, x + 3.0 * text.chars().count() as f32, y + 3.0)
    }

    #[test]
    fn the_curve_next_to_its_label_is_taken() {
        // three rising voltages (not flat: flat lines are taken for the grid): VBE(sat) from y 40, VBE(on) from 65,
        // VCE(sat) from 95
        let img = page(&[(40.0, -0.15), (65.0, -0.15), (95.0, -0.15)]);
        let words = vec![label("VBE(sat) @ IC/IB = 10", 60.0, 40.0), label("VBE @ VCE = 1.0 V", 60.0, 65.0), label("VCE(sat) @ IC/IB = 10", 60.0, 95.0), label("TJ = 25°C", 140.0, 15.0)];
        let y_at = |p: &Pick, x: f32| match p {
            Pick::One { path, .. } => path.iter().find(|q| (q.0 - x).abs() < 0.3).map(|q| q.1),
            _ => None,
        };
        let p = pick(&img, 2.0, &words, &axes(), Curve::VbeSat, &[]);
        assert!((y_at(&p, 100.0).unwrap() - 28.5).abs() < 1.5, "{p:?}");
        let p = pick(&img, 2.0, &words, &axes(), Curve::VceSat, &[]);
        assert!((y_at(&p, 100.0).unwrap() - 83.5).abs() < 1.5);
        let p = pick(&img, 2.0, &words, &axes(), Curve::VbeOn, &[]);
        assert!((y_at(&p, 100.0).unwrap() - 53.5).abs() < 1.5);
        // hFE for three temperatures: the 25 °C one
        let img = page(&[(40.0, -0.2), (60.0, -0.2), (80.0, -0.2)]);
        let words = vec![label("150°C", 30.0, 36.0), label("25°C", 30.0, 56.0), label("−55°C", 30.0, 76.0)];
        let p = pick(&img, 2.0, &words, &axes(), Curve::DcGain, &[]);
        assert!(matches!(&p, Pick::One { by, .. } if by == "25°C"), "{p:?}");
        assert!((y_at(&p, 100.0).unwrap() - 44.5).abs() < 1.5);
        // no label: the person picks
        assert!(matches!(pick(&img, 2.0, &[], &axes(), Curve::DcGain, &[]), Pick::Several(c) if c.len() == 3));
        // the 25 °C curve not traced (2N3903's Figure 15): its label does not take the 150 °C curve, which its own
        // label is nearer to
        let img = page(&[(40.0, -0.2), (80.0, -0.2)]);
        let words = vec![label("150°C", 30.0, 36.0), label("25°C", 30.0, 50.0), label("−55°C", 30.0, 76.0)];
        assert!(matches!(pick(&img, 2.0, &words, &axes(), Curve::DcGain, &[]), Pick::Several(c) if c.len() == 2));
        // one curve: that one
        let img = page(&[(60.0, -0.2)]);
        assert!(matches!(pick(&img, 2.0, &[], &axes(), Curve::Forward, &[]), Pick::One { by, .. } if by.is_empty()));
    }

    #[test]
    fn a_trace_that_kinks_or_goes_the_wrong_way_is_not_taken() {
        // a line up to x 50, then a sharp turn down (onto another curve): the longer stretch is kept
        let mut path: Vec<(f32, f32)> = (0..200).map(|i| (i as f32 * 0.5, 100.0 - i as f32 * 0.25)).collect();
        path.extend((0..60).map(|i| (100.0 + i as f32 * 0.5, 50.0 + i as f32 * 1.0)));
        let s = smooth_part(&path);
        assert!(s.len() >= 190 && s.len() <= 210 && s[0] == (0.0, 100.0), "{}", s.len());
        // a forward curve rises: its page y falls along it
        let a = axes();
        assert!(!goes_its_way(&[(20.0, 50.0), (30.0, 60.0), (40.0, 70.0)], Curve::Forward, &a));
        let ticks = |v: &[(f32, &str)]| v.iter().map(|(p, t)| (*p, t.to_string())).collect();
        let up = ReadAxes { x: a.x.clone(), y: ReadAxis { ticks: ticks(&[(10.0, "100"), (60.0, "10"), (110.0, "1")]), log: true, prefix: 1.0 } };
        assert!(goes_its_way(&[(20.0, 90.0), (30.0, 70.0), (40.0, 30.0)], Curve::Forward, &up));
        assert!(!goes_its_way(&[(20.0, 30.0), (30.0, 70.0), (40.0, 90.0)], Curve::Forward, &up));
        assert!(goes_its_way(&[(20.0, 30.0), (30.0, 70.0), (40.0, 90.0)], Curve::Capacitance, &up));
        assert!(goes_its_way(&[(20.0, 30.0), (30.0, 70.0), (40.0, 30.0)], Curve::DcGain, &up));
    }

    #[test]
    fn a_curve_broken_at_grid_lines_is_put_back_together() {
        // y = 100 − x / 2 in three pieces with gaps at x 40 and 80, and a curve 20 points below in one piece
        let line = |x0: i32, x1: i32, a: f32| (x0 * 2..=x1 * 2).map(|i| (i as f32 / 2.0, a - i as f32 / 4.0)).collect::<Vec<_>>();
        let j = joined(vec![line(52, 80, 100.0), line(10, 38, 100.0), line(10, 120, 120.0), line(84, 120, 100.0)]);
        assert_eq!(j.len(), 2, "{:?}", j.iter().map(|p| (p[0], *p.last().unwrap())).collect::<Vec<_>>());
        let top = j.iter().find(|p| p[0].1 < 100.0).unwrap();
        assert_eq!((top[0].0, top.last().unwrap().0), (10.0, 120.0));
    }

    #[test]
    fn room_temperature_is_25_degrees_only() {
        for (t, want) in [("25°C", true), ("TA = 25 °C", true), ("Tj=+25˚C", true), ("125°C", false), ("−25°C", false), ("2.5°C", false), ("VCE = 25 V", false), ("mW/cm2", false)] {
            assert_eq!(wanted(Curve::DcGain, &[], t).is_some(), want, "{t}");
        }
        assert_eq!(wanted(Curve::Capacitance, &["cob"], "Cob"), Some(0));
        assert_eq!(wanted(Curve::Capacitance, &["cob"], "Cobalt"), None);
        assert_eq!(wanted(Curve::Capacitance, &["cib"], "Cob"), None);
        assert_eq!(wanted(Curve::ThermalImpedance, &[], "SINGLE PULSE"), Some(0));
    }
}
