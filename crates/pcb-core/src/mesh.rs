//! A triangle mesh of a part's body (from a STEP file): what the 3D preview draws in place of the guessed box.
//! Millimetres, in the part's own frame (x right, y up on the board, z up from the board face the part sits on).

use serde::{Deserialize, Serialize};

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
