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

/// Board B beside board A, joined by a cable: where B stands, the straight distance between the two connectors and a cable length to buy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beside {
    pub place: Place,
    /// Between the middles of the two connectors, mm.
    pub distance: f64,
    /// The cable's length: the distance with 20 % to spare and 20 mm for the bends into the connectors, rounded up to 10 mm.
    pub cable: f64,
}

/// The middle of a part's pads on its board, at the top of the part (the board's own frame: z = 0 at the bottom face).
pub fn connector_point(board: &Board, refdes: &str) -> Option<[f64; 3]> {
    let pins = pins_of(board, refdes);
    if pins.is_empty() {
        return None;
    }
    let n = pins.len() as f64;
    let (x, y) = pins.iter().fold((0.0, 0.0), |s, (_, p)| (s.0 + p.x / n, s.1 + p.y / n));
    Some([x, y, BOARD_THICKNESS + connector_height(board, refdes).unwrap_or(0.0)])
}

/// Board B laid beside board A, the two joined by a cable from connector `ra` on A to `rb` on B (E5): B in A's plane, `gap` mm past the
/// edge of A nearest A's connector, turned so that B's connector is on B's side facing A, and moved across so the two connectors are
/// in line. Every quarter turn is tried; the one that brings B's connector closest to its facing edge wins.
pub fn beside(a: &Board, ra: &str, b: &Board, rb: &str, gap: f64) -> Result<Beside, String> {
    let (Some(ca), Some(cb)) = (connector_point(a, ra), connector_point(b, rb)) else {
        return Err(format!("{} の基板に {} のパッドが無いか、もう一方に {} のパッドがありません。回路図から基板に取り込んでから置いてください", a.name, ra, rb));
    };
    let (Some((alo, ahi)), Some((blo, bhi))) = (a.bounds(), b.bounds()) else { return Err("基板の外形がありません".into()) };
    // the edge of A nearest its connector: the axis (0 = x, 1 = y) and the way out (+1 / −1)
    let edges = [(ca[0] - alo.x, 0, -1.0), (ahi.x - ca[0], 0, 1.0), (ca[1] - alo.y, 1, -1.0), (ahi.y - ca[1], 1, 1.0)];
    let &(_, axis, s) = edges.iter().min_by(|x, y| x.0.total_cmp(&y.0)).unwrap();
    let edge = if s > 0.0 { [ahi.x, ahi.y][axis] } else { [alo.x, alo.y][axis] };
    let across = 1 - axis;
    let mut best: Option<(f64, Place)> = None;
    for q in 0..4u8 {
        let turn = Place::new(q, false, [0.0; 3]);
        let (lo, hi) = turn.apply_box([blo.x, blo.y, 0.0], [bhi.x, bhi.y, BOARD_THICKNESS]);
        let t = turn.apply(cb);
        // B's edge that faces A, and how far B's connector is from it
        let near = if s > 0.0 { lo[axis] } else { hi[axis] };
        let off = (t[axis] - near).abs();
        let mut offset = [0.0; 3];
        offset[axis] = edge + s * gap - near;
        offset[across] = ca[across] - t[across];
        let place = Place { m: turn.m, offset };
        if best.is_none_or(|(o, _)| off < o - 1e-9) {
            best = Some((off, place));
        }
    }
    let (_, place) = best.ok_or("置き方が見つかりません")?;
    let w = place.apply(cb);
    let distance = ((w[0] - ca[0]).powi(2) + (w[1] - ca[1]).powi(2) + (w[2] - ca[2]).powi(2)).sqrt();
    let cable = ((distance * 1.2 + 20.0) / 10.0).ceil() * 10.0;
    Ok(Beside { place, distance, cable })
}

/// A cable drawn between two connector tops `p` and `q` (one frame): up 5 mm over the higher one, along x, along y, and down, as a
/// bundle 2 mm square, grey. Only to be seen in the 3D view.
pub fn cable_mesh(p: [f64; 3], q: [f64; 3]) -> Mesh3d {
    let top = p[2].max(q[2]) + 5.0;
    let corner = [q[0], p[1], top];
    let legs = [(p, [p[0], p[1], top]), ([p[0], p[1], top], corner), (corner, [q[0], q[1], top]), ([q[0], q[1], top], q)];
    let mut m = Mesh3d { palette: vec![[150, 150, 155]], ..Default::default() };
    let r = 1.0;
    for (a, b) in legs {
        let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r, a[2].min(b[2]) - r];
        let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r, a[2].max(b[2]) + r];
        let base = m.vertices.len() as u32;
        for k in 0..8 {
            m.vertices.push([if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }].map(|v| v as f32));
        }
        // the six faces, counter-clockwise from outside
        for [a, b, c, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            m.triangles.push([base + a, base + b, base + c]);
            m.triangles.push([base + a, base + c, base + d]);
            m.tri_color.extend([0, 0]);
        }
    }
    m
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

    /// Two boards side by side by a cable: B past A's edge nearest A's header, turned so its own header faces A, the headers in line; the
    /// cable's length is the distance with room to spare. The cable's mesh is closed (every edge in two triangles).
    #[test]
    fn a_board_is_laid_beside_the_other_for_a_cable() {
        // A's header near its right edge (x = 36 of 40), B's header near its left edge, running along y
        let a = header_board("main", (36.0, 10.0), true);
        let b = header_board("sensor", (3.0, 10.0), true);
        let s = beside(&a, "J1", &b, "J1", 20.0).unwrap();
        let (lo, hi) = s.place.apply_box([0.0, 0.0, 0.0], [40.0, 30.0, BOARD_THICKNESS]);
        assert!((lo[0] - 60.0).abs() < 1e-9 && hi[0] > lo[0], "B 20 mm right of A's edge at 40: {lo:?} {hi:?}");
        let (pa, pb) = (connector_point(&a, "J1").unwrap(), s.place.apply(connector_point(&b, "J1").unwrap()));
        assert!((pa[1] - pb[1]).abs() < 1e-9, "the headers in line: {pa:?} {pb:?}");
        assert!(pb[0] - 60.0 < 4.0, "B's header on its side facing A: {pb:?}");
        assert!((s.distance - (pb[0] - pa[0])).abs() < 1e-9 && s.cable >= s.distance * 1.2 && s.cable % 10.0 == 0.0, "{s:?}");
        // B's header on its far side: B is turned half round so the header faces A
        let far = header_board("sensor", (37.0, 10.0), true);
        let s = beside(&a, "J1", &far, "J1", 20.0).unwrap();
        assert!(s.distance < 30.0, "turned to face A: {s:?}");
        let m = cable_mesh([0.0, 0.0, 10.0], [30.0, 20.0, 10.0]);
        let mut edges = std::collections::HashMap::new();
        for t in &m.triangles {
            for k in 0..3 {
                let (u, v) = (t[k], t[(k + 1) % 3]);
                *edges.entry((u.min(v), u.max(v))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&n| n == 2) && m.tri_color.len() == m.triangles.len());
        assert!(m.volume() > 0.0, "faces outwards");
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
