//! Pins listed in lines of their own: "Pin 1. Input" / "2. Ground" / "3. Output" by a package's picture (onsemi's MC7800),
//! and the case outlines' styles ("STYLE 1:" / "PIN 1. EMITTER" / "2. BASE" / "3. COLLECTOR", several side by side) of
//! which the first page names the part's ("CASE 29 STYLE 1").

use crate::text::{Rect, Word};

/// One list of pins: the case outline's style it is under (None for a list of its own), the pins in order, where.
#[derive(Clone, Debug, PartialEq)]
pub struct PinList {
    pub style: Option<u32>,
    pub pins: Vec<(String, String)>,
    pub page: usize,
    pub rect: Rect,
}

/// A word that is a pin's number with its dot ("1.") and the number.
fn numbered(t: &str) -> Option<u32> {
    t.strip_suffix('.').and_then(|n| n.parse().ok()).filter(|n| (1..=40).contains(n))
}

/// The name after a number: its first word up to the next column, in capitals ("Input" → "INPUT", "SOURCE & SUBSTRATE" →
/// "SOURCE"). None when there is none or it is not a word.
fn name_after(line: &[&Word], from: f32, to: f32) -> Option<String> {
    let w = line.iter().filter(|w| w.rect.x0 >= from - 0.5 && w.rect.x0 < to).min_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0))?;
    let t = w.text.trim_matches(|c: char| !c.is_alphanumeric() && c != '+' && c != '-');
    // "NO CONNECTION": NC
    let next = line.iter().filter(|o| o.rect.x0 > w.rect.x1 && o.rect.x0 < to).min_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    if t.eq_ignore_ascii_case("no") && next.is_some_and(|n| n.text.to_ascii_lowercase().starts_with("conn")) {
        return Some("NC".into());
    }
    (t.chars().next().is_some_and(|c| c.is_alphabetic()) && t.len() <= 16).then(|| t.to_uppercase())
}

/// The pin lists of a page: each "PIN 1." (or "Pin 1."), and the lines under it with "2.", "3." … at the same place.
pub fn lists(page: usize, words: &[Word]) -> Vec<PinList> {
    let lines = crate::figures::lines(words);
    let mut out = vec![];
    for (li, line) in lines.iter().enumerate() {
        // the starts on this line: "PIN" then "1."
        let starts: Vec<usize> = (1..line.len()).filter(|&i| numbered(&line[i].text) == Some(1) && line[i - 1].text.eq_ignore_ascii_case("pin") && line[i].rect.x0 - line[i - 1].rect.x1 < 15.0).collect();
        for (k, &i) in starts.iter().enumerate() {
            let x = line[i].rect.x0;
            // the column reaches to the next list's "PIN"
            let to = starts.get(k + 1).map_or(f32::MAX, |&j| line[j - 1].rect.x0 - 2.0);
            let Some(first) = name_after(line, line[i].rect.x1, to) else { continue };
            let mut pins = vec![("1".to_string(), first)];
            let mut rect = line[i - 1].rect;
            let em = line[i].em().max(4.0);
            let mut base = line[i].baseline();
            for next in lines.iter().skip(li + 1) {
                if next[0].baseline() - base > 2.2 * em {
                    break;
                }
                let want = pins.len() as u32 + 1;
                // (a line of other text between: the text beside the picture, MC7800)
                let Some(n) = next.iter().find(|w| numbered(&w.text) == Some(want) && (w.rect.x0 - x).abs() < 4.0) else { continue };
                let Some(name) = name_after(next, n.rect.x1, to) else { break };
                pins.push((want.to_string(), name));
                rect = Rect { x0: rect.x0.min(n.rect.x0), y0: rect.y0, x1: rect.x1.max(n.rect.x1), y1: n.rect.y1 };
                base = next[0].baseline();
            }
            if pins.len() < 2 {
                continue;
            }
            // "STYLE 1:" just above, over this column
            let style = lines[..li].iter().rev().take(2).flat_map(|l| l.windows(2)).find_map(|p| {
                (p[0].text.eq_ignore_ascii_case("style") && (p[0].rect.x0 - (x - 12.0)).abs() < 20.0).then(|| p[1].text.trim_end_matches(':').parse::<u32>().ok()).flatten()
            });
            out.push(PinList { style, pins, page, rect });
        }
    }
    out
}

/// The style a page names the part's case outline by ("CASE 29 STYLE 1": "STYLE" then a number without a colon).
pub fn part_style(words: &[Word]) -> Option<u32> {
    let lines = crate::figures::lines(words);
    lines.iter().flat_map(|l| l.windows(2)).find_map(|p| (p[0].text.eq_ignore_ascii_case("style") && !p[1].text.ends_with(':')).then(|| p[1].text.parse().ok()).flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f32, y: f32) -> Vec<Word> {
        let mut x = x;
        text.split(' ')
            .map(|t| {
                let w = Word::for_test(t, x, y - 8.0, x + 5.0 * t.chars().count() as f32, y);
                x += 5.0 * t.chars().count() as f32 + 4.0;
                w
            })
            .collect()
    }

    #[test]
    fn a_list_of_its_own_and_styles_side_by_side() {
        // MC7800: by the package's picture
        let mut w = line("Pin 1. Input", 300.0, 100.0);
        w.extend(line("2. Ground", 318.0, 110.0));
        w.extend(line("These voltage regulators are monolithic", 60.0, 115.0));
        w.extend(line("3. Output", 318.0, 120.0));
        w.extend(line("Heatsink surface connected to Pin 2.", 300.0, 140.0));
        let l = lists(0, &w);
        assert_eq!(l.len(), 1, "{l:?}");
        assert_eq!((l[0].style, l[0].pins.iter().map(|p| p.1.as_str()).collect::<Vec<_>>()), (None, vec!["INPUT", "GROUND", "OUTPUT"]));
        // onsemi's case outline: two styles side by side
        let mut w = line("STYLE 1:", 79.0, 113.0);
        w.extend(line("STYLE 2:", 183.0, 113.0));
        w.extend(line("PIN 1. EMITTER", 86.0, 120.0));
        w.extend(line("PIN 1. BASE", 190.0, 120.0));
        // ("1." starts 19 points right of "PIN": the numbers under it there)
        w.extend(line("2. BASE", 105.0, 127.0));
        w.extend(line("2. EMITTER", 209.0, 127.0));
        w.extend(line("3. COLLECTOR", 105.0, 134.0));
        w.extend(line("3. COLLECTOR", 209.0, 134.0));
        w.extend(line("STYLE 8:", 400.0, 113.0));
        w.extend(line("PIN 1. ANODE", 407.0, 120.0));
        w.extend(line("2. NO CONNECTION", 426.0, 127.0));
        let l = lists(3, &w);
        assert_eq!(l.len(), 3, "{l:?}");
        assert_eq!((l[0].style, l[0].pins[0].1.as_str(), l[0].pins.len()), (Some(1), "EMITTER", 3));
        assert_eq!((l[2].style, l[2].pins[1].1.as_str()), (Some(8), "NC"), "no connection");
        assert_eq!((l[1].style, l[1].pins[0].1.as_str()), (Some(2), "BASE"));
        assert_eq!(part_style(&line("TO-92 CASE 29 STYLE 1", 300.0, 260.0)), Some(1));
        assert_eq!(part_style(&line("STYLE 1:", 300.0, 260.0)), None);
    }
}
