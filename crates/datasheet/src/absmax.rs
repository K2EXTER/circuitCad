//! The absolute maximum ratings table: its rows' symbols with their largest values, for the part's ratings (the most voltage, current,
//! power and temperature). The table is read from its heading down to the next table's heading ("Recommended Operating Conditions",
//! "Electrical Characteristics", "Thermal …"): a row is a symbol ("VDS", "ID", "TJ, TSTG") and the numbers beside it, on its line or
//! printed a little above or below it (a row whose label takes two lines puts its value on one of them: "5.7" over "ID").

use crate::text::{Word, normalize};
use crate::{Datasheet, Heading};

/// A row of the table: the symbol as written, the largest value in SI units (V, A, W, °C), the unit, and the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Rating {
    pub symbol: String,
    pub value: f64,
    pub unit: String,
    pub page: usize,
}

/// Headings that end the table.
const NEXT: &[&str] = &["recommended operating", "electrical characteristics", "thermal characteristics", "thermal information", "esd ratings", "推奨動作条件", "電気的特性", "熱特性", "熱に関する情報"];

/// A symbol of a rating: V, I, P or T, then a few letters or digits ("VDS", "ID", "IF(AV)", "TJ", "VCEO"), a trailing comma dropped.
fn symbol(t: &str) -> Option<String> {
    let t = t.trim().trim_end_matches(',');
    let first = t.chars().next()?;
    let shape = "VIPT".contains(first) && (2..=10).contains(&t.chars().count()) && t.chars().skip(1).all(|c| c.is_ascii_alphanumeric() || "()".contains(c));
    // a symbol has a capital after its first letter ("VDS", "TJ", "IF(AV)"), or is one of the few written in small letters
    let ok = shape && (t.chars().skip(1).any(|c| c.is_ascii_uppercase()) || ["Tj", "Tstg", "Ptot", "Tamb", "Ipeak"].contains(&t)) && !["TOP", "VIEW", "TTL", "TYP"].contains(&t);
    ok.then(|| t.to_string())
}

/// A number of the table: "30", "±12", "-55", "0.9"; with the unit written in the same word ("30V") taken apart.
fn number(t: &str) -> Option<(f64, Option<String>)> {
    let t = normalize(t.trim()).replace('±', "");
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(t.len());
    let (n, rest) = t.split_at(split);
    let v: f64 = n.parse().ok()?;
    let rest = rest.trim();
    (rest.is_empty() || unit_scale(rest).is_some()).then(|| (v, (!rest.is_empty()).then(|| rest.to_string())))
}

/// The scale of a unit word to SI: "mA" → 1e-3, "V" → 1, "°C" → 1.
fn unit_scale(u: &str) -> Option<f64> {
    let u = u.trim().replace('℃', "°c");
    let u = u.as_str();
    let base = ["v", "a", "w", "°c", "c"];
    if base.contains(&u.to_lowercase().as_str()) {
        return Some(1.0);
    }
    let (p, rest) = u.split_at(u.chars().next()?.len_utf8());
    if !base.contains(&rest.to_lowercase().as_str()) {
        return None;
    }
    Some(match p {
        "m" => 1e-3,
        "µ" | "μ" | "u" => 1e-6,
        "n" => 1e-9,
        "k" => 1e3,
        _ => return None,
    })
}

/// The rows of a table on a page, from `top` down to the next table's heading.
pub fn ratings(page: usize, words: &[Word], top: f32, bottom: f32) -> Vec<Rating> {
    let inside: Vec<&Word> = words.iter().filter(|w| w.rect.y0 > top && w.rect.y1 < bottom).collect();
    // the table ends at the next table's heading
    let joined = |y: f32| {
        let mut l: Vec<&&Word> = inside.iter().filter(|w| (w.baseline() - y).abs() < 2.0).collect();
        l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        normalize(&l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
    };
    let end = inside.iter().map(|w| w.baseline()).filter(|y| NEXT.iter().any(|n| joined(*y).contains(n))).fold(bottom, f32::min);
    let rows: Vec<&Word> = inside.into_iter().filter(|w| w.baseline() < end - 1.0).collect();
    let mut out: Vec<Rating> = vec![];
    for (w, sym) in rows.iter().filter_map(|w| symbol(&w.text).map(|sym| (w, sym))) {
        // the numbers right of the symbol on its line and on the lines within 8 points (a row whose label takes two lines, or one
        // line per version: "36 non-B" over "-0.3 … V" over "38 B only"): the largest
        let found: Vec<(&Word, f64, Option<String>)> = rows.iter().filter(|o| (o.baseline() - w.baseline()).abs() < 8.0 && o.rect.x0 > w.rect.x1).filter_map(|o| number(&o.text).map(|(v, u)| (*o, v, u))).collect();
        // each number with its unit: in the number's word, else the nearest unit word right of it within 8 points of its line or of
        // the symbol's; only those whose unit is the symbol's (V volts, I amperes, P watts, T degrees: not an order code beside the
        // table, "4000 / Tape & Reel"), the largest
        let unit_of = |n: &Word, inside: &Option<String>| {
            inside.clone().or_else(|| {
                rows.iter()
                    .filter(|o| ((o.baseline() - n.baseline()).abs() < 8.0 || (o.baseline() - w.baseline()).abs() < 8.0) && o.rect.x0 > n.rect.x1 && unit_scale(&o.text).is_some())
                    .min_by(|a, b| ((a.baseline() - n.baseline()).abs() * 10.0 + a.rect.x0).total_cmp(&((b.baseline() - n.baseline()).abs() * 10.0 + b.rect.x0)))
                    .map(|o| o.text.trim().to_string())
            })
        };
        let fits = |unit: &str| {
            let base = unit.to_lowercase().replace('℃', "°c");
            match sym.chars().next() {
                Some('V') => base.ends_with('v'),
                Some('I') => base.ends_with('a'),
                Some('P') => base.ends_with('w'),
                Some('T') => base.ends_with('c'),
                _ => false,
            }
        };
        let best = found.iter().filter_map(|(n, v, inside)| unit_of(n, inside).filter(|u| fits(u)).map(|u| (*v, u))).max_by(|a, b| a.0.total_cmp(&b.0));
        let Some((value, unit)) = best else { continue };
        if !out.iter().any(|r| r.symbol == sym) {
            let scale = unit_scale(&unit).unwrap_or(1.0);
            out.push(Rating { symbol: sym, value: value.abs() * scale, unit, page });
        }
    }
    out
}

impl Datasheet {
    /// The rows of the absolute maximum ratings table (its first page that has rows).
    pub fn absolute_maximum(&self) -> Vec<Rating> {
        let q = Heading::AbsMax.queries();
        for page in 0..self.page_count() {
            let h = self.page_size(page).map_or(842.0, |s| s.1);
            for hit in self.search_page_any(page, &q).into_iter().filter(|h| !h.in_contents) {
                let r = ratings(page, self.words(page), hit.rect.y1, h * 0.92);
                if r.len() >= 2 {
                    return r;
                }
            }
        }
        vec![]
    }
}

/// The first of `symbols` the table has (case and brackets ignored: "IF(AV)" finds "IF(AV)" and "IFAV").
pub fn pick<'a>(rows: &'a [Rating], symbols: &[&str]) -> Option<&'a Rating> {
    let key = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    symbols.iter().find_map(|s| rows.iter().find(|r| key(&r.symbol) == key(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0)
    }

    /// AO3400A's table: values on the symbol's line, a value printed above its symbol (a two-line row), a range of temperatures,
    /// and the table ending at the next heading.
    #[test]
    fn a_mosfet_table() {
        let words = vec![
            w("Parameter", 87.0, 430.0), w("Symbol", 262.0, 430.0), w("Maximum", 355.0, 430.0), w("Units", 474.0, 430.0),
            w("Drain-Source", 87.0, 441.0), w("30", 369.0, 441.0), w("V", 481.0, 441.0), w("VDS", 262.0, 441.0),
            w("Gate-Source", 87.0, 453.0), w("±12", 367.0, 453.0), w("V", 481.0, 453.0), w("VGS", 262.0, 453.0),
            w("5.7", 368.0, 465.0), w("TA=25°C", 168.0, 465.0), w("ID", 262.0, 471.0), w("4.7", 368.0, 477.0), w("A", 481.0, 477.0),
            w("1.4", 368.0, 503.0), w("PD", 262.0, 509.0), w("W", 480.0, 509.0),
            w("-55", 356.0, 527.0), w("to", 370.0, 527.0), w("150", 379.0, 527.0), w("°C", 479.0, 527.0), w("TJ,", 262.0, 527.0), w("TSTG", 275.0, 527.0),
            w("Thermal", 87.0, 548.0), w("Characteristics", 120.0, 548.0), w("RθJA", 262.0, 560.0), w("90", 369.0, 560.0), w("°C/W", 481.0, 560.0),
        ];
        let r = ratings(0, &words, 420.0, 700.0);
        let v = |s: &str| pick(&r, &[s]).map(|x| x.value);
        assert_eq!(v("VDS"), Some(30.0));
        assert_eq!(v("VGS"), Some(12.0));
        assert_eq!(v("ID"), Some(5.7), "the value over the symbol, at 25 °C: {r:?}");
        assert_eq!(v("TJ"), Some(150.0));
        assert!(r.iter().all(|x| !x.symbol.starts_with('R')), "the thermal table is not read");
    }

    #[test]
    fn units_scale_to_si() {
        assert_eq!(unit_scale("mA"), Some(1e-3));
        assert_eq!(unit_scale("V"), Some(1.0));
        assert_eq!(unit_scale("℃"), Some(1.0));
        assert_eq!(unit_scale("mm"), None);
        assert_eq!(number("±12"), Some((12.0, None)));
        assert_eq!(number("30V"), Some((30.0, Some("v".into()))));
        assert_eq!(symbol("TJ,").as_deref(), Some("TJ"));
        assert_eq!(symbol("Voltage"), None);
    }
}

#[cfg(test)]
mod real_pdf {
    /// AO3400A and TI's LM317L (skipped without the files).
    #[test]
    fn real_tables() {
        let open = |f: &str| crate::Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(f)).ok();
        if let Some(ds) = open("AO3400A_ds.pdf") {
            let r = ds.absolute_maximum();
            let v = |s: &str| super::pick(&r, &[s]).map(|x| x.value);
            assert_eq!((v("VDS"), v("ID"), v("PD"), v("TJ")), (Some(30.0), Some(5.7), Some(1.4), Some(150.0)), "{r:?}");
        }
    }
}
