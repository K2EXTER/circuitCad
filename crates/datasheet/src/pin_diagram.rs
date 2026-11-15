//! Pins read off a pin drawing (the package seen from above with a number at every pin and its name beside it), where there is no
//! table to read: each pin number is paired with the nearest name beside it on its line, or above or below it (SOT-23 drawings put the
//! names over and under the pins). The page is cut into bands at the captions ("Figure 4-1", "図 4-1"), and the pairs of a band into
//! drawings by where they are (data sheets put the drawings of two packages side by side); a drawing counts when its numbers run
//! 1, 2, … N each once.

use crate::text::{Rect, Word, normalize};

/// The most a name may stand from its number beside it on the line, and above or below it (points).
const BESIDE: f32 = 45.0;
const OVER: f32 = 22.0;
/// Pairs further apart than this across or down belong to different drawings (points): the two rows of a drawing stand up to about
/// 80 points apart and its pins 10 to 20 points one under the other, while drawings side by side stand 150 points and more apart and
/// drawings one under the other leave room for their titles.
const APART_X: f32 = 150.0;
const APART_Y: f32 = 45.0;

/// A pin number: 1 to 400 written alone.
fn number(w: &Word) -> Option<u32> {
    let t = w.text.trim();
    t.chars().all(|c| c.is_ascii_digit()).then(|| t.parse().ok()).flatten().filter(|n| (1..=400).contains(n))
}

/// Words of a drawing that are not names: its title and notes.
const NOT_NAMES: &[&str] = &["top", "view", "bottom", "side", "package", "pin", "pins", "not", "to", "scale", "exposed", "thermal", "pad", "die", "on", "underside", "notes", "note", "figure", "table", "and", "or", "of", "the", "nc:", "dnc", "(topview)", "(top", "view)", "上面図", "パッケージ", "ピン"];

/// A pin name: a short word with a letter that is not a number nor a word of the drawing's title; "-IN", "+IN", "V–" too.
fn name(w: &Word) -> Option<String> {
    let t = w.text.trim().trim_end_matches([',', ';']);
    let n = normalize(t);
    let first = t.chars().next()?;
    let ok = t.chars().count() <= 16 && !t.chars().any(char::is_whitespace) && t.chars().any(char::is_alphabetic) && (first.is_alphanumeric() || "+-−–".contains(first)) && !first.eq(&'(') && !NOT_NAMES.contains(&n.as_str());
    // "DNC" and "NC" are pins (do not connect, no connection): only the drawing's own words are left out
    // the part's own number printed in the drawing ("ADA4522-1") is not a pin
    let ok = ok && !(t.chars().count() >= 6 && crate::looks_like_part_number(t));
    let ok = ok || matches!(n.as_str(), "dnc" | "nc");
    ok.then(|| t.replace(['\u{2212}', '\u{2013}'], "-"))
}

fn centre(r: &Rect) -> (f32, f32) {
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

/// A caption's line: the line starts with "Figure", "Table", "図" or "表".
fn is_caption(w: &Word) -> bool {
    matches!(normalize(w.text.trim()).as_str(), "figure" | "table" | "図" | "表")
}

/// The pins of each drawing found in `area` of a page, the topmost and leftmost drawing first.
pub fn pins_in_drawings(words: &[Word], area: Rect) -> Vec<Vec<(String, String)>> {
    let inside: Vec<&Word> = words.iter().filter(|w| { let (x, y) = centre(&w.rect); x >= area.x0 && x <= area.x1 && y >= area.y0 && y <= area.y1 }).collect();
    // the captions cut the area into bands
    let mut cuts: Vec<f32> = inside.iter().filter(|w| is_caption(w)).map(|w| w.rect.y0).collect();
    cuts.push(area.y1);
    cuts.sort_by(f32::total_cmp);
    let mut out = vec![];
    let mut top = area.y0;
    for cut in cuts {
        let band: Vec<&Word> = inside.iter().copied().filter(|w| centre(&w.rect).1 > top && w.rect.y1 <= cut + 0.5).collect();
        top = cut + 1.0;
        out.extend(drawings_in(&band));
    }
    out
}

/// A drawing as it is gathered: its top, its left and its (number, name) pairs.
type Drawing = (f32, f32, Vec<(u32, String)>);

/// The drawings of one band.
fn drawings_in(words: &[&Word]) -> Vec<Vec<(String, String)>> {
    // a figure's number printed turned on its side ("16570-001" as a column of single characters) is not a pin number
    let upright = |w: &Word| {
        let c = centre(&w.rect);
        !words.iter().any(|o| !std::ptr::eq(*o, w) && o.text.trim().chars().count() == 1 && (centre(&o.rect).0 - c.0).abs() < 1.5 && (centre(&o.rect).1 - c.1).abs() < 6.0)
    };
    let nums: Vec<(usize, u32)> = words.iter().enumerate().filter_map(|(i, w)| number(w).filter(|_| upright(w)).map(|n| (i, n))).collect();
    // a word printed small under another ("IN" of "V" over "IN") is part of that name, not a name of its own
    let is_sub = |s: &Word| words.iter().any(|w| !std::ptr::eq(*w, s) && is_subscript_of(s, w));
    let names: Vec<(usize, String)> = words.iter().enumerate().filter_map(|(i, w)| if number(w).is_some() || is_sub(w) { None } else { name(w).map(|n| (i, n)) }).collect();
    // every number with every name near enough, the nearest pairs taken first
    let mut edges: Vec<(f32, usize, usize)> = vec![];
    for (a, &(i, _)) in nums.iter().enumerate() {
        let n = &words[i].rect;
        for (b, (j, _)) in names.iter().enumerate() {
            let m = &words[*j].rect;
            let same_line = (words[i].baseline() - words[*j].baseline()).abs() < 2.5;
            let gap_x = (m.x0 - n.x1).max(n.x0 - m.x1);
            let gap_y = (m.y0 - n.y1).max(n.y0 - m.y1);
            if same_line && (-1.0..=BESIDE).contains(&gap_x) {
                edges.push((gap_x.max(0.0), a, b));
            } else if (centre(m).0 - centre(n).0).abs() < 8.0 && (-1.0..=OVER).contains(&gap_y) {
                // over or under: a little behind one beside it
                edges.push((gap_y.max(0.0) + 5.0, a, b));
            }
        }
    }
    edges.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (mut num_used, mut name_used) = (vec![false; nums.len()], vec![false; names.len()]);
    // (number, name, where)
    let mut pairs: Vec<(u32, String, (f32, f32))> = vec![];
    for (_, a, b) in edges {
        if num_used[a] || name_used[b] {
            continue;
        }
        num_used[a] = true;
        name_used[b] = true;
        let (i, n) = nums[a];
        let (j, nm) = &names[b];
        pairs.push((n, with_subscript(words, *j, nm), centre(&words[i].rect)));
    }
    // drawings: pairs joined while they are near each other
    let mut group: Vec<usize> = (0..pairs.len()).collect();
    fn root(g: &mut [usize], mut k: usize) -> usize {
        while g[k] != k {
            g[k] = g[g[k]];
            k = g[k];
        }
        k
    }
    for a in 0..pairs.len() {
        for b in a + 1..pairs.len() {
            let (p, q) = (pairs[a].2, pairs[b].2);
            if (p.0 - q.0).abs() < APART_X && (p.1 - q.1).abs() < APART_Y {
                let (ra, rb) = (root(&mut group, a), root(&mut group, b));
                group[ra] = rb;
            }
        }
    }
    let mut drawings: Vec<Drawing> = vec![];
    let mut roots: Vec<usize> = vec![];
    for (k, (num, nm, at_xy)) in pairs.iter().enumerate() {
        let r = root(&mut group, k);
        let at = match roots.iter().position(|x| *x == r) {
            Some(at) => at,
            None => {
                roots.push(r);
                drawings.push((f32::MAX, f32::MAX, vec![]));
                drawings.len() - 1
            }
        };
        let d = &mut drawings[at];
        d.0 = d.0.min(at_xy.1);
        d.1 = d.1.min(at_xy.0);
        d.2.push((*num, nm.clone()));
    }
    // top to bottom, then left to right
    drawings.sort_by(|a, b| (a.0 / 40.0).floor().total_cmp(&(b.0 / 40.0).floor()).then(a.1.total_cmp(&b.1)));
    drawings
        .into_iter()
        .filter_map(|(_, _, mut pins)| {
            pins.sort_by_key(|p| p.0);
            let n = pins.len() as u32;
            // 1 … N, each once
            (n >= 2 && pins.iter().enumerate().all(|(k, p)| p.0 == k as u32 + 1)).then(|| pins.into_iter().map(|(k, nm)| (k.to_string(), nm)).collect())
        })
        .collect()
}

/// `s` is printed small right under the end of `w` (a subscript: "IN" of "V" over "IN").
fn is_subscript_of(s: &Word, w: &Word) -> bool {
    s.em() < w.em() * 0.9 && s.rect.y0 > w.rect.y0 && s.rect.y0 < w.rect.y1 + 3.0 && s.rect.x0 >= w.rect.x0 - 1.0 && s.rect.x0 <= w.rect.x1 + 2.0 && s.text.chars().all(|c| c.is_alphanumeric()) && number(s).is_none()
}

/// The name with what is printed small right under it ("V" over "IN": VIN).
fn with_subscript(words: &[&Word], j: usize, name: &str) -> String {
    let w = words[j];
    let sub = words.iter().find(|s| !std::ptr::eq(**s, w) && is_subscript_of(s, w));
    match sub {
        Some(s) => format!("{name}{}", s.text.trim()),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y, x + 5.0 * t.chars().count() as f32, y + 8.0)
    }

    /// An 8-pin dual drawing: names outside, numbers inside the outline, 1 to 4 down the left and 8 to 5 down the right.
    #[test]
    fn a_dual_drawing() {
        let mut words = vec![];
        for (k, (l, r)) in [("1OUT", "VCC"), ("1IN-", "2OUT"), ("1IN+", "2IN-"), ("GND", "2IN+")].iter().enumerate() {
            let y = 100.0 + 15.0 * k as f32;
            words.extend([w(l, 230.0, y), w(&(k + 1).to_string(), 280.0, y), w(&(8 - k).to_string(), 320.0, y), w(r, 335.0, y)]);
        }
        words.push(w("Figure", 60.0, 180.0));
        words.push(w("4-1.", 95.0, 180.0));
        let d = pins_in_drawings(&words, Rect { x0: 0.0, y0: 80.0, x1: 600.0, y1: 700.0 });
        assert_eq!(d.len(), 1);
        let lines = crate::pin_lines(&d[0]);
        assert_eq!(lines, "1 1OUT\n2 1IN-\n3 1IN+\n4 GND\n5 2IN+\n6 2IN-\n7 2OUT\n8 VCC");
    }

    /// Two 5-pin drawings side by side (SC70 and SOT-23): two results, the left first; a drawing with a number twice is not one.
    #[test]
    fn two_drawings_side_by_side() {
        let mut words = vec![];
        for x in [100.0, 350.0] {
            words.extend([w("D", x, 130.0), w("1", x + 35.0, 130.0), w("5", x + 95.0, 130.0), w("VDD", x + 125.0, 130.0)]);
            words.extend([w("S", x, 150.0), w("2", x + 35.0, 150.0)]);
            words.extend([w("GND", x - 10.0, 170.0), w("3", x + 35.0, 170.0), w("4", x + 95.0, 170.0), w("SEL", x + 125.0, 170.0)]);
        }
        let d = pins_in_drawings(&words, Rect { x0: 0.0, y0: 100.0, x1: 600.0, y1: 300.0 });
        assert_eq!(d.len(), 2);
        assert_eq!(crate::pin_lines(&d[0]), "1 D\n2 S\n3 GND\n4 SEL\n5 VDD");
    }

    /// SOT-23 drawn with the names over and under the pins.
    #[test]
    fn names_over_and_under() {
        let words = vec![w("D", 140.0, 80.0), w("3", 140.0, 95.0), w("1", 120.0, 130.0), w("2", 160.0, 130.0), w("G", 120.0, 145.0), w("S", 160.0, 145.0)];
        let d = pins_in_drawings(&words, Rect { x0: 0.0, y0: 0.0, x1: 600.0, y1: 300.0 });
        assert_eq!(crate::pin_lines(&d[0]), "1 G\n2 S\n3 D");
    }
}
