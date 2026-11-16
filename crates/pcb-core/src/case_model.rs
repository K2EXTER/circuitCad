//! A case read from its STEP file (E4): kept with the board, placed round it, drawn in the 3D view as the lines of its sharp edges, and
//! checked against the parts: a part's solid (each box the 3D view draws, or the box round a maker's mesh) that a wall, a boss or a rib
//! of the case runs into. The check is on boxes, so a round part can be reported near a wall it does not quite touch: "may collide".
//!
//! Coordinates are the 3D view's: board x and y, z = 0 at the board's bottom face. The case's mesh is kept as read (its own frame) and
//! placed by `rot` quarter turns about z and then `offset`.

use crate::mesh::Mesh3d;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseModel {
    /// The file it was read from (its name only), to tell the person.
    pub file: String,
    pub mesh: Mesh3d,
    /// Quarter turns about z (counter-clockwise from above), then the move, in mm.
    #[serde(default)]
    pub rot: u8,
    #[serde(default)]
    pub offset: [f64; 3],
}

/// How far below the board's bottom face the case's floor goes when it is first read (the room for posts and leads), mm.
pub const DEFAULT_DROP: f64 = 5.0;
/// Contact closer than this (mm) is not a collision: a post that the board rests on, a wall the board's edge touches.
const TOUCH: f64 = 0.05;
/// The angle between two faces above which their shared edge is drawn (degrees).
const CREASE_DEG: f64 = 30.0;
/// At most this many lines are drawn (a fine mesh of a moulded case has many).
const MAX_LINES: usize = 20_000;

impl CaseModel {
    /// The case from a STEP file, placed round the board: the middle of its box over the middle of the board, its bottom `DEFAULT_DROP`
    /// below the board.
    pub fn from_step(bytes: &[u8], file: &str, board: &Board) -> Result<CaseModel, String> {
        let mesh = crate::step::read_step(bytes)?;
        let (lo, hi) = mesh.bounds().ok_or("この STEP に立体がありません")?;
        let mid = board.bounds().map_or(Point::new(0.0, 0.0), |(a, b)| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0));
        let offset = [mid.x - (lo[0] + hi[0]) / 2.0, mid.y - (lo[1] + hi[1]) / 2.0, -DEFAULT_DROP - lo[2]];
        Ok(CaseModel { file: file.to_string(), mesh, rot: 0, offset })
    }

    /// The mesh where it stands.
    pub fn placed(&self) -> Mesh3d {
        self.mesh.placed(self.rot, self.offset)
    }

    /// The outer size of the case as placed (mm).
    pub fn size(&self) -> [f64; 3] {
        self.placed().bounds().map_or([0.0; 3], |(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])
    }

    /// The lines of its sharp edges (where two faces meet at more than `CREASE_DEG`, and the open edges), placed: how the 3D view draws
    /// the case without hiding the board inside.
    pub fn edge_lines(&self) -> Vec<([f64; 3], [f64; 3])> {
        let m = self.placed();
        // the same point in several faces is one point (a STEP reader writes each face's corners again)
        let mut ids: HashMap<[i64; 3], usize> = HashMap::new();
        let key = |v: [f32; 3]| v.map(|x| (f64::from(x) * 1e4).round() as i64);
        let id: Vec<usize> = m.vertices.iter().map(|v| { let n = ids.len(); *ids.entry(key(*v)).or_insert(n) }).collect();
        let normals = m.face_normals();
        let mut edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (t, tri) in m.triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (id[tri[k] as usize], id[tri[(k + 1) % 3] as usize]);
                if a != b {
                    edges.entry((a.min(b), a.max(b))).or_default().push(t);
                }
            }
        }
        let cos = CREASE_DEG.to_radians().cos();
        let mut pos: Vec<[f64; 3]> = vec![[0.0; 3]; ids.len()];
        for (i, v) in m.vertices.iter().enumerate() {
            pos[id[i]] = v.map(f64::from);
        }
        let mut out = vec![];
        for ((a, b), faces) in edges {
            let sharp = match faces.as_slice() {
                [f, g] => match (normals[*f], normals[*g]) {
                    (Some(n), Some(o)) => f64::from(n[0] * o[0] + n[1] * o[1] + n[2] * o[2]) < cos,
                    _ => false,
                },
                _ => true,
            };
            if sharp {
                out.push((pos[a], pos[b]));
                if out.len() >= MAX_LINES {
                    break;
                }
            }
        }
        out
    }
}

/// What the case runs into: the parts (index into `Board::parts`) and whether the board itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hits {
    pub parts: Vec<usize>,
    pub board: bool,
}

/// The parts and the board that a face of the case cuts into. Each part is tested by the boxes the 3D view draws for it (a maker's mesh
/// by the box round it); the board by the box round its outline, leaving out the case's faces that pass through its holes (posts and
/// screws). Touching (closer than `TOUCH`) does not count.
pub fn collisions(board: &Board, case: &CaseModel) -> Hits {
    let m = case.placed();
    let tris: Vec<[[f64; 3]; 3]> = m.triangles.iter().map(|t| t.map(|i| m.vertices[i as usize].map(f64::from))).collect();
    let boxes_of = |lo: [f64; 3], hi: [f64; 3]| ([lo[0] + TOUCH, lo[1] + TOUCH, lo[2] + TOUCH], [hi[0] - TOUCH, hi[1] - TOUCH, hi[2] - TOUCH]);
    let cuts = |lo: [f64; 3], hi: [f64; 3], skip: &dyn Fn(&[[f64; 3]; 3]) -> bool| {
        let (lo, hi) = boxes_of(lo, hi);
        lo.iter().zip(&hi).all(|(a, b)| a < b) && tris.iter().any(|t| !skip(t) && tri_box(t, lo, hi))
    };
    let mut hits = Hits::default();
    let mut boxes: Vec<(usize, [f64; 3], [f64; 3])> = crate::model3d::part_boxes(board).iter().map(|b| (b.part, b.lo, b.hi)).collect();
    boxes.extend(crate::model3d::part_meshes(board).iter().filter_map(|pm| pm.mesh.bounds().map(|(lo, hi)| (pm.part, lo, hi))));
    for (part, lo, hi) in boxes {
        if !hits.parts.contains(&part) && cuts(lo, hi, &|_| false) {
            hits.parts.push(part);
        }
    }
    hits.parts.sort_unstable();
    if let Some((lo, hi)) = board.bounds() {
        let holes: Vec<(Point, f64)> = board.holes.iter().map(|h| (h.pos, h.drill.max(h.slot) / 2.0 + 0.3)).collect();
        let through_hole = |t: &[[f64; 3]; 3]| {
            let c = Point::new((t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0);
            holes.iter().any(|(p, r)| p.dist(c) < *r)
        };
        hits.board = cuts([lo.x, lo.y, 0.0], [hi.x, hi.y, crate::model3d::BOARD_THICKNESS], &through_hole);
    }
    hits
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Does the triangle cut into the box `lo`..`hi`? The separating axis test (Akenine-Möller): the box's three axes, the triangle's
/// normal, and the nine crossings of the box's axes with the triangle's sides.
fn tri_box(t: &[[f64; 3]; 3], lo: [f64; 3], hi: [f64; 3]) -> bool {
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let h = [(hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0, (hi[2] - lo[2]) / 2.0];
    let v = [sub(t[0], c), sub(t[1], c), sub(t[2], c)];
    // the box's axes
    for k in 0..3 {
        let (mn, mx) = (v[0][k].min(v[1][k]).min(v[2][k]), v[0][k].max(v[1][k]).max(v[2][k]));
        if mn > h[k] || mx < -h[k] {
            return false;
        }
    }
    let e = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    let separated = |axis: [f64; 3]| {
        if dot(axis, axis) < 1e-18 {
            return false;
        }
        let p = [dot(v[0], axis), dot(v[1], axis), dot(v[2], axis)];
        let r = h[0] * axis[0].abs() + h[1] * axis[1].abs() + h[2] * axis[2].abs();
        p.iter().cloned().fold(f64::MAX, f64::min) > r || p.iter().cloned().fold(f64::MIN, f64::max) < -r
    };
    // the triangle's plane
    if separated(cross(e[0], e[1])) {
        return false;
    }
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for ei in &e {
        for a in &axes {
            if separated(cross(*a, *ei)) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An open box (floor and four walls, 2 mm thick) as the STEP-read mesh would be: two triangles per face of each slab.
    fn tray(w: f32, d: f32, h: f32) -> Mesh3d {
        let mut m = Mesh3d { palette: vec![[200, 200, 200]], ..Default::default() };
        let mut slab = |lo: [f32; 3], hi: [f32; 3]| {
            let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
            for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
                let base = m.vertices.len() as u32;
                m.vertices.extend(f.iter().map(|i| c(*i)));
                m.triangles.push([base, base + 1, base + 2]);
                m.triangles.push([base, base + 2, base + 3]);
                m.tri_color.extend([0, 0]);
            }
        };
        slab([0.0, 0.0, 0.0], [w, d, 2.0]);
        slab([0.0, 0.0, 2.0], [2.0, d, h]);
        slab([w - 2.0, 0.0, 2.0], [w, d, h]);
        slab([2.0, 0.0, 2.0], [w - 2.0, 2.0, h]);
        slab([2.0, d - 2.0, 2.0], [w - 2.0, d, h]);
        m
    }

    fn part(b: &mut Board, refdes: &str, fp: &str, at: (f64, f64)) {
        let mut p = Pad::new(Point::new(at.0, at.1), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        p.refdes = refdes.into();
        p.pin = "1".into();
        b.pads.push(p);
        let mut q = Pad::new(Point::new(at.0 + 2.54, at.1), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        q.refdes = refdes.into();
        q.pin = "2".into();
        b.pads.push(q);
        b.parts.push(Part { refdes: refdes.into(), value: String::new(), footprint: fp.into(), pos: Point::new(at.0, at.1), rot: 0, side: Side::Top });
    }

    /// Read from a STEP (written by our own writer), the case sits round the board with its floor 5 mm below; a part in the middle is
    /// clear, one standing in a wall is reported; the board itself fits; moved over, the wall cuts the board too.
    #[test]
    fn a_case_from_its_step_is_placed_round_the_board_and_checked() {
        let mut b = Board::new("b", 40.0, 30.0);
        part(&mut b, "R1", "PinHeader_1x2", (20.0, 15.0));
        part(&mut b, "R2", "PinHeader_1x2", (-3.0, 15.0));
        // the case as a STEP file: our writer's board solid of a tray-shaped outline would be a plate, so the case is written from a
        // board whose solids are the tray (the reader takes them back)
        let mut shell = Board::new("case", 1.0, 1.0);
        shell.outline.clear();
        let tray_mesh = tray(50.0, 40.0, 20.0);
        shell.models3d.insert("TRAY".into(), tray_mesh.clone());
        shell.parts.push(Part { refdes: "CASE".into(), value: String::new(), footprint: "TRAY".into(), pos: Point::new(0.0, 0.0), rot: 0, side: Side::Top });
        let step = crate::step_out::write(&shell);
        let c = CaseModel::from_step(step.as_bytes(), "case.step", &b).unwrap();
        let (lo, hi) = c.placed().bounds().unwrap();
        assert!((lo[2] + DEFAULT_DROP).abs() < 1e-3, "the floor 5 mm below the board: {lo:?}");
        assert!(((lo[0] + hi[0]) / 2.0 - 20.0).abs() < 1e-3 && ((lo[1] + hi[1]) / 2.0 - 15.0).abs() < 1e-3, "centred on the board");
        let s = c.size();
        assert!((s[0] - 50.0).abs() < 1e-3 && (s[2] - 20.0).abs() < 1e-3, "{s:?}");
        assert!(!c.edge_lines().is_empty(), "the edges are drawn");
        // R2 hangs off the board's left edge, into the left wall (x from -5 to -3)
        let hits = collisions(&b, &c);
        assert_eq!(hits.parts.iter().map(|i| b.parts[*i].refdes.as_str()).collect::<Vec<_>>(), vec!["R2"], "{hits:?}");
        assert!(!hits.board, "the board fits inside");
        // moved 4 mm to the left, the left wall cuts the board too
        let moved = CaseModel { offset: [c.offset[0] + 4.0, c.offset[1], c.offset[2]], ..c.clone() };
        assert!(collisions(&b, &moved).board);
        // turned a quarter, the 50 x 40 case is 40 x 50
        let turned = CaseModel { rot: 1, ..c };
        let s = turned.size();
        assert!((s[0] - 40.0).abs() < 1e-3 && (s[1] - 50.0).abs() < 1e-3, "{s:?}");
    }

    /// The separating axis test: a triangle through a box, beside it, and across its corner.
    #[test]
    fn a_triangle_cuts_a_box_or_does_not() {
        let (lo, hi) = ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert!(tri_box(&[[-1.0, 0.5, 0.5], [2.0, 0.5, 0.5], [0.5, 2.0, 0.5]], lo, hi));
        assert!(!tri_box(&[[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]], lo, hi));
        // its plane passes the box's corner without touching it
        assert!(!tri_box(&[[1.5, 0.0, 1.5], [0.0, 1.5, 1.5], [1.5, 1.5, 0.0]].map(|p| p.map(|x| x + 0.6)), lo, hi));
    }
}
