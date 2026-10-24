use super::*;

/// What the window shows, built from the board when it is opened or refreshed.
pub(super) struct Scene {
    pub(super) pv: Preview,
    /// The board picture of the top and of the bottom (what the faces are painted with).
    pub(super) img: [Vec<[u8; 4]>; 2],
    /// Outline in counter-clockwise order, and its triangles.
    pub(super) outline: Vec<Point>,
    pub(super) tris: Vec<[Point; 3]>,
    pub(super) lo: Point,
    pub(super) hi: Point,
    pub(super) boxes: Vec<PartBox>,
    /// Parts that come with a mesh of their own (a maker's STEP), already placed on the board.
    pub(super) meshes: Vec<PartMesh>,
    /// `(refdes, value, footprint, height above the board)` for every part, in `Board::parts` order.
    pub(super) parts: Vec<(String, String, String, f64)>,
    /// The tallest part on each side, for case checks.
    pub(super) tallest: [Option<(String, f64)>; 2],
    /// Every net name on the board, for the highlight list.
    pub(super) nets: Vec<String>,
    /// For every part: how high it stands above the face it is on (mm), and whether that face is the bottom.
    pub(super) heights: Vec<f64>,
    pub(super) bottoms: Vec<bool>,
    /// How far (mm) each part is over the case height (0 = within it, or not checked), the limits drawn as rings
    /// (top, bottom), and the settings these were computed for.
    pub(super) over: Vec<f64>,
    pub(super) rings: [Option<f64>; 2],
    pub(super) applied: Option<(bool, u64, u64)>,
    /// Copper of the highlighted net as triangles (`true` = bottom face), drawn just above the board picture.
    pub(super) glow: Vec<(bool, [Point; 3])>,
}

/// Brightness for a face whose normal (view space) is `n`: lit from above-left of the viewer.
pub(super) fn light(n: [f32; 3]) -> f32 {
    let l = [-0.35f32, 0.45, 0.82];
    0.45 + 0.6 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

pub(super) type Face3 = (Vec<[f64; 3]>, [f32; 3]);

/// The faces of a solid (corners, outward normal): a box has six, a cylinder a ring of sides and two caps.
pub(super) fn solid_faces(b: &PartBox) -> Vec<Face3> {
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

pub(super) fn shade3(c: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.3);
    [(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8]
}

impl Scene {
    pub(super) fn build(board: &Board, ctx: &egui::Context, mask: bool, silk: bool) -> Result<Scene, String> {
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
        let meshes = part_meshes(board);
        // how high a mesh stands above the face its part is on
        let mesh_height = |m: &PartMesh| {
            let zs = m.mesh.vertices.iter().map(|v| f64::from(v[2]));
            if m.bottom { -zs.fold(f64::MAX, f64::min) } else { zs.fold(f64::MIN, f64::max) - BOARD_THICKNESS }
        };
        let mut parts: Vec<(String, String, String, f64)> = board.parts.iter().map(|p| (p.refdes.clone(), p.value.clone(), p.footprint.clone(), part_height_on(board, p))).collect();
        for m in &meshes {
            parts[m.part].3 = mesh_height(m).max(0.0);
        }
        let boxes = part_boxes(board);
        let mut tallest: [Option<(String, f64)>; 2] = [None, None];
        for b in &boxes {
            let side = b.bottom as usize;
            let height = if b.bottom { -b.lo[2] } else { b.hi[2] - BOARD_THICKNESS };
            if tallest[side].as_ref().is_none_or(|t| height > t.1) {
                tallest[side] = Some((b.refdes.clone(), height));
            }
        }
        for m in &meshes {
            let side = m.bottom as usize;
            let height = mesh_height(m).max(0.0);
            if tallest[side].as_ref().is_none_or(|t| height > t.1) {
                tallest[side] = Some((m.refdes.clone(), height));
            }
        }
        let mut heights = vec![0.0f64; board.parts.len()];
        for m in &meshes {
            heights[m.part] = mesh_height(m).max(0.0);
        }
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
        Ok(Scene { pv, img, outline, tris, lo, hi, boxes, meshes, parts, tallest, heights, bottoms, over, rings: [None; 2], applied: None, nets, glow: vec![] })
    }

    /// Marks the parts that stand higher than the case allows (`top` / `bottom` mm above each face; 0 = that face
    /// is not checked). Returns whether anything changed, so the picture is redrawn only then.
    pub(super) fn set_limits(&mut self, on: bool, top: f64, bottom: f64) -> bool {
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
    pub(super) fn over_list(&self) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = self.over.iter().enumerate().filter(|(_, o)| **o > 1e-6).map(|(i, o)| (self.parts[i].0.clone(), *o)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }

    pub(super) fn images(pv: &Preview, mask: bool, silk: bool) -> [Vec<[u8; 4]>; 2] {
        [pv.render(false, mask, silk), pv.render(true, mask, silk)]
    }

    /// The whole scene into a depth-buffered image of `w` x `h` samples. `cam` must already be in sample units
    /// (its origin relative to the image). `hover` is drawn brighter.
    pub(super) fn render(&self, cam: &Cam, w: usize, h: usize, show_parts: bool, hover: Option<usize>) -> Buffers {
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
        for m in &self.meshes {
            let too_tall = self.over.get(m.part).is_some_and(|o| *o > 1e-6);
            let verts: Vec<[f64; 3]> = m.mesh.vertices.iter().map(|v| v.map(f64::from)).collect();
            for (t, c) in m.mesh.triangles.iter().zip(&m.mesh.tri_color) {
                let (a, b, d) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
                let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
                let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len < 1e-12 {
                    continue;
                }
                let nv = cam.dir([(n[0] / len) as f32, (n[1] / len) as f32, (n[2] / len) as f32]);
                if nv[2] <= 1e-4 {
                    continue;
                }
                let base = m.mesh.palette.get(*c as usize).copied().unwrap_or([140, 140, 140]);
                let mut color = shade3(if too_tall { [225, 50, 40] } else { base }, light(nv));
                if hover == Some(m.part) {
                    color = [color[0].saturating_add(50), color[1].saturating_add(50), color[2].saturating_add(30)];
                }
                buf.triangle([vert(a, [0.0, 0.0]), vert(b, [0.0, 0.0]), vert(d, [0.0, 0.0])], Paint::Flat { color, part: m.part as u32 }, &pics);
            }
        }
        buf
    }
}
