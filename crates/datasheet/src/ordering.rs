//! The part numbers a data sheet orders by (its ordering information table: TI's "LP5907MFX-3.3/NOPB", onsemi's
//! "MC7805BDTG", AMS's "AMS1117-3.3"), and the output voltages it lists ("Standard Output Voltage Options: 1.2V, 1.8V",
//! Microchip). One data sheet covers parts that differ only in their output voltage or package: the form lets the person
//! pick one and takes its voltage.

use crate::text::Word;

/// The family a part's name stands for, to find its part numbers: its letters and digits up to the first other
/// character; a 78xx's or 79xx's voltage left off ("MC7800" → "MC78", "UA78L05" → "UA78", "LP5907" and "MCP1700" stay).
pub fn family(name: &str) -> String {
    let n = name.trim().to_ascii_uppercase();
    let letters: String = n.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits: String = n[letters.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    // 78xx and 79xx: the family is the four characters (the voltage follows: "MC7800" names them all)
    let digits = if digits.starts_with("78") || digits.starts_with("79") { &digits[..2] } else { digits.as_str() };
    format!("{letters}{digits}")
}

/// A part number as it would be named: without the packing and lead finish TI adds ("/NOPB", ".A", ".B") and the
/// marks after it ("*", ",").
fn clean(t: &str) -> String {
    let t = t.trim_end_matches(['*', ',', ';', ')']).trim_start_matches('(');
    let t = t.split('/').next().unwrap_or(t);
    let t = match t.rsplit_once('.') {
        // ".A", ".B": a letter after a dot (a voltage "-3.3" keeps its dot)
        Some((head, tail)) if tail.len() == 1 && tail.chars().all(|c| c.is_ascii_alphabetic()) => head,
        _ => t,
    };
    t.to_string()
}

/// The part numbers of the family (`family`) on the pages, each once, with the first page it is on: words that start
/// with the family and go on with letters, digits, "-" or "." (not the family's name alone, and not longer than 24).
pub fn part_numbers(pages: &[&[Word]], family: &str) -> Vec<(String, usize)> {
    let fam = family.to_ascii_uppercase();
    if fam.len() < 3 {
        return vec![];
    }
    let mut out: Vec<(String, usize)> = vec![];
    for (p, words) in pages.iter().enumerate() {
        for w in words.iter() {
            let t = clean(w.text.trim());
            let up = t.to_ascii_uppercase();
            if up.len() <= fam.len() || up.len() > 24 || !up.starts_with(&fam) {
                continue;
            }
            // (an evaluation module is no part: "UA78LEVM-075")
            if !up.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') || up.ends_with(['-', '.']) || up.contains("EVM") {
                continue;
            }
            if !out.iter().any(|o| o.0.eq_ignore_ascii_case(&t)) {
                out.push((t, p));
            }
        }
    }
    out
}

/// The package each part number is ordered in, from the ordering table's line: the first package name (`draft::package_of`)
/// right of the part number on its line ("LP5907MFX-3.3/NOPB  Active  Production  SOT-23 (DBV) | 5" → "SOT-23"). Of a part
/// number written several times, the first line that names a package. Part numbers on no such line are left out.
pub fn part_packages(pages: &[&[Word]], family: &str) -> Vec<(String, String)> {
    let fam = family.to_ascii_uppercase();
    let mut out: Vec<(String, String)> = vec![];
    if fam.len() < 3 {
        return out;
    }
    for words in pages {
        for line in crate::figures::lines(words) {
            for (k, w) in line.iter().enumerate() {
                let t = clean(w.text.trim());
                if !t.to_ascii_uppercase().starts_with(&fam) || t.len() <= fam.len() || out.iter().any(|o| o.0.eq_ignore_ascii_case(&t)) {
                    continue;
                }
                // the next part number on the line ends this one's cells
                let cells = line[k + 1..].iter().take_while(|n| !clean(n.text.trim()).to_ascii_uppercase().starts_with(&fam));
                let pkg = cells.flat_map(|n| n.tokens().map(str::to_string).collect::<Vec<_>>()).find_map(|tok| crate::draft::package_of(&tok));
                if let Some((f, pins)) = pkg {
                    let name = match pins {
                        Some(n) => format!("{}-{n}", f.to_uppercase()),
                        None => f.to_uppercase(),
                    };
                    out.push((t, name));
                }
            }
        }
    }
    out
}

/// The output voltages a data sheet lists as its options: the numbers with "V" after "Output Voltage Options" (or
/// "fixed output voltages") on its line and the two lines after it ("Standard Output Voltage Options:" / "- 1.2V, 1.8V,
/// 2.5V, 2.8V, 3.0V, 3.3V, 5.0V", MCP1700). Each once, in the order written.
pub fn voltage_options(pages: &[&[Word]]) -> Vec<f64> {
    let mut out: Vec<f64> = vec![];
    for words in pages {
        let lines = crate::figures::lines(words);
        for (i, l) in lines.iter().enumerate() {
            let text = l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ").to_ascii_lowercase();
            if !(text.contains("output voltage options") || text.contains("fixed output voltages") || text.contains("output voltage option:")) {
                continue;
            }
            let start = text.find("option").or_else(|| text.find("voltages")).unwrap_or(0);
            let mut texts = vec![text[start..].to_string()];
            texts.extend(lines.iter().skip(i + 1).take(2).map(|l| l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")));
            for t in texts {
                for tok in t.split([' ', ',', ';']) {
                    let tok = tok.trim();
                    if let Some(v) = tok.strip_suffix('V').or_else(|| tok.strip_suffix('v')).and_then(|n| n.parse::<f64>().ok())
                        && v > 0.0
                        && v < 100.0
                        && !out.contains(&v)
                    {
                        out.push(v);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, y: f32) -> Vec<Word> {
        let mut x = 50.0;
        text.split(' ')
            .map(|t| {
                let w = Word::for_test(t, x, y - 8.0, x + 5.0 * t.chars().count() as f32, y);
                x += 5.0 * t.chars().count() as f32 + 4.0;
                w
            })
            .collect()
    }

    #[test]
    fn families_and_their_part_numbers() {
        assert_eq!(family("MC7800"), "MC78");
        assert_eq!(family("LP5907"), "LP5907");
        assert_eq!(family("UA78L05"), "UA78");
        assert_eq!(family("AMS1117"), "AMS1117");
        assert_eq!(family("MCP1700"), "MCP1700");
        let mut p1 = line("LP5907A33YKMR.B Active Production DSBGA", 100.0);
        p1.extend(line("LP5907MFX-1.2/NOPB Active Production SOT-23", 120.0));
        p1.extend(line("LP5907MFX-1.2/NOPB.A Active", 130.0));
        p1.extend(line("LP5907MFX-3.3/NOPB Active", 140.0));
        p1.extend(line("the LP5907 is a low-noise LDO (LP5907EVM)", 160.0));
        let pages: Vec<&[Word]> = vec![&[], &p1];
        let parts: Vec<String> = part_numbers(&pages, "LP5907").into_iter().map(|p| p.0).collect();
        assert_eq!(parts, vec!["LP5907A33YKMR", "LP5907MFX-1.2", "LP5907MFX-3.3"], "not the evaluation module");
        let mc = line("MC7805ABD2TR4G 5.0 V NCV7805ABD2TR4G* MC7812CTG", 100.0);
        let pages: Vec<&[Word]> = vec![&mc];
        assert_eq!(part_numbers(&pages, &family("MC7800")).into_iter().map(|p| p.0).collect::<Vec<_>>(), vec!["MC7805ABD2TR4G", "MC7812CTG"]);
    }

    /// TI's ordering table: a part number per line and its package a few cells to the right; a line with two part numbers
    /// gives each the package after it.
    #[test]
    fn part_numbers_get_their_packages() {
        let mut p = line("LP5907MFX-1.2/NOPB Active Production SOT-23 (DBV) | 5 3000 | LARGE T&R", 100.0);
        p.extend(line("LP5907SNX-1.2/NOPB Active Production X2SON (DQN) | 4 3000", 112.0));
        p.extend(line("LP5907UVE-1.2/NOPB Active Production DSBGA (YKG) | 4", 124.0));
        p.extend(line("LP5907MFX-1.2/NOPB.A Active Production SOT-23 (DBV) | 5", 136.0));
        p.extend(line("MC7805CTG TO-220 MC7805CD2TG D2PAK", 148.0));
        let pages: Vec<&[Word]> = vec![&p];
        let got = part_packages(&pages, "LP5907");
        assert_eq!(got, vec![("LP5907MFX-1.2".to_string(), "SOT-23".to_string()), ("LP5907SNX-1.2".into(), "X2SON".into())], "DSBGA is no package name the draft knows");
        assert_eq!(part_packages(&pages, "MC78"), vec![("MC7805CTG".to_string(), "TO-220".to_string()), ("MC7805CD2TG".into(), "D2PAK".into())]);
    }

    #[test]
    fn voltage_options_are_listed() {
        let mut p = line("• Standard Output Voltage Options:", 100.0);
        p.extend(line("- 1.2V, 1.8V, 2.5V, 2.8V, 3.0V, 3.3V, 5.0V", 112.0));
        p.extend(line("• Stable with 1.0 µF Ceramic Output Capacitor", 124.0));
        let pages: Vec<&[Word]> = vec![&p];
        assert_eq!(voltage_options(&pages), vec![1.2, 1.8, 2.5, 2.8, 3.0, 3.3, 5.0]);
    }
}
