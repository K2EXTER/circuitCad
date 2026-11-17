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

impl crate::Datasheet {
    /// The land patterns of every page with a land pattern heading that has one.
    pub fn land_patterns(&self) -> Vec<LandPattern> {
        let q = crate::Heading::LandPattern.queries();
        (0..self.page_count())
            .filter(|&p| self.search_page_any(p, &q).iter().any(|h| !h.in_contents))
            .filter_map(|p| land_pattern(p, self.words(p), self.page_size(p).map_or(842.0, |s| s.1)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0)
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
}
