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

/// Every `name = value` of a conditions text or a graph's label ("VIN = 5V, IL = 500mA" → [("VIN", 5), ("IL", 0.5)]), in SI
/// units: the word before each "=" and its value by [`condition`].
pub fn conditions_in(text: &str) -> Vec<(String, f64)> {
    // ("·" joins the conditions borrowed from another line: "IOUT = 1mA · BW = 10Hz to 100kHz")
    let text = text.replace('·', ",");
    let text = text.as_str();
    let mut out: Vec<(String, f64)> = vec![];
    for (i, _) in text.match_indices('=') {
        let before = text[..i].trim_end();
        let name: String = before.rsplit(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ';').next().unwrap_or("").chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
        if name.is_empty() || !name.starts_with(|c: char| c.is_alphabetic()) || out.iter().any(|o| o.0 == name) {
            continue;
        }
        if let Some(v) = condition(text, &name) {
            out.push((name, v));
        }
    }
    out
}

/// The value of `name=…` in a conditions text, in SI units and as a magnitude.
pub fn condition(text: &str, name: &str) -> Option<f64> {
    let key = normalize(name).replace(' ', "");
    // "VDS=VGS ID=250µA", "VGS= ±12V", "(IC = 2.0 mA, VCE = 5.0 V)": a new part starts at a word holding an '='
    // (or at the word before a lone "=", its name); the words after it up to the next one are its value
    // (a closing bracket or a comma ends a value: "kHz) BC547B" is not one unit)
    let raw: Vec<&str> = text.split_inclusive(|c: char| c == ',' || c == ';' || c.is_whitespace()).map(str::trim).filter(|s| !s.is_empty()).collect();
    let toks: Vec<(&str, bool)> = raw.iter().map(|s| (s.trim_end_matches([',', ';']).trim_matches(['(', ')']), s.ends_with([')', ',', ';']))).filter(|t| !t.0.is_empty()).collect();
    let mut parts: Vec<String> = vec![];
    let mut closed = true;
    for (i, (tok, ends)) in toks.iter().enumerate() {
        let names_next = toks.get(i + 1).is_some_and(|n| n.0.starts_with('='));
        match parts.last_mut() {
            Some(last) if !closed && !names_next && (!tok.contains('=') || last.ends_with('=') || tok.starts_with('=')) => last.push_str(tok),
            _ => parts.push(tok.to_string()),
        }
        closed = *ends;
    }
    for (i, part) in parts.iter().enumerate() {
        let Some((k, v)) = part.split_once('=') else { continue };
        if normalize(k) != key {
            continue;
        }
        // "IF = IR = 10 mA": a name with no value of its own has the next one's
        let v = if v.trim().is_empty() { parts.get(i + 1).and_then(|p| p.split_once('=')).map_or("", |n| n.1) } else { v };
        let v = v.trim().trim_start_matches(['±', '-', '+', '\u{2212}']);
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
    // onsemi's "mAdc", "Vdc": a direct value's unit
    let u = u.strip_suffix("dc").or_else(|| u.strip_suffix("DC")).filter(|p| !p.is_empty()).unwrap_or(&u);
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
#[derive(Clone)]
struct Header {
    y: f32,
    min: f32,
    /// None: the table has only min and max (Fairchild's small-signal transistors)
    typ: Option<f32>,
    max: f32,
    unit: Option<f32>,
    /// Left edge of the conditions column, when headed ("Conditions", "Test Conditions", "条件").
    conditions: Option<f32>,
    /// The symbol column's heading (x0, x1), when there is one ("Symbol", "記号"): the symbols are under it, which
    /// may be right of the parameter names and the conditions (onsemi's small-signal transistors).
    symbol: Option<(f32, f32)>,
    /// Where the conditions end: the leftmost min column of the header line. A table with a group of min / typ / max
    /// for each grade side by side (onsemi's MC7800: MC78xxB and MC78xxC) is read in one group; the cells of the
    /// other ("− 0.2 −") are not conditions.
    cond_end: f32,
    /// Every group of min / typ / max on the header line, left to right, with the part numbers printed over it (onsemi's
    /// MC7800: "MC7805B, NCV7805B" over the first, "MC7805C" over the second; "MC7805AB/MC7805AC" over a page's only one).
    /// The columns above (`min`, `typ`, `max`) are one of them, the one read when no part number is asked for.
    groups: Vec<Group>,
}

/// One group of min / typ / max columns of a header line, and the part numbers (grades) printed over it.
#[derive(Clone, Debug)]
struct Group {
    min: f32,
    typ: Option<f32>,
    max: f32,
    labels: Vec<String>,
}

/// A word printed over a group of columns as a part number or grade ("MC7805B,", "NCV7805B", "MC7805AB/MC7805AC" → its
/// pieces): letters and digits, both, at least four.
fn grade_labels(t: &str) -> Vec<String> {
    t.split(['/', ',', ';'])
        .map(|p| p.trim().trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_ascii_uppercase())
        .filter(|p| p.len() >= 4 && p.chars().all(|c| c.is_ascii_alphanumeric()) && p.chars().any(|c| c.is_ascii_digit()) && p.chars().any(|c| c.is_ascii_alphabetic()))
        .collect()
}

/// The longest of `labels` that `part` starts with (its grade: "MC7805C" of "MC7805CTG"), if any.
fn grade_in<'a>(labels: impl Iterator<Item = &'a String>, part: &str) -> Option<&'a String> {
    let part = part.trim().to_ascii_uppercase();
    labels.filter(|l| part.starts_with(l.as_str())).max_by_key(|l| l.len())
}

impl Header {
    /// The header with the columns of the group whose part number `part` is of. `None` when the table names its groups'
    /// part numbers and `part` is of none of them (another grade's table: MC7805AB's page for MC7805CTG); the header as it
    /// is when it names none.
    fn for_part(&self, part: Option<&str>) -> Option<Header> {
        let Some(part) = part else { return Some(self.clone()) };
        if self.groups.iter().all(|g| g.labels.is_empty()) {
            return Some(self.clone());
        }
        let g = self.groups.iter().filter_map(|g| grade_in(g.labels.iter(), part).map(|l| (g, l.len()))).max_by_key(|x| x.1)?.0;
        Some(Header { min: g.min, typ: g.typ, max: g.max, ..self.clone() })
    }
}

/// The grade of the part number `part` among `grades` (`grades`): the longest that `part` starts with.
pub fn grade_of(grades: &[String], part: &str) -> Option<String> {
    grade_in(grades.iter(), part).cloned()
}

/// The part numbers (grades) a page's tables print over their groups of min / typ / max columns.
pub fn grades(words: &[Word]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in headers(words).into_iter().flat_map(|h| h.groups).flat_map(|g| g.labels) {
        if !out.contains(&l) {
            out.push(l);
        }
    }
    out
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
    // the line is found by its max (every table has one); min must be on it, typ may be
    for w in words.iter().filter(|w| is(w, &["max", "maximum", "最大"])) {
        let y = centre(&w.rect).1;
        let same = |names: &[&str]| words.iter().filter(|o| (centre(&o.rect).1 - y).abs() < 4.0 && is(o, names)).map(|o| centre(&o.rect).0).min_by(|a, b| (a - centre(&w.rect).0).abs().total_cmp(&(b - centre(&w.rect).0).abs()));
        let Some(min) = same(&["min", "minimum", "最小"]).filter(|m| *m < centre(&w.rect).0) else { continue };
        let max = centre(&w.rect).0;
        let typ = same(&["typ", "typical", "標準", "代表値"]).filter(|t| min < *t && *t < max);
        // the other headings on the line, or on the line above when the header takes two ("Parameter Symbol Unit
        // Conditions" over "Limits" over "Min. Typ. Max.", ROHM's Japanese data sheets): up to 12 points above
        let on_line = |o: &Word| (centre(&o.rect).1 - y).abs() < 4.0;
        let above = |o: &Word| (-12.0..=-4.0).contains(&(centre(&o.rect).1 - y));
        let heading = |names: &[&str]| words.iter().filter(|o| on_line(o) && is(o, names)).map(|o| centre(&o.rect).0).next().or_else(|| words.iter().filter(|o| above(o) && is(o, names)).map(|o| centre(&o.rect).0).next());
        let unit = same(&["units", "unit", "単位"]).or_else(|| heading(&["units", "unit", "単位"]));
        let cond = |o: &Word| normalize(&o.text).contains("condition") || o.text.contains("条件");
        let first_x = |f: &dyn Fn(&Word) -> bool| words.iter().filter(|o| f(o) && cond(o)).map(|o| o.rect.x0).fold(None, |a: Option<f32>, x| Some(a.map_or(x, |a| a.min(x))));
        let conditions = first_x(&on_line).or_else(|| first_x(&above));
        let symbol = words.iter().find(|o| on_line(o) && is(o, &["symbol", "sym", "記号"])).or_else(|| words.iter().find(|o| above(o) && is(o, &["symbol", "sym", "記号"]))).map(|o| (o.rect.x0, o.rect.x1));
        if !out.iter().any(|h: &Header| (h.y - y).abs() < 4.0) {
            out.push(Header { y, min, typ, max, unit, conditions, symbol, cond_end: min, groups: vec![] });
        }
    }
    let mins: Vec<(f32, f32)> = words.iter().filter(|w| is(w, &["min", "minimum", "最小"])).map(|w| centre(&w.rect)).collect();
    for h in &mut out {
        h.cond_end = mins.iter().filter(|(_, y)| (y - h.y).abs() < 4.0).map(|&(x, _)| x).fold(h.min, f32::min);
        // the groups: each max of the line with the nearest min left of it and a typ between
        let on = |names: &[&str]| -> Vec<f32> {
            let mut v: Vec<f32> = words.iter().filter(|o| (centre(&o.rect).1 - h.y).abs() < 4.0 && is(o, names)).map(|o| centre(&o.rect).0).collect();
            v.sort_by(f32::total_cmp);
            v
        };
        let (lo, mid) = (on(&["min", "minimum", "最小"]), on(&["typ", "typical", "標準", "代表値"]));
        for max in on(&["max", "maximum", "最大"]) {
            let Some(min) = lo.iter().copied().filter(|m| *m < max).fold(None, |a: Option<f32>, m| Some(a.map_or(m, |a| a.max(m)))) else { continue };
            let typ = mid.iter().copied().find(|t| min < *t && *t < max);
            h.groups.push(Group { min, typ, max, labels: vec![] });
        }
        // the part numbers printed over them (up to 20 points above the line), each over the group it stands nearest
        for w in words.iter().filter(|w| (4.0..=20.0).contains(&(h.y - centre(&w.rect).1))) {
            let labels = grade_labels(&w.text);
            let x = centre(&w.rect).0;
            let near = h.groups.iter_mut().filter(|g| x > g.min - 25.0 && x < g.max + 25.0).min_by(|a, b| (x - (a.min + a.max) / 2.0).abs().total_cmp(&(x - (b.min + b.max) / 2.0).abs()));
            if let Some(g) = near {
                g.labels.extend(labels);
            }
        }
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    out
}

/// A number in a cell ("1.05", "−0.9", "–3" with an en dash as TI's LP5907 prints it; "±12" is not a value), as printed.
fn number(t: &str) -> Option<f64> {
    let t = t.trim().replace(['\u{2212}', '\u{2013}'], "-");
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

/// The symbol's key for matching: no spaces, no brackets ("V GS(th)" -> "VGSth"), lower case unless `case`.
/// (Japanese data sheets write the delta as "⊿", the right triangle: "⊿VO" is ΔVO, not VO.)
fn key(s: &str, case: bool) -> String {
    let t: String = s.chars().map(|c| if c == '⊿' { 'Δ' } else { c }).filter(|c| c.is_alphanumeric()).collect();
    if case { t } else { normalize(&t) }
}

/// The rows of `symbol` on a page. The case is kept when the page has the symbol in that case (hFE, the DC gain,
/// and hfe, the small-signal one, are different rows); otherwise it is ignored (VGS(th), VGS(TH)).
pub fn find(page: usize, words: &[Word], symbol: &str) -> Vec<SpecValue> {
    find_for(page, words, symbol, None)
}

/// `find` for the part number `part`: a table with a group of min / typ / max for each grade side by side (onsemi's MC7800:
/// MC78xxB and MC78xxC) is read in the group whose part number `part` is of, and a table that names the part numbers of its
/// groups but none of `part`'s is left out (another grade's page). Tables that name none are read as `find` reads them.
pub fn find_for(page: usize, words: &[Word], symbol: &str, part: Option<&str>) -> Vec<SpecValue> {
    let words = &*relative_to_output(words);
    // a symbol as written, then in any case
    let exact = find_case(page, words, symbol, true, false, part);
    if exact.is_empty() { find_case(page, words, symbol, false, false, part) } else { exact }
}

/// The lines whose parameter's name starts with `name` under a symbol shared by several (TI's LP5907: "Line regulation" and
/// "Load regulation" under one ΔVOUT). For a data sheet where `find` finds no row of that symbol on any page: a page's
/// "Output current" is not another page's "Output" symbol (UA78L).
pub fn find_by_name(page: usize, words: &[Word], name: &str) -> Vec<SpecValue> {
    find_by_name_for(page, words, name, None)
}

/// `find_by_name` for the part number `part` (see `find_for`).
pub fn find_by_name_for(page: usize, words: &[Word], name: &str, part: Option<&str>) -> Vec<SpecValue> {
    find_case(page, &relative_to_output(words), name, true, true, part)
}

/// The output voltage's limits written as fractions of the nominal (ROHM's fixed-output regulators: "VO×0.99 VO VO×1.01"
/// over min, typ and max, "V" as the unit) as plain numbers: 0.99, 1 and 1.01, the line's unit "×VO" (the fractions of the
/// output, for the caller to multiply by the part's voltage).
fn relative_to_output(words: &[Word]) -> std::borrow::Cow<'_, [Word]> {
    let factor = |t: &str| {
        let (name, k) = t.trim().split_once('×')?;
        matches!(name, "VO" | "VOUT" | "Vo" | "Vout").then_some(())?;
        k.parse::<f64>().ok().filter(|k| (0.5..2.0).contains(k))
    };
    let lines: Vec<f32> = words.iter().filter(|w| factor(&w.text).is_some()).map(|w| w.baseline()).collect();
    if lines.is_empty() {
        return std::borrow::Cow::Borrowed(words);
    }
    let mut out = words.to_vec();
    for y in lines {
        let on: Vec<usize> = (0..out.len()).filter(|&i| (out[i].baseline() - y).abs() < 2.0).collect();
        let fx: Vec<usize> = on.iter().copied().filter(|&i| factor(&out[i].text).is_some()).collect();
        let (lo, hi) = (fx.iter().map(|&i| out[i].rect.x0).fold(f32::MAX, f32::min), fx.iter().map(|&i| out[i].rect.x1).fold(f32::MIN, f32::max));
        let renamed = |w: &Word, t: &str| Word::on_line(t, w.rect, w.baseline());
        for &i in &on {
            let w = &out[i];
            let t = w.text.trim();
            let new = if let Some(k) = factor(t) {
                Some(format!("{k}"))
            } else if matches!(t, "VO" | "VOUT" | "Vo" | "Vout") && w.rect.x0 > lo && w.rect.x1 < hi {
                // the nominal between the two limits
                Some("1".to_string())
            } else if t == "V" && w.rect.x0 >= hi {
                Some("×VO".to_string())
            } else {
                None
            };
            if let Some(n) = new {
                out[i] = renamed(w, &n);
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

fn find_case(page: usize, words: &[Word], symbol: &str, case: bool, by_name: bool, part: Option<&str>) -> Vec<SpecValue> {
    let want = key(symbol, case);
    if want.is_empty() {
        return vec![];
    }
    let heads = headers(words);
    let mut out = vec![];
    for h in &heads {
        // the table runs from its header down to the next header (or the page's end)
        let bottom = heads.iter().map(|o| o.y).find(|&y| y > h.y + 1.0).unwrap_or(f32::MAX);
        // the columns of the part's grade, or none of this table's
        let Some(h) = h.for_part(part) else { continue };
        let h = &h;
        let body: Vec<&Word> = words.iter().filter(|w| centre(&w.rect).1 > h.y + 3.0 && centre(&w.rect).1 < bottom).collect();
        let first_x = body.iter().map(|w| w.rect.x0).fold(f32::MAX, f32::min);
        // the conditions: their own column when headed; with a symbol column right of the text, everything left of
        // the min column (the parameter's name, its conditions in brackets, a grade "BC547B"); else a guess
        // a symbol column right of the text: each row starts at its symbol's line (the parameter's name is there
        // too) and goes down to the next symbol
        let symbol_on_right = h.symbol.is_some_and(|s| s.0 > first_x + 60.0);
        // (a "Conditions" heading may be centred over its column: the column starts up to 40 points left of it)
        // or further, where its conditions start: a name just before an "=" (TI's LP5907: "f = 100Hz", "IOUT = 20mA" from
        // 46 points left of "TEST CONDITIONS")
        let starts = |c: f32| {
            body.iter()
                .filter(|w| w.rect.x0 > c - 90.0 && w.rect.x0 < c - 40.0 && w.text.chars().next().is_some_and(|ch| ch.is_alphabetic()))
                .filter(|w| body.iter().any(|e| e.text.starts_with('=') && e.rect.x0 > w.rect.x0 && e.rect.x0 - w.rect.x1 < 25.0 && (centre(&e.rect).1 - centre(&w.rect).1).abs() < 4.0))
                .map(|w| w.rect.x0)
                .fold(c - 40.0, f32::min)
        };
        let left = h.conditions.map(|c| starts(c).max(first_x)).unwrap_or(if symbol_on_right { first_x } else { h.min - 120.0 });
        // symbols: under the symbol heading, or the first word of a line in the table's first column (left
        // aligned; the parameter's name follows)
        let under = |w: &Word| match h.symbol {
            Some((s0, s1)) => centre(&w.rect).0 > s0 - 20.0 && centre(&w.rect).0 < s1 + 20.0 && w.rect.x1 < h.min - 5.0,
            None => w.rect.x0 < first_x + 25.0 && w.rect.x1 < left,
        };
        let mut symbols: Vec<&&Word> = body.iter().filter(|w| under(w) && !w.text.trim().is_empty()).collect();
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
        // (a "±40" on the symbol's own line is its value, only not read: LM358's ISC does not take the sink current's
        // lines above it)
        let signed = |sy: f32| body.iter().any(|w| (centre(&w.rect).1 - sy).abs() < 3.0 && w.text.trim().starts_with('±') && column(h, centre(&w.rect).0).is_some());
        let bare: Vec<bool> = sym_y.iter().map(|&sy| !ys.iter().any(|&y| (y - sy).abs() < 3.0) && !signed(sy)).collect();
        let owner = |y: f32| -> Option<usize> {
            if let Some(i) = sym_y.iter().position(|&sy| (sy - y).abs() < 3.0) {
                return Some(i);
            }
            if symbol_on_right {
                return (0..sym_y.len()).filter(|&i| sym_y[i] < y).max_by(|&a, &b| sym_y[a].total_cmp(&sym_y[b]));
            }
            let nearest = |only_bare: bool| (0..sym_y.len()).filter(|&i| !only_bare || bare[i]).min_by(|&a, &b| (sym_y[a] - y).abs().total_cmp(&(sym_y[b] - y).abs()));
            // a line as near to the symbol above as to the one below (each printed in the middle of its lines, the rows
            // touching: LM358's rail swing VO and output current IO, 28.9 points either way): the one whose lines reach as
            // far on its other side, a row being about as tall above its symbol as below
            let close: Vec<usize> = (0..sym_y.len()).filter(|&i| bare[i] && (sym_y[i] - y).abs() < 30.0).collect();
            if let [a, b] = close[..]
                && ((sym_y[a] - y).abs() - (sym_y[b] - y).abs()).abs() < 3.0
            {
                // how far the symbol's lines go on the side away from `y`, up to the next symbol
                let reach = |i: usize| {
                    let up = y > sym_y[i];
                    let limit = sym_y.iter().copied().filter(|&s| if up { s < sym_y[i] } else { s > sym_y[i] }).fold(if up { f32::MIN } else { f32::MAX }, |m, s| if up { m.max(s) } else { m.min(s) });
                    ys.iter().filter(|&&o| if up { o < sym_y[i] - 1.0 && o > limit + 1.0 } else { o > sym_y[i] + 1.0 && o < limit - 1.0 }).map(|o| (o - sym_y[i]).abs()).fold(0.0, f32::max)
                };
                let fits: Vec<usize> = [a, b].into_iter().filter(|&i| (sym_y[i] - y).abs() <= reach(i) + 4.0).collect();
                if let [one] = fits[..] {
                    return Some(one);
                }
            }
            nearest(true).filter(|&i| (sym_y[i] - y).abs() < 30.0).or_else(|| nearest(false))
        };
        for (si, s) in symbols.iter().enumerate().filter(|(_, w)| !by_name && key(&symbol_text(&body, w), case) == want) {
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
                let conditions = conditions_near(words, h, left, y, (top, end), &symbols, symbol_on_right);
                let unit = unit_near(words, &band, h, y);
                out.push(SpecValue { page, symbol: symbol_text(&body, s), conditions, min: v[0], typ: v[1], max: v[2], unit, rect: rect.unwrap_or(s.rect) });
            }
        }
        // no symbol of that name: a line whose parameter's name starts with it, under a symbol shared by several (TI's LP5907:
        // "Line regulation" and "Load regulation" under one ΔVOUT)
        if by_name {
            for &y in &ys {
                let Some(si) = owner(y) else { continue };
                // a line of its own symbol is named by it ("Output Short Circuit Current IOUT_SC", MCP1700), not by its first word
                if (sym_y[si] - y).abs() < 3.0 {
                    continue;
                }
                let line = line_at(words, y);
                let name = line.iter().filter(|w| w.rect.x1 < left - 2.0 && !symbols.iter().any(|s| std::ptr::eq(**s, **w)) && w.text.starts_with(|c: char| c.is_alphabetic())).min_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
                let Some(name) = name.filter(|n| key(&n.text, case) == want) else { continue };
                let mine: Vec<f32> = ys.iter().copied().filter(|&o| owner(o) == Some(si)).collect();
                let sy = sym_y[si];
                let (top, end) = (mine.iter().copied().fold(sy, f32::min) - 6.0, mine.iter().copied().fold(sy, f32::max) + 6.0);
                let band: Vec<&&Word> = body.iter().filter(|w| (top..end).contains(&centre(&w.rect).1)).collect();
                let (mut v, mut rect) = ([None, None, None], None::<Rect>);
                for w in &line {
                    if let (Some(x), Some(c)) = (number(&w.text), column(h, centre(&w.rect).0)) {
                        v[c] = Some(x);
                        rect = Some(rect.map_or(w.rect, |r| Rect { x0: r.x0.min(w.rect.x0), y0: r.y0.min(w.rect.y0), x1: r.x1.max(w.rect.x1), y1: r.y1.max(w.rect.y1) }));
                    }
                }
                // the conditions of this line or the nearest above it within the row (not the line of the next name)
                let conditions = conditions_near(words, h, left, y, (top, y + 3.0), &symbols, true);
                let unit = unit_near(words, &band, h, y);
                out.push(SpecValue { page, symbol: name.text.clone(), conditions, min: v[0], typ: v[1], max: v[2], unit, rect: rect.unwrap_or(name.rect) });
            }
        }
    }
    // some PDFs draw a line twice, a little apart (a fake bold): the same row is found twice
    let mut unique: Vec<SpecValue> = vec![];
    for v in out {
        if !unique.iter().any(|u| u.symbol == v.symbol && u.conditions == v.conditions && (u.min, u.typ, u.max) == (v.min, v.typ, v.max) && (u.rect.y0 - v.rect.y0).abs() < 3.0) {
            unique.push(v);
        }
    }
    unique
}

/// The symbol with the pieces printed right after it on its line ("V" + "GS(th)" as subscript).
fn symbol_text(body: &[&Word], s: &Word) -> String {
    let mut t = s.text.clone();
    let mut end = s.rect.x1;
    let mut next: Vec<&&Word> = body.iter().filter(|w| (centre(&w.rect).1 - centre(&s.rect).1).abs() < 5.0 && w.rect.x0 >= end - 0.5).collect();
    next.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    let mut rest = vec![];
    for w in next {
        if w.rect.x0 - end > 1.0 {
            rest.push(w);
            continue;
        }
        if !rest.is_empty() {
            break;
        }
        t.push_str(&w.text);
        end = w.rect.x1;
    }
    // a difference written apart ("VI − VO", onsemi's MC7800 dropout): a minus within 8 points, then a symbol within 8
    if let [dash, other, ..] = rest[..]
        && matches!(dash.text.trim(), "−" | "-" | "–")
        && dash.rect.x0 - end < 8.0
        && other.rect.x0 - dash.rect.x1 < 8.0
        && other.text.starts_with(|c: char| c.is_ascii_uppercase())
        && other.text.chars().count() <= 6
    {
        t.push('−');
        t.push_str(&other.text);
    }
    t
}

/// Which of min (0), typ (1), max (2) a number at `x` is under.
fn column(h: &Header, x: f32) -> Option<usize> {
    let cols = [Some(h.min), h.typ, Some(h.max)];
    let gap = match h.typ {
        Some(t) => (t - h.min).abs().min((h.max - t).abs()),
        None => (h.max - h.min).abs(),
    };
    let (i, best) = cols.iter().enumerate().filter_map(|(i, c)| c.map(|c| (i, (c - x).abs()))).min_by(|a, b| a.1.total_cmp(&b.1))?;
    (best < gap * 0.6).then_some(i)
}

/// The words of a line as text: words that touch (a subscript printed as its own word, "I" + "C") are joined
/// without a space ("IC = 10 mA").
fn joined(words: &[&Word]) -> String {
    let mut s = String::new();
    let mut end = f32::MIN;
    for w in words {
        if !s.is_empty() && w.rect.x0 - end > 0.8 {
            s.push(' ');
        }
        s.push_str(&w.text);
        end = w.rect.x1;
    }
    s
}

/// The conditions on the line `y` (words between the conditions column and the min column); when it has none,
/// the nearest line inside the row that has some, else the nearest line with an `=` within 30 points (a group of
/// rows printed once for all: Ciss, Coss, Crss).
/// With `downward` (a table whose rows start at their symbol), the conditions are borrowed only from above: a
/// condition line applies to the lines under it.
fn conditions_near(words: &[Word], h: &Header, left: f32, y: f32, (top, end): (f32, f32), symbols: &[&&Word], downward: bool) -> String {
    // a conditions column right of the values and the unit (ROHM's Japanese data sheets): the words of the line there
    // (its heading is centred over it: the column starts up to 40 points left of it, right of the unit)
    if let Some(c) = h.conditions.filter(|c| *c > h.max) {
        let from = (c - 40.0).max(h.unit.map_or(h.max, |u| u + 10.0));
        return joined(&line_at(words, y).into_iter().filter(|w| w.rect.x0 >= from).collect::<Vec<_>>());
    }
    let in_cond = |w: &Word| w.rect.x0 >= left - 2.0 && w.rect.x1 < h.cond_end - 8.0 && !symbols.iter().any(|s| std::ptr::eq(**s, w));
    let at = |y: f32| -> String { joined(&line_at(words, y).into_iter().filter(|w| in_cond(w)).collect::<Vec<_>>()) };
    // the conditions of a line only: from the word before the first '=' or bracket to the last word with an '=' or a
    // closing bracket and the number and unit after it
    let conditions_only = |y: f32| -> String {
        let text = joined(&line_at(words, y).into_iter().filter(|w| in_cond(w)).collect::<Vec<_>>());
        let l: Vec<&str> = text.split(' ').collect();
        let Some(last) = l.iter().rposition(|w| w.contains('=') || w.contains(')')) else { return String::new() };
        let mut end = last + 1;
        // the number and unit after it, and a range's end ("(VOUT(NOM) + 1V) to 5.5V", LP5907)
        // (a number and its unit may be one word: "10Hz", "5.5V,")
        // (not a part's name: "2N3903")
        let value = |t: &str| {
            let t = t.trim_end_matches(',');
            let digits = t.trim_start_matches(['-', '−']);
            let unit = digits.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
            number(t).is_some() || prefix(t).is_some() || (unit.len() < digits.len() && (unit.is_empty() || prefix(unit).is_some()))
        };
        while end < l.len() && end <= last + 4 && (value(l[end]) || (l[end] == "to" && l.get(end + 1).is_some_and(|n| value(n)))) {
            end += 1;
        }
        let first = l[..=last].iter().position(|w| w.contains('=') || w.starts_with('(')).unwrap_or(0).saturating_sub(1);
        l[first..end].join(" ")
    };
    let own = at(y);
    if own.contains('=') {
        // a range broken over two lines ("IO = 10mA to" over "1500mA, TJ = 25°C", TI's LM317): its end from the next line
        let toks: Vec<&str> = own.split(' ').collect();
        if let Some(i) = toks.iter().position(|t| *t == "to")
            && !toks.get(i + 1).is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        {
            let next = words
                .iter()
                .filter(|w| in_cond(w) && centre(&w.rect).1 > y + 4.0 && centre(&w.rect).1 < y + 16.0)
                .min_by(|a, b| (centre(&a.rect).1, a.rect.x0).partial_cmp(&(centre(&b.rect).1, b.rect.x0)).unwrap_or(std::cmp::Ordering::Equal))
                .filter(|w| w.text.starts_with(|c: char| c.is_ascii_digit()));
            if let Some(n) = next {
                let mut t: Vec<&str> = toks.clone();
                t.insert(i + 1, n.text.trim());
                return t.join(" ");
            }
        }
        // a condition shared by the row's lines, on a line of the row with no values (LP5907's noise: "BW = 10Hz to
        // 100kHz" by the parameter's name, "IOUT = 1mA" and "IOUT = 250mA" on the lines of the values)
        let line_y = |w: &Word| centre(&w.rect).1;
        // (only the line of the row's symbol: another line of the row may be a line of values in other columns, NDS7002A's
        // "TC = 125°C")
        let on_symbol = |c: f32| symbols.iter().any(|s| (centre(&s.rect).1 - c).abs() < 4.0);
        let mut shared: Vec<f32> = words
            .iter()
            .filter(|w| w.text.contains('=') && in_cond(w) && (top..end).contains(&line_y(w)) && (line_y(w) - y).abs() > 3.0 && on_symbol(line_y(w)))
            .map(line_y)
            .collect();
        shared.dedup_by(|a, b| (*a - *b).abs() < 3.0);
        // (a value may sit between the two lines of its conditions: LM317's "65" 4.8 points below)
        let has_values = |c: f32| words.iter().any(|w| (line_y(w) - c).abs() < 6.0 && w.rect.x0 >= h.min - 8.0 && number(&w.text).is_some());
        let with: Vec<String> = shared.into_iter().filter(|&c| !has_values(c)).map(conditions_only).filter(|t| !t.is_empty() && !own.contains(t.as_str())).collect();
        if !with.is_empty() {
            return format!("{own} · {}", with.join(" · "));
        }
        return own;
    }
    let reach = if downward { 80.0 } else { 30.0 };
    let mut ys: Vec<f32> = words
        .iter()
        .filter(|w| w.text.contains('=') && in_cond(w))
        .map(|w| centre(&w.rect).1)
        .filter(|&c| (c - y).abs() < reach && (!downward || (c < y && c > top)))
        .collect();
    // inside the row first, then the nearer (in steps of 3 points, a line's spacing being about 13), then above:
    // the last row of a group (Crss under Coss) takes the group's conditions, not the next row's
    ys.sort_by(|a, b| {
        let outside = |c: f32| !(top..end).contains(&c);
        let step = |c: f32| ((c - y).abs() / 3.0).round() as i32;
        (outside(*a), step(*a), *a > y).cmp(&(outside(*b), step(*b), *b > y))
    });
    // a line of its own words without conditions keeps those with a digit (a grade: "BC547B"; not the parameter's
    // name) before the borrowed conditions
    // (a range written with signs keeps its words: "10 Hz ≤ f ≤ 100 kHz", onsemi's MC7800 noise, not "10 100")
    let ranged = own.contains(['≤', '≥', '<', '>']) && own.chars().any(|c| c.is_ascii_digit());
    let own: String = if ranged { own.clone() } else { own.split_whitespace().filter(|w| w.chars().any(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(" ") };
    match ys.first() {
        Some(&c) if own.trim().is_empty() => conditions_only(c),
        Some(&c) => format!("{} · {}", own.trim(), conditions_only(c)),
        None => own,
    }
}

/// The unit in the units column nearest to line `y` within the row, or just above the row on a line without values
/// (TI's LP5907: "%VOUT" by the parameter's name, 12 points over its "–3 3" and outside the row of ΔVOUT under it).
fn unit_near(words: &[Word], band: &[&&Word], h: &Header, y: f32) -> Option<String> {
    let ux = h.unit?;
    // a unit with a prefix, or a percentage ("%/V", "%VO", "%/W": a regulator's line and load regulation)
    // (and a unit per volt of output: onsemi's 78xx noise "µV/VO", its µ printed apart as "V/VO")
    let is_unit = |w: &Word| (centre(&w.rect).0 - ux).abs() < 20.0 && (prefix(&w.text).is_some() || w.text.trim().starts_with('%') || w.text.contains("/VO") || w.text.trim() == "×VO");
    let no_values = |c: f32| !words.iter().any(|o| (centre(&o.rect).1 - c).abs() < 3.0 && number(&o.text).is_some() && column(h, centre(&o.rect).0).is_some());
    let above = words.iter().filter(|w| is_unit(w) && centre(&w.rect).1 < y && y - centre(&w.rect).1 < 14.0 && no_values(centre(&w.rect).1));
    let nearest = |c: &mut dyn Iterator<Item = &Word>| c.min_by(|a, b| (centre(&a.rect).1 - y).abs().total_cmp(&(centre(&b.rect).1 - y).abs())).map(|w| w.text.trim().to_string());
    // first a unit on this line or on a line without values: a unit printed on another line with values is more likely
    // that line's (LM358: the "mV" of the rail swing over the source current's "–20 –30", whose "mA" is 17 points
    // below them); else any unit of the row (a unit printed once on one of the two lines of a row)
    let own_or_free = |w: &Word| (centre(&w.rect).1 - y).abs() < 3.0 || no_values(centre(&w.rect).1);
    nearest(&mut band.iter().map(|w| &***w).filter(|w| is_unit(w) && own_or_free(w)).chain(above))
        .or_else(|| nearest(&mut band.iter().map(|w| &***w).filter(|w| is_unit(w))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// onsemi's MC7800 page: a group of min / typ / max for each grade side by side, the grades printed over them. A part
    /// number reads its grade's group; a part number of no grade on the page reads nothing there; without one, as before.
    #[test]
    fn a_grades_group_of_columns() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y - 8.0, x + 5.0 * t.chars().count() as f32, y);
        let page = vec![
            w("MC7805B,", 307.0, 77.9), w("NCV7805B", 347.0, 77.9), w("MC7805C", 444.0, 77.9),
            w("Characteristic", 129.0, 92.3), w("Symbol", 256.0, 92.3), w("Min", 302.0, 92.3), w("Typ", 340.0, 92.3), w("Max", 377.0, 92.3),
            w("Min", 416.0, 92.3), w("Typ", 455.0, 92.3), w("Max", 492.0, 92.3), w("Unit", 528.0, 92.3),
            w("Line", 65.0, 161.8), w("Regulation", 82.0, 161.8), w("Regline", 260.0, 161.8),
            w("−", 306.0, 161.8), w("5.0", 341.0, 161.8), w("100", 378.0, 161.8), w("−", 421.0, 161.8), w("0.5", 456.0, 161.8), w("20", 494.0, 161.8), w("mV", 529.0, 161.8),
        ];
        let typ_max = |part: Option<&str>| find_for(2, &page, "Regline", part).iter().map(|v| (v.typ, v.max)).collect::<Vec<_>>();
        assert_eq!(typ_max(Some("MC7805BTG")), [(Some(5.0), Some(100.0))]);
        assert_eq!(typ_max(Some("NCV7805BDTRKG")), [(Some(5.0), Some(100.0))]);
        assert_eq!(typ_max(Some("MC7805CTG")), [(Some(0.5), Some(20.0))]);
        assert!(typ_max(Some("MC7805ABTG")).is_empty(), "MC7805AB is another page's grade");
        assert_eq!(typ_max(None).len(), 1, "no part number: one group read, as before");
        assert_eq!(grades(&page), ["MC7805B", "NCV7805B", "MC7805C"]);
        assert_eq!(grade_labels("MC7805AB/MC7805AC/NCV7805AB"), ["MC7805AB", "MC7805AC", "NCV7805AB"]);
        assert!(grade_labels("Characteristic").is_empty() && grade_labels("(Note").is_empty());
    }

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

    /// TI's LM317: a percentage unit ("%/V") is the row's, not the "mV" of the next; a range broken over two lines takes its end.
    #[test]
    fn a_percentage_unit_and_a_range_over_two_lines_are_read() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let words = vec![
            w("PARAMETER", 60.0, 100.0), w("TEST", 185.0, 100.0), w("CONDITIONS", 210.0, 100.0), w("MIN", 437.0, 100.0), w("TYP", 473.0, 100.0), w("MAX", 502.0, 100.0), w("UNIT", 529.0, 100.0),
            w("Line", 60.0, 120.0), w("TJ", 185.0, 120.0), w("=", 196.0, 120.0), w("25°C", 203.0, 120.0), w("0.01", 473.0, 120.0), w("0.04", 504.0, 120.0), w("%/V", 531.0, 120.0),
            w("Load", 60.0, 150.0), w("IO", 185.0, 150.0), w("=", 196.0, 150.0), w("10mA", 203.0, 150.0), w("to", 230.0, 150.0), w("20", 479.0, 150.0), w("70", 511.0, 150.0), w("mV", 533.0, 150.0),
            w("1500mA,", 185.0, 160.0),
        ];
        let line = find(0, &words, "Line");
        assert_eq!((line[0].typ, line[0].unit.as_deref()), (Some(0.01), Some("%/V")), "{line:?}");
        let load = find(0, &words, "Load");
        assert!(load[0].conditions.contains("10mA to 1500mA"), "{load:?}");
    }

    /// ROHM's BDxxHC5WEFJ: the fixed output's limits as fractions of it ("VO×0.99 VO VO×1.01"), the unit "×VO"; the symbol
    /// "VO" left of them stays the symbol.
    #[test]
    fn limits_as_fractions_of_the_output() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let words = vec![
            w("Parameter", 62.0, 100.0), w("Symbol", 225.0, 100.0), w("Min.", 280.0, 100.0), w("Typ.", 325.0, 100.0), w("Max.", 366.0, 100.0), w("Unit", 410.0, 100.0), w("Conditions", 437.0, 100.0),
            w("VFB", 230.0, 120.0), w("0.792", 279.0, 120.0), w("0.800", 323.0, 120.0), w("0.808", 366.0, 120.0), w("V", 413.0, 120.0), w("IO=0A", 437.0, 120.0),
            w("VO", 232.0, 134.0), w("VO×0.99", 262.0, 134.0), w("VO", 329.0, 134.0), w("VO×1.01", 352.0, 134.0), w("V", 413.0, 134.0), w("IO=0A", 437.0, 134.0),
        ];
        let vo = find(0, &words, "VO");
        assert_eq!(vo.iter().map(|v| (v.min, v.typ, v.max, v.unit.as_deref())).collect::<Vec<_>>(), vec![(Some(0.99), Some(1.0), Some(1.01), Some("×VO"))], "{vo:?}");
        assert_eq!(find(0, &words, "VFB")[0].unit.as_deref(), Some("V"), "the other lines keep their unit");
    }

    /// TI's LP5907: the conditions start left of their heading, a frequency on each line of the PSRR, and the noise's
    /// band once on the line of its symbol with a current on each line of values.
    #[test]
    fn conditions_left_of_their_heading_and_one_for_the_row() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let line = |x: f32, y: f32, text: &str| -> Vec<Word> {
            let mut x = x;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 3.0; r }).collect()
        };
        let mut words = vec![w("PARAMETER", 116.0, 300.0), w("TEST", 274.0, 300.0), w("CONDITIONS", 297.0, 300.0), w("MIN", 419.0, 300.0), w("TYP", 458.0, 300.0), w("MAX", 495.0, 300.0), w("UNIT", 526.0, 300.0)];
        words.extend(line(228.0, 320.0, "f = 100Hz, IOUT = 20mA"));
        words.push(w("90", 464.0, 320.0));
        words.extend([w("PSRR", 59.0, 327.0), w("Power-supply", 106.0, 327.0), w("dB", 530.0, 327.0)]);
        words.extend(line(228.0, 334.0, "f = 1kHz, IOUT = 20mA"));
        words.push(w("82", 464.0, 334.0));
        words.extend(line(327.0, 360.0, "IOUT = 1mA"));
        words.push(w("10", 464.0, 360.0));
        words.extend([w("eN", 59.0, 367.0), w("Output", 106.0, 367.0), w("noise", 132.0, 367.0), w("µVRMS", 524.0, 367.0)]);
        words.extend(line(228.0, 366.0, "BW = 10Hz to 100kHz"));
        words.extend(line(327.0, 374.0, "IOUT = 250mA"));
        words.push(w("6.5", 462.0, 374.0));
        let psrr = find(0, &words, "PSRR");
        assert_eq!(psrr.len(), 2, "{psrr:?}");
        assert_eq!((psrr[0].typ, psrr[0].condition("f")), (Some(90.0), Some(100.0)), "{psrr:?}");
        assert_eq!(psrr[1].condition("f"), Some(1000.0));
        let en = find(0, &words, "eN");
        assert_eq!(en.len(), 2, "{en:?}");
        assert!(en.iter().all(|v| v.conditions.contains("BW = 10Hz to 100kHz")), "{en:?}");
        assert!(en[0].typ == Some(10.0) && en[0].conditions.starts_with("IOUT = 1mA"), "{en:?}");
    }

    /// TI's LP5907: "Line regulation" and "Load regulation" under one ΔVOUT, their names on the lines of their values; another
    /// row's name on the line of its own symbol is not looked up by name.
    #[test]
    fn lines_named_under_a_shared_symbol() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let line = |x: f32, y: f32, text: &str| -> Vec<Word> {
            let mut x = x;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 3.0; r }).collect()
        };
        let mut words = vec![w("PARAMETER", 116.0, 300.0), w("TEST", 274.0, 300.0), w("CONDITIONS", 297.0, 300.0), w("MIN", 419.0, 300.0), w("TYP", 458.0, 300.0), w("MAX", 495.0, 300.0), w("UNIT", 526.0, 300.0)];
        words.push(w("ΔVOUT", 59.0, 330.0));
        words.extend(line(228.0, 340.0, "VIN = 3.3V to 5.5V,"));
        words.extend(line(106.0, 345.0, "Line regulation"));
        words.extend([w("0.02", 458.0, 345.0), w("%/V", 528.0, 345.0)]);
        words.extend(line(106.0, 365.0, "Load regulation"));
        words.extend(line(228.0, 365.0, "IOUT = 1mA to 250mA"));
        words.extend([w("0.001", 453.0, 365.0), w("%/mA", 525.0, 365.0)]);
        words.extend([w("ISC", 59.0, 390.0)]);
        words.extend(line(106.0, 390.0, "Output short circuit current"));
        words.extend([w("408", 458.0, 390.0), w("mA", 528.0, 390.0)]);
        assert!(find(0, &words, "Line").is_empty(), "no symbol \"Line\"");
        let line_reg = find_by_name(0, &words, "Line");
        assert_eq!((line_reg.len(), line_reg[0].typ, line_reg[0].unit.as_deref()), (1, Some(0.02), Some("%/V")), "{line_reg:?}");
        assert!(line_reg[0].conditions.contains("3.3V to 5.5V"), "{line_reg:?}");
        let load = find_by_name(0, &words, "Load");
        assert_eq!((load[0].typ, load[0].conditions.as_str()), (Some(0.001), "IOUT = 1mA to 250mA"), "{load:?}");
        assert!(find_by_name(0, &words, "Output").is_empty(), "the line of ISC is ISC's");
    }

    /// TI's LM358: the rail swing VO and the output current IO each printed in the middle of its lines, the rows touching; the
    /// source current's line, 28.9 points from either symbol, is IO's (VO's lines reach 17 points above it, so not 29 below).
    #[test]
    fn a_line_halfway_between_two_symbols_goes_to_the_row_it_fits() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let mut words = vec![w("PARAMETER", 87.0, 480.0), w("MIN", 440.0, 480.0), w("TYP", 475.0, 480.0), w("MAX", 509.0, 480.0), w("UNIT", 533.0, 480.0)];
        words.extend([w("2", 479.0, 538.0), w("3", 516.0, 538.0), w("V", 537.0, 538.0)]);
        words.extend([w("VO", 59.0, 555.0), w("Voltage", 95.0, 555.0)]);
        words.extend([w("5", 479.0, 572.5), w("20", 516.0, 572.5), w("mV", 535.0, 572.5)]);
        words.extend([w("–20", 436.0, 584.0), w("–30", 472.0, 584.0)]);
        words.extend([w("–10", 436.0, 607.0), w("mA", 535.0, 607.0)]);
        words.extend([w("IO", 59.0, 613.0), w("Output", 95.0, 613.0)]);
        words.extend([w("10", 440.0, 619.0), w("20", 475.0, 619.0)]);
        words.extend([w("12", 440.0, 642.0), w("30", 475.0, 642.0), w("µA", 535.0, 642.0)]);
        let io = find(0, &words, "IO");
        assert!(io.iter().any(|v| v.min == Some(-20.0)), "the source current is IO's: {io:?}");
        let vo = find(0, &words, "VO");
        assert!(!vo.iter().any(|v| v.min == Some(-20.0)) && vo.len() == 2, "{vo:?}");
    }

    /// TI's LP5907: the output voltage tolerance under ΔVOUT prints its min with an en dash ("–3") and its unit
    /// ("%VOUT") by the parameter's name, 12 points over the values and outside the row; the next line's "%/V" is not it.
    #[test]
    fn an_en_dash_minus_and_a_unit_above_the_values() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let line = |x: f32, y: f32, text: &str| -> Vec<Word> {
            let mut x = x;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 3.0; r }).collect()
        };
        let mut words = vec![w("PARAMETER", 116.0, 300.0), w("TEST", 274.0, 300.0), w("CONDITIONS", 297.0, 300.0), w("MIN", 419.0, 300.0), w("TYP", 458.0, 300.0), w("MAX", 495.0, 300.0), w("UNIT", 526.0, 300.0)];
        words.extend(line(106.0, 355.0, "Output voltage tolerance"));
        words.push(w("%VOUT", 523.0, 355.0));
        words.extend(line(228.0, 366.0, "IOUT = 1mA to 250mA"));
        words.extend([w("–3", 424.0, 366.0), w("3", 500.0, 366.0)]);
        words.push(w("ΔVOUT", 59.0, 374.0));
        words.extend(line(106.0, 395.0, "Line regulation"));
        words.extend([w("0.02", 458.0, 395.0), w("%/V", 528.0, 395.0)]);
        let v = find(0, &words, "ΔVOUT");
        let tol = v.iter().find(|v| v.max == Some(3.0)).expect("the tolerance's line");
        assert_eq!((tol.min, tol.unit.as_deref()), (Some(-3.0), Some("%VOUT")), "{v:?}");
    }

    /// onsemi's MC7800: a group of min / typ / max for each grade side by side; the cells of the left group ("− 0.2 −")
    /// are not the conditions of the row read in the right one.
    #[test]
    fn the_cells_of_another_column_group_are_not_conditions() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let line = |x: f32, y: f32, text: &str| -> Vec<Word> {
            let mut x = x;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 3.0; r }).collect()
        };
        let mut words = vec![w("Characteristic", 65.0, 300.0), w("Symbol", 260.0, 300.0)];
        words.extend(line(306.0, 300.0, "Min Typ Max"));
        words.extend([w("Min", 421.0, 300.0), w("Typ", 456.0, 300.0), w("Max", 498.0, 300.0), w("Unit", 529.0, 300.0)]);
        words.extend(line(65.0, 340.0, "Short Circuit Current Limit (TA = 25°C)"));
        words.extend([w("ISC", 265.0, 340.0), w("A", 533.0, 340.0)]);
        words.extend(line(73.0, 349.0, "Vin = 35 Vdc"));
        words.extend([w("−", 306.0, 349.0), w("0.2", 341.0, 349.0), w("−", 383.0, 349.0), w("−", 421.0, 349.0), w("0.6", 456.0, 349.0), w("−", 498.0, 349.0)]);
        let v = find(0, &words, "ISC");
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].conditions.contains("Vin = 35 Vdc") && !v[0].conditions.contains("0.2"), "{v:?}");
    }

    /// TI's LM358: the short-circuit current is printed "±40 ±60" (not read) on the line of its symbol; the lines of the
    /// sink current above it stay the output current's, and the unit of the line over the source current's values is
    /// not theirs (their "mA" is below them).
    #[test]
    fn a_signed_value_keeps_its_symbol_from_taking_the_lines_above() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let line = |x: f32, y: f32, text: &str| -> Vec<Word> {
            let mut x = x;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 3.0; r }).collect()
        };
        let mut words = vec![w("PARAMETER", 87.0, 300.0), w("TEST", 230.0, 300.0), w("CONDITIONS", 255.0, 300.0), w("MIN", 440.0, 300.0), w("TYP", 475.0, 300.0), w("MAX", 509.0, 300.0), w("UNIT", 533.0, 300.0)];
        words.extend(line(184.0, 320.0, "VS = 5 V"));
        words.extend([w("5", 479.0, 320.0), w("20", 516.0, 320.0), w("mV", 535.0, 320.0)]);
        words.extend([w("–20", 436.0, 331.0), w("–30", 472.0, 331.0)]);
        words.extend(line(184.0, 337.0, "VS = 15 V; VO = 0 V"));
        words.push(w("mA", 535.0, 348.0));
        words.extend([w("IO", 59.0, 360.0), w("Output", 87.0, 360.0)]);
        words.extend(line(184.0, 360.0, "VS = 15 V; VO = 15 V"));
        words.extend([w("10", 440.0, 355.0), w("20", 475.0, 355.0)]);
        words.extend(line(184.0, 373.0, "VO = 200 mV"));
        words.extend([w("60", 456.0, 373.0), w("100", 481.0, 373.0), w("μA", 537.0, 373.0)]);
        words.extend([w("ISC", 59.0, 385.0), w("Short-circuit", 87.0, 385.0)]);
        words.extend(line(184.0, 385.0, "VS = 20 V"));
        words.extend([w("±40", 481.0, 385.0), w("±60", 515.0, 385.0), w("mA", 537.0, 385.0)]);
        assert!(find(0, &words, "ISC").is_empty(), "{:?}", find(0, &words, "ISC"));
        let io = find(0, &words, "IO");
        assert!(io.iter().any(|v| v.min == Some(60.0)), "the sink current at 200 mV is IO's: {io:?}");
        let source = io.iter().find(|v| v.min == Some(-20.0)).expect("the source current");
        assert_eq!(source.unit.as_deref(), Some("mA"), "{io:?}");
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

    /// A table laid out like onsemi's small-signal transistors: the symbol column right of the text, each row
    /// starting at its symbol, the conditions in brackets with spaces, grades on their own lines, and the DC hFE
    /// next to the small-signal hfe.
    #[test]
    fn a_symbol_column_on_the_right_with_grades() {
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 7.0);
        let mut words = vec![w("Characteristic", 150.0, 70.0), w("Symbol", 350.0, 70.0), w("Min", 405.0, 70.0), w("Typ", 445.0, 70.0), w("Max", 485.0, 70.0), w("Unit", 525.0, 70.0)];
        let line = |y: f32, text: &str| -> Vec<Word> {
            let mut x = 65.0;
            text.split(' ').map(|t| { let r = w(t, x, y); x += 5.0 * t.chars().count() as f32 + 4.0; r }).collect()
        };
        words.extend(line(100.0, "DC Current Gain"));
        words.push(w("hFE", 360.0, 100.0));
        words.extend(line(110.0, "(IC = 2.0 mA, VCE = 5.0 V)"));
        words.extend([w("BC547", 315.0, 110.0), w("110", 410.0, 110.0), w("800", 491.0, 110.0)]);
        words.extend([w("BC547B", 309.0, 120.0), w("200", 409.0, 120.0), w("290", 450.0, 120.0), w("450", 491.0, 120.0)]);
        words.extend(line(130.0, "(IC = 100 mA, VCE = 5.0 V)"));
        words.extend([w("BC547B", 309.0, 130.0), w("180", 450.0, 130.0)]);
        words.extend(line(150.0, "Small - Signal Current Gain"));
        words.push(w("hfe", 362.0, 150.0));
        words.extend(line(160.0, "(IC = 2.0 mA, VCE = 5.0 V, f = 1.0 kHz)"));
        words.extend([w("BC547B", 309.0, 160.0), w("240", 409.0, 160.0), w("330", 450.0, 160.0), w("500", 491.0, 160.0)]);
        let h = find(0, &words, "hFE");
        assert_eq!(h.len(), 3, "{h:?}");
        assert_eq!((h[1].typ, h[1].condition("IC"), h[1].conditions.starts_with("BC547B")), (Some(290.0), Some(2e-3), true), "{:?}", h[1]);
        assert_eq!((h[2].typ, h[2].condition("IC")), (Some(180.0), Some(0.1)), "{:?}", h[2]);
        let s = find(0, &words, "hfe");
        assert_eq!((s.len(), s[0].typ, s[0].condition("f")), (1, Some(330.0), Some(1000.0)), "{s:?}");
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
        // onsemi's direct-value units: 10 mAdc is 10 mA
        assert_eq!(prefix("mAdc"), Some(1e-3));
        assert_eq!(prefix("Vdc"), Some(1.0));
        assert_eq!(condition("(IC = 10 mAdc, VCE = 1.0 Vdc)", "IC"), Some(10.0 * 1e-3));
        // a chain: 1N4148's trr at "IF = IR = 10 mA"
        assert_eq!(condition("IF = IR = 10 mA,", "IF"), Some(10.0 * 1e-3));
        assert_eq!(condition("IF = IR = 10 mA,", "IR"), Some(10.0 * 1e-3));
        assert_eq!(prefix("Typ"), None);
        assert_eq!(condition("VGS=4.5V, ID=250µA", "ID"), Some(250e-6));
        assert_eq!(condition("VGS=4.5V, ID=250µA", "vgs"), Some(4.5));
        assert_eq!(condition("VDS=VGS ID=-4.0A", "ID"), Some(4.0), "conditions without a comma between them");
        assert_eq!(condition("VDS=0V, VGS= ±12V", "VGS"), Some(12.0));
        assert_eq!(condition("VGS=-10V, ID=-4.0A", "ID"), Some(4.0));
        assert_eq!(condition("VDS=VGS", "VDS"), None);
        assert_eq!(condition("(IC = 2.0 mA, VCE = 5.0 V)", "IC"), Some(2e-3));
        assert_eq!(prefix("kHz"), Some(1e3));
        assert_eq!(condition("f = 1.0 kHz)", "f"), Some(1e3));
        assert_eq!(condition("(IC = 2.0 mA, VCE = 5.0 V, f = 1.0 kHz) BC547B", "f"), Some(1e3));
        assert_eq!(condition("(IC = 2.0 mA, VCE = 5.0 V)", "VCE"), Some(5.0));
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

    /// ROHM's Japanese table: "Parameter Symbol Unit Conditions" over "Limits" over "Min. Typ. Max.", the symbols right of
    /// the Japanese names, the conditions right of the unit; and "⊿VO" (the right triangle) is ΔVO, not VO.
    #[test]
    fn a_header_over_two_lines_and_conditions_on_the_right() {
        use super::*;
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let words = vec![
            w("Parameter", 109.0, 341.0), w("Symbol", 221.0, 341.0), w("Unit", 408.0, 341.0), w("Conditions", 475.0, 341.0),
            w("Min.", 282.0, 348.0), w("Typ.", 326.0, 348.0), w("Max.", 367.0, 348.0),
            w("入出力電圧差1", 62.0, 416.0), w("VCO1", 228.0, 416.0), w("-", 289.0, 416.0), w("0.12", 326.0, 416.0), w("0.19", 368.0, 416.0), w("V", 413.0, 416.0), w("VCC=5V,", 437.0, 416.0), w("IO=300mA", 476.0, 416.0),
            w("出力安定度", 62.0, 430.0), w("⊿VO", 226.0, 430.0), w("-", 289.0, 430.0), w("1", 333.0, 430.0), w("10", 370.0, 430.0), w("mV", 411.0, 430.0), w("IO=0→1A", 437.0, 430.0),
        ];
        let v = find(0, &words, "VCO1");
        assert_eq!((v.len(), v[0].typ, v[0].max, v[0].unit.as_deref(), v[0].conditions.as_str()), (1, Some(0.12), Some(0.19), Some("V"), "VCC=5V, IO=300mA"), "{v:?}");
        assert!(find(0, &words, "VO").is_empty(), "⊿VO is not VO");
        assert_eq!(find(0, &words, "ΔVO")[0].max, Some(10.0));
    }
}
