//! Words drawn with glyphs that have no Unicode: some PDFs (Rubycon's catalogues) embed fonts without the table that
//! says which letter each glyph is, so the page shows "100  5×11  0.90" but its text is empty. The glyphs are still
//! told apart (font and glyph number, written as letters of the Private Use Area by `text.rs`); the OCR of the page
//! reads most of them, and every glyph is given the letter the OCR read for it most often, word by word where the
//! OCR read as many letters as the word has glyphs. A glyph the OCR never reads alone (the "×" of "5×11", which the
//! OCR leaves out) is "×" when it always stands between two digits. The words then come back exactly, also where the
//! OCR misread them.

use crate::text::{BLIND_BASE, Word};
use std::collections::HashMap;

fn center(w: &Word) -> (f32, f32) {
    ((w.rect.x0 + w.rect.x1) / 2.0, (w.rect.y0 + w.rect.y1) / 2.0)
}

/// The OCR's words lying on a blind word, left to right, joined.
fn ocr_text_on(b: &Word, ocr: &[Word]) -> String {
    let mut on: Vec<&Word> = ocr
        .iter()
        .filter(|o| {
            let (x, y) = center(o);
            x >= b.rect.x0 - 1.5 && x <= b.rect.x1 + 1.5 && y >= b.rect.y0 - 2.0 && y <= b.rect.y1 + 2.0
        })
        .collect();
    on.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    on.iter().map(|o| o.text.trim()).collect::<Vec<_>>().join("")
}

fn is_blind(c: char) -> bool {
    (BLIND_BASE..BLIND_BASE + 0x1000).contains(&(c as u32))
}

/// The letters of the glyphs, learned from the OCR's words on the blind words.
pub(crate) fn learn(blind: &[Word], ocr: &[Word]) -> HashMap<char, char> {
    let mut votes: HashMap<char, HashMap<char, usize>> = HashMap::new();
    for b in blind {
        let read = ocr_text_on(b, ocr);
        let glyphs: Vec<char> = b.text.chars().collect();
        let letters: Vec<char> = read.chars().filter(|c| !c.is_whitespace()).collect();
        if letters.len() != glyphs.len() {
            continue;
        }
        for (g, l) in glyphs.iter().zip(letters) {
            *votes.entry(*g).or_default().entry(l).or_default() += 1;
        }
    }
    let mut map = HashMap::new();
    for (g, v) in &votes {
        let total: usize = v.values().sum();
        if let Some((l, n)) = v.iter().max_by_key(|(_, n)| **n)
            && *n * 5 >= total * 3
        {
            map.insert(*g, *l);
        }
    }
    // a glyph read as a cross ("X", "x", ">") or not at all, always between two digits: the "×" of a size
    let digit = |c: Option<&char>, map: &HashMap<char, char>| c.and_then(|c| map.get(c)).is_some_and(|l| l.is_ascii_digit());
    let mut between: HashMap<char, (usize, usize)> = HashMap::new();
    for b in blind {
        let g: Vec<char> = b.text.chars().collect();
        for i in 0..g.len() {
            let e = between.entry(g[i]).or_default();
            e.0 += 1;
            if i > 0 && digit(g.get(i - 1), &map) && digit(g.get(i + 1), &map) {
                e.1 += 1;
            }
        }
    }
    for (g, (all, inside)) in between {
        let crossish = map.get(&g).is_none_or(|l| matches!(l, 'X' | 'x' | '>' | '*' | '×'));
        if crossish && all >= 2 && inside == all {
            map.insert(g, '×');
        }
    }
    map
}

/// The blind words in letters: every glyph known, else what the OCR read on the word, else left out.
pub(crate) fn recover(blind: &[Word], ocr: &[Word]) -> Vec<Word> {
    if ocr.is_empty() {
        return vec![];
    }
    let map = learn(blind, ocr);
    blind
        .iter()
        .filter_map(|b| {
            let known: Option<String> = b.text.chars().map(|c| if is_blind(c) { map.get(&c).copied() } else { Some(c) }).collect();
            let text = match known {
                Some(t) => t,
                None => {
                    let read = ocr_text_on(b, ocr);
                    if read.is_empty() {
                        return None;
                    }
                    read
                }
            };
            let mut w = b.clone();
            w.text = text;
            Some(w)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(n: u32) -> char {
        char::from_u32(BLIND_BASE + n).unwrap()
    }

    fn blind(glyphs: &[u32], x: f32, y: f32) -> Word {
        let t: String = glyphs.iter().map(|n| g(*n)).collect();
        Word::for_test(&t, x, y - 7.0, x + 5.0 * glyphs.len() as f32, y)
    }

    fn read(t: &str, x: f32, y: f32) -> Word {
        Word::for_test(t, x, y - 7.0, x + 5.0 * t.chars().count() as f32, y)
    }

    /// Rubycon's table: digits the OCR reads, a size it reads in pieces or not at all.
    #[test]
    fn letters_learned_from_the_ocr_and_the_cross_between_digits() {
        // glyphs: 0 → "1", 1 → "0", 2 → "5", 3 → "×", 4 → "."
        let blind = vec![
            blind(&[0, 1, 1], 10.0, 20.0),       // 100
            blind(&[2, 3, 0, 0], 50.0, 20.0),    // 5×11
            blind(&[0, 1, 1, 1], 10.0, 32.0),    // 1000
            blind(&[0, 1, 3, 0, 2], 50.0, 32.0), // 10×15
            blind(&[1, 4, 2], 90.0, 32.0),       // 0.5
            blind(&[2, 2], 10.0, 44.0),          // 55
            blind(&[0, 1, 1], 10.0, 56.0),       // 100
        ];
        let ocr = vec![
            read("100", 10.0, 20.0),
            read("1000", 10.0, 32.0),
            read("0.5", 90.0, 32.0),
            read("55", 10.0, 44.0),
            // the OCR misreads one: the vote keeps the glyphs' letters
            read("IOO", 10.0, 56.0),
            // the size in two pieces, one glyph short: no votes
            read("10X1", 50.0, 32.0),
        ];
        let words = recover(&blind, &ocr);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["100", "5×11", "1000", "10×15", "0.5", "55", "100"]);
    }

    #[test]
    fn nothing_without_the_ocr() {
        assert!(recover(&[blind(&[0, 1], 0.0, 10.0)], &[]).is_empty());
    }
}
