//! A picture of the board for someone who looks instead of reading numbers (the AI side of `ccad`, a report): one copper layer
//! with its pours, tracks, pads, vias and holes, the silk, a millimetre grid with its numbers, the nets to look at in a colour of
//! their own, and the places the DRC and the ratsnest complain about, numbered. The picture is drawn from the design, not from
//! the Gerber files, and the tracks are the plain ones (not the rounded and teared finish).

use crate::drc::{self, Connectivity};
use crate::gerber_view::{rasterize, Aperture, Frame, Prim};
use crate::model::*;
use crate::strokefont::text_strokes;

pub struct PictureOpts {
    /// The layer to show.
    pub side: Side,
    /// The part of the board to show, `(low corner, high corner)` in mm (the whole board when `None`).
    pub area: Option<(Point, Point)>,
    /// Pixels on the long side.
    pub max_side: usize,
    /// Nets drawn in a colour of their own (on top of the layer).
    pub highlight: Vec<String>,
    /// Number the DRC violations and draw the open connections.
    pub marks: bool,
    /// Show the other copper layers faintly under the layer.
    pub others: bool,
    /// Grid spacing in mm (0: a spacing that suits the picture).
    pub grid: f64,
    /// Write the net name beside every pad of the layer (to read which pad is which net without a list).
    pub net_names: bool,
    /// Parts to ring and name (designators): where they are.
    pub parts: Vec<String>,
    /// Routes to show as shadows (the way a net could be wired: not on the board): the net, its tracks and its vias.
    pub ghost: Vec<(String, Vec<Track>, Vec<Via>)>,
    /// Draw the copper with the board's finish (round corners, teardrops, filled notches) as the editor and the Gerber show it.
    pub finish: bool,
}

impl Default for PictureOpts {
    fn default() -> Self {
        PictureOpts { side: Side::Top, area: None, max_side: 1600, highlight: vec![], marks: false, others: false, grid: 0.0, net_names: false, parts: vec![], ghost: vec![], finish: false }
    }
}

pub struct Picture {
    pub w: usize,
    pub h: usize,
    /// RGBA, row by row from the top.
    pub rgba: Vec<u8>,
    /// What the numbers on the picture mean (one line each), and the scale.
    pub legend: Vec<String>,
}

const SUPERSAMPLE: usize = 2;

const BACKGROUND: [u8; 3] = [18, 20, 24];
const BOARD: [u8; 3] = [22, 54, 40];
const HOLE: [u8; 3] = [8, 8, 10];
const SILK: [u8; 3] = [235, 235, 235];
const MARK: [u8; 3] = [255, 60, 200];
const AIRWIRE: [u8; 3] = [255, 235, 40];
const HIGHLIGHTS: [[u8; 3]; 6] = [[255, 235, 40], [40, 230, 240], [255, 110, 255], [120, 255, 90], [255, 150, 40], [150, 160, 255]];

fn layer_colour(side: Side) -> [u8; 3] {
    match side {
        Side::Top => [205, 70, 55],
        Side::Bottom => [70, 120, 230],
        Side::Inner(1) => [215, 185, 60],
        Side::Inner(2) => [185, 90, 205],
        Side::Inner(_) => [90, 200, 160],
    }
}

/// One layer of the picture at the fine resolution: pixels as RGB floats.
struct Canvas {
    f: Frame,
    rgb: Vec<[f32; 3]>,
}

impl Canvas {
    fn new(f: Frame) -> Canvas {
        let bg = BACKGROUND.map(f32::from);
        Canvas { f, rgb: vec![bg; f.w * f.h] }
    }

    /// Paint `colour` where `prims` cover, with `alpha`.
    fn paint(&mut self, prims: &[Prim], colour: [u8; 3], alpha: f32) {
        if prims.is_empty() {
            return;
        }
        let cov = rasterize(prims, &self.f);
        let c = colour.map(f32::from);
        for (px, v) in self.rgb.iter_mut().zip(cov) {
            if v > 0 {
                for k in 0..3 {
                    px[k] = px[k] * (1.0 - alpha) + c[k] * alpha;
                }
            }
        }
    }

    /// Lines of `width` mm.
    fn lines(&mut self, segs: &[[Point; 2]], width: f64, colour: [u8; 3], alpha: f32) {
        let prims: Vec<Prim> = segs.iter().map(|s| Prim::Line { a: s[0], b: s[1], width, clear: false }).collect();
        self.paint(&prims, colour, alpha);
    }

    /// Text centred at `at`, `height` mm tall, in strokes of `width` mm.
    fn text(&mut self, at: Point, text: &str, height: f64, width: f64, colour: [u8; 3], alpha: f32) {
        let segs: Vec<[Point; 2]> = text_strokes(text, height, false).into_iter().map(|[a, b]| [Point::new(at.x + a.x, at.y + a.y), Point::new(at.x + b.x, at.y + b.y)]).collect();
        self.lines(&segs, width, colour, alpha);
    }

    /// The finished picture: each block of `SUPERSAMPLE` x `SUPERSAMPLE` pixels averaged into one.
    fn finish(&self) -> (usize, usize, Vec<u8>) {
        let (w, h) = (self.f.w / SUPERSAMPLE, self.f.h / SUPERSAMPLE);
        let mut out = vec![255u8; w * h * 4];
        for j in 0..h {
            for i in 0..w {
                let mut sum = [0.0f32; 3];
                for dj in 0..SUPERSAMPLE {
                    for di in 0..SUPERSAMPLE {
                        let p = self.rgb[(j * SUPERSAMPLE + dj) * self.f.w + i * SUPERSAMPLE + di];
                        for k in 0..3 {
                            sum[k] += p[k];
                        }
                    }
                }
                let n = (SUPERSAMPLE * SUPERSAMPLE) as f32;
                for k in 0..3 {
                    out[(j * w + i) * 4 + k] = (sum[k] / n).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        (w, h, out)
    }
}

fn aperture(shape: PadShape) -> Aperture {
    match shape {
        PadShape::Circle { d } => Aperture::Circle(d),
        PadShape::Rect { w, h } => Aperture::Rect(w, h),
        PadShape::Oval { w, h } => Aperture::Oval(w, h),
    }
}

/// The zone fills of a layer as regions (the holes of a fill cleared again); `only` limits them to one net.
fn zone_prims(fills: &[(Side, String, Vec<crate::zonefill::FillShape>)], side: Side, only: Option<&str>) -> Vec<Prim> {
    let mut out = vec![];
    for (s, net, shapes) in fills {
        if *s != side || only.is_some_and(|n| n != net) {
            continue;
        }
        for shape in shapes {
            out.push(Prim::Region { pts: shape.outer.clone(), clear: false });
            for h in &shape.holes {
                out.push(Prim::Region { pts: h.clone(), clear: true });
            }
        }
    }
    out
}

/// Tracks, pads and vias of a layer as primitives; `only` limits them to one net.
fn copper_prims(board: &Board, conn: &Connectivity, side: Side, only: Option<&str>) -> Vec<Prim> {
    let keep = |net: Option<&str>| only.is_none_or(|n| net == Some(n));
    let mut out = vec![];
    for (i, t) in board.tracks.iter().enumerate().filter(|(_, t)| t.side == side) {
        if keep(conn.track_net(i)) {
            out.push(Prim::Line { a: t.a, b: t.b, width: t.width, clear: false });
        }
    }
    for (j, p) in board.pads.iter().enumerate().filter(|(_, p)| p.drill.is_some() || p.side == side) {
        if keep(conn.pad_net(j)) {
            out.push(Prim::Flash { ap: aperture(p.shape), at: p.pos, clear: false });
        }
    }
    for (k, v) in board.vias.iter().enumerate() {
        if keep(conn.via_net(k)) {
            out.push(Prim::Flash { ap: Aperture::Circle(v.diameter), at: v.pos, clear: false });
        }
    }
    out
}

fn hole_prims(board: &Board) -> Vec<Prim> {
    let mut out = vec![];
    for p in &board.pads {
        if let Some(d) = p.drill {
            if p.slot > d {
                let off = (p.slot - d) / 2.0;
                let (a, b) = if p.slot_v { (Point::new(p.pos.x, p.pos.y - off), Point::new(p.pos.x, p.pos.y + off)) } else { (Point::new(p.pos.x - off, p.pos.y), Point::new(p.pos.x + off, p.pos.y)) };
                out.push(Prim::Line { a, b, width: d, clear: false });
            } else {
                out.push(Prim::Flash { ap: Aperture::Circle(d), at: p.pos, clear: false });
            }
        }
    }
    for v in &board.vias {
        out.push(Prim::Flash { ap: Aperture::Circle(v.drill), at: v.pos, clear: false });
    }
    for h in &board.holes {
        let (a, b) = h.ends();
        out.push(Prim::Line { a, b, width: h.drill, clear: false });
    }
    out
}

/// A grid spacing (mm) that leaves about 60 px between the lines.
fn nice_grid(px_per_mm: f64) -> f64 {
    let want = 60.0 / px_per_mm;
    [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0].into_iter().find(|g| *g >= want).unwrap_or(100.0)
}

/// The shortest way to say a number in the legend.
fn mm(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn render(board: &Board, opts: &PictureOpts) -> Picture {
    let (lo, hi) = opts.area.or_else(|| board.bounds()).unwrap_or((Point::new(0.0, 0.0), Point::new(10.0, 10.0)));
    let base = Frame::around(lo, hi, opts.max_side.clamp(200, 4000));
    let fine = Frame { x0: base.x0, y1: base.y1, px: base.px * SUPERSAMPLE as f64, w: base.w * SUPERSAMPLE, h: base.h * SUPERSAMPLE };
    let mut cv = Canvas::new(fine);
    // with the finish, the tracks are the derived ones and the patches are drawn as well
    let finished = if opts.finish { crate::smooth::finished(board) } else { None };
    let drawn: &Board = finished.as_ref().map_or(board, |f| &f.board);
    let conn = drc::connectivity(drawn);
    let fills = crate::zonefill::merged_fills(drawn, &conn);
    let patch_prims = |side: Side, only: Option<&str>| -> Vec<Prim> {
        finished.iter().flat_map(|f| f.patches.iter()).filter(|p| p.side == side && only.is_none_or(|n| conn.track_net(p.track) == Some(n))).map(|p| Prim::Region { pts: p.outline.clone(), clear: false }).collect()
    };
    let px = base.px; // pixels per mm in the finished picture
    let one_px = 1.0 / px; // a finished pixel in mm
    let mut legend = vec![format!("範囲 x {}〜{} mm、y {}〜{} mm、1 mm = {} px、格子の数字は mm (x は上の縁、y は左の縁)", mm(base.x0), mm(base.x0 + base.w as f64 / px), mm(base.y1 - base.h as f64 / px), mm(base.y1), mm(px))];
    legend.push(format!("層: {}", opts.side.label()));

    // the board
    if board.outline.len() >= 3 {
        cv.paint(&[Prim::Region { pts: board.outline.clone(), clear: false }], BOARD, 1.0);
    }
    // the other layers, faint
    if opts.others {
        for s in board.copper_layers().into_iter().filter(|s| *s != opts.side) {
            cv.paint(&zone_prims(&fills, s, None), layer_colour(s), 0.12);
            cv.paint(&copper_prims(drawn, &conn, s, None), layer_colour(s), 0.2);
        }
    }
    // the layer: pours, then tracks, pads and vias
    let colour = layer_colour(opts.side);
    cv.paint(&zone_prims(&fills, opts.side, None), colour, 0.35);
    cv.paint(&copper_prims(drawn, &conn, opts.side, None), colour, 0.9);
    cv.paint(&patch_prims(opts.side, None), colour, 0.9);
    cv.paint(&hole_prims(board), HOLE, 1.0);
    // the nets to look at
    for (k, net) in opts.highlight.iter().enumerate() {
        let c = HIGHLIGHTS[k % HIGHLIGHTS.len()];
        cv.paint(&zone_prims(&fills, opts.side, Some(net)), c, 0.55);
        cv.paint(&copper_prims(drawn, &conn, opts.side, Some(net)), c, 0.95);
        cv.paint(&patch_prims(opts.side, Some(net)), c, 0.95);
        cv.paint(&hole_prims(board), HOLE, 1.0);
        legend.push(format!("ネット {net}: {}", ["黄", "水色", "ピンク", "黄緑", "橙", "薄紫"][k % HIGHLIGHTS.len()]));
    }
    // the shadows: wide and see-through, with a thin line down the middle, a ring for each via
    for (k, (net, tracks, vias)) in opts.ghost.iter().enumerate() {
        // the colour of the net when it is highlighted too, else the next free one
        let ci = opts.highlight.iter().position(|h| h == net).unwrap_or(opts.highlight.len() + opts.ghost.iter().take(k).filter(|(n, _, _)| !opts.highlight.contains(n)).count());
        let c = HIGHLIGHTS[ci % HIGHLIGHTS.len()];
        let mine: Vec<&Track> = tracks.iter().filter(|t| t.side == opts.side).collect();
        let wide: Vec<Prim> = mine.iter().map(|t| Prim::Line { a: t.a, b: t.b, width: t.width.max(8.0 * one_px), clear: false }).collect();
        cv.paint(&wide, c, 0.4);
        let centre: Vec<[Point; 2]> = mine.iter().map(|t| [t.a, t.b]).collect();
        cv.lines(&centre, one_px * 1.5, c, 1.0);
        let rings: Vec<Prim> = vias.iter().map(|v| Prim::Flash { ap: Aperture::Circle(v.diameter.max(10.0 * one_px)), at: v.pos, clear: false }).collect();
        cv.paint(&rings, c, 0.6);
        let length: f64 = tracks.iter().map(|t| t.a.dist(t.b)).sum();
        legend.push(format!("影 (おすすめの取り回し) {net}: {}、全層の長さ {} mm、ビア {} 個 (この層に描いた線は {} 本)", ["黄", "水色", "ピンク", "黄緑", "橙", "薄紫"][ci % HIGHLIGHTS.len()], mm((length * 10.0).round() / 10.0), vias.len(), mine.len()));
    }
    // the silk of an outer layer
    if !opts.side.is_inner() {
        let lines: Vec<[Point; 2]> = board.silk.iter().filter(|l| l.side == opts.side).map(|l| [l.a, l.b]).collect();
        cv.lines(&lines, 0.15, SILK, 0.8);
        for t in board.texts.iter().filter(|t| t.side == opts.side) {
            let segs: Vec<[Point; 2]> = t.strokes().into_iter().map(|[a, b]| [Point::new(t.pos.x + a.x, t.pos.y + a.y), Point::new(t.pos.x + b.x, t.pos.y + b.y)]).collect();
            cv.lines(&segs, (t.height / 8.0).max(0.1), SILK, 0.8);
        }
    }
    // the grid and its numbers
    let step = if opts.grid > 0.0 { opts.grid } else { nice_grid(px) };
    let (x_lo, x_hi) = (base.x0, base.x0 + base.w as f64 / px);
    let (y_lo, y_hi) = (base.y1 - base.h as f64 / px, base.y1);
    let mut grid: Vec<[Point; 2]> = vec![];
    let mut x = (x_lo / step).ceil() * step;
    while x <= x_hi {
        grid.push([Point::new(x, y_lo), Point::new(x, y_hi)]);
        x += step;
    }
    let mut y = (y_lo / step).ceil() * step;
    while y <= y_hi {
        grid.push([Point::new(x_lo, y), Point::new(x_hi, y)]);
        y += step;
    }
    cv.lines(&grid, one_px, [255, 255, 255], 0.13);
    let label_h = 9.0 * one_px;
    let mut x = (x_lo / step).ceil() * step;
    while x <= x_hi {
        cv.text(Point::new(x + 0.5 * one_px * 12.0, y_hi - label_h), &mm(x), label_h, one_px * 1.2, [255, 255, 255], 0.75);
        x += step;
    }
    let mut y = (y_lo / step).ceil() * step;
    while y <= y_hi {
        let t = mm(y);
        cv.text(Point::new(x_lo + crate::strokefont::text_width(&t, label_h) / 2.0 + one_px * 3.0, y + label_h), &t, label_h, one_px * 1.2, [255, 255, 255], 0.75);
        y += step;
    }
    // the net of every pad, small, over the copper
    if opts.net_names {
        let h = (0.5f64).max(9.0 * one_px);
        for (j, p) in board.pads.iter().enumerate().filter(|(_, p)| p.drill.is_some() || p.side == opts.side) {
            let Some(net) = conn.pad_net(j).filter(|n| !n.is_empty()) else { continue };
            let (w, ph) = p.shape.size();
            // beside the pad when it is wide, above it otherwise
            let at = Point::new(p.pos.x, p.pos.y + ph / 2.0 + h * 0.8);
            let _ = w;
            cv.text(at, net, h, (h / 7.0).max(one_px * 1.1), [150, 255, 200], 0.95);
        }
    }
    // the parts that were asked for
    for r in &opts.parts {
        let Some(part) = board.parts.iter().find(|p| &p.refdes == r) else {
            legend.push(format!("部品 {r}: 基板にありません"));
            continue;
        };
        let rad = 2.5;
        let ring: Vec<[Point; 2]> = (0..32).map(|k| {
            let (a, b) = (std::f64::consts::TAU * k as f64 / 32.0, std::f64::consts::TAU * (k as f64 + 1.0) / 32.0);
            [Point::new(part.pos.x + rad * a.cos(), part.pos.y + rad * a.sin()), Point::new(part.pos.x + rad * b.cos(), part.pos.y + rad * b.sin())]
        }).collect();
        cv.lines(&ring, one_px * 2.0, [255, 160, 40], 1.0);
        cv.text(Point::new(part.pos.x, part.pos.y + rad + 12.0 * one_px), r, 12.0 * one_px, one_px * 1.8, [255, 160, 40], 1.0);
        legend.push(format!("部品 {r}: ({}, {}) 面 {}", mm(part.pos.x), mm(part.pos.y), part.side.label()));
    }
    // the places to look at
    if opts.marks {
        let open = crate::ratsnest::open_connections(board);
        for w in &open {
            cv.lines(&[[w.a, w.b]], one_px * 2.0, AIRWIRE, 0.9);
        }
        // with the finish the places are those of the finished copper (what the editor and the Gerber files show)
        let viol = if opts.finish { drc::check_final(board) } else { drc::check(board) };
        for (n, v) in viol.iter().enumerate() {
            let r = 14.0 * one_px;
            let ring: Vec<[Point; 2]> = (0..24).map(|k| {
                let (a, b) = (std::f64::consts::TAU * k as f64 / 24.0, std::f64::consts::TAU * (k as f64 + 1.0) / 24.0);
                [Point::new(v.at.x + r * a.cos(), v.at.y + r * a.sin()), Point::new(v.at.x + r * b.cos(), v.at.y + r * b.sin())]
            }).collect();
            cv.lines(&ring, one_px * 2.0, MARK, 1.0);
            cv.text(Point::new(v.at.x + r + 8.0 * one_px, v.at.y + r), &(n + 1).to_string(), 12.0 * one_px, one_px * 1.8, MARK, 1.0);
            legend.push(format!("{}: ({}, {}) {}", n + 1, mm(v.at.x), mm(v.at.y), v.message));
        }
        legend.push(format!("黄色の線: 未配線 {} 本。ピンクの輪と番号: DRC 違反", open.len()));
    }
    let (w, h, rgba) = cv.finish();
    Picture { w, h, rgba, legend }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("t", 30.0, 20.0);
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.0), b: Point::new(25.0, 5.0), width: 1.0 });
        let mut p = Pad::new(Point::new(5.0, 5.0), PadShape::Rect { w: 2.0, h: 2.0 }, None, Side::Top);
        p.net = "A".into();
        b.pads.push(p);
        b
    }

    fn at(pic: &Picture, mm_x: f64, mm_y: f64, area_lo: Point, area_hi: Point, max_side: usize) -> [u8; 3] {
        let f = Frame::around(area_lo, area_hi, max_side);
        let i = ((mm_x - f.x0) * f.px) as usize;
        let j = ((f.y1 - mm_y) * f.px) as usize;
        let k = (j * pic.w + i) * 4;
        [pic.rgba[k], pic.rgba[k + 1], pic.rgba[k + 2]]
    }

    #[test]
    fn the_track_is_copper_coloured_and_the_highlight_changes_it() {
        let b = board();
        let (lo, hi) = b.bounds().unwrap();
        let plain = render(&b, &PictureOpts { max_side: 600, ..Default::default() });
        assert_eq!(plain.w * plain.h * 4, plain.rgba.len());
        let c = at(&plain, 15.0, 5.0, lo, hi, 600);
        assert!(c[0] > 150 && c[1] < 120, "copper on the top is red: {c:?}");
        let off_track = at(&plain, 15.0, 12.0, lo, hi, 600);
        assert!(off_track[0] < 60, "bare board: {off_track:?}");
        let lit = render(&b, &PictureOpts { max_side: 600, highlight: vec!["A".into()], ..Default::default() });
        let c2 = at(&lit, 5.0, 5.0, lo, hi, 600);
        assert!(c2[1] > 200, "the pad of net A is drawn in the highlight colour: {c2:?}");
        assert!(lit.legend.iter().any(|l| l.contains("ネット A")));
    }

    #[test]
    fn a_crop_is_bigger_per_mm_and_the_marks_are_numbered() {
        let mut b = board();
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 5.8), b: Point::new(25.0, 5.8), width: 0.4 }); // 0.1 mm from the first, not touching: too near
        let mut q = Pad::new(Point::new(25.0, 5.8), PadShape::Rect { w: 0.4, h: 0.4 }, None, Side::Top);
        q.net = "B".into();
        b.pads.push(q);
        let whole = render(&b, &PictureOpts { max_side: 600, ..Default::default() });
        let crop = render(&b, &PictureOpts { max_side: 600, area: Some((Point::new(4.0, 3.0), Point::new(10.0, 8.0))), marks: true, ..Default::default() });
        assert!(crop.legend[0].contains("1 mm ="));
        assert!(crop.legend.iter().any(|l| l.starts_with("1: (")), "{:?}", crop.legend);
        assert!(whole.legend.len() < crop.legend.len());
    }
}
