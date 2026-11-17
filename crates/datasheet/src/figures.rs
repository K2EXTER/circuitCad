//! The captions of a data sheet's graphs ("Figure 9. DC Current Gain", "Fig. 3 - Reverse Light Current vs. Irradiance")
//! and which curve of a part each graph shows, so that a field of the part form can be taken to its graph.

use crate::text::{Rect, Word, normalize};

/// A curve the part form reads off a graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// Forward current against forward voltage (a diode, an LED).
    Forward,
    /// Capacitance against reverse voltage.
    Capacitance,
    /// A transistor's DC current gain (hFE) against collector current.
    DcGain,
    VbeOn,
    VbeSat,
    VceSat,
    /// Transient thermal impedance against pulse time.
    ThermalImpedance,
    /// An optocoupler's current transfer ratio against forward current.
    Ctr,
    /// A photodiode's light current against irradiance.
    LightCurrent,
}

/// A graph's caption on a page.
#[derive(Clone, Debug)]
pub struct Figure {
    pub page: usize,
    /// Where the caption is (its first line).
    pub rect: Rect,
    /// The caption as printed, a second line joined when the first breaks off ("… vs. Forward" / "Current").
    pub caption: String,
    pub curves: Vec<Curve>,
    /// Where the graph itself likely is (above or below the caption, as the maker places them; `place_graphs`), to
    /// bring the graph into view rather than its caption.
    pub area: Rect,
    /// The area on the other side of the caption.
    other: Rect,
    /// Numbers printed in the caption's column above and below it (tick labels of a graph).
    numbers: (usize, usize),
}

/// How far a graph reaches from its caption (points).
const GRAPH_REACH: f32 = 230.0;

/// The areas above and below a caption, and how many numbers are in each.
fn sides(words: &[Word], caption: Rect) -> (Rect, Rect, (usize, usize)) {
    let (x0, x1) = (caption.x0 - 60.0, caption.x1.max(caption.x0 + 200.0) + 60.0);
    let above = Rect { x0, y0: caption.y0 - GRAPH_REACH, x1, y1: caption.y1 };
    let below = Rect { x0, y0: caption.y0, x1, y1: caption.y1 + GRAPH_REACH };
    let numbers = |y0: f32, y1: f32| {
        words
            .iter()
            .filter(|w| {
                let (cx, cy) = ((w.rect.x0 + w.rect.x1) / 2.0, (w.rect.y0 + w.rect.y1) / 2.0);
                cx > x0 && cx < x1 && cy > y0 && cy < y1
            })
            .flat_map(|w| w.tokens())
            .filter(|t| crate::graph::tick_value(t).is_some())
            .count()
    };
    (above, below, (numbers(above.y0, caption.y0), numbers(caption.y1, below.y1)))
}

/// Puts each graph's area on the side of its caption the maker uses: the topmost caption of a page has a graph on
/// its side only (above it there is only the page's header), so the pages' topmost captions vote. Without tick labels
/// as text (drawn as lines) there is no vote, and the graphs stay above, as most makers place them.
pub fn place_graphs(figures: &mut [Figure]) {
    let mut pages: Vec<usize> = figures.iter().map(|f| f.page).collect();
    pages.dedup();
    let (mut above, mut below) = (0, 0);
    for p in pages {
        if let Some(top) = figures.iter().filter(|f| f.page == p).min_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0)) {
            match top.numbers {
                (a, _) if a >= 3 => above += 1,
                (_, b) if b >= 3 => below += 1,
                _ => {}
            }
        }
    }
    if below > above {
        for f in figures {
            std::mem::swap(&mut f.area, &mut f.other);
        }
    }
}

/// Starts a caption: "Fig.", "Fig", "Figure", "Fig.7", "図1" (not "Figures", nor "Noise Figure" in a table).
fn starts_caption(w: &Word, next: Option<&&Word>) -> bool {
    let t = normalize(&w.text);
    let rest = if let Some(r) = t.strip_prefix("figure") {
        r
    } else if let Some(r) = t.strip_prefix("fig") {
        r
    } else if let Some(r) = t.strip_prefix('図') {
        r
    } else {
        return false;
    };
    let rest = rest.trim_start_matches('.');
    if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return true;
    }
    // the number is the next word ("Figure 7.", "Fig. 3")
    rest.is_empty() && next.is_some_and(|n| n.text.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// The words of a page in lines (by baseline), each left to right.
fn lines(words: &[Word]) -> Vec<Vec<&Word>> {
    let mut sorted: Vec<&Word> = words.iter().collect();
    sorted.sort_by(|a, b| a.baseline().total_cmp(&b.baseline()).then(a.rect.x0.total_cmp(&b.rect.x0)));
    let mut out: Vec<Vec<&Word>> = vec![];
    for w in sorted {
        match out.last_mut() {
            Some(l) if (l[0].baseline() - w.baseline()).abs() < 2.0 => l.push(w),
            _ => out.push(vec![w]),
        }
    }
    for l in &mut out {
        l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    }
    out
}

/// A word as a plain lower case word without the punctuation around it.
fn plain(w: &str) -> String {
    normalize(w).trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// The caption's text breaks off and goes on on the next line: it ends on a joining word, or has a "vs." with at most
/// one word after it; or the next line starts with "vs.".
fn runs_on(text: &[&str], next: &[&str]) -> bool {
    let words: Vec<String> = text.iter().map(|t| plain(t)).filter(|t| !t.is_empty()).collect();
    let Some(last) = words.last() else { return false };
    if ["vs", "versus", "with", "of", "and", "to", "on", "for"].contains(&last.as_str()) {
        return true;
    }
    if let Some(i) = words.iter().rposition(|w| w == "vs" || w == "versus") {
        return words.len() - i - 1 <= 1;
    }
    next.first().is_some_and(|f| ["vs", "versus"].contains(&plain(f).as_str()))
}

/// The graph captions of a page.
pub fn figures(page: usize, words: &[Word]) -> Vec<Figure> {
    let lines = lines(words);
    let mut out = vec![];
    for (li, line) in lines.iter().enumerate() {
        // where captions start on the line: the first word, or after a wide gap (two graphs side by side)
        let starts: Vec<usize> = (0..line.len())
            .filter(|&i| {
                let gap_before = i == 0 || line[i].rect.x0 - line[i - 1].rect.x1 > 2.0 * line[i].em().max(4.0);
                gap_before && starts_caption(line[i], line.get(i + 1))
            })
            .collect();
        for (k, &s) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(line.len());
            let own = &line[s..end];
            // a sentence that starts with a figure's name ("Figure 7-1 shows the typical application circuit …")
            if own.len() > 14 {
                continue;
            }
            let em = own[0].em().max(4.0);
            let x0 = own[0].rect.x0;
            let x1 = own.iter().map(|w| w.rect.x1).fold(f32::MIN, f32::max);
            let right = starts.get(k + 1).map_or(f32::MAX, |&n| line[n].rect.x0);
            let rect = own.iter().fold(own[0].rect, |r, w| Rect { x0: r.x0.min(w.rect.x0), y0: r.y0.min(w.rect.y0), x1: r.x1.max(w.rect.x1), y1: r.y1.max(w.rect.y1) });
            let mut text: Vec<&str> = own.iter().map(|w| w.text.as_str()).collect();
            // the line below, under this caption only
            if let Some(below) = lines.get(li + 1)
                && below[0].baseline() - own[0].baseline() < 2.4 * em
            {
                let more: Vec<&str> = below.iter().filter(|w| w.rect.x0 >= x0 - 2.0 * em && w.rect.x1 <= x1 + 2.0 * em && w.rect.x0 < right).map(|w| w.text.as_str()).collect();
                if !more.is_empty() && !below.iter().any(|w| w.rect.x0 >= x0 - 2.0 * em && w.rect.x0 < right && starts_caption(w, None)) && runs_on(&text, &more) {
                    text.extend(more);
                }
            }
            let caption = text.join(" ");
            let curves = curves(&caption);
            let (area, other, numbers) = sides(words, rect);
            out.push(Figure { page, rect, caption, curves, area, other, numbers });
        }
    }
    out
}

/// What a caption's graph shows, from its words. Graphs against temperature, relative or normalised values, ratings
/// (derating, safe operating area) and test circuits show none of the curves the form reads.
pub fn curves(caption: &str) -> Vec<Curve> {
    let mut t = String::new();
    for c in normalize(caption).chars() {
        t.push(if c.is_alphanumeric() { c } else { ' ' });
    }
    let t = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    let has = |s: &str, keys: &[&str]| keys.iter().any(|k| s.contains(k));
    if has(&t, &[" derating", " safe operating", " test circuit", " waveform", " surge", " dissipation", " power rating", " schematic", " drawing", "定格"]) {
        return vec![];
    }
    if has(&t, &[" transient thermal", " thermal response", "過渡熱"]) {
        return vec![Curve::ThermalImpedance];
    }
    // what is shown, and what it is shown against
    let split = [" vs ", " versus ", " variation with "].iter().filter_map(|s| t.find(s).map(|i| (i, s.len()))).min();
    let (what, against) = match split {
        Some((i, n)) => (&t[..i + 1], &t[i + n - 1..]),
        None => (t.as_str(), ""),
    };
    if has(what, &[" relative ", " normalized ", " normalised ", " small signal", " bandwidth", "相対"])
        || has(against, &[" temperature", " ambient", " wavelength", " angular", " duty", " illuminance", " time ", " load resistance", " lead length", "温度"])
    {
        return vec![];
    }
    if has(what, &[" capacitance", "容量"]) && !has(what, &[" gate charge"]) {
        return vec![Curve::Capacitance];
    }
    if has(what, &[" current transfer ratio", " ctr ", "電流伝達率"]) {
        return if against.is_empty() || has(against, &[" forward", "順"]) { vec![Curve::Ctr] } else { vec![] };
    }
    if has(what, &[" light current", " photocurrent", " photo current", "光電流"]) {
        return if has(against, &[" irradiance", "放射照度"]) { vec![Curve::LightCurrent] } else { vec![] };
    }
    if has(what, &[" forward current", " forward voltage", " forward characteristic", "順方向"]) && !has(what, &[" reverse"]) {
        return vec![Curve::Forward];
    }
    // a transistor's curves are against collector current (an optocoupler's saturation voltage is against forward current)
    if has(against, &[" forward"]) {
        return vec![];
    }
    if has(what, &[" current gain", " hfe ", "電流増幅率"]) {
        return vec![Curve::DcGain];
    }
    if has(what, &[" base emitter saturation"]) {
        return vec![Curve::VbeSat];
    }
    if has(what, &[" collector emitter saturation voltage", " collector saturation voltage"]) {
        return vec![Curve::VceSat];
    }
    if has(what, &[" base emitter on voltage"]) {
        return vec![Curve::VbeOn];
    }
    // "On" Voltages, "Saturation" and "On" Voltages: the three on one graph
    if has(what, &[" on voltage"]) {
        return vec![Curve::VbeOn, Curve::VbeSat, Curve::VceSat];
    }
    vec![]
}

/// The figures that show `curve`, the likeliest first: a "DC current gain" before a plain "current gain" (that can be the
/// small signal one), then in page order.
pub fn showing(figures: &[Figure], curve: Curve) -> Vec<&Figure> {
    let mut v: Vec<&Figure> = figures.iter().filter(|f| f.curves.contains(&curve)).collect();
    let rank = |f: &Figure| {
        let t = normalize(&f.caption);
        u8::from(!(curve == Curve::DcGain && t.contains("dc")))
    };
    v.sort_by_key(|f| rank(f));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captions_say_which_curve() {
        use Curve::*;
        for (caption, want) in [
            ("Figure 9. DC Current Gain", vec![DcGain]),
            ("Fig.7 Forward Current vs. Forward Voltage", vec![Forward]),
            ("Fig. 3 - Typical Instantaneous Forward Characteristics", vec![Forward]),
            ("Figure 1. Typical Forward Voltage", vec![Forward]),
            ("Fig. 6 - Diode Capacitance vs. Reverse Voltage", vec![Capacitance]),
            ("Figure 8: Capacitance Characteristics", vec![Capacitance]),
            ("Figure 11. \u{201C}On\u{201D} Voltages", vec![VbeOn, VbeSat, VceSat]),
            ("Figure 2. \u{201C}Saturation\u{201D} and \u{201C}On\u{201D} Voltages", vec![VbeOn, VbeSat, VceSat]),
            ("Figure 3. Base\u{2212}Emitter Saturation Voltage", vec![VbeSat]),
            ("Figure 4. Base\u{2212}Emitter On Voltage", vec![VbeOn]),
            ("Figure 2. Collector\u{2212}Emitter Saturation Voltage", vec![VceSat]),
            ("Fig 11. Maximum Effective Transient Thermal Impedance, Junction-to-Case", vec![ThermalImpedance]),
            ("Figure 5. Thermal Response", vec![ThermalImpedance]),
            ("Fig.6 Current Transfer Ratio vs. Forward Current", vec![Ctr]),
            ("Fig. 3 - Reverse Light Current vs. Irradiance", vec![LightCurrent]),
            // not the curves the form reads
            ("Fig. 4 - Reverse Light Current vs. Illuminance", vec![]),
            ("Fig.9 Relative Current Transfer Ratio vs. Ambient Temperature", vec![]),
            ("Fig.1 Forward Current vs. Ambient Temperature", vec![]),
            ("Fig. 1 - Typical Forward Voltage vs. Junction Temperature", vec![]),
            ("Figure 1. Normalized DC Current Gain", vec![]),
            ("Figure 6. Current\u{2212}Gain \u{2212} Bandwidth Product", vec![]),
            ("Figure 7. Small\u{2212}Signal Current Gain", vec![]),
            ("Figure 10. Collector Saturation Region", vec![]),
            ("Fig. 1 - Forward Current Derating Curve", vec![]),
            ("Fig. 2 - Maximum Non-Repetitive Peak Forward Surge Current", vec![]),
            ("Figure 7: Gate-Charge Characteristics", vec![]),
            ("Fig.12 Collector-emitter Saturation Voltage vs. Forward Current", vec![]),
        ] {
            assert_eq!(curves(caption), want, "{caption}");
        }
    }

    fn w(text: &str, x0: f32, y: f32) -> Word {
        Word::for_test(text, x0, y - 8.0, x0 + 5.0 * text.chars().count() as f32, y)
    }

    #[test]
    fn graphs_are_on_the_side_of_the_captions_the_maker_uses() {
        let caption = |page: usize, y: f32| Figure {
            page,
            rect: Rect { x0: 100.0, y0: y, x1: 200.0, y1: y + 8.0 },
            caption: String::new(),
            curves: vec![],
            area: Rect { x0: 40.0, y0: y - GRAPH_REACH, x1: 260.0, y1: y + 8.0 },
            other: Rect { x0: 40.0, y0: y, x1: 260.0, y1: y + 8.0 + GRAPH_REACH },
            numbers: (0, 6),
        };
        // Sharp: the captions over their graphs, the top one of each page with nothing over it
        let mut f = vec![caption(5, 100.0), caption(5, 400.0), caption(6, 80.0)];
        f[1].numbers = (9, 6);
        place_graphs(&mut f);
        assert!(f.iter().all(|f| f.area.y0 >= f.rect.y0), "{:?}", f.iter().map(|f| f.area).collect::<Vec<_>>());
        // onsemi, Vishay: under their graphs, so the top one has tick labels over it
        let mut f = vec![caption(5, 300.0), caption(5, 600.0)];
        f[0].numbers = (12, 6);
        place_graphs(&mut f);
        assert!(f.iter().all(|f| f.area.y1 <= f.rect.y1));
        // tick labels drawn as lines: above
        let mut f = vec![caption(5, 300.0)];
        f[0].numbers = (0, 0);
        place_graphs(&mut f);
        assert!(f[0].area.y1 <= f[0].rect.y1);
        // the numbers counted are the tick labels over the caption in its column, a run of labels read as one word too
        let words = vec![w("0.1 1 10 100", 100.0, 280.0), w("1000", 100.0, 320.0), w("5", 600.0, 280.0)];
        assert_eq!(sides(&words, Rect { x0: 100.0, y0: 300.0, x1: 200.0, y1: 308.0 }).2, (4, 1));
    }

    #[test]
    fn two_captions_side_by_side_and_one_on_two_lines() {
        // Fig.6's caption goes on on the next line; Fig.7 sits beside it; a sentence that names a figure is not one
        let words = vec![
            w("Fig.6", 50.0, 300.0),
            w("Current", 80.0, 300.0),
            w("Transfer", 120.0, 300.0),
            w("Ratio", 165.0, 300.0),
            w("vs.", 195.0, 300.0),
            w("Forward", 215.0, 300.0),
            w("Fig.7", 330.0, 300.0),
            w("DC", 360.0, 300.0),
            w("Current", 375.0, 300.0),
            w("Gain", 415.0, 300.0),
            w("Current", 120.0, 311.0),
            w("as", 50.0, 400.0),
            w("described", 62.0, 400.0),
            w("in", 110.0, 400.0),
            w("Figure", 122.0, 400.0),
            w("5", 155.0, 400.0),
            // a sentence that starts with a figure's name
            w("Figure", 50.0, 500.0),
            w("7-1", 82.0, 500.0),
        ];
        let long: Vec<Word> = "shows the typical application circuit for the part and the input and output capacitors".split(' ').enumerate().map(|(i, t)| w(t, 100.0 + 60.0 * i as f32, 500.0)).collect();
        let words: Vec<Word> = words.into_iter().chain(long).collect();
        let f = figures(2, &words);
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!(f[0].caption, "Fig.6 Current Transfer Ratio vs. Forward Current");
        assert_eq!(f[0].curves, vec![Curve::Ctr]);
        assert_eq!(f[1].caption, "Fig.7 DC Current Gain", "the line below is under Fig.6, not Fig.7");
        assert_eq!(f[1].curves, vec![Curve::DcGain]);
        assert_eq!(showing(&f, Curve::DcGain)[0].page, 2);
    }
}
