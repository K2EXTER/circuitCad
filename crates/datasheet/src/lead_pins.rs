//! The pins of a three-lead MOSFET or transistor whose data sheet numbers none (候補 Q13・Q14): AO3400A, BSS138, IRF540N and
//! NDT3055L name the leads G, D and S only in the symbol and the package drawing, PN2222A writes "E B C" under its TO-92's
//! picture. In the order they are trusted:
//! - a sentence that numbers them ("Pin 1 = Gate", "1. Gate 2. Drain 3. Source", "Pin 1: G", "1 Base 2 Emitter 3 Collector");
//! - a drawing of a package seen from the front (SOT-223, TO-220, DPAK, TO-92): the letters G, D, S (or E, B, C) in a row are
//!   pins 1, 2, 3 from the left (NDT3055L: "G D S" with the tab's "D" above, under "Pinout"; PN2222A: "E B C" by "TO-92");
//! - the usual order of the package (SOT-23: 1 G, 2 S, 3 D; 1 B, 2 E, 3 C), only offered: the person checks it against the
//!   drawing.

use crate::figures::lines;
use crate::text::{Word, normalize};
use crate::{Datasheet, Rect};

/// Where the pins came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeadPinsFrom {
    /// A sentence of the data sheet numbers them.
    Text,
    /// The letters of a pinout drawing of a package seen from the front, left to right.
    Row,
    /// The usual order of the package, not read from this data sheet.
    Usual,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LeadPins {
    pub pins: Vec<(String, String)>,
    pub page: usize,
    pub area: Rect,
    pub from: LeadPinsFrom,
}

/// G, D, S (a MOSFET's) or B, C, E (a transistor's) for a word naming a lead ("Gate", "DRAIN", "Emitter", "S"); single letters
/// only when `letters`.
fn role(word: &str, letters: bool) -> Option<&'static str> {
    let w = normalize(word);
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    Some(match w {
        "gate" => "G",
        "drain" => "D",
        "source" => "S",
        "base" => "B",
        "collector" => "C",
        "emitter" => "E",
        "g" if letters => "G",
        "d" if letters => "D",
        "s" if letters => "S",
        "b" if letters => "B",
        "c" if letters => "C",
        "e" if letters => "E",
        _ => return None,
    })
}

/// A pin number of a three- or four-lead part written as a word ("1", "1.", "1:", "1)", "1=").
fn pin_number(word: &str) -> Option<u32> {
    let t = word.trim_end_matches(['.', ':', ')', '=', ',', '-']);
    t.parse::<u32>().ok().filter(|n| (1..=4).contains(n))
}

/// G, D and S (or B, E and C) each once, nothing else, the numbers each once: three leads, or four with a tab that is the
/// drain (or the collector).
fn complete(pins: &[(u32, &'static str)]) -> bool {
    let has = |r: &str| pins.iter().filter(|p| p.1 == r).count();
    let mut nums: Vec<u32> = pins.iter().map(|p| p.0).collect();
    nums.sort();
    nums.dedup();
    let set = |a: &str, b: &str, tab: &str| has(a) == 1 && has(b) == 1 && (has(tab) == 1 || (has(tab) == 2 && pins.len() == 4)) && has(a) + has(b) + has(tab) == pins.len();
    nums.len() == pins.len() && (set("G", "S", "D") || set("B", "E", "C"))
}

fn sorted(mut pins: Vec<(u32, &'static str)>) -> Vec<(String, String)> {
    pins.sort();
    pins.into_iter().map(|(n, r)| (n.to_string(), r.to_string())).collect()
}

/// The pins a line of text numbers: "Pin 1 = Gate", "1. Gate 2. Drain 3. Source", "Pin 1: G, Pin 2: D, Pin 3: S". A letter
/// alone counts only after "pin" ("1 S" is likely a table's cells).
pub fn from_line(words: &[&str]) -> Vec<(u32, &'static str)> {
    let mut out: Vec<(u32, &'static str)> = vec![];
    let mut i = 0;
    while i < words.len() {
        let after_pin = i > 0 && normalize(words[i - 1]).trim_matches(|c: char| !c.is_alphanumeric()) == "pin";
        if let Some(n) = pin_number(words[i]) {
            // the name after the number, past a separator word ("=", ":", "–")
            let mut j = i + 1;
            while j < words.len() && matches!(words[j], "=" | ":" | "-" | "–" | "−" | "is") {
                j += 1;
            }
            if let Some(r) = words.get(j).and_then(|w| role(w, after_pin)) {
                if !out.iter().any(|p| p.0 == n) {
                    out.push((n, r));
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The pins numbered by the text of a page: from one line, or from the lines of a list ("1. Gate" / "2. Drain" / "3. Source").
pub fn from_text(page: usize, words: &[Word]) -> Option<LeadPins> {
    // a case outline's list of styles (onsemi: "STYLE 2: PIN 1. GATE 2. SOURCE …" for each style) names the leads of many
    // parts; which style is this part's is `Datasheet::find_pin_list`'s to say
    if words.iter().filter(|w| normalize(&w.text).starts_with("style")).count() >= 2 {
        return None;
    }
    let mut found: Vec<(u32, &'static str)> = vec![];
    let mut area: Option<Rect> = None;
    for line in lines(words) {
        let texts: Vec<&str> = line.iter().map(|w| w.text.as_str()).collect();
        let got = from_line(&texts);
        if got.is_empty() {
            continue;
        }
        if complete(&got) {
            return Some(LeadPins { pins: sorted(got), page, area: span(&line), from: LeadPinsFrom::Text });
        }
        // a list over lines: the next numbers, none twice
        if got.iter().all(|g| !found.iter().any(|f| f.0 == g.0)) {
            found.extend(got);
            let r = span(&line);
            area = Some(area.map_or(r, |a| Rect { x0: a.x0.min(r.x0), y0: a.y0.min(r.y0), x1: a.x1.max(r.x1), y1: a.y1.max(r.y1) }));
            if complete(&found) {
                return Some(LeadPins { pins: sorted(found), page, area: area?, from: LeadPinsFrom::Text });
            }
        } else {
            found.clear();
            area = None;
        }
    }
    None
}

fn span(line: &[&Word]) -> Rect {
    line.iter().fold(Rect { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN }, |a, w| Rect { x0: a.x0.min(w.rect.x0), y0: a.y0.min(w.rect.y0), x1: a.x1.max(w.rect.x1), y1: a.y1.max(w.rect.y1) })
}

/// The letters G, D and S in a row inside `area` (a pinout drawing of a package seen from the front): pins 1, 2, 3 from the
/// left. Three single letters on one line, each once, no further apart than 80 points.
pub fn from_row(page: usize, words: &[Word], area: Rect) -> Option<LeadPins> {
    let inside: Vec<Word> = words.iter().filter(|w| w.rect.y0 >= area.y0 && w.rect.y1 <= area.y1 && w.rect.x0 >= area.x0 && w.rect.x1 <= area.x1).cloned().collect();
    for line in lines(&inside) {
        let letters: Vec<(&Word, &'static str)> = line.iter().filter(|w| w.text.trim().chars().count() == 1).filter_map(|w| role(&w.text, true).map(|r| (*w, r))).collect();
        if letters.len() != 3 {
            continue;
        }
        let pins: Vec<(u32, &'static str)> = letters.iter().enumerate().map(|(i, (_, r))| (i as u32 + 1, *r)).collect();
        let close = letters.windows(2).all(|p| p[1].0.rect.x0 - p[0].0.rect.x1 < 80.0);
        if close && complete(&pins) {
            let ws: Vec<&Word> = letters.iter().map(|(w, _)| *w).collect();
            return Some(LeadPins { pins: sorted(pins), page, area: span(&ws), from: LeadPinsFrom::Row });
        }
    }
    None
}

/// Packages drawn from the front with their leads numbered 1, 2, 3 from the left (the tab, if any, is the middle lead's).
pub fn front_view(family: &str) -> bool {
    matches!(family, "sot-223" | "to-220" | "to-252" | "to-263" | "dpak" | "d2pak" | "to-92")
}

/// The usual order of a MOSFET's (or, `bipolar`, a transistor's) leads in a package, and the parts it was seen on. Only a
/// suggestion: makers differ (the TO-92 BC547 is C B E where 2N3904 is E B C).
/// MOSFET: most small-signal SOT-23 parts 1 G, 2 S, 3 D; the power packages 1 G, 2 D, 3 S; the TO-92 2N7000 1 S, 2 G, 3 D.
/// Transistor: SOT-23 1 B, 2 E, 3 C; the power packages 1 B, 2 C, 3 E; TO-92 1 E, 2 B, 3 C.
pub fn usual(family: &str, bipolar: bool) -> Option<(Vec<(String, String)>, &'static str)> {
    let p = |v: &[(&str, &str)]| v.iter().map(|(n, r)| (n.to_string(), r.to_string())).collect::<Vec<_>>();
    Some(match (family, bipolar) {
        ("sot-23" | "sot-323" | "sc-70", false) => (p(&[("1", "G"), ("2", "S"), ("3", "D")]), "AO3400A・BSS138・2N7002"),
        ("sot-223" | "to-220" | "to-252" | "to-263" | "dpak" | "d2pak", false) => (p(&[("1", "G"), ("2", "D"), ("3", "S")]), "NDT3055L・IRF540N"),
        ("to-92", false) => (p(&[("1", "S"), ("2", "G"), ("3", "D")]), "2N7000・BS170"),
        ("sot-23" | "sot-323" | "sc-70", true) => (p(&[("1", "B"), ("2", "E"), ("3", "C")]), "MMBT3904・BC847"),
        ("sot-223" | "to-220" | "to-252" | "to-263" | "dpak" | "d2pak", true) => (p(&[("1", "B"), ("2", "C"), ("3", "E")]), "TIP120・BCP56・MJD31"),
        ("to-92", true) => (p(&[("1", "E"), ("2", "B"), ("3", "C")]), "2N3904・PN2222A (BC547 は逆の C B E)"),
        _ => return None,
    })
}

impl Datasheet {
    /// The pins of a MOSFET or transistor whose data sheet has no pin table (`lead_pins`): from a sentence, else from the
    /// letters of a drawing when the package (`family`) is seen from the front (under a pinout heading, or by the package's
    /// name). The usual order (`usual`) is the caller's to offer.
    pub fn lead_pins(&self, family: Option<&str>) -> Option<LeadPins> {
        for page in 0..self.page_count() {
            if let Some(p) = from_text(page, self.words(page)) {
                return Some(p);
            }
        }
        if !family.is_some_and(front_view) {
            return None;
        }
        let queries = crate::Heading::PinConfig.queries();
        for page in 0..self.page_count() {
            let Some((w, h)) = self.page_size(page) else { continue };
            for hit in self.search_page_any(page, &queries).into_iter().filter(|h| !h.in_contents) {
                // the drawing under the heading, up to 150 points down and across its half of the page
                let (x0, x1) = if hit.rect.x0 > w / 2.0 { (w / 2.0, w) } else { (0.0, w / 2.0) };
                let area = Rect { x0, y0: hit.rect.y1, x1, y1: (hit.rect.y1 + 150.0).min(h) };
                if let Some(p) = from_row(page, self.words(page), area) {
                    return Some(p);
                }
            }
        }
        // no heading: the package's picture by its name on the first pages (PN2222A: "E B C" under the TO-92, 11 points
        // below its "TO-92")
        let family = family?;
        for page in 0..self.page_count().min(3) {
            for w in self.words(page).iter().filter(|w| crate::draft::package_of(&w.text).is_some_and(|k| k.0 == family)) {
                let area = Rect { x0: w.rect.x0 - 120.0, y0: w.rect.y0 - 60.0, x1: w.rect.x1 + 120.0, y1: w.rect.y1 + 60.0 };
                if let Some(p) = from_row(page, self.words(page), area) {
                    return Some(p);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y, x + 6.0 * t.chars().count() as f32, y + 8.0)
    }

    #[test]
    fn sentences_that_number_the_leads() {
        let cases: &[(&str, &[(&str, &str)])] = &[
            ("Pin 1 = Gate, Pin 2 = Source, Pin 3 = Drain", &[("1", "G"), ("2", "S"), ("3", "D")]),
            ("1. Gate 2. Drain 3. Source 4. Drain", &[("1", "G"), ("2", "D"), ("3", "S"), ("4", "D")]),
            ("Pin 1: G Pin 2: D Pin 3: S", &[("1", "G"), ("2", "D"), ("3", "S")]),
        ];
        for (text, want) in cases {
            let words: Vec<&str> = text.split(' ').collect();
            let got = from_line(&words);
            assert!(complete(&got), "{text}: {got:?}");
            let want: Vec<(String, String)> = want.iter().map(|(n, r)| (n.to_string(), r.to_string())).collect();
            assert_eq!(sorted(got), want, "{text}");
        }
        // a letter alone after a number is not a lead's name (a table's "1 S" cells), nor are two gates
        assert!(!complete(&from_line(&["1", "G", "2", "S", "3", "D"])));
        assert!(!complete(&from_line(&["Pin", "1", "=", "Gate", "Pin", "2", "=", "Gate", "Pin", "3", "=", "Drain"])));
    }

    #[test]
    fn a_list_over_lines() {
        let words = vec![w("1.", 60.0, 100.0), w("Gate", 75.0, 100.0), w("2.", 60.0, 112.0), w("Drain", 75.0, 112.0), w("3.", 60.0, 124.0), w("Source", 75.0, 124.0)];
        let p = from_text(0, &words).expect("the list");
        assert_eq!(p.pins, vec![("1".into(), "G".into()), ("2".into(), "D".into()), ("3".into(), "S".into())]);
        assert_eq!(p.from, LeadPinsFrom::Text);
    }

    /// onsemi's NDT3055L: "PINOUT DIAGRAM" over the tab's "D" and the leads' "G D S".
    #[test]
    fn the_letters_of_a_front_view_in_a_row() {
        let words = vec![w("D", 454.0, 395.0), w("G", 424.0, 459.0), w("D", 455.0, 459.0), w("S", 485.0, 459.0), w("Symbol", 64.0, 462.0)];
        let p = from_row(0, &words, Rect { x0: 300.0, y0: 380.0, x1: 600.0, y1: 520.0 }).expect("the row");
        assert_eq!(p.pins, vec![("1".into(), "G".into()), ("2".into(), "D".into()), ("3".into(), "S".into())]);
        assert!(from_row(0, &words[..2], Rect { x0: 300.0, y0: 380.0, x1: 600.0, y1: 520.0 }).is_none());
    }

    /// A transistor's leads: a list with no dots ("1 BASE 2 EMITTER 3 COLLECTOR"), PN2222A's "E B C" under its TO-92's
    /// picture; a MOSFET's and a transistor's letters are not mixed.
    #[test]
    fn a_transistors_leads() {
        let got = from_line(&["1", "BASE", "2", "EMITTER", "3", "COLLECTOR"]);
        assert_eq!(sorted(got), vec![("1".into(), "B".into()), ("2".into(), "E".into()), ("3".into(), "C".into())]);
        let words = vec![w("TO-92", 448.0, 281.0), w("E", 390.0, 292.0), w("B", 396.0, 292.0), w("C", 403.0, 292.0)];
        let p = from_row(0, &words, Rect { x0: 300.0, y0: 220.0, x1: 600.0, y1: 350.0 }).expect("E B C");
        assert_eq!(crate::pin_lines(&p.pins), "1 E
2 B
3 C");
        assert!(!complete(&[(1, "G"), (2, "B"), (3, "D")]));
    }

    #[test]
    fn the_usual_orders() {
        assert_eq!(usual("sot-23", false).unwrap().0[1], ("2".to_string(), "S".to_string()));
        assert_eq!(usual("to-220", false).unwrap().0[1], ("2".to_string(), "D".to_string()));
        assert_eq!(usual("sot-23", true).unwrap().0[0], ("1".to_string(), "B".to_string()));
        assert!(usual("soic", false).is_none());
    }

    /// The real data sheets (`microohm-meter/`, not in the repository).
    #[test]
    #[ignore]
    fn real_mosfets() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter");
        let open = |f: &str| Datasheet::open_file(&dir.join(f)).ok();
        if let Some(ds) = open("NDT3055L-D.pdf") {
            let p = ds.lead_pins(Some("sot-223")).expect("NDT3055L's pinout");
            assert_eq!((p.from, crate::pin_lines(&p.pins)), (LeadPinsFrom::Row, "1 G\n2 D\n3 S".to_string()));
        }
        if let Some(ds) = open("AO3400A_ds.pdf") {
            // a top view of a SOT-23 is not read as a row: the usual order is offered instead
            assert!(ds.lead_pins(Some("sot-23")).is_none());
        }
    }
}
