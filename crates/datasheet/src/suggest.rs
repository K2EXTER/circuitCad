//! Guesses from the first pages of a data sheet: what the part is called and what kind of part it is. They are
//! offered as candidates for a person to pick from, never filled in by themselves.

use crate::text::{Word, normalize};

/// Most candidates offered.
const MAX_NAMES: usize = 8;

/// Does the text look like a part number: "LM393B", "2N3904", "AO3400A", "74HC14"? Letters and digits mixed, not a
/// value ("30V", "5.7A"), not a package ("SOT-23", "TSSOP-8") and not a chip size ("0603").
pub fn looks_like_part_number(t: &str) -> bool {
    if !(3..=24).contains(&t.len()) || !t.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c)) {
        return false;
    }
    if !t.chars().any(|c| c.is_ascii_alphabetic()) || !t.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    // a number and a unit
    let digits_end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    if digits_end > 0 && t.len() - digits_end <= 3 && t[digits_end..].chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    // "8-lead", "3.8-V", "85-value": a number, a hyphen and a word; "28-May-2026": a date
    if digits_end > 0 && t[digits_end..].starts_with('-') && t[digits_end + 1..].chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    // (ROHM writes it with dots: "18.Nov.2013")
    if t.len() > 5 && t[t.len() - 5..].starts_with(['-', '.', '/']) && t[t.len() - 4..].chars().all(|c| c.is_ascii_digit()) && matches!(&t[t.len() - 4..t.len() - 2], "19" | "20") {
        return false;
    }
    // "1.Specific", "2.Unsuitable": a numbered heading
    if digits_end > 0 && t[digits_end..].chars().all(|c| c.is_ascii_alphabetic()) && t[..digits_end].ends_with('.') {
        return false;
    }
    let up = t.to_ascii_uppercase();
    // standards and document marks ("MIL-PRF-38535" on TI's first pages, "IATF16949", "Fig.1", ROHM's "Table1")
    if ["AEC", "REV.", "REV-", "ISO", "IEC", "JESD", "JEDEC", "ROHS", "MIL-", "IPC-", "EIA-", "ANSI", "IEEE", "IATF", "CECC", "FIG.", "FIG-", "TABLE"].iter().any(|p| up.starts_with(p)) {
        return false;
    }
    // UL and a number ("UL94", "UL1577"), not ULN2003
    if up.strip_prefix("UL").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
        return false;
    }
    // the E series of values ("E24", "E96")
    if matches!(up.as_str(), "E3" | "E6" | "E12" | "E24" | "E48" | "E96" | "E192") {
        return false;
    }
    const PACKAGES: &[&str] = &[
        "SOT", "SOIC", "SOP", "SSOP", "TSSOP", "VSSOP", "MSOP", "DIP", "PDIP", "QFN", "VQFN", "WQFN", "DFN", "WSON", "QFP", "TQFP", "LQFP", "BGA", "TO-", "DO-", "SC-",
        "SMA", "SMB", "SMC", "LGA", "SON", "DPAK", "D2PAK",
    ];
    !PACKAGES.iter().any(|p| up.starts_with(p) && up[p.len()..].starts_with(|c: char| c.is_ascii_digit() || c == '-'))
}

fn trim_name(t: &str) -> &str {
    t.trim_matches(|c: char| "-._".contains(c))
}

/// Part numbers that the first page may be about, the likeliest first: those in the largest lettering (the title),
/// then those that come up again and again on the page.
pub fn name_candidates(page0: &[Word]) -> Vec<String> {
    let parts = |w: &Word| -> Vec<String> { w.tokens().map(trim_name).filter(|t| looks_like_part_number(t)).map(String::from).collect() };
    let biggest = page0.iter().filter(|w| !parts(w).is_empty()).map(|w| w.em).fold(0.0_f32, f32::max);
    let mut out: Vec<String> = vec![];
    let add = |out: &mut Vec<String>, t: String| {
        if out.len() < MAX_NAMES && !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
    };
    for w in page0.iter().filter(|w| w.em >= 0.8 * biggest) {
        for t in parts(w) {
            add(&mut out, t);
        }
    }
    // the others, by how often they appear
    let mut seen: Vec<(String, usize)> = vec![];
    for w in page0 {
        for t in parts(w) {
            match seen.iter_mut().find(|(s, _)| s.eq_ignore_ascii_case(&t)) {
                Some((_, n)) => *n += 1,
                None => seen.push((t, 1)),
            }
        }
    }
    seen.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (t, n) in seen {
        if n >= 2 {
            add(&mut out, t);
        }
    }
    out
}

/// (category name used in the part list, words that point to it). The first of equal scores wins, so the more
/// specific kinds come first. A word that also has other meanings ("gate" of a MOSFET) is not used alone.
const KINDS: &[(&str, &[&str])] = &[
    ("コンパレータ", &["comparator", "コンパレータ"]),
    ("オペアンプ", &["operational amplifier", "op amp", "op-amp", "オペアンプ", "演算増幅器"]),
    ("レギュレータ", &["regulator", "low dropout", "low-dropout", "converter", "step-down", "buck", "dc-dc", "レギュレータ", "コンバータ", "降圧", "スイッチングレギュレータ"]),
    ("フォトカプラ", &["photocoupler", "optocoupler", "opto-coupler", "optoisolator", "opto-isolator", "photo coupler", "フォトカプラ"]),
    ("フォトダイオード", &["photodiode", "photo diode", "フォトダイオード"]),
    ("ダーリントン", &["darlington", "ダーリントン"]),
    ("MOSFET", &["mosfet", "mos fet", "power mos", "n-channel", "p-channel", "enhancement mode", " fet "]),
    ("ツェナーダイオード", &["zener", "ツェナー"]),
    ("ダイオード", &["rectifier", "schottky", "diode", "ダイオード", "整流"]),
    ("トランジスタ", &["transistor", "トランジスタ"]),
    ("アナログスイッチ", &["analog switch", "multiplexer", "mux", "spst", "spdt", "アナログスイッチ", "マルチプレクサ"]),
    ("ロジックIC", &["logic gate", "logic family", "logic ic", "cmos logic", "flip-flop", "schmitt", "nand", "counter", "ロジック", "カウンタ"]),
    ("マイコン", &["microcontroller", "mcu", "マイコン", "マイクロコントローラ"]),
];

/// How often `key` (a word of kind `kind`) is in `text`, leaving out where it is part of a longer word of another kind:
/// a "photodiode" is not a "diode", a "phototransistor" (in an optocoupler's sheet) not a "transistor".
fn hits(text: &str, key: &str, kind: &str) -> usize {
    let inside: usize = KINDS
        .iter()
        .filter(|(other, _)| *other != kind)
        .flat_map(|(_, keys)| keys.iter())
        .filter(|longer| longer.len() > key.len() && longer.contains(key))
        .map(|longer| text.matches(longer).count())
        .sum();
    let photo_transistor = if key == "transistor" { text.matches("phototransistor").count() + text.matches("photo transistor").count() } else { 0 };
    text.matches(key).count().saturating_sub(inside + photo_transistor)
}

/// A word in the title's lettering (at least this part of the biggest on the first page) counts this many times
/// more than one in the text: a part is what its title says it is.
const TITLE_WEIGHT: usize = 4;

/// The kind of part the first pages talk about most, as a category name. None when nothing points to a kind.
pub fn category_hint(first_pages: &[&[Word]]) -> Option<&'static str> {
    // words are split at spaces, so a phrase is looked for in the text joined with spaces
    let join = |words: Vec<&Word>| words.iter().map(|w| normalize(&w.text)).collect::<Vec<_>>().join(" ");
    let all = join(first_pages.iter().flat_map(|p| p.iter()).collect());
    let page0 = first_pages.first().copied().unwrap_or(&[]);
    let biggest = page0.iter().map(|w| w.em).fold(0.0_f32, f32::max);
    let title = join(page0.iter().filter(|w| w.em >= 0.6 * biggest).collect());
    KINDS
        .iter()
        .map(|(name, keys)| (*name, keys.iter().map(|k| hits(&all, k, name) + TITLE_WEIGHT * hits(&title, k, name)).sum::<usize>()))
        .filter(|(_, n)| *n > 0)
        .fold(None, |best: Option<(&str, usize)>, c| if best.is_none_or(|b| c.1 > b.1) { Some(c) } else { best })
        .map(|(name, _)| name)
        // a Darlington's sheet says "transistor" far more often than "Darlington"; another transistor's says the
        // word not at all
        .map(|name| if name == "トランジスタ" && all.contains("darlington") { "ダーリントン" } else { name })
        // nothing else: "PNP Silicon" on a sheet whose "transistor" the OCR misspelt (a scan)
        .or_else(|| ["npn", "pnp", "bipolar"].iter().any(|k| all.split(|c: char| !c.is_alphanumeric()).any(|w| w == *k)).then_some("トランジスタ"))
}

/// Whether a transistor is a P type: "PNP" or "P-Channel" said more often than "NPN" or "N-Channel" on the first
/// pages (a sheet for a complementary pair says both: the more frequent, which is the first part's, wins; a tie is N).
/// None when neither is said.
pub fn p_type(first_pages: &[&[Word]]) -> Option<bool> {
    let text: String = first_pages.iter().flat_map(|p| p.iter()).map(|w| normalize(&w.text)).collect::<Vec<_>>().join(" ");
    let words: Vec<&str> = text.split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).collect();
    let count = |keys: &[&str]| words.iter().filter(|w| keys.contains(w)).count() + keys.iter().filter(|k| k.contains(' ')).map(|k| text.matches(*k).count()).sum::<usize>();
    let p = count(&["pnp", "p-channel", "p-ch", "p channel"]);
    let n = count(&["npn", "n-channel", "n-ch", "n channel"]);
    (p + n > 0).then_some(p > n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_inside_a_longer_word_of_another_kind_does_not_count() {
        let text = "silicon pin photodiode. the photodiode is a diode for light";
        assert_eq!((hits(text, "diode", "ダイオード"), hits(text, "photodiode", "フォトダイオード")), (1, 2));
        let opto = "photocoupler: an infrared emitting diode and a phototransistor. transistor output";
        assert_eq!(hits(opto, "transistor", "トランジスタ"), 1);
    }

    #[test]
    fn a_misspelt_transistor_is_told_by_npn_or_pnp() {
        let page = vec![Word::for_test("General", 0.0, 0.0, 30.0, 8.0), Word::for_test("Tansistors", 32.0, 0.0, 80.0, 8.0), Word::for_test("PNP", 0.0, 10.0, 15.0, 18.0), Word::for_test("Silicon", 17.0, 10.0, 50.0, 18.0)];
        assert_eq!(category_hint(&[&page]), Some("トランジスタ"));
        let regulator = vec![Word::for_test("Voltage", 0.0, 0.0, 30.0, 8.0), Word::for_test("Regulator", 32.0, 0.0, 80.0, 8.0), Word::for_test("NPN", 0.0, 10.0, 15.0, 18.0)];
        assert_ne!(category_hint(&[&regulator]), Some("トランジスタ"), "another kind named: that one");
    }

    #[test]
    fn a_p_type_is_told_by_what_the_sheet_says() {
        let page = |t: &str| t.split(' ').enumerate().map(|(i, w)| Word::for_test(w, 10.0 * i as f32, 0.0, 10.0 * i as f32 + 8.0, 8.0)).collect::<Vec<_>>();
        assert_eq!(p_type(&[&page("General Purpose Transistors PNP Silicon")]), Some(true));
        assert_eq!(p_type(&[&page("NPN Silicon Transistor")]), Some(false));
        assert_eq!(p_type(&[&page("P-Channel Enhancement Mode MOSFET")]), Some(true));
        assert_eq!(p_type(&[&page("Darlington NPN TIP120 NPN TIP125 PNP")]), Some(false), "a pair: the more frequent");
        assert_eq!(p_type(&[&page("Op amp")]), None);
    }

    #[test]
    fn part_numbers_are_told_from_values_and_packages() {
        for ok in ["LM393B", "2N3904", "AO3400A", "74HC14", "ADS124S08", "1N4148", "MCP6001-E", "ULN2003A", "E3V-L"] {
            assert!(looks_like_part_number(ok), "{ok}");
        }
        for no in ["30V", "5.7A", "SOT-23", "SOIC-8", "TSSOP14", "0603", "LM", "123", "DO-41", "Dual", "10uF", "8-lead", "3.8-V", "28-May-2026", "AEC-Q200", "Rev.001", "MIL-PRF-38535", "MIL-R-22684", "IATF16949", "UL94", "E24", "E96", "Fig.1", "1.Specific", "18.Nov.2013", "Table1"] {
            assert!(!looks_like_part_number(no), "{no}");
        }
    }
}
