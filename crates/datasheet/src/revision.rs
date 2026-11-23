//! Which edition of a data sheet this is (候補 Q9): its revision and date as the maker prints them in the page foot or head
//! (onsemi "April, 2024 − Rev. 7", Infineon "Rev. 2.1, 2026-07-24", Vishay "Revision: 22-Jun-2026", Analog Devices "Rev I;
//! 3/25", Alpha & Omega "Rev 3: Dec 2011", TI's literature number with its revision letter "SLVS044Z" and "REVISED APRIL 2025",
//! Microchip's "DS20001826E"), a fingerprint of the file, and the table values read off it (`SheetValue`), so a part made from
//! it can be checked against a newer edition (`compare`).

use crate::spec_table::SpecValue;
use crate::text::{Word, normalize};

/// The revision and date read off a data sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetRevision {
    /// As printed ("29", "2.1", "I", "SLVS044Z", "DS20001826E").
    pub rev: Option<String>,
    /// As printed ("April, 2024", "2026-07-24", "22-Jun-2026", "3/25", "APRIL 2025").
    pub date: Option<String>,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month(t: &str) -> bool {
    let t = t.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
    t.len() >= 3 && MONTHS.iter().any(|m| t.starts_with(m)) && t.chars().all(|c| c.is_ascii_alphabetic())
}

fn year(t: &str) -> bool {
    let t = t.trim_matches(|c: char| !c.is_ascii_digit());
    t.len() == 4 && (t.starts_with("19") || t.starts_with("20")) && t.chars().all(|c| c.is_ascii_digit())
}

/// A date in one word: "2026-07-24", "22-Jun-2026", "3/25", "2024/05/01".
fn date_word(t: &str) -> bool {
    let t = t.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | ')' | '('));
    let parts: Vec<&str> = t.split(['-', '/']).collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty()) && parts.iter().any(|p| p.chars().all(|c| c.is_ascii_digit())) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()) || month(p)) && t.chars().any(|c| c.is_ascii_digit())
}

/// The date on a line, starting the search at word `from`: one word, or a month and a year ("April, 2024", "Dec 2011").
fn date_in(toks: &[String], from: usize) -> Option<String> {
    for k in from..toks.len() {
        if date_word(&toks[k]) {
            return Some(toks[k].trim_matches(|c: char| matches!(c, ',' | ';' | '.')).to_string());
        }
        if month(&toks[k]) && toks.get(k + 1).is_some_and(|y| year(y)) {
            return Some(format!("{} {}", toks[k].trim_matches(|c: char| !c.is_alphabetic()), toks[k + 1].trim_matches(|c: char| !c.is_ascii_digit())));
        }
    }
    None
}

/// A literature number with its revision: TI's "SLVS044Z" (four letters, three digits, a letter or two), Microchip's "DS20001826E".
fn literature(t: &str) -> bool {
    // Microchip's "DS20001826E-page 1"
    let t = t.trim_matches(|c: char| !c.is_alphanumeric());
    let t = t.strip_suffix("-page").unwrap_or(t);
    let (letters, rest) = t.split_at(t.find(|c: char| c.is_ascii_digit()).unwrap_or(t.len()));
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    let tail = &rest[digits..];
    let upper = |s: &str| s.chars().all(|c| c.is_ascii_uppercase());
    (letters.len() == 4 && upper(letters) && digits == 3 && (1..=2).contains(&tail.len()) && upper(tail)) || (letters == "DS" && digits >= 5 && tail.len() == 1 && upper(tail))
}

/// The revision and date of the words of a page (its first or last page).
pub fn read(words: &[Word]) -> SheetRevision {
    let mut out = SheetRevision::default();
    for line in crate::figures::lines(words) {
        let toks: Vec<String> = line.iter().map(|w| w.text.trim().to_string()).collect();
        for (k, t) in toks.iter().enumerate() {
            let low = normalize(t);
            let low = low.trim_end_matches([':', '.']);
            if out.rev.is_none() && matches!(low, "rev" | "revision") {
                // the next word, not a date ("Revision: 22-Jun-2026" has none) nor "Rev. 2.1," with its comma
                if let Some(n) = toks.get(k + 1).map(|n| n.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '.')).to_string()).filter(|n| !n.is_empty() && !date_word(n) && n.chars().count() <= 6) {
                    out.rev = Some(n);
                }
                // the date after it, else before it on the line (onsemi "April, 2024 − Rev. 7")
                if out.date.is_none() {
                    out.date = date_in(&toks, k + 1).or_else(|| date_in(&toks[..k], 0));
                }
            }
            // TI: "REVISED APRIL 2025"
            if out.date.is_none() && low == "revised" {
                out.date = date_in(&toks, k + 1);
            }
            if out.rev.is_none() && literature(t) {
                let t = t.trim_matches(|c: char| !c.is_alphanumeric());
                out.rev = Some(t.strip_suffix("-page").unwrap_or(t).to_string());
            }
            // Nexperia: "1 April 2023 Product data sheet", "Date of release: 1 April 2023"
            if out.date.is_none() && (low == "release" || (low == "product" && toks.get(k + 1).is_some_and(|d| normalize(d) == "data"))) {
                out.date = date_in(&toks, k + 1).or_else(|| date_in(&toks[..k], 0));
            }
        }
    }
    out
}

/// Nexperia's edition in its revision history table: the first row's "v.2", with its date as eight digits ("20230401").
pub fn edition(words: &[Word]) -> Option<String> {
    crate::figures::lines(words).into_iter().find_map(|l| {
        let toks: Vec<&str> = l.iter().map(|w| w.text.trim()).collect();
        let dated = toks.iter().any(|t| t.len() == 8 && t.starts_with("20") && t.chars().all(|c| c.is_ascii_digit()));
        let v = toks.iter().find(|t| t.strip_prefix("v.").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.')))?;
        dated.then(|| v.to_string())
    })
}

/// FNV-1a over the file's bytes, as 16 hex digits: tells two files apart without keeping them (not a cryptographic hash).
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The symbols looked for in a sheet's tables to keep with a part: the usual ones of diodes, transistors, MOSFETs, regulators,
/// references and op-amps.
pub const KEY_SYMBOLS: &[&str] = &[
    "VF", "IR", "VBR", "VRRM", "IF", "CT", "trr", "hFE", "VCE(sat)", "VBE(sat)", "VBE(on)", "fT", "Cobo", "VGS(th)", "RDS(on)", "Ciss", "Coss", "Crss", "Qg", "gfs", "VSD", "VO", "VOUT",
    "VDO", "IQ", "IGND", "Vref", "VREF", "IADJ", "VOS", "IB", "IIB", "GBW", "SR", "AVOL", "CMRR", "PSRR", "en",
];

/// One table value kept with a part: its symbol, conditions, min / typ / max in the table's unit, and the page (from 1).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetValue {
    pub symbol: String,
    pub conditions: String,
    pub min: Option<f64>,
    pub typ: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
    pub page: usize,
}

impl SheetValue {
    pub fn from_spec(v: &SpecValue) -> SheetValue {
        SheetValue { symbol: v.symbol.clone(), conditions: v.conditions.clone(), min: v.min, typ: v.typ, max: v.max, unit: v.unit.clone().unwrap_or_default(), page: v.page + 1 }
    }

    /// "min 0.6 / typ 0.7 / max 0.8 V".
    pub fn shown(&self) -> String {
        let f = |n: &str, v: Option<f64>| v.map(|v| format!("{n} {v}"));
        let parts: Vec<String> = [f("min", self.min), f("typ", self.typ), f("max", self.max)].into_iter().flatten().collect();
        format!("{} {}", parts.join(" / "), self.unit).trim().to_string()
    }

    fn same_numbers(&self, o: &SheetValue) -> bool {
        let eq = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= 1e-9 * a.abs().max(1.0),
            (None, None) => true,
            _ => false,
        };
        eq(self.min, o.min) && eq(self.typ, o.typ) && eq(self.max, o.max) && self.unit == o.unit
    }
}

/// The values of `KEY_SYMBOLS` in a sheet: each symbol's rows (three at most, the first ones printed).
pub fn key_values(spec_values: impl Fn(&str) -> Vec<SpecValue>) -> Vec<SheetValue> {
    let mut out: Vec<SheetValue> = vec![];
    for s in KEY_SYMBOLS {
        for v in spec_values(s).iter().take(3) {
            let v = SheetValue::from_spec(v);
            if !out.iter().any(|o| o.symbol == v.symbol && o.conditions == v.conditions && o.same_numbers(&v)) {
                out.push(v);
            }
        }
    }
    out
}

/// What changed between the values kept from an older edition and those of a newer one.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The same symbol and conditions with other numbers.
    Changed { old: SheetValue, new: SheetValue },
    /// Not found in the newer edition (gone, or written another way).
    Missing(SheetValue),
    /// Only in the newer edition.
    Added(SheetValue),
}

/// The values of `old` against `new`, row by row: a row is the same row when its symbol is and its conditions are (written alike,
/// spaces aside); else, for a symbol with one row on each side, those two.
pub fn compare(old: &[SheetValue], new: &[SheetValue]) -> Vec<Change> {
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    let mut used = vec![false; new.len()];
    let mut out = vec![];
    for o in old {
        let same_symbol: Vec<usize> = (0..new.len()).filter(|&k| !used[k] && new[k].symbol.eq_ignore_ascii_case(&o.symbol)).collect();
        let hit = same_symbol.iter().copied().find(|&k| squeeze(&new[k].conditions) == squeeze(&o.conditions)).or_else(|| {
            (same_symbol.len() == 1 && old.iter().filter(|x| x.symbol.eq_ignore_ascii_case(&o.symbol)).count() == 1).then(|| same_symbol[0])
        });
        match hit {
            Some(k) => {
                used[k] = true;
                if !o.same_numbers(&new[k]) {
                    out.push(Change::Changed { old: o.clone(), new: new[k].clone() });
                }
            }
            None => out.push(Change::Missing(o.clone())),
        }
    }
    out.extend(new.iter().zip(&used).filter(|(_, u)| !**u).map(|(n, _)| Change::Added(n.clone())));
    out
}

impl crate::Datasheet {
    /// The revision and date of the sheet: from its first page, else its last (Nexperia and others print it at the end).
    pub fn revision(&self) -> SheetRevision {
        let first = read(self.words(0));
        if first.rev.is_some() && first.date.is_some() || self.page_count() < 2 {
            return first;
        }
        let last = read(self.words(self.page_count() - 1));
        // Nexperia: no "Rev" in the feet, the edition in the revision history near the end
        let rev = first.rev.or(last.rev).or_else(|| (0..self.page_count()).rev().take(4).find_map(|p| edition(self.words(p))));
        SheetRevision { rev, date: first.date.or(last.date) }
    }

    /// The `KEY_SYMBOLS` values of the sheet's tables.
    pub fn key_values(&self) -> Vec<SheetValue> {
        key_values(|s| self.spec_values(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(texts: &[&str]) -> Vec<Word> {
        texts.iter().enumerate().map(|(i, t)| Word::for_test(t, 60.0 + 40.0 * i as f32, 700.0, 95.0 + 40.0 * i as f32, 708.0)).collect()
    }

    #[test]
    fn revisions_and_dates_as_makers_print_them() {
        let r = |t: &[&str]| read(&line(t));
        assert_eq!(r(&["April,", "2024", "−", "Rev.", "7", "BSS138/D"]), SheetRevision { rev: Some("7".into()), date: Some("April 2024".into()) });
        assert_eq!(r(&["Rev.", "2.1,", "2026-07-24"]), SheetRevision { rev: Some("2.1".into()), date: Some("2026-07-24".into()) });
        assert_eq!(r(&["Revision:", "22-Jun-2026", "Document", "Number:", "29044"]), SheetRevision { rev: None, date: Some("22-Jun-2026".into()) });
        assert_eq!(r(&["Rev", "I;", "3/25"]), SheetRevision { rev: Some("I".into()), date: Some("3/25".into()) });
        assert_eq!(r(&["Rev", "3:", "Dec", "2011", "www.aosmd.com"]), SheetRevision { rev: Some("3".into()), date: Some("Dec 2011".into()) });
        assert_eq!(r(&["SLVS044Z", "–", "SEPTEMBER", "1997", "–", "REVISED", "APRIL", "2025"]), SheetRevision { rev: Some("SLVS044Z".into()), date: Some("APRIL 2025".into()) });
        assert_eq!(r(&["DS20001826E-page", "1"]).rev.as_deref(), Some("DS20001826E"), "Microchip's page foot");
        assert_eq!(r(&["1", "April", "2023", "Product", "data", "sheet"]).date.as_deref(), Some("April 2023"), "Nexperia");
        assert_eq!(edition(&line(&["PESD5V0U1BA", "v.2", "20230401", "Product", "data", "sheet", "-", "PESD5V0U1BA", "v.1"])).as_deref(), Some("v.2"));
        assert_eq!(r(&["DS20001826E"]).rev.as_deref(), Some("DS20001826E"));
        assert_eq!(r(&["Reverse", "Battery", "Protection"]), SheetRevision::default(), "a word that starts with rev");
    }

    #[test]
    fn values_of_two_editions_are_compared() {
        let v = |symbol: &str, cond: &str, typ: f64| SheetValue { symbol: symbol.into(), conditions: cond.into(), min: None, typ: Some(typ), max: None, unit: "V".into(), page: 3 };
        let old = [v("VF", "IF = 10 mA", 0.70), v("VF", "IF = 100 mA", 0.85), v("IR", "VR = 75 V", 1.0)];
        let new = [v("VF", "IF=10mA", 0.70), v("VF", "IF = 100 mA", 0.88), v("CT", "VR = 0", 2.0)];
        let c = compare(&old, &new);
        assert_eq!(c.len(), 3, "{c:?}");
        assert!(matches!(&c[0], Change::Changed { old, new } if old.typ == Some(0.85) && new.typ == Some(0.88)));
        assert!(matches!(&c[1], Change::Missing(o) if o.symbol == "IR"));
        assert!(matches!(&c[2], Change::Added(n) if n.symbol == "CT"));
        assert_eq!(fingerprint(b"abc").len(), 16);
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
    }
}
