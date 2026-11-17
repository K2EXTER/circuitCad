//! The heat figures of a data sheet: the thermal characteristics table's thermal resistances (junction to ambient, to case, to
//! lead) and the absolute maximum ratings' power derating ("Derate above 25°C 5.0 mW/°C": a thermal resistance of
//! 1 / 5.0 mW/°C = 200 °C/W, to the ambient after a "TA = 25°C" power, to the case after a "TC = 25°C" one).
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
    if let Some(j) = words.iter().position(|w| w.starts_with("junction")) {
        for w in &words[j + 1..] {
            match *w {
                w if w.starts_with("ambient") => return Some(To::Ambient),
                w if w.starts_with("case") => return Some(To::Case),
                w if w.starts_with("lead") => return Some(To::Lead),
                _ => {}
            }
        }
    }
    for w in &words {
        // "rθja", "rthja", "θja", the θ lost ("rja") or in a symbol font as "q" ("rqjl")
        let Some(rest) = w.strip_prefix('r').or_else(|| w.starts_with('θ').then_some(*w)) else { continue };
        let s = rest.strip_prefix("th").or_else(|| rest.strip_prefix('θ')).or_else(|| rest.strip_prefix('ϑ')).or_else(|| rest.strip_prefix('q')).unwrap_or(rest);
        {
            match s {
                "ja" => return Some(To::Ambient),
                "jc" => return Some(To::Case),
                "jl" => return Some(To::Lead),
                _ => {}
            }
        }
    }
    None
}

/// The thermal resistances of one page, in the order printed.
pub fn read(page: usize, words: &[Word]) -> Vec<ThermalRow> {
    let lines: Vec<Vec<String>> = lines(words).into_iter().map(|l| l.iter().map(|w| normalize(&w.text)).collect()).collect();
    let mut out = vec![];
    for (i, toks) in lines.iter().enumerate() {
        let text = toks.join(" ");
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
            out.push(ThermalRow { page, to: if case == Some(true) { To::Case } else { To::Ambient }, k_per_w: 1.0 / slope, text: text.clone(), from_derating: true, derate_from: from, columns: 1 });
            continue;
        }
        let symbol = toks.iter().any(|t| {
            let t = t.trim_matches(|c: char| !c.is_alphanumeric());
            ["rθja", "rθjc", "rθjl", "rthja", "rthjc", "rthjl", "θja", "θjc", "θjl", "rja", "rjc", "rjl", "rqja", "rqjc", "rqjl"].contains(&t)
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
        if v > 0.0 {
            out.push(ThermalRow { page, to, k_per_w: v, text, from_derating: false, derate_from: None, columns });
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
    ["2s2p", "high-k", "high k", "51-7", "in2", "in²", "inch", "cm2", "cm²", "copper area", "2 oz", "1 oz", "heatsink", "heat sink", "ambient thermal resistance"].iter().any(|k| t.contains(k))
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
    }

    #[test]
    fn a_large_board_is_told_from_the_smallest_pads() {
        assert!(on_large_copper("Thermal Resistance, Junction-to-Ambient (Note 1: JEDEC 2s2p)"));
        assert!(on_large_copper("Mounted on 1 in2 FR-4 pad"));
        assert!(!on_large_copper("Thermal Resistance, Junction-to-Ambient 200 °C/W"));
        assert!(on_large_copper("RθJA Junction-to-ambient thermal resistance 97.1 °C/W"), "TI's table");
    }
}
