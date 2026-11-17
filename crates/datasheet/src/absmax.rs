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
const NEXT: &[&str] = &["recommended operating", "electrical characteristics", "electro-optical", "optical characteristics", "basic characteristics", "thermal characteristics", "thermal information", "esd ratings", "推奨動作条件", "推奨動作範囲", "電気的特性", "熱特性", "熱に関する情報"];

/// A symbol of a rating: V, I, P or T, then a few letters or digits ("VDS", "ID", "IF(AV)", "TJ", "VCEO"), a trailing comma dropped.
fn symbol(t: &str) -> Option<String> {
    let t = t.trim().trim_end_matches(',');
    let first = t.chars().next()?;
    let shape = "VIPT".contains(first) && (2..=10).contains(&t.chars().count()) && t.chars().skip(1).all(|c| c.is_ascii_alphanumeric() || "()".contains(c));
    // a symbol has a capital after its first letter ("VDS", "TJ", "IF(AV)"), or is one of the few written in small letters
    let ok = shape && (t.chars().skip(1).any(|c| c.is_ascii_uppercase()) || ["Tj", "Tstg", "Ptot", "Tamb", "Ipeak"].contains(&t)) && !["TOP", "VIEW", "TTL", "TYP"].contains(&t);
    ok.then(|| t.to_string())
}

/// The symbol a row stands for when it is written in words only (TI's LM317: "Input-output voltage differential −0.3 40 V" with no
/// symbol): the input-output differential ("VI-VO"), the input or supply voltage, the output current, the power dissipation, the
/// junction temperature. None for other words.
fn label_symbol(text: &str) -> Option<&'static str> {
    let t = normalize(text).replace(['-', '–', '−'], " ");
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let has = |k: &str| t.contains(k);
    Some(if (has("input output") || has("input to output")) && has("differential") {
        "VI-VO"
    } else if t.starts_with("input voltage") {
        "VI"
    } else if t.starts_with("supply voltage") {
        "VCC"
    } else if t.starts_with("output current") {
        "IO"
    } else if has("power dissipation") {
        "PD"
    } else if has("junction temperature") {
        "TJ"
    } else {
        return None;
    })
}

/// A number of the table: "30", "±12", "-55", "0.9"; with the unit written in the same word ("30V") taken apart.
fn number(t: &str) -> Option<(f64, Option<String>)> {
    let t = normalize(t.trim()).replace('±', "");
    // a range in one word, as Japanese data sheets write it ("-0.3～10.0", ROHM): its upper end (the full-width wave
    // dash is "~" after `normalize`)
    let t = match t.split_once('~') {
        Some((_, hi)) if !hi.trim().is_empty() => hi.trim().to_string(),
        _ => t,
    };
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(t.len());
    let (n, rest) = t.split_at(split);
    let v: f64 = n.parse().ok()?;
    let rest = rest.trim();
    (rest.is_empty() || unit_scale(rest).is_some()).then(|| (v, (!rest.is_empty()).then(|| rest.to_string())))
}

/// "Vdc", "mAdc" (onsemi's units) as "V", "mA".
fn without_dc(u: &str) -> &str {
    u.strip_suffix("dc").or_else(|| u.strip_suffix("DC")).filter(|r| !r.is_empty()).unwrap_or(u)
}

/// The scale of a unit word to SI: "mA" → 1e-3, "V" → 1, "°C" → 1.
fn unit_scale(u: &str) -> Option<f64> {
    let u = without_dc(u.trim()).replace('℃', "°c");
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
/// `part`: the part number, for a table with a column per part ("TIP120, TIP121, TIP122" over "60 80 100"): its column's
/// number is taken where a row has one number per column; otherwise (and for temperatures) the largest.
pub fn ratings(page: usize, words: &[Word], top: f32, bottom: f32, part: Option<&str>) -> Vec<Rating> {
    let inside: Vec<&Word> = words.iter().filter(|w| w.rect.y0 > top && w.rect.y1 < bottom).collect();
    // the table ends at the next table's heading
    let joined = |y: f32| {
        let mut l: Vec<&&Word> = inside.iter().filter(|w| (w.baseline() - y).abs() < 2.0).collect();
        l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        normalize(&l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
    };
    let end = inside.iter().map(|w| w.baseline()).filter(|y| NEXT.iter().any(|n| joined(*y).contains(n))).fold(bottom, f32::min);
    let rows: Vec<&Word> = inside.into_iter().filter(|w| w.baseline() < end - 1.0).collect();
    // the part's column: its name in a line of two or more part numbers (x of its centre, how many columns)
    let column = part.and_then(|name| {
        let names = |y: f32| rows.iter().filter(|o| (o.baseline() - y).abs() < 2.0 && crate::suggest::looks_like_part_number(o.text.trim().trim_end_matches(','))).count();
        rows.iter().find(|w| w.text.trim().trim_end_matches(',').eq_ignore_ascii_case(name.trim())).map(|w| ((w.rect.x0 + w.rect.x1) / 2.0, names(w.baseline()))).filter(|c| c.1 >= 2)
    });
    let mut out: Vec<Rating> = vec![];
    // the rows with a symbol, then the rows written in words only: the last word of the label (left of the row's first number)
    // stands for the symbol
    let mut keyed: Vec<(&Word, String)> = rows.iter().filter_map(|w| symbol(&w.text).map(|sym| (*w, sym))).collect();
    let mut lines: Vec<f32> = rows.iter().map(|w| w.baseline()).collect();
    lines.sort_by(f32::total_cmp);
    lines.dedup_by(|a, b| (*a - *b).abs() < 2.0);
    for y in lines {
        let mut line: Vec<&Word> = rows.iter().filter(|w| (w.baseline() - y).abs() < 2.0).copied().collect();
        line.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        if line.iter().any(|w| symbol(&w.text).is_some()) || keyed.iter().any(|(k, _)| (k.baseline() - y).abs() < 8.0 && line.iter().any(|w| w.rect.x0 > k.rect.x1 && number(&w.text).is_some())) {
            continue;
        }
        // a limit written from a supply rail ("(V-) - 300 mV to (V+) + 300 mV") is not a value of its own
        let whole = normalize(&line.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "));
        if ["v+", "v-", "v−", "vcc", "vdd", "vee", "vss"].iter().any(|r| whole.split(|c: char| c.is_whitespace() || c == '(' || c == ')').any(|t| t == *r)) {
            continue;
        }
        let label: Vec<&Word> = line.iter().take_while(|w| number(&w.text).is_none() && !matches!(w.text.trim(), "–" | "−" | "-")).copied().collect();
        let text = label.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        if let (Some(sym), Some(last)) = (label_symbol(&text), label.last()) {
            keyed.push((last, sym.to_string()));
        }
    }
    for (w, sym) in keyed {
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
            let base = without_dc(unit).to_lowercase().replace('℃', "°c");
            match sym.chars().next() {
                Some('V') => base.ends_with('v'),
                Some('I') => base.ends_with('a'),
                Some('P') => base.ends_with('w'),
                Some('T') => base.ends_with('c'),
                _ => false,
            }
        };
        // a minus printed as a word of its own just left of the number ("– 65 to +150")
        let signed = |n: &Word, v: f64| {
            let dash = rows.iter().any(|o| (o.baseline() - n.baseline()).abs() < 2.0 && matches!(o.text.trim(), "–" | "−" | "-") && o.rect.x1 <= n.rect.x0 + 0.5 && n.rect.x0 - o.rect.x1 < 4.0);
            if dash { -v.abs() } else { v }
        };
        let fitting: Vec<(f64, String, &Word)> = found
            .iter()
            .filter_map(|(n, v, inside)| unit_of(n, inside).filter(|u| fits(u)).map(|u| (signed(n, *v), u, *n)))
            // a temperature's maximum is above zero: "−55 to °C" whose upper end is drawn, not written, has none
            .filter(|f| !sym.starts_with('T') || f.0 > 0.0)
            .collect();
        let on_line = fitting.iter().filter(|f| (f.2.baseline() - w.baseline()).abs() < 2.0).count();
        let best = match column {
            // one number per part on the symbol's line: the part's own
            Some((x, n)) if !sym.starts_with('T') && on_line == n => fitting
                .iter()
                .filter(|f| (f.2.baseline() - w.baseline()).abs() < 2.0)
                .min_by(|a, b| ((a.2.rect.x0 + a.2.rect.x1) / 2.0 - x).abs().total_cmp(&((b.2.rect.x0 + b.2.rect.x1) / 2.0 - x).abs())),
            _ => fitting.iter().max_by(|a, b| a.0.total_cmp(&b.0)),
        }
        .map(|f| (f.0, f.1.clone()));
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
        self.absolute_maximum_of(None)
    }

    /// The same, the column of `part` taken in a table with a column per part number.
    pub fn absolute_maximum_of(&self, part: Option<&str>) -> Vec<Rating> {
        let q = Heading::AbsMax.queries();
        for page in 0..self.page_count() {
            let h = self.page_size(page).map_or(842.0, |s| s.1);
            for hit in self.search_page_any(page, &q).into_iter().filter(|h| !h.in_contents) {
                let r = ratings(page, self.words(page), hit.rect.y1, h * 0.92, part);
                if r.len() >= 2 {
                    return r;
                }
            }
        }
        vec![]
    }
}

impl Datasheet {
    /// The rows of the recommended operating conditions table, read as the absolute maximum ratings are (the largest value of
    /// each row): a part whose absolute maximum ratings give no current (TI's LM317) gives its output current here.
    pub fn recommended_operating_of(&self, part: Option<&str>) -> Vec<Rating> {
        let q: Vec<crate::Query> = ["recommended operating conditions", "recommended operating condition", "推奨動作条件"].iter().filter_map(|t| crate::Query::new(t)).collect();
        for page in 0..self.page_count() {
            let h = self.page_size(page).map_or(842.0, |s| s.1);
            for hit in self.search_page_any(page, &q).into_iter().filter(|h| !h.in_contents) {
                let r = ratings(page, self.words(page), hit.rect.y1, h * 0.92, part);
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
        let r = ratings(0, &words, 420.0, 700.0, None);
        let v = |s: &str| pick(&r, &[s]).map(|x| x.value);
        assert_eq!(v("VDS"), Some(30.0));
        assert_eq!(v("VGS"), Some(12.0));
        assert_eq!(v("ID"), Some(5.7), "the value over the symbol, at 25 °C: {r:?}");
        assert_eq!(v("TJ"), Some(150.0));
        assert!(r.iter().all(|x| !x.symbol.starts_with('R')), "the thermal table is not read");
    }

    /// TIP120's table (onsemi): a column per part, "Vdc" and "Adc", one value shared by the columns, a range of temperatures.
    #[test]
    fn a_table_with_a_column_per_part() {
        let words = vec![
            w("TIP120,", 388.0, 72.0), w("TIP121,", 428.0, 72.0), w("TIP122,", 469.0, 72.0),
            w("Collector−Emitter", 127.0, 97.0), w("VCEO", 80.0, 97.0), w("60", 397.0, 97.0), w("80", 438.0, 97.0), w("100", 477.0, 97.0), w("Vdc", 521.0, 97.0),
            w("IC", 86.0, 140.0), w("5.0", 437.0, 140.0), w("Adc", 521.0, 140.0),
            w("IB", 86.0, 165.0), w("120", 436.0, 165.0), w("mAdc", 518.0, 165.0),
            w("TJ,", 77.0, 244.0), w("–65", 421.0, 244.0), w("to", 437.0, 244.0), w("+150", 446.0, 244.0), w("°C", 523.0, 244.0),
        ];
        let v = |r: &[Rating], s: &str| pick(r, &[s]).map(|x| x.value);
        let r = ratings(0, &words, 60.0, 300.0, Some("TIP120"));
        assert_eq!((v(&r, "VCEO"), v(&r, "IC"), v(&r, "IB"), v(&r, "TJ")), (Some(60.0), Some(5.0), Some(0.12), Some(150.0)), "{r:?}");
        assert_eq!(v(&ratings(0, &words, 60.0, 300.0, Some("TIP122")), "VCEO"), Some(100.0));
        // without the part, the largest (the table's own rule)
        assert_eq!(v(&ratings(0, &words, 60.0, 300.0, None), "VCEO"), Some(100.0));
    }

    /// A temperature range whose upper end is not in the text (P2N2222A: "−55 to °C"), or whose minus is a word of its own
    /// (1N4728A: "– 65 to °C"): no maximum, not 55 or 65.
    #[test]
    fn a_temperature_without_its_upper_end_is_not_read() {
        for low in [vec![w("−55", 271.0, 359.0)], vec![w("–", 263.0, 359.0), w("65", 269.0, 359.0)]] {
            let mut words = vec![w("TJ,", 231.0, 359.0), w("to", 288.0, 359.0), w("°C", 315.0, 359.0), w("VCEO", 231.0, 340.0), w("40", 271.0, 340.0), w("Vdc", 315.0, 340.0)];
            words.extend(low);
            let r = ratings(0, &words, 300.0, 400.0, None);
            assert!(pick(&r, &["TJ"]).is_none() && pick(&r, &["VCEO"]).is_some(), "{r:?}");
        }
        let words = vec![w("TJ,", 231.0, 359.0), w("–", 263.0, 359.0), w("65", 269.0, 359.0), w("to", 288.0, 359.0), w("+200", 300.0, 359.0), w("°C", 335.0, 359.0)];
        assert_eq!(pick(&ratings(0, &words, 300.0, 400.0, None), &["TJ"]).map(|x| x.value), Some(200.0));
    }

    #[test]
    fn units_scale_to_si() {
        assert_eq!(unit_scale("mAdc"), Some(1e-3));
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

    /// Rows written in words only (TI's LM317 has no symbol column): the input-output differential; a limit written from a supply
    /// rail is not one (ADA4522's "(V-) - 300 mV to (V+) + 300 mV").
    #[test]
    fn rows_without_a_symbol_are_read_by_their_words() {
        use super::ratings;
        use crate::text::Word;
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let words = vec![
            w("Power", 59.0, 150.0), w("dissipation", 84.0, 150.0), w("Internally", 381.0, 150.0), w("limited", 416.0, 150.0),
            w("Input-output", 59.0, 164.0), w("voltage", 104.0, 164.0), w("differential", 132.0, 164.0), w("−0.3", 353.0, 164.0), w("40", 455.0, 164.0), w("V", 529.0, 164.0),
            w("Input", 59.0, 178.0), w("Voltage", 82.0, 178.0), w("(V-)", 267.0, 178.0), w("-", 281.0, 178.0), w("300", 286.0, 178.0), w("mV", 301.0, 178.0),
            w("Tstg", 137.0, 192.0), w("Storage", 59.0, 192.0), w("150", 453.0, 192.0), w("°C", 528.0, 192.0),
        ];
        let r = ratings(0, &words, 100.0, 300.0, None);
        assert!(r.iter().any(|x| x.symbol == "VI-VO" && x.value == 40.0), "{r:?}");
        assert!(!r.iter().any(|x| x.symbol == "VI"), "the rail-relative input voltage: {r:?}");
        assert!(r.iter().any(|x| x.symbol == "Tstg"));
    }

    /// ROHM's Japanese table: a range in one word with a full-width wave dash ("-0.3～10.0"), and a footnote mark ("*1") beside it
    /// that is not the value; the table ends at 推奨動作範囲.
    #[test]
    fn a_japanese_range_in_one_word() {
        use super::*;
        let w = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0);
        let words = vec![
            w("VCC", 229.0, 88.0), w("-0.3～10.0", 306.0, 88.0), w("1", 354.0, 85.0), w("*", 351.0, 88.0), w("V", 411.0, 88.0),
            w("Tstg", 228.0, 146.0), w("-55～+150", 313.0, 146.0), w("℃", 409.0, 146.0),
            w("推奨動作範囲(Ta=25℃)", 52.0, 212.0),
            w("IO", 234.0, 292.0), w("1.5", 359.0, 292.0), w("A", 412.0, 292.0),
        ];
        let r = ratings(4, &words, 70.0, 800.0, None);
        let got: Vec<(&str, f64)> = r.iter().map(|r| (r.symbol.as_str(), r.value)).collect();
        assert_eq!(got, vec![("VCC", 10.0), ("Tstg", 150.0)], "{r:?}");
    }
}
