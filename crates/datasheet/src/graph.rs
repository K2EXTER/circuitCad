//! Reading points off a graph drawn on a page (M10, step 2): each axis is set by ticks (a position on the page and
//! the value printed there), and a point's value comes from the two ticks nearest to it, linearly or on a log scale.
//! With more than two ticks the scale is piecewise, so a graph whose grid lines are not an exact log axis (the
//! 2N3906's Figure 15) is read right by giving every labelled tick. GUI-independent; the window is
//! `app/src/part_form/graph_reader.rs`.

/// A position along an axis (page points: x for the horizontal axis, y for the vertical) and the value there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    pub at: f32,
    pub value: f64,
}

/// One axis of a graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub log: bool,
    pub ticks: Vec<Tick>,
}

impl Axis {
    /// Why the axis cannot be used yet, in words for the person setting it (None: ready).
    pub fn problem(&self) -> Option<String> {
        let t = self.sorted();
        if t.len() < 2 {
            return Some("目盛りを 2 つ以上入れてください (離れた 2 つの数字。対数の軸は 10 倍ずつ離れたもの)".into());
        }
        if t.windows(2).any(|w| (w[1].at - w[0].at).abs() < 2.0) {
            return Some("同じ位置に目盛りが 2 つあります。どちらかを消してください".into());
        }
        if self.log && t.iter().any(|k| k.value <= 0.0) {
            return Some("対数の軸に 0 以下の値があります。対数の軸か、目盛りの値を確かめてください".into());
        }
        let up = t[1].value > t[0].value;
        if t.windows(2).any(|w| (w[1].value > w[0].value) != up || w[1].value == w[0].value) {
            return Some("目盛りの値が、位置の順に増えて (または減って) いません。値の入れ違いを確かめてください".into());
        }
        None
    }

    fn sorted(&self) -> Vec<Tick> {
        let mut t = self.ticks.clone();
        t.sort_by(|a, b| a.at.total_cmp(&b.at));
        t
    }

    /// The value at position `at`: between the two nearest ticks (outside them, the end pair is extended).
    pub fn value(&self, at: f32) -> Option<f64> {
        if self.problem().is_some() {
            return None;
        }
        let t = self.sorted();
        let i = t.windows(2).position(|w| at <= w[1].at).unwrap_or(t.len() - 2).min(t.len() - 2);
        let (a, b) = (t[i], t[i + 1]);
        let f = (at - a.at) as f64 / (b.at - a.at) as f64;
        Some(if self.log { 10f64.powf(a.value.log10() + f * (b.value.log10() - a.value.log10())) } else { a.value + f * (b.value - a.value) })
    }

    /// The position of `value` (the inverse of [`Axis::value`]), to draw the grid the ticks make.
    pub fn position(&self, value: f64) -> Option<f32> {
        if self.problem().is_some() || (self.log && value <= 0.0) {
            return None;
        }
        let t = self.sorted();
        let m = |v: f64| if self.log { v.log10() } else { v };
        let up = t[1].value > t[0].value;
        let i = t.windows(2).position(|w| if up { value <= w[1].value } else { value >= w[1].value }).unwrap_or(t.len() - 2);
        let (a, b) = (t[i], t[i + 1]);
        let f = (m(value) - m(a.value)) / (m(b.value) - m(a.value));
        Some(a.at + (f as f32) * (b.at - a.at))
    }
}

/// A graph on one page: its two axes and the points picked on it (page points).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub page: usize,
    pub x: Axis,
    pub y: Axis,
    pub points: Vec<(f32, f32)>,
}

impl Graph {
    /// The value of a page position, when both axes are set.
    pub fn value_at(&self, x: f32, y: f32) -> Option<(f64, f64)> {
        Some((self.x.value(x)?, self.y.value(y)?))
    }

    /// The picked points as values, in the order of the horizontal axis's value.
    pub fn values(&self) -> Option<Vec<(f64, f64)>> {
        let mut v: Vec<(f64, f64)> = self.points.iter().map(|&(x, y)| self.value_at(x, y)).collect::<Option<_>>()?;
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(v)
    }
}

/// The value of a tick label as printed ("10", "0.1", "1.0E-3", "10m", "1µ", "100k"): a number with an optional
/// SI prefix. None for anything else (a unit, a word).
pub fn tick_value(text: &str) -> Option<f64> {
    let t = text.trim().replace('\u{2212}', "-").replace(',', "");
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let (num, last) = t.split_at(t.len() - t.chars().last()?.len_utf8());
    let k = match last {
        "p" => 1e-12,
        "n" => 1e-9,
        "u" | "µ" | "μ" => 1e-6,
        "m" => 1e-3,
        "k" | "K" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        _ => return None,
    };
    num.parse::<f64>().ok().map(|v| v * k)
}

/// Every number printed on a page as (text, centre x, centre y), one per label (several labels read as one word are
/// split): the candidates for a graph's tick labels.
pub fn number_labels(words: &[crate::Word]) -> Vec<(String, f32, f32)> {
    let mut out: Vec<(String, f32, f32)> = vec![];
    for w in words {
        let y = (w.rect.y0 + w.rect.y1) / 2.0;
        let mut x = w.rect.x0;
        while x <= w.rect.x1 {
            if let Some((t, x0, x1)) = w.token_span_at(x)
                && tick_value(t).is_some()
                && !out.iter().any(|(s, a, b)| s == t && (*a - (x0 + x1) / 2.0).abs() < 0.1 && (*b - y).abs() < 0.1)
            {
                out.push((t.to_string(), (x0 + x1) / 2.0, y));
            }
            x += 0.5;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(log: bool, ticks: &[(f32, f64)]) -> Axis {
        Axis { log, ticks: ticks.iter().map(|&(at, value)| Tick { at, value }).collect() }
    }

    #[test]
    fn a_log_axis_from_two_decades_reads_in_between_and_beyond() {
        // 1 mA at 100 pt, 100 mA at 300 pt (one decade per 100 pt)
        let a = axis(true, &[(300.0, 0.1), (100.0, 0.001)]);
        assert!((a.value(200.0).unwrap() - 0.01).abs() < 1e-12);
        assert!((a.value(150.0).unwrap() / 10f64.powf(-2.5) - 1.0).abs() < 1e-9);
        assert!((a.value(400.0).unwrap() - 1.0).abs() < 1e-9, "beyond the last tick the end pair goes on");
        assert!((a.position(0.01).unwrap() - 200.0).abs() < 1e-4);
    }

    #[test]
    fn a_linear_axis_that_grows_downwards_on_the_page() {
        // a vertical axis: 0 V at the bottom (y = 500), 1 V at the top (y = 100)
        let a = axis(false, &[(500.0, 0.0), (100.0, 1.0)]);
        assert!((a.value(300.0).unwrap() - 0.5).abs() < 1e-12);
        assert!((a.position(0.25).unwrap() - 400.0).abs() < 1e-4);
    }

    #[test]
    fn more_ticks_follow_a_grid_that_is_not_exactly_logarithmic() {
        // labels at 1, 2, 5, 10 placed evenly (as on the 2N3906's Figure 15): with all four, 3 is read between 2 and 5
        let a = axis(true, &[(0.0, 1.0), (100.0, 2.0), (200.0, 5.0), (300.0, 10.0)]);
        let v = a.value(150.0).unwrap();
        assert!((v - (2.0f64 * 5.0).sqrt()).abs() < 1e-9, "{v}");
        // with only the ends, the same place would read 4.6 (a plain log axis)
        assert!((axis(true, &[(0.0, 1.0), (300.0, 10.0)]).value(200.0).unwrap() - 4.6416).abs() < 1e-3);
    }

    #[test]
    fn axes_that_cannot_be_used_say_why() {
        assert!(axis(false, &[(0.0, 1.0)]).problem().unwrap().contains("2 つ以上"));
        assert!(axis(true, &[(0.0, 0.0), (100.0, 10.0)]).problem().unwrap().contains("0 以下"));
        assert!(axis(false, &[(0.0, 1.0), (100.0, 3.0), (200.0, 2.0)]).problem().unwrap().contains("入れ違い"));
        assert!(axis(false, &[(0.0, 1.0), (0.5, 3.0)]).problem().unwrap().contains("同じ位置"));
        assert_eq!(axis(false, &[(0.0, 1.0)]).value(3.0), None);
    }

    #[test]
    fn points_come_out_as_values_in_the_order_of_x() {
        let g = Graph { page: 0, x: axis(false, &[(100.0, 0.0), (500.0, 1.0)]), y: axis(true, &[(400.0, 0.001), (100.0, 1.0)]), points: vec![(400.0, 100.0), (200.0, 300.0)] };
        let v = g.values().unwrap();
        assert!((v[0].0 - 0.25).abs() < 1e-9 && (v[0].1 - 0.01).abs() < 1e-9, "{v:?}");
        assert!((v[1].0 - 0.75).abs() < 1e-9 && (v[1].1 - 1.0).abs() < 1e-9, "{v:?}");
    }

    #[test]
    fn tick_labels_are_read_as_printed() {
        for (t, v) in [("10", 10.0), ("0.1", 0.1), ("1.0E-3", 1e-3), ("10m", 0.01), ("1µ", 1e-6), ("100k", 1e5), ("\u{2212}5", -5.0), ("1,000", 1000.0)] {
            assert_eq!(tick_value(t), Some(v), "{t}");
        }
        for t in ["mA", "VF", "", "Fig."] {
            assert_eq!(tick_value(t), None, "{t}");
        }
    }
}

/// On a real data sheet (the AO3400A's in `microohm-meter/`, on the author's PC; skipped elsewhere): the axes of
/// Figure 6 (body diode, log current) set from two labels each put the other labels where they are printed.
#[cfg(test)]
mod real_pdf {
    use super::*;

    #[test]
    fn labels_of_a_log_graph_line_up_with_its_axis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/AO3400A_ds.pdf");
        let Ok(ds) = crate::Datasheet::open_file(&path) else { return };
        let all = number_labels(ds.words(2));
        // Figure 6 is the lower right graph: its x labels are 0.0 .. 1.0 under it, its y labels 1.0E-05 .. 1.0E+01
        let find = |t: &str, right: bool| all.iter().filter(|l| l.0 == t && (l.1 > 300.0) == right).max_by(|a, b| a.2.total_cmp(&b.2)).cloned().unwrap_or_else(|| panic!("{t}: {all:?}"));
        let (x02, x08) = (find("0.2", true), find("0.8", true));
        let x = Axis { log: false, ticks: vec![Tick { at: x02.1, value: 0.2 }, Tick { at: x08.1, value: 0.8 }] };
        let (y4, y0) = (find("1.0E-04", true), find("1.0E+00", true));
        let y = Axis { log: true, ticks: vec![Tick { at: y4.2, value: 1e-4 }, Tick { at: y0.2, value: 1.0 }] };
        for (t, v) in [("0.4", 0.4), ("0.6", 0.6), ("1.0", 1.0)] {
            let l = find(t, true);
            let got = x.value(l.1).unwrap();
            assert!((got - v).abs() < 0.005, "{t}: read {got}");
        }
        for (t, v) in [("1.0E-05", 1e-5), ("1.0E-02", 1e-2), ("1.0E+01", 10.0)] {
            let l = find(t, true);
            let got = y.value(l.2).unwrap();
            assert!((got / v).log10().abs() < 0.02, "{t}: read {got}");
        }
    }
}
