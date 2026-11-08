//! Writing the sheet as an SVG picture (File menu, `--svg file.svg`).
//!
//! The sheet is painted by the same `draw_*` functions as the canvas, into a throwaway egui context; the shapes egui
//! produces (lines, circles, rectangles, polygons, text) are then written out as SVG elements. So the picture cannot
//! drift from the screen: a new symbol or label shows up in the file without any extra work. The grid, the selection,
//! the error rings and the editing marks are left out; the voltage and current figures are kept when they are showing.

use super::*;
use egui::epaint::{ClippedShape, ColorMode, Shape};

/// Pixels per grid step in the file (the canvas default is 22).
const EXPORT_ZOOM: f32 = 24.0;
/// Empty room around the parts (in grid steps) while painting, so labels and figures are not cut off.
const MARGIN: f32 = 12.0;
/// The empty border kept around the drawing in the file, in pixels.
const BORDER: f32 = 16.0;
const FONT_FAMILY: &str = "'BIZ UDGothic','BIZ UDPGothic','Yu Gothic UI','Meiryo','Hiragino Sans','Noto Sans CJK JP',sans-serif";

impl SchEditor {
    /// The shown sheet as an SVG document, or `None` when it has nothing on it.
    pub fn sheet_svg(&mut self) -> Option<String> {
        let mut pts: Vec<(f32, f32)> = vec![];
        for c in &self.sch.components {
            let (x0, x1, y0, y1) = comp_extents(c);
            for corner in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let r = rot_f(corner, c.rot);
                pts.push((c.pos.0 as f32 + r.0, c.pos.1 as f32 + r.1));
            }
        }
        pts.extend(self.sch.wires.iter().flat_map(|w| [w.a, w.b]).map(|p| (p.0 as f32, p.1 as f32)));
        pts.extend(self.sch.buses.iter().flat_map(|w| [w.a, w.b]).map(|p| (p.0 as f32, p.1 as f32)));
        pts.extend(self.sch.labels.iter().map(|l| (l.pos.0 as f32, l.pos.1 as f32)));
        pts.extend(self.sch.ports.iter().map(|p| (p.pos.0 as f32, p.pos.1 as f32)));
        if pts.is_empty() {
            return None;
        }
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let size = egui::Vec2::new((x1 - x0 + 2.0 * MARGIN) * EXPORT_ZOOM, (y1 - y0 + 2.0 * MARGIN) * EXPORT_ZOOM);
        let rect = Rect::from_min_size(Pos2::ZERO, size);

        // the canvas refreshes this once a frame; a file written before the first frame (`--dc --svg`) has to do it itself
        if self.dc.is_some() {
            self.sig_cache = self.signature();
        }
        // paint with the view and the selection set aside, and put them back afterwards
        let saved = (self.zoom, self.pan, self.selected.take(), std::mem::take(&mut self.multi));
        self.zoom = EXPORT_ZOOM;
        self.pan = Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);

        let ctx = egui::Context::default();
        crate::startup::install_japanese_font(&ctx);
        // the fonts take effect on the pass after they are set
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let input = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let painter = ui.painter_at(rect);
            self.draw_wires(&painter, rect);
            self.draw_buses(&painter, rect);
            self.draw_junctions(&painter, rect);
            self.draw_components(&painter, rect);
            self.draw_labels(&painter, rect);
            self.draw_ports(&painter, rect);
            self.draw_voltages(&painter, rect);
            self.draw_currents(&painter, rect);
            self.draw_title_block(&painter, rect);
        });

        out.textures_delta.clear();
        self.zoom = saved.0;
        self.pan = saved.1;
        self.selected = saved.2;
        self.multi = saved.3;
        Some(shapes_to_svg(&out.shapes, SCH_BG))
    }

    /// File → 回路図をSVGで保存: asks for the place and writes the file.
    pub fn export_svg(&mut self) {
        let Some(svg) = self.sheet_svg() else {
            self.status = "回路図が空なので、SVG に書き出せません。部品を置いてからもう一度お試しください".into();
            return;
        };
        let Some(path) = rfd::FileDialog::new().add_filter("SVG画像", &["svg"]).set_file_name("schematic.svg").save_file() else { return };
        self.status = match std::fs::write(&path, svg) {
            Ok(()) => format!("SVG を保存しました: {}", path.display()),
            Err(e) => format!("SVG を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください"),
        };
    }
}

fn rgb(c: egui::Color32) -> String {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `fill="..."` / `stroke="..."` pair of attributes for one colour (with an opacity when it is translucent).
fn paint(attr: &str, c: egui::Color32) -> String {
    if c == egui::Color32::TRANSPARENT || c.a() == 0 {
        return format!(" {attr}=\"none\"");
    }
    let a = c.to_srgba_unmultiplied()[3];
    if a == 255 {
        format!(" {attr}=\"{}\"", rgb(c))
    } else {
        format!(" {attr}=\"{}\" {attr}-opacity=\"{:.2}\"", rgb(c), a as f32 / 255.0)
    }
}

fn stroke_attrs(s: egui::Stroke) -> String {
    if s.width <= 0.0 || s.color.a() == 0 {
        return " stroke=\"none\"".into();
    }
    format!("{} stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"", paint("stroke", s.color), s.width)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes egui's shapes as an SVG document on a `background` sheet, cropped to what was drawn.
pub(super) fn shapes_to_svg(shapes: &[ClippedShape], background: egui::Color32) -> String {
    let mut body = String::new();
    let mut bounds: Option<Rect> = None;
    for clipped in shapes {
        add_shape(&clipped.shape, &mut body, &mut bounds);
    }
    let b = bounds.unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))).expand(BORDER);
    let (w, h) = (b.width().ceil(), b.height().ceil());
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"{:.1} {:.1} {w} {h}\" font-family=\"{FONT_FAMILY}\">\n<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{w}\" height=\"{h}\"{}/>\n{body}</svg>\n",
        b.min.x,
        b.min.y,
        b.min.x,
        b.min.y,
        paint("fill", background),
    )
}

fn add_shape(shape: &Shape, out: &mut String, bounds: &mut Option<Rect>) {
    let mut grow = |r: Rect| {
        if r.is_positive() || r.width() >= 0.0 && r.height() >= 0.0 {
            *bounds = Some(bounds.map_or(r, |b| b.union(r)));
        }
    };
    match shape {
        Shape::Vec(v) => {
            for s in v {
                add_shape(s, out, bounds);
            }
        }
        Shape::LineSegment { points, stroke } => {
            out.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\"{}/>\n",
                points[0].x, points[0].y, points[1].x, points[1].y, stroke_attrs(*stroke)
            ));
            grow(shape.visual_bounding_rect());
        }
        Shape::Circle(c) => {
            out.push_str(&format!("<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\"{}{}/>\n", c.center.x, c.center.y, c.radius, paint("fill", c.fill), stroke_attrs(c.stroke)));
            grow(shape.visual_bounding_rect());
        }
        Shape::Rect(r) => {
            let rx = r.corner_radius.nw as f32;
            out.push_str(&format!(
                "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"{rx}\"{}{}/>\n",
                r.rect.min.x, r.rect.min.y, r.rect.width(), r.rect.height(), paint("fill", r.fill), stroke_attrs(r.stroke)
            ));
            grow(shape.visual_bounding_rect());
        }
        Shape::Path(p) if p.points.len() >= 2 => {
            let pts: Vec<String> = p.points.iter().map(|q| format!("{:.2},{:.2}", q.x, q.y)).collect();
            let stroke = match &p.stroke.color {
                ColorMode::Solid(c) => stroke_attrs(egui::Stroke::new(p.stroke.width, *c)),
                ColorMode::UV(_) => " stroke=\"none\"".into(),
            };
            let tag = if p.closed { "polygon" } else { "polyline" };
            let fill = if p.closed { paint("fill", p.fill) } else { " fill=\"none\"".into() };
            out.push_str(&format!("<{tag} points=\"{}\"{fill}{stroke}/>\n", pts.join(" ")));
            grow(shape.visual_bounding_rect());
        }
        Shape::Text(t) => {
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
            // the text keeps the width egui laid it out with (letter spacing is adjusted), so right- and centre-aligned
            // labels stay where the canvas puts them whatever font the viewer picks
            out.push_str(&format!(
                "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{size:.1}\" dominant-baseline=\"central\" textLength=\"{:.2}\" lengthAdjust=\"spacing\"{}>{}</text>\n",
                t.pos.x,
                t.pos.y + g.y / 2.0,
                g.x,
                paint("fill", color),
                escape(text)
            ));
            grow(Rect::from_min_size(t.pos, g));
        }
        // meshes (the grid dots), callbacks and curves are not part of the drawing
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add(Kind::VSource, (4, 8), 0);
        let r = e.sch.add(Kind::Resistor, (14, 4), 0);
        e.sch.components[r].value = "1k".into();
        e.sch.wires.push(sch_core::Wire { a: (4, 6), b: (4, 4) });
        e.sch.wires.push(sch_core::Wire { a: (4, 4), b: (12, 4) });
        e.sch.labels.push(sch_core::Label { pos: (4, 10), text: "GND".into() });
        e
    }

    #[test]
    fn an_empty_sheet_has_no_svg() {
        assert!(SchEditor::default().sheet_svg().is_none());
    }

    #[test]
    fn the_sheet_becomes_an_svg_with_its_wires_parts_and_names() {
        let mut e = sample();
        e.selected = Some(Sel::Comp(1));
        let (zoom, pan) = (e.zoom, e.pan);
        let svg = e.sheet_svg().expect("a sheet with parts");
        assert!(svg.starts_with("<svg ") && svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<line "), "wires and symbol strokes");
        assert!(svg.contains(">R1</text>") || svg.contains(">R2</text>"), "the part's name: {svg}");
        assert!(svg.contains(">1k</text>") && svg.contains(">GND</text>"), "the value and the net label");
        // the sheet's colour is the background, and the selection colour (blue) is not used
        assert!(svg.contains("#faf8eb"), "background");
        assert!(!svg.contains("#0050dc"), "the selection is not drawn");
        // the editor's view and selection are as they were
        assert_eq!((e.zoom, e.pan), (zoom, pan));
        assert_eq!(e.selected, Some(Sel::Comp(1)));
    }

    #[test]
    fn text_is_escaped_and_the_viewbox_is_cropped_to_the_drawing() {
        let mut e = sample();
        e.sch.labels.push(sch_core::Label { pos: (20, 12), text: "A&B<1>".into() });
        let svg = e.sheet_svg().unwrap();
        assert!(svg.contains("A&amp;B&lt;1&gt;"));
        // the painting area was 24+ grid steps wide on each side; the file is cropped to the drawing plus a border
        let w: f32 = svg.split("width=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        assert!(w < 40.0 * EXPORT_ZOOM, "cropped, was {w}");
    }
}
