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
    /// The smooth normal of every corner of every mesh triangle (`Mesh3d::corner_normals`, made once with the scene).
    pub(super) mesh_normals: Vec<Vec<Option<[[f32; 3]; 3]>>>,
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
    /// The case box drawn as a wire frame (inside size, centred on the board; turned a quarter when the board only fits that way), and the settings it was made for.
    pub(super) case: Option<CaseWire>,
    pub(super) case_key: Option<[u64; 5]>,
    /// Copper of the highlighted net as triangles (`true` = bottom face), drawn just above the board picture.
    pub(super) glow: Vec<(bool, [Point; 3])>,
    /// This scene's own number (the GPU's copy of it is replaced when another scene is built).
    pub(super) serial: u64,
    /// The heat colour of every part (`None`: its own colour), its `(temperature °C, power W)` when the run warmed it, and the
    /// temperatures these were made for (`heat.rs`).
    pub(super) heat: Vec<Option<[u8; 3]>>,
    pub(super) temps: Vec<Option<(f64, f64)>>,
    pub(super) heat_applied: Option<PartTemps>,
    /// The case read from its STEP (`Board::case3d`): the lines of its sharp edges, and the parts (by index) and whether the board that it
    /// runs into (`pcb_core::case_model::collisions`).
    pub(super) case_lines: Vec<([f64; 3], [f64; 3])>,
    pub(super) case_hits: Vec<bool>,
    pub(super) case_board_hit: bool,
    /// The other boards of a multi-board project where they stand (`BoardEditor::others3d`), drawn like a maker's mesh but not picked,
    /// with their corner normals.
    pub(super) others: Vec<(pcb_core::mesh::Mesh3d, CornerNormals)>,
}

/// The case read from its STEP for the view: its edge lines, which parts it runs into, and whether the board.
type CaseView = (Vec<([f64; 3], [f64; 3])>, Vec<bool>, bool);

fn case_model_view(board: &Board) -> CaseView {
    match &board.case3d {
        Some(c) => {
            let h = pcb_core::case_model::collisions(board, c);
            (c.edge_lines(), (0..board.parts.len()).map(|i| h.parts.contains(&i)).collect(), h.board)
        }
        None => (vec![], vec![false; board.parts.len()], false),
    }
}

impl Scene {
    /// The other boards of the project to draw with this one (in this board's frame).
    pub(super) fn set_others(&mut self, others: &[pcb_core::mesh::Mesh3d]) {
        self.others = others.iter().map(|m| (m.clone(), m.corner_normals(CREASE_DEG))).collect();
    }

    /// The case read from its STEP again (moved, turned or taken away), without building the rest of the scene.
    pub(super) fn set_case_model(&mut self, board: &Board) {
        (self.case_lines, self.case_hits, self.case_board_hit) = case_model_view(board);
    }
}

/// The smooth normal at each corner of each triangle of a mesh (`None`: flat).
pub(super) type CornerNormals = Vec<Option<[[f32; 3]; 3]>>;

/// The colour of the case read from its STEP (its edges).
pub(super) const CASE_STEP: [u8; 3] = [150, 200, 230];

/// Brightness for a face whose normal (view space) is `n`: lit from above-left of the viewer.
pub(super) fn light(n: [f32; 3]) -> f32 {
    let l = [-0.35f32, 0.45, 0.82];
    0.45 + 0.6 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

/// The wire frame of the case's inside: x and y of two corners, z of the floor and the lid; and the screw posts and the openings
/// in the walls (`pcb_core::case::inside`) as lines (from, to, colour), all in the board's frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CaseWire {
    pub lo: Point,
    pub hi: Point,
    pub z0: f64,
    pub z1: f64,
    pub lines: Vec<([f64; 3], [f64; 3], [u8; 3])>,
    /// The posts, openings and notes the lines were drawn from (for the window's text).
    pub inside: pcb_core::case::Inside,
}

/// The colours of the posts (red when they run into something) and of the openings.
const POST: [u8; 3] = [240, 160, 60];
const POST_BAD: [u8; 3] = [240, 70, 60];
const OPENING: [u8; 3] = [230, 90, 220];

/// The case's frame, posts and openings for the 3D view, in the board's frame (`None`: no floor given).
pub(super) fn case_wire(board: &pcb_core::model::Board, spec: &pcb_core::case::CaseSpec) -> Option<CaseWire> {
    use pcb_core::case::Wall;
    let seat = pcb_core::case::seat(board, spec)?;
    let (w, d) = (spec.inner_w, spec.inner_d);
    let (a, b) = (seat.unplace(Point::new(0.0, 0.0)), seat.unplace(Point::new(w, d)));
    let z0 = -seat.raise;
    let z1 = BOARD_THICKNESS + if spec.top > 0.0 { spec.top } else { 10.0 };
    let ins = pcb_core::case::inside(board, spec);
    let mut lines = vec![];
    for p in &ins.posts {
        let col = if p.problem.is_some() { POST_BAD } else { POST };
        let r = p.outer_d / 2.0;
        let n = 16;
        let at = |k: usize| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            (p.hole.x + r * t.cos(), p.hole.y + r * t.sin())
        };
        for k in 0..n {
            let (q0, q1) = (at(k), at(k + 1));
            for z in [z0, 0.0] {
                lines.push(([q0.0, q0.1, z], [q1.0, q1.1, z], col));
            }
            if k % 4 == 0 {
                lines.push(([q0.0, q0.1, z0], [q0.0, q0.1, 0.0], col));
            }
        }
    }
    // an opening: its four corners on the wall's inside face, from the case's frame (z up from the floor) to the board's
    let on_wall = |wall: Wall, along: f64, up: f64| {
        let c = match wall {
            Wall::Front => Point::new(along, 0.0),
            Wall::Back => Point::new(along, d),
            Wall::Left => Point::new(0.0, along),
            Wall::Right => Point::new(w, along),
        };
        let q = seat.unplace(c);
        [q.x, q.y, up - seat.raise]
    };
    for o in &ins.openings {
        let c = [on_wall(o.wall, o.along.0, o.up.0), on_wall(o.wall, o.along.1, o.up.0), on_wall(o.wall, o.along.1, o.up.1), on_wall(o.wall, o.along.0, o.up.1)];
        for i in 0..4 {
            lines.push((c[i], c[(i + 1) % 4], OPENING));
        }
    }
    Some(CaseWire { lo: Point::new(a.x.min(b.x), a.y.min(b.y)), hi: Point::new(a.x.max(b.x), a.y.max(b.y)), z0, z1, lines, inside: ins })
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

/// Faces that turn less than this (degrees) from their neighbours shade as one curved surface (a cylinder cut into 16 sides turns
/// 22.5° from side to side); sharper edges, a box's 90° or a 45° chamfer, stay sharp.
pub(super) const CREASE_DEG: f64 = 40.0;

/// Whether the edge `p`-`q` of a face of the guessed solid `b` is drawn as a dark line: every edge but those between two sides of a
/// cylinder (along its axis), which shade as one surface.
pub(super) fn hard_edge(b: &PartBox, normal: [f32; 3], p: [f64; 3], q: [f64; 3]) -> bool {
    let BodyShape::Cylinder(axis) = b.shape else { return true };
    let a = axis as usize;
    normal[a].abs() > 0.5 || (0..3).any(|k| k != a && (p[k] - q[k]).abs() > 1e-9)
}

/// The colour `c` made lighter by `d` (the part under the pointer).
pub(super) fn brighten(c: [u8; 3], d: [u8; 3]) -> [u8; 3] {
    [c[0].saturating_add(d[0]), c[1].saturating_add(d[1]), c[2].saturating_add(d[2])]
}

/// The normal the light is worked out with at corner `p` of a face of the guessed solid `b`: round on a cylinder's side (straight out
/// from its axis, so the 32 sides shade as one surface), the face's own elsewhere.
pub(super) fn solid_shade(b: &PartBox, p: [f64; 3], normal: [f32; 3]) -> [f32; 3] {
    let BodyShape::Cylinder(axis) = b.shape else { return normal };
    let a = axis as usize;
    if normal[a].abs() > 0.5 {
        return normal; // a cap
    }
    let (i, j) = ((a + 1) % 3, (a + 2) % 3);
    let (ri, rj) = (((b.hi[i] - b.lo[i]) / 2.0).max(1e-9), ((b.hi[j] - b.lo[j]) / 2.0).max(1e-9));
    let (ci, cj) = ((b.lo[i] + b.hi[i]) / 2.0, (b.lo[j] + b.hi[j]) / 2.0);
    // the gradient of the ellipse's equation: outward and square to its side
    let (gi, gj) = ((p[i] - ci) / (ri * ri), (p[j] - cj) / (rj * rj));
    let len = gi.hypot(gj);
    if len < 1e-12 {
        return normal;
    }
    let mut n = [0.0f32; 3];
    n[i] = (gi / len) as f32;
    n[j] = (gj / len) as f32;
    n
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
        let mesh_normals = meshes.iter().map(|m| m.mesh.corner_normals(CREASE_DEG)).collect();
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
        let (case_lines, case_hits, case_board_hit) = case_model_view(board);
        Ok(Scene { pv, img, outline, tris, lo, hi, boxes, meshes, mesh_normals, parts, tallest, heights, bottoms, over, rings: [None; 2], applied: None, case: None, case_key: None, nets, glow: vec![], serial: super::gpu::next_serial(), heat: vec![None; board.parts.len()], temps: vec![None; board.parts.len()], heat_applied: None, case_lines, case_hits, case_board_hit, others: vec![] })
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

    /// Shows the inside of the case as a wire box with its posts and openings (`None`: no box). Returns whether anything changed.
    pub(super) fn set_case(&mut self, spec: Option<pcb_core::case::CaseSpec>, board: &pcb_core::model::Board) -> bool {
        let key = spec.map(|s| [s.inner_w.to_bits(), s.inner_d.to_bits(), s.top.to_bits(), s.bottom.to_bits(), s.clearance.to_bits()]);
        if self.case_key == key {
            return false;
        }
        self.case_key = key;
        self.case = spec.filter(|s| s.inner_w > 0.0 && s.inner_d > 0.0).and_then(|s| case_wire(board, &s));
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
        if let Some(w) = &self.case {
            let col = [90, 170, 255];
            let corners = [(w.lo.x, w.lo.y), (w.hi.x, w.lo.y), (w.hi.x, w.hi.y), (w.lo.x, w.hi.y)];
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                for z in [w.z0, w.z1] {
                    buf.line(vert([a.0, a.1, z], [0.0, 0.0]), vert([b.0, b.1, z], [0.0, 0.0]), col);
                }
                buf.line(vert([a.0, a.1, w.z0], [0.0, 0.0]), vert([a.0, a.1, w.z1], [0.0, 0.0]), col);
            }
            for (a, b, c) in &w.lines {
                buf.line(vert(*a, [0.0, 0.0]), vert(*b, [0.0, 0.0]), *c);
            }
        }
        for (a, b) in &self.case_lines {
            buf.line(vert(*a, [0.0, 0.0]), vert(*b, [0.0, 0.0]), CASE_STEP);
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
                let base = self.part_color(b.part, b.color);
                let lift = if hover == Some(b.part) { [50, 50, 30] } else { [0, 0, 0] };
                let lit = |p: &[f64; 3]| brighten(shade3(base, light(cam.dir(solid_shade(b, *p, normal)))), lift);
                let vs: Vec<Vert> = corners.iter().map(|p| vert(*p, [0.0, 0.0])).collect();
                let cs: Vec<[u8; 3]> = corners.iter().map(lit).collect();
                for k in 1..vs.len().saturating_sub(1) {
                    buf.triangle([vs[0], vs[k], vs[k + 1]], Paint::Shaded { colors: [cs[0], cs[k], cs[k + 1]], part: b.part as u32 }, &pics);
                }
                // a thin dark edge round every face, so the shape reads even where two faces are lit alike
                let dark = shade3(b.color, 0.4);
                for k in 0..vs.len() {
                    let k1 = (k + 1) % vs.len();
                    if hard_edge(b, normal, corners[k], corners[k1]) {
                        buf.line(vs[k], vs[k1], dark);
                    }
                }
            }
        }
        for (m, smooth) in self.meshes.iter().zip(&self.mesh_normals) {
            let verts: Vec<[f64; 3]> = m.mesh.vertices.iter().map(|v| v.map(f64::from)).collect();
            for ((t, c), sn) in m.mesh.triangles.iter().zip(&m.mesh.tri_color).zip(smooth) {
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
                let base = self.part_color(m.part, m.mesh.palette.get(*c as usize).copied().unwrap_or([140, 140, 140]));
                let lift = if hover == Some(m.part) { [50, 50, 30] } else { [0, 0, 0] };
                // the corner's smooth normal, or the face's where it has turned away from the viewer (at the outline)
                let lit = |n: [f32; 3]| {
                    let v = cam.dir(n);
                    brighten(shade3(base, light(if v[2] > 0.0 { v } else { nv })), lift)
                };
                let cs = sn.map_or([lit([nv[0], nv[1], nv[2]]); 3], |sn| sn.map(lit));
                buf.triangle([vert(a, [0.0, 0.0]), vert(b, [0.0, 0.0]), vert(d, [0.0, 0.0])], Paint::Shaded { colors: cs, part: m.part as u32 }, &pics);
            }
        }
        // the other boards of the project: not a part of this board (no part number, never picked)
        for (m, smooth) in &self.others {
            let verts: Vec<[f64; 3]> = m.vertices.iter().map(|v| v.map(f64::from)).collect();
            for ((t, c), sn) in m.triangles.iter().zip(&m.tri_color).zip(smooth) {
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
                let base = m.palette.get(*c as usize).copied().unwrap_or([140, 140, 140]);
                let lit = |n: [f32; 3]| {
                    let v = cam.dir(n);
                    shade3(base, light(if v[2] > 0.0 { v } else { nv }))
                };
                let cs = sn.map_or([lit([nv[0], nv[1], nv[2]]); 3], |sn| sn.map(lit));
                buf.triangle([vert(a, [0.0, 0.0]), vert(b, [0.0, 0.0]), vert(d, [0.0, 0.0])], Paint::Shaded { colors: cs, part: u32::MAX }, &pics);
            }
        }
        buf
    }
}
