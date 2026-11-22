//! A triangle mesh of a part's body (from a STEP file): what the 3D preview draws in place of the guessed box.
//! Millimetres, in the part's own frame (x right, y up on the board, z up from the board face the part sits on).

use serde::{Deserialize, Serialize};

/// The most triangles a part keeps for the 3D view (a bigger mesh is thinned out with `thin_out`): 100 parts of this size
/// still draw in about 70 ms.
pub const KEEP_TRIANGLES: usize = 12_000;

/// The mesh with at most about `keep` triangles: the vertices are snapped to a grid that gets coarser until it fits, and
/// the triangles that fall flat are dropped. A preview needs the outline, not every facet.
pub fn thin_out(mesh: Mesh3d, keep: usize) -> Mesh3d {
    if mesh.triangles.len() <= keep {
        return mesh;
    }
    let Some((lo, hi)) = mesh.bounds() else { return mesh };
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]).max(1e-6);
    let mut cells = 400.0;
    loop {
        let step = size / cells;
        let mut index: std::collections::HashMap<(i64, i64, i64), u32> = std::collections::HashMap::new();
        let mut out = Mesh3d { palette: mesh.palette.clone(), ..Default::default() };
        let mut map = vec![0u32; mesh.vertices.len()];
        for (i, v) in mesh.vertices.iter().enumerate() {
            let key = ((f64::from(v[0]) / step).round() as i64, (f64::from(v[1]) / step).round() as i64, (f64::from(v[2]) / step).round() as i64);
            map[i] = *index.entry(key).or_insert_with(|| {
                out.vertices.push([(key.0 as f64 * step) as f32, (key.1 as f64 * step) as f32, (key.2 as f64 * step) as f32]);
                (out.vertices.len() - 1) as u32
            });
        }
        for (t, c) in mesh.triangles.iter().zip(&mesh.tri_color) {
            let n = [map[t[0] as usize], map[t[1] as usize], map[t[2] as usize]];
            if n[0] != n[1] && n[1] != n[2] && n[0] != n[2] {
                out.triangles.push(n);
                out.tri_color.push(*c);
            }
        }
        if out.triangles.len() <= keep || cells < 20.0 {
            return out;
        }
        cells *= 0.8;
    }
}

/// Where a part's mesh came from (E3b): the maker's STEP file on this computer (the path only: the file is often not ours to pass on)
/// and how it was turned (`rot`, a rotation by quarter turns, rows) and moved (`shift`, mm) to stand over the pads: a point p of the
/// file is `rot · p + shift` in the mesh. The board's STEP takes the original curved faces from it when the file is there, else the
/// mesh's triangles.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelSource {
    pub path: String,
    #[serde(default = "identity")]
    pub rot: [[f64; 3]; 3],
    #[serde(default)]
    pub shift: [f64; 3],
}

fn identity() -> [[f64; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

impl Default for ModelSource {
    fn default() -> Self {
        ModelSource { path: String::new(), rot: identity(), shift: [0.0; 3] }
    }
}

impl ModelSource {
    /// The file at `path` as read, not turned or moved.
    pub fn new(path: &str) -> ModelSource {
        ModelSource { path: path.to_string(), ..Default::default() }
    }

    /// A point of the file (millimetres) where the mesh has it.
    pub fn place(&self, p: [f64; 3]) -> [f64; 3] {
        let v = self.turn(p);
        [v[0] + self.shift[0], v[1] + self.shift[1], v[2] + self.shift[2]]
    }

    /// A direction of the file, turned as the mesh was.
    pub fn turn(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| self.rot[i][0] * p[0] + self.rot[i][1] * p[1] + self.rot[i][2] * p[2])
    }

    /// Then turned as `Mesh3d::turned(axis, quarter)` turns a mesh.
    pub fn turned(&self, axis: usize, quarter: u8) -> ModelSource {
        let (i, j) = ((axis + 1) % 3, (axis + 2) % 3);
        let q = |v: [f64; 3]| -> [f64; 3] {
            let mut w = v;
            let (a, b) = (v[i], v[j]);
            (w[i], w[j]) = match quarter % 4 {
                0 => (a, b),
                1 => (-b, a),
                2 => (-a, -b),
                _ => (b, -a),
            };
            w
        };
        // the columns of the rotation and the shift turn alike
        let cols: [[f64; 3]; 3] = std::array::from_fn(|c| q([self.rot[0][c], self.rot[1][c], self.rot[2][c]]));
        ModelSource { path: self.path.clone(), rot: std::array::from_fn(|r| std::array::from_fn(|c| cols[c][r])), shift: q(self.shift) }
    }

    /// Then moved by `d` (mm).
    pub fn shifted(&self, d: [f64; 3]) -> ModelSource {
        ModelSource { shift: [self.shift[0] + d[0], self.shift[1] + d[1], self.shift[2] + d[2]], ..self.clone() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh3d {
    pub vertices: Vec<[f32; 3]>,
    /// Corner indices into `vertices`, counter-clockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
    /// The colours used (RGB).
    pub palette: Vec<[u8; 3]>,
    /// For each triangle, the index into `palette`.
    pub tri_color: Vec<u8>,
}

impl Mesh3d {
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// Smallest box around the mesh: (min, max).
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut it = self.vertices.iter();
        let first = it.next()?;
        let (mut lo, mut hi) = (first.map(f64::from), first.map(f64::from));
        for v in it {
            for k in 0..3 {
                lo[k] = lo[k].min(f64::from(v[k]));
                hi[k] = hi[k].max(f64::from(v[k]));
            }
        }
        Some((lo, hi))
    }

    /// The unit normal of every triangle (from its corners' order; `None` for a triangle with no area).
    pub fn face_normals(&self) -> Vec<Option<[f32; 3]>> {
        self.triangles.iter().map(|t| unit(self.area_normal(t))).collect()
    }

    /// Twice the area times the outward normal of triangle `t` (counter-clockwise from outside).
    fn area_normal(&self, t: &[u32; 3]) -> [f64; 3] {
        let p = |k: usize| self.vertices[t[k] as usize].map(f64::from);
        let (a, b, c) = (p(0), p(1), p(2));
        let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
    }

    /// A normal for each corner of each triangle, for smooth shading: the mean of the normals of the triangles that meet at that
    /// point, each weighted by its angle there (so how a face was cut into triangles does not tilt it) (corners closer than `WELD` mm count as one point, as a STEP or OBJ often repeats the corners of each face)
    /// and that turn less than `crease_deg` from this triangle. A curved surface cut into flat triangles then shades as a curve,
    /// while the edges of a box (90°) or a chamfer stay sharp. A triangle with no area gets `None` (it is not drawn).
    pub fn corner_normals(&self, crease_deg: f64) -> Vec<Option<[[f32; 3]; 3]>> {
        const WELD: f64 = 1e-3;
        let key = |v: &[f32; 3]| v.map(|x| (f64::from(x) / WELD).round() as i64);
        let unit_n: Vec<Option<[f64; 3]>> = self.triangles.iter().map(|t| unit64(self.area_normal(t))).collect();
        let mut at: std::collections::HashMap<[i64; 3], Vec<u32>> = std::collections::HashMap::new();
        for (i, t) in self.triangles.iter().enumerate() {
            if unit_n[i].is_none() {
                continue;
            }
            for &c in t {
                at.entry(key(&self.vertices[c as usize])).or_default().push(i as u32);
            }
        }
        let cos = crease_deg.to_radians().cos();
        self.triangles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let n = unit_n[i]?;
                Some(t.map(|c| {
                    let mut sum = [0.0f64; 3];
                    let here = key(&self.vertices[c as usize]);
                    for &j in at.get(&here).map_or(&[][..], |v| v.as_slice()) {
                        let Some(m) = unit_n[j as usize] else { continue };
                        if n[0] * m[0] + n[1] * m[1] + n[2] * m[2] >= cos {
                            let w = self.corner_angle(&self.triangles[j as usize], here, &key);
                            sum = [sum[0] + m[0] * w, sum[1] + m[1] * w, sum[2] + m[2] * w];
                        }
                    }
                    unit64(sum).unwrap_or(n).map(|x| x as f32)
                }))
            })
            .collect()
    }

    /// The angle (radians) of triangle `t` at its corner whose welded key is `here` (0 if none is).
    fn corner_angle(&self, t: &[u32; 3], here: [i64; 3], key: &impl Fn(&[f32; 3]) -> [i64; 3]) -> f64 {
        let Some(k) = (0..3).find(|&k| key(&self.vertices[t[k] as usize]) == here) else { return 0.0 };
        let p = |i: usize| self.vertices[t[i % 3] as usize].map(f64::from);
        let (a, b, c) = (p(k), p(k + 1), p(k + 2));
        let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let (lu, lw) = ((u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt(), (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt());
        if lu < 1e-12 || lw < 1e-12 {
            return 0.0;
        }
        ((u[0] * w[0] + u[1] * w[1] + u[2] * w[2]) / (lu * lw)).clamp(-1.0, 1.0).acos()
    }

    /// Enclosed volume (mm³) by the divergence theorem: positive when the triangles face outwards.
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let p = |k: usize| self.vertices[t[k] as usize].map(f64::from);
                let (a, b, c) = (p(0), p(1), p(2));
                let cross = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
                (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0
            })
            .sum()
    }

    /// Turned by `quarter` quarter turns about the axis `axis` (0 = x, 1 = y, 2 = z).
    pub fn turned(&self, axis: usize, quarter: u8) -> Mesh3d {
        let (i, j) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut m = self.clone();
        for v in &mut m.vertices {
            let (a, b) = (v[i], v[j]);
            (v[i], v[j]) = match quarter % 4 {
                0 => (a, b),
                1 => (-b, a),
                2 => (-a, -b),
                _ => (b, -a),
            };
        }
        m
    }

    /// The mesh moved by `d` (millimetres).
    pub fn shifted(&self, d: [f64; 3]) -> Mesh3d {
        let mut m = self.clone();
        for v in &mut m.vertices {
            for k in 0..3 {
                v[k] = (f64::from(v[k]) + d[k]) as f32;
            }
        }
        m
    }

    /// The mesh moved by `d` after being turned by `quarter_turns` quarter turns about z (counter-clockwise).
    pub fn placed(&self, quarter_turns: u8, d: [f64; 3]) -> Mesh3d {
        let mut m = self.clone();
        for v in &mut m.vertices {
            let (x, y) = (f64::from(v[0]), f64::from(v[1]));
            let (x, y) = match quarter_turns % 4 {
                0 => (x, y),
                1 => (-y, x),
                2 => (-x, -y),
                _ => (y, -x),
            };
            *v = [(x + d[0]) as f32, (y + d[1]) as f32, (f64::from(v[2]) + d[2]) as f32];
        }
        m
    }
}

fn unit64(n: [f64; 3]) -> Option<[f64; 3]> {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (len > 1e-12).then(|| n.map(|x| x / len))
}

fn unit(n: [f64; 3]) -> Option<[f32; 3]> {
    unit64(n).map(|v| v.map(|x| x as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model source turned and moved puts a point where the mesh turned and moved the same way has it.
    #[test]
    fn a_model_source_follows_its_mesh() {
        let m = Mesh3d { vertices: vec![[1.0, 2.0, 3.0]], triangles: vec![], palette: vec![], tri_color: vec![] };
        let (mut mm, mut src) = (m.clone(), ModelSource::new("x.step"));
        for (axis, q) in [(0, 1), (2, 3), (1, 2), (2, 1)] {
            mm = mm.turned(axis, q);
            src = src.turned(axis, q);
        }
        mm = mm.shifted([5.0, -1.0, 0.5]);
        src = src.shifted([5.0, -1.0, 0.5]);
        let p = src.place([1.0, 2.0, 3.0]);
        assert!((0..3).all(|k| (p[k] - f64::from(mm.vertices[0][k])).abs() < 1e-6), "{p:?} {:?}", mm.vertices[0]);
    }

    /// A cylinder of radius 1 along z, `seg` sides, each side face a separate quad (its own corners, as a STEP gives them), with caps.
    fn cylinder(seg: usize) -> Mesh3d {
        let mut m = Mesh3d { palette: vec![[200, 200, 200]], ..Default::default() };
        let at = |k: usize, z: f32| {
            let t = std::f32::consts::TAU * k as f32 / seg as f32;
            [t.cos(), t.sin(), z]
        };
        for k in 0..seg {
            let b = m.vertices.len() as u32;
            m.vertices.extend([at(k, 0.0), at(k + 1, 0.0), at(k + 1, 2.0), at(k, 2.0)]);
            m.triangles.extend([[b, b + 1, b + 2], [b, b + 2, b + 3]]);
        }
        for (z, up) in [(2.0, true), (0.0, false)] {
            let c = m.vertices.len() as u32;
            m.vertices.push([0.0, 0.0, z]);
            for k in 0..seg {
                m.vertices.push(at(k, z));
            }
            for k in 0..seg as u32 {
                let (a, b) = (c + 1 + k, c + 1 + (k + 1) % seg as u32);
                m.triangles.push(if up { [c, a, b] } else { [c, b, a] });
            }
        }
        m.tri_color = vec![0; m.triangles.len()];
        m
    }

    /// The sides of a cylinder shade round: each corner's normal points straight out from the axis (across the separate side
    /// faces), while the rim against the flat caps stays sharp (the caps keep their own normal).
    #[test]
    fn a_cylinder_shades_round_and_keeps_its_rim_sharp() {
        let m = cylinder(24);
        let n = m.corner_normals(40.0);
        for (t, cn) in m.triangles.iter().zip(&n) {
            let cn = cn.unwrap();
            let face = m.face_normals()[0];
            assert!(face.is_some());
            for (k, &c) in t.iter().enumerate() {
                let v = m.vertices[c as usize];
                let r = (v[0] * v[0] + v[1] * v[1]).sqrt();
                let nk = cn[k];
                if nk[2].abs() > 0.99 {
                    continue; // a cap
                }
                assert!((r - 1.0).abs() < 1e-5);
                assert!((nk[0] - v[0]).abs() < 1e-4 && (nk[1] - v[1]).abs() < 1e-4 && nk[2].abs() < 1e-5, "side corner {v:?}: {nk:?}");
            }
        }
        // every cap corner keeps the cap's own normal, even on the rim
        let sides = 2 * 24;
        for cn in &n[sides..] {
            for nk in cn.unwrap() {
                assert!(nk[2].abs() > 0.9999, "{nk:?}");
            }
        }
        // a crease of 1° keeps every face flat: the corners have the face's own normal
        let flat = m.corner_normals(1.0);
        for (f, cn) in m.face_normals().iter().zip(&flat) {
            let f = f.unwrap();
            for nk in cn.unwrap() {
                assert!((0..3).all(|k| (nk[k] - f[k]).abs() < 1e-5));
            }
        }
    }

    #[test]
    fn a_triangle_with_no_area_has_no_normals() {
        let m = Mesh3d { vertices: vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], triangles: vec![[0, 1, 2]], palette: vec![[0; 3]], tri_color: vec![0] };
        assert_eq!(m.corner_normals(40.0), vec![None]);
        assert_eq!(m.face_normals(), vec![None]);
    }
}
