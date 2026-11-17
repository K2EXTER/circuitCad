//! A graph's axes read from the numbers printed around it: the column of tick labels on its left, the row under it,
//! linear or logarithmic, and the SI prefix of the axis titles' units ("(mA)"). The part form's graph window starts
//! with them, so a graph is read with one click on its curve.

use crate::text::{Rect, Word};

/// A tick label: its text and where it is printed.
#[derive(Clone, Debug)]
struct Label {
    text: String,
    value: f64,
    x0: f32,
    x1: f32,
    y: f32,
}

impl Label {
    fn cx(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }
}

/// One axis as read: the ticks (position along the axis: x across, y up; the label as printed), log or linear, and the
/// multiplier of its unit's prefix (1e-3 for "mA").
#[derive(Clone, Debug, PartialEq)]
pub struct ReadAxis {
    pub ticks: Vec<(f32, String)>,
    pub log: bool,
    pub prefix: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReadAxes {
    pub x: ReadAxis,
    pub y: ReadAxis,
}

/// The tick labels inside `area`: numbers (with an SI letter), and "10" with a raised exponent ("10" "2" read as 100,
/// the way Vishay writes a log axis).
fn labels(words: &[Word], area: Rect) -> Vec<Label> {
    let mut out: Vec<Label> = vec![];
    for w in words {
        let y = (w.rect.y0 + w.rect.y1) / 2.0;
        if y < area.y0 || y > area.y1 || w.rect.x1 < area.x0 || w.rect.x0 > area.x1 {
            continue;
        }
        let mut x = w.rect.x0;
        while x <= w.rect.x1 {
            if let Some((t, x0, x1)) = w.token_span_at(x) {
                let t = t.trim_end_matches('%');
                // "1x10" before a raised exponent (TI's UA78L): a "10" for the exponents below
                let t = if ["1x10", "1×10", "1X10", "1·10"].contains(&t) { "10" } else { t };
                // (the same number printed again over itself, a fraction of a point off: one label)
                if let Some(value) = crate::graph::tick_value(t)
                    && !out.iter().any(|l| ((l.x0 - x0).abs() < 0.1 && (l.y - y).abs() < 0.1) || (l.text == t && (l.x0 - x0).abs() < 1.5 && (l.y - y).abs() < 1.5))
                {
                    out.push(Label { text: t.to_string(), value, x0, x1, y });
                }
            }
            x += 0.5;
        }
    }
    // exponents: a small integer just right of and above a "10"
    let mut gone = vec![false; out.len()];
    for i in 0..out.len() {
        if out[i].text != "10" {
            continue;
        }
        if let Some(j) = (0..out.len()).find(|&j| {
            j != i && !gone[j] && out[j].text.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) && out[j].x0 >= out[i].x1 - 1.0 && out[j].x0 <= out[i].x1 + 4.0 && out[i].y - out[j].y > 1.0 && out[i].y - out[j].y < 6.0
        }) {
            let e: i32 = out[j].text.parse().unwrap_or(0);
            out[i].value = 10f64.powi(e);
            out[i].text = format!("1E{e}");
            out[i].x1 = out[j].x1;
            gone[j] = true;
        }
    }
    out.into_iter().zip(gone).filter(|(_, g)| !g).map(|(l, _)| l).collect()
}

/// The values change the same way along the positions (all up or all down), and no two share a place.
fn monotonic(v: &[(f32, f64)]) -> bool {
    v.len() >= 3 && v.windows(2).all(|w| (w[1].0 - w[0].0).abs() > 2.0) && (v.windows(2).all(|w| w[1].1 > w[0].1) || v.windows(2).all(|w| w[1].1 < w[0].1))
}

/// How far the labels miss an even scale: the largest miss of the straight line through the two end labels, in parts
/// of the axis (the value, or its logarithm, against the position).
fn miss(v: &[(f32, f64)], log: bool) -> f64 {
    if log && v.iter().any(|p| p.1 <= 0.0) {
        return f64::INFINITY;
    }
    let f = |x: f64| if log { x.log10() } else { x };
    let (a, b) = (v[0], v[v.len() - 1]);
    let (fa, fb) = (f(a.1), f(b.1));
    v.iter().map(|p| ((f(p.1) - fa) / (fb - fa) - ((p.0 - a.0) / (b.0 - a.0)) as f64).abs()).fold(0.0, f64::max)
}

/// A label of a linear axis that is off the line through the others by more than a tenth of a step (an OCR's "600"
/// among 0, 500, 1000). The line is the median of the slopes between neighbours and the median offset, so one bad
/// label does not move it.
fn misread(v: &[(f32, f64)]) -> Option<usize> {
    let median = |mut x: Vec<f64>| {
        x.sort_by(|a, b| a.total_cmp(b));
        x[x.len() / 2]
    };
    let slope = median(v.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0) as f64).collect());
    let offset = median(v.iter().map(|p| p.1 - slope * p.0 as f64).collect());
    let step = (v[v.len() - 1].1 - v[0].1).abs() / (v.len() - 1) as f64;
    v.iter().position(|p| (offset + slope * p.0 as f64 - p.1).abs() > 0.1 * step)
}

/// Whether an axis is log, and the labels that make it: all of them, or without a stray one at an end (a "0" under a
/// log axis, the next graph's first label). None when no even scale fits (the labels are not one axis).
fn settle(v: &[(f32, f64)]) -> Option<(std::ops::Range<usize>, bool)> {
    let n = v.len();
    for r in [0..n, 0..n - 1, 1..n] {
        let part = &v[r.clone()];
        if part.len() < 3 {
            continue;
        }
        let (lin, log) = (miss(part, false), miss(part, true));
        if lin.min(log) < 0.1 {
            // a linear axis is printed evenly: a label off its place by a tenth of a step is misread (by the OCR)
            if lin <= log && misread(part).is_some() {
                continue;
            }
            return Some((r, log < lin));
        }
    }
    None
}

/// The multiplier of a unit in brackets ("mA" 1e-3), None when it is not a unit.
fn bracket_unit(t: &str) -> Option<f64> {
    if let Some(k) = per_root_hertz(t) {
        return Some(k);
    }
    // "(mAdc)", "(Vdc)": a direct current or voltage (onsemi)
    let t = without_dc(t);
    let up = t.to_ascii_uppercase();
    if ["AMP", "AMPS", "VOLT", "VOLTS", "A", "V"].contains(&up.as_str()) {
        return Some(1.0);
    }
    for u in ["A", "V", "F", "s", "W", "Hz", "Ω"] {
        if let Some(p) = t.strip_suffix(u) {
            return match p {
                "" => Some(1.0),
                "m" => Some(1e-3),
                "µ" | "μ" | "u" => Some(1e-6),
                "n" => Some(1e-9),
                "p" => Some(1e-12),
                "k" => Some(1e3),
                _ => None,
            };
        }
    }
    None
}

/// The multiplier of a noise density's unit, a voltage or current per root hertz: "nV/√Hz" 1e-9, "µV/rtHz", and
/// "nVrms/Hz" as a title turned up the page reads without its root sign (ADR4525). None for any other unit.
fn per_root_hertz(t: &str) -> Option<f64> {
    let (num, den) = t.split_once('/')?;
    let den = den.trim_start_matches(['√', '(']).trim_start_matches("rt").trim_start_matches("sqrt").trim_end_matches(')').trim_start_matches('(');
    if den != "Hz" {
        return None;
    }
    let num = num.trim().trim_end_matches("rms").trim_end_matches("RMS").trim();
    ["V", "A"].iter().find_map(|u| num.strip_suffix(u)).and_then(|p| match p {
        "" => Some(1.0),
        "m" => Some(1e-3),
        "µ" | "μ" | "u" => Some(1e-6),
        "n" => Some(1e-9),
        "p" => Some(1e-12),
        "f" => Some(1e-15),
        _ => None,
    })
}

/// A unit without its "dc" ("mAdc" → "mA").
fn without_dc(t: &str) -> &str {
    t.strip_suffix("dc").or_else(|| t.strip_suffix("DC")).filter(|u| !u.is_empty()).unwrap_or(t)
}

/// The multiplier of a unit written in an axis title: "(mA)" 1e-3, "(µA)" 1e-6, "(pF)" 1e-12, "(A)" or "AMP" 1. A title
/// printed turned up the page comes as one letter per word: the letters of each column are read bottom to top.
pub(crate) fn unit_prefix(words: &[&Word]) -> Option<f64> {
    let mut columns: Vec<(f32, Vec<&Word>)> = vec![];
    for w in words.iter().filter(|w| w.text.chars().count() <= 2) {
        let x = (w.rect.x0 + w.rect.x1) / 2.0;
        match columns.iter_mut().find(|c| (c.0 - x).abs() < 2.5) {
            Some(c) => c.1.push(w),
            None => columns.push((x, vec![w])),
        }
    }
    for (_, mut col) in columns {
        col.sort_by(|a, b| b.rect.y0.total_cmp(&a.rect.y0));
        let text: String = col.iter().map(|w| w.text.as_str()).collect();
        let mut rest = text.as_str();
        while let Some(open) = rest.find('(') {
            let after = &rest[open + 1..];
            let Some(close) = after.find(')') else { break };
            if let Some(k) = bracket_unit(&after[..close]) {
                return Some(k);
            }
            rest = &after[close + 1..];
        }
    }
    for w in words {
        // a noise density's unit as one word ("(nV/√Hz)"; its tokens part at the slash)
        if let Some(k) = per_root_hertz(w.text.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']')) {
            return Some(k);
        }
        for t in w.tokens() {
            let raw = t.trim_matches(|c: char| c == '(' || c == ')' || c == ',' || c == '[' || c == ']');
            if let Some(k) = per_root_hertz(raw) {
                return Some(k);
            }
            let t = without_dc(raw);
            // a unit per something ("mW/cm²" is the unit the form wants as written)
            if w.text.contains(&format!("{t}/")) {
                continue;
            }
            let up = t.to_ascii_uppercase();
            if ["AMP", "AMPS", "VOLT", "VOLTS"].contains(&up.as_str()) {
                return Some(1.0);
            }
            let units = ["A", "V", "F", "s", "W", "Hz", "Ω"];
            for u in units {
                if let Some(p) = t.strip_suffix(u) {
                    let k = match p {
                        "" => 1.0,
                        "m" => 1e-3,
                        "µ" | "μ" | "u" => 1e-6,
                        "n" => 1e-9,
                        "p" => 1e-12,
                        "k" => 1e3,
                        _ => continue,
                    };
                    // a bare "A" or "V" is often a symbol in the title ("V" in "VF, ..."): only in brackets
                    if p.is_empty() && !w.text.contains(&format!("({raw})")) && !w.text.contains(&format!("[{raw}]")) {
                        continue;
                    }
                    return Some(k);
                }
            }
        }
    }
    None
}

/// The axes of the graph next to a caption (`figures::Figure`'s `area` and `rect`). With two graphs side by side under
/// one caption, the one nearest the caption's middle (the left one on a tie). None when the numbers do not make two
/// axes of three labels or more each (labels drawn as pictures, a bar chart, a graph with numbers inside).
pub fn read(words: &[Word], area: Rect, caption: Rect) -> Option<ReadAxes> {
    read_lined(words, area, caption, 2.0)
}

/// [`read`] for words read by the OCR: their boxes are a little uneven, so a column's labels line up within `line`
/// points rather than 2.
pub fn read_ocr(words: &[Word], area: Rect, caption: Rect) -> Option<ReadAxes> {
    read_lined(words, area, caption, 4.0)
}

fn read_lined(words: &[Word], area: Rect, caption: Rect, line: f32) -> Option<ReadAxes> {
    let wide = Rect { x0: area.x0 - 140.0, y0: area.y0 - 20.0, x1: area.x1 + 140.0, y1: area.y1 + 20.0 };
    let all = labels(words, wide);
    // a PNP's graph has negative numbers (−10 −100 −1000 on a log axis): when the axes do not come out as printed, they
    // are read as magnitudes (the form wants magnitudes)
    read_labels(&all, words, caption, line).or_else(|| {
        all.iter().any(|l| l.value < 0.0).then_some(())?;
        let magnitudes: Vec<Label> = all.iter().map(|l| Label { text: l.text.trim_start_matches(['-', '\u{2212}']).to_string(), value: l.value.abs(), ..l.clone() }).collect();
        read_labels(&magnitudes, words, caption, line)
    })
}

fn read_labels(all: &[Label], words: &[Word], caption: Rect, line: f32) -> Option<ReadAxes> {
    let mut best: Option<(f32, ReadAxes)> = None;
    // columns of labels on the left of a plot: right edges in line (or centres), values going one way; a column split
    // where the labels are far apart (two graphs one over the other)
    let mut cols: Vec<Vec<&Label>> = vec![];
    for l0 in all {
        for key in [|l: &Label| l.x1, |l: &Label| l.cx()] {
            let k0 = key(l0);
            let mut col: Vec<&Label> = all.iter().filter(|l| (key(l) - k0).abs() < line).collect();
            col.sort_by(|a, b| a.y.total_cmp(&b.y));
            col.dedup_by(|b, a| (a.y - b.y).abs() < 1.5);
            if col.len() < 3 {
                continue;
            }
            let mut gaps: Vec<f32> = col.windows(2).map(|w| w[1].y - w[0].y).collect();
            gaps.sort_by(|a, b| a.total_cmp(b));
            let limit = (2.5 * gaps[gaps.len() / 2]).max(30.0);
            let mut run: Vec<&Label> = vec![];
            for l in col {
                if run.last().is_some_and(|p: &&Label| l.y - p.y > limit) {
                    cols.push(std::mem::take(&mut run));
                }
                run.push(l);
            }
            cols.push(run);
        }
    }
    // the longest stretch of each run whose values go one way (the graph above's last label may line up with the
    // column: 1N5400's "0.01" over "100 10 1")
    let cols: Vec<Vec<&Label>> = cols
        .into_iter()
        .filter_map(|c| {
            let mut best: Option<(usize, usize)> = None;
            for i in 0..c.len() {
                for j in (i + 3..=c.len()).rev() {
                    if monotonic(&c[i..j].iter().map(|l| (l.y, l.value)).collect::<Vec<_>>()) {
                        if best.is_none_or(|(a, b)| j - i > b - a) {
                            best = Some((i, j));
                        }
                        break;
                    }
                }
            }
            best.map(|(i, j)| c[i..j].to_vec())
        })
        .collect();
    let mut cols = cols;
    cols.sort_by(|a, b| a[0].y.total_cmp(&b[0].y).then(a[0].x1.total_cmp(&b[0].x1)));
    cols.dedup_by(|b, a| a.len() == b.len() && a.iter().zip(b.iter()).all(|(p, q)| std::ptr::eq(*p, *q)));
    for col in &cols {
        {
            let pts: Vec<(f32, f64)> = col.iter().map(|l| (l.y, l.value)).collect();
            let (col_x1, bottom) = (col.iter().map(|l| l.x1).fold(f32::MIN, f32::max), col[col.len() - 1].y);
            // the row under the plot, starting at the column and up to the next graph
            // (within 25 points, or a step and a half of the column when its bottom label is not printed: "0")
            let mut steps: Vec<f32> = col.windows(2).map(|w| w[1].y - w[0].y).collect();
            steps.sort_by(|a, b| a.total_cmp(b));
            let reach = (1.5 * steps[steps.len() / 2]).max(25.0);
            let mut row: Vec<&Label> = all.iter().filter(|l| l.y > bottom + 1.0 && l.y < bottom + reach && l.cx() > col_x1 && l.x0 < col_x1 + 400.0).collect();
            // the nearest line with two labels or more (not a lone "0" at the foot of the next graph's column; not the
            // fuller row of a graph beside it, a little lower)
            let mut ys: Vec<f32> = row.iter().map(|l| l.y).collect();
            ys.sort_by(|a, b| a.total_cmp(b));
            // the line of the label nearest the column, when it has two labels or more (the graph beside it may print
            // its row a point or two higher)
            let line_of = |y: f32| row.iter().filter(|l| (l.y - y).abs() < 1.5).count();
            let first = row.iter().min_by(|a, b| a.cx().total_cmp(&b.cx())).map(|l| l.y).filter(|y| line_of(*y) >= 2);
            let Some(row_y) = first.or_else(|| ys.iter().copied().find(|y| line_of(*y) >= 2)) else { continue };
            row.retain(|l| (l.y - row_y).abs() < 1.5);
            row.sort_by(|a, b| a.cx().total_cmp(&b.cx()));
            if row.len() >= 3 {
                let mut gaps: Vec<f32> = row.windows(2).map(|w| w[1].cx() - w[0].cx()).collect();
                gaps.sort_by(|a, b| a.total_cmp(b));
                let median = gaps[gaps.len() / 2];
                if let Some(cut) = row.windows(2).position(|w| w[1].cx() - w[0].cx() > 2.5 * median.max(8.0)) {
                    row.truncate(cut + 1);
                }
            }
            // the next graph's row starts again from its low end
            if let Some(back) = row.windows(2).position(|w| w[1].value <= w[0].value) {
                row.truncate(back + 1);
            }
            let xp: Vec<(f32, f64)> = row.iter().map(|l| (l.cx(), l.value)).collect();
            // two labels a power of ten apart and well apart make a log axis by themselves (the OCR missed the "1" of
            // "1 10 100")
            let decades = |v: &[(f32, f64)]| {
                v.len() == 2 && v[0].1 > 0.0 && v[1].1 > v[0].1 && (v[1].0 - v[0].0) > 40.0 && {
                    let d = (v[1].1 / v[0].1).log10();
                    d >= 0.99 && (d - d.round()).abs() < 1e-6
                }
            };
            // the row starts at the plot's left edge, just right of the column (two decades: the first label may be
            // the missed one, a label's spacing further)
            let start = row.first().map_or(f32::MAX, |l| l.cx() - col_x1);
            if start > 40.0 && !(decades(&xp) && start <= xp[1].0 - xp[0].0 + 40.0) {
                continue;
            }
            let xs = if decades(&xp) { Some((0..2, true)) } else if monotonic(&xp) { settle(&xp) } else { None };
            let (Some((xr, xlog)), Some((yr, ylog))) = (xs, settle(&pts)) else { continue };
            let row = &row[xr];
            let col = &col[yr];
            let (x0, x1) = (row[0].x0, row[row.len() - 1].x1);
            // the titles: under the row (across), left of the column (up)
            let under: Vec<&Word> = words.iter().filter(|w| w.rect.y0 > row_y && w.rect.y0 < row_y + 22.0 && w.rect.x1 > x0 && w.rect.x0 < x1).collect();
            let left: Vec<&Word> = words.iter().filter(|w| w.rect.x1 < col[0].x0 + 1.0 && w.rect.x1 > col[0].x0 - 30.0 && w.rect.y1 > col[0].y - 10.0 && w.rect.y0 < bottom + 10.0).collect();
            let axes = ReadAxes {
                x: ReadAxis { ticks: row.iter().map(|l| (l.cx(), l.text.clone())).collect(), log: xlog, prefix: unit_prefix(&under).unwrap_or(1.0) },
                y: ReadAxis { ticks: col.iter().map(|l| (l.y, l.text.clone())).collect(), log: ylog, prefix: unit_prefix(&left).unwrap_or(1.0) },
            };
            // the graph nearest the caption's middle; on a tie (one caption under two graphs) the left one
            // under its own caption a graph spans the caption's middle; one caption under two graphs (TIP120's NPN and
            // PNP) is between them, and the left one is taken (the first part named)
            let mid = (caption.x0 + caption.x1) / 2.0;
            // (between columns of one graph, the one with more labels: right edges and centres may line up differently)
            let d = if x0 <= mid && mid <= x1 { 0.0 } else { 1e6 + ((x0 + x1) / 2.0 - mid).abs().min((mid - x1).abs()).max(0.0).div_euclid(150.0) * 1e3 } + x0 / 1000.0 - 0.01 * (col.len() + row.len()) as f32;
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, axes));
            }
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, x0: f32, y: f32) -> Word {
        Word::for_test(text, x0, y - 4.0, x0 + 4.0 * text.chars().count() as f32, y + 4.0)
    }

    #[test]
    fn a_log_log_graph_with_its_units() {
        // hFE 200 … 20000 up the left (right-aligned), IC 0.1 … 10 A across, "(AMP)" under it
        let mut words = vec![];
        for (i, v) in ["20000", "10000", "5000", "2000", "1000", "500", "200"].iter().enumerate() {
            let len = 4.0 * v.len() as f32;
            words.push(w(v, 100.0 - len, 100.0 + 25.0 * i as f32));
        }
        for (i, v) in ["0.1", "0.2", "0.5", "1.0", "2.0", "5.0", "10"].iter().enumerate() {
            words.push(w(v, 110.0 + 30.0 * i as f32, 262.0));
        }
        words.push(w("IC, COLLECTOR CURRENT (AMP)", 150.0, 275.0));
        // the next graph to the right
        words.push(w("20000", 330.0, 100.0));
        for (i, v) in ["0.1", "1.0", "10"].iter().enumerate() {
            words.push(w(v, 350.0 + 80.0 * i as f32, 262.0));
        }
        let caption = Rect { x0: 160.0, y0: 290.0, x1: 260.0, y1: 298.0 };
        let a = read(&words, Rect { x0: 100.0, y0: 60.0, x1: 320.0, y1: 298.0 }, caption).unwrap();
        assert_eq!(a.x.ticks.len(), 7, "{:?}", a.x.ticks);
        assert!(a.x.log && a.y.log);
        assert_eq!(a.x.prefix, 1.0);
        assert_eq!(a.y.ticks[0].1, "20000");
    }

    #[test]
    fn a_label_of_the_graph_above_in_line_with_the_column_is_left_out() {
        // "0.01" (the graph above's lowest label) right-aligned with "100 10 1"
        let mut words = vec![w("0.01", 84.0, 30.0)];
        for (i, v) in ["100", "10", "1"].iter().enumerate() {
            words.push(w(v, 100.0 - 4.0 * v.len() as f32, 100.0 + 65.0 * i as f32));
        }
        for (i, v) in ["0.1", "1", "10", "100"].iter().enumerate() {
            words.push(w(v, 104.0 + 52.0 * i as f32, 238.0));
        }
        let a = read(&words, Rect { x0: 80.0, y0: 20.0, x1: 330.0, y1: 260.0 }, Rect { x0: 150.0, y0: 260.0, x1: 250.0, y1: 268.0 }).unwrap();
        assert_eq!(a.y.ticks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), vec!["100", "10", "1"]);
        assert!(a.x.log && a.y.log);
    }

    #[test]
    fn a_pnp_graph_with_negative_numbers_is_read_as_magnitudes() {
        // hFE 0 … 100 up, IC −10 … −1000 mA across on a log axis
        let mut words = vec![];
        for (i, v) in ["100", "80", "60", "40", "20", "0"].iter().enumerate() {
            words.push(w(v, 100.0 - 4.0 * v.len() as f32, 100.0 + 30.0 * i as f32));
        }
        for (i, v) in ["\u{2212}10", "\u{2212}100", "\u{2212}1000"].iter().enumerate() {
            words.push(w(v, 102.0 + 90.0 * i as f32, 262.0));
        }
        let a = read(&words, Rect { x0: 80.0, y0: 60.0, x1: 330.0, y1: 280.0 }, Rect { x0: 150.0, y0: 280.0, x1: 250.0, y1: 288.0 }).unwrap();
        assert!(a.x.log && !a.y.log);
        assert_eq!(a.x.ticks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), vec!["10", "100", "1000"]);
    }

    #[test]
    fn a_number_printed_twice_over_itself_is_one_label() {
        let words = vec![w("0.1", 88.47, 644.9), w("0.1", 88.75, 644.8), w("1", 144.2, 644.9)];
        let l = labels(&words, Rect { x0: 0.0, y0: 0.0, x1: 500.0, y1: 900.0 });
        assert_eq!(l.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["0.1", "1"]);
    }

    #[test]
    fn two_labels_a_decade_apart_make_a_log_axis() {
        let mut words = vec![];
        for (i, v) in ["3000", "2500", "2000", "1500", "1000", "500"].iter().enumerate() {
            words.push(w(v, 100.0 - 4.0 * v.len() as f32, 100.0 + 34.0 * i as f32));
        }
        words.push(w("10", 190.0, 300.0));
        words.push(w("100", 290.0, 300.0));
        let a = read(&words, Rect { x0: 80.0, y0: 60.0, x1: 330.0, y1: 320.0 }, Rect { x0: 150.0, y0: 320.0, x1: 250.0, y1: 328.0 }).unwrap();
        assert!(a.x.log && a.x.ticks.len() == 2 && !a.y.log, "{a:?}");
        // two labels that are not a decade apart are not an axis
        words.retain(|w| w.text != "100");
        words.push(w("50", 290.0, 300.0));
        assert!(read(&words, Rect { x0: 80.0, y0: 60.0, x1: 330.0, y1: 320.0 }, Rect { x0: 150.0, y0: 320.0, x1: 250.0, y1: 328.0 }).is_none());
    }

    #[test]
    fn a_misread_label_of_a_linear_axis_is_left_out() {
        let even: Vec<(f32, f64)> = (0..7).map(|i| (100.0 + 30.0 * i as f32, 500.0 * i as f64)).collect();
        assert_eq!(settle(&even), Some((0..7, false)));
        let mut bad = even.clone();
        bad[1].1 = 600.0;
        // 600 where 500 belongs: no even scale with it (only the end labels may be dropped, and it is not one)
        assert_eq!(settle(&bad), None);
        bad[1].1 = 500.0;
        bad[0].1 = 100.0;
        assert_eq!(settle(&bad), Some((1..7, false)), "a misread end label is dropped");
    }

    #[test]
    fn a_linear_axis_in_milliamps_and_raised_exponents() {
        // IF 0 … 50 mA up, VF 0.5 … 1.5 V across
        let mut words = vec![];
        for (i, v) in ["50", "40", "30", "20", "10", "0"].iter().enumerate() {
            words.push(w(v, 100.0 - 4.0 * v.len() as f32, 100.0 + 30.0 * i as f32));
        }
        for (i, v) in ["0.5", "1.0", "1.5"].iter().enumerate() {
            words.push(w(v, 110.0 + 80.0 * i as f32, 262.0));
        }
        // "(mA)" turned up the page: one letter per word, ")" at the top
        for (i, c) in [")", "A", "m", "("].iter().enumerate() {
            words.push(Word::for_test(c, 70.0, 150.0 + 4.0 * i as f32, 74.0, 154.0 + 4.0 * i as f32));
        }
        let a = read(&words, Rect { x0: 80.0, y0: 60.0, x1: 300.0, y1: 280.0 }, Rect { x0: 150.0, y0: 280.0, x1: 250.0, y1: 288.0 }).unwrap();
        assert!(!a.x.log && !a.y.log);
        assert_eq!(a.y.prefix, 1e-3);
        // 10 with a raised 1, 2, 3 (Vishay)
        let mut words = vec![];
        for (i, e) in ["1", "2", "3"].iter().enumerate() {
            let x = 110.0 + 60.0 * i as f32;
            words.push(w("10", x, 262.0));
            words.push(Word::for_test(e, x + 8.5, 255.0, x + 11.0, 259.0));
        }
        let l = labels(&words, Rect { x0: 0.0, y0: 0.0, x1: 500.0, y1: 500.0 });
        assert_eq!(l.iter().map(|l| l.value).collect::<Vec<_>>(), vec![10.0, 100.0, 1000.0]);
        assert_eq!(l[1].text, "1E2");
    }

    #[test]
    fn a_unit_with_dc_after_it() {
        let t = |s: &str| Word::for_test(s, 0.0, 0.0, 10.0, 8.0);
        let (ma, v, vce) = (t("(mAdc)"), t("(Vdc)"), t("VCE,"));
        assert_eq!(unit_prefix(&[&t("COLLECTOR"), &ma]), Some(1e-3));
        assert_eq!(unit_prefix(&[&v]), Some(1.0));
        assert_eq!(bracket_unit("mAdc"), Some(1e-3));
        assert_eq!(unit_prefix(&[&vce]), None, "a symbol is not a unit");
    }

    #[test]
    fn a_noise_density_unit_gives_its_voltage_prefix() {
        let t = |s: &str| Word::for_test(s, 0.0, 0.0, 10.0, 8.0);
        assert_eq!(unit_prefix(&[&t("(nV/√Hz)")]), Some(1e-9));
        assert_eq!(unit_prefix(&[&t("µV/√Hz")]), Some(1e-6));
        assert_eq!(bracket_unit("nVrms/Hz"), Some(1e-9), "the root sign lost from a title turned up the page");
        assert_eq!(bracket_unit("µV/rtHz"), Some(1e-6));
        assert_eq!(unit_prefix(&[&t("mW/cm²")]), None, "other units per something stay as written");
    }
}
