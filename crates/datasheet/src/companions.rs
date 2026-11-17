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
}
