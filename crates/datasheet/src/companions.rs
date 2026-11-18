//! The capacitors a data sheet asks for beside the part: "CIN = 1µF", "COUT ≥ 2.2 µF", "Cin = 0.33 µF" in the test conditions of
//! the electrical characteristics and in the typical application. Each name is taken with the value written most often (the
//! conditions repeat it on many rows), and whether it is a least value ("≥").

use crate::figures::lines;
use crate::spec_table::prefix;
use crate::text::{Word, normalize};

/// A capacitor named in the data sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundCap {
    /// The name as the data sheet writes it, upper case ("CIN", "COUT", "C1").
    pub name: String,
    pub farads: f64,
    /// Written as a least value ("COUT ≥ 1 µF").
    pub at_least: bool,
    /// The first page it is written on.
    pub page: usize,
    /// How many times this name was written with this value.
    pub count: usize,
}

/// A capacitor's name: "CIN", "COUT", "CI", "CO", "CBYP", "CNR", "CSS", "CFF", "CBOOT", "C1" (a letter C and a short tail).
fn cap_name(t: &str) -> Option<String> {
    let t = t.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_uppercase();
    let rest = t.strip_prefix('C')?;
    let tails = ["IN", "OUT", "I", "O", "BYP", "NR", "SS", "FF", "BOOT", "BST", "SET", "REF", "ADJ"];
    let digits = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let letters = &rest[..rest.len() - digits.len()];
    let ok = (tails.contains(&letters) && digits.chars().all(|c| c.is_ascii_digit()) && digits.len() <= 1) || (letters.is_empty() && (1..=2).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()));
    ok.then_some(t)
}

/// A capacitance written as one token or two ("1µF", "0.33" "µF", "=1µF"): farads.
fn farads(toks: &[String], i: usize) -> Option<(f64, usize)> {
    let t = toks.get(i)?.trim_start_matches(['=', '≥', '>', '≧']).trim_end_matches([',', ';', ')']);
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let x: f64 = digits.parse().ok()?;
    let (unit, used) = if digits.len() < t.len() { (&t[digits.len()..], 1) } else { (toks.get(i + 1).map_or("", |u| u.trim_end_matches([',', ';', ')'])), 2) };
    // a capacitance: the unit's base is F
    // "uF", and the Greek mu (U+03BC) some fonts give for the micro sign (TI's LM317)
    let u = unit.replace(['u', '\u{3bc}'], "µ");
    if !u.to_ascii_lowercase().ends_with('f') {
        return None;
    }
    let k = prefix(&u.replace('f', "F"))?;
    Some((x * k, used))
}

/// The capacitors of every page's lines (`pages`: the words of each page).
pub fn read(pages: &[&[Word]]) -> Vec<FoundCap> {
    // (name, value, at least, page)
    let mut seen: Vec<(String, f64, bool, usize)> = vec![];
    for (page, words) in pages.iter().enumerate() {
        for line in lines(words) {
            let toks: Vec<String> = line.iter().map(|w| normalize(&w.text).to_string()).collect();
            for i in 0..toks.len() {
                // "CIN = 1µF", "CIN=1µF", "COUT ≥ 2.2 µF"; the name may carry its sign ("CIN=")
                let (name_tok, rest) = match toks[i].find(['=', '≥', '>', '≧']) {
                    Some(k) if k > 0 => (&toks[i][..k], Some(&toks[i][k..])),
                    _ => (toks[i].as_str(), None),
                };
                let Some(name) = cap_name(name_tok) else { continue };
                let (sign, at) = match rest {
                    Some(r) if r.chars().count() > 1 => (r.chars().next().unwrap_or('='), None),
                    Some(r) => (r.chars().next().unwrap_or('='), Some(i + 1)),
                    None => match toks.get(i + 1).and_then(|t| t.chars().next()) {
                        Some(c @ ('=' | '≥' | '>' | '≧')) => (c, Some(if toks[i + 1].chars().count() > 1 { i + 1 } else { i + 2 })),
                        _ => continue,
                    },
                };
                let value = match at {
                    // the value joined to the sign: "CIN=1µF" (the rest after the name)
                    None => rest.and_then(|r| farads(&[r.to_string()], 0).or_else(|| farads(&[r.to_string(), toks.get(i + 1).cloned().unwrap_or_default()], 0))),
                    Some(j) => farads(&toks, j),
                };
                if let Some((f, _)) = value
                    && f > 0.0
                {
                    seen.push((name, f, sign != '=', page));
                }
            }
        }
    }
    // each name with its most written value
    let mut out: Vec<FoundCap> = vec![];
    for (name, f, at_least, page) in &seen {
        if out.iter().any(|o| o.name == *name) {
            continue;
        }
        let same = |o: &&(String, f64, bool, usize)| o.0 == *name;
        let mut best: Option<(f64, bool, usize, usize)> = None;
        for (_, g, l, p) in seen.iter().filter(same) {
            let n = seen.iter().filter(same).filter(|o| (o.1 / g - 1.0).abs() < 1e-6).count();
            if best.is_none_or(|b| n > b.3) {
                best = Some((*g, *l, *p, n));
            }
        }
        let (g, l, p, n) = best.unwrap_or((*f, *at_least, *page, 1));
        out.push(FoundCap { name: name.clone(), farads: g, at_least: l, page: p, count: n });
    }
    out
}

/// The ESR a data sheet asks of the capacitors beside the part: a range ("5mΩ to 500mΩ", a table's "ESR … 5 500 mΩ", "0.3Ωから
/// 5Ωの範囲") or a bound ("ESR below 20mΩ"), in ohms, where it is written, and whether it is said of the input capacitor too
/// ("Input/output capacitor ESR range").
#[derive(Clone, Debug, PartialEq)]
pub struct FoundEsr {
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub page: usize,
    pub input_too: bool,
}

/// The resistances written in a text, in ohms: "5mΩ", "500 mΩ", "0.3Ω", and the bare numbers just before a unit standing alone
/// ("5 500 mΩ" of a table's row).
fn ohms_in(text: &str) -> Vec<f64> {
    // (lower case after `normalize`: ω; the ohm sign U+2126 too)
    let t = text.replace("ohms", "Ω").replace("ohm", "Ω").replace(['Ω', 'ω'], "Ω");
    let chars: Vec<char> = t.chars().collect();
    let mut out = vec![];
    let mut bare: Vec<f64> = vec![];
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '.')) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let v: f64 = chars[start..i].iter().collect::<String>().parse().unwrap_or(f64::NAN);
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let k = match chars.get(j) {
                Some('m') if chars.get(j + 1) == Some(&'Ω') => Some((1e-3, j + 2)),
                Some('k') | Some('K') if chars.get(j + 1) == Some(&'Ω') => Some((1e3, j + 2)),
                Some('Ω') => Some((1.0, j + 1)),
                _ => None,
            };
            match k {
                Some((k, end)) if v.is_finite() => {
                    out.extend(bare.drain(..).map(|b| b * k));
                    out.push(v * k);
                    i = end;
                }
                _ if v.is_finite() => bare.push(v),
                _ => {}
            }
            continue;
        }
        i += 1;
    }
    out
}

/// The ESR asked of the capacitors (`FoundEsr`): the first line naming ESR with two resistances (a range), else with one and
/// a word of bound before it ("below", "less than", "<", "max", "以下", "未満"; "above", ">", "min", "以上").
pub fn esr(pages: &[&[Word]]) -> Option<FoundEsr> {
    let mut bound: Option<FoundEsr> = None;
    for (page, words) in pages.iter().enumerate() {
        for line in lines(words) {
            let mut l = line.clone();
            l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
            let text = l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let low = normalize(&text);
            let Some(at) = low.find("esr") else { continue };
            let after = &low[at..];
            let values = ohms_in(after);
            let input_too = low.contains("input/output") || low.contains("output/input") || low.contains("input and output") || low.contains("入出力");
            match values.as_slice() {
                [a, b, ..] if a < b => return Some(FoundEsr { lo: Some(*a), hi: Some(*b), page, input_too }),
                [v] if bound.is_none() => {
                    let first = after.find(|c: char| c.is_ascii_digit()).unwrap_or(after.len());
                    let before = &after[..first];
                    let below = ["below", "less than", "<", "max", "以下", "未満", "under"].iter().any(|k| before.contains(k));
                    let above = ["above", "greater than", ">", "min", "以上", "over"].iter().any(|k| before.contains(k));
                    // (an estimate in a design example is no requirement: TI's "RESR_MAX is approximately 1.5 mΩ")
                    if below != above && !before.contains("approx") {
                        bound = Some(FoundEsr { lo: above.then_some(*v), hi: below.then_some(*v), page, input_too });
                    }
                }
                _ => {}
            }
        }
    }
    bound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(texts: &[&str], y: f32) -> Vec<Word> {
        texts.iter().enumerate().map(|(i, t)| Word::for_test(t, 60.0 + 40.0 * i as f32, y, 95.0 + 40.0 * i as f32, y + 8.0)).collect()
    }

    #[test]
    fn the_capacitors_are_read_with_their_most_written_values() {
        // LM317L: the conditions on many rows, one row with another COUT
        let mut p1 = line(&["IO", "=", "2.5mA,", "COUT", "=", "0µF,", "CIN", "=", "1µF"], 100.0);
        p1.extend(line(&["VI", "=", "3V,", "COUT", "=", "1µF,", "CIN", "=", "0.1µF"], 120.0));
        p1.extend(line(&["VI", "=", "5V,", "COUT", "=", "1µF,", "CIN", "=", "0.1µF"], 140.0));
        let p2 = line(&["COUT", "≥", "2.2", "µF", "C1", "=", "0.1", "µF", "CIN=0.33µF"], 100.0);
        let found = read(&[&p1, &p2]);
        let got: Vec<(String, f64, bool, usize)> = found.iter().map(|c| (c.name.clone(), (c.farads * 1e9).round() / 1e9, c.at_least, c.count)).collect();
        let of = |n: &str| got.iter().find(|g| g.0 == n).cloned().unwrap();
        assert_eq!(of("COUT"), ("COUT".into(), 1e-6, false, 2), "{got:?}");
        assert_eq!(of("CIN"), ("CIN".into(), 1e-7, false, 2), "{got:?}");
        assert!(got.iter().any(|g| g.0 == "C1" && (g.1 - 1e-7).abs() < 1e-12), "{got:?}");
        // a name written once with a sign of its own: "COUT ≥ 2.2 µF" after the rows above is not the most written
        assert!(!found.iter().any(|c| c.name == "COUT" && c.at_least));
        // not capacitors: "CI" of a resistor value, a part number, a capacitance without F
        assert!(read(&[&line(&["CLK", "=", "1", "MHz", "CIN", "=", "10", "V"], 100.0)]).is_empty());
    }

    #[test]
    fn the_esr_asked_of_the_capacitors() {
        let line = |t: &str, y: f32| -> Vec<Word> {
            let mut x = 50.0;
            t.split(' ').map(|p| { let w = Word::for_test(p, x, y, x + 5.0 * p.chars().count() as f32, y + 8.0); x += 5.0 * p.chars().count() as f32 + 4.0; w }).collect()
        };
        let one = |t: &str| esr(&[&line(t, 100.0)]);
        // TI's LP5907: a table row and a sentence
        assert_eq!(one("ESR Output/input capacitance 5 500 mΩ").map(|e| (e.lo, e.hi, e.input_too)), Some((Some(0.005), Some(0.5), true)));
        assert_eq!(one("Input/output capacitor ESR range 5mΩ to 500mΩ").map(|e| (e.lo, e.hi)), Some((Some(0.005), Some(0.5))));
        // LTC's LT3045: a bound; ROHM's BA1117FP in Japanese
        assert_eq!(one("capacitor with an ESR below 20mΩ and an ESL").map(|e| (e.lo, e.hi, e.input_too)), Some((None, Some(0.02), false)));
        assert_eq!(one("ESR はタンタルコンデンサを使用の場合、0.3Ωから 5Ωの範囲としてく").map(|e| (e.lo, e.hi)), Some((Some(0.3), Some(5.0))));
        // a low ESR with no value, or one value with no bound, asks nothing
        assert!(one("Connect a minimum 1µF low-ESR capacitor").is_none());
        assert!(one("RESR_MAX is approximately 1.5 mΩ of two").is_none());
    }
}
