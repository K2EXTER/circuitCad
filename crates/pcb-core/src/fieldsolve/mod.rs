//! A 2-D field solver for the cross-section of a track: the potential around the copper is found on a mesh (`quad.rs`: a quadtree
//! fine only next to the copper, bilinear elements with the permittivity of each cell), the capacitance per length comes from the
//! energy of that field, and the impedance from the capacitance with the board material and without it (`Z0 = η0 / √(k·k_air)`,
//! `εeff = k / k_air`).
//!
//! Unlike the closed forms of `impedance.rs` (±10 %), it takes the copper thickness, a solder mask over the outer layer and copper
//! of the ground beside the track (a grounded coplanar line) as they are, and checks itself against the exact answers of a thin
//! stripline and a thin coupled stripline (within about 1 %). A maker's own figure is still the one to trust: the real εr and
//! thicknesses of its stack-up are what matter most.
//!
//! Every section here is symmetric about the middle (x = 0), so only the right half is solved: the middle is a mirror (a single
//! track, or a pair driven together) or at 0 V (a pair driven in opposite directions).

use crate::impedance::{Kind, Stackup};

mod quad;

/// Impedance of free space (Ω): `Z0 = η0 / √(k·k_air)` with `k` the capacitance per length over ε0.
const ETA0: f64 = 376.730_313_668;
/// ps per mm at the speed of light (1 / c).
const PS_PER_MM: f64 = 3.335_640_95;
/// The quadtree mesh: cells of 1/160 of the smallest feature at the corners of the copper, 1/12 along its sides, growing by 0.3 of
/// the distance from them. Within about 0.2 % of the exact answers with 1-3 thousand nodes (the rows-and-columns mesh needed 27
/// thousand and an extrapolation from two meshes for that).
const QUAD: quad::Params = quad::Params { corner_div: 160.0, edge_div: 12.0, grow: 0.3 };
/// The rows-and-columns grid the picture of the field is sampled on (finest cell, growth).
const DISPLAY: (f64, f64) = (16.0, 1.15);

/// What the section looks like besides the stack-up: one track or a pair, and copper of the ground beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub kind: Kind,
    /// Width of a track (mm).
    pub width: f64,
    /// Edge-to-edge space of a pair (mm); `None` for one track.
    pub space: Option<f64>,
    /// Gap (mm) from the outer edge of the track(s) to ground copper on the same layer on both sides (a grounded coplanar line), or
    /// `None` when there is none nearby.
    pub ground_gap: Option<f64>,
}

impl Section {
    pub fn single(kind: Kind, width: f64) -> Self {
        Self { kind, width, space: None, ground_gap: None }
    }
    pub fn pair(kind: Kind, width: f64, space: f64) -> Self {
        Self { kind, width, space: Some(space), ground_gap: None }
    }
}

/// One way of driving the line: its impedance, effective permittivity and what follows from them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    /// Characteristic impedance (Ω) of one track in this mode.
    pub z: f64,
    /// Effective relative permittivity: the wave travels as if the whole space were filled with it.
    pub er_eff: f64,
    /// Capacitance (pF/m) and inductance (nH/m) per length of one track.
    pub c_pf_m: f64,
    pub l_nh_m: f64,
    /// Propagation delay (ps/mm).
    pub delay_ps_mm: f64,
}

impl Mode {
    fn from_k(k: f64, k_air: f64) -> Self {
        let z = ETA0 / (k * k_air).sqrt();
        let er_eff = k / k_air;
        let c = k * 8.854_187_812_8; // ε0 in pF/m
        Self { z, er_eff, c_pf_m: c, l_nh_m: z * z * c * 1e-3, delay_ps_mm: er_eff.sqrt() * PS_PER_MM }
    }
}

/// The answer for a section: one track has `single`; a pair has its odd (opposite) and even (together) modes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Solved {
    Single(Mode),
    Pair { odd: Mode, even: Mode },
}

impl Solved {
    /// The impedance that matters: Z0 of one track, or the differential impedance (2·Zodd) of a pair.
    pub fn main_z(&self) -> f64 {
        match self {
            Solved::Single(m) => m.z,
            Solved::Pair { odd, .. } => 2.0 * odd.z,
        }
    }
    /// The common-mode impedance (Zeven / 2) of a pair.
    pub fn common_z(&self) -> Option<f64> {
        match self {
            Solved::Pair { even, .. } => Some(even.z / 2.0),
            Solved::Single(_) => None,
        }
    }
}

/// Answers already worked out (a window asks about the same section every frame, a check about each track of a net), keyed by the
/// bits of the stack-up and the section.
static MEMO: std::sync::Mutex<Option<std::collections::HashMap<Vec<u64>, Solved>>> = std::sync::Mutex::new(None);

fn memo_key(s: &Stackup, sec: &Section) -> Vec<u64> {
    let f = |x: f64| x.to_bits();
    vec![
        f(s.er), f(s.height), f(s.plane_gap), f(s.copper_um), f(s.etch_um), f(s.mask_um), f(s.mask_top_um), f(s.mask_er),
        sec.kind as u64, f(sec.width), sec.space.map_or(u64::MAX, f), sec.ground_gap.map_or(u64::MAX, f),
    ]
}

/// Solves the section (about 0.02 s for a track, twice that for a pair; the same question again is answered from memory).
pub fn solve(s: &Stackup, sec: &Section) -> Solved {
    let key = memo_key(s, sec);
    if let Some(r) = MEMO.lock().unwrap().as_ref().and_then(|m| m.get(&key)) {
        return *r;
    }
    let r = solve_now(s, sec);
    let mut m = MEMO.lock().unwrap();
    let m = m.get_or_insert_with(Default::default);
    if m.len() > 2000 {
        m.clear();
    }
    m.insert(key, r);
    r
}

/// Whether `solve` has this answer already (a window can show the quick estimate while it has not).
pub fn is_known(s: &Stackup, sec: &Section) -> bool {
    MEMO.lock().unwrap().as_ref().is_some_and(|m| m.contains_key(&memo_key(s, sec)))
}

/// The width (mm) of one track for a wanted impedance (Ω), from the field; `None` when no width from 0.03 to 20 mm gives it.
pub fn width_for(s: &Stackup, sec: &Section, target: f64) -> Option<f64> {
    let start = crate::impedance::width_for(s, sec.kind, target).unwrap_or(0.3);
    find(|w| solve(s, &Section { width: w, space: None, ..*sec }).main_z(), start, (0.03, 20.0), target, true)
}

/// The width (mm) of each track of a pair `space` mm apart for a wanted differential impedance (Ω).
pub fn width_for_differential(s: &Stackup, sec: &Section, space: f64, target: f64) -> Option<f64> {
    let start = crate::impedance::width_for_differential(s, sec.kind, space, target).unwrap_or(0.2);
    find(|w| solve(s, &Section { width: w, space: Some(space), ..*sec }).main_z(), start, (0.03, 20.0), target, true)
}

/// The space (mm) of a pair of tracks of `width` mm for a wanted differential impedance (Ω).
pub fn space_for_differential(s: &Stackup, sec: &Section, width: f64, target: f64) -> Option<f64> {
    let start = crate::impedance::space_for_differential(s, sec.kind, width, target).unwrap_or(0.2);
    find(|sp| solve(s, &Section { width, space: Some(sp), ..*sec }).main_z(), start, (0.01, 20.0), target, false)
}

/// Where `f` (falling with x when `falling`, else rising) is `target`, starting at `x0`: Z follows a power of the width or the space
/// closely, so steps on ln Z over ln x (secant) get there in a few solves. `None` when the range does not hold the target.
fn find(f: impl Fn(f64) -> f64, x0: f64, (lo, hi): (f64, f64), target: f64, falling: bool) -> Option<f64> {
    let lt = target.ln();
    let mut a = x0.clamp(lo, hi);
    let mut fa = f(a).ln();
    let mut b = (a * if (fa > lt) == falling { 1.1 } else { 0.9 }).clamp(lo, hi);
    let mut fb = f(b).ln();
    for _ in 0..30 {
        if ((fb - lt) / lt).abs() < 1e-4 {
            return Some(b);
        }
        let slope = (fb - fa) / (b.ln() - a.ln());
        if !slope.is_finite() || slope == 0.0 || (slope < 0.0) != falling {
            break;
        }
        let next = (b.ln() + (lt - fb) / slope).exp().clamp(lo, hi);
        if next == b {
            // pinned at an end of the range: out of reach
            return None;
        }
        (a, fa) = (b, fb);
        b = next;
        fb = f(b).ln();
    }
    // did not settle: bisect the range (slow but sure), when the target is in it
    let (zl, zh) = (f(lo), f(hi));
    if !((zl.min(zh))..=(zl.max(zh))).contains(&target) {
        return None;
    }
    let (mut l, mut h) = (lo, hi);
    for _ in 0..40 {
        let m = (l * h).sqrt();
        if (f(m) > target) == falling {
            l = m;
        } else {
            h = m;
        }
    }
    Some((l * h).sqrt())
}

/// The quadtree mesh of a section, and εr when the whole box is of one material (a stripline: then the air-filled answer is k / εr).
fn quad_mesh(s: &Stackup, sec: &Section) -> (quad::Mesh, Option<f64>) {
    let (x, y) = extent(s, sec);
    let dom = quad::domain(x, y, (sec.kind == Kind::Microstrip).then_some(s.height));
    let sh = shapes(s, sec, Some((dom.0 * dom.1 as f64, dom.0 * dom.2 as f64)));
    let one = (sh.eps.len() == 1 && sh.eps[0].0.x0 <= 0.0 && sh.eps[0].0.y0 <= 0.0 && sh.eps[0].0.y1 >= dom.0 * dom.2 as f64 - 1e-9).then(|| sh.eps[0].1);
    (quad::Mesh::new(&sh, dom, QUAD), one)
}

fn solve_now(s: &Stackup, sec: &Section) -> Solved {
    let (mesh, one) = quad_mesh(s, sec);
    // every mode with the materials and in air: independent solves on the same mesh, run side by side
    let modes: &[bool] = if sec.space.is_some() { &[true, false] } else { &[false] };
    let jobs: Vec<(bool, bool)> = modes.iter().flat_map(|&odd| [(odd, false), (odd, true)]).filter(|&(_, air)| !(air && one.is_some())).collect();
    let ks = crate::par::par_map(&jobs, |&(odd, air)| mesh.solve(odd, air).0);
    let k = |odd: bool, air: bool| match jobs.iter().position(|j| *j == (odd, air)) {
        Some(i) => ks[i],
        None => ks[jobs.iter().position(|j| *j == (odd, false)).expect("solved with the materials")] / one.unwrap_or(1.0),
    };
    let mode = |odd: bool| Mode::from_k(k(odd, false), k(odd, true));
    if sec.space.is_some() {
        Solved::Pair { odd: mode(true), even: mode(false) }
    } else {
        Solved::Single(mode(false))
    }
}

/// Characteristic impedance (Ω) of one track, from the field.
pub fn single_ended(s: &Stackup, kind: Kind, width: f64) -> f64 {
    solve(s, &Section::single(kind, width)).main_z()
}

/// Differential impedance (Ω) of a pair, from the field.
pub fn differential(s: &Stackup, kind: Kind, width: f64, space: f64) -> f64 {
    solve(s, &Section::pair(kind, width, space)).main_z()
}

/// The potential over the right half of a section, for drawing: node coordinates (mm, x from the middle), the potential at each
/// node (row by row, `v[j * xs.len() + i]`, the driven track at 1 V), the permittivity of each cell, and the copper.
#[derive(Clone, Debug)]
pub struct FieldMap {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub v: Vec<f64>,
    /// εr of the cell right of / above node (i, j): `eps[j * (xs.len() - 1) + i]`.
    pub eps: Vec<f64>,
    /// The copper as it is after etching (corners (x, y), counter-clockwise from the bottom left) with its potential (1: the track,
    /// 0: ground), the right half only.
    pub copper: Vec<([[f64; 2]; 4], f64)>,
    /// The left edge is at 0 V (a pair driven in opposite directions): the left half is the mirror with the sign turned.
    pub odd: bool,
    /// Number of nodes the potential was solved at (on the quadtree; `xs`, `ys` are where it is sampled for drawing).
    pub nodes: usize,
}

/// The field of a section for drawing (the odd mode for a pair: the one its differential impedance comes from).
pub fn field_map(s: &Stackup, sec: &Section) -> FieldMap {
    let g = Geometry::meshed(s, sec, sec.space.is_some(), DISPLAY.0, DISPLAY.1);
    let (mesh, _) = quad_mesh(s, sec);
    let (_, v) = mesh.solve(g.odd, false);
    let mut pot = Vec::with_capacity(g.xs.len() * g.ys.len());
    for &y in &g.ys {
        for &x in &g.xs {
            pot.push(mesh.potential(&v, x, y));
        }
    }
    FieldMap { eps: g.cell_eps(), copper: g.copper.iter().map(|c| (c.0.corners(), c.1)).collect(), odd: g.odd, nodes: mesh.nodes, xs: g.xs, ys: g.ys, v: pot }
}

/// A trapezoid with a flat bottom and top: `x0..x1` at `y0`, `tx0..tx1` at `y1` (a rectangle when they are the same). Etched copper
/// is one: its top is narrower than its bottom.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    tx0: f64,
    tx1: f64,
}

impl Rect {
    fn new(x0: f64, x1: f64, y0: f64, y1: f64) -> Self {
        Self { x0, x1, y0, y1, tx0: x0, tx1: x1 }
    }
    fn trapezoid(x0: f64, x1: f64, y0: f64, y1: f64, tx0: f64, tx1: f64) -> Self {
        Self { x0, x1, y0, y1, tx0, tx1 }
    }
    fn holds(&self, x: f64, y: f64) -> bool {
        const E: f64 = 1e-9;
        if y < self.y0 - E || y > self.y1 + E {
            return false;
        }
        let f = if self.y1 > self.y0 { ((y - self.y0) / (self.y1 - self.y0)).clamp(0.0, 1.0) } else { 0.0 };
        let (l, r) = (self.x0 + (self.tx0 - self.x0) * f, self.x1 + (self.tx1 - self.x1) * f);
        x >= l - E && x <= r + E
    }
    /// The corners, counter-clockwise from the bottom left.
    fn corners(&self) -> [[f64; 2]; 4] {
        [[self.x0, self.y0], [self.x1, self.y0], [self.tx1, self.y1], [self.tx0, self.y1]]
    }
}

/// The right half of a section on its mesh. The bottom (the plane under the track), the far right and the top (far above a
/// microstrip, the upper plane of a stripline) are at 0 V.
struct Geometry {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Material rectangles, later ones over earlier ones; the rest is air.
    eps: Vec<(Rect, f64)>,
    /// Copper with its potential.
    copper: Vec<(Rect, f64)>,
    odd: bool,
}

/// The materials and the copper of the right half of a section, in a box `xmax` × `ymax` (at least the one it needs; a mesh may
/// ask for a bigger one). `fine` is the smallest size that matters (track, space, gap, copper and mask thickness, height).
pub(crate) struct Shapes {
    eps: Vec<(Rect, f64)>,
    copper: Vec<(Rect, f64)>,
    xmax: f64,
    ymax: f64,
    fine: f64,
    width: f64,
}

/// The box a section needs: far enough that the field there no longer matters (20 times the size over a plane, 8 plane gaps in a
/// stripline, where it falls off fast).
fn extent(s: &Stackup, sec: &Section) -> (f64, f64) {
    let t = (s.copper_um / 1000.0).max(0.0);
    let w = sec.width.max(1e-4);
    let x1 = sec.space.map_or(w / 2.0, |sp| sp.max(1e-4) / 2.0 + w);
    match sec.kind {
        Kind::Microstrip => {
            let size = s.height + x1;
            (x1 + 20.0 * size, s.height + t + 20.0 * size)
        }
        Kind::Stripline => {
            let b = s.plane_gap.max(t + 1e-3);
            (x1 + 8.0 * b, b)
        }
    }
}

fn shapes(s: &Stackup, sec: &Section, bigger: Option<(f64, f64)>) -> Shapes {
    let (mut xmax, mut ymax) = extent(s, sec);
    if let Some((x, y)) = bigger {
        xmax = xmax.max(x);
        if sec.kind == Kind::Microstrip {
            ymax = ymax.max(y);
        }
    }
    let t = (s.copper_um / 1000.0).max(0.0);
    let w = sec.width.max(1e-4);
    // the track(s) in the right half: one track straddles the middle, a pair starts half the space from it
    let (x0, x1) = match sec.space {
        Some(sp) => (sp.max(1e-4) / 2.0, sp.max(1e-4) / 2.0 + w),
        None => (0.0, w / 2.0),
    };
    // the finished copper: the top narrower than the bottom by the etch (half of it on each outer edge; not at the middle of one
    // track, which is not an edge)
    let e = (s.etch_um / 1000.0).clamp(0.0, w * 0.9) / 2.0;
    let etched = |x0: f64, x1: f64, y0: f64, y1: f64| Rect::trapezoid(x0, x1, y0, y1, if x0 > 0.0 { x0 + e } else { x0 }, x1 - e);
    let mut eps = vec![];
    let mut copper = vec![];
    let mut feats = vec![w, s.height, s.plane_gap];
    if let Some(sp) = sec.space {
        feats.push(sp);
    }
    if let Some(g) = sec.ground_gap {
        feats.push(g);
    }
    if t > 0.0 {
        feats.push(t);
    }
    let ty0;
    match sec.kind {
        Kind::Microstrip => {
            let h = s.height;
            ty0 = h;
            eps.push((Rect::new(0.0, xmax, 0.0, h), s.er));
            let (m, mt) = (s.mask_um / 1000.0, s.mask_top_um / 1000.0);
            if m > 0.0 {
                feats.push(m);
                eps.push((Rect::new(0.0, xmax, h, h + m), s.mask_er));
            }
            if mt > 0.0 {
                feats.push(mt);
                // over the copper it follows the copper's shape, as thick on the sides as on the top
                let c = etched(x0, x1, h, h + t);
                let inner = x0 > 0.0;
                eps.push((Rect::trapezoid(if inner { x0 - mt } else { 0.0 }, x1 + mt, h, h + t + mt, if inner { c.tx0 - mt } else { 0.0 }, c.tx1 + mt), s.mask_er));
                if let Some(g) = sec.ground_gap {
                    eps.push((Rect::new(x1 + g - mt, xmax, h, h + t + mt), s.mask_er));
                }
            }
        }
        Kind::Stripline => {
            let b = ymax;
            ty0 = (b - t) / 2.0;
            eps.push((Rect::new(0.0, xmax, 0.0, b), s.er));
        }
    }
    copper.push((etched(x0, x1, ty0, ty0 + t), 1.0));
    if let Some(g) = sec.ground_gap {
        let gx = x1 + g.max(1e-4);
        copper.push((Rect::trapezoid(gx, xmax, ty0, ty0 + t, gx + e, xmax), 0.0));
    }
    let fine = feats.iter().copied().filter(|f| *f > 0.0).fold(f64::INFINITY, f64::min);
    Shapes { eps, copper, xmax, ymax, fine, width: w }
}

impl Geometry {
    fn meshed(s: &Stackup, sec: &Section, odd: bool, div: f64, ratio: f64) -> Self {
        let Shapes { eps, copper, xmax, ymax, fine, width: w } = shapes(s, sec, None);
        // fine cells at the copper's edges (the field is sharpest there), coarser at the edges of the materials, coarse at the box
        let hmin = (fine / div).min(w / (div * 3.0));
        let (hmat, hbox) = (4.0 * hmin, (xmax.min(ymax)) / 50.0);
        let mut xk = vec![(0.0, hmin), (xmax, hbox)];
        let mut yk = vec![(0.0, hmat), (ymax, hbox)];
        for (r, h) in eps.iter().map(|(r, _)| (r, hmat)).chain(copper.iter().map(|(r, _)| (r, hmin))) {
            xk.extend([r.x0, r.x1, r.tx0, r.tx1].map(|x| (x, if x >= xmax { hbox } else { h })));
            yk.extend([r.y0, r.y1].map(|y| (y, if y >= ymax { hbox } else { h })));
        }
        let xs = axis(xk, ratio, 0.0, xmax);
        let ys = axis(yk, ratio, 0.0, ymax);
        Self { xs, ys, eps, copper, odd }
    }

    /// εr of each cell of the grid (row by row).
    fn cell_eps(&self) -> Vec<f64> {
        let (nx, ny) = (self.xs.len(), self.ys.len());
        let mut eps = vec![1.0; (nx - 1) * (ny - 1)];
        for j in 0..ny - 1 {
            let yc = (self.ys[j] + self.ys[j + 1]) / 2.0;
            for i in 0..nx - 1 {
                let xc = (self.xs[i] + self.xs[i + 1]) / 2.0;
                if let Some((_, e)) = self.eps.iter().rev().find(|(r, _)| r.holds(xc, yc)) {
                    eps[j * (nx - 1) + i] = *e;
                }
            }
        }
        eps
    }
}

/// Mesh lines along one axis through every key `(position, cell next to it)`, the cells growing by `ratio` away from each key until
/// they meet the ones growing from the next key.
fn axis(mut keys: Vec<(f64, f64)>, ratio: f64, lo: f64, hi: f64) -> Vec<f64> {
    keys.retain(|k| k.0 >= lo - 1e-12 && k.0 <= hi + 1e-12);
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    // keys at the same place: the finest cell wins
    let mut merged: Vec<(f64, f64)> = vec![];
    for k in keys {
        match merged.last_mut() {
            Some(m) if (k.0 - m.0).abs() < 1e-7 => m.1 = m.1.min(k.1),
            _ => merged.push(k),
        }
    }
    let mut out = vec![merged[0].0];
    for w in merged.windows(2) {
        let ((a, ha), (b, hb)) = (w[0], w[1]);
        let len = b - a;
        // grow from both ends, always on the side with the smaller next step, until they meet
        let (mut left, mut right) = (vec![], vec![]);
        let (mut hl, mut hr, mut sum) = (ha, hb, 0.0);
        while sum < len {
            if hl <= hr {
                left.push(hl);
                sum += hl;
                hl *= ratio;
            } else {
                right.push(hr);
                sum += hr;
                hr *= ratio;
            }
        }
        let scale = len / sum;
        let mut x = a;
        for st in left.iter().chain(right.iter().rev()).take(left.len() + right.len() - 1) {
            x += st * scale;
            out.push(x);
        }
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Complete elliptic integral of the first kind K(k), by the arithmetic-geometric mean.
    fn ellk(k: f64) -> f64 {
        let (mut a, mut b) = (1.0, (1.0 - k * k).sqrt());
        for _ in 0..40 {
            let (a2, b2) = ((a + b) / 2.0, (a * b).sqrt());
            a = a2;
            b = b2;
        }
        std::f64::consts::PI / (2.0 * a)
    }

    fn thin(er: f64, b: f64) -> Stackup {
        Stackup { er, plane_gap: b, copper_um: 0.0, mask_um: 0.0, mask_top_um: 0.0, etch_um: 0.0, ..Default::default() }
    }

    fn close(got: f64, want: f64, tol: f64) -> bool {
        (got - want).abs() / want < tol
    }

    /// A stripline of no thickness has an exact answer (Cohn): Z0 = η0 / (4√εr) · K(k) / K(k'), k = sech(πw / 2b).
    #[test]
    fn a_thin_stripline_matches_the_exact_answer() {
        for (w, b) in [(0.15, 0.36), (0.5, 0.36), (1.0, 0.5)] {
            let s = thin(4.0, b);
            let k = 1.0 / (std::f64::consts::PI * w / (2.0 * b)).cosh();
            let want = ETA0 / (4.0 * 2.0) * ellk(k) / ellk((1.0 - k * k).sqrt());
            let got = solve(&s, &Section::single(Kind::Stripline, w));
            let Solved::Single(m) = got else { panic!() };
            assert!(close(m.z, want, 0.004), "w={w} b={b}: {} vs {want}", m.z);
            assert!((m.er_eff - 4.0).abs() < 1e-9, "all in the board: εeff = εr");
        }
    }

    /// A thin coupled stripline too (Cohn): Zodd and Zeven from k_o = tanh(πw/2b)·coth(π(w+s)/2b), k_e with tanh for coth.
    #[test]
    fn a_thin_coupled_stripline_matches_the_exact_answer() {
        let (w, sp, b) = (0.2, 0.2, 0.4);
        let s = thin(1.0, b);
        let pi = std::f64::consts::PI;
        let a = (pi * w / (2.0 * b)).tanh();
        let c = (pi * (w + sp) / (2.0 * b)).tanh();
        let z = |k: f64| ETA0 / 4.0 * ellk((1.0 - k * k).sqrt()) / ellk(k);
        let (zo, ze) = (z(a / c), z(a * c));
        let Solved::Pair { odd, even } = solve(&s, &Section::pair(Kind::Stripline, w, sp)) else { panic!() };
        assert!(close(odd.z, zo, 0.004), "odd {} vs {zo}", odd.z);
        assert!(close(even.z, ze, 0.004), "even {} vs {ze}", even.z);
    }

    /// Hammerstad-Jensen (1980) for a thin microstrip: within about 0.2 % of measured and solved values for any width.
    fn hammerstad_jensen(er: f64, u: f64) -> (f64, f64) {
        let pi = std::f64::consts::PI;
        let f = 6.0 + (2.0 * pi - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
        let z_air = ETA0 / (2.0 * pi) * (f / u + (1.0 + 4.0 / (u * u)).sqrt()).ln();
        let a = 1.0 + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
        let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
        let ee = (er + 1.0) / 2.0 + (er - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b);
        (z_air / ee.sqrt(), ee)
    }

    #[test]
    fn a_thin_microstrip_matches_hammerstad_and_jensen() {
        for (w, h) in [(0.3, 1.53), (3.0, 1.53), (0.35, 0.2), (1.0, 0.2)] {
            let s = Stackup { er: 4.5, height: h, copper_um: 0.0, mask_um: 0.0, mask_top_um: 0.0, etch_um: 0.0, ..Default::default() };
            let (zw, ew) = hammerstad_jensen(4.5, w / h);
            let Solved::Single(m) = solve(&s, &Section::single(Kind::Microstrip, w)) else { panic!() };
            assert!(close(m.z, zw, 0.006), "w={w} h={h}: Z {} vs {zw}", m.z);
            assert!(close(m.er_eff, ew, 0.006), "w={w} h={h}: εeff {} vs {ew}", m.er_eff);
        }
    }

    #[test]
    fn copper_thickness_and_a_mask_lower_the_impedance_of_a_microstrip() {
        let base = Stackup { height: 0.2, copper_um: 0.0, mask_um: 0.0, mask_top_um: 0.0, etch_um: 0.0, ..Default::default() };
        let z = |s: &Stackup| single_ended(s, Kind::Microstrip, 0.35);
        let (z_thin, z_thick) = (z(&base), z(&Stackup { copper_um: 35.0, ..base }));
        let z_mask = z(&Stackup { copper_um: 35.0, mask_um: 20.0, mask_top_um: 10.0, ..base });
        assert!(z_thick < z_thin && z_mask < z_thick, "{z_thin} > {z_thick} > {z_mask}");
        // a few ohms, not tens
        assert!(z_thin - z_mask > 1.0 && z_thin - z_mask < 8.0, "{z_thin} {z_mask}");
    }

    /// The etched track is narrower at its top, so it has a little less capacitance than the drawn rectangle: a higher impedance.
    #[test]
    fn the_etched_track_has_a_higher_impedance_than_the_drawn_rectangle() {
        let s = Stackup { height: 0.2, mask_um: 0.0, mask_top_um: 0.0, etch_um: 0.0, ..Default::default() };
        let drawn = single_ended(&s, Kind::Microstrip, 0.35);
        let etched = single_ended(&Stackup { etch_um: 25.0, ..s }, Kind::Microstrip, 0.35);
        assert!(etched > drawn && etched - drawn < 2.0, "{drawn} -> {etched}");
        // the map has the trapezoid
        let m = field_map(&Stackup { etch_um: 25.0, ..s }, &Section::single(Kind::Microstrip, 0.35));
        let c = m.copper[0].0;
        assert!((c[1][0] - 0.175).abs() < 1e-9 && (c[2][0] - (0.175 - 0.0125)).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn ground_copper_beside_the_track_lowers_its_impedance_more_the_closer_it_is() {
        let s = Stackup { height: 1.53, ..Default::default() };
        let z = |g: Option<f64>| solve(&s, &Section { ground_gap: g, ..Section::single(Kind::Microstrip, 1.0) }).main_z();
        let (far, mid, near) = (z(None), z(Some(0.5)), z(Some(0.15)));
        assert!(far > mid && mid > near, "{far} {mid} {near}");
    }

    #[test]
    fn a_pair_far_apart_is_two_single_tracks_and_closer_it_is_lower() {
        let s = Stackup { height: 0.2, ..Default::default() };
        let z0 = single_ended(&s, Kind::Microstrip, 0.3);
        let far = differential(&s, Kind::Microstrip, 0.3, 3.0);
        assert!(close(far, 2.0 * z0, 0.01), "{far} vs 2 x {z0}");
        assert!(differential(&s, Kind::Microstrip, 0.3, 0.15) < differential(&s, Kind::Microstrip, 0.3, 0.4));
    }

    #[test]
    fn the_width_and_the_space_for_a_wanted_impedance_give_it_back() {
        let s = Stackup { height: 0.2, ..Default::default() };
        let one = Section::single(Kind::Microstrip, 0.0);
        let w = width_for(&s, &one, 50.0).unwrap();
        assert!((single_ended(&s, Kind::Microstrip, w) - 50.0).abs() < 0.05, "{w}");
        let w = width_for_differential(&s, &one, 0.2, 90.0).unwrap();
        assert!((differential(&s, Kind::Microstrip, w, 0.2) - 90.0).abs() < 0.05, "{w}");
        let sp = space_for_differential(&s, &one, 0.25, 90.0).unwrap();
        assert!((differential(&s, Kind::Microstrip, 0.25, sp) - 90.0).abs() < 0.05, "{sp}");
        // out of reach: a 1.5 ohm track, and a pair above twice a single track
        assert!(width_for(&s, &one, 1.5).is_none());
        assert!(space_for_differential(&s, &one, 1.0, 400.0).is_none());
    }

    #[test]
    fn the_field_map_has_the_track_at_one_volt_and_the_plane_at_zero() {
        let m = field_map(&Stackup::default(), &Section::single(Kind::Microstrip, 1.0));
        let nx = m.xs.len();
        assert!(m.v[..nx].iter().all(|v| *v == 0.0), "the plane");
        let j = m.ys.iter().position(|y| (*y - 1.53).abs() < 1e-9).unwrap();
        assert!((m.v[j * nx] - 1.0).abs() < 1e-6, "the middle of the track: {}", m.v[j * nx]);
        assert!(m.v.iter().all(|v| (-1e-9..=1.0 + 1e-9).contains(v)));
    }
}
