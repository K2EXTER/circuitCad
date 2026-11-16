//! Several boards put together in 3D (E5, stage 2): where each board stands (`Place`: quarter turns about z, turned over, moved), a
//! board stacked on another by their connectors (`mate`: the second board's connector pins over the first's), what collides between
//! boards, and each board as one mesh for the 3D view of the others.
//!
//! A place is a turn that keeps the axes (a signed permutation with determinant +1: turned about z by quarter turns, and turned over
//! about y) and a move, so boxes stay boxes and the STEP placement is an ordinary axis placement.

use crate::mesh::Mesh3d;
use crate::model::*;
use crate::model3d::BOARD_THICKNESS;
use serde::{Deserialize, Serialize};

/// Where a board stands in the frame of the first board: `p' = m·p + offset` (mm, the 3D view's frame: z = 0 at the bottom face).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub m: [[i8; 3]; 3],
    pub offset: [f64; 3],
}

impl Default for Place {
    fn default() -> Self {
        Place { m: [[1, 0, 0], [0, 1, 0], [0, 0, 1]], offset: [0.0; 3] }
    }
}

impl Place {
    /// Turned `quarter_turns` counter-clockwise about z, turned over first (`over`: about y, the top face down) when asked, then moved.
    pub fn new(quarter_turns: u8, over: bool, offset: [f64; 3]) -> Place {
        let flip = if over { [[-1, 0, 0], [0, 1, 0], [0, 0, -1]] } else { [[1, 0, 0], [0, 1, 0], [0, 0, 1]] };
        let rot = match quarter_turns % 4 {
            0 => [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
            1 => [[0, -1, 0], [1, 0, 0], [0, 0, 1]],
            2 => [[-1, 0, 0], [0, -1, 0], [0, 0, 1]],
            _ => [[0, 1, 0], [-1, 0, 0], [0, 0, 1]],
        };
        Place { m: mat_mul(rot, flip), offset }
    }

    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let mut q = self.offset;
        for (i, row) in self.m.iter().enumerate() {
            q[i] += f64::from(row[0]) * p[0] + f64::from(row[1]) * p[1] + f64::from(row[2]) * p[2];
        }
        q
    }

    /// The box `lo`..`hi` where it stands (still a box: the turn keeps the axes).
    pub fn apply_box(&self, lo: [f64; 3], hi: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let (a, b) = (self.apply(lo), self.apply(hi));
        ([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])])
    }

    /// `self` after `inner`: a point placed by `inner`, then by `self`.
    pub fn then(&self, inner: &Place) -> Place {
        let m = mat_mul(self.m, inner.m);
        let offset = self.apply(inner.offset);
        Place { m, offset }
    }

    /// The place that undoes this one.
    pub fn inverse(&self) -> Place {
        let mut t = [[0i8; 3]; 3];
        for (i, row) in self.m.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                t[j][i] = *v;
            }
        }
        let back = Place { m: t, offset: [0.0; 3] };
        let o = back.apply(self.offset);
        Place { m: t, offset: [-o[0], -o[1], -o[2]] }
    }

    /// Is the board turned over (its top face looking down)?
    pub fn is_over(&self) -> bool {
        self.m[2][2] < 0
    }
}

fn mat_mul(a: [[i8; 3]; 3], b: [[i8; 3]; 3]) -> [[i8; 3]; 3] {
    let mut c = [[0i8; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    c
}

/// The pads of a part by pin name (the middle of the pads, when a pin has several).
fn pins_of(board: &Board, refdes: &str) -> Vec<(String, Point)> {
    let mut out: Vec<(String, Point, usize)> = vec![];
    for p in board.pads.iter().filter(|p| p.refdes == refdes) {
        match out.iter_mut().find(|q| q.0 == p.pin) {
            Some(q) => {
                q.1 = Point::new(q.1.x + p.pos.x, q.1.y + p.pos.y);
                q.2 += 1;
            }
            None => out.push((p.pin.clone(), p.pos, 1)),
        }
    }
    out.into_iter().map(|(n, s, k)| (n, Point::new(s.x / k as f64, s.y / k as f64))).collect()
}

/// How the second board sits on the first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mated {
    /// Where board B stands in board A's frame.
    pub place: Place,
    /// The worst distance between a pin of A and its pin of B, mm (more than a few tenths: the connectors do not line up).
    pub worst: f64,
    /// The pins compared.
    pub pins: usize,
}

/// Board B stacked on board A by their connectors `ra` (on A) and `rb` (on B): B parallel to A, `gap` mm above A's top face (B's bottom face
/// there), turned so that B's pins come over A's (`pairs`: pin of A to pin of B; empty = the same names). `over`: B upside down, its top
/// face towards A (two boards whose connectors face each other). Every quarter turn is tried; the one that brings the pins closest wins.
pub fn mate(a: &Board, ra: &str, b: &Board, rb: &str, pairs: &[(String, String)], gap: f64, over: bool) -> Result<Mated, String> {
    let (pa, pb) = (pins_of(a, ra), pins_of(b, rb));
    if pa.is_empty() || pb.is_empty() {
        return Err(format!("{} の基板に {} のパッドが無いか、もう一方に {} のパッドがありません。回路図から基板に取り込んでから重ねてください", a.name, ra, rb));
    }
    let matched: Vec<(Point, Point)> = if pairs.is_empty() {
        pa.iter().filter_map(|(n, p)| pb.iter().find(|q| &q.0 == n).map(|q| (*p, q.1))).collect()
    } else {
        pairs.iter().filter_map(|(x, y)| Some((pa.iter().find(|q| &q.0 == x)?.1, pb.iter().find(|q| &q.0 == y)?.1))).collect()
    };
    if matched.is_empty() {
        return Err(format!("{ra} と {rb} に同じ名前のピンがありません。ピンの組を書いてください"));
    }
    // B's bottom face at A's top face + gap; turned over, B's top face there
    let z = BOARD_THICKNESS + gap + if over { BOARD_THICKNESS } else { 0.0 };
    let mut best: Option<Mated> = None;
    for q in 0..4u8 {
        let turn = Place::new(q, over, [0.0; 3]);
        // move so that the middles of the matched pins meet
        let n = matched.len() as f64;
        let ma = matched.iter().fold([0.0, 0.0], |s, (p, _)| [s[0] + p.x / n, s[1] + p.y / n]);
        let mb = matched.iter().map(|(_, p)| turn.apply([p.x, p.y, 0.0])).fold([0.0, 0.0], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n]);
        let place = Place { m: turn.m, offset: [ma[0] - mb[0], ma[1] - mb[1], z] };
        let worst = matched.iter().map(|(p, r)| {
            let w = place.apply([r.x, r.y, 0.0]);
            ((w[0] - p.x).powi(2) + (w[1] - p.y).powi(2)).sqrt()
        }).fold(0.0, f64::max);
        if best.is_none_or(|b| worst < b.worst - 1e-9) {
            best = Some(Mated { place, worst, pins: matched.len() });
        }
    }
    best.ok_or_else(|| "重ね方が見つかりません".into())
}

/// The height a stacking connector stands above its board (its 3D height): the usual gap to stack the next board on it.
pub fn connector_height(board: &Board, refdes: &str) -> Option<f64> {
    board.parts.iter().find(|p| p.refdes == refdes).map(|p| crate::model3d::part_height_on(board, p))
}

/// A solid box with what it is (a designator, or "" for the board itself).
type Named = (String, [f64; 3], [f64; 3]);

/// Every solid box of a board (each part's 3D boxes, a maker's mesh by its box, and the board itself), with what it is.
fn boxes(board: &Board) -> Vec<Named> {
    let mut v: Vec<Named> = crate::model3d::part_boxes(board).into_iter().map(|b| (b.refdes, b.lo, b.hi)).collect();
    v.extend(crate::model3d::part_meshes(board).into_iter().filter_map(|m| m.mesh.bounds().map(|(lo, hi)| (m.refdes, lo, hi))));
    if let Some((lo, hi)) = board.bounds() {
        v.push((String::new(), [lo.x, lo.y, 0.0], [hi.x, hi.y, BOARD_THICKNESS]));
    }
    v
}

/// What runs into what between the boards where they stand: (board, part or "" for the board itself, other board, its part), each pair
/// once. The 3D boxes are compared (a round part can be reported near something it just clears); touching closer than 0.05 mm is not a
/// collision (pins resting in a socket).
pub fn clashes(boards: &[(&str, &Board, Place)]) -> Vec<(String, String, String, String)> {
    const TOUCH: f64 = 0.05;
    let placed: Vec<Vec<Named>> = boards.iter().map(|(_, b, pl)| boxes(b).into_iter().map(|(n, lo, hi)| { let (lo, hi) = pl.apply_box(lo, hi); (n, lo, hi) }).collect()).collect();
    let mut out = vec![];
    for i in 0..boards.len() {
        for j in i + 1..boards.len() {
            for (na, la, ha) in &placed[i] {
                for (nb, lb, hb) in &placed[j] {
                    if (0..3).all(|k| la[k].max(lb[k]) + TOUCH < ha[k].min(hb[k])) {
                        let item = (boards[i].0.to_string(), na.clone(), boards[j].0.to_string(), nb.clone());
                        if !out.contains(&item) {
                            out.push(item);
                        }
                    }
                }
            }
        }
    }
    out
}

/// The whole board as one mesh for the 3D view of another board: written as a STEP of its board and solids and read back, so the holes,
/// round parts and colours are as the board's own 3D view and STEP have them.
pub fn board_mesh(board: &Board) -> Result<Mesh3d, String> {
    crate::step::read_step(crate::step_out::write(board).as_bytes())
}

/// The mesh where it stands.
pub fn placed_mesh(mesh: &Mesh3d, place: &Place) -> Mesh3d {
    let mut m = mesh.clone();
    for v in &mut m.vertices {
        *v = place.apply(v.map(f64::from)).map(|x| x as f32);
    }
    // turned over, the triangles would face inwards: turn them back
    let det: i32 = {
        let a = place.m;
        let d = |i: usize, j: usize| i32::from(a[i][j]);
        d(0, 0) * (d(1, 1) * d(2, 2) - d(1, 2) * d(2, 1)) - d(0, 1) * (d(1, 0) * d(2, 2) - d(1, 2) * d(2, 0)) + d(0, 2) * (d(1, 0) * d(2, 1) - d(1, 1) * d(2, 0))
    };
    if det < 0 {
        for t in &mut m.triangles {
            t.swap(1, 2);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_board(name: &str, at: (f64, f64), vertical: bool) -> Board {
        let mut b = Board::new(name, 40.0, 30.0);
        for i in 0..4 {
            let (x, y) = if vertical { (at.0, at.1 + 2.54 * i as f64) } else { (at.0 + 2.54 * i as f64, at.1) };
            let mut p = Pad::new(Point::new(x, y), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = "J1".into();
            p.pin = (i + 1).to_string();
            b.pads.push(p);
        }
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "PinHeader_1x4".into(), pos: Point::new(at.0, at.1), rot: 0, side: Side::Top });
        b
    }

    /// A turn and its inverse undo each other; turning over twice is no turn.
    #[test]
    fn places_compose_and_undo() {
        let p = Place::new(1, true, [3.0, 4.0, 5.0]);
        let q = p.then(&p.inverse());
        let x = q.apply([1.0, 2.0, 3.0]);
        assert!((x[0] - 1.0).abs() < 1e-12 && (x[1] - 2.0).abs() < 1e-12 && (x[2] - 3.0).abs() < 1e-12, "{x:?}");
        assert!(p.is_over() && !Place::new(0, true, [0.0; 3]).then(&Place::new(0, true, [0.0; 3])).is_over());
        let (lo, hi) = Place::new(1, false, [0.0; 3]).apply_box([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
        assert_eq!((lo, hi), ([-1.0, 0.0, 0.0], [0.0, 2.0, 1.0]));
    }

    /// A board whose header runs along y is stacked on one whose header runs along x: B is turned a quarter so its pins land on A's pins,
    /// and stands the gap above A's top face.
    #[test]
    fn a_board_is_stacked_with_its_pins_on_the_others() {
        let a = header_board("main", (10.0, 10.0), false);
        let b = header_board("sensor", (5.0, 20.0), true);
        let m = mate(&a, "J1", &b, "J1", &[], 8.5, false).unwrap();
        assert!(m.worst < 1e-9 && m.pins == 4, "{m:?}");
        for i in 0..4 {
            let pa = a.pads.iter().find(|p| p.pin == (i + 1).to_string()).unwrap().pos;
            let pb = b.pads.iter().find(|p| p.pin == (i + 1).to_string()).unwrap().pos;
            let w = m.place.apply([pb.x, pb.y, 0.0]);
            assert!((w[0] - pa.x).abs() < 1e-9 && (w[1] - pa.y).abs() < 1e-9 && (w[2] - (BOARD_THICKNESS + 8.5)).abs() < 1e-9, "pin {}", i + 1);
        }
        // a header whose pins are numbered the other way does not line up without pin pairs: the worst pin is far off
        let mut r = header_board("rev", (5.0, 20.0), true);
        for p in r.pads.iter_mut() {
            p.pin = (5 - p.pin.parse::<i32>().unwrap()).to_string();
        }
        let m = mate(&a, "J1", &r, "J1", &[], 8.5, false).unwrap();
        assert!(m.worst < 1e-9, "turned half round it lines up again: {m:?}");
        // turned over: B's top face down, the gap from its top face
        let m = mate(&a, "J1", &b, "J1", &[], 5.0, true).unwrap();
        assert!(m.place.is_over() && m.worst < 1e-9);
        let top = m.place.apply([0.0, 0.0, BOARD_THICKNESS]);
        assert!((top[2] - (BOARD_THICKNESS + 5.0)).abs() < 1e-9, "B's top face is 5 mm above A's: {top:?}");
    }

    /// Boards stacked too close collide (the header of A reaches into B); far enough they do not.
    #[test]
    fn boards_too_close_collide() {
        let a = header_board("main", (10.0, 10.0), false);
        let b = header_board("sensor", (10.0, 10.0), false);
        let close = mate(&a, "J1", &b, "J1", &[], 2.0, false).unwrap();
        let hits = clashes(&[("main", &a, Place::default()), ("sensor", &b, close.place)]);
        assert!(hits.iter().any(|h| h.0 == "main" && h.1 == "J1" && h.2 == "sensor"), "{hits:?}");
        let h = connector_height(&a, "J1").unwrap();
        let far = mate(&a, "J1", &b, "J1", &[], h + 6.5, false).unwrap();
        assert!(clashes(&[("main", &a, Place::default()), ("sensor", &b, far.place)]).is_empty());
    }

    /// The board read back as a mesh has its size; placed and turned over, its triangles still face out (positive volume).
    #[test]
    fn a_board_becomes_a_mesh_that_can_be_placed() {
        let a = header_board("main", (10.0, 10.0), false);
        let m = board_mesh(&a).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi[0] - lo[0] - 40.0).abs() < 1e-3 && (hi[1] - lo[1] - 30.0).abs() < 1e-3);
        let v = m.volume();
        let over = placed_mesh(&m, &Place::new(1, true, [5.0, 5.0, 20.0]));
        assert!((over.volume() - v).abs() < 1e-3 * v, "{} vs {v}", over.volume());
    }
}
