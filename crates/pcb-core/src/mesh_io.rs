//! Triangle meshes in the OBJ format (with its MTL colour file): the common format the 3D shapes of parts go in and out by, beside the
//! STEP of solids. SolidCraft (and FreeCAD, Blender ...) turns a maker's STEP into an OBJ with every curved face cut finely, which our own
//! STEP reader (`step.rs`) cannot do for domes, rounds and free-form faces; the OBJ comes back into a `Mesh3d` here.
//!
//! Read: `v x y z` vertices, `f a b c ...` faces (fanned into triangles; `a/b/c` takes the vertex, a negative index counts from the end),
//! `usemtl NAME` with the MTL's `newmtl NAME` / `Kd r g b` for the colour; normals, texture coordinates, groups and smoothing are ignored.
//! Write: one `o` per named mesh, one material per colour.

use crate::mesh::Mesh3d;
use std::collections::HashMap;
use std::fmt::Write as _;

/// The colours of an MTL file: material name → RGB.
pub fn read_mtl(text: &str) -> HashMap<String, [u8; 3]> {
    let mut out = HashMap::new();
    let mut name = String::new();
    for line in text.lines().map(str::trim) {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("newmtl") => name = w.collect::<Vec<_>>().join(" "),
            Some("Kd") => {
                let c: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
                if let [r, g, b, ..] = c.as_slice() {
                    out.insert(name.clone(), [r, g, b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                }
            }
            _ => {}
        }
    }
    out
}

/// An OBJ file (and the colours of its MTL file) as one mesh, millimetres as written.
pub fn read_obj(text: &str, colors: &HashMap<String, [u8; 3]>) -> Result<Mesh3d, String> {
    const GREY: [u8; 3] = [160, 160, 160];
    let mut m = Mesh3d::default();
    let mut palette_of: HashMap<[u8; 3], u8> = HashMap::new();
    let mut pick = |rgb: [u8; 3], m: &mut Mesh3d| -> u8 {
        *palette_of.entry(rgb).or_insert_with(|| {
            m.palette.push(rgb);
            (m.palette.len() - 1).min(255) as u8
        })
    };
    // faces before any `usemtl` are grey
    let mut color = pick(GREY, &mut m);
    for (n, line) in text.lines().enumerate() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let c: Vec<f32> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                let [x, y, z] = c.as_slice() else { return Err(format!("OBJ の {} 行目: 頂点 (v) の座標が 3 つありません", n + 1)) };
                m.vertices.push([*x, *y, *z]);
            }
            Some("f") => {
                let count = m.vertices.len() as i64;
                let idx: Vec<u32> = w
                    .map(|t| {
                        let i: i64 = t.split('/').next().unwrap_or("").parse().map_err(|_| format!("OBJ の {} 行目: 面 (f) の「{t}」を番号として読めません", n + 1))?;
                        let i = if i < 0 { count + i } else { i - 1 };
                        if (0..count).contains(&i) { Ok(i as u32) } else { Err(format!("OBJ の {} 行目: 面が無い頂点 {} を指しています", n + 1, i + 1)) }
                    })
                    .collect::<Result<_, String>>()?;
                for k in 1..idx.len().saturating_sub(1) {
                    m.triangles.push([idx[0], idx[k], idx[k + 1]]);
                    m.tri_color.push(color);
                }
            }
            Some("usemtl") => {
                let name = w.collect::<Vec<_>>().join(" ");
                color = pick(colors.get(&name).copied().unwrap_or(GREY), &mut m);
            }
            _ => {}
        }
    }
    if m.triangles.is_empty() {
        return Err("OBJ に面 (f) がありません".into());
    }
    Ok(m)
}

/// Meshes as an OBJ file (one `o` per name) and its MTL file (one material per colour); `mtl_name` is the MTL file's name the OBJ refers to.
pub fn write_obj(meshes: &[(&str, &Mesh3d)], mtl_name: &str) -> (String, String) {
    let (mut obj, mut mtl) = (String::new(), String::new());
    let _ = writeln!(obj, "# circuitCad OBJ (mm)\nmtllib {mtl_name}");
    let mut colors: Vec<[u8; 3]> = vec![];
    let mut base = 1usize;
    for (name, m) in meshes {
        let _ = writeln!(obj, "o {}", name.replace(char::is_whitespace, "_"));
        for v in &m.vertices {
            let _ = writeln!(obj, "v {} {} {}", v[0], v[1], v[2]);
        }
        let mut current = None;
        for (t, c) in m.triangles.iter().zip(&m.tri_color) {
            let rgb = m.palette.get(*c as usize).copied().unwrap_or([160, 160, 160]);
            let k = colors.iter().position(|x| *x == rgb).unwrap_or_else(|| {
                colors.push(rgb);
                colors.len() - 1
            });
            if current != Some(k) {
                let _ = writeln!(obj, "usemtl c{k}");
                current = Some(k);
            }
            let _ = writeln!(obj, "f {} {} {}", base + t[0] as usize, base + t[1] as usize, base + t[2] as usize);
        }
        base += m.vertices.len();
    }
    let _ = writeln!(mtl, "# circuitCad colours");
    for (k, c) in colors.iter().enumerate() {
        let [r, g, b] = c.map(|v| f64::from(v) / 255.0);
        let _ = writeln!(mtl, "newmtl c{k}\nKd {r:.4} {g:.4} {b:.4}");
    }
    (obj, mtl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_obj_with_quads_negative_indices_and_colours_reads_as_triangles() {
        let mtl = "newmtl can\nKd 0.122 0.247 0.627\nnewmtl lead\nKd 0.784 0.784 0.784\n";
        let obj = "o can\nusemtl can\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1/1/1 2/2/2 3/3/3 4/4/4\nusemtl lead\nv 0 0 1\nf -1 1 2\n";
        let m = read_obj(obj, &read_mtl(mtl)).unwrap();
        assert_eq!(m.triangles, vec![[0, 1, 2], [0, 2, 3], [4, 0, 1]]);
        let rgb: Vec<[u8; 3]> = m.tri_color.iter().map(|c| m.palette[*c as usize]).collect();
        assert_eq!(rgb, vec![[31, 63, 160], [31, 63, 160], [200, 200, 200]]);
        assert!(read_obj("v 0 0 0\nf 1 2 3\n", &HashMap::new()).unwrap_err().contains("頂点"));
        assert!(read_obj("v 0 0 0\n", &HashMap::new()).unwrap_err().contains("面"));
    }

    #[test]
    fn a_mesh_written_as_obj_reads_back_the_same() {
        let a = Mesh3d { vertices: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 3.0]], triangles: vec![[0, 1, 2]], palette: vec![[9, 99, 199]], tri_color: vec![0] };
        let b = Mesh3d { vertices: vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0]], triangles: vec![[0, 2, 1]], palette: vec![[255, 0, 0]], tri_color: vec![0] };
        let (obj, mtl) = write_obj(&[("A", &a), ("B b", &b)], "x.mtl");
        assert!(obj.contains("o A") && obj.contains("o B_b") && obj.contains("mtllib x.mtl"));
        let m = read_obj(&obj, &read_mtl(&mtl)).unwrap();
        assert_eq!(m.vertices.len(), 6);
        assert_eq!(m.triangles, vec![[0, 1, 2], [3, 5, 4]]);
        let rgb: Vec<[u8; 3]> = m.tri_color.iter().map(|c| m.palette[*c as usize]).collect();
        assert_eq!(rgb, vec![[9, 99, 199], [255, 0, 0]]);
    }
}
