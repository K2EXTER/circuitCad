//! Writing the sheet as a PDF (File menu, `--pdf file.pdf`): the same picture as the SVG, on one page cut to the drawing.
//!
//! Lines, circles, rectangles and polygons become PDF paths. Text becomes the outlines of the letters of the bundled font
//! (BIZ UD Gothic, OFL), so every viewer shows the Japanese the same way without the font being put into the file; the price is that the
//! text of the PDF cannot be searched or copied.

use super::*;
use egui::epaint::ClippedShape;
use egui::epaint::ColorMode;
use std::fmt::Write as _;

/// PDF units (points) per screen pixel of the picture.
const K: f32 = 0.75;
/// The empty border kept around the drawing, in pixels (the same as the SVG).
const BORDER: f32 = 16.0;

/// Colour `c` laid over `bg`, as three numbers 0..1 (a translucent colour is mixed in, so the file needs no transparency).
fn mix(c: egui::Color32, bg: egui::Color32) -> [f32; 3] {
    let a = c.a() as f32 / 255.0;
    let f = |x: u8, y: u8| (x as f32 * a + y as f32 * (1.0 - a)) / 255.0;
    [f(c.r(), bg.r()), f(c.g(), bg.g()), f(c.b(), bg.b())]
}

fn visible(c: egui::Color32) -> bool {
    c.a() > 0
}

/// Collects the page: the picture's pixel space is turned over (PDF counts up from the bottom) and cut to `bounds`.
struct Page<'a> {
    out: String,
    min: egui::Pos2,
    height: f32,
    bg: egui::Color32,
    face: Option<ttf_parser::Face<'a>>,
}

struct Outline<'a> {
    page: &'a Page<'a>,
    /// Where the glyph's origin is on the picture, and px per font unit.
    origin: egui::Pos2,
    scale: f32,
    path: String,
    last: (f32, f32),
}

impl Outline<'_> {
    fn pt(&self, x: f32, y: f32) -> (f32, f32) {
        // font units are y-up, the picture is y-down
        self.page.pt(egui::Pos2::new(self.origin.x + x * self.scale, self.origin.y - y * self.scale))
    }
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        let _ = writeln!(self.path, "{:.2} {:.2} m", p.0, p.1);
        self.last = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        let _ = writeln!(self.path, "{:.2} {:.2} l", p.0, p.1);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // a quadratic curve as the cubic with the same shape
        let (c, p) = (self.pt(x1, y1), self.pt(x, y));
        let s = self.last;
        let _ = writeln!(self.path, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", s.0 + 2.0 / 3.0 * (c.0 - s.0), s.1 + 2.0 / 3.0 * (c.1 - s.1), p.0 + 2.0 / 3.0 * (c.0 - p.0), p.1 + 2.0 / 3.0 * (c.1 - p.1), p.0, p.1);
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, p) = (self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        let _ = writeln!(self.path, "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c", a.0, a.1, b.0, b.1, p.0, p.1);
        self.last = p;
    }
    fn close(&mut self) {
        self.path.push_str("h\n");
    }
}

impl<'a> Page<'a> {
    fn pt(&self, p: egui::Pos2) -> (f32, f32) {
        ((p.x - self.min.x) * K, (self.height - (p.y - self.min.y)) * K)
    }

    fn fill_color(&mut self, c: egui::Color32) {
        let [r, g, b] = mix(c, self.bg);
        let _ = writeln!(self.out, "{r:.3} {g:.3} {b:.3} rg");
    }

    fn stroke_style(&mut self, s: egui::Stroke) {
        let [r, g, b] = mix(s.color, self.bg);
        let _ = writeln!(self.out, "{r:.3} {g:.3} {b:.3} RG {:.2} w 1 J 1 j", s.width * K);
    }

    /// Paint the path just drawn: filled, stroked or both.
    fn paint(&mut self, fill: egui::Color32, stroke: egui::Stroke) {
        let (f, s) = (visible(fill), stroke.width > 0.0 && visible(stroke.color));
        if f {
            self.fill_color(fill);
        }
        if s {
            self.stroke_style(stroke);
        }
        self.out.push_str(match (f, s) {
            (true, true) => "B\n",
            (true, false) => "f\n",
            (false, true) => "S\n",
            (false, false) => "n\n",
        });
    }

    fn shape(&mut self, shape: &egui::Shape) {
        use egui::Shape;
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| self.shape(s)),
            Shape::LineSegment { points, stroke } => {
                let (a, b) = (self.pt(points[0]), self.pt(points[1]));
                let _ = writeln!(self.out, "{:.2} {:.2} m {:.2} {:.2} l", a.0, a.1, b.0, b.1);
                self.paint(egui::Color32::TRANSPARENT, *stroke);
            }
            Shape::Circle(c) => {
                // four cubic curves
                const Q: f32 = 0.552_284_7;
                let (cx, cy) = self.pt(c.center);
                let r = c.radius * K;
                let k = r * Q;
                let _ = writeln!(
                    self.out,
                    "{:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h",
                    cx + r, cy,
                    cx + r, cy + k, cx + k, cy + r, cx, cy + r,
                    cx - k, cy + r, cx - r, cy + k, cx - r, cy,
                    cx - r, cy - k, cx - k, cy - r, cx, cy - r,
                    cx + k, cy - r, cx + r, cy - k, cx + r, cy
                );
                self.paint(c.fill, c.stroke);
            }
            Shape::Rect(r) => {
                let (a, b) = (self.pt(r.rect.left_bottom()), self.pt(r.rect.right_top()));
                let _ = writeln!(self.out, "{:.2} {:.2} {:.2} {:.2} re", a.0, a.1, b.0 - a.0, b.1 - a.1);
                self.paint(r.fill, r.stroke);
            }
            Shape::Path(p) if p.points.len() >= 2 => {
                for (i, q) in p.points.iter().enumerate() {
                    let q = self.pt(*q);
                    let _ = writeln!(self.out, "{:.2} {:.2} {}", q.0, q.1, if i == 0 { "m" } else { "l" });
                }
                if p.closed {
                    self.out.push_str("h\n");
                }
                let stroke = match &p.stroke.color {
                    ColorMode::Solid(c) => egui::Stroke::new(p.stroke.width, *c),
                    ColorMode::UV(_) => egui::Stroke::NONE,
                };
                self.paint(if p.closed { p.fill } else { egui::Color32::TRANSPARENT }, stroke);
            }
            Shape::Text(t) => self.text(t),
            // meshes (the grid dots), callbacks and curves are not part of the drawing
            _ => {}
        }
    }

    /// The letters as filled outlines. The line keeps the width egui laid it out with (the spacing between letters is adjusted).
    fn text(&mut self, t: &egui::epaint::TextShape) {
        let Some(face) = self.face.clone() else { return };
        let text = t.galley.text();
        if text.trim().is_empty() {
            return;
        }
        let sec = t.galley.job.sections.first().map(|s| s.format.clone());
        let size = sec.as_ref().map_or(13.0, |f| f.font_id.size);
        let mut color = sec.as_ref().map_or(t.fallback_color, |f| f.color);
        if color == egui::Color32::PLACEHOLDER {
            color = t.fallback_color;
        }
        if let Some(o) = t.override_text_color {
            color = o;
        }
        let g = t.galley.size();
        let upem = face.units_per_em() as f32;
        let scale = size / upem;
        let lines: Vec<&str> = text.split('\n').collect();
        let line_h = g.y / lines.len() as f32;
        // the middle of the line box sits at the middle between the font's ascent and descent
        let mid = (face.ascender() as f32 + face.descender() as f32) / 2.0 * scale;
        self.fill_color(color);
        for (n, line) in lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let adv: Vec<f32> = chars.iter().map(|c| face.glyph_index(*c).and_then(|gid| face.glyph_hor_advance(gid)).map_or(upem * 0.5, |a| a as f32) * scale).collect();
            let natural: f32 = adv.iter().sum();
            let extra = if chars.len() > 1 { ((g.x - natural) / (chars.len() - 1) as f32).clamp(-0.3 * size, 0.3 * size) } else { 0.0 };
            // one line is as wide as the whole galley; with several lines the widest one is, so only a single line is stretched
            let extra = if lines.len() == 1 { extra } else { 0.0 };
            let base_y = t.pos.y + line_h * (n as f32 + 0.5) + mid;
            let mut x = t.pos.x;
            let mut path = String::new();
            for (c, a) in chars.iter().zip(&adv) {
                if let Some(gid) = face.glyph_index(*c) {
                    let mut ob = Outline { page: self, origin: egui::Pos2::new(x, base_y), scale, path: String::new(), last: (0.0, 0.0) };
                    if face.outline_glyph(gid, &mut ob).is_some() {
                        path.push_str(&ob.path);
                    }
                }
                x += a + extra;
            }
            if !path.is_empty() {
                self.out.push_str(&path);
                // nonzero winding: the holes of the letters are drawn the other way round
                self.out.push_str("f\n");
            }
        }
    }
}

/// Where the drawing lies, so the page can be cut to it.
fn bounds_of(shapes: &[ClippedShape]) -> Option<egui::Rect> {
    let mut b: Option<egui::Rect> = None;
    fn walk(s: &egui::Shape, b: &mut Option<egui::Rect>) {
        use egui::Shape;
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, b)),
            Shape::LineSegment { .. } | Shape::Circle(_) | Shape::Rect(_) | Shape::Path(_) | Shape::Text(_) => {
                let r = s.visual_bounding_rect();
                if r.is_finite() && r.width() >= 0.0 && r.height() >= 0.0 {
                    *b = Some(b.map_or(r, |x| x.union(r)));
                }
            }
            _ => {}
        }
    }
    for c in shapes {
        walk(&c.shape, &mut b);
    }
    b
}

/// The drawing as the bytes of a one-page PDF on a `background` sheet, cut to what was drawn.
pub(super) fn shapes_to_pdf(shapes: &[ClippedShape], background: egui::Color32) -> Vec<u8> {
    let b = bounds_of(shapes).unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(100.0))).expand(BORDER);
    let (w, h) = (b.width().ceil(), b.height().ceil());
    let mut page = Page { out: String::new(), min: b.min, height: h, bg: background, face: ttf_parser::Face::parse(crate::startup::BUNDLED_FONT, 0).ok() };
    // the sheet
    page.fill_color(background);
    let _ = writeln!(page.out, "0 0 {:.2} {:.2} re f", w * K, h * K);
    for c in shapes {
        page.shape(&c.shape);
    }
    assemble(&page.out, w * K, h * K)
}

/// The file: catalog, page tree, one page and its content, with the table of byte offsets at the end.
fn assemble(content: &str, w: f32, h: f32) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![];
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Resources << >> /Contents 4 0 R >>").into_bytes(),
        [format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(), content.as_bytes(), b"endstream"].concat(),
    ];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        let r = e.sch.add(Kind::Resistor, (20, 20), 0);
        e.sch.components[r].value = "1k".into();
        e.sch.labels.push(Label { pos: (24, 20), text: "電源".into() });
        e.sch.title_block.title = "5 V 電源".into();
        e
    }

    #[test]
    fn an_empty_sheet_has_no_pdf() {
        assert!(SchEditor::default().sheet_pdf().is_none());
    }

    #[test]
    fn the_sheet_becomes_a_pdf_that_a_reader_opens_and_draws() {
        let bytes = sample().sheet_pdf().expect("a sheet with parts");
        assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n"));
        // the reader of the datasheet window opens it: one page, and the page has ink on it
        let doc = datasheet::Datasheet::open(bytes).expect("a valid PDF");
        assert_eq!(doc.page_count(), 1);
        let (w, h) = doc.page_size(0).unwrap();
        assert!(w > 50.0 && h > 50.0, "{w} x {h}");
        let img = doc.render(0, 1.0).unwrap();
        // the sheet is cream; anything darker is a stroke or a letter
        let dark = img.rgba.chunks(4).filter(|p| p[0] < 120 && p[1] < 120 && p[2] < 120).count();
        assert!(dark > 200, "the wires, the symbol and the letters are drawn ({dark} dark pixels)");
        let cream = img.rgba.chunks(4).filter(|p| p[0] > 240 && p[1] > 235 && p[2] > 220 && p[2] < 245).count();
        assert!(cream > img.rgba.len() / 8, "the background is the sheet colour");
    }

    #[test]
    fn the_text_is_drawn_as_outlines_even_for_japanese() {
        let with = sample().sheet_pdf().unwrap();
        let mut plain = sample();
        plain.sch.labels.clear();
        plain.sch.title_block = Default::default();
        let without = plain.sheet_pdf().unwrap();
        assert!(with.len() > without.len() + 500, "the letters add curves ({} vs {})", with.len(), without.len());
    }

    /// Look at a PDF by hand: `CCAD_PDF=in.pdf CCAD_PNG=out.png cargo test -p app look_at_pdf -- --ignored`.
    #[test]
    #[ignore]
    fn look_at_pdf() {
        let (Ok(i), Ok(o)) = (std::env::var("CCAD_PDF"), std::env::var("CCAD_PNG")) else { return };
        let doc = datasheet::Datasheet::open_file(std::path::Path::new(&i)).unwrap();
        let img = doc.render(0, 2.0).unwrap();
        image::save_buffer(o, &img.rgba, img.width as u32, img.height as u32, image::ColorType::Rgba8).unwrap();
    }
}
