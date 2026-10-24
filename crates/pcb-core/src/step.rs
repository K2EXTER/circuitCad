//! STEP (ISO 10303-21: AP203 / AP214 / AP242 solids as makers publish them for their parts) read into a triangle mesh.
//!
//! What is read: the solids (`MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`, `SHELL_BASED_SURFACE_MODEL`) with their faces on planes,
//! cylinders, cones, spheres, tori and anything else (a flat patch through the outline), edges that are lines, circles,
//! ellipses and B-splines, the length unit and the colours (`STYLED_ITEM`). Not read: assembly placements (parts that are
//! moved inside the file), curved faces whose inside has no outline point (a full sphere), and anything but the shape.
//! Curved surfaces are cut along their outline only, so a dome or a fillet shows as a few flat facets.

use crate::mesh::Mesh3d;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

type V3 = [f64; 3];

/// Most triangles a part may have: a bigger file is a whole assembly, not a part.
const MAX_TRIANGLES: usize = 300_000;
/// A full circle is cut into this many straight pieces.
const CIRCLE_STEPS: usize = 24;

// ------------------------------------------------------------------ the file

#[derive(Clone, Debug)]
enum Val {
    Null,
    Ref(u32),
    Num(f64),
    Str,
    /// `.T.`, `.F.`, `.MILLI.`
    Enum(String),
    List(Vec<Val>),
    /// `LENGTH_MEASURE(25.4)`
    Typed(Vec<Val>),
}

/// One entity: a simple one has a single (name, arguments) part, a complex one `(A(..) B(..))` several.
type Parts = Vec<(String, Vec<Val>)>;

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"/*") {
                match self.s[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i += p + 4,
                    None => self.i = self.s.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&self) -> u8 {
        self.s.get(self.i).copied().unwrap_or(0)
    }

    fn word(&mut self) -> String {
        let st = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).to_ascii_uppercase()
    }

    fn value(&mut self) -> Result<Val, String> {
        self.ws();
        match self.peek() {
            b'#' => {
                self.i += 1;
                let n = self.word();
                n.parse().map(Val::Ref).map_err(|_| format!("bad reference #{n}"))
            }
            b'$' | b'*' => {
                self.i += 1;
                Ok(Val::Null)
            }
            b'\'' => {
                self.i += 1;
                let mut out = vec![];
                while self.i < self.s.len() {
                    if self.s[self.i] == b'\'' {
                        if self.s.get(self.i + 1) == Some(&b'\'') {
                            out.push(b'\'');
                            self.i += 2;
                            continue;
                        }
                        break;
                    }
                    out.push(self.s[self.i]);
                    self.i += 1;
                }
                self.i += 1;
                let _ = out;
                Ok(Val::Str)
            }
            b'.' if !self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                self.i += 1;
                let w = self.word();
                if self.peek() == b'.' {
                    self.i += 1;
                }
                Ok(Val::Enum(w))
            }
            b'(' => {
                self.i += 1;
                Ok(Val::List(self.items()?))
            }
            c if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => {
                let st = self.i;
                self.i += 1;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || matches!(self.s[self.i], b'.' | b'e' | b'E') || (matches!(self.s[self.i], b'-' | b'+') && matches!(self.s[self.i - 1], b'e' | b'E'))) {
                    self.i += 1;
                }
                let t = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
                t.parse().map(Val::Num).map_err(|_| format!("bad number {t}"))
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.word();
                self.ws();
                if self.peek() == b'(' {
                    self.i += 1;
                    Ok(Val::Typed(self.items()?))
                } else {
                    Ok(Val::Enum(name))
                }
            }
            c => Err(format!("unexpected '{}' at byte {}", c as char, self.i)),
        }
    }

    /// The values up to the closing parenthesis (the opening one is read).
    fn items(&mut self) -> Result<Vec<Val>, String> {
        let mut v = vec![];
        loop {
            self.ws();
            match self.peek() {
                b')' => {
                    self.i += 1;
                    return Ok(v);
                }
                b',' => self.i += 1,
                0 => return Err("the file ends inside an entity".into()),
                _ => v.push(self.value()?),
            }
        }
    }

    fn part(&mut self) -> Result<(String, Vec<Val>), String> {
        self.ws();
        let name = self.word();
        self.ws();
        if self.peek() != b'(' {
            return Err(format!("entity {name} has no arguments"));
        }
        self.i += 1;
        Ok((name, self.items()?))
    }
}

struct Step {
    ents: HashMap<u32, Parts>,
    /// How far a straight piece may stray from the curve it stands for, in the file's unit.
    tol: f64,
}

impl Step {
    fn parse(bytes: &[u8]) -> Result<Step, String> {
        let start = find(bytes, b"DATA;").ok_or("not a STEP file: no DATA section")? + 5;
        let mut r = Reader { s: bytes, i: start };
        let mut ents = HashMap::new();
        loop {
            r.ws();
            if r.i >= bytes.len() || bytes[r.i..].starts_with(b"ENDSEC") {
                break;
            }
            if r.peek() != b'#' {
                return Err(format!("expected an entity at byte {}", r.i));
            }
            r.i += 1;
            let id: u32 = r.word().parse().map_err(|_| "bad entity number".to_string())?;
            r.ws();
            if r.peek() != b'=' {
                return Err(format!("entity #{id} has no '='"));
            }
            r.i += 1;
            r.ws();
            let parts = if r.peek() == b'(' {
                r.i += 1;
                let mut ps = vec![];
                loop {
                    r.ws();
                    match r.peek() {
                        b')' => {
                            r.i += 1;
                            break;
                        }
                        0 => return Err("the file ends inside an entity".into()),
                        _ => ps.push(r.part()?),
                    }
                }
                ps
            } else {
                vec![r.part()?]
            };
            r.ws();
            if r.peek() == b';' {
                r.i += 1;
            }
            ents.insert(id, parts);
        }
        if ents.is_empty() {
            return Err("the STEP file has no entities".into());
        }
        let mut s = Step { ents, tol: 0.03 };
        s.tol = 0.03 / s.unit_mm();
        Ok(s)
    }

    fn part(&self, id: u32, name: &str) -> Option<&Vec<Val>> {
        self.ents.get(&id)?.iter().find(|p| p.0 == name).map(|p| &p.1)
    }

    fn name(&self, id: u32) -> &str {
        self.ents.get(&id).and_then(|p| p.first()).map_or("", |p| p.0.as_str())
    }

    fn args(&self, id: u32) -> &[Val] {
        self.ents.get(&id).and_then(|p| p.first()).map_or(&[], |p| p.1.as_slice())
    }

    fn point(&self, id: u32) -> Option<V3> {
        let a = self.part(id, "CARTESIAN_POINT")?;
        vec3(a.get(1)?)
    }

    fn direction(&self, id: u32) -> Option<V3> {
        let a = self.part(id, "DIRECTION")?;
        unit(vec3(a.get(1)?)?)
    }

    fn frame(&self, id: u32) -> Option<Frame> {
        let a = self.part(id, "AXIS2_PLACEMENT_3D")?;
        let o = self.point(r#ref(a.get(1)?)?)?;
        let z = a.get(2).and_then(r#ref).and_then(|d| self.direction(d)).unwrap_or([0.0, 0.0, 1.0]);
        let hint = a.get(3).and_then(r#ref).and_then(|d| self.direction(d));
        Some(Frame::new(o, z, hint))
    }

    /// Millimetres per unit of the file's length unit.
    fn unit_mm(&self) -> f64 {
        let mut length_units: Vec<&Parts> = self.ents.values().filter(|p| p.iter().any(|q| q.0 == "LENGTH_UNIT")).collect();
        length_units.sort_by_key(|p| p.iter().all(|q| q.0 != "CONVERSION_BASED_UNIT"));
        for parts in length_units {
            if let Some((_, a)) = parts.iter().find(|p| p.0 == "CONVERSION_BASED_UNIT") {
                // a conversion unit (the inch): its measure says how many of the base unit one of these is
                for v in a {
                    if let Some(m) = r#ref(v).and_then(|m| self.part(m, "LENGTH_MEASURE_WITH_UNIT"))
                        && let Some(Val::Typed(n)) = m.first()
                        && let Some(Val::Num(x)) = n.first()
                    {
                        let base_mm = m.get(1).and_then(r#ref).is_some_and(|u| self.ents.get(&u).is_some_and(|p| p.iter().any(|q| q.0 == "SI_UNIT" && q.1.iter().any(|w| matches!(w, Val::Enum(e) if e == "MILLI")))));
                        return if base_mm { *x } else { *x * 1000.0 };
                    }
                }
            }
            if let Some((_, a)) = parts.iter().find(|p| p.0 == "SI_UNIT") {
                let prefix = a.first().and_then(|v| if let Val::Enum(e) = v { Some(e.as_str()) } else { None });
                return match prefix {
                    Some("MILLI") => 1.0,
                    Some("CENTI") => 10.0,
                    Some("MICRO") => 0.001,
                    Some("KILO") => 1e6,
                    _ => 1000.0,
                };
            }
        }
        1.0
    }

    /// The colour of each styled item (a solid or a face): item number -> RGB.
    fn colours(&self) -> HashMap<u32, [u8; 3]> {
        let mut out = HashMap::new();
        for (id, parts) in &self.ents {
            let Some((_, a)) = parts.first().filter(|p| p.0 == "STYLED_ITEM" || p.0 == "OVER_RIDING_STYLED_ITEM") else { continue };
            let Some(item) = a.get(2).and_then(r#ref) else { continue };
            let _ = id;
            if let Some(c) = a.get(1).and_then(|s| self.find_colour(s, 0)) {
                out.insert(item, c);
            }
        }
        out
    }

    fn find_colour(&self, v: &Val, depth: u32) -> Option<[u8; 3]> {
        if depth > 12 {
            return None;
        }
        match v {
            Val::Ref(id) => {
                if let Some(a) = self.part(*id, "COLOUR_RGB") {
                    let c = |k: usize| if let Some(Val::Num(x)) = a.get(k) { (x.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 128 };
                    return Some([c(1), c(2), c(3)]);
                }
                self.args(*id).iter().find_map(|x| self.find_colour(x, depth + 1))
            }
            Val::List(l) | Val::Typed(l) => l.iter().find_map(|x| self.find_colour(x, depth + 1)),
            _ => None,
        }
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn r#ref(v: &Val) -> Option<u32> {
    if let Val::Ref(n) = v { Some(*n) } else { None }
}

fn num(v: Option<&Val>) -> Option<f64> {
    if let Some(Val::Num(x)) = v { Some(*x) } else { None }
}

fn vec3(v: &Val) -> Option<V3> {
    let Val::List(l) = v else { return None };
    Some([num(l.first())?, num(l.get(1))?, num(l.get(2))?])
}

fn is_true(v: Option<&Val>) -> bool {
    !matches!(v, Some(Val::Enum(e)) if e == "F")
}

// ------------------------------------------------------------------ vectors

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    (l > 1e-12).then(|| mul(a, 1.0 / l))
}

fn dist(a: V3, b: V3) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// An origin and three perpendicular unit vectors.
#[derive(Clone, Copy, Debug)]
struct Frame {
    o: V3,
    x: V3,
    y: V3,
    z: V3,
}

impl Frame {
    fn new(o: V3, z: V3, x_hint: Option<V3>) -> Frame {
        let z = unit(z).unwrap_or([0.0, 0.0, 1.0]);
        let h = x_hint.unwrap_or(if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] });
        // the reference direction, made perpendicular to the axis
        let x = unit(sub(h, mul(z, dot(h, z)))).unwrap_or_else(|| unit(cross(z, [0.0, 0.0, 1.0])).unwrap_or([1.0, 0.0, 0.0]));
        Frame { o, x, y: cross(z, x), z }
    }

    /// Place on the circle of this frame: `r` along x, `r2` along y, at angle `t`.
    fn at(&self, r: f64, r2: f64, t: f64) -> V3 {
        add(self.o, add(mul(self.x, r * t.cos()), mul(self.y, r2 * t.sin())))
    }

    fn rel(&self, p: V3) -> (f64, f64, f64) {
        let d = sub(p, self.o);
        (dot(d, self.x), dot(d, self.y), dot(d, self.z))
    }
}

// ------------------------------------------------------------------ edges

/// Points of a B-spline curve, `n` of them over its whole range.
fn spline_points(s: &Step, id: u32, n: usize) -> Option<Vec<V3>> {
    let parts = s.ents.get(&id)?;
    // a simple B_SPLINE_CURVE_WITH_KNOTS has the name first; the parts of a complex entity have none
    let (mut deg, mut ctrl, mut mults, mut knots, mut weights) = (0usize, vec![], vec![], vec![], vec![]);
    for (name, a) in parts {
        let off = usize::from(parts.len() == 1);
        match name.as_str() {
            "B_SPLINE_CURVE_WITH_KNOTS" | "B_SPLINE_CURVE" => {
                if let Some(Val::Num(d)) = a.get(off) {
                    deg = *d as usize;
                }
                if let Some(Val::List(l)) = a.get(off + 1) {
                    ctrl = l.iter().filter_map(|v| r#ref(v).and_then(|p| s.point(p))).collect::<Vec<_>>();
                }
                if name == "B_SPLINE_CURVE_WITH_KNOTS" {
                    let o2 = if parts.len() == 1 { off + 5 } else { 0 };
                    if let Some(Val::List(l)) = a.get(o2) {
                        mults = l.iter().filter_map(|v| num(Some(v))).map(|x| x as usize).collect::<Vec<_>>();
                    }
                    if let Some(Val::List(l)) = a.get(o2 + 1) {
                        knots = l.iter().filter_map(|v| num(Some(v))).collect::<Vec<_>>();
                    }
                }
            }
            "RATIONAL_B_SPLINE_CURVE" => {
                if let Some(Val::List(l)) = a.last() {
                    weights = l.iter().filter_map(|v| num(Some(v))).collect::<Vec<_>>();
                }
            }
            _ => {}
        }
    }
    if deg == 0 || ctrl.len() <= deg || mults.len() != knots.len() || knots.is_empty() {
        return None;
    }
    let kv: Vec<f64> = knots.iter().zip(&mults).flat_map(|(k, m)| std::iter::repeat_n(*k, *m)).collect();
    if kv.len() != ctrl.len() + deg + 1 {
        return None;
    }
    let w = |i: usize| weights.get(i).copied().unwrap_or(1.0);
    let (t0, t1) = (kv[deg], kv[ctrl.len()]);
    let eval = |t: f64| -> V3 {
        // the span: the last knot interval that starts at or before t
        let mut k = deg;
        while k + 1 < ctrl.len() && kv[k + 1] <= t {
            k += 1;
        }
        // de Boor on homogeneous points (x*w, y*w, z*w, w)
        let mut d: Vec<[f64; 4]> = (0..=deg).map(|j| {
            let i = k + j - deg;
            [ctrl[i][0] * w(i), ctrl[i][1] * w(i), ctrl[i][2] * w(i), w(i)]
        }).collect();
        for r in 1..=deg {
            for j in (r..=deg).rev() {
                let i = j + k - deg;
                let den = kv[i + deg + 1 - r] - kv[i];
                let a = if den.abs() < 1e-14 { 0.0 } else { (t - kv[i]) / den };
                let prev = d[j - 1];
                for (c, x) in d[j].iter_mut().enumerate() {
                    *x = (1.0 - a) * prev[c] + a * *x;
                }
            }
        }
        let h = d[deg];
        let ww = if h[3].abs() < 1e-14 { 1.0 } else { h[3] };
        [h[0] / ww, h[1] / ww, h[2] / ww]
    };
    Some((0..n).map(|i| eval(t0 + (t1 - t0) * i as f64 / (n - 1) as f64)).collect())
}

/// The polyline with the points that lie within `tol` of the line between their neighbours left out (Douglas-Peucker).
fn simplify(pts: &[V3], tol: f64) -> Vec<V3> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let ab = sub(b, a);
    let len2 = dot(ab, ab).max(1e-18);
    let (mut far, mut at) = (0.0, 0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let t = (dot(sub(*p, a), ab) / len2).clamp(0.0, 1.0);
        let d = dist(*p, add(a, mul(ab, t)));
        if d > far {
            (far, at) = (d, i);
        }
    }
    if far <= tol {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=at], tol);
    left.pop();
    left.extend(simplify(&pts[at..], tol));
    left
}

/// The angle of `p` about the frame's axis, and the sweep going from angle `a1` to `a2` (counter-clockwise when `ccw`).
fn sweep(a1: f64, a2: f64, ccw: bool, closed: bool) -> f64 {
    let d = if ccw { (a2 - a1).rem_euclid(TAU) } else { -((a1 - a2).rem_euclid(TAU)) };
    if closed && d.abs() < 1e-9 { if ccw { TAU } else { -TAU } } else { d }
}

impl Step {
    /// The points of an edge curve, in the direction of the edge (from its first vertex to its second).
    fn edge_points(&self, edge: u32) -> Option<Vec<V3>> {
        let a = self.part(edge, "EDGE_CURVE")?;
        let v = |k: usize| a.get(k).and_then(r#ref).and_then(|vp| self.part(vp, "VERTEX_POINT")).and_then(|x| x.get(1)).and_then(r#ref).and_then(|p| self.point(p));
        let (p1, p2) = (v(1)?, v(2)?);
        let same = is_true(a.get(4));
        let closed = dist(p1, p2) < 1e-9;
        let mut curve = a.get(3).and_then(r#ref)?;
        // a curve on a surface: the 3D curve is its first argument
        while matches!(self.name(curve), "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE") {
            curve = self.args(curve).get(1).and_then(r#ref)?;
        }
        match self.name(curve) {
            "CIRCLE" | "ELLIPSE" => {
                let c = self.args(curve);
                let f = self.frame(c.get(1).and_then(r#ref)?)?;
                let (r1, r2) = if self.name(curve) == "CIRCLE" { let r = num(c.get(2))?; (r, r) } else { (num(c.get(2))?, num(c.get(3))?) };
                let ang = |p: V3| {
                    let (x, y, _) = f.rel(p);
                    (y / r2).atan2(x / r1)
                };
                let d = sweep(ang(p1), ang(p2), same, closed);
                let full = ((PI / (1.0 - (self.tol / r1.max(r2)).min(0.5)).acos()).ceil() as usize).clamp(8, 4 * CIRCLE_STEPS);
                let n = ((d.abs() / TAU * full as f64).ceil() as usize).max(1);
                let a1 = ang(p1);
                let mut pts: Vec<V3> = (0..=n).map(|i| f.at(r1, r2, a1 + d * i as f64 / n as f64)).collect();
                pts[0] = p1;
                pts[n] = p2;
                Some(pts)
            }
            n if n.starts_with("B_SPLINE_CURVE") || self.ents.get(&curve).is_some_and(|p| p.iter().any(|q| q.0.starts_with("B_SPLINE_CURVE"))) => {
                let _ = n;
                let all = spline_points(self, curve, 33)?;
                let near = |p: V3| (0..all.len()).min_by(|&i, &j| dist(all[i], p).total_cmp(&dist(all[j], p))).unwrap_or(0);
                let (i1, i2) = (near(p1), near(p2));
                let mut pts: Vec<V3> = if same {
                    if i1 <= i2 { all[i1..=i2].to_vec() } else { all[i1..].iter().chain(all[..=i2].iter()).copied().collect() }
                } else if i1 >= i2 {
                    all[i2..=i1].iter().rev().copied().collect()
                } else {
                    all[..=i1].iter().rev().chain(all[i2..].iter().rev()).copied().collect()
                };
                if closed && pts.len() < 3 {
                    pts = if same { all.clone() } else { all.iter().rev().copied().collect() };
                }
                let last = pts.len() - 1;
                pts[0] = p1;
                pts[last] = p2;
                Some(if closed { pts } else { simplify(&pts, self.tol) })
            }
            _ => Some(vec![p1, p2]),
        }
    }

    /// The points around an `EDGE_LOOP`, without the repeated start.
    fn loop_points(&self, lp: u32, reversed: bool) -> Vec<V3> {
        let mut out: Vec<V3> = vec![];
        let Some(Val::List(edges)) = self.args(lp).get(1) else { return out };
        for e in edges {
            let Some(oe) = r#ref(e) else { continue };
            let a = self.args(oe);
            let Some(edge) = a.get(3).and_then(r#ref) else { continue };
            let Some(mut pts) = self.edge_points(edge) else { continue };
            if !is_true(a.get(4)) {
                pts.reverse();
            }
            for p in pts {
                if out.last().is_none_or(|l| dist(*l, p) > 1e-9) {
                    out.push(p);
                }
            }
        }
        while out.len() > 1 && dist(out[0], out[out.len() - 1]) < 1e-9 {
            out.pop();
        }
        if reversed {
            out.reverse();
        }
        out
    }
}

// ------------------------------------------------------------------ faces

#[derive(Clone, Copy, Debug)]
enum Surf {
    Plane(Frame),
    Cylinder(Frame, f64),
    Cone(Frame, f64),
    Sphere(Frame, f64),
    Torus(Frame, f64, f64),
    Other,
}

impl Step {
    fn surface(&self, id: u32) -> Surf {
        let a = self.args(id);
        let fr = || a.get(1).and_then(r#ref).and_then(|f| self.frame(f));
        match (self.name(id), fr()) {
            ("PLANE", Some(f)) => Surf::Plane(f),
            ("CYLINDRICAL_SURFACE", Some(f)) => num(a.get(2)).map_or(Surf::Other, |r| Surf::Cylinder(f, r)),
            ("CONICAL_SURFACE", Some(f)) => Surf::Cone(f, num(a.get(2)).unwrap_or(0.0)),
            ("SPHERICAL_SURFACE", Some(f)) => num(a.get(2)).map_or(Surf::Other, |r| Surf::Sphere(f, r)),
            ("TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE", Some(f)) => match (num(a.get(2)), num(a.get(3))) {
                (Some(big), Some(small)) => Surf::Torus(f, big, small),
                _ => Surf::Other,
            },
            _ => Surf::Other,
        }
    }
}

/// The loops of a face as flat (u, v) outlines, with the 3D points they came from.
fn flatten(surf: Surf, loops: &[Vec<V3>]) -> Vec<Vec<[f64; 2]>> {
    // a normal for the faces that have no simple parameters: the outline's own plane
    let plane_frame = || {
        let l = &loops[0];
        let mut n = [0.0; 3];
        for i in 0..l.len() {
            n = add(n, cross(l[i], l[(i + 1) % l.len()]));
        }
        Frame::new(l[0], n, None)
    };
    let angular = |f: &Frame, second: &dyn Fn(f64, f64, f64) -> (f64, f64)| -> Vec<Vec<[f64; 2]>> {
        // each loop: angle about the axis (continued without jumps), height, distance from the axis
        let mut turns: Vec<Vec<(f64, f64, f64)>> = loops
            .iter()
            .map(|l| {
                let mut prev = 0.0;
                l.iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let (x, y, z) = f.rel(*p);
                        let mut t = y.atan2(x);
                        if i > 0 {
                            while t - prev > PI {
                                t -= TAU;
                            }
                            while t - prev < -PI {
                                t += TAU;
                            }
                        }
                        prev = t;
                        (t, z, (x * x + y * y).sqrt())
                    })
                    .collect()
            })
            .collect();
        // a hole belongs to the same turn of the surface as the outline
        let mean = |l: &Vec<(f64, f64, f64)>| l.iter().map(|p| p.0).sum::<f64>() / l.len() as f64;
        let centre = turns.first().map_or(0.0, mean);
        for l in turns.iter_mut().skip(1) {
            let k = ((centre - mean(l)) / TAU).round();
            for p in l.iter_mut() {
                p.0 += k * TAU;
            }
        }
        turns.iter().map(|l| l.iter().map(|&(t, z, rho)| { let (u, v) = second(t, z, rho); [u, v] }).collect()).collect()
    };
    match surf {
        Surf::Plane(f) => loops.iter().map(|l| l.iter().map(|p| { let (x, y, _) = f.rel(*p); [x, y] }).collect()).collect(),
        Surf::Cylinder(f, r) => angular(&f, &|t, z, _| (t * r, z)),
        Surf::Cone(f, r) => angular(&f, &|t, z, rho| (t * r.max(rho).max(1e-3), z)),
        Surf::Sphere(f, r) => angular(&f, &|t, z, _| (t * r, (z / r).clamp(-1.0, 1.0).asin() * r)),
        Surf::Torus(f, big, small) => angular(&f, &|t, z, rho| (t * big, (z).atan2(rho - big) * small)),
        Surf::Other => {
            let f = plane_frame();
            loops.iter().map(|l| l.iter().map(|p| { let (x, y, _) = f.rel(*p); [x, y] }).collect()).collect()
        }
    }
}

/// Triangles of a polygon with holes, as corner indices into the concatenation of `loops`.
fn triangulate(loops: &[Vec<[f64; 2]>]) -> Vec<[usize; 3]> {
    use i_overlay::core::fill_rule::FillRule;
    use i_triangle::float::custom::CustomTriangulatable;
    use i_triangle::int::validation::Validation;
    let flat: Vec<[f64; 2]> = loops.iter().flatten().copied().collect();
    if flat.len() < 3 {
        return vec![];
    }
    let contours: Vec<Vec<[f64; 2]>> = loops.iter().filter(|l| l.len() >= 3).cloned().collect();
    if contours.is_empty() {
        return vec![];
    }
    // every outline point is kept (a curved face is a rectangle with many points on its sides)
    let mut validation = Validation::<i32>::with_fill_rule(FillRule::EvenOdd);
    validation.options.preserve_input_collinear = true;
    validation.options.preserve_output_collinear = true;
    let t = contours.as_slice().custom_triangulate(validation).to_triangulation::<u32>();
    // the triangulation hands back the points as numbers: find each one among the input points
    let which: Vec<usize> = t
        .points
        .iter()
        .map(|p| (0..flat.len()).min_by(|&i, &j| d2(flat[i], *p).total_cmp(&d2(flat[j], *p))).unwrap_or(0))
        .collect();
    t.indices.as_chunks::<3>().0.iter().map(|i| [which[i[0] as usize], which[i[1] as usize], which[i[2] as usize]]).collect()
}

fn d2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

struct Builder {
    mesh: Mesh3d,
}

impl Builder {
    fn colour(&mut self, c: [u8; 3]) -> u8 {
        match self.mesh.palette.iter().position(|p| *p == c) {
            Some(i) => i as u8,
            None if self.mesh.palette.len() < 255 => {
                self.mesh.palette.push(c);
                (self.mesh.palette.len() - 1) as u8
            }
            None => 0,
        }
    }
}

impl Step {
    /// Adds the triangles of one `ADVANCED_FACE`.
    fn add_face(&self, face: u32, colour: [u8; 3], scale: f64, b: &mut Builder) {
        let a = self.args(face);
        let Some(Val::List(bounds)) = a.get(1) else { return };
        let Some(sid) = a.get(2).and_then(r#ref) else { return };
        let surf = self.surface(sid);
        let same = is_true(a.get(3));
        let mut outer = None;
        let mut loops: Vec<Vec<V3>> = vec![];
        for bd in bounds {
            let Some(bid) = r#ref(bd) else { continue };
            let ba = self.args(bid);
            let Some(lp) = ba.get(1).and_then(r#ref) else { continue };
            let pts = self.loop_points(lp, !is_true(ba.get(2)));
            if pts.len() < 3 {
                continue;
            }
            if self.name(bid) == "FACE_OUTER_BOUND" {
                outer = Some(loops.len());
            }
            loops.push(pts);
        }
        if loops.is_empty() {
            return;
        }
        if let Some(k) = outer {
            loops.swap(0, k);
        }
        let uv = flatten(surf, &loops);
        let flat_pts: Vec<V3> = loops.iter().flatten().copied().collect();
        let tris = triangulate(&uv);
        if tris.is_empty() {
            return;
        }
        let ci = b.colour(colour);
        let base = b.mesh.vertices.len() as u32;
        b.mesh.vertices.extend(flat_pts.iter().map(|p| mul(*p, scale).map(|x| x as f32)));
        let sign = if same { 1.0 } else { -1.0 };
        for t in tris {
            let (pa, pb, pc) = (flat_pts[t[0]], flat_pts[t[1]], flat_pts[t[2]]);
            let n = cross(sub(pb, pa), sub(pc, pa));
            let centre = mul(add(add(pa, pb), pc), 1.0 / 3.0);
            let out = match surf {
                Surf::Plane(f) => Some(mul(f.z, sign)),
                Surf::Cylinder(f, _) | Surf::Cone(f, _) => {
                    let (x, y, _) = f.rel(centre);
                    Some(mul(add(mul(f.x, x), mul(f.y, y)), sign))
                }
                Surf::Sphere(f, _) => Some(mul(sub(centre, f.o), sign)),
                Surf::Torus(f, big, _) => {
                    let (x, y, _) = f.rel(centre);
                    let rho = (x * x + y * y).sqrt().max(1e-12);
                    let ring = add(f.o, add(mul(f.x, x / rho * big), mul(f.y, y / rho * big)));
                    Some(mul(sub(centre, ring), sign))
                }
                Surf::Other => None,
            };
            let flip = out.is_some_and(|o| dot(n, o) < 0.0);
            let (i, j, k) = (base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
            b.mesh.triangles.push(if flip { [i, k, j] } else { [i, j, k] });
            b.mesh.tri_color.push(ci);
        }
    }
}

/// Reads a STEP file's solids into a mesh (millimetres, in the file's own frame).
pub fn read_step(data: &[u8]) -> Result<Mesh3d, String> {
    let s = Step::parse(data)?;
    let scale = s.unit_mm();
    let colours = s.colours();
    let mut b = Builder { mesh: Mesh3d::default() };
    let mut ids: Vec<u32> = s.ents.keys().copied().collect();
    ids.sort_unstable();
    let default = [140u8, 140, 140];
    let mut solids = 0;
    for id in ids {
        let shells: Vec<u32> = match s.name(id) {
            "MANIFOLD_SOLID_BREP" => s.args(id).get(1).and_then(r#ref).into_iter().collect(),
            "BREP_WITH_VOIDS" => s.args(id).get(1).and_then(r#ref).into_iter().collect(),
            "SHELL_BASED_SURFACE_MODEL" => match s.args(id).get(1) {
                Some(Val::List(l)) => l.iter().filter_map(r#ref).collect(),
                _ => vec![],
            },
            _ => continue,
        };
        let solid_colour = colours.get(&id).copied();
        for sh in shells {
            let Some(Val::List(faces)) = s.args(sh).get(1) else { continue };
            for f in faces.iter().filter_map(r#ref) {
                if !matches!(s.name(f), "ADVANCED_FACE" | "FACE_SURFACE") {
                    continue;
                }
                let c = colours.get(&f).copied().or(solid_colour).unwrap_or(default);
                s.add_face(f, c, scale, &mut b);
                if b.mesh.triangles.len() > MAX_TRIANGLES {
                    return Err(format!("このSTEPは大きすぎます (三角形が {MAX_TRIANGLES} を超えました)。部品1個のデータか確かめてください"));
                }
            }
        }
        solids += 1;
    }
    if solids == 0 {
        return Err("STEPの中に、立体 (MANIFOLD_SOLID_BREP など) が見つかりません。曲面だけのデータや、形を持たないファイルかもしれません".into());
    }
    if b.mesh.triangles.is_empty() {
        return Err("STEPの立体から三角形を作れませんでした".into());
    }
    Ok(b.mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(t: &str) -> Result<Mesh3d, String> {
        read_step(t.as_bytes())
    }

    /// A box 0..w x 0..d x 0..h as an AP214-style solid with six planar faces and twelve line edges.
    fn box_step(w: f64, d: f64, h: f64, extra: &str) -> String {
        let mut s = String::from("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('t'),'2;1');\nENDSEC;\nDATA;\n");
        let corners = [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, d, 0.0], [0.0, d, 0.0], [0.0, 0.0, h], [w, 0.0, h], [w, d, h], [0.0, d, h]];
        let mut n = 100;
        let mut line = |s: &mut String, f: &str| {
            n += 1;
            s.push_str(&format!("#{n}={f};\n"));
            n
        };
        let pts: Vec<usize> = corners.iter().map(|c| line(&mut s, &format!("CARTESIAN_POINT('',({:.6},{:.6},{:.6}))", c[0], c[1], c[2]))).collect();
        let vps: Vec<usize> = pts.iter().map(|p| line(&mut s, &format!("VERTEX_POINT('',#{p})"))).collect();
        let dirs: Vec<usize> = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].iter().map(|d| line(&mut s, &format!("DIRECTION('',({},{},{}))", d[0], d[1], d[2]))).collect();
        let mut edge = std::collections::HashMap::new();
        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
            let (pa, pb) = (corners[a], corners[b]);
            let axis = (0..3).find(|&k| (pa[k] - pb[k]).abs() > 1e-9).unwrap();
            let len = (pb[axis] - pa[axis]).abs();
            let sign = if pb[axis] > pa[axis] { 1.0 } else { -1.0 };
            let dir = if sign > 0.0 { dirs[axis] } else { line(&mut s, &format!("DIRECTION('',({},{},{}))", if axis == 0 { -1 } else { 0 }, if axis == 1 { -1 } else { 0 }, if axis == 2 { -1 } else { 0 })) };
            let vec = line(&mut s, &format!("VECTOR('',#{dir},{len:.6})"));
            let ln = line(&mut s, &format!("LINE('',#{},#{vec})", pts[a]));
            let e = line(&mut s, &format!("EDGE_CURVE('',#{},#{},#{ln},.T.)", vps[a], vps[b]));
            edge.insert((a, b), (e, true));
            edge.insert((b, a), (e, false));
        }
        // faces: corners in counter-clockwise order seen from outside, and the outward normal
        let faces: [([usize; 4], [f64; 3]); 6] = [
            ([0, 3, 2, 1], [0.0, 0.0, -1.0]),
            ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
            ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
            ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
            ([2, 3, 7, 6], [0.0, 1.0, 0.0]),
            ([3, 0, 4, 7], [-1.0, 0.0, 0.0]),
        ];
        let mut face_ids = vec![];
        for (c, nrm) in faces {
            let oes: Vec<usize> = (0..4).map(|k| {
                let (e, fwd) = edge[&(c[k], c[(k + 1) % 4])];
                line(&mut s, &format!("ORIENTED_EDGE('',*,*,#{e},{})", if fwd { ".T." } else { ".F." }))
            }).collect();
            let lp = line(&mut s, &format!("EDGE_LOOP('',({}))", oes.iter().map(|o| format!("#{o}")).collect::<Vec<_>>().join(",")));
            let fb = line(&mut s, &format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
            let o = line(&mut s, &format!("CARTESIAN_POINT('',({:.6},{:.6},{:.6}))", corners[c[0]][0], corners[c[0]][1], corners[c[0]][2]));
            let nd = line(&mut s, &format!("DIRECTION('',({},{},{}))", nrm[0], nrm[1], nrm[2]));
            let ax = line(&mut s, &format!("AXIS2_PLACEMENT_3D('',#{o},#{nd},$)"));
            let pl = line(&mut s, &format!("PLANE('',#{ax})"));
            face_ids.push(line(&mut s, &format!("ADVANCED_FACE('',(#{fb}),#{pl},.T.)")));
        }
        let shell = line(&mut s, &format!("CLOSED_SHELL('',({}))", face_ids.iter().map(|f| format!("#{f}")).collect::<Vec<_>>().join(",")));
        let solid = line(&mut s, &format!("MANIFOLD_SOLID_BREP('body',#{shell})"));
        s.push_str(&extra.replace("@SOLID", &format!("#{solid}")));
        s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        s
    }

    #[test]
    fn a_box_becomes_twelve_triangles_with_the_right_volume() {
        let m = rs(&box_step(10.0, 6.0, 3.0, "")).unwrap();
        assert_eq!(m.triangles.len(), 12);
        assert!((m.volume() - 180.0).abs() < 1e-3, "volume {}", m.volume());
        let (lo, hi) = m.bounds().unwrap();
        assert_eq!(lo.map(|x| x.round()), [0.0, 0.0, 0.0]);
        assert_eq!(hi.map(|x| x.round()), [10.0, 6.0, 3.0]);
    }

    #[test]
    fn the_length_unit_scales_to_millimetres() {
        let inch = "#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#901=(CONVERSION_BASED_UNIT('INCH',#902)LENGTH_UNIT()NAMED_UNIT(*));\n#902=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#900);\n";
        let m = rs(&box_step(1.0, 1.0, 1.0, inch)).unwrap();
        assert!((m.volume() - 25.4f64.powi(3)).abs() < 0.5, "volume {}", m.volume());
        let m = rs(&box_step(1.0, 1.0, 1.0, "#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\n")).unwrap();
        assert!((m.volume() - 1e9).abs() < 1e3, "metres: {}", m.volume());
    }

    #[test]
    fn a_styled_colour_reaches_the_triangles() {
        let style = "#800=COLOUR_RGB('',1.0,0.0,0.0);\n#801=FILL_AREA_STYLE_COLOUR('',#800);\n#802=FILL_AREA_STYLE('',(#801));\n#803=SURFACE_STYLE_FILL_AREA(#802);\n#804=SURFACE_SIDE_STYLE('',(#803));\n#805=SURFACE_STYLE_USAGE(.BOTH.,#804);\n#806=PRESENTATION_STYLE_ASSIGNMENT((#805));\n#807=STYLED_ITEM('',(#806),@SOLID);\n";
        let m = rs(&box_step(2.0, 2.0, 2.0, style)).unwrap();
        assert_eq!(m.palette, vec![[255, 0, 0]]);
    }

    /// A cylinder of radius 2 and height 5 on the z axis: a round cap each end and a side face cut along a seam.
    fn cylinder_step() -> String {
        "ISO-10303-21;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(0.,0.,5.));\n#3=DIRECTION('',(0.,0.,1.));\n#4=DIRECTION('',(1.,0.,0.));\n\
#5=AXIS2_PLACEMENT_3D('',#1,#3,#4);\n#6=AXIS2_PLACEMENT_3D('',#2,#3,#4);\n#7=CIRCLE('',#5,2.);\n#8=CIRCLE('',#6,2.);\n\
#9=CARTESIAN_POINT('',(2.,0.,0.));\n#10=CARTESIAN_POINT('',(2.,0.,5.));\n#11=VERTEX_POINT('',#9);\n#12=VERTEX_POINT('',#10);\n\
#13=DIRECTION('',(0.,0.,1.));\n#14=VECTOR('',#13,5.);\n#15=LINE('',#9,#14);\n\
#16=EDGE_CURVE('',#11,#11,#7,.T.);\n#17=EDGE_CURVE('',#12,#12,#8,.T.);\n#18=EDGE_CURVE('',#11,#12,#15,.T.);\n\
#20=ORIENTED_EDGE('',*,*,#16,.F.);\n#21=EDGE_LOOP('',(#20));\n#22=FACE_OUTER_BOUND('',#21,.T.);\n#23=PLANE('',#5);\n#24=ADVANCED_FACE('',(#22),#23,.F.);\n\
#30=ORIENTED_EDGE('',*,*,#17,.T.);\n#31=EDGE_LOOP('',(#30));\n#32=FACE_OUTER_BOUND('',#31,.T.);\n#33=PLANE('',#6);\n#34=ADVANCED_FACE('',(#32),#33,.T.);\n\
#40=ORIENTED_EDGE('',*,*,#16,.T.);\n#41=ORIENTED_EDGE('',*,*,#18,.T.);\n#42=ORIENTED_EDGE('',*,*,#17,.F.);\n#43=ORIENTED_EDGE('',*,*,#18,.F.);\n\
#44=EDGE_LOOP('',(#40,#41,#42,#43));\n#45=FACE_OUTER_BOUND('',#44,.T.);\n#46=CYLINDRICAL_SURFACE('',#5,2.);\n#47=ADVANCED_FACE('',(#45),#46,.T.);\n\
#50=CLOSED_SHELL('',(#24,#34,#47));\n#51=MANIFOLD_SOLID_BREP('',#50);\nENDSEC;\n"
            .into()
    }

    #[test]
    fn a_cylinder_is_a_closed_outward_solid() {
        let m = rs(&cylinder_step()).unwrap();
        let ideal = PI * 4.0 * 5.0;
        // a 24-sided prism holds a bit less than the true cylinder
        assert!(m.volume() > 0.0 && (m.volume() / ideal) > 0.97 && (m.volume() / ideal) < 1.0, "volume {} of {ideal}", m.volume());
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi[2] - lo[2] - 5.0).abs() < 1e-6 && (hi[0] - 2.0).abs() < 1e-6 && (lo[0] + 2.0).abs() < 0.1);
    }

    #[test]
    fn a_hole_in_a_plane_stays_open() {
        // the top of a box with a square hole: outer loop and an inner one, 4 + 4 corners
        let mut s = String::from("DATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=AXIS2_PLACEMENT_3D('',#1,#2,$);\n#4=PLANE('',#3);\n");
        let mut id = 10;
        let mut ring = |s: &mut String, pts: [[f64; 2]; 4]| -> usize {
            let vps: Vec<usize> = pts.iter().map(|p| {
                s.push_str(&format!("#{id}=CARTESIAN_POINT('',({},{},0.));\n#{}=VERTEX_POINT('',#{id});\n", p[0], p[1], id + 1));
                id += 2;
                id - 1
            }).collect();
            let oes: Vec<usize> = (0..4).map(|k| {
                let (a, b) = (k, (k + 1) % 4);
                s.push_str(&format!("#{id}=EDGE_CURVE('',#{},#{},#{id}9,.T.);\n#{}=ORIENTED_EDGE('',*,*,#{id},.T.);\n", vps[a], vps[b], id + 1));
                id += 2;
                id - 1
            }).collect();
            s.push_str(&format!("#{id}=EDGE_LOOP('',({}));\n", oes.iter().map(|o| format!("#{o}")).collect::<Vec<_>>().join(",")));
            id += 1;
            id - 1
        };
        // the edges are straight: an unknown curve entity falls back to the vertex-to-vertex line
        let outer = ring(&mut s, [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let inner = ring(&mut s, [[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]]);
        s.push_str(&format!("#900=FACE_OUTER_BOUND('',#{outer},.T.);\n#901=FACE_BOUND('',#{inner},.T.);\n#902=ADVANCED_FACE('',(#900,#901),#4,.T.);\n#903=CLOSED_SHELL('',(#902));\n#904=MANIFOLD_SOLID_BREP('',#903);\nENDSEC;\n"));
        let m = rs(&s).unwrap();
        let area: f64 = m
            .triangles
            .iter()
            .map(|t| {
                let p = |k: usize| m.vertices[t[k] as usize].map(f64::from);
                let (a, b, c) = (p(0), p(1), p(2));
                0.5 * dot(cross(sub(b, a), sub(c, a)), [0.0, 0.0, 1.0])
            })
            .sum();
        assert!((area - 96.0).abs() < 1e-3, "area {area}");
    }

    #[test]
    fn bad_files_give_a_reason() {
        assert!(rs("hello").unwrap_err().contains("DATA"));
        assert!(rs("DATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;").unwrap_err().contains("立体"));
    }
}


#[cfg(test)]
mod real_file {
    /// Reads the file named by the environment variable CCAD_STEP (for trying a maker's file by hand).
    #[test]
    #[ignore]
    fn read_the_file_in_ccad_step() {
        let Ok(p) = std::env::var("CCAD_STEP") else { return };
        let text = std::fs::read(&p).unwrap();
        let t0 = std::time::Instant::now();
        let m = super::read_step(&text).unwrap();
        println!("triangles {} vertices {} colours {:?} bounds {:?} volume {:.2} in {:?}", m.triangles.len(), m.vertices.len(), m.palette, m.bounds(), m.volume(), t0.elapsed());
    }
}

