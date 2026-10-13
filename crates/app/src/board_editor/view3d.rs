//! "3Dで見る" window: the board as the fab will make it (the picture rebuilt from the exported
//! fabrication files, put on the top and bottom faces) with solids for every part. Drawn by a small
//! software rasteriser (`raster3d`): perspective projection, back-face culling and a depth buffer, so what is
//! nearer always hides what is behind it. Hovering a part names it, clicking selects it in the board editor.

use super::raster3d::{outline_part, Buffers, Paint, Picture, Vert, NO_PART};
use super::*;
use pcb_core::gerber_view::Preview;
use pcb_core::model3d::{part_boxes, part_height_on, BodyShape, PartBox, BOARD_THICKNESS};

/// What the window shows, built from the board when it is opened or refreshed.
struct Scene {
    pv: Preview,
    /// The board picture of the top and of the bottom (what the faces are painted with).
    img: [Vec<[u8; 4]>; 2],
    /// Outline in counter-clockwise order, and its triangles.
    outline: Vec<Point>,
    tris: Vec<[Point; 3]>,
    lo: Point,
    hi: Point,
    boxes: Vec<PartBox>,
    /// `(refdes, value, footprint, height above the board)` for every part, in `Board::parts` order.
    parts: Vec<(String, String, String, f64)>,
    /// The tallest part on each side, for case checks.
    tallest: [Option<(String, f64)>; 2],
    /// Every net name on the board, for the highlight list.
    nets: Vec<String>,
    /// For every part: how high it stands above the face it is on (mm), and whether that face is the bottom.
    heights: Vec<f64>,
    bottoms: Vec<bool>,
    /// How far (mm) each part is over the case height (0 = within it, or not checked), the limits drawn as rings
    /// (top, bottom), and the settings these were computed for.
    over: Vec<f64>,
    rings: [Option<f64>; 2],
    applied: Option<(bool, u64, u64)>,
    /// Copper of the highlighted net as triangles (`true` = bottom face), drawn just above the board picture.
    glow: Vec<(bool, [Point; 3])>,
}

/// Colour of the highlighted net, and how far (mm) it floats above the board so it wins the depth test.
const GLOW: [u8; 3] = [255, 60, 170];
const GLOW_LIFT: f64 = 0.05;
/// Largest zoom (the scroll wheel and "選択部品へ寄る" share it).
const MAX_ZOOM: f32 = 30.0;

pub(super) struct View3d {
    pub(super) open: bool,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
    spin: bool,
    show_parts: bool,
    show_names: bool,
    mask: bool,
    silk: bool,
    scene: Option<Scene>,
    error: Option<String>,
    /// Rebuild the scene on the next frame (set when the window opens).
    stale: bool,
    /// The part under the pointer in the last picture (drawn brighter in the next one).
    hover: Option<usize>,
    /// Changes whenever the scene or its pictures change, so a kept picture is not reused for another.
    generation: u32,
    /// The last picture, kept while nothing that shows in it changes.
    cache: Option<Cached>,
    /// The net drawn in colour on the board, and the net of the editor's selection last followed (so picking
    /// another net in the list is not undone by the same selection every frame).
    net: Option<String>,
    followed: Option<String>,
    /// A part to move the camera to on the next frame.
    focus: Option<usize>,
    /// The case height check: on/off and the room above the top face and below the bottom face (mm).
    check_height: bool,
    limit_top: f64,
    limit_bottom: f64,
}

/// A finished picture of the scene: what it was drawn for, the image, and the part at each pixel.
struct Cached {
    key: PictureKey,
    tex: egui::TextureHandle,
    ids: Vec<u32>,
    size: [usize; 2],
}

#[derive(PartialEq)]
struct PictureKey {
    camera: [u32; 5],
    size: [usize; 2],
    parts: bool,
    hover: Option<usize>,
    selected: Option<usize>,
    generation: u32,
    /// Drawn coarsely (while the view is being dragged or zoomed) or fully.
    draft: bool,
}

impl Default for View3d {
    fn default() -> Self {
        Self { open: false, yaw: 0.0, pitch: 0.9, zoom: 1.0, pan: Vec2::ZERO, spin: false, show_parts: true, show_names: false, mask: true, silk: true, scene: None, error: None, stale: true, hover: None, generation: 0, cache: None, net: None, followed: None, focus: None, check_height: false, limit_top: 15.0, limit_bottom: 5.0 }
    }
}

/// The camera: where world points land on the screen.
#[derive(Clone, Copy)]
pub(super) struct Cam {
    pub yaw: f32,
    pub pitch: f32,
    pub scale: f32,
    pub center: [f32; 3],
    pub dist: f32,
    pub origin: Pos2,
}

impl Cam {
    /// A direction in view space: (right, up, towards the viewer). Used for lighting and culling.
    pub(super) fn dir(&self, v: [f32; 3]) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let (x1, y1) = (v[0] * cy - v[1] * sy, v[0] * sy + v[1] * cy);
        [x1, y1 * sp + v[2] * cp, -y1 * cp + v[2] * sp]
    }

    /// Screen position of a world point and how near the viewer it is (bigger = nearer).
    pub(super) fn project(&self, p: [f64; 3]) -> (Pos2, f32) {
        let rel = [p[0] as f32 - self.center[0], p[1] as f32 - self.center[1], p[2] as f32 - self.center[2]];
        let [x, up, w] = self.dir(rel);
        let f = self.dist / (self.dist - w).max(self.dist * 0.2);
        (Pos2::new(self.origin.x + x * f * self.scale, self.origin.y - up * f * self.scale), w)
    }
}

/// Brightness for a face whose normal (view space) is `n`: lit from above-left of the viewer.
fn light(n: [f32; 3]) -> f32 {
    let l = [-0.35f32, 0.45, 0.82];
    0.45 + 0.6 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

type Face3 = (Vec<[f64; 3]>, [f32; 3]);

/// The faces of a solid (corners, outward normal): a box has six, a cylinder a ring of sides and two caps.
fn solid_faces(b: &PartBox) -> Vec<Face3> {
    let (l, h) = (b.lo, b.hi);
    match b.shape {
        BodyShape::Box => vec![
            (vec![[l[0], l[1], h[2]], [h[0], l[1], h[2]], [h[0], h[1], h[2]], [l[0], h[1], h[2]]], [0.0, 0.0, 1.0]),
            (vec![[l[0], l[1], l[2]], [l[0], h[1], l[2]], [h[0], h[1], l[2]], [h[0], l[1], l[2]]], [0.0, 0.0, -1.0]),
            (vec![[l[0], l[1], l[2]], [h[0], l[1], l[2]], [h[0], l[1], h[2]], [l[0], l[1], h[2]]], [0.0, -1.0, 0.0]),
            (vec![[h[0], h[1], l[2]], [l[0], h[1], l[2]], [l[0], h[1], h[2]], [h[0], h[1], h[2]]], [0.0, 1.0, 0.0]),
            (vec![[l[0], h[1], l[2]], [l[0], l[1], l[2]], [l[0], l[1], h[2]], [l[0], h[1], h[2]]], [-1.0, 0.0, 0.0]),
            (vec![[h[0], l[1], l[2]], [h[0], h[1], l[2]], [h[0], h[1], h[2]], [h[0], l[1], h[2]]], [1.0, 0.0, 0.0]),
        ],
        BodyShape::Cylinder(axis) => {
            let a = axis as usize;
            let (i, j) = ((a + 1) % 3, (a + 2) % 3);
            let (ci, cj) = ((l[i] + h[i]) / 2.0, (l[j] + h[j]) / 2.0);
            let (ri, rj) = ((h[i] - l[i]) / 2.0, (h[j] - l[j]) / 2.0);
            const N: usize = 32;
            let at = |k: usize, along: f64| {
                let t = std::f64::consts::TAU * k as f64 / N as f64;
                let mut p = [0.0; 3];
                p[a] = along;
                p[i] = ci + ri * t.cos();
                p[j] = cj + rj * t.sin();
                p
            };
            let mut faces = vec![];
            for k in 0..N {
                let tm = std::f64::consts::TAU * (k as f64 + 0.5) / N as f64;
                let mut n = [0.0f32; 3];
                n[i] = tm.cos() as f32;
                n[j] = tm.sin() as f32;
                faces.push((vec![at(k, l[a]), at(k + 1, l[a]), at(k + 1, h[a]), at(k, h[a])], n));
            }
            let (mut up, mut down) = ([0.0f32; 3], [0.0f32; 3]);
            up[a] = 1.0;
            down[a] = -1.0;
            faces.push(((0..N).map(|k| at(k, h[a])).collect(), up));
            faces.push(((0..N).rev().map(|k| at(k, l[a])).collect(), down));
            faces
        }
    }
}

fn shade3(c: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.3);
    [(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8]
}

impl Scene {
    fn build(board: &Board, ctx: &egui::Context, mask: bool, silk: bool) -> Result<Scene, String> {
        let _ = ctx;
        let files = export::fab_files(board);
        let pv = Preview::from_files(&files, 1000)?;
        let mut outline = board.outline.clone();
        if pcb_core::geom::signed_area(&outline) < 0.0 {
            outline.reverse();
        }
        let tris = triangulate(&outline);
        if tris.is_empty() {
            return Err("基板の外形が面になっていません。外形を決めてから開いてください".into());
        }
        let (lo, hi) = board.bounds().ok_or("基板の外形がありません")?;
        let img = Self::images(&pv, mask, silk);
        let parts: Vec<(String, String, String, f64)> = board.parts.iter().map(|p| (p.refdes.clone(), p.value.clone(), p.footprint.clone(), part_height_on(board, p))).collect();
        let boxes = part_boxes(board);
        let mut tallest: [Option<(String, f64)>; 2] = [None, None];
        for b in &boxes {
            let side = b.bottom as usize;
            let height = if b.bottom { -b.lo[2] } else { b.hi[2] - BOARD_THICKNESS };
            if tallest[side].as_ref().is_none_or(|t| height > t.1) {
                tallest[side] = Some((b.refdes.clone(), height));
            }
        }
        let mut heights = vec![0.0f64; board.parts.len()];
        for b in &boxes {
            let height = if b.bottom { -b.lo[2] } else { b.hi[2] - BOARD_THICKNESS };
            if let Some(h) = heights.get_mut(b.part) {
                *h = h.max(height);
            }
        }
        let bottoms: Vec<bool> = board.parts.iter().map(|p| p.side == Side::Bottom).collect();
        let over = vec![0.0; board.parts.len()];
        let mut nets: Vec<String> = board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.clone()).collect();
        nets.sort();
        nets.dedup();
        Ok(Scene { pv, img, outline, tris, lo, hi, boxes, parts, tallest, heights, bottoms, over, rings: [None; 2], applied: None, nets, glow: vec![] })
    }

    /// Marks the parts that stand higher than the case allows (`top` / `bottom` mm above each face; 0 = that face
    /// is not checked). Returns whether anything changed, so the picture is redrawn only then.
    fn set_limits(&mut self, on: bool, top: f64, bottom: f64) -> bool {
        let key = Some((on, top.to_bits(), bottom.to_bits()));
        if self.applied == key {
            return false;
        }
        self.applied = key;
        let lim = |side_bottom: bool| if !on { 0.0 } else if side_bottom { bottom } else { top };
        self.over = (0..self.parts.len()).map(|i| if lim(self.bottoms[i]) > 0.0 { (self.heights[i] - lim(self.bottoms[i])).max(0.0) } else { 0.0 }).collect();
        self.rings = [Some(lim(false)).filter(|l| *l > 0.0), Some(lim(true)).filter(|l| *l > 0.0)];
        true
    }

    /// The parts over the case height, worst first: `(refdes, mm over)`.
    fn over_list(&self) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = self.over.iter().enumerate().filter(|(_, o)| **o > 1e-6).map(|(i, o)| (self.parts[i].0.clone(), *o)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }

    fn images(pv: &Preview, mask: bool, silk: bool) -> [Vec<[u8; 4]>; 2] {
        [pv.render(false, mask, silk), pv.render(true, mask, silk)]
    }

    /// The whole scene into a depth-buffered image of `w` x `h` samples. `cam` must already be in sample units
    /// (its origin relative to the image). `hover` is drawn brighter.
    fn render(&self, cam: &Cam, w: usize, h: usize, show_parts: bool, hover: Option<usize>) -> Buffers {
        let mut buf = Buffers::new(w, h, [34, 38, 46]);
        let pics = [
            Picture { w: self.pv.frame.w, h: self.pv.frame.h, rgba: &self.img[0] },
            Picture { w: self.pv.frame.w, h: self.pv.frame.h, rgba: &self.img[1] },
        ];
        let vert = |p: [f64; 3], uv: [f32; 2]| {
            let (pos, depth) = cam.project(p);
            Vert { pos, q: 1.0 / (cam.dist - depth).max(cam.dist * 0.2), uv }
        };
        // the top and bottom faces: the board picture mapped by the picture's own frame (it has a margin around the
        // outline), so the copper lies exactly where the parts' pads are
        let fr = self.pv.frame;
        for bottom in [false, true] {
            let up = if bottom { -1.0 } else { 1.0 };
            if cam.dir([0.0, 0.0, up])[2] <= 1e-4 {
                continue;
            }
            let z = if bottom { 0.0 } else { BOARD_THICKNESS };
            for t in &self.tris {
                let corner = |p: &Point| {
                    let u = ((p.x - fr.x0) * fr.px / fr.w as f64) as f32;
                    let v = ((fr.y1 - p.y) * fr.px / fr.h as f64) as f32;
                    // the bottom picture is the view from the back: mirrored left to right
                    vert([p.x, p.y, z], [if bottom { 1.0 - u } else { u }, v])
                };
                buf.triangle([corner(&t[0]), corner(&t[1]), corner(&t[2])], Paint::Texture(bottom as usize), &pics);
            }
            let zg = if bottom { -GLOW_LIFT } else { BOARD_THICKNESS + GLOW_LIFT };
            for (_, t) in self.glow.iter().filter(|(b, _)| *b == bottom) {
                let vs = t.map(|p| vert([p.x, p.y, zg], [0.0, 0.0]));
                buf.triangle(vs, Paint::Flat { color: GLOW, part: NO_PART }, &pics);
            }
        }
        // the edge of the board
        let n = self.outline.len();
        for i in 0..n {
            let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            // counter-clockwise outline: the outside is to the right of the direction of travel
            let nv = cam.dir([(dy / len) as f32, (-dx / len) as f32, 0.0]);
            if nv[2] <= 1e-4 {
                continue;
            }
            let c = |x: f64, y: f64, z: f64| vert([x, y, z], [0.0, 0.0]);
            let (p0, p1, p2, p3) = (c(a.x, a.y, 0.0), c(b.x, b.y, 0.0), c(b.x, b.y, BOARD_THICKNESS), c(a.x, a.y, BOARD_THICKNESS));
            let paint = Paint::Flat { color: shade3([176, 150, 92], light(nv)), part: NO_PART };
            buf.triangle([p0, p1, p2], paint, &pics);
            buf.triangle([p0, p2, p3], paint, &pics);
        }
        // the case height as a ring round the outline, above the top face and below the bottom face
        for (k, ring) in self.rings.iter().enumerate() {
            if let Some(l) = ring {
                let z = if k == 0 { BOARD_THICKNESS + l } else { -l };
                for i in 0..n {
                    let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
                    buf.line(vert([a.x, a.y, z], [0.0, 0.0]), vert([b.x, b.y, z], [0.0, 0.0]), [255, 170, 40]);
                }
            }
        }
        if !show_parts {
            return buf;
        }
        for b in &self.boxes {
            for (corners, normal) in solid_faces(b) {
                let nv = cam.dir(normal);
                if nv[2] <= 1e-4 {
                    continue;
                }
                let too_tall = self.over.get(b.part).is_some_and(|o| *o > 1e-6);
                let mut color = shade3(if too_tall { [225, 50, 40] } else { b.color }, light(nv));
                if hover == Some(b.part) {
                    color = [color[0].saturating_add(50), color[1].saturating_add(50), color[2].saturating_add(30)];
                }
                let vs: Vec<Vert> = corners.iter().map(|p| vert(*p, [0.0, 0.0])).collect();
                let paint = Paint::Flat { color, part: b.part as u32 };
                for k in 1..vs.len().saturating_sub(1) {
                    buf.triangle([vs[0], vs[k], vs[k + 1]], paint, &pics);
                }
                // a thin dark edge round every face, so the shape reads even where two faces are lit alike
                let dark = shade3(b.color, 0.4);
                for k in 0..vs.len() {
                    buf.line(vs[k], vs[(k + 1) % vs.len()], dark);
                }
            }
        }
        buf
    }
}

/// The camera for a window of `rect`: the longest side of the board fills most of it at zoom 1.
fn camera(scene: &Scene, rect: Rect, yaw: f32, pitch: f32, zoom: f32, pan: Vec2) -> Cam {
    let longest = (scene.hi.x - scene.lo.x).max(scene.hi.y - scene.lo.y) as f32;
    // the camera stands back three times the board's diagonal
    let extent = ((scene.hi.x - scene.lo.x).hypot(scene.hi.y - scene.lo.y)) as f32;
    Cam {
        yaw,
        pitch,
        scale: 0.95 * rect.width().min(rect.height()) / (longest * 1.15).max(1.0) * zoom,
        center: [((scene.lo.x + scene.hi.x) / 2.0) as f32, ((scene.lo.y + scene.hi.y) / 2.0) as f32, (BOARD_THICKNESS / 2.0) as f32],
        dist: extent * 3.0,
        origin: rect.center() + pan,
    }
}

/// Where to put the camera to look at part `part` closely: `(pitch, zoom, pan)`. A part on the far face turns the
/// board over (looks from below) so it is seen, not hidden behind the board.
fn focus_on(scene: &Scene, part: usize, rect: Rect, yaw: f32, pitch: f32) -> Option<(f32, f32, Vec2)> {
    let mut boxes = scene.boxes.iter().filter(|b| b.part == part);
    let first = boxes.next()?;
    let (mut lo, mut hi) = (first.lo, first.hi);
    let mut bottom = first.bottom;
    for b in boxes {
        for k in 0..3 {
            lo[k] = lo[k].min(b.lo[k]);
            hi[k] = hi[k].max(b.hi[k]);
        }
        bottom |= b.bottom;
    }
    let pitch = if bottom && pitch.sin() > 0.0 {
        -0.9
    } else if !bottom && pitch.sin() < 0.0 {
        0.9
    } else {
        pitch
    };
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() as f32;
    let base = camera(scene, rect, yaw, pitch, 1.0, Vec2::ZERO).scale;
    let zoom = (0.4 * rect.width().min(rect.height()) / diag.max(2.0) / base).clamp(0.2, MAX_ZOOM);
    let at = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let (pos, _) = camera(scene, rect, yaw, pitch, zoom, Vec2::ZERO).project(at);
    Some((pitch, zoom, rect.center() - pos))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Quality {
    /// While the view is moved: fast and a little blurry.
    Draft,
    /// Rendered `n` times over and averaged (smooth edges).
    Fine(usize),
}

/// Index into a picture of `size` samples for the pointer at `p` over the window `rect` (the picture is stretched
/// to the window, so a draft has fewer samples than pixels).
fn sample_at(rect: Rect, size: [usize; 2], p: Pos2) -> Option<usize> {
    let (u, v) = ((p.x - rect.min.x) / rect.width().max(1.0), (p.y - rect.min.y) / rect.height().max(1.0));
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return None;
    }
    let (x, y) = ((u * size[0] as f32) as usize, (v * size[1] as f32) as usize);
    (x < size[0] && y < size[1]).then_some(y * size[0] + x)
}

/// One picture of the scene at `ss` x supersampling: the pixels, the part under each, and the size.
/// `hover` and `selected` get an outline (pass `None` for a clean picture to save).
fn snapshot(scene: &Scene, rect: Rect, cam: &Cam, show_parts: bool, q: Quality, hover: Option<usize>, selected: Option<usize>) -> (Vec<[u8; 4]>, Vec<u32>, usize, usize) {
    // a draft is drawn at half the size (a quarter of the pixels, no averaging); a fine picture at `n` times, then averaged
    let (s, ss) = match q {
        Quality::Draft => (0.5f32, 1),
        Quality::Fine(n) => (n as f32, n),
    };
    let local = Cam { scale: cam.scale * s, origin: Pos2::new((cam.origin.x - rect.min.x) * s, (cam.origin.y - rect.min.y) * s), ..*cam };
    let (w, h) = ((rect.width().round().max(1.0) * s).round().max(1.0) as usize, (rect.height().round().max(1.0) * s).round().max(1.0) as usize);
    let buf = scene.render(&local, w, h, show_parts, hover);
    let (mut pixels, ids, w, h) = buf.resolve(ss);
    if let Some(s) = selected {
        outline_part(&mut pixels, &ids, w, h, s as u32, [90, 220, 255]);
    }
    if let Some(hv) = hover {
        outline_part(&mut pixels, &ids, w, h, hv as u32, [255, 230, 90]);
    }
    (pixels, ids, w, h)
}

/// A convex outline of the segment `a`-`b` swept by a disc of radius `r` (a track, a round or oval pad, a via).
fn capsule(a: Point, b: Point, r: f64) -> Vec<Point> {
    const N: usize = 8;
    let th = if (b.x - a.x).hypot(b.y - a.y) < 1e-9 { 0.0 } else { (b.y - a.y).atan2(b.x - a.x) };
    let half = std::f64::consts::FRAC_PI_2;
    let mut out = Vec::with_capacity(2 * N + 2);
    for (c, from) in [(b, th - half), (a, th + half)] {
        for k in 0..=N {
            let t = from + std::f64::consts::PI * k as f64 / N as f64;
            out.push(Point::new(c.x + r * t.cos(), c.y + r * t.sin()));
        }
    }
    out
}

/// The copper of net `net` (tracks, pads, vias, and what the zones join to it) as triangles per face. Inner
/// layers are not visible from outside, so they are left out.
fn net_glow(board: &Board, conn: &drc::Connectivity, net: &str) -> Vec<(bool, [Point; 3])> {
    let mut out: Vec<(bool, [Point; 3])> = vec![];
    let mut add = |bottom: bool, poly: &[Point]| {
        for k in 1..poly.len().saturating_sub(1) {
            out.push((bottom, [poly[0], poly[k], poly[k + 1]]));
        }
    };
    for (i, t) in board.tracks.iter().enumerate() {
        if conn.track_net(i) == Some(net) {
            match t.side {
                Side::Top => add(false, &capsule(t.a, t.b, t.width / 2.0)),
                Side::Bottom => add(true, &capsule(t.a, t.b, t.width / 2.0)),
                Side::Inner(_) => {}
            }
        }
    }
    for (i, p) in board.pads.iter().enumerate() {
        if conn.pad_net(i) != Some(net) {
            continue;
        }
        let poly = match p.shape {
            PadShape::Circle { d } => capsule(p.pos, p.pos, d / 2.0),
            PadShape::Rect { w, h } => vec![Point::new(p.pos.x - w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y + h / 2.0), Point::new(p.pos.x - w / 2.0, p.pos.y + h / 2.0)],
            PadShape::Oval { w, h } if w >= h => {
                let d = (w - h) / 2.0;
                capsule(Point::new(p.pos.x - d, p.pos.y), Point::new(p.pos.x + d, p.pos.y), h / 2.0)
            }
            PadShape::Oval { w, h } => {
                let d = (h - w) / 2.0;
                capsule(Point::new(p.pos.x, p.pos.y - d), Point::new(p.pos.x, p.pos.y + d), w / 2.0)
            }
        };
        let faces: &[bool] = match (p.drill.is_some(), p.side) {
            (true, _) => &[false, true],
            (false, Side::Top) => &[false],
            (false, Side::Bottom) => &[true],
            _ => &[],
        };
        for &f in faces {
            add(f, &poly);
        }
    }
    for (i, v) in board.vias.iter().enumerate() {
        if conn.via_net(i) == Some(net) {
            let poly = capsule(v.pos, v.pos, v.diameter / 2.0);
            add(false, &poly);
            add(true, &poly);
        }
    }
    out
}

impl BoardEditor {
    /// Opens (and rebuilds) the 3D window.
    pub fn open_view3d(&mut self) {
        self.view3d.open = true;
        self.view3d.stale = true;
    }

    /// Recomputes the copper of the highlighted net (after the scene was rebuilt or another net was picked).
    fn apply_net_glow(&mut self) {
        let glow = match &self.view3d.net {
            Some(n) => net_glow(&self.board, &self.cached_connectivity(), n),
            None => vec![],
        };
        if let Some(sc) = &mut self.view3d.scene {
            sc.glow = glow;
        }
        self.view3d.generation += 1;
    }

    pub fn view3d_window(&mut self, ctx: &egui::Context) {
        if !self.view3d.open {
            return;
        }
        if self.view3d.stale {
            self.view3d.stale = false;
            self.view3d.generation += 1;
            match Scene::build(&self.board, ctx, self.view3d.mask, self.view3d.silk) {
                Ok(s) => {
                    self.view3d.scene = Some(s);
                    self.view3d.error = None;
                    self.apply_net_glow();
                }
                Err(e) => {
                    self.view3d.scene = None;
                    self.view3d.error = Some(e);
                }
            }
        }
        // a track, pad or via picked in the editor lights up its net here (once per pick, so the list stays usable)
        let sel_net: Option<String> = {
            let b = &self.board;
            match self.selected {
                Some(Item::Track(i)) if i < b.tracks.len() => self.cached_connectivity().track_net(i).map(str::to_string),
                Some(Item::Pad(i)) if i < b.pads.len() => self.cached_connectivity().pad_net(i).map(str::to_string),
                Some(Item::Via(i)) if i < b.vias.len() => self.cached_connectivity().via_net(i).map(str::to_string),
                _ => None,
            }
        };
        let mut new_net: Option<Option<String>> = None;
        if sel_net.is_some() && sel_net != self.view3d.followed {
            new_net = Some(sel_net.clone());
        }
        self.view3d.followed = sel_net;
        if self.view3d.scene.is_some() {
            let (on, top, bottom) = (self.view3d.check_height, self.view3d.limit_top, self.view3d.limit_bottom);
            if self.view3d.scene.as_mut().is_some_and(|sc| sc.set_limits(on, top, bottom)) {
                self.view3d.generation += 1;
            }
        }
        let mut open = true;
        let mut retex = false;
        let mut stale = false;
        let mut want_help = false;
        let mut save_png = false;
        let mut note: Option<String> = None;
        let mut pick: Option<usize> = None;
        let selected = if let Some(Item::Part(i)) = self.selected { Some(i) } else { None };
        let v = &mut self.view3d;
        egui::Window::new("3Dで見る").open(&mut open).default_size([900.0, 680.0]).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("上から").on_hover_text("真上から見ます (部品の配置・配線の確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                if ui.button("斜め").on_hover_text("斜め上から見ます (部品の高さや立体感)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.9, Vec2::ZERO);
                }
                if ui.button("横から").on_hover_text("真横から見ます (部品の高さや、ケースに入る高さの確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.12, Vec2::ZERO);
                }
                if ui.button("裏から").on_hover_text("基板を裏返して真下から見ます (裏面の部品と配線)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, -std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                ui.separator();
                ui.checkbox(&mut v.show_parts, "部品").on_hover_text("部品の本体を表示します。形と高さはフットプリントからの目安です");
                ui.checkbox(&mut v.show_names, "部品名").on_hover_text("部品番号 (R1 など) を表示します");
                if ui.checkbox(&mut v.mask, "ソルダーレジスト").on_hover_text("基板の緑色の保護膜。オフにすると、銅箔の配線がそのまま見えます").changed() {
                    retex = true;
                }
                if ui.checkbox(&mut v.silk, "シルク").on_hover_text("白い印刷 (部品の枠や文字)").changed() {
                    retex = true;
                }
                ui.checkbox(&mut v.check_height, "ケース高さ").on_hover_text("ケースに入る高さを調べます。基板の面からこの高さを超える部品は赤くなり、高さの上限が橙の枠で見えます。0 にした面は調べません");
                if v.check_height {
                    ui.add(egui::DragValue::new(&mut v.limit_top).range(0.0..=200.0).speed(0.1).prefix("表 ").suffix(" mm")).on_hover_text("表側の、基板の面からケースまでの高さ (部品に使える高さ)");
                    ui.add(egui::DragValue::new(&mut v.limit_bottom).range(0.0..=200.0).speed(0.1).prefix("裏 ").suffix(" mm")).on_hover_text("裏側の、基板の面からケースまでの高さ");
                }
                ui.checkbox(&mut v.spin, "自動回転").on_hover_text("ゆっくり回して、いろいろな向きから見せます");
                if ui.button("更新").on_hover_text("基板エディタで変更した内容を反映します").clicked() {
                    stale = true;
                }
                if ui.add_enabled(selected.is_some(), egui::Button::new("選択部品へ寄る")).on_hover_text("基板エディタで選んでいる部品に近づきます。裏面の部品なら基板を裏返して見ます。ダブルクリックでも、部品の上なら寄れます").clicked() {
                    v.focus = selected;
                }
                if ui.button("PNG保存").on_hover_text("いま見えている向きの絵を画像ファイルに保存します (発注前の確認や、人に見せるときに)").clicked() {
                    save_png = true;
                }
                if let Some(sc) = &v.scene {
                    let shown = v.net.clone().unwrap_or_else(|| "なし".into());
                    egui::ComboBox::from_id_salt("view3d_net").selected_text(format!("ネット強調: {shown}")).show_ui(ui, |ui| {
                        if ui.selectable_label(v.net.is_none(), "なし").clicked() {
                            new_net = Some(None);
                        }
                        for n in &sc.nets {
                            if ui.selectable_label(v.net.as_deref() == Some(n.as_str()), n).clicked() {
                                new_net = Some(Some(n.clone()));
                            }
                        }
                    }).response.on_hover_text("選んだネットの銅 (配線・パッド・ビア) を桃色に光らせます。基板エディタで配線やパッドを選んでも、そのネットが光ります");
                }
                if ui.small_button("操作").on_hover_text("回す・動かす・拡大の操作を開きます").clicked() {
                    want_help = true;
                }
            });
            // (the rotate / pan / zoom operations are in the guide)
            if let Some(e) = &v.error {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return;
            }
            let Some(scene) = &v.scene else { return };
            let (w, h) = (scene.hi.x - scene.lo.x, scene.hi.y - scene.lo.y);
            let mut info = format!("基板 {w:.1} × {h:.1} × {BOARD_THICKNESS} mm");
            if let Some((r, t)) = &scene.tallest[0] {
                info += &format!(" / 表側の最も高い部品 {r} (基板の面から {t:.1} mm)");
            }
            if let Some((r, t)) = &scene.tallest[1] {
                info += &format!(" / 裏側 {r} ({t:.1} mm)");
            }
            ui.small(info);
            if v.check_height {
                let over = scene.over_list();
                if over.is_empty() {
                    ui.colored_label(Color32::from_rgb(70, 190, 100), "✔ どの部品もケースの高さに収まっています");
                } else {
                    let names: Vec<String> = over.iter().take(6).map(|(r, o)| format!("{r} (+{o:.1} mm)")).collect();
                    let more = if over.len() > 6 { format!(" ほか {} 個", over.len() - 6) } else { String::new() };
                    ui.colored_label(Color32::from_rgb(230, 70, 60), format!("⚠ ケースの高さを超える部品 (赤): {}{more}", names.join(", ")));
                }
            }
            let size = ui.available_size().max(Vec2::new(300.0, 240.0));
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            painter.rect_filled(resp.rect, 4.0, Color32::from_rgb(34, 38, 46));
            if resp.dragged_by(egui::PointerButton::Primary) {
                let d = resp.drag_delta();
                v.yaw -= d.x * 0.01;
                v.pitch = (v.pitch + d.y * 0.01).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            }
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                v.pan += resp.drag_delta();
            }
            if resp.double_clicked() {
                if let Some(i) = v.hover.filter(|_| v.show_parts) {
                    v.focus = Some(i);
                } else {
                    (v.yaw, v.pitch, v.zoom, v.pan) = (0.0, 0.9, 1.0, Vec2::ZERO);
                }
            }
            if let Some(i) = v.focus.take()
                && let Some((pitch, zoom, pan)) = focus_on(scene, i, resp.rect, v.yaw, v.pitch)
            {
                (v.pitch, v.zoom, v.pan) = (pitch, zoom, pan);
            }
            let mut moving = resp.dragged();
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    moving = true;
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.2, MAX_ZOOM);
                }
            }
            if v.spin {
                v.yaw += ui.input(|i| i.stable_dt) * 0.5;
                ui.ctx().request_repaint();
            }
            let cam = camera(scene, resp.rect, v.yaw, v.pitch, v.zoom, v.pan);
            // the picture: drawn at twice the size and averaged (release builds), and kept while nothing in it changes
            let quality = if moving { Quality::Draft } else { Quality::Fine(if cfg!(debug_assertions) { 1 } else { 2 }) };
            let size_px = [resp.rect.width().round().max(1.0) as usize, resp.rect.height().round().max(1.0) as usize];
            let key = PictureKey {
                camera: [v.yaw.to_bits(), v.pitch.to_bits(), v.zoom.to_bits(), v.pan.x.to_bits(), v.pan.y.to_bits()],
                size: size_px,
                parts: v.show_parts,
                hover: v.hover,
                selected,
                generation: v.generation,
                draft: moving,
            };
            if moving {
                // one more frame once it stops, to draw the full picture
                ui.ctx().request_repaint();
            }
            if v.cache.as_ref().is_none_or(|c| c.key != key) {
                let (pixels, ids, w, h) = snapshot(scene, resp.rect, &cam, v.show_parts, quality, v.hover, selected);
                let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
                let tex = ui.ctx().load_texture("board3d_view", egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR);
                v.cache = Some(Cached { key, tex, ids, size: [w, h] });
            }
            if save_png {
                // a clean picture (no outlines), drawn at twice the size
                let (pixels, _, w, h) = snapshot(scene, resp.rect, &cam, v.show_parts, Quality::Fine(2), None, None);
                if let Some(path) = rfd::FileDialog::new().add_filter("PNG画像", &["png"]).set_file_name("board3d.png").save_file() {
                    let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
                    note = Some(match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
                        Ok(()) => format!("3D表示を保存しました: {}", path.display()),
                        Err(e) => format!("3D表示を保存できませんでした: {e} (保存先のフォルダに書き込めるか確認してください)"),
                    });
                }
            }
            let mut hover = None;
            if let Some(c) = &v.cache {
                painter.image(c.tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if let Some(p) = resp.hover_pos()
                    && let Some(k) = sample_at(resp.rect, c.size, p)
                {
                    let id = c.ids[k];
                    hover = (id != NO_PART && v.show_parts).then_some(id as usize);
                }
            }
            if hover != v.hover {
                // the part under the pointer changed: draw it brighter in the next picture
                v.hover = hover;
                ui.ctx().request_repaint();
            }
            if v.show_names {
                let near_side = cam.pitch.sin() <= 0.0;
                for (i, (name, ..)) in scene.parts.iter().enumerate() {
                    if let Some(b) = scene.boxes.iter().filter(|b| b.part == i && b.bottom == near_side).max_by(|a, b| a.hi[2].total_cmp(&b.hi[2])) {
                        let z = if near_side { b.lo[2] } else { b.hi[2] };
                        let (pos, _) = cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, z]);
                        painter.text(pos, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), Color32::WHITE);
                    }
                }
            }
            if let Some(i) = hover {
                if let Some((r, val, fp, hh)) = scene.parts.get(i) {
                    egui::Area::new(egui::Id::new("view3d_tip")).order(egui::Order::Tooltip).fixed_pos(resp.hover_pos().unwrap_or_default() + Vec2::new(14.0, 14.0)).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.strong(r);
                            ui.label(format!("値: {}", if val.is_empty() { "—" } else { val }));
                            ui.label(format!("フットプリント: {}", if fp.is_empty() { "—" } else { fp }));
                            ui.label(format!("高さの目安: {hh:.1} mm"));
                            if let Some(o) = scene.over.get(i).filter(|o| **o > 1e-6) {
                                ui.colored_label(Color32::from_rgb(230, 70, 60), format!("ケースの高さを {o:.1} mm 超えています"));
                            }
                        });
                    });
                }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    pick = Some(i);
                }
            }
        });
        if let Some(n) = new_net {
            self.view3d.net = n;
            self.apply_net_glow();
        }
        if let Some(n) = note {
            self.status = n;
        }
        if retex && let Some(s) = &mut self.view3d.scene {
            s.img = Scene::images(&s.pv, self.view3d.mask, self.view3d.silk);
            self.view3d.generation += 1;
        }
        if let Some(i) = pick {
            self.selected = Some(Item::Part(i));
            self.multi.clear();
            if let Some(p) = self.board.parts.get(i) {
                self.status = format!("3D表示から {} を選びました (基板エディタで選択中)", p.refdes);
            }
        }
        if want_help {
            self.help_request = Some(crate::help::HelpTab::View3d);
        }
        if stale {
            self.view3d.stale = true;
        }
        if !open {
            self.view3d.open = false;
            self.view3d.scene = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(yaw: f32, pitch: f32) -> Cam {
        Cam { yaw, pitch, scale: 10.0, center: [0.0, 0.0, 0.0], dist: 300.0, origin: Pos2::new(100.0, 100.0) }
    }

    fn solid_box(shape: BodyShape) -> PartBox {
        PartBox { refdes: "U1".into(), part: 0, lo: [0.0, 0.0, 1.6], hi: [4.0, 4.0, 4.0], color: [30, 30, 30], bottom: false, shape }
    }

    #[test]
    fn straight_down_shows_the_board_as_a_map_and_higher_points_are_nearer() {
        let c = cam(0.0, std::f32::consts::FRAC_PI_2);
        let (p, w0) = c.project([0.0, 0.0, 0.0]);
        let (q, w1) = c.project([5.0, 3.0, 0.0]);
        // +x goes right, +y goes up the screen (smaller y)
        assert!(q.x > p.x && q.y < p.y);
        let (_, wz) = c.project([0.0, 0.0, 10.0]);
        assert!(wz > w0 && (w1 - w0).abs() < 1e-4);
        // the top face (normal +z) faces the viewer from above and not from below
        assert!(c.dir([0.0, 0.0, 1.0])[2] > 0.99);
        assert!(cam(0.0, -std::f32::consts::FRAC_PI_2).dir([0.0, 0.0, 1.0])[2] < -0.99);
    }

    #[test]
    fn from_the_side_the_height_is_the_vertical_axis() {
        let c = cam(0.0, 0.0);
        let (p, _) = c.project([0.0, 0.0, 0.0]);
        let (q, _) = c.project([0.0, 0.0, 4.0]);
        assert!(q.y < p.y && (q.x - p.x).abs() < 1e-4);
        // the viewer stands on the -y side: the -y wall faces them, the +y wall does not
        assert!(c.dir([0.0, -1.0, 0.0])[2] > 0.99 && c.dir([0.0, 1.0, 0.0])[2] < -0.99);
    }

    #[test]
    fn a_box_shows_three_faces_and_a_cylinder_its_visible_half() {
        let c = cam(0.5, 0.9);
        let faces = |b: &PartBox| solid_faces(b).iter().filter(|(_, n)| c.dir(*n)[2] > 1e-4).count();
        assert_eq!(faces(&solid_box(BodyShape::Box)), 3);
        // 32 side facets, about half face the viewer, plus the top cap
        let cyl = faces(&solid_box(BodyShape::Cylinder(2)));
        assert!((16..=18).contains(&cyl), "{cyl}");
        // a cylinder lying along x has a cap at each end and a ring of sides
        assert_eq!(solid_faces(&solid_box(BodyShape::Cylinder(0))).len(), 34);
        // brighter when facing the light than when turned away
        assert!(light([-0.3, 0.4, 0.85]) > light([0.9, -0.3, 0.1]));
    }

    /// The board picture lies where the parts' pads are: looking straight down, the copper of a pad is under the
    /// pad's position in the 3D scene (the picture has a margin around the outline, which the mapping must account for).
    #[test]
    fn the_board_picture_and_the_part_positions_agree() {
        let ctx = egui::Context::default();
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, name) in [(8.0, "R1"), (30.0, "R2")] {
            for dx in [-1.0, 1.0] {
                let mut pad = Pad::new(Point::new(x + dx, 12.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                pad.refdes = name.into();
                b.pads.push(pad);
            }
            b.parts.push(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, 12.0), rot: 0, side: Side::Top });
        }
        let s = Scene::build(&b, &ctx, false, false).unwrap();
        let (w, h) = (800usize, 500usize);
        let cam = Cam { yaw: 0.0, pitch: std::f32::consts::FRAC_PI_2, scale: 16.0, center: [20.0, 12.5, 0.8], dist: 300.0, origin: Pos2::new(w as f32 / 2.0, h as f32 / 2.0) };
        let (img, _, w, _) = s.render(&cam, w, h, false, None).resolve(1);
        let at = |x: f64, y: f64| {
            let (p, _) = cam.project([x, y, BOARD_THICKNESS]);
            img[p.y.round() as usize * w + p.x.round() as usize]
        };
        let gold = |c: [u8; 4]| c[0] > 180 && c[1] > 140 && c[2] < 120;
        for pad in &b.pads {
            assert!(gold(at(pad.pos.x, pad.pos.y)), "no copper under the pad at {:?}: {:?}", pad.pos, at(pad.pos.x, pad.pos.y));
        }
        // between the two pads of a part and away from them it is bare board
        assert!(!gold(at(8.0, 12.0)) && !gold(at(20.0, 20.0)));
        // the copper of a pad ends where the pad ends (0.6 mm from its centre, but not 1 mm)
        assert!(gold(at(7.0 + 0.5, 12.0)) && !gold(at(7.0 - 1.0, 12.0)));
    }

    fn two_resistors() -> Board {
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, name) in [(8.0, "R1"), (30.0, "R2")] {
            for (dx, net) in [(-1.0, "A"), (1.0, "B")] {
                let mut pad = Pad::new(Point::new(x + dx, 12.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                pad.refdes = name.into();
                pad.net = net.into();
                b.pads.push(pad);
            }
            b.parts.push(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, 12.0), rot: 0, side: Side::Top });
        }
        b.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(9.0, 12.0), b: Point::new(15.0, 12.0), width: 0.5 });
        b
    }

    fn top_view(s: &Scene) -> (Rect, Cam) {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        (rect, camera(s, rect, 0.0, std::f32::consts::FRAC_PI_2, 1.0, Vec2::ZERO))
    }

    #[test]
    fn a_picked_net_lights_its_copper_and_nothing_else() {
        let b = two_resistors();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        assert_eq!(s.nets, vec!["A".to_string(), "B".to_string()]);
        // net B: the pads at x=9 and x=31 and the track leaving the first
        s.glow = net_glow(&b, &pcb_core::drc::connectivity(&b), "B");
        assert!(!s.glow.is_empty() && s.glow.iter().all(|(bottom, _)| !bottom));
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, false, Quality::Fine(1), None, None);
        let at = |x: f64, y: f64| {
            let (p, _) = cam.project([x, y, BOARD_THICKNESS]);
            img[p.y.round() as usize * 800 + p.x.round() as usize]
        };
        let lit = |c: [u8; 4]| c[0] > 200 && c[1] < 100 && c[2] > 120;
        assert!(lit(at(9.0, 12.0)) && lit(at(12.0, 12.0)) && lit(at(31.0, 12.0)), "pad, track, pad");
        // the other net's pads (x=7, x=29) and bare board are not lit
        assert!(!lit(at(7.0, 12.0)) && !lit(at(29.0, 12.0)) && !lit(at(20.0, 12.0)));
        s.glow.clear();
        let (img, ..) = snapshot(&s, rect, &cam, false, Quality::Fine(1), None, None);
        let (p, _) = cam.project([9.0, 12.0, BOARD_THICKNESS]);
        assert!(!lit(img[p.y.round() as usize * 800 + p.x.round() as usize]));
    }

    #[test]
    fn focusing_a_part_centres_it_and_fills_more_of_the_view() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 500.0));
        let (pitch, zoom, pan) = focus_on(&s, 1, rect, 0.0, 0.9).unwrap();
        assert!(zoom > 2.0, "a small part is magnified: {zoom}");
        let cam = camera(&s, rect, 0.0, pitch, zoom, pan);
        let (lo, hi) = (s.boxes.iter().filter(|b| b.part == 1).map(|b| b.lo[2]).fold(f64::MAX, f64::min), s.boxes.iter().filter(|b| b.part == 1).map(|b| b.hi[2]).fold(0.0, f64::max));
        let (p, _) = cam.project([30.0, 12.0, (lo + hi) / 2.0]);
        assert!((p.x - 400.0).abs() < 20.0 && (p.y - 250.0).abs() < 20.0, "{p:?}");
        assert!(focus_on(&s, 9, rect, 0.0, 0.9).is_none());
        // a part on the bottom is looked at from below
        let mut b2 = two_resistors();
        b2.parts[1].side = Side::Bottom;
        let s2 = Scene::build(&b2, &egui::Context::default(), false, false).unwrap();
        assert!(focus_on(&s2, 1, rect, 0.0, 0.9).unwrap().0 < 0.0);
    }

    #[test]
    fn a_snapshot_is_a_clean_picture_of_the_window_size() {
        let b = two_resistors();
        let s = Scene::build(&b, &egui::Context::default(), true, true).unwrap();
        let (rect, cam) = top_view(&s);
        let (px, ids, w, h) = snapshot(&s, rect, &cam, true, Quality::Fine(2), Some(0), Some(1));
        assert_eq!((w, h, px.len(), ids.len()), (800, 500, 400_000, 400_000));
        // hover / selected outlines are colours that only outlines use
        assert!(px.iter().any(|c| c[..3] == [255, 230, 90]));
        let (clean, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(2), None, None);
        assert!(!clean.iter().any(|c| c[..3] == [255, 230, 90] || c[..3] == [90, 220, 255]));
    }

    fn tall_and_short() -> Board {
        let mut b = two_resistors();
        for (dx, pin) in [(-1.0, "1"), (1.0, "2")] {
            let mut pad = Pad::new(Point::new(20.0 + dx, 20.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            pad.refdes = "C1".into();
            pad.pin = pin.into();
            b.pads.push(pad);
        }
        b.parts.push(pcb_core::Part { refdes: "C1".into(), value: "100u".into(), footprint: "CP_Radial_D5_P2.0".into(), pos: Point::new(20.0, 20.0), rot: 0, side: Side::Top });
        b
    }

    #[test]
    fn parts_over_the_case_height_turn_red_and_are_listed() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        // not checked: nothing is over
        assert!(s.set_limits(false, 3.0, 0.0) && s.over_list().is_empty());
        assert!(!s.set_limits(false, 3.0, 0.0), "same settings: nothing to redraw");
        // 3 mm of room above the top face: the 4.5 mm can is 1.5 mm over, the chips are not
        assert!(s.set_limits(true, 3.0, 5.0));
        let list = s.over_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "C1");
        assert!((list[0].1 - 1.5).abs() < 0.01, "{:?}", list);
        assert_eq!(s.rings[0], Some(3.0));
        // a limit of 0 on a face means that face is not checked
        s.set_limits(true, 0.0, 5.0);
        assert!(s.over_list().is_empty() && s.rings[0].is_none());
        // seen from above, the can is red; with the check off it is its own colour
        s.set_limits(true, 3.0, 5.0);
        let (rect, cam) = top_view(&s);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS + 4.5]);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[0] > 150 && c[1] < 90, "red can: {c:?}");
        s.set_limits(false, 3.0, 5.0);
        let (img, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let c = img[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!(c[1] > 90, "orange can: {c:?}");
    }

    #[test]
    fn a_draft_is_half_size_and_hover_still_finds_the_part() {
        let b = tall_and_short();
        let mut s = Scene::build(&b, &egui::Context::default(), false, false).unwrap();
        s.set_limits(false, 0.0, 0.0);
        let (rect, cam) = top_view(&s);
        let (px, ids, w, h) = snapshot(&s, rect, &cam, true, Quality::Draft, None, None);
        assert_eq!((w, h, px.len(), ids.len()), (400, 250, 100_000, 100_000));
        // the pointer over the can in the window finds the can in the half-size picture
        let (p, _) = cam.project([20.0, 20.0, BOARD_THICKNESS + 4.5]);
        let k = sample_at(rect, [w, h], p).unwrap();
        assert_eq!(ids[k], 2, "C1 is the third part");
        assert!(sample_at(rect, [w, h], Pos2::new(-5.0, 10.0)).is_none() && sample_at(rect, [w, h], Pos2::new(800.0, 10.0)).is_none());
        // the draft looks like the fine picture at the same place (the can's colour)
        let (fine, ..) = snapshot(&s, rect, &cam, true, Quality::Fine(1), None, None);
        let d = px[(p.y / 2.0) as usize * 400 + (p.x / 2.0) as usize];
        let f = fine[p.y.round() as usize * 800 + p.x.round() as usize];
        assert!((d[0] as i32 - f[0] as i32).abs() < 12 && (d[1] as i32 - f[1] as i32).abs() < 12);
    }

    #[test]
    fn the_window_builds_a_scene_with_part_info_and_the_tallest_part() {
        let ctx = egui::Context::default();
        let mut b = Board::new("round", 40.0, 25.0);
        b.outline = pcb_core::outline::rounded_rect(0.0, 0.0, 40.0, 25.0, 5.0);
        for k in 0..2 {
            let mut pad = Pad::new(Point::new(20.0 + 2.54 * k as f64, 12.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            pad.refdes = "J1".into();
            b.pads.push(pad);
        }
        b.parts.push(pcb_core::Part { refdes: "J1".into(), value: "conn".into(), footprint: "PinHeader_1x2".into(), pos: Point::new(20.0, 12.0), rot: 0, side: Side::Top });
        let s = Scene::build(&b, &ctx, true, true).unwrap();
        assert!(s.boxes.len() >= 3, "plastic and pins");
        assert!(s.tris.len() > 10, "{}", s.tris.len());
        assert_eq!(s.parts[0].0, "J1");
        let (r, t) = s.tallest[0].clone().unwrap();
        assert_eq!(r, "J1");
        assert!(t > 8.0, "the pins make the header the tall part: {t}");
        assert!(s.tallest[1].is_none());
        // a board without an outline is explained
        b.outline.clear();
        assert!(Scene::build(&b, &ctx, true, true).is_err());
    }
}
