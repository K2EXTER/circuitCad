//! Reading text that a page draws as lines rather than letters (the tick labels of some makers' graphs) with the OCR
//! built into Windows (`Windows.Media.Ocr`): nothing to ship, and it works offline. On other systems there is none
//! and the callers go on as before (the person clicks the ticks).

use crate::PageImage;

/// A word the OCR read, with its box in the picture's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// The words in a picture, None when there is no OCR (not Windows, no language for it) or it failed.
pub fn read(img: &PageImage) -> Option<Vec<OcrWord>> {
    imp::read(img)
}

/// The picture turned a quarter clockwise: a title printed up the page (an axis's) reads left to right in it. A pixel
/// (x, y) of the turned picture is (y, height - 1 - x) of the original.
pub fn turned(img: &PageImage) -> PageImage {
    let (w, h) = (img.width, img.height);
    let mut rgba = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            // the original (x, y) goes to (h - 1 - y, x)
            let (nx, ny) = (h - 1 - y, x);
            let (s, d) = ((y * w + x) * 4, (ny * h + nx) * 4);
            rgba[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
    PageImage { width: h, height: w, rgba }
}

/// Letters the OCR takes digits for, in a word made of digits and such letters only ("1O0" is 100, "l0" is 10, "ooo"
/// is 000; a lone "o" or "I" stays a letter).
pub fn as_number(text: &str) -> String {
    let like = |c: char| c.is_ascii_digit() || ".,Oo|lI".contains(c);
    if !text.chars().all(like) || (text.chars().count() < 2 && !text.chars().any(|c| c.is_ascii_digit())) {
        return text.to_string();
    }
    let t: String = text
        .chars()
        .map(|c| match c {
            'O' | 'o' => '0',
            'l' | 'I' | '|' => '1',
            _ => c,
        })
        .collect();
    // "20,000" groups thousands; "0,5" is a decimal comma
    match t.split_once(',') {
        Some((a, b)) if b.len() == 3 && !a.is_empty() && !b.contains(',') && a != "0" => format!("{a}{b}"),
        _ => t.replace(',', "."),
    }
}

/// The words with letters read for digits (`as_number`), and a number the OCR split in two put back together ("1"
/// and "000" a few pixels apart on one line: 1000).
pub fn joined(words: Vec<OcrWord>) -> Vec<OcrWord> {
    let mut out: Vec<OcrWord> = vec![];
    let mut words: Vec<OcrWord> = words.into_iter().map(|w| OcrWord { text: as_number(&w.text), ..w }).collect();
    words.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));
    // digits, or letters the OCR takes digits for (a lone "I" before "000")
    let number = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || ".Oo|lI".contains(c));
    for w in words {
        if let Some(last) = out.iter_mut().rev().find(|l| (l.y0 - w.y0).abs() < 0.3 * (w.y1 - w.y0) && w.x0 > l.x0) {
            let gap = w.x0 - last.x1;
            if number(&last.text) && number(&w.text) && gap >= 0.0 && gap < 0.35 * (w.y1 - w.y0) {
                last.text = as_number(&format!("{}{}", last.text, w.text));
                last.x1 = w.x1;
                last.y0 = last.y0.min(w.y0);
                last.y1 = last.y1.max(w.y1);
                continue;
            }
        }
        out.push(w);
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::OcrWord;
    use crate::PageImage;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::core::HSTRING;

    pub fn read(img: &PageImage) -> Option<Vec<OcrWord>> {
        // straight RGBA to BGRA, then the work on a thread of its own (WinRT in the multithreaded apartment, not the
        // window's thread)
        let mut bgra = img.rgba.clone();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let (w, h) = (img.width as i32, img.height as i32);
        std::thread::spawn(move || run(bgra, w, h).ok().flatten()).join().ok().flatten()
    }

    fn run(bgra: Vec<u8>, w: i32, h: i32) -> windows::core::Result<Option<Vec<OcrWord>>> {
        let max = OcrEngine::MaxImageDimension()? as i32;
        if w > max || h > max || w < 40 || h < 40 {
            return Ok(None);
        }
        // English reads digits, units and Latin letters best; else the person's own language (Japanese reads them too)
        let engine = Language::CreateLanguage(&HSTRING::from("en-US")).and_then(|l| OcrEngine::TryCreateFromLanguage(&l)).or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())?;
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, w, h)?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut out = vec![];
        for line in result.Lines()? {
            for word in line.Words()? {
                let r = word.BoundingRect()?;
                out.push(OcrWord { text: word.Text()?.to_string(), x0: r.X, y0: r.Y, x1: r.X + r.Width, y1: r.Y + r.Height });
            }
        }
        Ok(Some(out))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn read(_: &crate::PageImage) -> Option<Vec<super::OcrWord>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_read_for_digits_in_a_number() {
        assert_eq!(as_number("1O0"), "100");
        assert_eq!(as_number("l0"), "10");
        assert_eq!(as_number("0,5"), "0.5");
        assert_eq!(as_number("20,000"), "20000");
        assert_eq!(as_number("1O,OOO"), "10000");
        assert_eq!(as_number("Coss"), "Coss", "a word stays a word");
        assert_eq!(as_number("ooo"), "000");
        assert_eq!(as_number("o"), "o");
    }

    #[test]
    fn a_number_split_in_two_is_put_back() {
        let w = |t: &str, x0: f32, x1: f32| OcrWord { text: t.into(), x0, y0: 100.0, x1, y1: 120.0 };
        let j = joined(vec![w("1", 10.0, 16.0), w("ooo", 19.0, 40.0), w("2", 100.0, 106.0), w("Cds", 50.0, 70.0), w("I", 200.0, 202.0), w("000", 204.0, 220.0)]);
        let texts: Vec<&str> = j.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, vec!["1000", "Cds", "2", "1000"], "far apart or a word: not joined; a lone I before digits is a 1");
    }

    #[test]
    fn a_turned_picture_puts_the_left_edge_on_top() {
        // 3 x 2: the top left pixel is black
        let mut rgba = vec![255u8; 3 * 2 * 4];
        rgba[0..4].copy_from_slice(&[0, 0, 0, 255]);
        let t = turned(&PageImage { width: 3, height: 2, rgba });
        assert_eq!((t.width, t.height), (2, 3));
        // turned a quarter clockwise, the top left goes to the top right
        assert_eq!(&t.rgba[4..8], &[0, 0, 0, 255]);
    }
}
