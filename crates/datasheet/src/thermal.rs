//! The heat figures of a data sheet: the thermal characteristics table's thermal resistances (junction to ambient, to case, to
//! lead) and the absolute maximum ratings' power derating ("Derate above 25°C 5.0 mW/°C": a thermal resistance of
//! 1 / 5.0 mW/°C = 200 °C/W, to the ambient after a "TA = 25°C" power, to the case after a "TC = 25°C" one).
//!
//! TI's "Thermal Information" table has a column per package ("D (SOIC)", "DGK (VSSOP)" …): each value is kept with the
//! column's heading (`ThermalRow::cells`), so the part's own package can be chosen (`ThermalRow::for_package`, 候補 Q2). Its
//! junction-to-board thermal resistance (RθJB) and characterization parameters (ψJT: junction to the package's top, to work out
//! the junction's temperature from the top's measured one; ψJB) are read too. A row's note ("RθJA (Note 1)", "(1)") is added
//! to its text, as the board it was measured on is often said only there.
//!
//! The θ in front of a symbol is often in a symbol font that gives no letter ("R JA"), so a row is known by its words
//! ("Thermal Resistance, Junction−to−Ambient") as well as by its symbol (RθJA, RthJA).

use crate::figures::lines;
use crate::text::{Word, normalize};

/// Where a thermal resistance goes from the junction to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    Ambient,
    Case,
    Lead,
    /// Junction to the board (RθJB).
    Board,
    /// The junction-to-top characterization parameter (ψJT): the junction's temperature over the package top's, per watt.
    PsiTop,
    /// The junction-to-board characterization parameter (ψJB).
    PsiBoard,
}

/// One thermal resistance read off a page.
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalRow {
    pub page: usize,
    pub to: To,
    /// K/W (= °C/W).
    pub k_per_w: f64,
    /// The row as printed (its conditions: the board it was measured on, a note number).
    pub text: String,
    /// From a "Derate above" row (1 / the derating) rather than the thermal table.
    pub from_derating: bool,
    /// For a derating row: the temperature the power starts to fall from (°C).
    pub derate_from: Option<f64>,
    /// How many value cells the row has before its unit (numbers and dashes): 3 or more is a table with a column per package
    /// (TI's "Thermal Information"), where the value taken (the last) need not be the part's.
    pub columns: usize,
    /// The values are one per package (several of them), so the one taken need not be the part's: 3 or more values, or 2 in TI's
    /// "Thermal Information" table ("RθJA Junction-to-ambient thermal resistance 59.6 41.0", its columns being packages; another
    /// maker's two values are a typ and a max).
    pub per_package: bool,
    /// A row with a value per package: each value (None for a dash) with its column's heading ("D (SOIC)"), when the heading
    /// was found above the row.
    pub cells: Vec<(String, Option<f64>)>,
}

impl ThermalRow {
    /// The row's value for a package family ("soic", "to-220"): the column whose heading names it, as a row of one value
    /// with the heading said in its text. None when no heading names it, or its cell is a dash.
    pub fn for_package(&self, family: &str) -> Option<ThermalRow> {
        let names = |h: &str| h.split(|c: char| c.is_whitespace() || c == '(' || c == ')').any(|t| crate::draft::package_of(t).is_some_and(|(f, _)| same_family(f, family)));
        let hits: Vec<&(String, Option<f64>)> = self.cells.iter().filter(|(h, _)| names(h)).collect();
        let (head, v) = hits.first()?;
        // two columns of one family (LM317's KCS and KCT TO-220s) with different values: not this one to choose
        if hits.iter().any(|(_, x)| x != v) {
            return None;
        }
        Some(ThermalRow { k_per_w: (*v)?, text: format!("{} [{head} の列]", self.text), columns: 1, per_package: false, cells: vec![], ..self.clone() })
    }
}

/// Two package families that are one: equal, or a plastic DIP ("PDIP") and a DIP.
fn same_family(a: &str, b: &str) -> bool {
    a == b || matches!((a, b), ("pdip", "dip") | ("dip", "pdip"))
}

/// TI's package designators and their families (its "D (SOIC)", "DBV (SOT-23)"), for a table that prints the codes only
/// (TL431's "P PW D PS DCK DBV DBZ LP PK").
const TI_CODES: &[(&str, &str)] = &[
    ("D", "soic"),
    ("P", "pdip"),
    ("N", "pdip"),
    ("PW", "tssop"),
    ("DGK", "vssop"),
    ("DGN", "msop"),
    ("PS", "so"),
    ("NS", "so"),
    ("DBV", "sot-23"),
    ("DBZ", "sot-23"),
    ("DDF", "sot-23"),
    ("DCK", "sc-70"),
    ("LP", "to-92"),
    ("PK", "sot-89"),
    ("DCY", "sot-223"),
    ("KCS", "to-220"),
    ("KCT", "to-220"),
    ("KTT", "to-263"),
    ("DQN", "x2son"),
];

/// The headings of a table's package columns above line `i`, each with its centre's x: a line with package names in
/// brackets ("(SOIC) (VSSOP) (PDIP)", two or more of them known) joined with the codes on one of the two lines over it ("D DGK
/// P"), or a line of three or more of TI's package codes alone, named by `TI_CODES`.
fn column_heads(lines: &[Vec<&Word>], i: usize) -> Vec<(String, f32)> {
    let centre = |w: &Word| (w.rect.x0 + w.rect.x1) / 2.0;
    let y = lines[i][0].baseline();
    let code_of = |w: &Word| TI_CODES.iter().find(|(c, _)| *c == w.text.trim()).map(|(_, f)| *f);
    // the codes: short upper case words within 16 points over the names' line
    let codes_near = |k: usize| -> Vec<&Word> {
        let yk = lines[k][0].baseline();
        (k.saturating_sub(4)..k).filter(|j| yk - lines[*j][0].baseline() < 16.0).flat_map(|j| lines[j].iter().copied()).filter(|w| { let t = w.text.trim(); !t.is_empty() && t.len() <= 4 && t.chars().all(|c| c.is_ascii_uppercase()) }).collect()
    };
    for k in (i.saturating_sub(14)..i).rev() {
        if y - lines[k][0].baseline() > 160.0 {
            break;
        }
        let bracketed: Vec<&&Word> = lines[k].iter().filter(|w| { let t = w.text.trim(); t.starts_with('(') && t.ends_with(')') && t.len() > 3 }).collect();
        if bracketed.iter().filter(|w| crate::draft::package_of(w.text.trim()).is_some()).count() >= 2 {
            let codes = codes_near(k);
            return bracketed
                .iter()
                .map(|n| {
                    let code = codes.iter().filter(|c| (centre(c) - centre(n)).abs() < 25.0).min_by(|a, b| (centre(a) - centre(n)).abs().total_cmp(&(centre(b) - centre(n)).abs()));
                    (code.map_or(n.text.trim().to_string(), |c| format!("{} {}", c.text.trim(), n.text.trim())), centre(n))
                })
                .collect();
        }
        let codes: Vec<&&Word> = lines[k].iter().filter(|w| code_of(w).is_some()).collect();
        if codes.len() >= 3 {
            return codes.iter().map(|c| (format!("{} ({})", c.text.trim(), code_of(c).unwrap_or("").to_uppercase()), centre(c))).collect();
        }
    }
    vec![]
}

/// The notes a row points to ("(1)", "[1]", "Note 1", "Notes 1, 2"), by number.
fn note_marks(toks: &[String]) -> Vec<u32> {
    let mut out = vec![];
    for (k, t) in toks.iter().enumerate() {
        let inner = t.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | ',' | ':'));
        let bracketed = (t.starts_with('(') && t.ends_with(')')) || (t.starts_with('[') && t.ends_with(']'));
        if bracketed && let Ok(n) = inner.parse::<u32>() && (1..=20).contains(&n) {
            out.push(n);
        }
        if matches!(t.trim_start_matches('('), "note" | "notes") && let Some(n) = toks.get(k + 1).and_then(|n| n.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().ok()) {
            out.push(n);
        }
    }
    out
}

/// The text of note `n` on the page: a line starting "(n)", "n.", "[n]" or "Note n" ("Note 1: Surface mounted on FR-4 …").
fn note_text(lines: &[Vec<String>], n: u32) -> Option<String> {
    let s = n.to_string();
    lines.iter().find_map(|l| {
        let first = l.first()?;
        let start = if *first == format!("({s})") || *first == format!("[{s}]") || *first == format!("{s}.") || *first == format!("{s}:") {
            1
        } else if matches!(first.as_str(), "note" | "notes") && l.get(1).is_some_and(|x| x.trim_matches(|c: char| !c.is_ascii_digit()) == s) {
            2
        } else {
            return None;
        };
        let rest = l[start..].join(" ");
        (rest.len() > 8).then_some(rest)
    })
}

/// A plain number ("200", "0.52"), not a word with digits in it.
fn number(t: &str) -> Option<f64> {
    let t = t.trim_matches(|c: char| c == '(' || c == ')' || c == ',');
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    t.parse().ok()
}

/// The factor to K/W of a thermal resistance unit ("°C/W", "K/W", "℃/W"); None for another word.
fn per_watt(t: &str) -> bool {
    let t: String = t.chars().filter(|c| !matches!(c, '°' | '˚' | 'º' | '℃')).collect();
    matches!(t.as_str(), "c/w" | "k/w" | "/w")
}

/// W/K of a derating unit ("mW/°C" 1e-3, "W/°C" 1); None for another word.
fn per_kelvin(t: &str) -> Option<f64> {
    let t: String = t.chars().filter(|c| !matches!(c, '°' | '˚' | 'º' | '℃')).collect();
    match t.as_str() {
        "mw/c" | "mw/k" | "mw/" => Some(1e-3),
        "w/c" | "w/k" | "w/" => Some(1.0),
        _ => None,
    }
}

/// Where a row goes, by its words ("junction-to-ambient", "junction/ambient", "junction to ambient air from case": the first
/// after "junction") or its symbol ("RθJA", "RthJC", "θJL").
fn to_of(tokens: &[String]) -> Option<To> {
    let joined = tokens.join(" ").replace(['−', '–', '-', '/', ','], " ");
    let words: Vec<&str> = joined.split_whitespace().collect();
    let psi = words.iter().any(|w| w.starts_with("characteri"));
    if let Some(j) = words.iter().position(|w| w.starts_with("junction")) {
        for w in &words[j + 1..] {
            match *w {
                w if w.starts_with("ambient") => return Some(To::Ambient),
                w if w.starts_with("case") => return Some(To::Case),
                w if w.starts_with("lead") => return Some(To::Lead),
                w if w.starts_with("top") && psi => return Some(To::PsiTop),
                w if w.starts_with("board") => return Some(if psi { To::PsiBoard } else { To::Board }),
                _ => {}
            }
        }
    }
    for w in &words {
        // TI's "ψJT", "ψJB"
        if let Some(rest) = w.strip_prefix('ψ').or_else(|| w.strip_prefix("psi")) {
            match rest {
                "jt" => return Some(To::PsiTop),
                "jb" => return Some(To::PsiBoard),
                _ => {}
            }
        }
        // "rθja", "rthja", "θja", the θ lost ("rja") or in a symbol font as "q" ("rqjl")
        let Some(rest) = w.strip_prefix('r').or_else(|| w.starts_with('θ').then_some(*w)) else { continue };
        let s = rest.strip_prefix("th").or_else(|| rest.strip_prefix('θ')).or_else(|| rest.strip_prefix('ϑ')).or_else(|| rest.strip_prefix('q')).unwrap_or(rest);
        {
            match s.split('(').next().unwrap_or(s) {
                "ja" => return Some(To::Ambient),
                "jc" => return Some(To::Case),
                "jl" => return Some(To::Lead),
                "jb" => return Some(To::Board),
                _ => {}
            }
        }
    }
    None
}

/// The thermal resistances of one page, in the order printed.
pub fn read(page: usize, words: &[Word]) -> Vec<ThermalRow> {
    let placed = lines(words);
    let lines: Vec<Vec<String>> = placed.iter().map(|l| l.iter().map(|w| normalize(&w.text)).collect()).collect();
    let mut out = vec![];
    for (i, toks) in lines.iter().enumerate() {
        let text = toks.join(" ");
        // the Japanese derating, in a note under the table ("Ta≧25℃の場合は、9.6mW/℃で軽減。", ROHM): the slope and its unit
        // in one word with the text after it
        if text.contains("軽減") || text.contains("低減") || text.contains("減じ") {
            let slope = toks.iter().find_map(|t| {
                let at = t.find("mw/").or_else(|| t.find("w/"))?;
                let digits: String = t[..at].chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
                let unit: String = t[at..].chars().take_while(|c| !matches!(c, 'で' | '、' | '。' | ')' | ' ')).collect();
                Some(digits.parse::<f64>().ok()? * per_kelvin(&unit)?)
            });
            if let Some(slope) = slope.filter(|s| *s > 0.0) {
                // "Ta≧25℃": the temperature the power starts to fall from
                let after = |name: &str| {
                    text.match_indices(name).find_map(|(i, _)| {
                        let rest = text[i + name.len()..].trim_start();
                        let rest = rest.strip_prefix(['≧', '≥', '>', '='])?;
                        let d: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                        d.parse::<f64>().ok()
                    })
                };
                let case = after("tc").is_some();
                let from = after("tc").or_else(|| after("ta"));
                out.push(ThermalRow { page, to: if case { To::Case } else { To::Ambient }, k_per_w: 1.0 / slope, text: text.clone(), from_derating: true, derate_from: from, columns: 1, per_package: false, cells: vec![] });
                continue;
            }
        }
        if text.contains("derat") && text.contains("above") {
            // "Derate above 25°C 5.0 mW/°C": the slope before its unit, the temperature after "above"
            let Some(u) = toks.iter().position(|t| per_kelvin(t).is_some()) else { continue };
            let Some(slope) = toks[..u].iter().rev().find_map(|t| number(t)).map(|x| x * per_kelvin(&toks[u]).unwrap_or(1.0)) else { continue };
            let from = toks.iter().position(|t| t == "above").and_then(|a| toks.get(a + 1)).and_then(|t| {
                let d: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                d.parse::<f64>().ok()
            });
            if slope <= 0.0 {
                continue;
            }
            // to the case when the power above it is given at the case's temperature ("PD @ TC = 25°C")
            let case = (i.saturating_sub(3)..=i).rev().find_map(|k| {
                let t: Vec<String> = lines[k].iter().map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).collect();
                if t.iter().any(|t| t == "tc" || t == "case") {
                    Some(true)
                } else if t.iter().any(|t| t == "ta" || t == "tamb" || t == "ambient") {
                    Some(false)
                } else {
                    None
                }
            });
            out.push(ThermalRow { page, to: if case == Some(true) { To::Case } else { To::Ambient }, k_per_w: 1.0 / slope, text: text.clone(), from_derating: true, derate_from: from, columns: 1, per_package: false, cells: vec![] });
            continue;
        }
        let symbol = toks.iter().any(|t| {
            let t = t.trim_matches(|c: char| !c.is_alphanumeric());
            // "rθjc(top)" too
            ["rθja", "rθjc", "rθjl", "rθjb", "rthja", "rthjc", "rthjl", "rthjb", "θja", "θjc", "θjl", "θjb", "rja", "rjc", "rjl", "rqja", "rqjc", "rqjl", "ψjt", "ψjb", "psijt", "psijb"].iter().any(|s| t == *s || t.strip_prefix(s).is_some_and(|r| r.starts_with('(')))
        });
        // a symbol, or the words "junction … ambient / case / lead" (AO3400A's "Maximum Junction-to-Ambient Steady-State",
        // IRF540N's "Junction-to-Case" with its symbol on a line of its own); with a unit on it or near it below
        if !(symbol || text.contains("junction")) {
            continue;
        }
        // a short time's thermal resistance ("t ≤ 10s", AO3400A) is not the steady one
        if toks.iter().any(|t| t.contains('≤')) {
            continue;
        }
        // the unit on the row, or in the table's unit column printed on a line near it (Infineon's IRF540N, Vishay's SS12)
        let u = match toks.iter().position(|t| per_watt(t)) {
            Some(u) => u,
            None if lines[i.saturating_sub(3)..(i + 4).min(lines.len())].iter().any(|l| l.iter().any(|t| per_watt(t))) => toks.len(),
            None => continue,
        };
        // the value nearest before the unit (the max of "typ max"), not a note's number ("Note 1")
        let Some(v) = (0..u).rev().take(2).find_map(|k| if k > 0 && toks[k - 1].trim_start_matches('(') == "note" { None } else { number(&toks[k]) }) else { continue };
        let Some(to) = to_of(toks) else { continue };
        let columns = toks[..u].iter().filter(|t| number(t).is_some() || matches!(t.as_str(), "-" | "—" | "–")).count();
        // TI's words: "junction-to-ambient thermal resistance", the kind before "thermal resistance"
        let ti = matches!((text.find("junction-to"), text.find("thermal resistance")), (Some(j), Some(t)) if j < t);
        let per_package = columns >= 3 || (columns >= 2 && ti);
        // a value per package: each with its column's heading
        let cells = if columns >= 2 {
            let heads = column_heads(&placed, i);
            let centre = |w: &Word| (w.rect.x0 + w.rect.x1) / 2.0;
            let head = |x: f32| heads.iter().filter(|h| (h.1 - x).abs() < 30.0).min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map(|h| h.0.clone());
            placed[i][..u.min(placed[i].len())]
                .iter()
                .zip(&toks[..u])
                .filter(|(_, t)| number(t).is_some() || matches!(t.as_str(), "-" | "—" | "–"))
                .filter_map(|(w, t)| head(centre(w)).map(|h| (h, number(t))))
                .collect()
        } else {
            vec![]
        };
        // the notes it points to, in its text ("RθJA (Note 1) 200 °C/W" and "1. Mounted on 1 in² of copper")
        let notes: Vec<String> = note_marks(toks).into_iter().filter_map(|n| note_text(&lines, n)).collect();
        let text = if notes.is_empty() { text } else { format!("{text} / 注: {}", notes.join(" / ")) };
        if v > 0.0 {
            out.push(ThermalRow { page, to, k_per_w: v, text, from_derating: false, derate_from: None, columns, per_package, cells });
        }
    }
    out
}

/// What a junction-to-ambient row says about the board it was measured on: true for a large board or added copper
/// (JEDEC's 2s2p "high-K" board, "1 in²" of copper), where the part alone on its smallest pads runs hotter. TI's "Thermal
/// Information" table ("RθJA Junction-to-ambient thermal resistance", the words in that order) is on JEDEC's high-K board, as its
/// note says.
pub fn on_large_copper(text: &str) -> bool {
    let t = normalize(text).replace(['−', '–'], "-");
    ["2s2p", "high-k", "high k", "51-7", "in2", "in²", "inch", "in sq", "sq in", "sq. in", "cm2", "cm²", "copper area", "2 oz", "1 oz", "heatsink", "heat sink", "ambient thermal resistance"].iter().any(|k| t.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(texts: &[&str], y: f32) -> Vec<Word> {
        texts.iter().enumerate().map(|(i, t)| Word::for_test(t, 60.0 + 40.0 * i as f32, y, 95.0 + 40.0 * i as f32, y + 8.0)).collect()
    }

    #[test]
    fn the_thermal_table_and_the_derating_give_the_resistances() {
        // onsemi's TIP120: the power at the case, then at the ambient, each with its derating; the table with the θ lost
        let mut w = line(&["Total", "Power", "Dissipation", "@", "TC", "=", "25°C", "65", "W"], 100.0);
        w.extend(line(&["Derate", "above", "25°C", "0.52", "W/°C"], 115.0));
        w.extend(line(&["Total", "Power", "Dissipation", "@", "TA", "=", "25°C", "2.0", "W"], 130.0));
        w.extend(line(&["Derate", "above", "25°C", "0.016", "W/°C"], 145.0));
        w.extend(line(&["R", "Thermal", "Resistance,", "Junction−to−Case", "1.92", "°C/W"], 200.0));
        w.extend(line(&["R", "Thermal", "Resistance,", "Junction−to−Ambient", "62.5", "°C/W"], 215.0));
        let r = read(1, &w);
        let got: Vec<(To, f64, bool)> = r.iter().map(|r| (r.to, (r.k_per_w * 100.0).round() / 100.0, r.from_derating)).collect();
        assert_eq!(got, vec![(To::Case, 1.92, true), (To::Ambient, 62.5, true), (To::Case, 1.92, false), (To::Ambient, 62.5, false)]);
        assert_eq!(r[1].derate_from, Some(25.0));
        // Vishay's 1N4148 and 1N5400: the words, or only the symbol; "from case" after "ambient" is a condition
        let w = line(&["Thermal", "resistance", "junction", "to", "ambient", "air", "from", "case", "350", "K/W", "RthJA"], 100.0);
        assert_eq!(read(0, &w)[0].to, To::Ambient);
        let w = line(&["Typical", "thermal", "resistance", "RθJA", "20", "°C/W"], 100.0);
        assert_eq!((read(0, &w)[0].to, read(0, &w)[0].k_per_w), (To::Ambient, 20.0));
        // a line with a junction and no thermal resistance unit is not a row
        assert!(read(0, &line(&["Junction", "Temperature", "150", "°C"], 100.0)).is_empty());
        // TI's table: a column per package
        let w = line(&["RθJA", "Junction-to-ambient", "thermal", "resistance", "139.5", "156.7", "149.4", "°C/W"], 100.0);
        assert_eq!(read(0, &w)[0].columns, 3);
        // TI's two packages (LM317's new chip) are packages too; another maker's "typ max" is not (AO3400A)
        let w = line(&["Rθ(JA)", "Junction-to-ambient", "thermal", "resistance", "59.6", "41.0", "°C/W"], 100.0);
        assert!(read(0, &w)[0].per_package);
        let w = line(&["Maximum", "Junction-to-Ambient", "Steady-State", "100", "125", "°C/W"], 100.0);
        assert!(!read(0, &w)[0].per_package);
    }

    #[test]
    fn a_large_board_is_told_from_the_smallest_pads() {
        assert!(on_large_copper("Thermal Resistance, Junction-to-Ambient (Note 1: JEDEC 2s2p)"));
        assert!(on_large_copper("Mounted on 1 in2 FR-4 pad"));
        assert!(!on_large_copper("Thermal Resistance, Junction-to-Ambient 200 °C/W"));
        assert!(on_large_copper("RθJA Junction-to-ambient thermal resistance 97.1 °C/W"), "TI's table");
    }

    /// TI's "Thermal Information": a column per package, headed by its code and name over the values; ψJT and RθJB read,
    /// and the SOIC's column chosen (LM358's table, 候補 Q2).
    #[test]
    fn a_column_per_package_is_chosen_by_its_heading() {
        let at = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 6.0 * t.chars().count() as f32, y + 8.0);
        let mut w = vec![at("THERMAL", 85.0, 330.0), at("METRIC", 122.0, 330.0), at("D", 202.0, 330.0), at("DGK", 239.0, 330.0), at("P", 287.0, 330.0), at("UNIT", 530.0, 330.0)];
        w.extend([at("(SOIC)", 194.0, 340.0), at("(VSSOP)", 232.0, 340.0), at("(PDIP)", 279.0, 340.0)]);
        w.extend([at("Junction-to-ambient", 101.0, 362.0), at("124.7", 196.0, 370.0), at("181.4", 238.0, 370.0), at("80.9", 282.0, 370.0), at("°C/W", 530.0, 370.0), at("RθJA", 59.0, 370.0)]);
        w.extend([at("characterization", 101.0, 437.0), at("19.2", 198.0, 437.0), at("11.8", 240.0, 437.0), at("40", 285.0, 437.0), at("°C/W", 530.0, 437.0), at("ψJT", 59.0, 437.0)]);
        w.extend([at("67.9", 198.0, 412.0), at("102.9", 238.0, 412.0), at("—", 282.0, 412.0), at("°C/W", 530.0, 412.0), at("RθJB", 59.0, 412.0)]);
        let r = read(4, &w);
        let ja = r.iter().find(|r| r.to == To::Ambient).unwrap();
        assert!(ja.per_package && ja.cells.len() == 3 && ja.cells[1].0 == "DGK (VSSOP)", "{:?}", ja.cells);
        let soic = ja.for_package("soic").unwrap();
        assert!(soic.k_per_w == 124.7 && !soic.per_package && soic.text.contains("D (SOIC)"), "{soic:?}");
        assert!(ja.for_package("to-220").is_none());
        let jt = r.iter().find(|r| r.to == To::PsiTop).unwrap();
        assert_eq!(jt.for_package("dip").map(|r| r.k_per_w), Some(40.0));
        let jb = r.iter().find(|r| r.to == To::Board).unwrap();
        assert!(jb.for_package("dip").is_none(), "a dash is no value");
    }

    /// TL431's table prints the codes alone ("P PW D … LP PK"): TI's codes name the families; LM317's two TO-220 columns with
    /// different values are not chosen between.
    #[test]
    fn ti_codes_alone_name_the_columns() {
        let at = |t: &str, x: f32, y: f32| Word::for_test(t, x, y, x + 6.0 * t.chars().count() as f32, y + 8.0);
        let mut w = vec![at("THERMAL", 76.0, 425.0), at("P", 191.0, 425.0), at("D", 269.0, 425.0), at("LP", 462.0, 425.0), at("UNIT", 531.0, 425.0)];
        w.extend([at("85", 189.0, 459.0), at("97", 267.0, 459.0), at("140", 461.0, 459.0), at("°C/W", 531.0, 459.0), at("RθJA", 59.0, 459.0)]);
        let r = read(4, &w);
        assert_eq!(r[0].for_package("to-92").map(|r| r.k_per_w), Some(140.0), "{:?}", r[0].cells);
        let mut two = r[0].clone();
        two.cells = vec![("KCS (TO-220)".into(), Some(23.5)), ("KCT (TO-220)".into(), Some(37.9))];
        assert!(two.for_package("to-220").is_none());
    }

    /// A row's note, where the board it was measured on is said: onsemi's "RθJA (Note 1)" and "1. Surface mounted on FR4 board
    /// using 1 in sq pad size".
    #[test]
    fn a_rows_note_is_in_its_text() {
        let mut w = line(&["Thermal", "Resistance,", "Junction-to-Ambient", "(Note", "1)", "RθJA", "125", "°C/W"], 100.0);
        w.extend(line(&["1.", "Surface", "mounted", "on", "FR4", "board", "using", "1", "in", "sq", "pad", "size."], 300.0));
        let r = read(0, &w);
        assert!(r[0].text.contains("注: surface mounted on fr4"), "{:?}", r[0].text);
        assert!(on_large_copper(&r[0].text), "{:?}", r[0].text);
    }

    /// ROHM's Japanese note under the ratings: "Ta≧25℃の場合は、9.6mW/℃で軽減。" (the slope in one word with the text after it).
    #[test]
    fn a_japanese_derating_note_gives_the_resistance() {
        let w = line(&["※3:", "70mm×70mm×1.6mm", "ガラスエポキシ基板実装時。Ta≧25℃の場合は、9.6mW/℃で軽減。"], 265.0);
        let r = read(3, &w);
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0].k_per_w - 104.17).abs() < 0.01 && r[0].from_derating && r[0].to == To::Ambient, "{r:?}");
        assert_eq!(r[0].derate_from, Some(25.0));
    }
}

#[cfg(test)]
mod real_pdf {
    use super::To;

    /// TI's tables with a column per package (`microohm-meter/`, not in the repository): TL431's codes alone, LM393's Japanese
    /// version, LM139's codes over the names.
    #[test]
    #[ignore]
    fn ti_columns_by_package() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter");
        for (f, family, want) in [("tl431.pdf", "to-92", 140.0), ("tl431.pdf", "sot-89", 52.0), ("lm393.pdf", "soic", 148.5), ("lm139.pdf", "tssop", 136.6)] {
            let Ok(ds) = crate::Datasheet::open_file(&dir.join(f)) else { continue };
            let ja = ds.thermal().into_iter().find(|r| r.to == To::Ambient && r.per_package).unwrap_or_else(|| panic!("{f}"));
            assert_eq!(ja.for_package(family).map(|r| r.k_per_w), Some(want), "{f} {family}: {:?}", ja.cells);
        }
    }
}
