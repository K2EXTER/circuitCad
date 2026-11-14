//! Values of the electrical characteristics table (M10, step 3): the row of a symbol (`VGS(th)`, `RDS(ON)`, `Ciss`,
//! `VF` …) read as min / typ / max, its unit and its test conditions, so the part form can offer them for its
//! fields. The table is found by its header line (`Min Typ Max`, `最小 標準 最大`); a symbol's row runs down to
//! the next symbol in the same column (one symbol often has several lines: RDS(ON) at three gate voltages).
//! Nothing is guessed when the header is not found. GUI-independent.

use crate::text::{Rect, Word, normalize};

/// One line of a symbol's row.
#[derive(Clone, Debug, PartialEq)]
pub struct SpecValue {
    pub page: usize,
    /// The symbol as printed.
    pub symbol: String,
    /// The test conditions as printed ("VGS=4.5V, ID=5A"); the nearest ones when the line has none of its own.
    pub conditions: String,
    /// In the table's unit (see `unit`).
    pub min: Option<f64>,
    pub typ: Option<f64>,
    pub max: Option<f64>,
    /// As printed ("mΩ", "pF"), when found.
    pub unit: Option<String>,
    /// The numbers' line on the page (to show where it came from).
    pub rect: Rect,
}

impl SpecValue {
    /// `typ` in SI units (the unit's prefix applied: 18 mΩ -> 0.018).
    pub fn typ_si(&self) -> Option<f64> {
        Some(self.typ? * prefix(self.unit.as_deref()?)?)
    }

    pub fn min_si(&self) -> Option<f64> {
        Some(self.min? * prefix(self.unit.as_deref()?)?)
    }

    pub fn max_si(&self) -> Option<f64> {
        Some(self.max? * prefix(self.unit.as_deref()?)?)
    }

    /// A condition's value in SI units: `condition("ID")` of "VGS=4.5V, ID=250µA" is 250e-6. Signs are dropped (a
    /// P channel's "-4.5V" is 4.5), so the form works with magnitudes.
    pub fn condition(&self, name: &str) -> Option<f64> {
        condition(&self.conditions, name)
    }
}

/// The value of `name=…` in a conditions text, in SI units and as a magnitude.
pub fn condition(text: &str, name: &str) -> Option<f64> {
    let key = normalize(name).replace(' ', "");
    // "VDS=VGS ID=250µA", "VGS= ±12V": a new part starts at each word holding an '='
    let mut parts: Vec<String> = vec![];
    for tok in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty()) {
        match parts.last_mut() {
            Some(last) if !tok.contains('=') || last.ends_with('=') => last.push_str(tok),
            _ => parts.push(tok.to_string()),
        }
    }
    for part in &parts {
        let Some((k, v)) = part.split_once('=') else { continue };
        if normalize(k) != key {
            continue;
        }
        let v = v.trim_start_matches(['±', '-', '+']);
        let digits: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let x: f64 = digits.parse().ok()?;
        return Some(x * prefix(&v[digits.len()..]).unwrap_or(1.0));
    }
    None
}

/// The factor of a unit's SI prefix ("mΩ" 1e-3, "pF" 1e-12, "V" 1, "µA" 1e-6). None for something that is not a unit.
pub fn prefix(unit: &str) -> Option<f64> {
    // the ohm sign (U+2126) stands for the Greek capital omega in some fonts' text
    let u = unit.trim().trim_end_matches(['.', ')']).trim_start_matches('(').replace('\u{2126}', "Ω");
    let u = u.as_str();
    const BASES: [&str; 11] = ["V", "A", "Ω", "ohm", "F", "S", "s", "W", "Hz", "C", "mho"];
    for b in BASES {
        if let Some(p) = u.strip_suffix(b) {
            return match p {
                "" => Some(1.0),
                "k" | "K" => Some(1e3),
                "M" => Some(1e6),
                "m" => Some(1e-3),
                "µ" | "μ" | "u" => Some(1e-6),
                "n" => Some(1e-9),
                "p" => Some(1e-12),
                _ => None,
            };
        }
    }
    None
}

/// The table's columns from its header line.
struct Header {
    y: f32,
    min: f32,
    typ: f32,
    max: f32,
    unit: Option<f32>,
    /// Left edge of the conditions column, when headed ("Conditions", "Test Conditions", "条件").
    conditions: Option<f32>,
}

fn centre(r: &Rect) -> (f32, f32) {
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

/// Header lines: a line holding min, typ and max (English or Japanese).
fn headers(words: &[Word]) -> Vec<Header> {
    let is = |w: &Word, names: &[&str]| {
        let t = normalize(w.text.trim_end_matches('.'));
        names.contains(&t.as_str())
    };
    let mut out = vec![];
    for w in words.iter().filter(|w| is(w, &["typ", "typical", "標準", "代表値"])) {
        let y = centre(&w.rect).1;
        let same = |names: &[&str]| words.iter().filter(|o| (centre(&o.rect).1 - y).abs() < 4.0 && is(o, names)).map(|o| centre(&o.rect).0).min_by(|a, b| (a - centre(&w.rect).0).abs().total_cmp(&(b - centre(&w.rect).0).abs()));
        let (Some(min), Some(max)) = (same(&["min", "minimum", "最小"]), same(&["max", "maximum", "最大"])) else { continue };
        let unit = same(&["units", "unit", "単位"]);
        let conditions = words.iter().filter(|o| (centre(&o.rect).1 - y).abs() < 4.0 && (normalize(&o.text).contains("condition") || o.text.contains("条件"))).map(|o| o.rect.x0).fold(None, |a: Option<f32>, x| Some(a.map_or(x, |a| a.min(x))));
        if !out.iter().any(|h: &Header| (h.y - y).abs() < 4.0) {
            out.push(Header { y, min, typ: centre(&w.rect).0, max, unit, conditions });
        }
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    out
}

/// A number in a cell ("1.05", "−0.9", "±12" is not a value), as printed.
fn number(t: &str) -> Option<f64> {
    let t = t.trim().replace('\u{2212}', "-");
    if t.starts_with('±') {
        return None;
    }
    t.parse::<f64>().ok()
}

/// The words of one printed line (centre within 3 points of `y`), left to right.
fn line_at(words: &[Word], y: f32) -> Vec<&Word> {
    let mut l: Vec<&Word> = words.iter().filter(|w| (centre(&w.rect).1 - y).abs() < 3.0).collect();
    l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    l
}

/// The symbol's key for matching: lower case, no spaces, no brackets ("V GS(th)" and "VGS(TH)" -> "vgsth").
fn key(s: &str) -> String {
    normalize(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The rows of `symbol` on a page.
pub fn find(page: usize, words: &[Word], symbol: &str) -> Vec<SpecValue> {
    let want = key(symbol);
    if want.is_empty() {
        return vec![];
    }
    let heads = headers(words);
    let mut out = vec![];
    for h in &heads {
        // the table runs from its header down to the next header (or the page's end)
        let bottom = heads.iter().map(|o| o.y).find(|&y| y > h.y + 1.0).unwrap_or(f32::MAX);
        let left = h.conditions.unwrap_or(h.min - 120.0);
        let body: Vec<&Word> = words.iter().filter(|w| centre(&w.rect).1 > h.y + 3.0 && centre(&w.rect).1 < bottom).collect();
        // symbols: the words left of the parameter text, i.e. in the first column (left of a third of the way to Min)
        // symbols: the first word of a line in the table's first column (left aligned; the parameter's name follows)
        let first_x = body.iter().map(|w| w.rect.x0).fold(f32::MAX, f32::min);
        let mut symbols: Vec<&&Word> = body.iter().filter(|w| w.rect.x0 < first_x + 25.0 && w.rect.x1 < left && !w.text.trim().is_empty()).collect();
        symbols.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        let mut seen: Vec<f32> = vec![];
        symbols.retain(|w| {
            let y = centre(&w.rect).1;
            let fresh = !seen.iter().any(|s| (s - y).abs() < 3.0);
            seen.push(y);
            fresh
        });
        // every line with a number under min / typ / max
        let mut ys: Vec<f32> = body.iter().filter(|w| number(&w.text).is_some() && column(h, centre(&w.rect).0).is_some()).map(|w| centre(&w.rect).1).collect();
        ys.sort_by(f32::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 3.0);
        // a line belongs to the symbol printed on it; a line without one to the nearest symbol that has no numbers on
        // its own line (a symbol printed once in the middle of its lines: RDS(ON) at three gate voltages)
        let sym_y: Vec<f32> = symbols.iter().map(|w| centre(&w.rect).1).collect();
        let bare: Vec<bool> = sym_y.iter().map(|&sy| !ys.iter().any(|&y| (y - sy).abs() < 3.0)).collect();
        let owner = |y: f32| -> Option<usize> {
            if let Some(i) = sym_y.iter().position(|&sy| (sy - y).abs() < 3.0) {
                return Some(i);
            }
            let nearest = |only_bare: bool| (0..sym_y.len()).filter(|&i| !only_bare || bare[i]).min_by(|&a, &b| (sym_y[a] - y).abs().total_cmp(&(sym_y[b] - y).abs()));
            nearest(true).filter(|&i| (sym_y[i] - y).abs() < 30.0).or_else(|| nearest(false))
        };
        for (si, s) in symbols.iter().enumerate().filter(|(_, w)| key(&symbol_text(&body, w)) == want) {
            let mine: Vec<f32> = ys.iter().copied().filter(|&y| owner(y) == Some(si)).collect();
            let sy = sym_y[si];
            let (top, end) = (mine.iter().copied().fold(sy, f32::min) - 6.0, mine.iter().copied().fold(sy, f32::max) + 6.0);
            let band: Vec<&&Word> = body.iter().filter(|w| (top..end).contains(&centre(&w.rect).1)).collect();
            for y in mine {
                let line = line_at(words, y);
                let (mut v, mut rect) = ([None, None, None], None::<Rect>);
                for w in &line {
                    if let (Some(x), Some(c)) = (number(&w.text), column(h, centre(&w.rect).0)) {
                        v[c] = Some(x);
                        rect = Some(rect.map_or(w.rect, |r| Rect { x0: r.x0.min(w.rect.x0), y0: r.y0.min(w.rect.y0), x1: r.x1.max(w.rect.x1), y1: r.y1.max(w.rect.y1) }));
                    }
                }
                let conditions = conditions_near(words, h, left, y, top, end);
                let unit = unit_near(&band, h, y);
                out.push(SpecValue { page, symbol: symbol_text(&body, s), conditions, min: v[0], typ: v[1], max: v[2], unit, rect: rect.unwrap_or(s.rect) });
            }
        }
    }
    out
}

/// The symbol with the pieces printed right after it on its line ("V" + "GS(th)" as subscript).
fn symbol_text(body: &[&Word], s: &Word) -> String {
    let mut t = s.text.clone();
    let mut end = s.rect.x1;
    let mut next: Vec<&&Word> = body.iter().filter(|w| (centre(&w.rect).1 - centre(&s.rect).1).abs() < 5.0 && w.rect.x0 >= end - 0.5).collect();
    next.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    for w in next {
        if w.rect.x0 - end > 1.0 {
            break;
        }
        t.push_str(&w.text);
        end = w.rect.x1;
    }
    t
}

/// Which of min (0), typ (1), max (2) a number at `x` is under.
fn column(h: &Header, x: f32) -> Option<usize> {
    let d = [h.min, h.typ, h.max].map(|c| (c - x).abs());
    let gap = ((h.typ - h.min).abs()).min((h.max - h.typ).abs());
    let (i, best) = d.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1))?;
    (*best < gap * 0.6).then_some(i)
}

/// The conditions on the line `y` (words between the conditions column and the min column); when it has none,
/// the nearest line inside the row that has some, else the nearest line with an `=` within 30 points (a group of
/// rows printed once for all: Ciss, Coss, Crss).
fn conditions_near(words: &[Word], h: &Header, left: f32, y: f32, top: f32, end: f32) -> String {
    let at = |y: f32| -> String {
        line_at(words, y).iter().filter(|w| w.rect.x0 >= left - 2.0 && w.rect.x1 < h.min - 8.0).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    };
    let own = at(y);
    if own.contains('=') {
        return own;
    }
    let mut ys: Vec<f32> = words.iter().filter(|w| w.text.contains('=') && w.rect.x0 >= left - 2.0 && w.rect.x1 < h.min - 8.0).map(|w| centre(&w.rect).1).filter(|&c| (c - y).abs() < 30.0).collect();
    // inside the row first, then the nearer (in steps of 3 points, a line's spacing being about 13), then above:
    // the last row of a group (Crss under Coss) takes the group's conditions, not the next row's
    ys.sort_by(|a, b| {
        let outside = |c: f32| !(top..end).contains(&c);
        let step = |c: f32| ((c - y).abs() / 3.0).round() as i32;
        (outside(*a), step(*a), *a > y).cmp(&(outside(*b), step(*b), *b > y))
    });
    ys.first().map(|&c| at(c)).unwrap_or(own)
}

/// The unit in the units column nearest to line `y` within the row.
fn unit_near(band: &[&&Word], h: &Header, y: f32) -> Option<String> {
    let ux = h.unit?;
    band.iter()
        .filter(|w| (centre(&w.rect).0 - ux).abs() < 20.0 && prefix(&w.text).is_some())
        .min_by(|a, b| (centre(&a.rect).1 - y).abs().total_cmp(&(centre(&b.rect).1 - y).abs()))
        .map(|w| w.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table laid out like a Japanese data sheet's: one symbol over two lines, and a header in Japanese.
    fn table() -> Vec<Word> {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 6.0 * t.chars().count() as f32, y + 8.0);
        vec![
            w("記号", 20.0, 100.0), w("項目", 70.0, 100.0), w("条件", 150.0, 100.0), w("最小", 260.0, 100.0), w("標準", 300.0, 100.0), w("最大", 340.0, 100.0), w("単位", 380.0, 100.0),
            w("IR", 20.0, 115.0), w("逆電流", 70.0, 115.0), w("VR=40V", 150.0, 115.0), w("0.5", 342.0, 115.0), w("mA", 382.0, 115.0),
            w("IF=0.1A", 150.0, 128.0), w("0.30", 300.0, 128.0),
            w("VF", 20.0, 134.0), w("順電圧", 70.0, 134.0), w("V", 384.0, 134.0),
            w("IF=1A", 150.0, 140.0), w("0.45", 300.0, 140.0), w("0.55", 340.0, 140.0),
        ]
    }

    #[test]
    fn a_symbol_printed_between_its_lines_gets_both() {
        let words = table();
        let vf = find(0, &words, "VF");
        assert_eq!(vf.len(), 2, "{vf:?}");
        assert_eq!((vf[0].typ, vf[0].condition("IF"), vf[0].unit.as_deref()), (Some(0.30), Some(0.1), Some("V")));
        assert_eq!((vf[1].typ, vf[1].max, vf[1].condition("IF")), (Some(0.45), Some(0.55), Some(1.0)));
        let ir = find(0, &words, "I R");
        assert_eq!((ir[0].max_si(), ir[0].typ), (Some(0.5e-3), None), "{ir:?}");
        assert!(find(0, &words, "Ciss").is_empty());
        // no header, no values (nothing is guessed)
        assert!(find(0, &words[7..], "VF").is_empty());
    }

    #[test]
    fn units_and_conditions_are_read_in_si() {
        assert_eq!(prefix("mΩ"), Some(1e-3));
        assert_eq!(prefix("m\u{2126}"), Some(1e-3));
        assert_eq!(prefix("pF"), Some(1e-12));
        assert_eq!(prefix("V"), Some(1.0));
        assert_eq!(prefix("µA"), Some(1e-6));
        assert_eq!(prefix("nC"), Some(1e-9));
        assert_eq!(prefix("mm"), None);
        assert_eq!(prefix("Typ"), None);
        assert_eq!(condition("VGS=4.5V, ID=250µA", "ID"), Some(250e-6));
        assert_eq!(condition("VGS=4.5V, ID=250µA", "vgs"), Some(4.5));
        assert_eq!(condition("VDS=VGS ID=-4.0A", "ID"), Some(4.0), "conditions without a comma between them");
        assert_eq!(condition("VDS=0V, VGS= ±12V", "VGS"), Some(12.0));
        assert_eq!(condition("VGS=-10V, ID=-4.0A", "ID"), Some(4.0));
        assert_eq!(condition("VDS=VGS", "VDS"), None);
    }
}

/// On a real data sheet (the AO3400A's in `microohm-meter/`, on the author's PC; skipped elsewhere).
#[cfg(test)]
mod real_pdf {
    #[test]
    fn the_mosfet_table_gives_its_typical_values_with_their_conditions() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/AO3400A_ds.pdf");
        let Ok(ds) = crate::Datasheet::open_file(&path) else { return };
        let vth = ds.spec_values("VGS(th)");
        let v = &vth[0];
        assert_eq!((v.min, v.typ, v.max), (Some(0.65), Some(1.05), Some(1.45)));
        assert_eq!(v.typ_si(), Some(1.05));
        assert!((v.condition("ID").unwrap() - 250e-6).abs() < 1e-12, "{v:?}");
        let r = ds.spec_values("RDS(ON)");
        let at = |vgs: f64| r.iter().find(|x| x.condition("VGS") == Some(vgs)).unwrap_or_else(|| panic!("{vgs}: {r:?}"));
        assert!((at(10.0).typ_si().unwrap() - 0.018).abs() < 1e-12);
        assert!((at(4.5).typ_si().unwrap() - 0.019).abs() < 1e-12);
        assert!((at(2.5).typ_si().unwrap() - 0.024).abs() < 1e-12);
        assert_eq!(at(10.0).condition("ID"), Some(5.7));
        let ciss = &ds.spec_values("Ciss")[0];
        assert!((ciss.typ_si().unwrap() - 630e-12).abs() < 1e-18, "{ciss:?}");
        assert_eq!(ciss.condition("VDS"), Some(15.0), "the group's conditions: {ciss:?}");
        let crss = &ds.spec_values("Crss")[0];
        assert!((crss.typ_si().unwrap() - 50e-12).abs() < 1e-18, "{crss:?}");
        assert_eq!(crss.condition("VDS"), Some(15.0), "the group's conditions, not the next row's: {crss:?}");
        let g = &ds.spec_values("gFS")[0];
        assert_eq!((g.typ_si(), g.condition("VDS"), g.condition("ID")), (Some(33.0), Some(5.0), Some(5.7)));
        assert!(ds.spec_values("hFE").is_empty());
    }
}
