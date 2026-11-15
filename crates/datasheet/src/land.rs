//! The pads of a recommended land pattern drawing, from the way its sizes are written: "14X (1.55)" is a size that 14 things have
//! (14 pads: their length or width), "12X (1.27)" one that 12 gaps have (the pitch between 14 pads in two rows), a bare "(5.4)" a
//! size once (the distance between the rows' centres). Texas Instruments draws every "EXAMPLE BOARD LAYOUT" this way. Drawings
//! that write their sizes without counts, or draw them as lines, are not read.

use crate::draft::{PackageName, package_of};
use crate::text::{Word, normalize};

/// A dual-row (or single-row) land pattern read off a page.
#[derive(Clone, Debug, PartialEq)]
pub struct LandPattern {
    pub page: usize,
    /// The package the page is about (from its title lines), if it says.
    pub family: Option<&'static str>,
    pub pins: u32,
    /// Pad length (the long side) and width, mm.
    pub pad_l: f64,
    pub pad_w: f64,
    /// Between neighbouring pads, and between the centres of the two rows (none for a single row), mm.
    pub pitch: Option<f64>,
    pub row: Option<f64>,
}

impl LandPattern {
    /// Pins in two rows of equal length (the form's "デュアル").
    pub fn dual(&self) -> bool {
        self.row.is_some() && self.pins.is_multiple_of(2)
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

/// A size written on the page: its value in mm and where it stands.
struct Size {
    mm: f64,
    x0: f32,
    base: f32,
}

/// The sizes of a page. With millimetres in square brackets on the page, those are the sizes (the bracketed numbers over them are
/// inches); else the bracketed ones, with a bracket split from its number ("(.061" ")") joined.
fn sizes(words: &[Word]) -> Vec<Size> {
    let square: Vec<Size> = words.iter().filter_map(|w| in_square(&w.text).map(|mm| Size { mm, x0: w.rect.x0, base: w.baseline() })).collect();
    if !square.is_empty() {
        return square;
    }
    let mut out = vec![];
    for (i, w) in words.iter().enumerate() {
        let joined = match w.text.trim() {
            t if t.starts_with('(') && !t.ends_with(')') => words.get(i + 1).filter(|n| n.text.trim() == ")" && (n.baseline() - w.baseline()).abs() < 2.0).map(|_| format!("{t})")),
            t => Some(t.to_string()),
        };
        if let Some(mm) = joined.as_deref().and_then(in_brackets) {
            out.push(Size { mm, x0: w.rect.x0, base: w.baseline() });
        }
    }
    out
}

/// The land pattern of one page: the sizes that the most things have are the pads (the bigger the length, the smaller the width),
/// the pitch is had by one gap fewer per row (two rows: pins − 2), the rows' distance is the biggest bare size beyond the pad.
pub fn land_pattern(page: usize, words: &[Word], page_height: f32) -> Option<LandPattern> {
    let sizes = sizes(words);
    // the size of a count: right after it on its line, or (millimetres under inches) just under that
    let of_count = |w: &Word| -> Option<usize> {
        let beside = |s: &Size| (s.base - w.baseline()).abs() < 2.0 && s.x0 > w.rect.x0 && s.x0 - w.rect.x1 < 8.0;
        let under = |s: &Size| s.base > w.baseline() + 3.0 && s.base < w.baseline() + 14.0 && s.x0 > w.rect.x0 - 2.0 && s.x0 < w.rect.x1 + 30.0;
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
    let bare: Vec<f64> = sizes.iter().zip(&used).filter(|(_, u)| !**u).map(|(s, _)| s.mm).collect();
    // the pads: the count had by two sizes, the biggest such count
    let pins = counted.iter().map(|c| c.0).filter(|n| counted.iter().filter(|c| c.0 == *n).count() >= 2).max()?;
    let sizes: Vec<f64> = counted.iter().filter(|c| c.0 == pins).map(|c| c.1).collect();
    let pad_l = sizes.iter().copied().fold(f64::MIN, f64::max);
    let pad_w = sizes.iter().copied().fold(f64::MAX, f64::min);
    if pad_l <= pad_w {
        return None;
    }
    let row = bare.iter().copied().filter(|v| *v > pad_l).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    let gaps = if row.is_some() && pins.is_multiple_of(2) { pins - 2 } else { pins - 1 };
    let pitch = counted.iter().find(|c| c.0 == gaps && c.1 < pad_l * 2.0).map(|c| c.1);
    // the package: named in the page's title lines (the top 15 %)
    let family = words.iter().filter(|w| w.rect.y1 < page_height * 0.15).find_map(|w| package_of(&normalize(&w.text)).map(|p| p.0));
    Some(LandPattern { page, family, pins, pad_l, pad_w, pitch, row })
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
        assert_eq!(l, LandPattern { page: 22, family: Some("soic"), pins: 14, pad_l: 1.55, pad_w: 0.6, pitch: Some(1.27), row: Some(5.4) });
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
        }
    }
}
