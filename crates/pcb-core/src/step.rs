//! STEP (ISO 10303-21: AP203 / AP214 / AP242 solids as makers publish them for their parts) read into a triangle mesh.
//!
//! What is read: the solids (`MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`, `SHELL_BASED_SURFACE_MODEL`) with their faces on planes,
//! cylinders, cones, spheres, tori and anything else (a flat patch through the outline), edges that are lines, circles,
//! ellipses and B-splines, the length unit, the colours (`STYLED_ITEM`) and the placements of an assembly (a lead that the
//! file defines once and places eight times: `MAPPED_ITEM`, or `ITEM_DEFINED_TRANSFORMATION` between two representations).
//! Not read: curved faces whose inside has no outline point (a full sphere), and anything but the shape.
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
    /// A number written without a decimal point (a degree, a count): written back the same way.
    Int(i64),
    Str,
    /// `.T.`, `.F.`, `.MILLI.`
    Enum(String),
    List(Vec<Val>),
    /// `LENGTH_MEASURE(25.4)`: the type's name and its values
    Typed(String, Vec<Val>),
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
                if !t.contains(['.', 'e', 'E'])
                    && let Ok(i) = t.parse::<i64>()
                {
                    return Ok(Val::Int(i));
                }
                t.parse().map(Val::Num).map_err(|_| format!("bad number {t}"))
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.word();
                self.ws();
                if self.peek() == b'(' {
                    self.i += 1;
                    Ok(Val::Typed(name, self.items()?))
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

    /// The items (solids, mapped items) a representation holds.
    fn rep_items(&self, rep: u32) -> Vec<u32> {
        let Some(parts) = self.ents.get(&rep) else { return vec![] };
        parts
            .iter()
            .filter(|p| p.0.ends_with("REPRESENTATION"))
            .find_map(|p| match p.1.get(1) {
                Some(Val::List(l)) => Some(l.iter().filter_map(r#ref).collect()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Every instance of every solid: the placements that carry it from its own frame to the file's. An assembly defines a
    /// lead once and places it again and again, by `MAPPED_ITEM` or by a relationship with a transformation between two
    /// representations. Empty when the file has no placements (every solid is then where its coordinates say).
    fn placements(&self) -> HashMap<u32, Vec<Chain>> {
        let mut children: HashMap<u32, Vec<(u32, (Frame, Frame))>> = HashMap::new();
        let mut is_child = std::collections::HashSet::new();
        let mut ids: Vec<u32> = self.ents.keys().copied().collect();
        ids.sort_unstable();
        // product definition (or assembly usage) -> its shape representation
        let mut def_rep: HashMap<u32, u32> = HashMap::new();
        for &id in &ids {
            if let Some(a) = self.part(id, "SHAPE_DEFINITION_REPRESENTATION")
                && let (Some(shape), Some(rep)) = (a.first().and_then(r#ref), a.get(1).and_then(r#ref))
                && let Some(def) = self.args(shape).get(2).and_then(r#ref)
            {
                def_rep.insert(def, rep);
            }
        }
        // the relationship of two representations -> the assembly usage it belongs to
        let mut usage: HashMap<u32, u32> = HashMap::new();
        for &id in &ids {
            if let Some(a) = self.part(id, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
                && let (Some(rel), Some(pds)) = (a.first().and_then(r#ref), a.get(1).and_then(r#ref))
                && let Some(nauo) = self.args(pds).get(2).and_then(r#ref)
            {
                usage.insert(rel, nauo);
            }
        }
        for &id in &ids {
            // simple: REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(name, description, rep1, rep2, transformation);
            // complex: (REPRESENTATION_RELATIONSHIP(name, description, rep1, rep2) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(transformation) ...)
            let (reps, tf) = if let Some(a) = self.part(id, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION").filter(|a| a.len() >= 5) {
                ((a.get(2).and_then(r#ref), a.get(3).and_then(r#ref)), a.get(4).and_then(r#ref))
            } else if let (Some(r), Some(t)) = (self.part(id, "REPRESENTATION_RELATIONSHIP"), self.part(id, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")) {
                ((r.get(2).and_then(r#ref), r.get(3).and_then(r#ref)), t.first().and_then(r#ref))
            } else {
                continue;
            };
            let (Some(r1), Some(r2), Some(tf)) = (reps.0, reps.1, tf) else { continue };
            let Some(t) = self.part(tf, "ITEM_DEFINED_TRANSFORMATION") else { continue };
            let (Some(a), Some(b)) = (t.get(2).and_then(r#ref).and_then(|f| self.frame(f)), t.get(3).and_then(r#ref).and_then(|f| self.frame(f))) else { continue };
            // which side is the part: the assembly usage says (relating = the assembly, related = the part); else rep1 is the part
            let by_usage = usage.get(&id).and_then(|n| {
                let u = self.args(*n);
                Some((def_rep.get(&u.get(4).and_then(r#ref)?).copied()?, def_rep.get(&u.get(3).and_then(r#ref)?).copied()?))
            });
            let (child, parent, xf) = match by_usage {
                Some((c, p)) if c == r2 && p == r1 => (r2, r1, (b, a)),
                _ => (r1, r2, (a, b)),
            };
            children.entry(parent).or_default().push((child, xf));
            is_child.insert(child);
        }
        // a mapped item inside a representation: the map's origin is moved onto the item's target
        let mut owner: HashMap<u32, u32> = HashMap::new();
        for &id in &ids {
            for item in self.rep_items(id) {
                owner.insert(item, id);
            }
        }
        for &id in &ids {
            let Some(a) = self.part(id, "MAPPED_ITEM") else { continue };
            let (Some(map), Some(target)) = (a.get(1).and_then(r#ref), a.get(2).and_then(r#ref)) else { continue };
            let m = self.args(map);
            let (Some(origin), Some(rep)) = (m.first().and_then(r#ref).and_then(|f| self.frame(f)), m.get(1).and_then(r#ref)) else { continue };
            let (Some(target), Some(&parent)) = (self.frame(target), owner.get(&id)) else { continue };
            children.entry(parent).or_default().push((rep, (origin, target)));
            is_child.insert(rep);
        }
        if children.is_empty() {
            return HashMap::new();
        }
        // from every assembly nobody places: its solids, and the representations placed in it (a representation that places
        // nothing and is placed by nobody is a copy of a part, not the assembly: a solid is drawn where the assembly puts it)
        let mut out: HashMap<u32, Vec<Chain>> = HashMap::new();
        let mut budget = 20_000usize;
        let mut stack: Vec<(u32, Chain)> = ids.iter().filter(|id| !is_child.contains(id) && children.contains_key(id)).map(|&id| (id, vec![])).collect();
        stack.reverse();
        while let Some((rep, chain)) = stack.pop() {
            budget = budget.saturating_sub(1);
            if budget == 0 || chain.len() > 12 {
                continue;
            }
            for item in self.rep_items(rep) {
                if matches!(self.name(item), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "SHELL_BASED_SURFACE_MODEL") {
                    out.entry(item).or_default().push(chain.clone());
                }
            }
            for (child, xf) in children.get(&rep).into_iter().flatten() {
                // the part's own placement first, then the placements of the assembly it stands in
                let mut c = vec![*xf];
                c.extend(chain.iter().copied());
                stack.push((*child, c));
            }
        }
        out
    }

    /// Millimetres per unit of the file's length unit.
    fn unit_mm(&self) -> f64 {
        // the units the file's geometry context names come first (a file can hold other length units too, a metre for a
        // tolerance); then the order is the entity number, so the answer does not depend on the order of a hash map
        let in_context: std::collections::HashSet<u32> = self
            .ents
            .values()
            .flat_map(|p| p.iter())
            .filter(|q| q.0 == "GLOBAL_UNIT_ASSIGNED_CONTEXT")
            .flat_map(|q| q.1.iter())
            .flat_map(|v| if let Val::List(l) = v { l.iter().filter_map(r#ref).collect::<Vec<_>>() } else { vec![] })
            .collect();
        let mut length_units: Vec<(&u32, &Parts)> = self.ents.iter().filter(|(_, p)| p.iter().any(|q| q.0 == "LENGTH_UNIT")).collect();
        length_units.sort_by_key(|(id, p)| (!in_context.contains(*id), p.iter().all(|q| q.0 != "CONVERSION_BASED_UNIT"), **id));
        let length_units: Vec<&Parts> = length_units.into_iter().map(|(_, p)| p).collect();
        for parts in length_units {
            if let Some((_, a)) = parts.iter().find(|p| p.0 == "CONVERSION_BASED_UNIT") {
                // a conversion unit (the inch): its measure says how many of the base unit one of these is
                for v in a {
                    if let Some(m) = r#ref(v).and_then(|m| self.part(m, "LENGTH_MEASURE_WITH_UNIT"))
                        && let Some(Val::Typed(_, n)) = m.first()
                        && let Some(x) = num(n.first())
                    {
                        let x = &x;
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

    /// Radians in one of the file's plane angles: a conversion-based plane angle unit (the degree) gives its measure (π/180), else 1.
    fn angle_rad(&self) -> f64 {
        let mut units: Vec<(&u32, &Parts)> = self.ents.iter().filter(|(_, p)| p.iter().any(|q| q.0 == "PLANE_ANGLE_UNIT")).collect();
        units.sort_by_key(|(id, _)| **id);
        for (_, parts) in units {
            if let Some((_, a)) = parts.iter().find(|p| p.0 == "CONVERSION_BASED_UNIT") {
                for v in a {
                    if let Some(m) = r#ref(v).and_then(|m| self.part(m, "PLANE_ANGLE_MEASURE_WITH_UNIT"))
                        && let Some(Val::Typed(_, n)) = m.first()
                        && let Some(x) = num(n.first())
                    {
                        return x;
                    }
                }
                return PI / 180.0;
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
                    let c = |k: usize| if let Some(x) = num(a.get(k)) { (x.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 128 };
                    return Some([c(1), c(2), c(3)]);
                }
                self.args(*id).iter().find_map(|x| self.find_colour(x, depth + 1))
            }
            Val::List(l) | Val::Typed(_, l) => l.iter().find_map(|x| self.find_colour(x, depth + 1)),
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
    match v {
        Some(Val::Num(x)) => Some(*x),
        Some(Val::Int(i)) => Some(*i as f64),
        _ => None,
    }
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

/// Where a solid stands in the file: placements from the solid's own frame outwards, each a frame in the part and the frame
/// it is moved onto in the assembly.
type Chain = Vec<(Frame, Frame)>;

/// The point `p` moved along the chain.
fn place(chain: &Chain, p: V3) -> V3 {
    chain.iter().fold(p, |p, (from, to)| {
        let (x, y, z) = from.rel(p);
        add(to.o, add(mul(to.x, x), add(mul(to.y, y), mul(to.z, z))))
    })
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
                if let Some(d) = num(a.get(off)) {
                    deg = d as usize;
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
        // a seam (the edge where a closed surface meets itself, gone up and back down in the same loop) is left out: in (u, v) both
        // passes lie at the same angle and the outline would fold back on itself; the rest still closes (`close_wrapped`)
        let edge_of = |e: &Val| r#ref(e).and_then(|oe| self.args(oe).get(3).and_then(r#ref));
        let mut uses: HashMap<u32, usize> = HashMap::new();
        for e in edges {
            if let Some(id) = edge_of(e) {
                *uses.entry(id).or_default() += 1;
            }
        }
        for e in edges {
            let Some(oe) = r#ref(e) else { continue };
            let a = self.args(oe);
            let Some(edge) = a.get(3).and_then(r#ref) else { continue };
            if uses.get(&edge).copied().unwrap_or(0) > 1 {
                continue;
            }
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
    /// A cone: its frame, then the radius as a line along the axis (`rho = a + b·z`, fitted to the face's outline in `add_face`, so the
    /// file's angle unit does not matter) and the radius one turn of u is laid out at (the widest of the face).
    Cone(Frame, f64, f64, f64),
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
            // the half-angle's tangent rides in the slope's place until `fit_cone` makes the line from the outline
            ("CONICAL_SURFACE", Some(f)) => Surf::Cone(f, num(a.get(2)).unwrap_or(0.0), (num(a.get(3)).unwrap_or(0.0) * self.angle_rad()).tan(), 0.0),
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
        Surf::Cone(f, _, _, wide) => angular(&f, &|t, z, _| (t * wide.max(1e-3), z)),
        Surf::Sphere(f, r) => angular(&f, &|t, z, _| (t * r, (z / r).clamp(-1.0, 1.0).asin() * r)),
        Surf::Torus(f, big, small) => {
            // the angle round the tube can cross ±π (a face on the inner side of the ring, as a concave fillet is): kept going
            // without jumps along each loop like the angle round the axis, and every loop on the same turn as the outline
            let mut uv = angular(&f, &|t, z, rho| (t * big, (z).atan2(rho - big) * small));
            let period = TAU * small;
            for l in uv.iter_mut() {
                for i in 1..l.len() {
                    let prev = l[i - 1][1];
                    while l[i][1] - prev > period / 2.0 {
                        l[i][1] -= period;
                    }
                    while l[i][1] - prev < -period / 2.0 {
                        l[i][1] += period;
                    }
                }
            }
            let mean = |l: &Vec<[f64; 2]>| l.iter().map(|p| p[1]).sum::<f64>() / l.len().max(1) as f64;
            let centre = uv.first().map_or(0.0, mean);
            for l in uv.iter_mut().skip(1) {
                let k = ((centre - mean(l)) / period).round();
                for p in l.iter_mut() {
                    p[1] += k * period;
                }
            }
            uv
        }
        Surf::Other => {
            let f = plane_frame();
            loops.iter().map(|l| l.iter().map(|p| { let (x, y, _) = f.rel(*p); [x, y] }).collect()).collect()
        }
    }
}

/// A cone's radius along its axis, fitted to the face's outline: `rho = a + b·z` by least squares over the outline's points (they are
/// on the face whatever the file's angle unit); when the outline has one height only (a pointed cone bounded by its base circle), the
/// file's radius `r` at the frame's origin and the tangent of its half-angle `slope`.
fn fit_cone(f: Frame, r: f64, slope: f64, loops: &[Vec<V3>]) -> Surf {
    let pts: Vec<(f64, f64)> = loops.iter().flatten().map(|p| {
        let (x, y, z) = f.rel(*p);
        (z, (x * x + y * y).sqrt())
    }).collect();
    let n = pts.len().max(1) as f64;
    let (mz, mr) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let szz: f64 = pts.iter().map(|p| (p.0 - mz).powi(2)).sum();
    let szr: f64 = pts.iter().map(|p| (p.0 - mz) * (p.1 - mr)).sum();
    let (a, b) = if szz > 1e-9 * n { let b = szr / szz; (mr - b * mz, b) } else { (r, slope) };
    let wide = pts.iter().map(|p| p.1).fold(0.0, f64::max).max(1e-3);
    Surf::Cone(f, a, b, wide)
}

/// A B-spline surface (`B_SPLINE_SURFACE_WITH_KNOTS`, rational or not): degrees in u and v, the control net (rows along u), its
/// weights, and the full knot vectors.
struct BSurf {
    du: usize,
    dv: usize,
    ctrl: Vec<Vec<V3>>,
    w: Vec<Vec<f64>>,
    ku: Vec<f64>,
    kv: Vec<f64>,
}

/// De Boor's algorithm on homogeneous points (`pts`: the `deg + 1` points of the span starting at knot index `span - deg`).
fn de_boor(knots: &[f64], deg: usize, span: usize, mut d: Vec<[f64; 4]>, t: f64) -> [f64; 4] {
    for r in 1..=deg {
        for j in (r..=deg).rev() {
            let i = j + span - deg;
            let den = knots[i + deg + 1 - r] - knots[i];
            let a = if den.abs() < 1e-14 { 0.0 } else { (t - knots[i]) / den };
            let prev = d[j - 1];
            for (c, x) in d[j].iter_mut().enumerate() {
                *x = (1.0 - a) * prev[c] + a * *x;
            }
        }
    }
    d[deg]
}

/// The knot span holding `t`: the last interval that starts at or before it (the last one for the end of the range).
fn knot_span(knots: &[f64], deg: usize, n: usize, t: f64) -> usize {
    let mut k = deg;
    while k + 1 < n && knots[k + 1] <= t {
        k += 1;
    }
    k
}

impl BSurf {
    /// The parameter ranges (u, v).
    fn domain(&self) -> ([f64; 2], [f64; 2]) {
        ([self.ku[self.du], self.ku[self.ctrl.len()]], [self.kv[self.dv], self.kv[self.ctrl[0].len()]])
    }

    fn eval(&self, u: f64, v: f64) -> V3 {
        let (nu, nv) = (self.ctrl.len(), self.ctrl[0].len());
        let (su, sv) = (knot_span(&self.ku, self.du, nu, u), knot_span(&self.kv, self.dv, nv, v));
        let rows: Vec<[f64; 4]> = (0..=self.du)
            .map(|j| {
                let i = su + j - self.du;
                let d: Vec<[f64; 4]> = (0..=self.dv).map(|k| {
                    let m = sv + k - self.dv;
                    let (p, w) = (self.ctrl[i][m], self.w[i][m]);
                    [p[0] * w, p[1] * w, p[2] * w, w]
                }).collect();
                de_boor(&self.kv, self.dv, sv, d, v)
            })
            .collect();
        let h = de_boor(&self.ku, self.du, su, rows, u);
        let w = if h[3].abs() < 1e-14 { 1.0 } else { h[3] };
        [h[0] / w, h[1] / w, h[2] / w]
    }
}

/// One turn round the axis of a curved surface, in its u (as `flatten` lays it out), and the latitude of its poles in v (spheres only).
fn turn_and_pole(surf: Surf) -> Option<(f64, Option<f64>)> {
    match surf {
        Surf::Cylinder(_, r) if r > 0.0 => Some((TAU * r, None)),
        Surf::Sphere(_, r) if r > 0.0 => Some((TAU * r, Some(PI / 2.0 * r))),
        Surf::Torus(_, big, _) if big > 0.0 => Some((TAU * big, None)),
        Surf::Cone(_, _, _, wide) => Some((TAU * wide.max(1e-3), None)),
        _ => None,
    }
}

/// A face whose outline goes all the way round its surface's axis has no area in (u, v) as `flatten` lays it out (the loop is a line from
/// one turn to the next). Two such loops going opposite ways (a band of a cylinder or a torus, written without a seam edge) are joined into
/// one outline; one alone on a sphere (a dome) is closed over the pole on the side the face is on: the left of the loop in (u, v) when the
/// face goes with its surface (`same`), else the right. `loops` (3D) and `uv` stay index for index alike.
fn close_wrapped(surf: Surf, same: bool, tol: f64, loops: &mut Vec<Vec<V3>>, uv: &mut Vec<Vec<[f64; 2]>>) {
    let Some((turn, pole)) = turn_and_pole(surf) else { return };
    // how far each loop runs along u, end to end (`flatten` keeps the angle going without jumps)
    let run = |l: &Vec<[f64; 2]>| l.last().map_or(0.0, |e| e[0] - l[0][0]);
    let wrapping: Vec<usize> = (0..uv.len()).filter(|&i| uv[i].len() >= 2 && run(&uv[i]).abs() > 0.75 * turn).collect();
    match wrapping.as_slice() {
        [a, b] if run(&uv[*a]).signum() != run(&uv[*b]).signum() => {
            let (a, b) = (*a, *b);
            // the second loop moved by whole turns to start where the first ends, then the two as one outline
            let end = uv[a].last().map_or(0.0, |e| e[0]);
            let k = ((end - uv[b][0][0]) / turn).round();
            let moved: Vec<[f64; 2]> = uv[b].iter().map(|p| [p[0] + k * turn, p[1]]).collect();
            let (lb, ub) = (loops[b].clone(), moved);
            loops[a].extend(lb);
            uv[a].extend(ub);
            loops.remove(b);
            uv.remove(b);
            if a != 0 && b != 0 {
                return;
            }
            // the joined loop is the face's outline
            let a = if b < a { a - 1 } else { a };
            loops.swap(0, a);
            uv.swap(0, a);
        }
        [a] if matches!(surf, Surf::Cone(..)) => {
            // a pointed cone bounded by one circle: the face runs from the circle to the apex (rho = 0), straight up the meridians
            let Surf::Cone(f, ra, rb, _) = surf else { return };
            if rb.abs() < 1e-12 {
                return;
            }
            let a = *a;
            let za = -ra / rb;
            let dir = run(&uv[a]).signum();
            let apex = add(f.o, mul(f.z, za));
            let first = (uv[a][0], loops[a][0]);
            uv[a].push([first.0[0] + dir * turn, first.0[1]]);
            loops[a].push(first.1);
            let (u_end, u_start) = (uv[a].last().map_or(0.0, |e| e[0]), uv[a][0][0]);
            uv[a].push([u_end, za]);
            loops[a].push(apex);
            uv[a].push([u_start, za]);
            loops[a].push(apex);
            if a != 0 {
                loops.swap(0, a);
                uv.swap(0, a);
            }
        }
        [a] => {
            let (Some(pv), Surf::Sphere(f, r)) = (pole, surf) else { return };
            let a = *a;
            let dir = run(&uv[a]).signum();
            // the face's side of the loop: up (towards +v) when it is on the left of a loop running +u
            let up = (dir > 0.0) == same;
            let v = if up { pv } else { -pv };
            let top = add(f.o, mul(f.z, if up { r } else { -r }));
            // the loop closes back to its start a whole turn on: that point again first, so the last piece of the loop is not lost
            let first = (uv[a][0], loops[a][0]);
            uv[a].push([first.0[0] + dir * turn, first.0[1]]);
            loops[a].push(first.1);
            let (u_end, u_start) = (uv[a].last().map_or(0.0, |e| e[0]), uv[a][0][0]);
            // up the meridian at the loop's end to the pole, along the pole, and down the meridian at its start: the meridians are
            // curved on the sphere, so they get points of their own (a straight chord from the loop to the pole cuts through the dome)
            let v_end = uv[a].last().map_or(0.0, |e| e[1]);
            let v_start = uv[a][0][1];
            let at = |u: f64, vv: f64| {
                let (t, phi) = (u / r, vv / r);
                add(f.at(r * phi.cos(), r * phi.cos(), t), mul(f.z, r * phi.sin()))
            };
            let step = (8.0 * r * tol).sqrt().max(1e-3);
            let n_up = (((v - v_end).abs() / step).ceil() as usize).max(1);
            for k in 1..n_up {
                let vv = v_end + (v - v_end) * k as f64 / n_up as f64;
                uv[a].push([u_end, vv]);
                loops[a].push(at(u_end, vv));
            }
            uv[a].push([u_end, v]);
            loops[a].push(top);
            uv[a].push([u_start, v]);
            loops[a].push(top);
            let n_down = (((v - v_start).abs() / step).ceil() as usize).max(1);
            for k in 1..n_down {
                let vv = v + (v_start - v) * k as f64 / n_down as f64;
                uv[a].push([u_start, vv]);
                loops[a].push(at(u_start, vv));
            }
            if a != 0 {
                loops.swap(0, a);
                uv.swap(0, a);
            }
        }
        _ => {}
    }
}

/// At most this many points inside one curved face (a big dome gets them further apart).
const MAX_INNER: usize = 3000;

/// Is the point inside the face (inside an odd number of its loops)?
fn inside_loops(loops: &[Vec<[f64; 2]>], p: [f64; 2]) -> bool {
    let mut odd = false;
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
    }
    odd
}

/// How far the point is from the face's outlines.
fn edge_distance(loops: &[Vec<[f64; 2]>], p: [f64; 2]) -> f64 {
    let mut best = f64::MAX;
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = (dx * dx + dy * dy).max(1e-18);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
            best = best.min(d2(p, [a[0] + t * dx, a[1] + t * dy]).sqrt());
        }
    }
    best
}

/// The way from a curved face's (u, v) (as `flatten` lays it out) back to the surface, and the spacing of points along u and v that keeps
/// the flat triangles between them within `tol` of it (a chord of length s on a radius R sags s^2 / 8R). Planes need no points inside;
/// cones and other surfaces have no way back here and are left as they were.
type Back = Box<dyn Fn(f64, f64) -> V3>;
fn surface_grid(surf: Surf, tol: f64) -> Option<(f64, f64, Back)> {
    match surf {
        Surf::Cylinder(f, r) if r > 0.0 => {
            // straight along the axis: the points there only hold the rows across it
            let s = (8.0 * r * tol).sqrt();
            Some((s, s * 4.0, Box::new(move |u, v| add(f.at(r, r, u / r), mul(f.z, v)))))
        }
        Surf::Sphere(f, r) if r > 0.0 => {
            let s = (8.0 * r * tol).sqrt();
            Some((s, s, Box::new(move |u, v| {
                let (t, phi) = (u / r, v / r);
                add(f.at(r * phi.cos(), r * phi.cos(), t), mul(f.z, r * phi.sin()))
            })))
        }
        Surf::Cone(f, ra, rb, wide) if wide > 0.0 => {
            // a u step of s is at most s round the widest circle of the face; straight along the meridians
            let s = (8.0 * wide * tol).sqrt();
            Some((s, s * 4.0, Box::new(move |u, v| {
                let rho = ra + rb * v;
                add(f.at(rho, rho, u / wide), mul(f.z, v))
            })))
        }
        Surf::Torus(f, big, small) if small > 0.0 && big > 0.0 => {
            let s = (8.0 * small.min(big) * tol).sqrt();
            Some((s, s, Box::new(move |u, v| {
                let (t, phi) = (u / big, v / small);
                let rho = big + small * phi.cos();
                add(f.at(rho, rho, t), mul(f.z, small * phi.sin()))
            })))
        }
        _ => None,
    }
}

/// Points inside a curved face, in its (u, v) and on the surface: a grid of the spacing `surface_grid` gives (wider on a big face, at
/// most `MAX_INNER` points), clear of the outlines, so that a dome or a round comes out round and not as a few flat facets cut along
/// its outline.
fn inner_points(surf: Surf, uv: &[Vec<[f64; 2]>], tol: f64) -> Vec<([f64; 2], V3)> {
    let Some((su, sv, back)) = surface_grid(surf, tol) else { return vec![] };
    let Some(outer) = uv.first() else { return vec![] };
    let (lo, hi) = outer.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    let (mut su, mut sv) = (su.max(1e-4), sv.max(1e-4));
    while ((hi[0] - lo[0]) / su + 1.0) * ((hi[1] - lo[1]) / sv + 1.0) > MAX_INNER as f64 {
        su *= 1.25;
        sv *= 1.25;
    }
    let (nu, nv) = (((hi[0] - lo[0]) / su).floor() as usize, ((hi[1] - lo[1]) / sv).floor() as usize);
    // the grid centred on the face's box
    let (u0, v0) = (lo[0] + (hi[0] - lo[0] - nu as f64 * su) / 2.0, lo[1] + (hi[1] - lo[1] - nv as f64 * sv) / 2.0);
    let mut out = vec![];
    for i in 0..=nu {
        for j in 0..=nv {
            let p = [u0 + i as f64 * su, v0 + j as f64 * sv];
            if inside_loops(uv, p) && edge_distance(uv, p) > 0.4 * su.min(sv) {
                out.push((p, back(p[0], p[1])));
            }
        }
    }
    out
}

/// Triangles of a polygon with holes and extra points inside it, as corner indices into the concatenation of `loops` and then `inner`.
fn triangulate(loops: &[Vec<[f64; 2]>], inner: &[[f64; 2]]) -> Vec<[usize; 3]> {
    use i_overlay::core::fill_rule::FillRule;
    use i_triangle::float::custom::CustomTriangulatable;
    use i_triangle::int::validation::Validation;
    let flat: Vec<[f64; 2]> = loops.iter().flatten().chain(inner.iter()).copied().collect();
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
    // with points inside, the triangles are made Delaunay (edges flipped): else long slivers join the outline to far rows of the grid and
    // fold back and forth across a dome
    let t = if inner.is_empty() { contours.as_slice().custom_triangulate(validation).to_triangulation::<u32>() } else { contours.as_slice().custom_triangulate_with_steiner_points(inner, validation).into_delaunay().to_triangulation::<u32>() };
    // the triangulation hands back the points as numbers: find each one among the input points (the nearest, looked for in a grid)
    let (lo, hi) = flat.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    let cell = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / (flat.len() as f64).sqrt().max(1.0)).max(1e-9);
    let key = |p: [f64; 2]| (((p[0] - lo[0]) / cell).floor() as i64, ((p[1] - lo[1]) / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in flat.iter().enumerate() {
        grid.entry(key(*p)).or_default().push(i);
    }
    let nearest = |p: [f64; 2]| -> usize {
        let (kx, ky) = key(p);
        let near = (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (kx + dx, ky + dy))).filter_map(|k| grid.get(&k)).flatten().copied();
        near.min_by(|&i, &j| d2(flat[i], p).total_cmp(&d2(flat[j], p))).unwrap_or_else(|| (0..flat.len()).min_by(|&i, &j| d2(flat[i], p).total_cmp(&d2(flat[j], p))).unwrap_or(0))
    };
    let which: Vec<usize> = t.points.iter().map(|p| nearest(*p)).collect();
    t.indices.as_chunks::<3>().0.iter().map(|i| [which[i[0] as usize], which[i[1] as usize], which[i[2] as usize]]).collect()
}

fn d2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The parts of a B-spline surface entity: (simple entity: after its name) degrees, net, then the multiplicities and knots; (complex
/// entity) the same spread over `B_SPLINE_SURFACE`, `B_SPLINE_SURFACE_WITH_KNOTS` and `RATIONAL_B_SPLINE_SURFACE`.
impl Step {
    fn bspline_surface(&self, id: u32) -> Option<BSurf> {
        let parts = self.ents.get(&id)?;
        let nums = |v: Option<&Val>| -> Vec<f64> { if let Some(Val::List(l)) = v { l.iter().filter_map(|x| num(Some(x))).collect() } else { vec![] } };
        let (mut du, mut dv, mut ctrl, mut um, mut vm, mut uk, mut vk, mut w) = (0usize, 0usize, vec![], vec![], vec![], vec![], vec![], vec![]);
        let net = |v: Option<&Val>| -> Vec<Vec<V3>> {
            let Some(Val::List(rows)) = v else { return vec![] };
            rows.iter().map(|r| if let Val::List(l) = r { l.iter().filter_map(|x| r#ref(x).and_then(|p| self.point(p))).collect() } else { vec![] }).collect()
        };
        let simple = parts.len() == 1;
        for (name, a) in parts {
            let off = usize::from(simple);
            match name.as_str() {
                "B_SPLINE_SURFACE_WITH_KNOTS" if simple => {
                    du = num(a.get(off)).unwrap_or(0.0) as usize;
                    dv = num(a.get(off + 1)).unwrap_or(0.0) as usize;
                    ctrl = net(a.get(off + 2));
                    (um, vm, uk, vk) = (nums(a.get(off + 7)), nums(a.get(off + 8)), nums(a.get(off + 9)), nums(a.get(off + 10)));
                }
                "B_SPLINE_SURFACE" => {
                    du = num(a.first()).unwrap_or(0.0) as usize;
                    dv = num(a.get(1)).unwrap_or(0.0) as usize;
                    ctrl = net(a.get(2));
                }
                "B_SPLINE_SURFACE_WITH_KNOTS" => (um, vm, uk, vk) = (nums(a.first()), nums(a.get(1)), nums(a.get(2)), nums(a.get(3))),
                "RATIONAL_B_SPLINE_SURFACE" => {
                    if let Some(Val::List(rows)) = a.first() {
                        w = rows.iter().map(|r| nums(Some(r))).collect();
                    }
                }
                _ => {}
            }
        }
        let (nu, nv) = (ctrl.len(), ctrl.first().map_or(0, Vec::len));
        if du == 0 || dv == 0 || nu <= du || nv <= dv || ctrl.iter().any(|r| r.len() != nv) || um.len() != uk.len() || vm.len() != vk.len() {
            return None;
        }
        let full = |k: &[f64], m: &[f64]| -> Vec<f64> { k.iter().zip(m).flat_map(|(k, m)| std::iter::repeat_n(*k, *m as usize)).collect() };
        let (ku, kv) = (full(&uk, &um), full(&vk, &vm));
        if ku.len() != nu + du + 1 || kv.len() != nv + dv + 1 {
            return None;
        }
        if w.len() != nu || w.iter().any(|r| r.len() != nv) {
            w = vec![vec![1.0; nv]; nu];
        }
        Some(BSurf { du, dv, ctrl, w, ku, kv })
    }

    /// The triangles of a face on a B-spline surface (the corner where three rounds meet, a blend): the outline's points taken back to
    /// the surface's (u, v) (the nearest of a grid, then Gauss-Newton), the (u, v) scaled to lengths on the surface, a grid of points
    /// inside spaced by the surface's tightest bend, and the triangles turned to the surface's side. A side of the patch that shrinks to
    /// one point (a three-sided corner) is a pole: its point on the outline becomes the piece of that side between its neighbours.
    /// False when the outline does not lie on the surface (left to the flat fallback).
    fn add_spline_face(&self, bs: &BSurf, loops: &[Vec<V3>], same: bool, colour: [u8; 3], scale: f64, b: &mut Builder) -> bool {
        const N: usize = 24;
        let ([u0, u1], [v0, v1]) = bs.domain();
        if !(u1 > u0 && v1 > v0) {
            return false;
        }
        let at = |i: usize, j: usize| (u0 + (u1 - u0) * i as f64 / N as f64, v0 + (v1 - v0) * j as f64 / N as f64);
        let grid: Vec<Vec<V3>> = (0..=N).map(|i| (0..=N).map(|j| { let (u, v) = at(i, j); bs.eval(u, v) }).collect()).collect();
        // lengths across the patch, to scale (u, v) so that the triangles are not squashed
        let lu = (0..=N).map(|j| (0..N).map(|i| dist(grid[i + 1][j], grid[i][j])).sum::<f64>()).sum::<f64>() / (N + 1) as f64;
        let lv = (0..=N).map(|i| (0..N).map(|j| dist(grid[i][j + 1], grid[i][j])).sum::<f64>()).sum::<f64>() / (N + 1) as f64;
        if lu < 1e-9 || lv < 1e-9 {
            return false;
        }
        let (cu, cv) = (lu / (u1 - u0), lv / (v1 - v0));
        // sides that shrink to a point: (fixed u or v, which end, the point)
        let tol = self.tol;
        let mut poles: Vec<(bool, f64, V3)> = vec![];
        for (fixed_u, end, line) in [(true, u0, &grid[0]), (true, u1, &grid[N])] {
            if line.iter().all(|p| dist(*p, line[0]) < tol) {
                poles.push((fixed_u, end, line[0]));
            }
        }
        for (end, j) in [(v0, 0), (v1, N)] {
            if (0..=N).all(|i| dist(grid[i][j], grid[0][j]) < tol) {
                poles.push((false, end, grid[0][j]));
            }
        }
        let invert = |p: V3| -> (f64, f64, f64) {
            let (mut bi, mut bj, mut bd) = (0, 0, f64::MAX);
            for (i, row) in grid.iter().enumerate() {
                for (j, q) in row.iter().enumerate() {
                    let d = dist(*q, p);
                    if d < bd {
                        (bi, bj, bd) = (i, j, d);
                    }
                }
            }
            let (mut u, mut v) = at(bi, bj);
            let (hu, hv) = ((u1 - u0) * 1e-6, (v1 - v0) * 1e-6);
            for _ in 0..12 {
                let s = bs.eval(u, v);
                let r = sub(p, s);
                let su = mul(sub(bs.eval((u + hu).min(u1), v), bs.eval((u - hu).max(u0), v)), 1.0 / (2.0 * hu));
                let sv = mul(sub(bs.eval(u, (v + hv).min(v1)), bs.eval(u, (v - hv).max(v0))), 1.0 / (2.0 * hv));
                let (a, bb, c) = (dot(su, su), dot(su, sv), dot(sv, sv));
                let det = a * c - bb * bb;
                if det.abs() < 1e-18 {
                    break;
                }
                let (ru, rv) = (dot(su, r), dot(sv, r));
                let (du, dv) = ((c * ru - bb * rv) / det, (a * rv - bb * ru) / det);
                u = (u + du).clamp(u0, u1);
                v = (v + dv).clamp(v0, v1);
                if du.abs() < 1e-12 * (u1 - u0) && dv.abs() < 1e-12 * (v1 - v0) {
                    break;
                }
            }
            (u, v, dist(bs.eval(u, v), p))
        };
        let scaled = |u: f64, v: f64| [(u - u0) * cu, (v - v0) * cv];
        let mut flat3: Vec<Vec<V3>> = vec![];
        let mut uv: Vec<Vec<[f64; 2]>> = vec![];
        for l in loops {
            let inv: Vec<Option<(f64, f64)>> = l
                .iter()
                .map(|p| if poles.iter().any(|q| dist(q.2, *p) < tol) { None } else { let (u, v, d) = invert(*p); (d < 20.0 * tol).then_some((u, v)) })
                .collect();
            // a point off the surface (not a pole): this is not the face's surface after all
            if inv.iter().zip(l).any(|(x, p)| x.is_none() && !poles.iter().any(|q| dist(q.2, *p) < tol)) {
                return false;
            }
            let (mut pts, mut uvs) = (vec![], vec![]);
            let n = l.len();
            for k in 0..n {
                match inv[k] {
                    Some((u, v)) => {
                        pts.push(l[k]);
                        uvs.push(scaled(u, v));
                    }
                    None => {
                        // the pole: the stretch of its side between the neighbours' (u, v)
                        let Some(&(fixed_u, end, _)) = poles.iter().find(|q| dist(q.2, l[k]) < tol) else { continue };
                        let near = |step: isize| (1..n).find_map(|d| inv[((k as isize + step * d as isize).rem_euclid(n as isize)) as usize]);
                        for (u, v) in [near(-1), near(1)].into_iter().flatten() {
                            pts.push(l[k]);
                            uvs.push(if fixed_u { scaled(end, v) } else { scaled(u, end) });
                        }
                    }
                }
            }
            if pts.len() >= 3 {
                flat3.push(pts);
                uv.push(uvs);
            }
        }
        if uv.is_empty() {
            return false;
        }
        // the tightest bend of the patch (a chord c sagging s bends with radius c² / 8s), for the spacing of the points inside
        let mut radius = f64::MAX;
        for i in 1..N {
            for j in 1..N {
                for (a, m, c) in [(grid[i - 1][j], grid[i][j], grid[i + 1][j]), (grid[i][j - 1], grid[i][j], grid[i][j + 1])] {
                    let chord = dist(a, c);
                    let sag = dist(mul(add(a, c), 0.5), m);
                    if sag > 1e-9 && chord > 1e-9 {
                        radius = radius.min(chord * chord / (8.0 * sag));
                    }
                }
            }
        }
        let mut step = if radius < f64::MAX { (8.0 * radius * tol).sqrt() } else { lu.max(lv) / 4.0 };
        let (lo, hi) = uv[0].iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
        step = step.max(1e-4);
        while ((hi[0] - lo[0]) / step + 1.0) * ((hi[1] - lo[1]) / step + 1.0) > MAX_INNER as f64 {
            step *= 1.25;
        }
        let (nu, nv) = (((hi[0] - lo[0]) / step).floor() as usize, ((hi[1] - lo[1]) / step).floor() as usize);
        let (gu, gv) = (lo[0] + (hi[0] - lo[0] - nu as f64 * step) / 2.0, lo[1] + (hi[1] - lo[1] - nv as f64 * step) / 2.0);
        let mut inner: Vec<([f64; 2], V3)> = vec![];
        for i in 0..=nu {
            for j in 0..=nv {
                let q = [gu + i as f64 * step, gv + j as f64 * step];
                if inside_loops(&uv, q) && edge_distance(&uv, q) > 0.4 * step {
                    inner.push((q, bs.eval(u0 + q[0] / cu, v0 + q[1] / cv)));
                }
            }
        }
        let tris = triangulate(&uv, &inner.iter().map(|p| p.0).collect::<Vec<_>>());
        if tris.is_empty() {
            return false;
        }
        let flat_uv: Vec<[f64; 2]> = uv.iter().flatten().copied().chain(inner.iter().map(|p| p.0)).collect();
        let flat_pts: Vec<V3> = flat3.iter().flatten().copied().chain(inner.iter().map(|p| p.1)).collect();
        let placed: Vec<V3> = flat_pts.iter().map(|p| place(&b.chain, *p)).collect();
        let ci = b.colour(colour);
        let base = b.mesh.vertices.len() as u32;
        b.mesh.vertices.extend(placed.iter().map(|p| mul(*p, scale).map(|x| x as f32)));
        let sign = if same { 1.0 } else { -1.0 };
        let (hu, hv) = ((u1 - u0) * 1e-4, (v1 - v0) * 1e-4);
        for t in tris {
            let (pa, pb, pc) = (flat_pts[t[0]], flat_pts[t[1]], flat_pts[t[2]]);
            let n = cross(sub(pb, pa), sub(pc, pa));
            // the surface's normal at the triangle's middle
            let m = [(flat_uv[t[0]][0] + flat_uv[t[1]][0] + flat_uv[t[2]][0]) / 3.0, (flat_uv[t[0]][1] + flat_uv[t[1]][1] + flat_uv[t[2]][1]) / 3.0];
            let (u, v) = ((u0 + m[0] / cu).clamp(u0 + hu, u1 - hu), (v0 + m[1] / cv).clamp(v0 + hv, v1 - hv));
            let su = sub(bs.eval(u + hu, v), bs.eval(u - hu, v));
            let sv = sub(bs.eval(u, v + hv), bs.eval(u, v - hv));
            let out = mul(cross(su, sv), sign);
            let flip = dot(n, out) < 0.0;
            let (i, j, k) = (base + t[0] as u32, base + t[1] as u32, base + t[2] as u32);
            b.mesh.triangles.push(if flip { [i, k, j] } else { [i, j, k] });
            b.mesh.tri_color.push(ci);
        }
        true
    }
}

struct Builder {
    mesh: Mesh3d,
    /// Where the solid being read stands (see `Step::placements`); empty = at its own coordinates.
    chain: Chain,
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
        let surf = match surf {
            Surf::Cone(f, r, slope, _) => fit_cone(f, r, slope, &loops),
            Surf::Other => {
                if let Some(bs) = self.bspline_surface(sid)
                    && self.add_spline_face(&bs, &loops, same, colour, scale, b)
                {
                    return;
                }
                Surf::Other
            }
            s => s,
        };
        let mut uv = flatten(surf, &loops);
        close_wrapped(surf, same, self.tol, &mut loops, &mut uv);
        let inner = inner_points(surf, &uv, self.tol);
        let flat_pts: Vec<V3> = loops.iter().flatten().copied().chain(inner.iter().map(|p| p.1)).collect();
        // the corners are moved to where the solid stands; the sides of the triangles are judged in the solid's own frame
        let placed: Vec<V3> = flat_pts.iter().map(|p| place(&b.chain, *p)).collect();
        let tris = triangulate(&uv, &inner.iter().map(|p| p.0).collect::<Vec<_>>());
        if tris.is_empty() {
            return;
        }
        let ci = b.colour(colour);
        let base = b.mesh.vertices.len() as u32;
        b.mesh.vertices.extend(placed.iter().map(|p| mul(*p, scale).map(|x| x as f32)));
        let sign = if same { 1.0 } else { -1.0 };
        for t in tris {
            let (pa, pb, pc) = (flat_pts[t[0]], flat_pts[t[1]], flat_pts[t[2]]);
            let n = cross(sub(pb, pa), sub(pc, pa));
            let centre = mul(add(add(pa, pb), pc), 1.0 / 3.0);
            let out = match surf {
                Surf::Plane(f) => Some(mul(f.z, sign)),
                Surf::Cylinder(f, _) | Surf::Cone(f, ..) => {
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

// ------------------------------------------------------------------ copying solids out (E3b)

/// A real as STEP writes it: always with a decimal point.
fn real_text(v: f64) -> String {
    let t = format!("{v}");
    if t.contains(['.', 'e', 'E', 'N', 'i']) { t } else { format!("{t}.") }
}

/// A value as STEP text, its references renumbered by `map`.
fn val_text(v: &Val, map: &dyn Fn(u32) -> u32, out: &mut String) {
    match v {
        Val::Null => out.push('$'),
        Val::Ref(r) => out.push_str(&format!("#{}", map(*r))),
        Val::Num(x) => out.push_str(&real_text(*x)),
        Val::Int(i) => out.push_str(&i.to_string()),
        Val::Str => out.push_str("''"),
        Val::Enum(e) => out.push_str(&format!(".{e}.")),
        Val::List(l) => {
            out.push('(');
            for (k, x) in l.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                val_text(x, map, out);
            }
            out.push(')');
        }
        Val::Typed(name, l) => {
            out.push_str(name);
            val_text(&Val::List(l.clone()), map, out);
        }
    }
}

/// Solids copied out of a STEP file (E3b): the entities' text, numbered from `base` on, every one of them its own copy for each place the
/// file's assembly puts the solid; the solids among them with their colours, and the faces that have a colour of their own.
#[derive(Clone, Debug, Default)]
pub struct Copied {
    pub text: String,
    pub count: usize,
    pub solids: Vec<(usize, Option<[u8; 3]>)>,
    pub faces: Vec<(usize, [u8; 3])>,
}

/// The B-rep solids of a STEP file, as entities to write into another file from entity number `base` on: each solid's faces, edges,
/// curves and surfaces copied as they are (curved faces stay curved), with every point and direction moved where the file's assembly
/// puts the solid and then by `place` / `turn` (a rigid move in millimetres). Files in other length units than the millimetre, and files
/// with surface models only, are refused (their numbers would need scaling, or they are no solids): the caller writes the triangles.
pub fn copy_solids(data: &[u8], base: usize, place_p: &dyn Fn([f64; 3]) -> [f64; 3], turn: &dyn Fn([f64; 3]) -> [f64; 3]) -> Result<Copied, String> {
    let s = Step::parse(data)?;
    if (s.unit_mm() - 1.0).abs() > 1e-9 {
        return Err("the file's length unit is not the millimetre".into());
    }
    let colours = s.colours();
    let placed = s.placements();
    let mut ids: Vec<u32> = s.ents.keys().copied().collect();
    ids.sort_unstable();
    let solids: Vec<u32> = ids.iter().copied().filter(|&id| matches!(s.name(id), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")).collect();
    if solids.is_empty() {
        return Err("no solids".into());
    }
    let mut out = Copied::default();
    let refs_of = |id: u32| -> Vec<u32> {
        fn walk(v: &Val, out: &mut Vec<u32>) {
            match v {
                Val::Ref(r) => out.push(*r),
                Val::List(l) | Val::Typed(_, l) => l.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut v = vec![];
        for (_, a) in s.ents.get(&id).into_iter().flatten() {
            a.iter().for_each(|x| walk(x, &mut v));
        }
        v
    };
    for solid in solids {
        let chains = placed.get(&solid).cloned().unwrap_or_else(|| vec![vec![]]);
        for chain in chains {
            // every entity the solid reaches, numbered in the order met
            let mut order: Vec<u32> = vec![];
            let mut index: HashMap<u32, usize> = HashMap::new();
            let mut stack = vec![solid];
            while let Some(id) = stack.pop() {
                if index.contains_key(&id) || !s.ents.contains_key(&id) {
                    continue;
                }
                index.insert(id, order.len());
                order.push(id);
                if order.len() > 2_000_000 {
                    return Err("too many entities".into());
                }
                stack.extend(refs_of(id));
            }
            let first = base + out.count;
            let map = |r: u32| index.get(&r).map_or(0, |k| (first + k) as u32);
            let origin = place(&chain, [0.0; 3]);
            for (k, id) in order.iter().enumerate() {
                let parts = &s.ents[id];
                let mut line = format!("#{}=", first + k);
                // a point or a direction in space moves; one in a curve's parameter plane (2 numbers) stays
                let three = |a: &[Val]| -> Option<V3> { if let Some(Val::List(l)) = a.get(1) { (l.len() == 3).then(|| vec3(&a[1])).flatten() } else { None } };
                match (parts.as_slice(), parts.first().map(|p| p.0.as_str())) {
                    ([(_, a)], Some("CARTESIAN_POINT")) if three(a).is_some() => {
                        let p = place_p(place(&chain, three(a).unwrap_or_default()));
                        line.push_str(&format!("CARTESIAN_POINT('',({},{},{}))", real_text(p[0]), real_text(p[1]), real_text(p[2])));
                    }
                    ([(_, a)], Some("DIRECTION")) if three(a).is_some() => {
                        let d = turn(sub(place(&chain, three(a).unwrap_or_default()), origin));
                        line.push_str(&format!("DIRECTION('',({},{},{}))", real_text(d[0]), real_text(d[1]), real_text(d[2])));
                    }
                    ([(name, a)], _) => {
                        line.push_str(name);
                        val_text(&Val::List(a.clone()), &map, &mut line);
                    }
                    (many, _) => {
                        line.push('(');
                        for (name, a) in many {
                            line.push_str(name);
                            val_text(&Val::List(a.clone()), &map, &mut line);
                        }
                        line.push(')');
                    }
                }
                line.push_str(";\n");
                out.text.push_str(&line);
                if let Some(c) = colours.get(id)
                    && *id != solid
                    && matches!(s.name(*id), "ADVANCED_FACE" | "FACE_SURFACE")
                {
                    out.faces.push((first + k, *c));
                }
            }
            out.solids.push((first, colours.get(&solid).copied()));
            out.count += order.len();
        }
    }
    Ok(out)
}

/// Reads a STEP file's solids into a mesh (millimetres, in the file's own frame).
pub fn read_step(data: &[u8]) -> Result<Mesh3d, String> {
    let s = Step::parse(data)?;
    let scale = s.unit_mm();
    let colours = s.colours();
    let mut b = Builder { mesh: Mesh3d::default(), chain: vec![] };
    let mut ids: Vec<u32> = s.ents.keys().copied().collect();
    ids.sort_unstable();
    let default = [140u8, 140, 140];
    let mut solids = 0;
    let placed = s.placements();
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
        // a solid the assembly places several times is read once for each place
        for chain in placed.get(&id).cloned().unwrap_or_else(|| vec![vec![]]) {
        b.chain = chain;
        for sh in shells.iter().copied() {
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

    /// The frames the assembly tests place a part with: the origin, and the same orientation moved to x = 10 and x = 20.
    const FRAMES: &str = "#910=CARTESIAN_POINT('',(0.,0.,0.));
#911=DIRECTION('',(0.,0.,1.));
#912=DIRECTION('',(1.,0.,0.));
#913=AXIS2_PLACEMENT_3D('',#910,#911,#912);
#914=CARTESIAN_POINT('',(10.,0.,0.));
#915=AXIS2_PLACEMENT_3D('',#914,#911,#912);
#916=CARTESIAN_POINT('',(20.,0.,0.));
#917=AXIS2_PLACEMENT_3D('',#916,#911,#912);
";

    #[test]
    fn a_part_placed_twice_by_a_relationship_stands_in_both_places() {
        // the part (a 2 mm cube at the origin) is defined once; the assembly places it at x = 10 and x = 20
        let asm = format!(
            "{FRAMES}#920=SHAPE_REPRESENTATION('part',(#913,@SOLID),#999);
#921=SHAPE_REPRESENTATION('asm',(#913),#999);
#930=ITEM_DEFINED_TRANSFORMATION('','',#913,#915);
#931=(REPRESENTATION_RELATIONSHIP('','',#920,#921)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#930)SHAPE_REPRESENTATION_RELATIONSHIP());
#932=ITEM_DEFINED_TRANSFORMATION('','',#913,#917);
#933=(REPRESENTATION_RELATIONSHIP('','',#920,#921)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#932)SHAPE_REPRESENTATION_RELATIONSHIP());
"
        );
        let m = rs(&box_step(2.0, 2.0, 2.0, &asm)).unwrap();
        assert_eq!(m.triangles.len(), 24, "one cube for each place");
        let (lo, hi) = m.bounds().unwrap();
        assert!((lo[0] - 10.0).abs() < 1e-4 && (hi[0] - 22.0).abs() < 1e-4, "x runs {}..{}", lo[0], hi[0]);
        assert!((m.volume() - 16.0).abs() < 1e-3, "volume {}", m.volume());
    }

    #[test]
    fn a_copy_of_the_part_that_nobody_places_is_not_drawn_at_the_origin() {
        // files often hold the part's shape twice (a plain and an advanced-brep representation of it); only the one the assembly
        // places counts, or an unplaced lead would stand in the middle next to the placed ones
        let asm = format!(
            "{FRAMES}#920=SHAPE_REPRESENTATION('part',(#913,@SOLID),#999);
#921=SHAPE_REPRESENTATION('asm',(#913),#999);
#960=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#913,@SOLID),#999);
#930=ITEM_DEFINED_TRANSFORMATION('','',#913,#915);
#931=(REPRESENTATION_RELATIONSHIP('','',#920,#921)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#930)SHAPE_REPRESENTATION_RELATIONSHIP());
#932=ITEM_DEFINED_TRANSFORMATION('','',#913,#917);
#933=(REPRESENTATION_RELATIONSHIP('','',#920,#921)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#932)SHAPE_REPRESENTATION_RELATIONSHIP());
"
        );
        let m = rs(&box_step(2.0, 2.0, 2.0, &asm)).unwrap();
        let (lo, _) = m.bounds().unwrap();
        assert_eq!(m.triangles.len(), 24, "two places, no copy at the origin");
        assert!((lo[0] - 10.0).abs() < 1e-4, "something stands at x = {}", lo[0]);
    }

    #[test]
    fn a_mapped_item_places_the_part_where_its_target_is() {
        let asm = format!(
            "{FRAMES}#920=SHAPE_REPRESENTATION('part',(#913,@SOLID),#999);
#950=REPRESENTATION_MAP(#913,#920);
#940=MAPPED_ITEM('',#950,#915);
#941=MAPPED_ITEM('',#950,#917);
#921=SHAPE_REPRESENTATION('asm',(#940,#941),#999);
"
        );
        let m = rs(&box_step(2.0, 2.0, 2.0, &asm)).unwrap();
        assert_eq!(m.triangles.len(), 24);
        let (lo, hi) = m.bounds().unwrap();
        assert!((lo[0] - 10.0).abs() < 1e-4 && (hi[0] - 22.0).abs() < 1e-4, "x runs {}..{}", lo[0], hi[0]);
    }

    #[test]
    fn a_turned_placement_turns_the_part() {
        // the placement's x axis is the file's y axis: a 4 x 2 mm part lies along y afterwards, and its faces still point outwards
        let asm = "#910=CARTESIAN_POINT('',(0.,0.,0.));
#911=DIRECTION('',(0.,0.,1.));
#912=DIRECTION('',(1.,0.,0.));
#913=AXIS2_PLACEMENT_3D('',#910,#911,#912);
#914=DIRECTION('',(0.,1.,0.));
#915=AXIS2_PLACEMENT_3D('',#910,#911,#914);
#920=SHAPE_REPRESENTATION('part',(#913,@SOLID),#999);
#921=SHAPE_REPRESENTATION('asm',(#913),#999);
#930=ITEM_DEFINED_TRANSFORMATION('','',#913,#915);
#931=(REPRESENTATION_RELATIONSHIP('','',#920,#921)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#930)SHAPE_REPRESENTATION_RELATIONSHIP());
";
        let m = rs(&box_step(4.0, 2.0, 1.0, asm)).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!((hi[1] - lo[1] - 4.0).abs() < 1e-4 && (hi[0] - lo[0] - 2.0).abs() < 1e-4, "size {:?}", [hi[0] - lo[0], hi[1] - lo[1]]);
        assert!((m.volume() - 8.0).abs() < 1e-3, "volume {}", m.volume());
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
    fn a_second_length_unit_does_not_decide_the_scale() {
        // the geometry context names the millimetre; a metre stands in the file for something else (a tolerance)
        let units = "#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));
#901=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#902=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#901)));
";
        for _ in 0..20 {
            let m = rs(&box_step(2.0, 2.0, 2.0, units)).unwrap();
            assert!((m.volume() - 8.0).abs() < 1e-3, "volume {}", m.volume());
        }
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
pub(crate) mod tests_src {
    /// A cylinder of radius 2 and height 5 (two planes and a cylindrical face).
    pub fn cylinder() -> String {
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
}

#[cfg(test)]
mod curved {
    /// A hemisphere of radius 5 (SolidCraft's STEP: one sphere face bounded by the equator, with a seam up to the pole) comes out round:
    /// before the faces got points inside, the dome was a flat disc and its volume a few mm³ of 262.
    #[test]
    fn a_dome_is_round_inside() {
        let m = super::read_step(include_bytes!("../testdata/dome_r5.step")).unwrap();
        let want = 2.0 / 3.0 * std::f64::consts::PI * 125.0;
        let (_, hi) = m.bounds().unwrap();
        assert!((hi[2] - 5.0).abs() < 1e-3, "up to the pole: {hi:?}");
        assert!((m.volume() - want).abs() < 0.03 * want, "{} vs {want}", m.volume());
        assert!(m.triangles.len() < 1500, "{} triangles", m.triangles.len());
    }

    /// A post on a plate with a concave fillet round its foot and a convex one round its top (two tori, the foot's on the inner side of
    /// its ring where the angle round the tube crosses ±π): within 0.5 % of SolidCraft's 688.02 mm³.
    #[test]
    fn concave_and_convex_fillets_keep_their_volume() {
        let m = super::read_step(include_bytes!("../testdata/post_with_fillets.step")).unwrap();
        let want = 688.0201268;
        assert!((m.volume() - want).abs() < 0.005 * want, "{} vs {want}", m.volume());
    }

    /// The dome and a block with rounded edges (cylinders, and B-spline corners where three rounds meet) within 1 % of SolidCraft's
    /// volume: with the corners flat it was 1.2 % short (the corners got points inside in E4, the block alone went from 0.97 % to 0.42 %).
    #[test]
    fn rounded_shapes_keep_their_volume() {
        let m = super::read_step(include_bytes!("../testdata/dome_and_rounded_block.step")).unwrap();
        let want = 261.7993878 + 1133.250720;
        assert!((m.volume() - want).abs() < 0.01 * want, "{} vs {want}", m.volume());
    }

    /// A B-spline surface: a rational quadratic quarter of a cylinder of radius 3 and length 4 (the exact circle) evaluates on the
    /// cylinder everywhere, not only at its corners.
    #[test]
    fn a_rational_b_spline_surface_is_evaluated_on_its_cylinder() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let row = |x: f64, y: f64| vec![[x, y, 0.0], [x, y, 4.0]];
        let bs = super::BSurf { du: 2, dv: 1, ctrl: vec![row(3.0, 0.0), row(3.0, 3.0), row(0.0, 3.0)], w: vec![vec![1.0; 2], vec![h; 2], vec![1.0; 2]], ku: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], kv: vec![0.0, 0.0, 1.0, 1.0] };
        for i in 0..=10 {
            for j in 0..=4 {
                let p = bs.eval(i as f64 / 10.0, j as f64 / 4.0);
                assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 3.0).abs() < 1e-9 && (p[2] - 4.0 * j as f64 / 4.0).abs() < 1e-9, "{p:?}");
            }
        }
    }

    /// A solid copied out of a file into another (E3b): moved as asked, read back it is the same solid there, with its curved faces.
    #[test]
    fn a_solid_is_copied_with_its_curved_faces_and_moved() {
        let src = super::tests_src::cylinder();
        let c = super::copy_solids(src.as_bytes(), 1000, &|p| [p[0] + 10.0, p[1], p[2] + 1.0], &|d| d).unwrap();
        assert_eq!(c.solids.len(), 1);
        assert!(c.text.contains("CYLINDRICAL_SURFACE("), "the round face stays round");
        let file = format!("ISO-10303-21;\nDATA;\n{}ENDSEC;\n", c.text);
        let m = super::read_step(file.as_bytes()).unwrap();
        let orig = super::read_step(src.as_bytes()).unwrap();
        assert!((m.volume() - orig.volume()).abs() < 1e-6, "{} vs {}", m.volume(), orig.volume());
        let ((lo, _), (lo0, _)) = (m.bounds().unwrap(), orig.bounds().unwrap());
        assert!((lo[0] - lo0[0] - 10.0).abs() < 1e-4 && (lo[2] - lo0[2] - 1.0).abs() < 1e-4, "{lo:?} {lo0:?}");
        assert!(super::copy_solids(b"DATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;", 1, &|p| p, &|d| d).is_err());
    }

    /// A pointed cone (a pin's tip): its side is bounded by the base circle alone, closed over the apex, and holds π r² h / 3 (a bit less
    /// for the 24-sided base). Before, the side had no area in (u, v) and was left out.
    #[test]
    fn a_pointed_cone_is_closed_over_its_apex() {
        let t = "ISO-10303-21;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#3=DIRECTION('',(0.,0.,1.));
#4=DIRECTION('',(1.,0.,0.));
#5=AXIS2_PLACEMENT_3D('',#1,#3,#4);
#6=DIRECTION('',(0.,0.,-1.));
#8=AXIS2_PLACEMENT_3D('',#1,#6,#4);
#7=CIRCLE('',#5,2.);
#9=CARTESIAN_POINT('',(2.,0.,0.));
#11=VERTEX_POINT('',#9);
#16=EDGE_CURVE('',#11,#11,#7,.T.);
#20=ORIENTED_EDGE('',*,*,#16,.F.);
#21=EDGE_LOOP('',(#20));
#22=FACE_OUTER_BOUND('',#21,.T.);
#23=PLANE('',#5);
#24=ADVANCED_FACE('',(#22),#23,.F.);
#40=ORIENTED_EDGE('',*,*,#16,.T.);
#44=EDGE_LOOP('',(#40));
#45=FACE_OUTER_BOUND('',#44,.T.);
#46=CONICAL_SURFACE('',#8,2.,0.3805063771);
#47=ADVANCED_FACE('',(#45),#46,.T.);
#50=CLOSED_SHELL('',(#24,#47));
#51=MANIFOLD_SOLID_BREP('',#50);
ENDSEC;
";
        let m = super::read_step(t.as_bytes()).unwrap();
        let want = std::f64::consts::PI * 4.0 * 5.0 / 3.0;
        let (_, hi) = m.bounds().unwrap();
        assert!((hi[2] - 5.0).abs() < 1e-6, "up to the apex: {hi:?}");
        assert!(m.volume() > 0.97 * want && m.volume() < 1.0001 * want, "{} vs {want}", m.volume());
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


