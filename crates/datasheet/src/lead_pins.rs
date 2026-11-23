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
    /// The numbers written over the lead names of a drawing (BSS138's marking diagram).
    Stacked,
    /// The same, with the drawing's letters read by the OCR (they are lines or a picture, not text: IRF540N's "Pin 1").
    Drawn,
    /// The letters around a SOT-23 drawn from the top (at a slant, AO3400A): the lone lead is 3, the other two 1 and 2 going
    /// counter-clockwise (Q13).
    TopView,
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
    all_from_text(page, words).into_iter().next()
}

/// Every list of the page that numbers the leads, in order down the page: a sheet of several packages has one each (onsemi's
/// 2N7000 / 2N7002: "1 − Source 2 − Gate 3 − Drain" by its TO-92, "1 − Gate 2 − Source 3 − Drain" by its SOT-23; Q13).
pub fn all_from_text(page: usize, words: &[Word]) -> Vec<LeadPins> {
    // a case outline's list of styles (onsemi: "STYLE 2: PIN 1. GATE 2. SOURCE …" for each style) names the leads of many
    // parts; which style is this part's is `Datasheet::find_pin_list`'s to say
    if words.iter().filter(|w| normalize(&w.text).starts_with("style")).count() >= 2 {
        return vec![];
    }
    let mut out = vec![];
    let mut found: Vec<(u32, &'static str)> = vec![];
    let mut area: Option<Rect> = None;
    for line in lines(words) {
        let texts: Vec<&str> = line.iter().map(|w| w.text.as_str()).collect();
        let got = from_line(&texts);
        if got.is_empty() {
            continue;
        }
        if complete(&got) {
            out.push(LeadPins { pins: sorted(got), page, area: span(&line), from: LeadPinsFrom::Text });
            found.clear();
            area = None;
            continue;
        }
        // a list over lines: the next numbers, none twice
        if got.iter().all(|g| !found.iter().any(|f| f.0 == g.0)) {
            found.extend(got);
            let r = span(&line);
            area = Some(area.map_or(r, |a| Rect { x0: a.x0.min(r.x0), y0: a.y0.min(r.y0), x1: a.x1.max(r.x1), y1: a.y1.max(r.y1) }));
            if complete(&found)
                && let Some(a) = area
            {
                out.push(LeadPins { pins: sorted(std::mem::take(&mut found)), page, area: a, from: LeadPinsFrom::Text });
                area = None;
            }
        } else {
            // this line may start the next list
            found = got;
            area = Some(span(&line));
        }
    }
    out
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

/// The numbers written by the lead names of a drawing (Q13): over or under the name, a digit or "Pin N" in its column (centres within
/// 12 points, the next line: onsemi's BSS138 marking diagram "3" over "Drain", Infineon's IRF540N "Gate" over "Pin 1"), or a
/// lead's letter with a digit beside it on its line (within 20 points: the IRLML6344's "G 1", "3 D"). Each of the leads once.
pub fn from_stacked(page: usize, words: &[Word]) -> Option<LeadPins> {
    let centre = |r: &Rect| ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    // a digit, with a full stop or comma after it as the OCR reads "Pin 2, Tab"
    let digit = |w: &Word| {
        let t = w.text.trim().trim_end_matches(['.', ',', ':']);
        if t.chars().count() == 1 { pin_number(t).filter(|n| (1..=9).contains(n)) } else { None }
    };
    // the numbers: a lone digit, or "Pin" and the digit after it on its line
    let mut numbers: Vec<(u32, Rect, usize)> = words.iter().enumerate().filter_map(|(k, w)| digit(w).map(|n| (n, w.rect, k))).collect();
    for (k, w) in words.iter().enumerate() {
        if normalize(&w.text).eq_ignore_ascii_case("pin")
            && let Some((n, d)) = words.iter().filter_map(|d| digit(d).map(|n| (n, d))).find(|(_, d)| (centre(&d.rect).1 - centre(&w.rect).1).abs() < 4.0 && d.rect.x0 > w.rect.x1 - 1.0 && d.rect.x0 - w.rect.x1 < 8.0)
        {
            numbers.push((n, span(&[w, d]), k));
        }
    }
    let mut pins: Vec<(u32, &'static str)> = vec![];
    let mut ws: Vec<&Word> = vec![];
    for name in words {
        let letter = name.text.trim().chars().count() == 1;
        let Some(r) = role(&name.text, letter) else { continue };
        let (nc, nr) = (centre(&name.rect), &name.rect);
        let found = numbers.iter().find(|(_, d, _)| {
            let dc = centre(d);
            if letter {
                // beside it on its line
                (dc.1 - nc.1).abs() < 4.0 && (d.x0 - nr.x1).max(nr.x0 - d.x1) < 20.0
            } else {
                // over or under it, in its column
                (dc.0 - nc.0).abs() < 12.0 && ((nr.y0 - d.y1 >= -2.0 && nr.y0 - d.y1 < 16.0) || (d.y0 - nr.y1 >= -2.0 && d.y0 - nr.y1 < 16.0))
            }
        });
        if let Some(&(n, _, k)) = found {
            pins.push((n, r));
            ws.push(name);
            ws.push(&words[k]);
        }
    }
    pins.sort();
    pins.dedup();
    (complete(&pins) && pins.len() == 3).then(|| LeadPins { pins: sorted(pins), page, area: span(&ws), from: LeadPinsFrom::Stacked })
}

/// Three-lead packages with one lead on one side and two on the other (SOT-23 and its smaller kin).
pub fn one_and_two(family: &str) -> bool {
    matches!(family, "sot-23" | "sot-323" | "sc-70" | "sot-416" | "sc-59")
}

/// The pins of a SOT-23 from the letters around its drawing seen from the top ("Top View"; a "Bottom View" counts the other
/// way), wherever the drawing is turned (AO3400A draws it at a slant): the two letters closest together are on one side (pins 1
/// and 2), the third is pin 3, and seen from the top the pins go 1, 2, 3 counter-clockwise. The letters taken are the nearest of
/// each kind to the view's words (within 150 points), and none nearer to another view's words (Q13).
pub fn from_top_view(page: usize, words: &[Word]) -> Option<LeadPins> {
    let views: Vec<(Rect, bool)> = words
        .windows(2)
        .filter_map(|p| {
            let (a, b) = (normalize(&p[0].text).to_ascii_lowercase(), normalize(&p[1].text).to_ascii_lowercase());
            let top = a == "top";
            (b.starts_with("view") && (top || a == "bottom")).then(|| (span(&[&p[0], &p[1]]), top))
        })
        .collect();
    let centre = |r: &Rect| ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    let dist = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    // a top view first (it needs no turning over), else a bottom view
    let mut order: Vec<&(Rect, bool)> = views.iter().collect();
    order.sort_by_key(|v| !v.1);
    for (at, top) in order {
        let here = centre(at);
        let mut picked: Vec<(&Word, &'static str)> = vec![];
        for kind in [["G", "D", "S"], ["B", "C", "E"]] {
            picked.clear();
            for want in kind {
                let near = words
                    .iter()
                    .filter(|w| w.text.trim().chars().count() == 1 && role(&w.text, true) == Some(want))
                    .filter(|w| dist(centre(&w.rect), here) < 150.0)
                    // not a letter of another view drawn beside this one
                    .filter(|w| views.iter().all(|(r, _)| std::ptr::eq(r, at) || dist(centre(&w.rect), centre(r)) >= dist(centre(&w.rect), here)))
                    .min_by(|a, b| dist(centre(&a.rect), here).total_cmp(&dist(centre(&b.rect), here)));
                if let Some(w) = near {
                    picked.push((w, want));
                }
            }
            if picked.len() == 3 {
                break;
            }
        }
        if picked.len() != 3 {
            continue;
        }
        let pts: Vec<(f32, f32)> = picked.iter().map(|(w, _)| centre(&w.rect)).collect();
        // the lone lead: the one not in the closest pair
        let pair = [(0, 1, 2), (0, 2, 1), (1, 2, 0)].into_iter().min_by(|a, b| dist(pts[a.0], pts[a.1]).total_cmp(&dist(pts[b.0], pts[b.1]))).unwrap();
        let (i, j, lone) = pair;
        // 1 then 2 counter-clockwise (y up: the page's y goes down) on the top; clockwise drawn from the bottom
        let cross = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (b.0 - a.0) * (-(c.1 - a.1)) - (-(b.1 - a.1)) * (c.0 - a.0);
        let ccw = cross(pts[i], pts[j], pts[lone]) > 0.0;
        let (one, two) = if ccw == *top { (i, j) } else { (j, i) };
        let pins = vec![(1, picked[one].1), (2, picked[two].1), (3, picked[lone].1)];
        if !complete(&pins) {
            continue;
        }
        let ws: Vec<&Word> = picked.iter().map(|(w, _)| *w).collect();
        return Some(LeadPins { pins: sorted(pins), page, area: span(&ws), from: LeadPinsFrom::TopView });
    }
    None
}

/// Packages drawn from the front with their leads numbered 1, 2, 3 from the left (the tab, if any, is the middle lead's).
pub fn front_view(family: &str) -> bool {
    matches!(family, "sot-223" | "to-220" | "to-126" | "to-252" | "to-263" | "dpak" | "d2pak" | "to-92")
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
        // the BD135 to BD140's TO-126 (Q14): the emitter first, the collector (and the tab) in the middle
        ("to-126", true) => (p(&[("1", "E"), ("2", "C"), ("3", "B")]), "BD135・BD139・BD140"),
        _ => return None,
    })
}

impl Datasheet {
    /// The pins of a MOSFET or transistor whose data sheet has no pin table (`lead_pins`): from a sentence, else from the
    /// letters of a drawing when the package (`family`) is seen from the front (under a pinout heading, or by the package's
    /// name). The usual order (`usual`) is the caller's to offer.
    pub fn lead_pins(&self, family: Option<&str>) -> Option<LeadPins> {
        for page in 0..self.page_count() {
            let all = all_from_text(page, self.words(page));
            // several lists (one per package): the one nearest the package's name
            let names: Vec<Rect> = family.map_or(vec![], |f| self.words(page).iter().filter(|w| crate::draft::package_of(&w.text).is_some_and(|k| k.0 == f)).map(|w| w.rect).collect());
            let gap = |p: &LeadPins| {
                let c = ((p.area.x0 + p.area.x1) / 2.0, (p.area.y0 + p.area.y1) / 2.0);
                names.iter().map(|r| ((r.x0 + r.x1) / 2.0 - c.0).hypot((r.y0 + r.y1) / 2.0 - c.1)).fold(f32::MAX, f32::min)
            };
            if let Some(p) = if all.len() > 1 && !names.is_empty() { all.into_iter().min_by(|a, b| gap(a).total_cmp(&gap(b))) } else { all.into_iter().next() } {
                return Some(p);
            }
        }
        for page in 0..self.page_count().min(4) {
            if let Some(p) = from_stacked(page, self.words(page)) {
                return Some(p);
            }
        }
        // a SOT-23 drawn from the top (or the bottom) with its letters around it
        if family.is_some_and(one_and_two) {
            for page in 0..self.page_count().min(4) {
                if let Some(p) = from_top_view(page, self.words(page)) {
                    return Some(p);
                }
            }
        }
        // the labels drawn rather than written (IRF540N: "Gate" over "Pin 1" in the symbol's outlines; IRLB8721: the "LEAD
        // ASSIGNMENTS" of its TO-220AB outline, a picture): the first two pages and the last four (where the outlines are), read by
        // the OCR, with the readers above (Q13). Only when nothing was written: a page takes the OCR about a second
        let n = self.page_count();
        let mut pages: Vec<usize> = (0..n.min(2)).chain(n.saturating_sub(4)..n).collect();
        pages.dedup();
        for page in pages {
            let Some((w, h)) = self.page_size(page) else { continue };
            // the whole page first (small: about 3 pixels a point), to find where lead names or "Pin" are drawn; then that part
            // again at 4 pixels a point, where the 6-point digits are read
            let first = self.drawn_words_in(page, Rect { x0: 0.0, y0: 0.0, x1: w, y1: h });
            let names: Vec<&Word> = first.iter().filter(|x| role(&x.text, false).is_some() || normalize(&x.text).eq_ignore_ascii_case("pin")).collect();
            if names.is_empty() {
                continue;
            }
            let near = span(&names);
            let drawn = self.drawn_words_at(page, Rect { x0: near.x0 - 40.0, y0: near.y0 - 30.0, x1: near.x1 + 40.0, y1: near.y1 + 30.0 }, 4.0);
            let mut all = self.words(page).to_vec();
            all.extend(drawn.iter().cloned());
            let listed = || all_from_text(page, &drawn).into_iter().next();
            if let Some(mut p) = from_stacked(page, &all).or_else(listed).or_else(|| family.is_some_and(one_and_two).then(|| from_top_view(page, &all)).flatten()) {
                p.from = LeadPinsFrom::Drawn;
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
        // Q13: the letters around a SOT-23 seen from the top (AO3400A, AO3401A), the numbers by the names (BSS138's marking diagram,
        // the IRLML6344's "G 1", MMBT3904's, the BC556B's TO-92), and the list by the package's name (2N7000 / 2N7002), the drawn words (IRF540N, IRFZ44N)
        for (f, family, from, want) in [
            ("AO3400A_ds.pdf", "sot-23", LeadPinsFrom::TopView, "1 G
2 S
3 D"),
            ("AO3401A.pdf", "sot-23", LeadPinsFrom::TopView, "1 G
2 S
3 D"),
            ("bss138.pdf", "sot-23", LeadPinsFrom::Stacked, "1 G
2 S
3 D"),
            ("irlml6344.pdf", "sot-23", LeadPinsFrom::Stacked, "1 G
2 S
3 D"),
            ("mmbt3904lt1-d.pdf", "sot-23", LeadPinsFrom::Stacked, "1 B
2 E
3 C"),
            ("bc556b-d.pdf", "to-92", LeadPinsFrom::Stacked, "1 C
2 B
3 E"),
            ("nds7002a.pdf", "sot-23", LeadPinsFrom::Text, "1 G
2 S
3 D"),
            ("nds7002a.pdf", "to-92", LeadPinsFrom::Text, "1 S
2 G
3 D"),
            // the outline's "Gate" over "Pin 1" drawn as lines, read by the OCR
            ("irf540n.pdf", "to-220", LeadPinsFrom::Drawn, "1 G
2 D
3 S"),
            ("irfz44n.pdf", "to-220", LeadPinsFrom::Drawn, "1 G
2 D
3 S"),
        ] {
            if let Some(ds) = open(f) {
                let p = ds.lead_pins(Some(family)).unwrap_or_else(|| panic!("{f}"));
                assert_eq!((p.from, crate::pin_lines(&p.pins)), (from, want.to_string()), "{f} as {family}");
            }
        }
    }

    /// A SOT-23 drawn at a slant from the top, with its bottom view beside it (as the AO3400A's page draws them): the closest pair
    /// G, S is one side, the lone D is 3, and G comes before S going counter-clockwise; the bottom view's letters are not taken.
    #[test]
    fn a_slanted_top_view_numbers_its_leads() {
        let words = vec![
            w("Top", 160.0, 80.0), w("View", 182.0, 80.0), w("Bottom", 240.0, 80.0), w("View", 278.0, 80.0),
            w("D", 155.0, 105.0), w("S", 212.0, 133.0), w("G", 185.0, 155.0),
            w("D", 230.0, 100.0), w("G", 290.0, 132.0), w("S", 258.0, 152.0),
        ];
        let p = from_top_view(0, &words).expect("read");
        assert_eq!(crate::pin_lines(&p.pins), "1 G
2 S
3 D");
        // only the bottom view: counted the other way
        let bottom: Vec<Word> = words.iter().filter(|x| x.rect.x0 >= 230.0).cloned().collect();
        let p = from_top_view(0, &bottom).expect("read");
        assert_eq!(crate::pin_lines(&p.pins), "1 G
2 S
3 D", "the bottom view's mirror image gives the same pins");
    }

    /// Numbers over names ("3" over "Drain", "1 2" over "Gate Source"), "Pin 1" under a name, and a letter beside its number.
    #[test]
    fn numbers_by_the_names() {
        let over = vec![w("3", 120.0, 90.0), w("Drain", 110.0, 100.0), w("1", 104.0, 160.0), w("2", 143.0, 160.0), w("Gate", 98.0, 172.0), w("Source", 133.0, 172.0)];
        assert_eq!(crate::pin_lines(&from_stacked(0, &over).expect("over").pins), "1 G
2 S
3 D");
        let under = vec![w("Gate", 100.0, 100.0), w("Pin", 100.0, 111.0), w("1", 121.0, 111.0), w("Drain", 200.0, 60.0), w("Pin", 200.0, 71.0), w("2", 221.0, 71.0), w("Source", 200.0, 140.0), w("Pin", 205.0, 151.0), w("3", 226.0, 151.0)];
        assert_eq!(crate::pin_lines(&from_stacked(0, &under).expect("Pin N").pins), "1 G
2 D
3 S");
        let beside = vec![w("G", 100.0, 100.0), w("1", 110.0, 100.0), w("S", 100.0, 140.0), w("2", 110.0, 140.0), w("3", 170.0, 120.0), w("D", 180.0, 120.0)];
        assert_eq!(crate::pin_lines(&from_stacked(0, &beside).expect("beside").pins), "1 G
2 S
3 D");
        // two of the three: nothing
        assert!(from_stacked(0, &beside[..4]).is_none());
    }

    /// Two numbered lists on a page (a TO-92 and a SOT-23 part on one sheet): both kept, in order down the page.
    #[test]
    fn every_list_of_a_page() {
        let words = vec![w("1", 10.0, 10.0), w("Source", 20.0, 10.0), w("2", 10.0, 20.0), w("Gate", 20.0, 20.0), w("3", 10.0, 30.0), w("Drain", 20.0, 30.0),
            w("1", 10.0, 200.0), w("Gate", 20.0, 200.0), w("2", 10.0, 210.0), w("Source", 20.0, 210.0), w("3", 10.0, 220.0), w("Drain", 20.0, 220.0)];
        let all = all_from_text(0, &words);
        assert_eq!(all.iter().map(|p| crate::pin_lines(&p.pins)).collect::<Vec<_>>(), ["1 S
2 G
3 D", "1 G
2 S
3 D"]);
    }
}
