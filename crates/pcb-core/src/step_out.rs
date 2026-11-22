//! The board and its parts as a STEP file (ISO 10303-21, AP214), for the case's CAD (Fusion, FreeCAD, SolidWorks ...), as an assembly: the
//! board (`<name>_PCB`) and every part a product of its own (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`), so the CAD can pick a part and move it.
//! A part's shape is in its own frame (the footprint's x and y, z up from the face it sits on) and its placement puts it where it stands:
//! turned with it, and turned over for a part on the bottom (E3b). One solid for the
//! board (its outline raised to `model3d::BOARD_THICKNESS`, with the mounting holes and the through-hole drills cut through) and one per part
//! solid that the 3D view draws (`model3d::part_boxes`: boxes and cylinders, and `model3d::part_meshes`: the maker's STEP read into
//! triangles). Every solid is named (the board's name, or the part's designator) and coloured as in the 3D view.
//!
//! Round things are written round (E3b, 2026-11-16): a part's cylinder as two half `CYLINDRICAL_SURFACE` faces and two flat caps bounded by
//! `CIRCLE` arcs, the board's round holes and slots with walls of `CYLINDRICAL_SURFACE` and `PLANE` faces (`Exact`). Everything else is flat:
//! `ADVANCED_FACE`s on `PLANE`s bounded by `LINE` edges, a closed shell per solid (`MANIFOLD_SOLID_BREP`). A maker's model goes out with
//! the faces of its own STEP file when that file is on this computer (`Board::models3d_src`, `step::copy_solids`: curved faces stay
//! curved), else as the triangles it was read into. The copper and the silkscreen go out as thin solids on the board's faces (`layers`).
//! Vias are not written. `solids` keeps
//! the flat-faced version of every solid (a cylinder a 24-sided prism, a hole a 16-sided one) for the volume and the names.

use crate::geom::{inside_polygon, signed_area};
use crate::model::*;
use crate::model3d::{part_boxes, part_meshes, BodyShape, PartBox, BOARD_THICKNESS};
use std::collections::HashMap;
use std::fmt::Write as _;

/// A closed solid of flat faces: each face an outer loop and holes, by index into `vertices`, counter-clockwise (holes clockwise) seen from
/// outside.
#[derive(Clone, Debug, Default)]
pub struct Solid {
    pub name: String,
    pub color: [u8; 3],
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<(Vec<usize>, Vec<Vec<usize>>)>,
    /// The solid as it really is, when it is round somewhere: the file is written from this, the flat faces above stay for the volume.
    pub exact: Option<Exact>,
    /// A maker's model written from its own STEP file (the faces above are its triangles, written when the file cannot be copied).
    pub raw: Option<Raw>,
}

/// A maker's STEP file and how its solids stand in the part's frame.
#[derive(Clone, Debug)]
pub struct Raw {
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub src: crate::mesh::ModelSource,
}

/// A solid with round faces, written with its curved surfaces.
#[derive(Clone, Debug, PartialEq)]
pub enum Exact {
    /// A round cylinder: the centre of one end, the unit direction to the other, the radius and the length.
    Cylinder { base: [f64; 3], axis: [f64; 3], radius: f64, length: f64 },
    /// A flat plate between `z0` and `z1` over `outline` with round holes and slots cut through it: (centre, drill, slot length, along y).
    Plate { outline: Vec<Point>, holes: Vec<(Point, f64, f64, bool)>, z0: f64, z1: f64 },
}

const HOLE_SIDES: usize = 16;
const CYLINDER_SIDES: usize = 24;
const BOARD_COLOR: [u8; 3] = [30, 110, 60];

/// The loop of a hole (a circle, or the stadium of a slot) in the board's plane, counter-clockwise.
fn hole_loop(pos: Point, drill: f64, slot: f64, along_y: bool) -> Vec<Point> {
    let r = drill / 2.0;
    if slot <= drill {
        return (0..HOLE_SIDES).map(|k| { let a = std::f64::consts::TAU * k as f64 / HOLE_SIDES as f64; Point::new(pos.x + r * a.cos(), pos.y + r * a.sin()) }).collect();
    }
    // two half circles joined by straight sides
    let half = (slot - drill) / 2.0;
    let (ax, ay) = if along_y { (0.0, 1.0) } else { (1.0, 0.0) };
    let mut pts = vec![];
    for (end, from) in [(1.0, -std::f64::consts::FRAC_PI_2), (-1.0, std::f64::consts::FRAC_PI_2)] {
        let c = Point::new(pos.x + end * half * ax, pos.y + end * half * ay);
        let base = if along_y { from + std::f64::consts::FRAC_PI_2 } else { from };
        for k in 0..=HOLE_SIDES / 2 {
            let a = base + std::f64::consts::PI * k as f64 / (HOLE_SIDES / 2) as f64;
            pts.push(Point::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// A prism between `z0` and `z1` over `outer` (any winding) with `holes` cut through it.
pub(crate) fn prism(name: &str, color: [u8; 3], outer: &[Point], holes: &[Vec<Point>], z0: f64, z1: f64) -> Solid {
    let ccw = |p: &[Point], want: bool| -> Vec<Point> {
        let mut v = p.to_vec();
        if (signed_area(&v) > 0.0) != want {
            v.reverse();
        }
        v
    };
    let mut s = Solid { name: name.into(), color, ..Default::default() };
    // every loop: its bottom and top vertices
    let mut loops: Vec<(Vec<usize>, Vec<usize>)> = vec![];
    for (k, l) in std::iter::once(ccw(outer, true)).chain(holes.iter().map(|h| ccw(h, false))).enumerate() {
        let _ = k;
        let bottom: Vec<usize> = l.iter().map(|p| { s.vertices.push([p.x, p.y, z0]); s.vertices.len() - 1 }).collect();
        let top: Vec<usize> = l.iter().map(|p| { s.vertices.push([p.x, p.y, z1]); s.vertices.len() - 1 }).collect();
        loops.push((bottom, top));
    }
    // top: outer counter-clockwise, holes clockwise (as stored); bottom: both reversed
    let top_face = (loops[0].1.clone(), loops[1..].iter().map(|l| l.1.clone()).collect());
    let rev = |v: &Vec<usize>| v.iter().rev().copied().collect::<Vec<_>>();
    let bottom_face = (rev(&loops[0].0), loops[1..].iter().map(|l| rev(&l.0)).collect());
    s.faces.push(top_face);
    s.faces.push(bottom_face);
    // the walls: the material is on the left of every loop, so the wall a -> b faces to its right
    for (bottom, top) in &loops {
        let n = bottom.len();
        for i in 0..n {
            let j = (i + 1) % n;
            s.faces.push((vec![bottom[i], bottom[j], top[j], top[i]], vec![]));
        }
    }
    s
}

/// The board's solid: its outline raised to the board's thickness, with the mounting holes and the through-hole drills cut through
/// (holes that reach outside the outline or into another hole are left out).
pub fn board_solid(board: &Board) -> Option<Solid> {
    if board.outline.len() < 3 {
        return None;
    }
    // (centre, reach, the faceted loop, the hole as written round)
    type Kept = (Point, f64, Vec<Point>, (Point, f64, f64, bool));
    let mut holes: Vec<Kept> = vec![];
    let candidates = board.holes.iter().map(|h| (h.pos, h.drill, h.slot, h.slot_v)).chain(board.pads.iter().filter_map(|p| p.drill.map(|d| (p.pos, d, p.slot, p.slot_v))));
    for (pos, drill, slot, v) in candidates {
        if !(drill > 0.0) {
            continue;
        }
        let reach = drill.max(slot) / 2.0;
        let l = hole_loop(pos, drill, slot, v);
        if !l.iter().all(|p| inside_polygon(&board.outline, *p)) || holes.iter().any(|(q, r, ..)| q.dist(pos) < reach + r + 0.01) {
            continue;
        }
        holes.push((pos, reach, l, (pos, drill, slot, v)));
    }
    let name = if board.name.is_empty() { "board".to_string() } else { board.name.clone() };
    let mut s = prism(&name, BOARD_COLOR, &board.outline, &holes.iter().map(|h| h.2.clone()).collect::<Vec<_>>(), 0.0, BOARD_THICKNESS);
    s.exact = Some(Exact::Plate { outline: board.outline.clone(), holes: holes.into_iter().map(|h| h.3).collect(), z0: 0.0, z1: BOARD_THICKNESS });
    Some(s)
}

/// One solid of a part: a box, or a cylinder (a 24-sided prism) along its axis.
fn part_solid(b: &PartBox) -> Solid {
    let ([x0, y0, z0], [x1, y1, z1]) = (b.lo, b.hi);
    match b.shape {
        BodyShape::Box => prism(&b.refdes, b.color, &[Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)], &[], z0, z1),
        BodyShape::Cylinder(axis) => {
            // the circle in the two other axes, then turned into place
            let (lo, hi) = (b.lo, b.hi);
            let (u, v, w) = match axis {
                0 => (1usize, 2usize, 0usize),
                1 => (2, 0, 1),
                _ => (0, 1, 2),
            };
            let (cu, cv) = ((lo[u] + hi[u]) / 2.0, (lo[v] + hi[v]) / 2.0);
            let (ru, rv) = ((hi[u] - lo[u]) / 2.0, (hi[v] - lo[v]) / 2.0);
            let ring: Vec<Point> = (0..CYLINDER_SIDES).map(|k| { let a = std::f64::consts::TAU * k as f64 / CYLINDER_SIDES as f64; Point::new(cu + ru * a.cos(), cv + rv * a.sin()) }).collect();
            let mut s = prism(&b.refdes, b.color, &ring, &[], lo[w], hi[w]);
            // (u, v, w) of the prism back to (x, y, z): the prism was built with u as x, v as y, w as z
            for p in &mut s.vertices {
                let mut q = [0.0; 3];
                q[u] = p[0];
                q[v] = p[1];
                q[w] = p[2];
                *p = q;
            }
            // round (not squashed): written as a true cylinder
            if (ru - rv).abs() < 1e-9 && ru > 0.0 && hi[w] > lo[w] {
                let mut base = [0.0; 3];
                (base[u], base[v], base[w]) = (cu, cv, lo[w]);
                let mut axis = [0.0; 3];
                axis[w] = 1.0;
                s.exact = Some(Exact::Cylinder { base, axis, radius: ru, length: hi[w] - lo[w] });
            }
            s
        }
    }
}

/// A placed maker's mesh as a solid of triangles (the vertices welded where they are the same point).
fn mesh_solid(name: &str, mesh: &crate::mesh::Mesh3d) -> Solid {
    let mut s = Solid { name: name.into(), ..Default::default() };
    // the colour most of its triangles have
    let mut count: HashMap<u8, usize> = HashMap::new();
    for c in &mesh.tri_color {
        *count.entry(*c).or_default() += 1;
    }
    s.color = count.into_iter().max_by_key(|(_, n)| *n).and_then(|(c, _)| mesh.palette.get(c as usize).copied()).unwrap_or([150, 150, 150]);
    let mut at: HashMap<[u32; 3], usize> = HashMap::new();
    let mut index = |v: [f32; 3], s: &mut Solid| -> usize {
        *at.entry(v.map(f32::to_bits)).or_insert_with(|| {
            s.vertices.push(v.map(f64::from));
            s.vertices.len() - 1
        })
    };
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
        let (a, b, c) = (index(a, &mut s), index(b, &mut s), index(c, &mut s));
        if a != b && b != c && a != c {
            s.faces.push((vec![a, b, c], vec![]));
        }
    }
    s
}

/// Every solid of the board: the board and its parts.
pub fn solids(board: &Board) -> Vec<Solid> {
    let mut out: Vec<Solid> = board_solid(board).into_iter().collect();
    out.extend(part_boxes(board).iter().map(part_solid));
    out.extend(part_meshes(board).iter().map(|m| mesh_solid(&m.refdes, &m.mesh)));
    out
}

/// A STEP real: always with a decimal point.
fn real(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.to_string() }
}

/// A STEP string: the characters outside plain ASCII written as `\X2\...\X0\`, a quote doubled.
fn text(s: &str) -> String {
    let mut out = String::new();
    // a run of characters outside plain ASCII goes into one \X2\ ... \X0\
    let mut wide = false;
    for c in s.chars() {
        let plain = c.is_ascii() && !c.is_ascii_control();
        if plain && wide {
            out.push_str("\\X0\\");
            wide = false;
        }
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c if plain => out.push(c),
            c => {
                if !wide {
                    out.push_str("\\X2\\");
                    wide = true;
                }
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "{:04X}", u);
                }
            }
        }
    }
    if wide {
        out.push_str("\\X0\\");
    }
    out
}

/// Writes entities with numbers counting up.
struct Out {
    body: String,
    next: usize,
}

impl Out {
    fn add(&mut self, entity: impl AsRef<str>) -> usize {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.body, "#{id}={};", entity.as_ref());
        id
    }

    fn direction(&mut self, d: [f64; 3]) -> usize {
        self.add(format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2])))
    }

    fn point(&mut self, p: [f64; 3]) -> usize {
        self.add(format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (n > 1e-12).then(|| [v[0] / n, v[1] / n, v[2] / n])
}

/// The normal of a loop (Newell's method), pointing to the side it is counter-clockwise seen from.
fn normal(vs: &[[f64; 3]], l: &[usize]) -> Option<[f64; 3]> {
    let mut n = [0.0; 3];
    for i in 0..l.len() {
        let (a, b) = (vs[l[i]], vs[l[(i + 1) % l.len()]]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    unit(n)
}

/// One solid's topology and geometry; returns the `MANIFOLD_SOLID_BREP`.
fn write_solid(o: &mut Out, s: &Solid) -> Option<usize> {
    let points: Vec<usize> = s.vertices.iter().map(|p| o.point(*p)).collect();
    let verts: Vec<usize> = points.iter().map(|p| o.add(format!("VERTEX_POINT('',#{p})"))).collect();
    // one edge per pair of vertices, from the lower index to the higher
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    let mut edge = |o: &mut Out, a: usize, b: usize| -> Option<String> {
        let (lo, hi) = (a.min(b), a.max(b));
        let id = match edges.get(&(lo, hi)) {
            Some(id) => *id,
            None => {
                let d = sub(s.vertices[hi], s.vertices[lo]);
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let dir = o.direction(unit(d)?);
                let vec = o.add(format!("VECTOR('',#{dir},{})", real(len)));
                let line = o.add(format!("LINE('',#{},#{vec})", points[lo]));
                let id = o.add(format!("EDGE_CURVE('',#{},#{},#{line},.T.)", verts[lo], verts[hi]));
                edges.insert((lo, hi), id);
                id
            }
        };
        Some(format!("ORIENTED_EDGE('',*,*,#{id},{})", if a < b { ".T." } else { ".F." }))
    };
    let mut faces = vec![];
    for (outer, holes) in &s.faces {
        let Some(n) = normal(&s.vertices, outer) else { continue };
        let mut bounds = vec![];
        for (k, l) in std::iter::once(outer).chain(holes.iter()).enumerate() {
            let mut oes = vec![];
            for i in 0..l.len() {
                let oe = edge(o, l[i], l[(i + 1) % l.len()])?;
                oes.push(format!("#{}", o.add(oe)));
            }
            let lp = o.add(format!("EDGE_LOOP('',({}))", oes.join(",")));
            bounds.push(format!("#{}", o.add(format!("{}('',#{lp},.T.)", if k == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }))));
        }
        let origin = points[outer[0]];
        let refdir = unit(sub(s.vertices[outer[1]], s.vertices[outer[0]]))?;
        // the reference direction must lie in the plane
        let along = refdir[0] * n[0] + refdir[1] * n[1] + refdir[2] * n[2];
        let refdir = unit([refdir[0] - along * n[0], refdir[1] - along * n[1], refdir[2] - along * n[2]])?;
        let (nd, rd) = (o.direction(n), o.direction(refdir));
        let place = o.add(format!("AXIS2_PLACEMENT_3D('',#{origin},#{nd},#{rd})"));
        let plane = o.add(format!("PLANE('',#{place})"));
        faces.push(format!("#{}", o.add(format!("ADVANCED_FACE('',({}),#{plane},.T.)", bounds.join(",")))));
    }
    if faces.is_empty() {
        return None;
    }
    let shell = o.add(format!("CLOSED_SHELL('',({}))", faces.join(",")));
    Some(o.add(format!("MANIFOLD_SOLID_BREP('{}',#{shell})", text(&s.name))))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn add3(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + k * b[0], a[1] + k * b[1], a[2] + k * b[2]]
}

/// A piece of a hole's outline in the board's plane, from one point to the next counter-clockwise: straight, or an arc round `centre`.
#[derive(Clone, Copy)]
enum Seg {
    Line,
    Arc(Point),
}

/// A hole's outline counter-clockwise: its corners and the piece from each to the next (a round hole is two half circles; a slot a half
/// circle, a side, a half circle and a side).
fn hole_profile(c: Point, drill: f64, slot: f64, along_y: bool) -> Vec<(Point, Seg)> {
    let r = drill / 2.0;
    if slot <= drill {
        return vec![(Point::new(c.x + r, c.y), Seg::Arc(c)), (Point::new(c.x - r, c.y), Seg::Arc(c))];
    }
    let half = (slot - drill) / 2.0;
    if along_y {
        let (c1, c2) = (Point::new(c.x, c.y + half), Point::new(c.x, c.y - half));
        vec![(Point::new(c1.x + r, c1.y), Seg::Arc(c1)), (Point::new(c1.x - r, c1.y), Seg::Line), (Point::new(c2.x - r, c2.y), Seg::Arc(c2)), (Point::new(c2.x + r, c2.y), Seg::Line)]
    } else {
        let (c1, c2) = (Point::new(c.x + half, c.y), Point::new(c.x - half, c.y));
        vec![(Point::new(c1.x, c1.y - r), Seg::Arc(c1)), (Point::new(c1.x, c1.y + r), Seg::Line), (Point::new(c2.x, c2.y + r), Seg::Arc(c2)), (Point::new(c2.x, c2.y - r), Seg::Line)]
    }
}

impl Out {
    fn placement(&mut self, at: [f64; 3], axis: [f64; 3], refdir: [f64; 3]) -> usize {
        let (p, a, r) = (self.point(at), self.direction(axis), self.direction(refdir));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{a},#{r})"))
    }

    fn vertex(&mut self, p: [f64; 3]) -> usize {
        let pt = self.point(p);
        self.add(format!("VERTEX_POINT('',#{pt})"))
    }

    /// A straight edge from `a` (vertex `va`) to `b` (vertex `vb`).
    fn line_edge(&mut self, a: [f64; 3], b: [f64; 3], va: usize, vb: usize) -> usize {
        let d = sub(b, a);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let (p, dir) = (self.point(a), self.direction(unit(d).unwrap_or([1.0, 0.0, 0.0])));
        let vec = self.add(format!("VECTOR('',#{dir},{})", real(len)));
        let line = self.add(format!("LINE('',#{p},#{vec})"));
        self.add(format!("EDGE_CURVE('',#{va},#{vb},#{line},.T.)"))
    }

    /// An arc of the circle `circle` from vertex `va` to `vb`, counter-clockwise about the circle's axis.
    fn arc_edge(&mut self, circle: usize, va: usize, vb: usize) -> usize {
        self.add(format!("EDGE_CURVE('',#{va},#{vb},#{circle},.T.)"))
    }

    /// A face of `surface` bounded by loops of (edge, used along it): the first the outer bound.
    fn face(&mut self, loops: &[Vec<(usize, bool)>], surface: usize, same_sense: bool) -> usize {
        let mut bounds = vec![];
        for (k, l) in loops.iter().enumerate() {
            let oes: Vec<String> = l.iter().map(|(e, along)| format!("#{}", self.add(format!("ORIENTED_EDGE('',*,*,#{e},{})", if *along { ".T." } else { ".F." })))).collect();
            let lp = self.add(format!("EDGE_LOOP('',({}))", oes.join(",")));
            bounds.push(format!("#{}", self.add(format!("{}('',#{lp},.T.)", if k == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }))));
        }
        self.add(format!("ADVANCED_FACE('',({}),#{surface},{})", bounds.join(","), if same_sense { ".T." } else { ".F." }))
    }

    fn plane(&mut self, at: [f64; 3], normal: [f64; 3], refdir: [f64; 3]) -> usize {
        let pl = self.placement(at, normal, refdir);
        self.add(format!("PLANE('',#{pl})"))
    }
}

/// A round cylinder: two half cylinders between two arcs at each end (no seam edge), and the two round caps.
fn write_cylinder(o: &mut Out, base: [f64; 3], axis: [f64; 3], r: f64, len: f64) -> Vec<usize> {
    // a reference direction across the axis: θ = 0 there, θ = π opposite
    let refdir = unit(cross(axis, if axis[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] })).unwrap_or([1.0, 0.0, 0.0]);
    let top = add3(base, axis, len);
    let (p0b, p1b, p0t, p1t) = (add3(base, refdir, r), add3(base, refdir, -r), add3(top, refdir, r), add3(top, refdir, -r));
    let (v0b, v1b, v0t, v1t) = (o.vertex(p0b), o.vertex(p1b), o.vertex(p0t), o.vertex(p1t));
    let pb = o.placement(base, axis, refdir);
    let cb = o.add(format!("CIRCLE('',#{pb},{})", real(r)));
    let pt = o.placement(top, axis, refdir);
    let ct = o.add(format!("CIRCLE('',#{pt},{})", real(r)));
    let (ab, bb, at, bt) = (o.arc_edge(cb, v0b, v1b), o.arc_edge(cb, v1b, v0b), o.arc_edge(ct, v0t, v1t), o.arc_edge(ct, v1t, v0t));
    let (l0, l1) = (o.line_edge(p0b, p0t, v0b, v0t), o.line_edge(p1b, p1t, v1b, v1t));
    let place = o.placement(base, axis, refdir);
    let surf = o.add(format!("CYLINDRICAL_SURFACE('',#{place},{})", real(r)));
    let side_a = o.face(&[vec![(ab, true), (l1, true), (at, false), (l0, false)]], surf, true);
    let side_b = o.face(&[vec![(bb, true), (l0, true), (bt, false), (l1, false)]], surf, true);
    let top_pl = o.plane(top, axis, refdir);
    let cap_t = o.face(&[vec![(at, true), (bt, true)]], top_pl, true);
    let bot_pl = o.plane(base, [-axis[0], -axis[1], -axis[2]], refdir);
    let cap_b = o.face(&[vec![(bb, false), (ab, false)]], bot_pl, true);
    vec![side_a, side_b, cap_t, cap_b]
}

/// A plate between `z0` and `z1`: the outline's straight walls, and round holes and slots with walls of cylinders and planes.
fn write_plate(o: &mut Out, outline: &[Point], holes: &[(Point, f64, f64, bool)], z0: f64, z1: f64) -> Vec<usize> {
    let (zx, up, down) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
    let mut outer = outline.to_vec();
    if signed_area(&outer) < 0.0 {
        outer.reverse();
    }
    let at = |p: Point, z: f64| [p.x, p.y, z];
    let mut faces = vec![];
    // the outline: bottom and top corners, the edges along the bottom, the top and up the corners
    let n = outer.len();
    let vb: Vec<usize> = outer.iter().map(|p| o.vertex(at(*p, z0))).collect();
    let vt: Vec<usize> = outer.iter().map(|p| o.vertex(at(*p, z1))).collect();
    let eb: Vec<usize> = (0..n).map(|i| o.line_edge(at(outer[i], z0), at(outer[(i + 1) % n], z0), vb[i], vb[(i + 1) % n])).collect();
    let et: Vec<usize> = (0..n).map(|i| o.line_edge(at(outer[i], z1), at(outer[(i + 1) % n], z1), vt[i], vt[(i + 1) % n])).collect();
    let ev: Vec<usize> = (0..n).map(|i| o.line_edge(at(outer[i], z0), at(outer[i], z1), vb[i], vt[i])).collect();
    for i in 0..n {
        let j = (i + 1) % n;
        let Some(dir) = unit(sub(at(outer[j], z0), at(outer[i], z0))) else { continue };
        // the material is on the left of a counter-clockwise outline: the wall faces right
        let pl = o.plane(at(outer[i], z0), cross(dir, up), dir);
        faces.push(o.face(&[vec![(eb[i], true), (ev[j], true), (et[i], false), (ev[i], false)]], pl, true));
    }
    // the holes: each corner's bottom and top vertices, the pieces along the bottom and the top, and the edges up the corners
    let mut top_loops: Vec<Vec<(usize, bool)>> = vec![(0..n).map(|i| (et[i], true)).collect()];
    let mut bottom_loops: Vec<Vec<(usize, bool)>> = vec![(0..n).rev().map(|i| (eb[i], false)).collect()];
    for &(c, drill, slot, along_y) in holes {
        let prof = hole_profile(c, drill, slot, along_y);
        let m = prof.len();
        let hb: Vec<usize> = prof.iter().map(|(p, _)| o.vertex(at(*p, z0))).collect();
        let ht: Vec<usize> = prof.iter().map(|(p, _)| o.vertex(at(*p, z1))).collect();
        let hv: Vec<usize> = (0..m).map(|i| o.line_edge(at(prof[i].0, z0), at(prof[i].0, z1), hb[i], ht[i])).collect();
        let mut sb = vec![];
        let mut st = vec![];
        for i in 0..m {
            let j = (i + 1) % m;
            let (a, b) = (prof[i].0, prof[j].0);
            match prof[i].1 {
                Seg::Line => {
                    sb.push(o.line_edge(at(a, z0), at(b, z0), hb[i], hb[j]));
                    st.push(o.line_edge(at(a, z1), at(b, z1), ht[i], ht[j]));
                }
                Seg::Arc(centre) => {
                    let r = a.dist(centre);
                    let mut circle = |z: f64| {
                        let pl = o.placement(at(centre, z), up, zx);
                        o.add(format!("CIRCLE('',#{pl},{})", real(r)))
                    };
                    let (cb, ct) = (circle(z0), circle(z1));
                    sb.push(o.arc_edge(cb, hb[i], hb[j]));
                    st.push(o.arc_edge(ct, ht[i], ht[j]));
                }
            }
        }
        // the hole in the top face runs clockwise seen from above, in the bottom face counter-clockwise
        top_loops.push((0..m).rev().map(|i| (st[i], false)).collect());
        bottom_loops.push((0..m).map(|i| (sb[i], true)).collect());
        // the walls face into the hole
        for i in 0..m {
            let j = (i + 1) % m;
            let lp = vec![(st[i], true), (hv[j], false), (sb[i], false), (hv[i], true)];
            match prof[i].1 {
                Seg::Arc(centre) => {
                    let place = o.placement(at(centre, z0), up, zx);
                    let surf = o.add(format!("CYLINDRICAL_SURFACE('',#{place},{})", real(prof[i].0.dist(centre))));
                    faces.push(o.face(&[lp], surf, false));
                }
                Seg::Line => {
                    let dir = unit(sub(at(prof[j].0, z0), at(prof[i].0, z0))).unwrap_or(zx);
                    let pl = o.plane(at(prof[i].0, z0), cross(up, dir), dir);
                    faces.push(o.face(&[lp], pl, true));
                }
            }
        }
    }
    let top_pl = o.plane([0.0, 0.0, z1], up, zx);
    faces.push(o.face(&top_loops, top_pl, true));
    let bot_pl = o.plane([0.0, 0.0, z0], down, zx);
    faces.push(o.face(&bottom_loops, bot_pl, true));
    faces
}

/// A solid with round faces; returns the `MANIFOLD_SOLID_BREP`.
fn write_exact(o: &mut Out, name: &str, e: &Exact) -> usize {
    let faces = match e {
        Exact::Cylinder { base, axis, radius, length } => write_cylinder(o, *base, *axis, *radius, *length),
        Exact::Plate { outline, holes, z0, z1 } => write_plate(o, outline, holes, *z0, *z1),
    };
    let list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
    let shell = o.add(format!("CLOSED_SHELL('',({}))", list.join(",")));
    o.add(format!("MANIFOLD_SOLID_BREP('{}',#{shell})", text(name)))
}

/// Where a part stands, as the axes of its frame in the board's: the origin on the face it sits on, z away from the board, x the
/// footprint's x turned with the part (and pointing the other way on the bottom: turned over about y).
pub fn part_axes(part: &Part) -> Axes {
    let bottom = part.side == Side::Bottom;
    let x = crate::parts::rotate(Point::new(if bottom { -1.0 } else { 1.0 }, 0.0), part.rot);
    [[part.pos.x, part.pos.y, if bottom { 0.0 } else { BOARD_THICKNESS }], [0.0, 0.0, if bottom { -1.0 } else { 1.0 }], [x.x, x.y, 0.0]]
}

/// The solid moved from the board's frame into the frame `at` (its points and its exact shape).
fn to_local(mut s: Solid, at: &Axes) -> Solid {
    let [o, z, x] = *at;
    let y = cross(z, x);
    let local = |p: [f64; 3]| { let d = sub(p, o); [d[0] * x[0] + d[1] * x[1] + d[2] * x[2], d[0] * y[0] + d[1] * y[1] + d[2] * y[2], d[0] * z[0] + d[1] * z[1] + d[2] * z[2]] };
    let turn = |v: [f64; 3]| [v[0] * x[0] + v[1] * x[1] + v[2] * x[2], v[0] * y[0] + v[1] * y[1] + v[2] * y[2], v[0] * z[0] + v[1] * z[1] + v[2] * z[2]];
    for v in &mut s.vertices {
        *v = local(*v);
    }
    if let Some(Exact::Cylinder { base, axis, .. }) = &mut s.exact {
        (*base, *axis) = (local(*base), turn(*axis));
    }
    s
}

/// The board and its parts as a STEP file (AP214), lengths in millimetres, the board's bottom face at z = 0.
pub fn write(board: &Board) -> String {
    let name = if board.name.is_empty() { "board".to_string() } else { board.name.clone() };
    let mut groups: Vec<(String, Vec<Solid>, Axes)> = vec![];
    for (k, s) in solids(board).into_iter().enumerate() {
        // the board's solid first, then the parts by designator (a part's several solids stay together, in the part's frame)
        let pcb = k == 0 && s.exact.as_ref().is_some_and(|e| matches!(e, Exact::Plate { .. }));
        let key = if pcb { format!("{name}_PCB") } else { s.name.clone() };
        let at = if pcb { IDENTITY } else { board.parts.iter().find(|p| p.refdes == s.name).map_or(IDENTITY, part_axes) };
        let s = if pcb { s } else { to_local(s, &at) };
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(s),
            None => groups.push((key, vec![s], at)),
        }
    }
    // a maker's model whose STEP file is here: its own faces instead of the triangles
    for g in &mut groups {
        let Some(part) = board.parts.iter().find(|p| p.refdes == g.0) else { continue };
        let (Some(src), true) = (board.models3d_src.get(&part.footprint), board.models3d.contains_key(&part.footprint)) else { continue };
        if let (Ok(bytes), [one]) = (std::fs::read(&src.path), g.1.as_mut_slice()) {
            one.raw = Some(Raw { bytes: std::sync::Arc::new(bytes), src: src.clone() });
        }
    }
    groups.extend(crate::step_layers::layers(board));
    write_assembly(&name, groups)
}

/// Several boards put together (E5): one product per board with all its solids, placed where the board stands (`stack::Place`, a real
/// placement, unlike the parts of one board which keep the board's frame).
pub fn write_system(name: &str, boards: &[(&str, &Board, crate::stack::Place)]) -> String {
    let groups = boards
        .iter()
        .map(|(n, b, pl)| {
            let col = |k: usize| [f64::from(pl.m[0][k]), f64::from(pl.m[1][k]), f64::from(pl.m[2][k])];
            (n.to_string(), solids(b), [pl.offset, col(2), col(0)])
        })
        .collect();
    write_assembly(name, groups)
}

/// Where a product stands in the assembly: its origin, z axis and x axis.
pub type Axes = [[f64; 3]; 3];
const IDENTITY: Axes = [[0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]];

/// The STEP file of an assembly: under the root product named `name`, one product per group (its solids in one representation) placed
/// by its axes.
fn write_assembly(name: &str, groups: Vec<(String, Vec<Solid>, Axes)>) -> String {
    let name = name.to_string();
    let mut o = Out { body: String::new(), next: 1 };
    let app = o.add("APPLICATION_CONTEXT('core data for automotive mechanical design processes')");
    o.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let pctx = o.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let dctx = o.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let mm = o.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let rad = o.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
    let sr = o.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
    let unc = o.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{mm},'distance_accuracy_value','confusion accuracy')"));
    let ctx = o.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    // a product: its definition, and the shape that definition has
    let product = |o: &mut Out, n: &str| -> (usize, usize) {
        let p = o.add(format!("PRODUCT('{0}','{0}','',(#{pctx}))", text(n)));
        o.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{p}))"));
        let form = o.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{p})"));
        let def = o.add(format!("PRODUCT_DEFINITION('design','',#{form},#{dctx})"));
        let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{def})"));
        (def, pds)
    };
    let origin_axes = |o: &mut Out| -> usize {
        let origin = o.point([0.0; 3]);
        let (z, x) = (o.direction([0.0, 0.0, 1.0]), o.direction([1.0, 0.0, 0.0]));
        o.add(format!("AXIS2_PLACEMENT_3D('',#{origin},#{z},#{x})"))
    };
    // the assembly: the board and every part, each a product of its own so that the case's CAD can pick and move it
    let (root_def, root_pds) = product(&mut o, &name);
    let root_axes = origin_axes(&mut o);
    let mut styled = vec![];
    let mut children = vec![];
    for (gname, group, at) in &groups {
        let axes = origin_axes(&mut o);
        let mut items = vec![format!("#{axes}")];
        let style = |o: &mut Out, id: usize, c: [u8; 3]| -> String {
            let [r, g, b] = c.map(|c| f64::from(c) / 255.0);
            let colour = o.add(format!("COLOUR_RGB('',{},{},{})", real(r), real(g), real(b)));
            let fill = o.add(format!("FILL_AREA_STYLE_COLOUR('',#{colour})"));
            let area = o.add(format!("FILL_AREA_STYLE('',(#{fill}))"));
            let surf = o.add(format!("SURFACE_STYLE_FILL_AREA(#{area})"));
            let side = o.add(format!("SURFACE_SIDE_STYLE('',(#{surf}))"));
            let usage = o.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
            let assign = o.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
            format!("#{}", o.add(format!("STYLED_ITEM('color',(#{assign}),#{id})")))
        };
        for s in group {
            // a maker's own faces: copied in, each solid and face with the colour the file gave it
            if let Some(raw) = &s.raw
                && let Ok(c) = crate::step::copy_solids(&raw.bytes, o.next, &|p| raw.src.place(p), &|d| raw.src.turn(d))
            {
                o.body.push_str(&c.text);
                o.next += c.count;
                for (id, col) in &c.solids {
                    items.push(format!("#{id}"));
                    styled.push(style(&mut o, *id, col.unwrap_or(s.color)));
                }
                for (id, col) in &c.faces {
                    styled.push(style(&mut o, *id, *col));
                }
                continue;
            }
            let id = match &s.exact {
                Some(e) => write_exact(&mut o, &s.name, e),
                None => match write_solid(&mut o, s) {
                    Some(id) => id,
                    None => continue,
                },
            };
            items.push(format!("#{id}"));
            styled.push(style(&mut o, id, s.color));
        }
        if items.len() == 1 {
            continue;
        }
        let (def, pds) = product(&mut o, gname);
        let rep = o.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION('{}',({}),#{ctx})", text(gname), items.join(",")));
        o.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
        children.push((gname.clone(), def, rep, axes, *at));
    }
    // the root holds every group where it stands (the parts of one board: the board's frame, an identity placement)
    let mut root_items = vec![format!("#{root_axes}")];
    let mut placements = vec![];
    for (.., at) in &children {
        let (p, z, x) = (o.point(at[0]), o.direction(at[1]), o.direction(at[2]));
        placements.push(o.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})")));
    }
    root_items.extend(placements.iter().map(|p| format!("#{p}")));
    let root_rep = o.add(format!("SHAPE_REPRESENTATION('{}',({}),#{ctx})", text(&name), root_items.join(",")));
    o.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{root_pds},#{root_rep})"));
    for ((cname, def, rep, axes, _), at) in children.iter().zip(&placements) {
        let nauo = o.add(format!("NEXT_ASSEMBLY_USAGE_OCCURRENCE('{0}','{0}','',#{root_def},#{def},$)", text(cname)));
        let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{nauo})"));
        let idt = o.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{axes},#{at})"));
        let rel = o.add(format!("(REPRESENTATION_RELATIONSHIP('','',#{rep},#{root_rep})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt})SHAPE_REPRESENTATION_RELATIONSHIP())"));
        o.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rel},#{pds})"));
    }
    if !styled.is_empty() {
        o.add(format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#{ctx})", styled.join(",")));
    }
    format!(
        "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('circuitCad board and parts'),'2;1');
FILE_NAME('{0}.step','',(''),(''),'circuitCad','circuitCad','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));
ENDSEC;
DATA;
{1}ENDSEC;
END-ISO-10303-21;
",
        text(&name),
        o.body
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(s: &Solid) -> f64 {
        // the divergence theorem over the faces fanned into triangles (holes as faces of their own, turned the other way)
        let tri = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
        let mut v = 0.0;
        for (outer, holes) in &s.faces {
            for l in std::iter::once(outer).chain(holes) {
                for i in 1..l.len() - 1 {
                    v += tri(s.vertices[l[0]], s.vertices[l[i]], s.vertices[l[i + 1]]);
                }
            }
        }
        v
    }

    /// Every edge of a closed solid is used once in each direction.
    fn closed(s: &Solid) -> bool {
        let mut seen: HashMap<(usize, usize), i32> = HashMap::new();
        for (outer, holes) in &s.faces {
            for l in std::iter::once(outer).chain(holes) {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    *seen.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
                }
            }
        }
        seen.values().all(|n| *n == 0)
    }

    #[test]
    fn the_board_is_its_outline_raised_with_the_holes_cut_through() {
        let mut b = Board::new("t", 40.0, 30.0);
        b.holes.push(Hole { pos: Point::new(5.0, 5.0), drill: 3.2, slot: 0.0, slot_v: false, refdes: String::new() });
        // a hole that would cut the edge is left out
        b.holes.push(Hole { pos: Point::new(0.5, 15.0), drill: 3.2, slot: 0.0, slot_v: false, refdes: String::new() });
        let s = board_solid(&b).unwrap();
        assert!(closed(&s), "every edge twice, once each way");
        // one hole: 16-sided, its area a little under the circle's
        let polygon_area = 0.5 * HOLE_SIDES as f64 * 1.6f64.powi(2) * (std::f64::consts::TAU / HOLE_SIDES as f64).sin();
        assert!((volume(&s) - (40.0 * 30.0 - polygon_area) * BOARD_THICKNESS).abs() < 1e-6, "{}", volume(&s));
        assert_eq!(s.faces[0].1.len(), 1);
    }

    #[test]
    fn a_cylinder_stands_along_its_axis_and_every_solid_is_closed() {
        let pb = PartBox { refdes: "C1".into(), part: 0, lo: [0.0, 0.0, 1.6], hi: [6.0, 6.0, 12.6], color: [10, 20, 30], bottom: false, shape: BodyShape::Cylinder(2) };
        let s = part_solid(&pb);
        assert!(closed(&s));
        let ring = 0.5 * CYLINDER_SIDES as f64 * 9.0 * (std::f64::consts::TAU / CYLINDER_SIDES as f64).sin();
        assert!((volume(&s) - ring * 11.0).abs() < 1e-6, "{}", volume(&s));
        // lying along x: the same volume, the length along x
        let lying = PartBox { lo: [0.0, 0.0, 1.6], hi: [11.0, 6.0, 7.6], shape: BodyShape::Cylinder(0), ..pb };
        let s = part_solid(&lying);
        assert!(closed(&s) && (volume(&s) - ring * 11.0).abs() < 1e-6, "{}", volume(&s));
        assert!(s.vertices.iter().all(|v| v[0] >= -1e-9 && v[0] <= 11.0 + 1e-9));
    }

    /// The file is read back by our own STEP reader: the same box comes back with the same volume and place.
    #[test]
    fn the_file_reads_back_as_the_same_solids() {
        let mut b = Board::new("テスト基板", 40.0, 30.0);
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top }).unwrap();
        assert!(!part_boxes(&b).is_empty(), "the resistor has solids");
        let text = write(&b);
        assert!(text.starts_with("ISO-10303-21;"));
        assert!(text.contains("MANIFOLD_SOLID_BREP('R1'"), "named after the part");
        assert!(text.contains("PRODUCT('\\X2\\30C630B930C857FA677F\\X0\\'"), "the Japanese name is escaped as one run");
        // an assembly: the board, the resistor, and the top copper and silkscreen each a product of their own, under the board's
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE(").count(), 4, "the board, R1, the copper and the silkscreen");
        assert!(text.contains("_COPPER_TOP'") && text.contains("_SILK_TOP'") && !text.contains("_COPPER_BOTTOM'"), "nothing on the bottom");
        assert!(text.contains("PRODUCT('R1','R1'") && text.contains("_PCB','"), "products named after the part and the board");
        assert!(edges_pair_up(&text));
        let m = crate::step::read_step(text.as_bytes()).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!(lo[0].abs() < 1e-3 && lo[1].abs() < 1e-3 && lo[2].abs() < 1e-3 && (hi[0] - 40.0).abs() < 1e-3 && (hi[1] - 30.0).abs() < 1e-3, "{lo:?} {hi:?}");
        let layers = |b: &Board| -> f64 { crate::step_layers::layers(b).iter().flat_map(|g| g.1.iter()).map(volume).sum() };
        let want: f64 = solids(&b).iter().map(volume).sum::<f64>() + layers(&b);
        assert!((m.volume() - want).abs() < 1e-3 * want, "{} vs {want}", m.volume());
        // with a hole through the board: the face with an inner loop reads back too
        b.holes.push(Hole { pos: Point::new(5.0, 5.0), drill: 3.2, slot: 0.0, slot_v: false, refdes: String::new() });
        let m = crate::step::read_step(write(&b).as_bytes()).unwrap();
        let want: f64 = solids(&b).iter().map(volume).sum::<f64>() + layers(&b);
        assert!((m.volume() - want).abs() < 1e-3 * want, "with a hole: {} vs {want}", m.volume());
    }

    /// A part turned and on the bottom is written in its own frame and placed where it stands: read back, it fills the same box as the
    /// shapes in the board's frame; its placement is not the identity.
    #[test]
    fn parts_are_written_in_their_own_frame_and_placed() {
        let mut b = Board::new("t", 40.0, 30.0);
        b.set_part(Part { refdes: "C1".into(), value: String::new(), footprint: "C_1206".into(), pos: Point::new(12.0, 9.0), rot: 1, side: Side::Bottom }).unwrap();
        let boxes = part_boxes(&b);
        let (lo, hi) = boxes.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), x| (std::array::from_fn(|k| lo[k].min(x.lo[k])), std::array::from_fn(|k| hi[k].max(x.hi[k]))));
        let text = write(&b);
        // the part's own frame: its placement at its position, its z axis down
        assert!(text.contains("CARTESIAN_POINT('',(12.0,9.0,0.0))") && text.contains("DIRECTION('',(0.0,0.0,-1.0))"), "placed where it stands");
        let m = crate::step::read_step(text.as_bytes()).unwrap();
        let (mlo, _) = m.bounds().unwrap();
        assert!((mlo[2] - lo[2]).abs() < 1e-3, "below the board as before: {mlo:?} {lo:?}");
        // the part's own triangles (not the board's, the copper's or the silkscreen's colours), read back: the same box
        let skip = [BOARD_COLOR, [70, 150, 90], [212, 175, 55], [240, 240, 240]];
        let (mut plo, mut phi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (t, c) in m.triangles.iter().zip(&m.tri_color) {
            if skip.contains(&m.palette[*c as usize]) {
                continue;
            }
            for v in t.map(|i| m.vertices[i as usize].map(f64::from)) {
                (plo, phi) = (std::array::from_fn(|k| plo[k].min(v[k])), std::array::from_fn(|k| phi[k].max(v[k])));
            }
        }
        let near = |a: [f64; 3], c: [f64; 3]| (0..2).all(|k| (a[k] - c[k]).abs() < 1e-3);
        assert!(near(plo, lo) && near(phi, hi), "{plo:?} {phi:?} vs {lo:?} {hi:?}");
    }

    /// A maker's model whose STEP file is on this computer goes out with that file's curved faces, turned and moved as the mesh was;
    /// read back, it stands where the mesh does. Without the file, the triangles.
    #[test]
    fn a_makers_model_keeps_its_own_faces() {
        let src_text = crate::step::tests_src::cylinder();
        let dir = std::env::temp_dir().join("circuitcad_step_src");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("can.step");
        std::fs::write(&path, &src_text).unwrap();
        let src = crate::mesh::ModelSource::new(&path.to_string_lossy()).turned(0, 1).shifted([0.0, 0.0, 2.0]);
        let mesh = crate::step::read_step(src_text.as_bytes()).unwrap().turned(0, 1).shifted([0.0, 0.0, 2.0]);
        let mut b = Board::new("t", 40.0, 30.0);
        b.outline.clear();
        b.parts.push(Part { refdes: "U1".into(), value: String::new(), footprint: "CAN".into(), pos: Point::new(20.0, 10.0), rot: 0, side: Side::Top });
        b.models3d.insert("CAN".into(), mesh);
        let triangles = write(&b);
        assert!(!triangles.contains("CYLINDRICAL_SURFACE("), "no file named: the triangles");
        b.models3d_src.insert("CAN".into(), src);
        let text = write(&b);
        assert!(text.contains("CYLINDRICAL_SURFACE("), "the file's round face");
        let (a, c) = (crate::step::read_step(triangles.as_bytes()).unwrap(), crate::step::read_step(text.as_bytes()).unwrap());
        let ((alo, ahi), (clo, chi)) = (a.bounds().unwrap(), c.bounds().unwrap());
        assert!((0..3).all(|k| (alo[k] - clo[k]).abs() < 0.05 && (ahi[k] - chi[k]).abs() < 0.05), "{alo:?} {ahi:?} vs {clo:?} {chi:?}");
        // the file gone: the triangles again
        std::fs::remove_file(&path).unwrap();
        assert!(!write(&b).contains("CYLINDRICAL_SURFACE("));
    }

    /// Every `EDGE_CURVE` of the file used by exactly two oriented edges, once along it and once against it (each solid closed).
    fn edges_pair_up(text: &str) -> bool {
        let mut uses: HashMap<String, (i32, i32)> = HashMap::new();
        for line in text.lines().filter(|l| l.contains("ORIENTED_EDGE(")) {
            let inner = &line[line.find("ORIENTED_EDGE(").unwrap()..];
            let parts: Vec<&str> = inner.trim_end_matches(");").split(',').collect();
            let (edge, sense) = (parts[3].trim().to_string(), parts[4].trim());
            let e = uses.entry(edge).or_default();
            if sense.starts_with(".T.") { e.0 += 1 } else { e.1 += 1 }
        }
        let edges = text.lines().filter(|l| l.contains("=EDGE_CURVE(")).count();
        uses.len() == edges && uses.values().all(|u| *u == (1, 1))
    }

    /// Round holes, slots both ways and the parts' cylinders are written with curved faces and circles, and the file stays closed.
    #[test]
    fn round_holes_slots_and_cylinders_are_written_round() {
        let mut b = Board::new("t", 30.0, 20.0);
        b.holes.push(Hole { pos: Point::new(4.0, 4.0), drill: 3.2, slot: 0.0, slot_v: false, refdes: String::new() });
        b.holes.push(Hole { pos: Point::new(15.0, 4.0), drill: 1.2, slot: 4.0, slot_v: false, refdes: String::new() });
        b.holes.push(Hole { pos: Point::new(26.0, 14.0), drill: 1.0, slot: 3.0, slot_v: true, refdes: String::new() });
        let s = board_solid(&b).unwrap();
        assert!(matches!(&s.exact, Some(Exact::Plate { holes, .. }) if holes.len() == 3));
        let pb = PartBox { refdes: "C1".into(), part: 0, lo: [10.0, 8.0, 1.6], hi: [16.0, 14.0, 12.6], color: [10, 20, 30], bottom: false, shape: BodyShape::Cylinder(2) };
        assert!(matches!(part_solid(&pb).exact, Some(Exact::Cylinder { radius, length, .. }) if radius == 3.0 && length == 11.0));
        // squashed (an elliptic cross-section): stays faceted
        assert!(part_solid(&PartBox { hi: [17.0, 14.0, 12.6], ..pb.clone() }).exact.is_none());
        let mut o = Out { body: String::new(), next: 1 };
        let id = write_exact(&mut o, "t", s.exact.as_ref().unwrap());
        write_exact(&mut o, "C1", part_solid(&pb).exact.as_ref().unwrap());
        assert!(id > 0);
        // round hole: 1 cylinder (two half walls on one surface each); a slot: 2; the can: 1 (its two halves)
        assert_eq!(o.body.matches("CYLINDRICAL_SURFACE(").count(), 2 + 2 + 2 + 1, "{}", o.body.matches("CYLINDRICAL_SURFACE(").count());
        assert!(edges_pair_up(&o.body), "every edge used once each way");
        // the reader takes the file back: the box round it is the board's
        let m = crate::step::read_step(write(&b).as_bytes()).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi[0] - lo[0] - 30.0).abs() < 1e-3 && (hi[2] - lo[2] - BOARD_THICKNESS).abs() < 1e-3);
    }

    /// SolidCraft (its SMLib kernel) reads the file as closed solids of the exact volume: the board less a round hole and two slots,
    /// a cylinder of π r² h (`--ignored`: needs SolidCraft).
    #[test]
    #[ignore]
    fn solidcraft_reads_the_round_faces_exactly() {
        let Some((cli, _)) = crate::shape_script::find_solidcraft() else { panic!("SolidCraft is not installed") };
        let mut b = Board::new("t", 30.0, 20.0);
        b.holes.push(Hole { pos: Point::new(4.0, 4.0), drill: 3.2, slot: 0.0, slot_v: false, refdes: String::new() });
        b.holes.push(Hole { pos: Point::new(15.0, 4.0), drill: 1.2, slot: 4.0, slot_v: false, refdes: String::new() });
        b.holes.push(Hole { pos: Point::new(26.0, 14.0), drill: 1.0, slot: 3.0, slot_v: true, refdes: String::new() });
        let dir = std::env::temp_dir().join("circuitcad_step_round");
        std::fs::create_dir_all(&dir).unwrap();
        let (step, obj) = (dir.join("b.step"), dir.join("b.obj"));
        std::fs::write(&step, write(&b)).unwrap();
        let out = std::process::Command::new(cli).arg("run").arg("--in").arg(&step).arg("--out").arg(&obj).arg("--quality").arg("fine").arg("--quiet").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let m = crate::mesh_io::read_obj(&std::fs::read_to_string(&obj).unwrap(), &Default::default()).unwrap();
        let pi = std::f64::consts::PI;
        let holes = pi * 1.6 * 1.6 + (pi * 0.6 * 0.6 + 1.2 * 2.8) + (pi * 0.5 * 0.5 + 1.0 * 2.0);
        let want = (30.0 * 20.0 - holes) * BOARD_THICKNESS;
        assert!((m.volume() - want).abs() < 1e-3 * want, "{} vs {want}", m.volume());
    }

    /// Two boards stacked: the system STEP has a product per board, the second placed above the first; read back, the whole spans both.
    #[test]
    fn a_system_of_boards_is_written_with_each_board_where_it_stands() {
        let a = Board::new("main", 40.0, 30.0);
        let b = Board::new("sensor", 20.0, 10.0);
        let place = crate::stack::Place::new(1, false, [30.0, 5.0, 12.0]);
        let text = write_system("set", &[("main", &a, crate::stack::Place::default()), ("sensor", &b, place)]);
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE(").count(), 2);
        assert!(edges_pair_up(&text));
        let m = crate::step::read_step(text.as_bytes()).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        // the sensor board turned a quarter: 10 x 20 from (20, 5) to (30, 25), its bottom 12 mm up
        assert!((hi[2] - (12.0 + BOARD_THICKNESS)).abs() < 1e-3 && lo[2].abs() < 1e-3, "{lo:?} {hi:?}");
        assert!((hi[0] - 40.0).abs() < 1e-3 && (hi[1] - 30.0).abs() < 1e-3);
        let want = (40.0 * 30.0 + 20.0 * 10.0) * BOARD_THICKNESS;
        assert!((m.volume() - want).abs() < 1e-3 * want, "{} vs {want}", m.volume());
    }
}
