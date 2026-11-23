//! The pads of a recommended land pattern drawing, from the way its sizes are written: "14X (1.55)" is a size that 14 things have
//! (14 pads: their length or width), "12X (1.27)" one that 12 gaps have (the pitch between 14 pads in two rows), a bare "(5.4)" a
//! size once (the distance between the rows' centres). Texas Instruments draws every "EXAMPLE BOARD LAYOUT" this way. A QFN's
//! pads stand on four sides, so its pitch is had by pins − 4 gaps ("12X (0.5)" of 16 pads); a QFN's or SON's exposed pad is the
//! bare sizes inside the rows, with its pin number ("17" of 16 pins) written in it. Drawings that write their sizes without
//! counts, or draw them as lines, are not read.

use crate::draft::{PackageName, no_lead_sides, package_of};
use crate::text::{Word, normalize};

/// A land pattern read off a page: pads in one or two rows, or on four sides.
#[derive(Clone, Debug, PartialEq)]
pub struct LandPattern {
    pub page: usize,
    /// The package the page is about (from its title lines), if it says.
    pub family: Option<&'static str>,
    pub pins: u32,
    /// Pad length (the long side) and width, mm.
    pub pad_l: f64,
    pub pad_w: f64,
    /// Between neighbouring pads, and between the centres of the two rows (none for a single row), mm. On four sides, between
    /// the centres of the pads of opposite sides.
    pub pitch: Option<f64>,
    pub row: Option<f64>,
    /// The pads stand on four sides (QFN), not in two rows.
    pub quad: bool,
    /// The exposed pad in the middle: its size across the rows (x) and along them (y), mm, and its pin number.
    pub ep: Option<(f64, f64)>,
    pub ep_pin: Option<String>,
}

impl LandPattern {
    /// Pins in two rows of equal length (the form's "デュアル").
    pub fn dual(&self) -> bool {
        !self.quad && self.row.is_some() && self.pins.is_multiple_of(2)
    }

    /// Pins on four sides of equal length (the form's "クアッド").
    pub fn four_sided(&self) -> bool {
        self.quad && self.row.is_some() && self.pins.is_multiple_of(4)
    }

    /// Fits the package `p` (its family, and its pin count when it says one).
    pub fn fits(&self, p: &PackageName) -> bool {
        self.family == Some(p.family) && p.pins.is_none_or(|n| n == self.pins)
    }
}

/// "(1.55)" → 1.55; a radius "(R0.05)" or anything else is not a size.
fn in_brackets(t: &str) -> Option<f64> {
    let t = t.trim();
    let inner = t.strip_prefix('(')?.strip_suffix(')')?;
    inner.trim().parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 100.0)
}

/// "[1.55]" → 1.55: the millimetres written under the inches in TI's older drawings ("(.061)" over "[1.55]").
fn in_square(t: &str) -> Option<f64> {
    let inner = t.trim().strip_prefix('[')?.strip_suffix(']')?;
    inner.parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 100.0)
}

/// "14X" → 14.
fn count(t: &str) -> Option<u32> {
    let t = t.trim();
    let n = t.strip_suffix('X').or(t.strip_suffix('x'))?;
    n.parse().ok().filter(|n| (1..=400).contains(n))
}

/// A size written on the page: its value in mm and where it stands, and whether it is marked as a via's or as typical (a via's
/// spacing, "(0.58) TYP": not a pad's).
struct Size {
    mm: f64,
    x0: f32,
    base: f32,
    aside: bool,
    /// Followed by "HOLE", "DRILL" or "VIA": a hole's diameter, not a pad's size even when counted ("3X ( 0.85) HOLE").
    hole: bool,
}

/// Whether one of `marks` stands right after the word `w` (ending at `last`) on its line, or just under it.
fn marked(words: &[Word], w: &Word, last: &Word, marks: &[&str]) -> bool {
    words.iter().any(|m| {
        let t = normalize(&m.text);
        let t = t.trim_matches(|c: char| !c.is_alphanumeric());
        if !marks.contains(&t) {
            return false;
        }
        let beside = (m.baseline() - last.baseline()).abs() < 2.0 && m.rect.x0 >= last.rect.x1 - 1.0 && m.rect.x0 - last.rect.x1 < 10.0;
        let under = m.baseline() > w.baseline() + 3.0 && m.baseline() < w.baseline() + 13.0 && (m.rect.x0 - w.rect.x0).abs() < 18.0;
        beside || under
    })
}

/// The sizes of a page. With millimetres in square brackets on the page, those are the sizes (the bracketed numbers over them are
/// inches); else the bracketed ones, with a bracket split from its number ("(.061" ")", "(" "1.68)") joined.
fn sizes(words: &[Word]) -> Vec<Size> {
    let square: Vec<Size> = words.iter().filter_map(|w| in_square(&w.text).map(|mm| Size { mm, x0: w.rect.x0, base: w.baseline(), aside: false, hole: false })).collect();
    if !square.is_empty() {
        return square;
    }
    let mut out = vec![];
    for (i, w) in words.iter().enumerate() {
        let next = words.get(i + 1).filter(|n| (n.baseline() - w.baseline()).abs() < 2.0);
        let (joined, last) = match w.text.trim() {
            "(" => (next.filter(|n| n.text.trim().ends_with(')')).map(|n| format!("({}", n.text.trim())), next.unwrap_or(w)),
            t if t.starts_with('(') && !t.ends_with(')') => (next.filter(|n| n.text.trim() == ")").map(|_| format!("{t})")), next.unwrap_or(w)),
            t => (Some(t.to_string()), w),
        };
        if let Some(mm) = joined.as_deref().and_then(in_brackets) {
            let hole = marked(words, w, last, &["hole", "drill", "via"]);
            out.push(Size { mm, x0: w.rect.x0, base: w.baseline(), aside: hole || marked(words, w, last, &["typ"]), hole });
        }
    }
    out
}

/// The land pattern of one page: the sizes that the most things have are the pads (the bigger the length, the smaller the width),
/// the pitch is had by one gap fewer per row (two rows: pins − 2, four sides: pins − 4), the rows' distance is the biggest bare
/// size beyond the pad. An exposed pad is there when its pin number (pins + 1) is written; its sizes are the biggest bare ones
/// below the rows' distance.
pub fn land_pattern(page: usize, words: &[Word], page_height: f32) -> Option<LandPattern> {
    let sizes = sizes(words);
    // the size of a count: right after it on its line, or (millimetres under inches) just under that
    let of_count = |w: &Word| -> Option<usize> {
        let beside = |s: &Size| !s.hole && (s.base - w.baseline()).abs() < 2.0 && s.x0 > w.rect.x0 && s.x0 - w.rect.x1 < 8.0;
        let under = |s: &Size| !s.hole && s.base > w.baseline() + 3.0 && s.base < w.baseline() + 14.0 && s.x0 > w.rect.x0 - 2.0 && s.x0 < w.rect.x1 + 30.0;
        (0..sizes.len()).filter(|&k| beside(&sizes[k])).min_by(|a, b| sizes[*a].x0.total_cmp(&sizes[*b].x0)).or_else(|| (0..sizes.len()).filter(|&k| under(&sizes[k])).min_by(|a, b| sizes[*a].base.total_cmp(&sizes[*b].base)))
    };
    let mut counted: Vec<(u32, f64)> = vec![];
    let mut used = vec![false; sizes.len()];
    for w in words {
        if let Some(n) = count(&w.text)
            && let Some(k) = of_count(w)
        {
            counted.push((n, sizes[k].mm));
            used[k] = true;
        }
    }
    let bare: Vec<f64> = sizes.iter().zip(&used).filter(|(s, u)| !**u && !s.aside).map(|(s, _)| s.mm).collect();
    // the pads: the count had by two sizes, the biggest such count
    let pins = counted.iter().map(|c| c.0).filter(|n| counted.iter().filter(|c| c.0 == *n).count() >= 2).max()?;
    let pad_sizes: Vec<f64> = counted.iter().filter(|c| c.0 == pins).map(|c| c.1).collect();
    let pad_l = pad_sizes.iter().copied().fold(f64::MIN, f64::max);
    let pad_w = pad_sizes.iter().copied().fold(f64::MAX, f64::min);
    if pad_l <= pad_w {
        return None;
    }
    // the package: named in the page's title lines (the top 15 %)
    let family = words.iter().filter(|w| w.rect.y1 < page_height * 0.15).find_map(|w| package_of(&normalize(&w.text)).map(|p| p.0));
    let row = bare.iter().copied().filter(|v| *v > pad_l).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    // four sides: the title says a QFN, or the pitch is had by pins − 4 gaps and not by pins − 2
    let has_gaps = |g: u32| counted.iter().any(|c| c.0 == g && c.1 < pad_l * 2.0);
    let quad = row.is_some() && pins.is_multiple_of(4) && pins >= 8 && (family.and_then(no_lead_sides) == Some(4) || (has_gaps(pins - 4) && !has_gaps(pins - 2)));
    let gaps = match (quad, row.is_some() && pins.is_multiple_of(2)) {
        (true, _) => pins - 4,
        (false, true) => pins - 2,
        (false, false) => pins - 1,
    };
    let pitch = counted.iter().find(|c| c.0 == gaps && c.1 < pad_l * 2.0).map(|c| c.1);
    // the exposed pad: its number written inside, and the biggest bare sizes below the rows' distance (a second one less than
    // half the first is something else: the pad is then square)
    let ep_pin = (pins + 1).to_string();
    let ep = match row {
        Some(r) if words.iter().any(|w| w.text.trim() == ep_pin) => {
            let mut inside: Vec<f64> = bare.iter().copied().filter(|v| *v < r - 1e-9 && *v > pad_w).collect();
            inside.sort_by(|a, b| b.total_cmp(a));
            match inside.as_slice() {
                [] => None,
                [a] => Some((*a, *a)),
                [a, b, ..] if *b < a * 0.5 => Some((*a, *a)),
                // across the rows the pad must fit between the rows' pads; on four sides as written
                [a, b, ..] if !quad && *a > r - pad_l => Some((*b, *a)),
                [a, b, ..] => Some((*a, *b)),
            }
        }
        _ => None,
    };
    let ep_pin = ep.map(|_| ep_pin);
    Some(LandPattern { page, family, pins, pad_l, pad_w, pitch, row, quad, ep, ep_pin })
}

/// A size of an LTC (Analog Devices) drawing: "3.55 ±0.05", "0.50" over "BSC", "3.20 – 3.45", "5.10" over "(.201)" over "MIN",
/// with what is written by it.
struct LtcSize {
    mm: f64,
    bsc: bool,
    min: bool,
    /// Marked "(2 SIDES)" or "(4 SIDES)": an exposed pad's size.
    sides: bool,
    /// A range "3.20 – 3.45" (its middle).
    range: bool,
}

/// The recommended solder pad layout of an LTC (Analog Devices) package page (候補 Q3): the drawing just above its caption
/// "RECOMMENDED SOLDER PAD PITCH AND DIMENSIONS" / "RECOMMENDED SOLDER PAD LAYOUT", every size "value ±tolerance" (TI's are in
/// brackets with a count, "14X (1.55)"). The pitch is the BSC size, the pad's width the smallest size below it; the pad's
/// length L is the size whose double is the difference of two others (the outer and the inner spans: 3.55 − 2.15 = 2 × 0.70,
/// LT3045's DFN-10), or what the outer span (MIN) less the inner one (a range) halves to (5.10 − 3.32 ≈ 2 × 0.889, its
/// MSOP-12); the rows' centres are between. The sizes marked "(2 SIDES)" are the exposed pad's, the smaller across the rows.
/// The pins and the package from the page's title ("10-Lead Plastic DFN").
pub fn ltc_land(page: usize, words: &[Word], page_height: f32) -> Option<LandPattern> {
    let caption = words.iter().find(|w| normalize(&w.text) == "recommended" && words.iter().any(|o| (o.baseline() - w.baseline()).abs() < 2.0 && o.rect.x0 > w.rect.x1 && normalize(&o.text) == "solder"))?;
    let cap_end = words.iter().filter(|o| (o.baseline() - caption.baseline()).abs() < 2.0 && o.rect.x0 >= caption.rect.x0).map(|o| o.rect.x1).fold(caption.rect.x1, f32::max);
    // the drawing: above the caption, about as wide
    let inside: Vec<&Word> = words
        .iter()
        .filter(|w| w.baseline() < caption.baseline() - 2.0 && w.baseline() > caption.baseline() - 230.0 && w.rect.x0 >= caption.rect.x0 - 60.0 && w.rect.x1 <= cap_end + 60.0)
        .collect();
    let num = |t: &str| t.trim().parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 50.0);
    let near = |w: &Word, below: f32, dx: f32, f: &dyn Fn(&str) -> bool| inside.iter().any(|o| o.baseline() > w.baseline() + 2.0 && o.baseline() < w.baseline() + below && (o.rect.x0 - w.rect.x0).abs() < dx && f(&normalize(&o.text)));
    let beside = |w: &Word, f: &dyn Fn(&str) -> bool| inside.iter().find(|o| (o.baseline() - w.baseline()).abs() < 2.0 && o.rect.x0 >= w.rect.x1 - 0.5 && o.rect.x0 - w.rect.x1 < 6.0 && f(&o.text));
    let mut sizes: Vec<LtcSize> = vec![];
    for w in &inside {
        let Some(v) = num(&w.text) else { continue };
        let tol = beside(w, &|t| t.trim_start().starts_with('±'));
        let dash = beside(w, &|t| matches!(t.trim(), "–" | "-" | "−"));
        let range_end = dash.and_then(|d| inside.iter().find(|o| (o.baseline() - d.baseline()).abs() < 2.0 && o.rect.x0 > d.rect.x1 && o.rect.x0 - d.rect.x1 < 6.0).and_then(|o| num(&o.text)));
        // (under it, past the inches: "0.65" over "(.0256)" over "BSC")
        let bsc = beside(w, &|t| normalize(t) == "bsc").is_some() || near(w, 20.0, 15.0, &|t| t == "bsc");
        let min = near(w, 24.0, 15.0, &|t| t == "min");
        let sides = near(w, 12.0, 25.0, &|t| t.starts_with("(2") || t.starts_with("(4"));
        match (tol, range_end) {
            (_, Some(hi)) => sizes.push(LtcSize { mm: (v + hi) / 2.0, bsc: false, min: false, sides, range: true }),
            (Some(_), None) => sizes.push(LtcSize { mm: v, bsc, min, sides, range: false }),
            (None, None) if bsc || min => sizes.push(LtcSize { mm: v, bsc, min, sides, range: false }),
            _ => {}
        }
    }
    let pitch = sizes.iter().find(|s| s.bsc).map(|s| s.mm)?;
    let plain: Vec<f64> = sizes.iter().filter(|s| !s.bsc && !s.sides && !s.min && !s.range).map(|s| s.mm).collect();
    let pad_w = plain.iter().copied().filter(|v| *v < pitch).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v))))?;
    // the pad's length and the rows' centres: an outer and an inner span 2 L apart, or the MIN outer span and a range inside it
    let mut found: Option<(f64, f64)> = None;
    'pairs: for &l in plain.iter().filter(|l| **l > pad_w && **l < 3.0) {
        for &a in plain.iter() {
            for &b in plain.iter().filter(|b| **b < a) {
                if (a - b - 2.0 * l).abs() < 0.03 {
                    found = Some((l, (a + b) / 2.0));
                    break 'pairs;
                }
            }
        }
    }
    if found.is_none()
        && let (Some(outer), Some(inner)) = (sizes.iter().find(|s| s.min).map(|s| s.mm), sizes.iter().find(|s| s.range).map(|s| s.mm))
    {
        found = plain.iter().copied().filter(|l| *l > pad_w && *l < 3.0).map(|l| (l, (outer - inner - 2.0 * l).abs())).filter(|c| c.1 < 0.15).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(l, _)| (l, outer - l));
    }
    let (pad_l, row) = found?;
    // the package and its pins from the title ("10-Lead Plastic DFN (3mm × 3mm)", "12-Lead Plastic MSOP")
    let title: Vec<&Word> = words.iter().filter(|w| w.rect.y1 < page_height * 0.2).collect();
    let pins = title.iter().find_map(|w| w.text.trim().strip_suffix("-Lead").or_else(|| w.text.trim().strip_suffix("-lead")).and_then(|n| n.parse::<u32>().ok()))?;
    let family = title.iter().find_map(|w| package_of(&normalize(&w.text)).map(|p| p.0));
    let mut ep: Vec<f64> = sizes.iter().filter(|s| s.sides).map(|s| s.mm).collect();
    ep.sort_by(f64::total_cmp);
    ep.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // no size marked "(2 SIDES)" on a package the title says has an exposed pad (LT3045's MSOP-12 "Exposed Die Pad"): the plain
    // sizes over the pad's length that fit between the rows' pads
    if ep.is_empty() && title.iter().any(|w| normalize(&w.text) == "exposed") {
        ep = plain.iter().copied().filter(|v| *v > pad_l + 0.1 && *v < row - pad_l).collect();
        ep.sort_by(f64::total_cmp);
        ep.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    }
    let ep = match ep.as_slice() {
        [a, b] => Some((*a, *b)),
        [a] => Some((*a, *a)),
        _ => None,
    };
    let quad = family.and_then(no_lead_sides) == Some(4);
    Some(LandPattern { page, family, pins, pad_l, pad_w, pitch: Some(pitch), row: Some(row), quad, ep, ep_pin: ep.map(|_| (pins + 1).to_string()) })
}

/// "(2×)" → 2: how many of a size there are, as Nexperia writes it beside or under the size.
fn times(t: &str) -> Option<u32> {
    let inner = t.trim().strip_prefix('(')?.strip_suffix(')')?;
    let n = inner.strip_suffix(['×', 'x', 'X'])?;
    n.trim().parse().ok().filter(|n| (1..=400).contains(n))
}

/// Nexperia's "Reflow soldering footprint" of a package with two pads (候補 Q3; SOD323, SOD882): the drawing above its caption.
/// The sizes with "(2×)" are the two pads': the solder paste's, the solder land's and (in the newer drawings) the solder
/// resist's, each one a little bigger than the one before, so the land's is the second smallest of two or of three. Which way
/// a size runs is told by where the sizes stand: those under one another are of dimension lines that run across (the sizes
/// along the line through the two pads), those side by side are of lines that run up and down. The distance between the pads'
/// centres is the size drawn in a frame (`boxed`: the text does not say, so the caller looks at the picture); without one
/// nothing is read. `pad_l` is the land's size along the line through the pads, `pad_w` across it.
pub fn nexperia_land(page: usize, words: &[Word], boxed: &dyn Fn(&Word) -> bool) -> Option<LandPattern> {
    let on_line = |a: &Word, b: &Word| (a.baseline() - b.baseline()).abs() < 2.0;
    let caption = words.iter().find(|w| normalize(&w.text) == "reflow" && ["soldering", "footprint"].iter().all(|k| words.iter().any(|o| on_line(o, w) && o.rect.x0 > w.rect.x1 && normalize(&o.text) == *k)))?;
    // the drawing: above its caption, below the heading ("12. Soldering") or the caption of the drawing before it
    let top = words
        .iter()
        .filter(|o| o.baseline() < caption.baseline() - 2.0 && matches!(normalize(&o.text).as_str(), "soldering" | "footprint"))
        .map(|o| o.baseline())
        .fold(caption.baseline() - 320.0, f32::max);
    let inside: Vec<&Word> = words.iter().filter(|w| w.baseline() > top + 2.0 && w.baseline() < caption.baseline() - 2.0).collect();
    let nums: Vec<(&Word, f64)> = inside.iter().filter_map(|w| w.text.trim().parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 50.0).map(|v| (*w, v))).collect();
    let mid = |w: &Word| (w.rect.x0 + w.rect.x1) / 2.0;
    // each "(n×)" belongs to the size it stands right after, else to the nearest one it stands under
    let mut count: Vec<Option<u32>> = vec![None; nums.len()];
    for m in inside.iter().filter(|w| times(&w.text).is_some()) {
        let beside = (0..nums.len()).filter(|&k| on_line(nums[k].0, m) && m.rect.x0 - nums[k].0.rect.x1 > -1.0 && m.rect.x0 - nums[k].0.rect.x1 < 8.0).min_by(|a, b| nums[*b].0.rect.x1.total_cmp(&nums[*a].0.rect.x1));
        let under = || (0..nums.len()).filter(|&k| m.baseline() > nums[k].0.baseline() + 3.0 && m.baseline() < nums[k].0.baseline() + 14.0 && (mid(m) - mid(nums[k].0)).abs() < 10.0).min_by(|a, b| nums[*b].0.baseline().total_cmp(&nums[*a].0.baseline()));
        if let Some(k) = beside.or_else(under) {
            count[k] = times(&m.text);
        }
    }
    // two pads only: three or more stand in ways this does not tell apart
    let pads: Vec<usize> = (0..nums.len()).filter(|&k| count[k] == Some(2)).collect();
    if pads.is_empty() || count.iter().flatten().filter(|n| **n != 2).count() > pads.len() {
        return None;
    }
    // the biggest set of sizes under one another, and the biggest set side by side
    let biggest = |near: &dyn Fn(usize, usize) -> bool| -> Vec<usize> { pads.iter().map(|&a| pads.iter().copied().filter(|&b| near(a, b)).collect::<Vec<_>>()).max_by_key(Vec::len).unwrap_or_default() };
    let column = biggest(&|a, b| (mid(nums[a].0) - mid(nums[b].0)).abs() < 6.0);
    let line = biggest(&|a, b| (nums[a].0.baseline() - nums[b].0.baseline()).abs() < 3.0);
    if column.len() != line.len() || !(2..=3).contains(&column.len()) || column.iter().any(|k| line.contains(k)) {
        return None;
    }
    let land = |set: &[usize]| {
        let mut v: Vec<f64> = set.iter().map(|&k| nums[k].1).collect();
        v.sort_by(f64::total_cmp);
        v[1]
    };
    let (along, across) = (land(&column), land(&line));
    let framed: Vec<f64> = (0..nums.len()).filter(|&k| count[k].is_none() && boxed(nums[k].0)).map(|k| nums[k].1).collect();
    let [centres] = framed.as_slice() else { return None };
    if *centres <= along {
        return None;
    }
    let family = words.iter().filter(|w| on_line(w, caption) && w.rect.x0 > caption.rect.x1).find_map(|w| package_of(&normalize(&w.text)).map(|p| p.0));
    Some(LandPattern { page, family, pins: 2, pad_l: along, pad_w: across, pitch: None, row: Some(*centres), quad: false, ep: None, ep_pin: None })
}

/// Whether the word at `r` (page points) stands in a drawn frame in the picture `img` of its page (`k` pixels per point): a
/// dark line just over and just under it that covers its width, and one just left and just right that covers its height. A
/// dimension line runs into a size from the side, at half its height, so it makes no frame.
fn framed(img: &crate::PageImage, k: f32, r: &crate::Rect) -> bool {
    let dark = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= img.width as i64 || y >= img.height as i64 {
            return false;
        }
        let i = (y as usize * img.width + x as usize) * 4;
        img.rgba[i] as u32 + img.rgba[i + 1] as u32 + img.rgba[i + 2] as u32 <= 384
    };
    let px = |v: f32| (v * k).round() as i64;
    // the middle of the word: a frame a little smaller than the font's box still covers it
    let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
    let (xa, xb, ya, yb) = (px(r.x0 + 0.1 * w), px(r.x1 - 0.1 * w), px(r.y0 + 0.25 * h), px(r.y1 - 0.25 * h));
    let covered = |n: usize, of: i64| n as f32 >= 0.95 * (of + 1) as f32;
    let across = |from: i64, to: i64| (from..=to).any(|y| covered((xa..=xb).filter(|&x| dark(x, y)).count(), xb - xa));
    let down = |from: i64, to: i64| (from..=to).any(|x| covered((ya..=yb).filter(|&y| dark(x, y)).count(), yb - ya));
    across(px(r.y0 - 6.0), px(r.y0 + 1.5)) && across(px(r.y1 - 1.5), px(r.y1 + 6.0)) && down(px(r.x0 - 8.0), px(r.x0) - 1) && down(px(r.x1) + 1, px(r.x1 + 8.0))
}

/// One row of a chip part's dimension table (候補 Q3): its size code as printed ("RT0603", "0603"), the body's length L and width
/// W, and the terminal's length on the underside, each (min, max) in mm.
#[derive(Clone, Debug, PartialEq)]
pub struct ChipSize {
    pub page: usize,
    pub code: String,
    pub len: (f64, f64),
    pub width: (f64, f64),
    /// The terminal under the body (Yageo's I2, Murata's e, TDK's B, ROHM's b …), or half of L less the gap between the two
    /// (Murata's g) when only that is given.
    pub term: Option<(f64, f64)>,
    /// The column it was read from ("I2", "e", "(L − g) / 2").
    pub term_from: String,
}

impl ChipSize {
    /// The inch size code in it ("0603" of "RT0603", "0402(1005)"), when it has one of the usual ones.
    pub fn inch_code(&self) -> Option<&'static str> {
        const CODES: [&str; 11] = ["01005", "0201", "0402", "0603", "0805", "1206", "1210", "1812", "2010", "2512", "2220"];
        CODES.into_iter().find(|c| self.code.contains(c))
    }
}

/// A size with its tolerance, from one word or the words after it: "1.60±0.10", "1.60" "±0.10", "1.6" "±" "0.1", or a bare value.
/// The value and how many words it took.
fn toleranced(words: &[&Word], k: usize) -> Option<((f64, f64), usize)> {
    let t = |i: usize| words.get(i).map(|w| normalize(&w.text).replace('+', "").replace(['−', '–'], "-"));
    let first = t(k)?;
    let (v, rest) = match first.split_once('±') {
        Some((v, r)) => (v.parse::<f64>().ok()?, Some(r.to_string())),
        None => (first.parse::<f64>().ok()?, None),
    };
    let (tol, used) = match rest {
        Some(r) if !r.is_empty() => (r.parse::<f64>().ok()?, 1),
        Some(_) => (t(k + 1)?.parse::<f64>().ok()?, 2),
        None => match t(k + 1) {
            // a minimum only (TDK's "0.10 min."): no top
            Some(n) if matches!(n.as_str(), "min." | "min") => return (v > 0.0 && v < 20.0).then_some(((v, f64::INFINITY), 2)),
            Some(n) if n.starts_with('±') && n.len() > '±'.len_utf8() => (n.trim_start_matches('±').parse::<f64>().ok()?, 2),
            Some(n) if n == "±" => (t(k + 2)?.parse::<f64>().ok()?, 3),
            _ => (0.0, 1),
        },
    };
    (v > 0.0 && v < 20.0 && tol >= 0.0 && tol < v).then_some(((v - tol, v + tol), used))
}

/// The dimension table of a chip part's page: a heading line with the columns "L" and "W" (and others: "H", "T", "I1", "I2",
/// "a", "b", "e", "g" …), then a row per size starting with its code, each cell a size with its tolerance. The cells are put in
/// the column whose heading's centre is nearest.
pub fn chip_sizes(page: usize, words: &[Word]) -> Vec<ChipSize> {
    let lines = crate::figures::lines(words);
    let centre = |w: &Word| (w.rect.x0 + w.rect.x1) / 2.0;
    let head_name = |w: &Word| normalize(&w.text).trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        let names: Vec<(String, f32)> = l.iter().map(|w| (head_name(w), centre(w))).filter(|(n, _)| n.len() <= 2 && !n.is_empty() && n != "mm").collect();
        let col = |n: &str| names.iter().find(|h| h.0 == n).map(|h| h.1);
        let (Some(_), Some(_)) = (col("l"), col("w")) else { continue };
        // the rows under it: a code and its sizes, or a code alone with the sizes on the next line (Yageo's RT0201)
        let mut pending: Option<String> = None;
        for row in lines[i + 1..].iter().take(30) {
            let first = normalize(&row[0].text);
            let (code, start) = if toleranced(row, 0).is_some() {
                match pending.take() {
                    Some(c) => (c, 0),
                    None => continue,
                }
            } else if first.chars().any(|c| c.is_ascii_digit()) && !first.contains('.') {
                (row[0].text.trim().to_string(), 1)
            } else {
                continue;
            };
            let mut cells: Vec<(String, (f64, f64))> = vec![];
            let mut k = start;
            while k < row.len() {
                if let Some((v, used)) = toleranced(row, k) {
                    let x = centre(row[k]);
                    if let Some(h) = names.iter().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).filter(|h| (h.1 - x).abs() < 30.0) {
                        cells.push((h.0.clone(), v));
                    }
                    k += used;
                } else {
                    k += 1;
                }
            }
            let get = |n: &str| cells.iter().find(|c| c.0 == n).map(|c| c.1);
            let (Some(len), Some(width)) = (get("l"), get("w")) else {
                if start == 1 {
                    pending = Some(code);
                }
                continue;
            };
            // the terminal underneath: the bottom electrode first. A minimum only (TDK's "B 0.10 min."): its most is what the gap's
            // minimum leaves ("G 0.30 min.": (L − G) / 2), else a quarter of L (said so)
            let gap = get("g");
            let (term, term_from) = match ["i2", "e", "b", "a", "i1", "c"].iter().find_map(|n| get(n).map(|v| (v, n.to_uppercase()))) {
                Some((v, n)) if v.1.is_infinite() => match gap.map(|g| (len.1 - g.0) / 2.0).filter(|m| *m > v.0) {
                    Some(most) => (Some((v.0, most)), format!("{n} (最小) と (L − G) / 2")),
                    None => (Some((v.0, (len.1 / 4.0).max(v.0))), format!("{n} (最小) と L / 4 (推定)")),
                },
                Some((v, n)) if v.1 < len.0 / 2.0 => (Some(v), n),
                _ => match gap {
                    Some(g) if g.1 < len.0 => (Some(((len.0 - g.1) / 2.0, (len.1 - g.0) / 2.0)), "(L − g) / 2".into()),
                    _ => (None, String::new()),
                },
            };
            out.push(ChipSize { page, code, len, width, term, term_from });
        }
    }
    out
}

impl crate::Datasheet {
    /// The rows of the chip dimension tables of the whole sheet (`chip_sizes`).
    pub fn chip_sizes(&self) -> Vec<ChipSize> {
        (0..self.page_count()).flat_map(|p| chip_sizes(p, self.words(p))).collect()
    }

    /// The land patterns of every page with a land pattern heading that has one.
    pub fn land_patterns(&self) -> Vec<LandPattern> {
        let q = crate::Heading::LandPattern.queries();
        (0..self.page_count())
            .filter(|&p| self.search_page_any(p, &q).iter().any(|h| !h.in_contents))
            .filter_map(|p| {
                let h = self.page_size(p).map_or(842.0, |s| s.1);
                // the page's picture, drawn only when a size is asked whether it stands in a frame
                let img: std::cell::OnceCell<Option<crate::PageImage>> = std::cell::OnceCell::new();
                let boxed = |w: &Word| {
                    let Some(img) = img.get_or_init(|| self.render(p, 3.0).ok()) else { return false };
                    framed(img, img.width as f32 / self.page_size(p).map_or(1.0, |s| s.0), &w.rect)
                };
                // TI's bracketed sizes with counts, else LTC's sizes with tolerances, else Nexperia's soldering footprint
                land_pattern(p, self.words(p), h).or_else(|| ltc_land(p, self.words(p), h)).or_else(|| nexperia_land(p, self.words(p), &boxed))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yageo's RT table: "TYPE L (mm) W (mm) H (mm) I1 I2", a size and its tolerance as two words; a row split over two lines
    /// (the code alone with a letter of the figure beside it, the sizes on the next) is read; the bottom terminal is I2.
    #[test]
    fn a_chip_dimension_table() {
        let at = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let mut words = vec![at("TYPE", 49.0, 614.0), at("L", 96.0, 614.0), at("(mm)", 103.0, 614.0), at("W", 133.0, 614.0), at("(mm)", 146.0, 614.0), at("H", 179.0, 614.0), at("I1", 224.0, 614.0), at("I2", 267.0, 614.0)];
        for (y, code, v) in [(640.0, "RT0201", ["", "", "", "", "", "", "", "", "", ""]), (645.0, "", ["0.60", "±0.03", "0.30", "±0.03", "0.23", "±0.03", "0.10", "±0.05", "0.15", "±0.05"]), (658.0, "RT0402", ["1.00", "±0.10", "0.50", "±0.05", "0.30", "±0.05", "0.20", "±0.10", "0.25", "±0.10"]), (675.0, "RT0603", ["1.60", "±0.10", "0.80", "±0.10", "0.45", "±0.10", "0.25", "±0.15", "0.25", "±0.15"])] {
            if !code.is_empty() {
                words.push(at(code, 49.0, y));
            }
            if code == "RT0201" {
                words.push(at("W", 318.0, y));
            }
            for (k, t) in v.iter().enumerate().filter(|(_, t)| !t.is_empty()) {
                words.push(at(t, [88.0, 104.0, 131.0, 147.0, 174.0, 190.0, 217.0, 233.0, 260.0, 276.0][k], y));
            }
        }
        let s = chip_sizes(3, &words);
        assert_eq!(s.len(), 3, "{s:?}");
        assert_eq!((s[0].code.as_str(), s[0].inch_code()), ("RT0201", Some("0201")));
        let r = &s[2];
        assert_eq!((r.inch_code(), r.term_from.as_str()), (Some("0603"), "I2"));
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        assert!(near(r.len, (1.5, 1.7)) && near(r.width, (0.7, 0.9)) && near(r.term.unwrap(), (0.1, 0.4)), "{r:?}");
        // a size in one word, and Murata's gap g instead of the terminal
        let words = vec![at("L", 100.0, 100.0), at("W", 150.0, 100.0), at("g", 200.0, 100.0), at("GRM18", 40.0, 115.0), at("1.6±0.1", 95.0, 115.0), at("0.8±0.1", 145.0, 115.0), at("0.5", 200.0, 115.0)];
        let s = chip_sizes(0, &words);
        assert_eq!(s[0].term_from, "(L − g) / 2");
        assert!((s[0].term.unwrap().0 - 0.5).abs() < 1e-9, "{:?}", s[0]);
        // TDK's minimums: B 0.10 min., G 0.30 min. of a 1.00 ±0.05 long chip: the terminal 0.10 to (1.05 − 0.30) / 2
        let words = vec![at("Type", 41.0, 516.0), at("L", 82.0, 516.0), at("W", 150.0, 516.0), at("B", 286.0, 516.0), at("G", 329.0, 516.0), at("CGA2", 41.0, 527.0), at("1.00±0.05", 82.0, 527.0), at("0.50±0.05", 150.0, 527.0), at("0.10", 286.0, 527.0), at("min.", 303.0, 527.0), at("0.30", 329.0, 527.0), at("min.", 345.0, 527.0)];
        let s = chip_sizes(2, &words);
        assert!((s[0].term.unwrap().0 - 0.1).abs() < 1e-9 && (s[0].term.unwrap().1 - 0.375).abs() < 1e-9, "{:?}", s[0]);
    }

    fn w(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0)
    }

    /// LTC's (LT3045) DFN-10 recommended solder pads: sizes with tolerances, the outer and inner spans 2 × 0.70 apart, the pitch
    /// over "BSC", the exposed pad marked "(2 SIDES)"; the bottom view beside it below the caption is not read.
    #[test]
    fn an_ltc_recommended_solder_pad_layout() {
        let words = vec![
            w("10-Lead", 229.0, 120.0), w("Plastic", 265.0, 120.0), w("DFN", 296.0, 120.0),
            w("0.70", 252.0, 187.0), w("±0.05", 274.0, 187.0),
            w("3.55", 85.0, 215.0), w("±0.05", 107.0, 215.0), w("1.65", 137.0, 215.0), w("±0.05", 159.0, 215.0),
            w("2.15", 104.0, 224.0), w("±0.05", 126.0, 224.0), w("(2", 139.0, 224.0), w("SIDES)", 146.0, 224.0),
            w("0.25", 137.0, 273.0), w("±0.05", 159.0, 273.0),
            w("0.50", 224.0, 281.0), w("BSC", 224.0, 289.0),
            w("2.38", 194.0, 299.0), w("±0.05", 216.0, 299.0), w("(2", 196.0, 307.0), w("SIDES)", 203.0, 307.0),
            w("RECOMMENDED", 130.0, 322.0), w("SOLDER", 190.0, 322.0), w("PAD", 225.0, 322.0), w("PITCH", 245.0, 322.0),
            w("1.65", 294.0, 384.0), w("±0.10", 316.0, 384.0), w("(2", 295.0, 392.0), w("SIDES)", 303.0, 392.0),
        ];
        let l = ltc_land(28, &words, 792.0).expect("the layout");
        assert_eq!((l.family, l.pins, l.pad_l, l.pad_w, l.pitch), (Some("dfn"), 10, 0.70, 0.25, Some(0.50)));
        assert!((l.row.unwrap() - 2.85).abs() < 1e-9, "{l:?}");
        assert_eq!((l.ep, l.ep_pin.as_deref()), (Some((1.65, 2.38)), Some("11")));
        // TI's bracketed sizes are not LTC's
        assert!(ltc_land(0, &[w("14X", 60.0, 100.0), w("(1.55)", 80.0, 100.0)], 792.0).is_none());
    }

    /// Nexperia's SOD882 (PESD5V0V1BL-Q): paste, land and resist each "(2×)" — the land is the middle one (0.4 along, 0.7
    /// across); the pads' centres are the framed 0.7, not the occupied area's 1.3 or 0.9; "R0.05 (8×)" is no size of a pad.
    #[test]
    fn a_nexperia_sod882() {
        let words = vec![
            w("13.", 40.0, 97.0), w("Soldering", 60.0, 97.0),
            w("1.3", 148.0, 125.0), w("0.7", 148.0, 141.0), w("R0.05", 228.0, 141.0), w("(8×)", 256.0, 141.0),
            w("solder", 357.0, 168.0), w("lands", 390.0, 168.0),
            w("0.6", 221.0, 193.0), w("0.7", 241.0, 193.0), w("0.8", 260.0, 193.0), w("0.9", 74.0, 197.0),
            w("(2×)", 219.0, 201.0), w("(2×)", 239.0, 201.0), w("(2×)", 258.0, 201.0),
            w("0.3", 116.0, 249.0), w("(2×)", 114.0, 258.0), w("0.4", 116.0, 268.0), w("(2×)", 114.0, 277.0), w("0.5", 116.0, 288.0), w("(2×)", 114.0, 296.0),
            w("Fig.", 40.0, 313.0), w("11.", 60.0, 313.0), w("Reflow", 80.0, 313.0), w("soldering", 115.0, 313.0), w("footprint", 165.0, 313.0), w("for", 215.0, 313.0), w("DFN1006-2", 232.0, 313.0), w("(SOD882)", 285.0, 313.0),
        ];
        let frame = |f: &Word| f.text == "0.7" && f.rect.y0 == 141.0;
        let l = nexperia_land(8, &words, &frame).expect("the footprint");
        assert_eq!(l, LandPattern { page: 8, family: Some("sod-882"), pins: 2, pad_l: 0.4, pad_w: 0.7, pitch: None, row: Some(0.7), quad: false, ep: None, ep_pin: None });
        assert!(l.dual());
        // without a framed size the centres are not known: nothing is guessed
        assert!(nexperia_land(8, &words, &|_| false).is_none());
    }

    /// Nexperia's SOD323 (PESD5V0U1BA): only paste and land are sized, the count beside the size in one place and under it in
    /// the other — the land is the bigger (0.6 × 0.6); the wave soldering footprint under it is another drawing.
    #[test]
    fn a_nexperia_sod323() {
        let words = vec![
            w("12.", 40.0, 97.0), w("Soldering", 60.0, 97.0),
            w("3.05", 160.0, 125.0), w("2.1", 162.0, 141.0),
            w("1.65", 65.0, 190.0), w("0.95", 84.0, 190.0), w("0.5", 238.0, 190.0), w("(2×)", 254.0, 190.0), w("0.6", 270.0, 190.0), w("(2×)", 286.0, 190.0),
            w("2.2", 161.0, 239.0), w("0.5", 117.0, 251.0), w("(2×)", 115.0, 259.0), w("0.6", 117.0, 268.0), w("(2×)", 115.0, 276.0),
            w("Fig.", 40.0, 294.0), w("7.", 60.0, 294.0), w("Reflow", 80.0, 294.0), w("soldering", 115.0, 294.0), w("footprint", 165.0, 294.0), w("for", 215.0, 294.0), w("SOD323", 232.0, 294.0),
            w("5", 160.0, 310.0), w("2.9", 160.0, 326.0), w("1.5", 150.0, 342.0), w("(2×)", 166.0, 342.0),
            w("Fig.", 40.0, 492.0), w("8.", 60.0, 492.0), w("Wave", 80.0, 492.0), w("soldering", 115.0, 492.0), w("footprint", 165.0, 492.0), w("for", 215.0, 492.0), w("SOD323", 232.0, 492.0),
        ];
        let l = nexperia_land(7, &words, &|f: &Word| f.text == "2.2").expect("the footprint");
        assert_eq!((l.family, l.pins, l.pad_l, l.pad_w, l.pitch, l.row), (Some("sod-323"), 2, 0.6, 0.6, None, Some(2.2)));
    }

    /// A size stands in a frame when lines run over, under, left and right of it; a dimension line running into it does not.
    #[test]
    fn a_framed_size_in_the_picture() {
        let (wd, ht) = (60usize, 40usize);
        let mut img = crate::PageImage { width: wd, height: ht, rgba: vec![255; wd * ht * 4] };
        fn ink(img: &mut crate::PageImage, x: usize, y: usize) {
            let i = (y * img.width + x) * 4;
            img.rgba[i..i + 3].fill(0);
        }
        // a dimension line into the word from both sides, at half its height
        (0..60).filter(|x| !(20..40).contains(x)).for_each(|x| ink(&mut img, x, 20));
        let r = crate::Rect { x0: 22.0, y0: 15.0, x1: 38.0, y1: 25.0 };
        assert!(!framed(&img, 1.0, &r));
        // the frame
        (17..=43).for_each(|x| {
            ink(&mut img, x, 12);
            ink(&mut img, x, 28);
        });
        (12..=28).for_each(|y| {
            ink(&mut img, 17, y);
            ink(&mut img, 43, y);
        });
        assert!(framed(&img, 1.0, &r));
    }

    /// The SOIC-14 land pattern as TI writes it.
    #[test]
    fn a_ti_soic14() {
        let words = vec![
            w("D0014A", 58.0, 62.0), w("SOIC", 392.0, 62.0),
            w("14X", 184.0, 133.0), w("(1.55)", 200.0, 133.0), w("SYMM", 303.0, 133.0),
            w("14X", 178.0, 184.0), w("(0.6)", 194.0, 184.0),
            w("12X", 185.0, 235.0), w("(1.27)", 201.0, 235.0),
            w("(R0.05)", 182.0, 353.0), w("(5.4)", 306.0, 368.0), w("SCALE:8X", 297.0, 400.0),
        ];
        let l = land_pattern(22, &words, 792.0).unwrap();
        assert_eq!(l, LandPattern { page: 22, family: Some("soic"), pins: 14, pad_l: 1.55, pad_w: 0.6, pitch: Some(1.27), row: Some(5.4), quad: false, ep: None, ep_pin: None });
        assert!(l.dual());
        assert!(l.fits(&PackageName { name: "SOIC-14".into(), family: "soic", pins: Some(14), count: 1, page: 0 }));
        assert!(!l.fits(&PackageName { name: "SOIC-8".into(), family: "soic", pins: Some(8), count: 1, page: 0 }));
    }

    /// TI's older drawings: inches in brackets (the bracket sometimes apart from its number), millimetres in square brackets under them.
    #[test]
    fn inches_over_millimetres() {
        let words = vec![
            w("SOIC", 392.0, 62.0),
            w("8X", 179.0, 143.0), w("(.061", 191.0, 143.0), w(")", 214.0, 143.0), w("[1.55]", 194.0, 151.6),
            w("8X", 169.0, 207.0), w("(.024)", 181.0, 207.0), w("[0.6]", 184.0, 215.8),
            w("6X", 153.0, 284.5), w("(.050", 165.0, 284.5), w(")", 188.0, 284.5), w("[1.27]", 168.0, 293.2),
            w("(.213)", 297.0, 299.8), w("[5.4]", 300.0, 308.4),
        ];
        let l = land_pattern(27, &words, 792.0).unwrap();
        assert_eq!((l.pins, l.pad_l, l.pad_w, l.pitch, l.row), (8, 1.55, 0.6, Some(1.27), Some(5.4)));
    }

    /// TI's WQFN-16 (RTE, as LM339B's sheet draws it): pads on four sides, the pitch had by 12 gaps, the exposed pad "( 1.68)" with
    /// its bracket apart, written once (square) with "17" in it, and the vias' spacing "(0.58) TYP" that is no pad's.
    #[test]
    fn a_ti_wqfn16() {
        let words = vec![
            w("RTE0016C", 58.0, 64.0), w("WQFN", 391.0, 64.0),
            w("(", 285.0, 150.0), w("1.68)", 295.0, 150.0), w("SYMM", 289.0, 164.0), w("13", 336.0, 175.0), w("16", 254.0, 175.0),
            w("16X", 157.0, 202.0), w("(0.6)", 173.0, 202.0), w("1", 196.0, 222.0), w("12", 398.0, 235.0),
            w("16X", 140.0, 255.0), w("(0.24)", 156.0, 255.0), w("SYMM", 432.0, 268.0), w("17", 306.0, 268.0),
            w("(2.8)", 461.0, 277.0), w("(0.58)", 408.0, 290.0), w("TYP", 411.0, 298.0),
            w("12X", 155.0, 306.0), w("(0.5)", 172.0, 306.0), w("9", 398.0, 321.0), w("4", 195.0, 325.0),
            w("(", 153.0, 347.0), w("0.2)", 163.0, 347.0), w("TYP", 179.0, 347.0), w("VIA", 182.0, 355.0),
            w("(R0.05)", 174.0, 389.0), w("(0.58)", 246.0, 392.0), w("TYP", 269.0, 392.0), w("(2.8)", 291.0, 406.0),
        ];
        let l = land_pattern(40, &words, 792.0).unwrap();
        assert_eq!(l, LandPattern { page: 40, family: Some("wqfn"), pins: 16, pad_l: 0.6, pad_w: 0.24, pitch: Some(0.5), row: Some(2.8), quad: true, ep: Some((1.68, 1.68)), ep_pin: Some("17".into()) });
        assert!(l.four_sided() && !l.dual());
    }

    /// TI's WSON-8 (DSG, LM393B): two rows, an exposed pad of 0.9 across and 1.6 along the rows (1.6 does not fit between the rows'
    /// pads), "9" in it.
    #[test]
    fn a_ti_wson8() {
        let words = vec![
            w("DSG0008A", 58.0, 64.0), w("WSON", 390.0, 64.0),
            w("(0.9)", 294.0, 172.0), w("(", 352.0, 172.0), w("0.2)", 362.0, 172.0), w("VIA", 378.0, 172.0),
            w("8X", 193.0, 177.0), w("(0.5)", 205.0, 177.0), w("TYP", 352.0, 181.0), w("1", 227.0, 193.0), w("8", 373.0, 198.0),
            w("8X", 177.0, 223.0), w("(0.25)", 189.0, 223.0), w("(0.55)", 384.0, 233.0), w("SYMM", 197.0, 240.0), w("9", 306.0, 240.0),
            w("(1.6)", 409.0, 251.0), w("6X", 193.0, 277.0), w("(0.5)", 205.0, 277.0), w("5", 373.0, 290.0), w("4", 227.0, 297.0),
            w("(R0.05)", 172.0, 313.0), w("(1.9)", 294.0, 336.0),
        ];
        let l = land_pattern(48, &words, 792.0).unwrap();
        assert_eq!((l.family, l.pins, l.pad_l, l.pad_w, l.pitch, l.row), (Some("wson"), 8, 0.5, 0.25, Some(0.5), Some(1.9)));
        assert_eq!((l.quad, l.ep, l.ep_pin.as_deref()), (false, Some((0.9, 1.6)), Some("9")));
        assert!(l.dual());
    }

    #[test]
    fn sizes_and_counts_are_told_apart() {
        assert_eq!(in_brackets("(1.27)"), Some(1.27));
        assert_eq!(in_brackets("(R0.05)"), None);
        assert_eq!(in_brackets("1.27"), None);
        assert_eq!(count("14X"), Some(14));
        assert_eq!(count("SCALE:8X"), None);
    }
}

#[cfg(test)]
mod real_pdf {
    /// TI's land patterns (skipped without the files): SOIC-14 of SN74HC14, SOIC-8 of LM393, SOT-23-5 of TMUX1101.
    #[test]
    fn ti_land_patterns() {
        let open = |f: &str| crate::Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(f)).ok();
        if let Some(ds) = open("sn74hc14.pdf") {
            let l = ds.land_patterns();
            let soic = l.iter().find(|l| l.family == Some("soic")).unwrap();
            assert_eq!((soic.pins, soic.pad_l, soic.pad_w, soic.pitch, soic.row), (14, 1.55, 0.6, Some(1.27), Some(5.4)), "{l:?}");
        }
        if let Some(ds) = open("lm393.pdf") {
            let l = ds.land_patterns();
            let soic = l.iter().find(|l| l.family == Some("soic") && l.pins == 8).unwrap();
            assert_eq!((soic.pad_l, soic.pad_w, soic.pitch, soic.row), (1.55, 0.6, Some(1.27), Some(5.4)), "{l:?}");
            let wson = l.iter().find(|l| l.family == Some("wson")).unwrap();
            assert_eq!((wson.pins, wson.pad_l, wson.pad_w, wson.pitch, wson.row, wson.ep), (8, 0.5, 0.25, Some(0.5), Some(1.9), Some((0.9, 1.6))), "{l:?}");
        }
        if let Some(ds) = open("lm139.pdf") {
            let l = ds.land_patterns();
            let qfn = l.iter().find(|l| l.family == Some("wqfn")).unwrap();
            assert_eq!((qfn.pins, qfn.quad, qfn.pad_l, qfn.pad_w, qfn.pitch, qfn.row, qfn.ep), (16, true, 0.6, 0.24, Some(0.5), Some(2.8), Some((1.68, 1.68))), "{l:?}");
        }
    }

    /// Chip dimension tables (skipped without the files): Yageo's RT (the 0603 row's bottom terminal I2) and TDK's CGA (the
    /// terminal's minimum and the gap's).
    #[test]
    fn chip_dimension_tables() {
        let open = |f: &str| crate::Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(f)).ok();
        if let Some(ds) = open("PYU-RT_1-TO-0-01_ROHS_L.pdf") {
            let s = ds.chip_sizes();
            assert_eq!(s.len(), 9, "{s:?}");
            let r = s.iter().find(|c| c.inch_code() == Some("0603")).unwrap();
            assert!(r.term_from == "I2" && (r.term.unwrap().1 - 0.4).abs() < 1e-9, "{r:?}");
        }
        if let Some(ds) = open("mlcc_automotive_hightemp_ja.pdf") {
            let s = ds.chip_sizes();
            let r = s.iter().find(|c| c.code == "CGA2").unwrap();
            assert!((r.term.unwrap().0 - 0.1).abs() < 1e-9 && (r.term.unwrap().1 - 0.375).abs() < 1e-9, "{r:?}");
        }
    }

    /// Nexperia's reflow soldering footprints (skipped without the files): the framed centre distance is found in the page's
    /// picture, the land is the second smallest of the sizes with "(2×)".
    #[test]
    fn nexperia_soldering_footprints() {
        let open = |f: &str| crate::Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(f)).ok();
        if let Some(ds) = open("PESD5V0U1BA.pdf") {
            let l = ds.land_patterns();
            assert_eq!(l.iter().map(|l| (l.page, l.family, l.pins, l.pad_l, l.pad_w, l.row)).collect::<Vec<_>>(), [(7, Some("sod-323"), 2, 0.6, 0.6, Some(2.2))], "{l:?}");
        }
        if let Some(ds) = open("PESD5V0V1BL-Q.pdf") {
            let l = ds.land_patterns();
            assert_eq!(l.iter().map(|l| (l.page, l.family, l.pins, l.pad_l, l.pad_w, l.row)).collect::<Vec<_>>(), [(8, Some("sod-882"), 2, 0.4, 0.7, Some(0.7))], "{l:?}");
        }
    }
}
