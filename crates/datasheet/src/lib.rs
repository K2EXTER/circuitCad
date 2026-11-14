//! Opens a data sheet PDF and draws its pages. Pure Rust (hayro), no GUI types.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use std::cell::OnceCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

mod dimensions;
pub mod graph;
mod headings;
mod pin_table;
mod suggest;
mod text;
mod value;
pub use headings::{HEADINGS, Heading};
pub use dimensions::{Found, PageLength, find_length};
pub use pin_table::{PinCompare, PinDiff, PinDiffKind, Table, compare_pins, guess_columns, pin_lines, pins_from, read_table};
pub use suggest::looks_like_part_number;
pub use text::{Hit, Query, Rect, Word, normalize};
pub use value::{Converted, Quantity, Value, convert, is_unit_word, parse_value};

/// Longest side of a drawn page in pixels. The renderer's canvas is 16 bit, and a bigger picture only costs memory.
pub const MAX_SIDE_PX: f32 = 8192.0;

/// An opened PDF.
pub struct Datasheet {
    pdf: Pdf,
    pages: usize,
    /// The words of each page, read the first time they are asked for.
    words: Vec<OnceCell<Vec<Word>>>,
}

/// A drawn page: straight RGBA, 4 bytes per pixel, opaque white background.
pub struct PageImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Datasheet {
    /// Reads a PDF. The error says in Japanese why it could not be read.
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let pdf = match catch_unwind(AssertUnwindSafe(|| Pdf::new(bytes))) {
            Ok(Ok(p)) => p,
            Ok(Err(LoadPdfError::Decryption(_))) => return Err("このPDFはパスワードで守られているため、開けません。パスワードなしで開けるPDFを使ってください".into()),
            Ok(Err(LoadPdfError::Invalid)) => return Err("PDFとして読めませんでした。壊れているか、PDFではないファイルです".into()),
            Err(_) => return Err("PDFの読み込み中に失敗しました。ファイルが壊れている可能性があります".into()),
        };
        let pages = pdf.pages().len();
        if pages == 0 {
            return Err("このPDFにはページがありません".into());
        }
        Ok(Self { pdf, pages, words: (0..pages).map(|_| OnceCell::new()).collect() })
    }

    /// Reads a PDF file.
    pub fn open_file(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("ファイルを読めませんでした: {e}"))?;
        Self::open(bytes)
    }

    pub fn page_count(&self) -> usize {
        self.pages
    }

    /// Size of a page in points (1/72 inch), rotation applied.
    pub fn page_size(&self, page: usize) -> Option<(f32, f32)> {
        self.pdf.pages().get(page).map(|p| p.render_dimensions())
    }

    /// The words of a page with their places (points from the top left of the page as drawn). Empty for a page that is
    /// only a picture, and for a page number that does not exist.
    pub fn words(&self, page: usize) -> &[Word] {
        let Some(cell) = self.words.get(page) else { return &[] };
        cell.get_or_init(|| {
            let pages = self.pdf.pages();
            let Some(p) = pages.get(page) else { return vec![] };
            catch_unwind(AssertUnwindSafe(|| text::read_words(p))).unwrap_or_default()
        })
    }

    /// Where `query` is on a page.
    pub fn search_page(&self, page: usize, query: &Query) -> Vec<Hit> {
        text::search(page, self.words(page), query)
    }

    /// Where any of the queries is on a page, in reading order (top to bottom, left to right).
    pub fn search_page_any(&self, page: usize, queries: &[Query]) -> Vec<Hit> {
        let mut hits: Vec<Hit> = queries.iter().flat_map(|q| self.search_page(page, q)).collect();
        hits.sort_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0).then(a.rect.x0.total_cmp(&b.rect.x0)));
        hits.dedup_by(|b, a| a.rect == b.rect);
        hits
    }

    /// The words of a page inside a rectangle, as a table (for the pin table of a part).
    pub fn table_in(&self, page: usize, area: Rect) -> Table {
        read_table(self.words(page), area)
    }

    /// The numbers of a page that can be lengths (in millimetres, or as inches when no unit is written).
    pub fn lengths_on(&self, page: usize) -> Vec<PageLength> {
        dimensions::lengths(self.words(page))
    }

    /// The pages that have a drawing of the land pattern or of the package (not the table of contents).
    pub fn dimension_pages(&self) -> Vec<usize> {
        let queries: Vec<Query> = [Heading::LandPattern, Heading::Package].into_iter().flat_map(Heading::queries).collect();
        (0..self.pages).filter(|&p| self.search_page_any(p, &queries).iter().any(|h| !h.in_contents)).collect()
    }

    /// The unit written right after a word on its line ("mm" after "0.65"), if there is one.
    pub fn unit_after(&self, page: usize, word: &Word) -> Option<String> {
        text::unit_after(self.words(page), word)
    }

    /// Part numbers the first page may be about, the likeliest first (for a person to pick from).
    pub fn name_candidates(&self) -> Vec<String> {
        suggest::name_candidates(self.words(0))
    }

    /// The kind of part the first pages talk about most, as a category name of the part list.
    pub fn category_hint(&self) -> Option<&'static str> {
        let first: Vec<&[Word]> = (0..self.pages.min(2)).map(|p| self.words(p)).collect();
        suggest::category_hint(&first)
    }

    /// Draws a page. `scale` is pixels per point; it is lowered when the picture would exceed `MAX_SIDE_PX`.
    pub fn render(&self, page: usize, scale: f32) -> Result<PageImage, String> {
        let pages = self.pdf.pages();
        let Some(p) = pages.get(page) else { return Err(format!("{} ページ目はありません", page + 1)) };
        let (w, h) = p.render_dimensions();
        if !(w > 0.0 && h > 0.0) {
            return Err("ページの大きさが分かりません".into());
        }
        let scale = scale.clamp(0.05, MAX_SIDE_PX / w.max(h));
        let drawn = catch_unwind(AssertUnwindSafe(|| {
            let cache = RenderCache::new();
            render(
                p,
                &cache,
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
            )
        }))
        .map_err(|_| "このページを描いている途中で失敗しました".to_string())?;
        // the background is opaque, so premultiplied and straight alpha are the same bytes
        Ok(PageImage { width: drawn.width() as usize, height: drawn.height() as usize, rgba: drawn.data_as_u8_slice().to_vec() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF (A6 landscape-ish, 200 x 100 pt) with a black box on a white page, written by hand.
    fn tiny_pdf() -> Vec<u8> {
        pdf_with("0 g 20 20 60 40 re f", "<< >>")
    }

    /// The same one-page PDF with other content and resources.
    fn pdf_with(content: &str, resources: &str) -> Vec<u8> {
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources {resources} >>"),
        ];
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
        }
        offsets.push(out.len());
        out += &format!("4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len());
        let xref = out.len();
        out += &format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
        for o in &offsets {
            out += &format!("{o:010} 00000 n \n");
        }
        out += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1);
        out.into_bytes()
    }

    #[test]
    fn opens_and_draws_a_page() {
        let d = Datasheet::open(tiny_pdf()).unwrap();
        assert_eq!(d.page_count(), 1);
        assert_eq!(d.page_size(0), Some((200.0, 100.0)));
        let img = d.render(0, 2.0).unwrap();
        assert_eq!((img.width, img.height), (400, 200));
        assert_eq!(img.rgba.len(), 400 * 200 * 4);
        let px = |x: usize, y: usize| &img.rgba[(y * img.width + x) * 4..][..4];
        // the box spans x 20..80, y (from the top) 20..60 points: inside is black, outside is white
        assert_eq!(px(100, 80), &[0, 0, 0, 255]);
        assert_eq!(px(300, 150), &[255, 255, 255, 255]);
    }

    const HELVETICA: &str = "<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>";

    /// Text drawn with a standard font that is not embedded: "Pin 1 SOIC-8" on one line, "GND" below.
    fn text_pdf() -> Vec<u8> {
        pdf_with("BT /F1 10 Tf 20 70 Td (Pin 1 SOIC-8) Tj 0 -20 Td (GND) Tj ET", HELVETICA)
    }

    #[test]
    fn reads_words_with_places() {
        let d = Datasheet::open(text_pdf()).unwrap();
        let words = d.words(0);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Pin", "1", "SOIC-8", "GND"]);
        // the first line's baseline is 70 pt up from the bottom of a 100 pt page, so 30 pt from the top
        let pin = words[0].rect;
        assert!((pin.x0 - 20.0).abs() < 0.5, "{pin:?}");
        assert!(pin.y0 < 30.0 && pin.y1 > 30.0, "{pin:?}");
        // "Pin" in Helvetica 10 pt is 6.67 + 2.22 + 5.56 = 14.45 pt wide
        assert!((pin.x1 - pin.x0 - 14.45).abs() < 0.1, "{pin:?}");
        // the second line is 20 pt lower
        assert!(words[3].rect.y0 > words[2].rect.y0 + 15.0);
        assert!(d.words(7).is_empty());
        assert!(Datasheet::open(tiny_pdf()).unwrap().words(0).is_empty());
    }

    #[test]
    fn finds_words_and_phrases() {
        let d = Datasheet::open(text_pdf()).unwrap();
        // a part of a word, and any case
        let h = d.search_page(0, &Query::new("soic").unwrap());
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].text, "SOIC-8");
        assert_eq!(h[0].context, "Pin 1 SOIC-8");
        assert_eq!(h[0].rect, d.words(0)[2].rect);
        // words one after the other join into one box
        let h = d.search_page(0, &Query::new("pin 1").unwrap());
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].text, "Pin 1");
        assert!(h[0].rect.x1 > d.words(0)[1].rect.x0);
        // the order matters
        assert!(d.search_page(0, &Query::new("1 pin").unwrap()).is_empty());
        assert!(d.search_page(0, &Query::new("zzz").unwrap()).is_empty());
        assert!(Query::new("   ").is_none());
    }

    /// A title in big letters, then smaller lines: values, a package, the headings.
    fn title_pdf() -> Vec<u8> {
        pdf_with(
            "BT /F1 16 Tf 10 82 Td (LM393B, LM2903B) Tj 0 -20 Td (Dual Comparator) Tj /F1 8 Tf 0 -16 Td (30V 5.7A SOT-23 LM393B LM393B) Tj 0 -12 Td (Absolute Maximum Ratings) Tj ET",
            HELVETICA,
        )
    }

    #[test]
    fn suggests_names_and_kind_and_finds_headings() {
        let d = Datasheet::open(title_pdf()).unwrap();
        assert_eq!(d.name_candidates(), ["LM393B", "LM2903B"]);
        assert_eq!(d.category_hint(), Some("コンパレータ"));
        let h = d.search_page_any(0, &Heading::AbsMax.queries());
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].text, "Absolute Maximum Ratings");
        assert!(d.search_page_any(0, &Heading::LandPattern.queries()).is_empty());
        // the Japanese words are in every heading's list, for the Japanese data sheets
        assert!(HEADINGS.iter().all(|h| h.terms().iter().any(|t| !t.is_ascii())));
        assert!(Datasheet::open(text_pdf()).unwrap().category_hint().is_none());
    }

    #[test]
    fn a_row_of_the_table_of_contents_is_told_from_the_heading() {
        let d = Datasheet::open(pdf_with(
            "BT /F1 8 Tf 10 80 Td (Pin Configuration ............................ 4) Tj 0 -30 Td (Pin Configuration) Tj ET",
            HELVETICA,
        ))
        .unwrap();
        let h = d.search_page_any(0, &Heading::PinConfig.queries());
        assert_eq!(h.len(), 2);
        // top to bottom: the contents row first
        assert!(h[0].in_contents && h[0].context.contains("...."));
        assert!(!h[1].in_contents);
    }

    #[test]
    fn the_unit_after_a_number_is_found_on_its_line_only() {
        let d = Datasheet::open(pdf_with("BT /F1 10 Tf 10 70 Td (0.65 mm 1.27 BSC) Tj 0 -20 Td (3.9) Tj 40 0 Td (mm) Tj ET", HELVETICA)).unwrap();
        let w = d.words(0);
        let texts: Vec<&str> = w.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["0.65", "mm", "1.27", "BSC", "3.9", "mm"]);
        assert_eq!(d.unit_after(0, &w[0]).as_deref(), Some("mm"));
        // "BSC" is not a unit; a unit on the next line, or far away, does not count
        assert_eq!(d.unit_after(0, &w[2]), None);
        assert_eq!(d.unit_after(0, &w[4]), None);
    }

    #[test]
    fn a_caption_across_the_columns_does_not_join_them() {
        // the caption lies over the two number columns; the body rows are name | number | number | text
        let cell = |x: u32, y: u32, t: &str| format!("BT /F1 6 Tf {x} {y} Td ({t}) Tj ET ");
        let mut content = cell(55, 90, "Table 4-1. Pin Functions");
        for (y, row) in [(75, ["VIN", "1", "9", "Input"]), (65, ["GND", "2", "8", "Ground"]), (55, ["OUT", "3", "7", "Output"])] {
            for (x, t) in [10, 60, 100, 140].into_iter().zip(row) {
                content += &cell(x, y, t);
            }
        }
        let d = Datasheet::open(pdf_with(&content, HELVETICA)).unwrap();
        let t = d.table_in(0, Rect { x0: 0.0, y0: 0.0, x1: 200.0, y1: 100.0 });
        assert_eq!(t.columns, 4, "{t:?}");
        assert_eq!(guess_columns(&t), Some((1, 0)));
        assert_eq!(pin_lines(&pins_from(&t, 1, 0)), "1 VIN\n2 GND\n3 OUT");
    }

    #[test]
    fn lengths_of_a_land_pattern_page_are_read_with_their_units() {
        let content = "BT /F1 8 Tf 10 85 Td (Recommended Land Pattern) Tj 0 -14 Td (1.27) Tj 30 0 Td (0.60 mm) Tj -30 -14 Td (0.050) Tj 30 0 Td (5.7 A) Tj 40 0 Td (4.0-5.0) Tj ET";
        let d = Datasheet::open(pdf_with(content, HELVETICA)).unwrap();
        assert_eq!(d.dimension_pages(), [0]);
        let l = d.lengths_on(0);
        // 1.27 is itself, or inches; 0.60 mm is mm; 0.050 may be inches; 5.7 A is not a length; a range gives both ends
        assert!(find_length(&l, 1.27).is_some());
        assert!(find_length(&l, 0.6).is_some());
        assert!(find_length(&l, 0.050 * 25.4).is_some());
        assert!(find_length(&l, 5.7).is_none());
        assert!(find_length(&l, 4.0).is_some() && find_length(&l, 5.0).is_some());
        // a page without such a heading is not a land pattern page
        assert!(Datasheet::open(text_pdf()).unwrap().dimension_pages().is_empty());
    }

    #[test]
    fn picks_the_name_under_a_place_in_a_run_of_names() {
        let d = Datasheet::open(pdf_with("BT /F1 10 Tf 20 70 Td (LM393B,LM2903B,LM193) Tj ET", HELVETICA)).unwrap();
        let w = &d.words(0)[0];
        assert_eq!(w.text, "LM393B,LM2903B,LM193");
        assert_eq!(w.tokens().collect::<Vec<_>>(), ["LM393B", "LM2903B", "LM193"]);
        let at = |i: usize| w.token_at(w.rect.x0 + (w.rect.x1 - w.rect.x0) * (i as f32 + 0.5) / w.text.chars().count() as f32);
        assert_eq!(at(2), Some("LM393B"));
        assert_eq!(at(9), Some("LM2903B"));
        assert_eq!(at(18), Some("LM193"));
        assert_eq!(at(6), None); // on the comma
    }

    #[test]
    fn normalizes_for_search() {
        assert_eq!(normalize("\u{FF30}\u{FF49}\u{FF4E}"), "pin");
        assert_eq!(normalize("\u{FB01}ne"), "fine");
        assert_eq!(normalize("V\u{2212}1\u{2013}2"), "v-1-2");
    }

    #[test]
    fn big_scale_is_limited() {
        let d = Datasheet::open(tiny_pdf()).unwrap();
        let img = d.render(0, 1000.0).unwrap();
        assert!(img.width as f32 <= MAX_SIDE_PX && img.height as f32 <= MAX_SIDE_PX);
    }

    #[test]
    fn bad_files_give_a_message() {
        assert!(Datasheet::open(b"not a pdf".to_vec()).err().unwrap().contains("PDF"));
        assert!(Datasheet::open(vec![]).is_err());
        assert!(Datasheet::open(tiny_pdf()).unwrap().render(5, 1.0).is_err());
    }
}
