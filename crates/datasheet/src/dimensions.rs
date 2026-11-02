//! The lengths written on the pages of a data sheet that have a drawing, to tell whether a value typed into the form
//! is there. This says that a number is on the page, not what it means; a person decides whether it is the right one.

use crate::text::{Word, unit_after};
use crate::value::parse_value;

/// A number on a page, as the millimetres it may stand for. When no unit is written the number may also be inches
/// (a land pattern drawn in inches has "0.050" for 1.27 mm); those values are kept apart in `as_inch`, because a
/// match there is weaker.
#[derive(Clone, Debug, PartialEq)]
pub struct PageLength {
    pub text: String,
    pub mm: Vec<f64>,
    pub as_inch: Vec<f64>,
}

/// Where a value was found: the text of the number, and whether it matched as inches.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub text: String,
    pub as_inch: bool,
}

/// The numbers of a page that can be lengths. A number with a unit that is not a length (V, A, °) is left out.
pub fn lengths(words: &[Word]) -> Vec<PageLength> {
    let mut out = vec![];
    for w in words {
        for tok in w.tokens() {
            let Some(v) = parse_value(tok) else { continue };
            let unit = v.unit.clone().or_else(|| if w.text.ends_with(tok) { unit_after(words, w) } else { None });
            let numbers = v.ends.map_or(vec![v.number], |(a, b)| vec![a, b]);
            let as_inch: Vec<f64> = if unit.is_none() { numbers.iter().filter(|n| (0.0..=2.0).contains(*n)).map(|n| n * 25.4).collect() } else { vec![] };
            let mm: Vec<f64> = numbers
                .into_iter()
                .flat_map(|n| match unit.as_deref().map(str::to_lowercase).as_deref() {
                    None | Some("mm") => vec![n],
                    Some("in" | "inch" | "inches" | "\"" | "\u{2033}") => vec![n * 25.4],
                    Some("mil" | "mils") => vec![n * 0.0254],
                    Some("um" | "\u{B5}m" | "\u{3BC}m") => vec![n / 1000.0],
                    Some("cm") => vec![n * 10.0],
                    Some(_) => vec![],
                })
                .collect();
            if !mm.is_empty() {
                out.push(PageLength { text: tok.to_string(), mm, as_inch });
            }
        }
    }
    out
}

/// A number that `mm` (a value from the form) is the same as, to the digits a person types: two decimals, or a
/// small part of the value. A number that matches as it is comes before one that matches as inches.
pub fn find_length(page: &[PageLength], mm: f64) -> Option<Found> {
    let tolerance = (0.004 * mm.abs()).max(0.011);
    let near = |list: &[f64]| list.iter().any(|c| (c - mm).abs() <= tolerance);
    page.iter()
        .find(|p| near(&p.mm))
        .map(|p| Found { text: p.text.clone(), as_inch: false })
        .or_else(|| page.iter().find(|p| near(&p.as_inch)).map(|p| Found { text: p.text.clone(), as_inch: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str, mm: &[f64], as_inch: &[f64]) -> PageLength {
        PageLength { text: text.into(), mm: mm.to_vec(), as_inch: as_inch.to_vec() }
    }

    #[test]
    fn a_value_is_found_within_the_digits_a_person_types() {
        let page = vec![p("1.27", &[1.27], &[32.258]), p("0.050in", &[1.27], &[]), p("0.6", &[0.6], &[15.24]), p("5.4", &[5.4], &[])];
        assert_eq!(find_length(&page, 1.27), Some(Found { text: "1.27".into(), as_inch: false }));
        assert_eq!(find_length(&page, 1.3), None);
        // typed with fewer digits, or a little rounded
        assert_eq!(find_length(&page, 0.6).unwrap().text, "0.6");
        assert_eq!(find_length(&page, 5.4).unwrap().text, "5.4");
        assert!(find_length(&page, 5.45).is_none());
        // inches without a unit match too, but they say so
        assert_eq!(find_length(&page, 15.24), Some(Found { text: "0.6".into(), as_inch: true }));
        assert!(find_length(&[], 1.0).is_none());
    }

    #[test]
    fn a_direct_match_comes_before_an_inch_match() {
        let page = vec![p("0.05", &[0.05], &[1.27]), p("1.27", &[1.27], &[])];
        assert_eq!(find_length(&page, 1.27), Some(Found { text: "1.27".into(), as_inch: false }));
    }
}
