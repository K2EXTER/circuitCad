//! A first draft of a part from a whole data sheet: what can be found without a person pointing at it. The package's name (the family
//! most written, "SOT-223", "SOIC-8"), the pin table under a "Pin Configuration" heading, and the dimension table of the package
//! drawing (rows "A 1.50 1.63 1.80": a letter symbol and its min / nom / max). Every item says the page it came from, so the form can
//! show it for a check; nothing here is certain.

use crate::text::{Word, normalize};
use crate::{Datasheet, Heading, Rect};

/// A package family and how many pins its name says ("SOIC-8" → `soic`, 8).
#[derive(Clone, Debug, PartialEq)]
pub struct PackageName {
    /// As data sheets write it, upper case ("SOT-223", "SOIC-8").
    pub name: String,
    pub family: &'static str,
    pub pins: Option<u32>,
    /// How often it is written, and the first page.
    pub count: usize,
    pub page: usize,
}

/// Families of packages, longest first where one starts another ("sot-223" before "sot-23").
const FAMILIES: &[&str] = &[
    "sot-223", "sot-23", "sot-89", "sot-323", "sot-363", "sot-563", "sc-70", "sc-88", "sc-74", "soic", "tssop", "msop", "vssop", "ssop", "so", "pdip", "dip", "to-220", "to-92", "to-252", "to-263", "dpak", "d2pak", "qfn", "dfn", "lqfp", "tqfp", "qfp", "sod-123", "sod-323", "sod-523",
    "do-214ac", "do-214aa", "do-214ab", "do-35", "do-41", "sma", "smb", "smc",
];

/// One word as a package name: the family and the pin count written with it ("soic-8", "soic8", "8-soic", "sot-23-5").
pub fn package_of(word: &str) -> Option<(&'static str, Option<u32>)> {
    let w = normalize(word);
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    // "8-soic" / "8soic": the count first
    let (lead, rest) = match w.find(|c: char| !c.is_ascii_digit()) {
        Some(k) if k > 0 => (w[..k].parse::<u32>().ok(), w[k..].trim_start_matches('-')),
        _ => (None, w),
    };
    // "SOT23" and "TO220" are written without the dash too
    let squeezed: String = rest.chars().filter(|c| *c != '-').collect();
    for f in FAMILIES {
        let f_squeezed: String = f.chars().filter(|c| *c != '-').collect();
        let tail = match rest.strip_prefix(f) {
            Some(t) => t,
            None if f.contains('-') && squeezed.starts_with(&f_squeezed) => &squeezed[f_squeezed.len()..],
            None => continue,
        };
        let tail = tail.trim_start_matches('-');
        if tail.is_empty() {
            return Some((f, lead));
        }
        if let Ok(n) = tail.parse::<u32>()
            && lead.is_none()
            && (1..=400).contains(&n)
        {
            return Some((f, Some(n)));
        }
        return None;
    }
    None
}

/// The package names of the data sheet, the most written first (the first page counts twice: it names the part's own package).
pub fn package_names(pages: &[&[Word]]) -> Vec<PackageName> {
    let mut out: Vec<PackageName> = vec![];
    for (p, words) in pages.iter().enumerate() {
        for w in words.iter() {
            for t in w.tokens() {
                let Some((family, pins)) = package_of(t) else { continue };
                // the bare two-letter families are too easily other words
                if matches!(family, "so" | "sma" | "smb" | "smc") && pins.is_none() && !t.chars().all(|c| c.is_ascii_uppercase() || c == '-') {
                    continue;
                }
                let name = match pins {
                    Some(n) => format!("{}-{n}", family.to_uppercase()),
                    None => family.to_uppercase(),
                };
                let weight = if p == 0 { 2 } else { 1 };
                match out.iter_mut().find(|x| x.name == name) {
                    Some(x) => x.count += weight,
                    None => out.push(PackageName { name, family, pins, count: weight, page: p }),
                }
            }
        }
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then(b.pins.is_some().cmp(&a.pins.is_some())).then(a.page.cmp(&b.page)));
    out
}

/// A row of a dimension table, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dim {
    pub min: Option<f64>,
    pub nom: Option<f64>,
    pub max: Option<f64>,
}

impl Dim {
    /// The nominal value, else the middle of min and max, else whichever is given.
    pub fn typical(&self) -> Option<f64> {
        self.nom.or(match (self.min, self.max) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        })
    }

    /// (min, max), each falling back to the other values given.
    pub fn range(&self) -> Option<(f64, f64)> {
        let t = self.typical()?;
        Some((self.min.unwrap_or(t), self.max.unwrap_or(t)))
    }
}

/// The dimension table of a package drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct DimTable {
    pub page: usize,
    /// The table was in inches (its values are converted).
    pub from_inch: bool,
    /// (symbol as written, "A1", "e", "E1"; the row), in the table's order.
    pub rows: Vec<(String, Dim)>,
}

impl DimTable {
    /// The row of `symbol` (case matters: `e` is the pitch, `E` the width).
    pub fn get(&self, symbol: &str) -> Option<Dim> {
        self.rows.iter().find(|r| r.0 == symbol).map(|r| r.1)
    }
}

/// A dimension symbol: a letter, then up to two letters or digits ("A", "A1", "b1", "E1", "HE", "L1").
fn is_symbol(t: &str) -> bool {
    let mut c = t.chars();
    let Some(first) = c.next() else { return false };
    let rest: Vec<char> = c.collect();
    first.is_ascii_alphabetic() && rest.len() <= 2 && rest.iter().all(|c| c.is_ascii_alphanumeric()) && !matches!(t.to_ascii_lowercase().as_str(), "mm" | "in" | "min" | "nom" | "max" | "typ" | "dim" | "ref" | "bsc" | "no" | "of")
}

/// A cell of a dimension row: a number, or a dash for "not given".
fn cell(t: &str) -> Option<Option<f64>> {
    let n = normalize(t);
    if !n.is_empty() && n.chars().all(|c| c == '-' || c == '—') {
        return Some(None);
    }
    n.trim_start_matches('.').parse::<f64>().ok().filter(|_| n.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')).map(|_| n.parse::<f64>().ok())
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

/// The dimension table of a page: at least three rows of a symbol and one to three numbers. With both a millimetre and an inch
/// heading the numbers on the millimetre side are taken; with inches only, they are converted.
pub fn dimension_table(page: usize, words: &[Word]) -> Option<DimTable> {
    let is = |w: &Word, names: &[&str]| names.contains(&normalize(&w.text).trim_matches(|c: char| !c.is_alphabetic()));
    let heading_x = |names: &[&str]| words.iter().find(|w| is(w, names)).map(|w| (w.rect.x0 + w.rect.x1) / 2.0);
    const MM: &[&str] = &["millimeters", "millimetres", "mm"];
    const IN: &[&str] = &["inches", "inch"];
    // the column headings: a millimetre and an inch heading on one line (not "in inches and millimeters" in a sentence: there the
    // words are close together), else whichever there is
    let between = |a: &Word, b: &Word| {
        let (l, r) = if a.rect.x0 < b.rect.x0 { (a.rect.x1, b.rect.x0) } else { (b.rect.x1, a.rect.x0) };
        words.iter().any(|w| (w.baseline() - a.baseline()).abs() < 2.0 && w.rect.x0 >= l && w.rect.x1 <= r)
    };
    let pair = words.iter().filter(|m| is(m, MM)).find_map(|m| words.iter().find(|i| is(i, IN) && (i.baseline() - m.baseline()).abs() < 2.0 && !between(m, i)).map(|i| ((m.rect.x0 + m.rect.x1) / 2.0, (i.rect.x0 + i.rect.x1) / 2.0)));
    let (mm_x, in_x) = match pair {
        Some((m, i)) => (Some(m), Some(i)),
        None => (heading_x(MM), heading_x(IN)),
    };
    let (mm_x, in_x) = if pair.is_none() && mm_x.is_some() && in_x.is_some() { (mm_x, None) } else { (mm_x, in_x) };
    // a dimension table has a heading of its unit or of its symbol column
    if mm_x.is_none() && in_x.is_none() && heading_x(&["dim", "dimension", "dimensions"]).is_none() {
        return None;
    }
    let from_inch = in_x.is_some() && mm_x.is_none();
    // with both headings: only the numbers on the millimetre side of the line between them
    let keep = |x: f32| match (mm_x, in_x) {
        (Some(m), Some(i)) => (x < (m + i) / 2.0) == (m < i),
        _ => true,
    };
    let mut found: Vec<(f32, String, Dim)> = vec![];
    for line in lines(words) {
        // the symbol: the first symbol-like word with only numbers after it (labels of the drawing may stand left of the table)
        let Some(k) = (0..line.len()).find(|&k| is_symbol(line[k].text.trim()) && line.len() > k + 1 && line[k + 1..].iter().all(|w| cell(&w.text).is_some() || matches!(normalize(&w.text).as_str(), "bsc" | "ref" | "typ" | "typ."))) else { continue };
        let sym = line[k].text.trim();
        let cells: Vec<Option<f64>> = line[k + 1..].iter().filter(|w| keep((w.rect.x0 + w.rect.x1) / 2.0)).filter_map(|w| cell(&w.text)).collect();
        if cells.is_empty() || cells.len() > 3 || cells.iter().all(Option::is_none) {
            continue;
        }
        let scale = if from_inch { 25.4 } else { 1.0 };
        let v = |c: Option<f64>| c.map(|x| x * scale);
        let dim = match cells.as_slice() {
            [n] => Dim { min: None, nom: v(*n), max: None },
            [a, b] => Dim { min: v(*a), nom: None, max: v(*b) },
            [a, n, b] => Dim { min: v(*a), nom: v(*n), max: v(*b) },
            _ => continue,
        };
        found.push((line[k].rect.x0, sym.to_string(), dim));
    }
    // the table's symbols stand in one column: keep the biggest group of rows whose symbols start at the same place
    let column = found.iter().map(|f| f.0).max_by_key(|x| found.iter().filter(|g| (g.0 - x).abs() < 4.0).count())?;
    let mut rows: Vec<(String, Dim)> = vec![];
    for (x, sym, dim) in found {
        if (x - column).abs() < 4.0 && !rows.iter().any(|r| r.0 == sym) {
            rows.push((sym, dim));
        }
    }
    (rows.len() >= 3).then_some(DimTable { page, from_inch, rows })
}

/// The part of a page above its foot (the copyright and links at the bottom).
const FOOT: f32 = 0.92;

/// How many lines below a pin heading a table may start (its title and header rows).
const MAX_SKIP: usize = 8;

/// A pin table found on its own: the page, the area read and its columns, and the pins.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundPins {
    pub page: usize,
    pub area: Rect,
    pub num_col: usize,
    pub name_col: usize,
    pub pins: Vec<(String, String)>,
}

impl Datasheet {
    /// The package names the data sheet writes, the likeliest first.
    pub fn package_names(&self) -> Vec<PackageName> {
        let pages: Vec<&[Word]> = (0..self.page_count()).map(|p| self.words(p)).collect();
        package_names(&pages)
    }

    /// The dimension table of the package drawing (the first page with a package or land pattern heading that has one; else any page).
    pub fn dimension_table(&self) -> Option<DimTable> {
        let mut pages = self.dimension_pages();
        pages.extend((0..self.page_count()).filter(|p| !pages.contains(p)).collect::<Vec<_>>());
        pages.into_iter().find_map(|p| dimension_table(p, self.words(p)))
    }

    /// The pin table under a "Pin Configuration" heading: from each such heading down to the page's end, read as a table. Of the first
    /// page that gives a table whose pin numbers run from 1 nearly without gaps (`plausible_pins`; a data sheet of several variants
    /// shows the first part number's table first), the candidate with the fewest names twice (a pin drawing read as a table mixes its
    /// two sides up: "GND" twice), then the most pins.
    pub fn find_pin_table(&self) -> Option<FoundPins> {
        let queries = Heading::PinConfig.queries();
        let twice = |pins: &[(String, String)]| pins.iter().enumerate().filter(|(i, p)| pins[..*i].iter().any(|q| q.1 == p.1)).count();
        for page in 0..self.page_count() {
            let Some((w, h)) = self.page_size(page) else { continue };
            let mut found: Vec<FoundPins> = vec![];
            for hit in self.search_page_any(page, &queries).into_iter().filter(|h| !h.in_contents) {
                // from the heading, and from each of the next lines: a table's header rows ("NAME  NS, PW, J") can span its columns
                // and run them together, so starting below them reads the columns apart
                let mut tops: Vec<f32> = self.words(page).iter().filter(|w| w.rect.y0 > hit.rect.y1).map(|w| w.rect.y0).collect();
                tops.sort_by(f32::total_cmp);
                tops.dedup_by(|a, b| (*a - *b).abs() < 3.0);
                tops.truncate(MAX_SKIP);
                for y0 in std::iter::once(hit.rect.y1).chain(tops.into_iter().map(|y| y - 0.5)) {
                    // the page's foot (copyright, links) spans the page and would run every column together: left out
                    let area = Rect { x0: 0.0, y0, x1: w, y1: h * FOOT };
                    let table = self.table_in(page, area);
                    let Some((num_col, name_col)) = crate::guess_columns(&table) else { continue };
                    let pins = crate::pins_from(&table, num_col, name_col);
                    if plausible_pins(&pins) {
                        found.push(FoundPins { page, area, num_col, name_col, pins });
                    }
                }
            }
            if let Some(best) = found.into_iter().min_by(|a, b| twice(&a.pins).cmp(&twice(&b.pins)).then(b.pins.len().cmp(&a.pins.len()))) {
                return Some(best);
            }
        }
        None
    }
}

/// Pins that look like a part's pins: numbered 1, 2, 3 … with at most a quarter of the numbers up to the highest missing, and names
/// that are not single letters of a drawing's labels for most of them. Ball names (A1, B2) pass as they are.
pub fn plausible_pins(pins: &[(String, String)]) -> bool {
    if pins.len() < 2 {
        return false;
    }
    let nums: Vec<u32> = pins.iter().filter_map(|(n, _)| n.parse().ok()).collect();
    if nums.len() == pins.len() {
        let top = *nums.iter().max().unwrap_or(&0);
        if !nums.contains(&1) || nums.contains(&0) || top == 0 || (nums.len() as f64) < top as f64 * 0.75 {
            return false;
        }
    }
    let short = pins.iter().filter(|(_, name)| name.chars().count() < 2 && !matches!(name.as_str(), "A" | "K" | "G" | "D" | "S" | "B" | "C" | "E")).count();
    short * 2 < pins.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_that_run_from_one_pass_and_scattered_numbers_do_not() {
        let p = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert!(plausible_pins(&p(&[("1", "IN"), ("2", "GND"), ("3", "OUT")])));
        assert!(plausible_pins(&p(&[("1", "D"), ("2", "S"), ("3", "GND")])));
        assert!(!plausible_pins(&p(&[("3", "PD"), ("12", "R"), ("42", "R"), ("60", "VDSS")])), "numbers of a table of values");
        assert!(!plausible_pins(&p(&[("0", "CP"), ("1", "MR"), ("2", "CE"), ("5", "VCC"), ("6", "TC")])), "gaps");
        assert!(plausible_pins(&p(&[("A1", "VDD"), ("A2", "GND"), ("B1", "SDA")])), "balls");
    }

    #[test]
    fn package_names_are_read_in_the_ways_data_sheets_write_them() {
        assert_eq!(package_of("SOT−223"), Some(("sot-223", None)));
        assert_eq!(package_of("SOT-23-5"), Some(("sot-23", Some(5))));
        assert_eq!(package_of("SOIC8"), Some(("soic", Some(8))));
        assert_eq!(package_of("8-SOIC"), Some(("soic", Some(8))));
        assert_eq!(package_of("(TSSOP-14)"), Some(("tssop", Some(14))));
        assert_eq!(package_of("SOT-89"), Some(("sot-89", None)));
        assert_eq!(package_of("SOT23"), Some(("sot-23", None)));
        assert_eq!(package_of("TO220"), Some(("to-220", None)));
        assert_eq!(package_of("SOT223"), Some(("sot-223", None)));
        assert_eq!(package_of("SOLDER"), None);
        assert_eq!(package_of("smart"), None);
    }

    #[test]
    fn the_most_written_package_comes_first() {
        let w = |t: &str| Word::for_test(t, 0.0, 0.0, 10.0, 8.0);
        let p0 = vec![w("NDT3055L"), w("SOT-223")];
        let p1 = vec![w("SOT-23"), w("SOT-23"), w("SOT-223")];
        let names = package_names(&[&p0, &p1]);
        // SOT-223: twice on the first page (2 × 1... counted 2) + once = 3; SOT-23: 2
        assert_eq!(names[0].name, "SOT-223");
        assert_eq!((names[0].count, names[0].page), (3, 0));
        assert_eq!(names[1].name, "SOT-23");
    }

    /// A dimension table as onsemi prints it: millimetres only, dashes for what is not given, one number for the pitch.
    #[test]
    fn a_dimension_table_in_millimetres() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let mut words = vec![w("MILLIMETERS", 660.0, 460.0), w("DIM", 640.0, 475.0), w("MIN.", 670.0, 475.0), w("NOM.", 700.0, 475.0), w("MAX.", 730.0, 475.0)];
        for (k, (s, a, b, c)) in [("A", "---", "---", "1.80"), ("b", "0.60", "0.74", "0.88"), ("D", "6.30", "6.50", "6.70"), ("E1", "3.30", "3.50", "3.70")].iter().enumerate() {
            let y = 490.0 + 12.0 * k as f32;
            words.extend([w(s, 645.0, y), w(a, 670.0, y), w(b, 700.0, y), w(c, 730.0, y)]);
        }
        words.extend([w("e", 645.0, 540.0), w("2.30", 690.0, 540.0), w("BSC", 712.0, 540.0)]);
        let t = dimension_table(4, &words).unwrap();
        assert!(!t.from_inch);
        assert_eq!(t.get("A"), Some(Dim { min: None, nom: None, max: Some(1.80) }));
        assert_eq!(t.get("D").unwrap().typical(), Some(6.50));
        assert_eq!(t.get("e").unwrap().typical(), Some(2.30));
        assert_eq!(t.get("E1").unwrap().range(), Some((3.30, 3.70)));
        assert!(t.get("E").is_none(), "E1 is not E");
    }

    /// Millimetres and inches side by side: the millimetre columns are read. Inches alone are converted.
    #[test]
    fn a_dimension_table_with_inches() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let both = |mm_left: bool| {
            let (mx, ix) = if mm_left { (100.0, 250.0) } else { (250.0, 100.0) };
            let mut words = vec![w("MILLIMETERS", mx, 10.0), w("INCHES", ix, 10.0)];
            for (k, (s, mm, inch)) in [("A", ["1.35", "1.75"], ["0.053", "0.069"]), ("D", ["4.80", "5.00"], ["0.189", "0.197"]), ("E", ["5.80", "6.20"], ["0.228", "0.244"])].iter().enumerate() {
                let y = 30.0 + 12.0 * k as f32;
                words.push(w(s, 40.0, y));
                for (j, v) in mm.iter().enumerate() {
                    words.push(w(v, mx + 30.0 * j as f32, y));
                }
                for (j, v) in inch.iter().enumerate() {
                    words.push(w(v, ix + 30.0 * j as f32, y));
                }
            }
            words
        };
        for mm_left in [true, false] {
            let t = dimension_table(0, &both(mm_left)).unwrap();
            assert_eq!(t.get("D").unwrap().range(), Some((4.80, 5.00)), "mm on the left: {mm_left}");
        }
        let words = vec![w("INCHES", 100.0, 10.0), w("A", 40.0, 30.0), w("0.053", 100.0, 30.0), w("0.069", 130.0, 30.0), w("D", 40.0, 42.0), w("0.189", 100.0, 42.0), w("0.197", 130.0, 42.0), w("E", 40.0, 54.0), w("0.228", 100.0, 54.0), w("0.244", 130.0, 54.0)];
        let t = dimension_table(0, &words).unwrap();
        assert!(t.from_inch);
        let (lo, hi) = t.get("D").unwrap().range().unwrap();
        assert!((lo - 4.8006).abs() < 1e-9 && (hi - 5.0038).abs() < 1e-9);
    }
}

#[cfg(test)]
mod real_pdf {
    use crate::Datasheet;

    fn open(name: &str) -> Option<Datasheet> {
        Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(name)).ok()
    }

    /// onsemi's NDT3055L: an SOT-223 (its dimension table is drawn as lines, not text, so there is none to read; skipped without the file).
    #[test]
    fn ndt3055l_is_an_sot223() {
        let Some(ds) = open("NDT3055L-D.pdf") else { return };
        assert_eq!(ds.package_names()[0].name, "SOT-223");
        assert!(ds.dimension_table().is_none(), "{:?}", ds.dimension_table());
    }

    /// TI's pin tables: under "Pin Functions", below a header that spans the columns, above the page's foot; not the pin drawing above
    /// it, which reads as a table with its two sides mixed up (skipped without the files).
    #[test]
    fn ti_pin_tables_are_found() {
        if let Some(ds) = open("sn74hc14.pdf") {
            let p = ds.find_pin_table().unwrap();
            assert_eq!(p.page, 2);
            let lines = crate::pin_lines(&p.pins);
            assert!(lines.contains("7 GND
8 4Y
9 4A") && lines.ends_with("14 VCC"), "{lines}");
        }
        if let Some(ds) = open("lm393.pdf") {
            let p = ds.find_pin_table().unwrap();
            assert_eq!(crate::pin_lines(&p.pins), "1 1OUT
2 1IN-
3 1IN+
4 GND
5 2IN+
6 2IN-
7 2OUT
8 VCC");
        }
    }

    /// Vishay's MPM: inches on the left, millimetres on the right, labels of the drawing on the same lines (skipped without the file).
    #[test]
    fn mpm_dimension_table_in_millimetres() {
        let Some(ds) = open("mpm.pdf") else { return };
        let t = ds.dimension_table().unwrap();
        assert_eq!(t.page, 1);
        assert!(!t.from_inch);
        assert_eq!(t.get("A").unwrap().range(), Some((0.79, 1.02)));
        assert_eq!(t.get("B").unwrap().range(), Some((2.67, 3.05)));
        assert_eq!(t.get("W").unwrap().range(), Some((0.38, 0.54)));
    }
}

/// What the draft finds in the data sheets of `microohm-meter/` (`cargo test -p datasheet print_found -- --ignored --nocapture`), for
/// looking over after a change.
#[cfg(test)]
mod real_pins {
    #[test]
    #[ignore]
    fn print_found() {
        for f in ["sn74hc14.pdf", "lm393.pdf", "AO3400A_ds.pdf", "NDT3055L-D.pdf", "lm317l.pdf", "tmux1101.pdf", "ref5025-ep.pdf", "cd74hc4017.pdf", "ada4522-1_4522-2_4522-4.pdf"] {
            let Ok(ds) = crate::Datasheet::open_file(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter").join(f)) else { continue };
            let p = ds.find_pin_table();
            eprintln!("{f}: names {:?} pkg {:?} pins {:?} dims {:?}", ds.name_candidates().first(), ds.package_names().iter().take(3).map(|p| (p.name.clone(), p.count)).collect::<Vec<_>>(), p.map(|p| (p.page + 1, p.pins)), ds.dimension_table().map(|t| (t.page + 1, t.rows.len())));
        }
    }
}
