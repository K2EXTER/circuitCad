//! The 3D shape of the part from a maker's STEP file: read, turned and placed so that it stands on the board over the pads,
//! with a top view and a front view to check it. It is saved with the part and drawn by the board's 3D view.

use super::*;
use pcb_core::mesh::Mesh3d;

/// The most triangles kept with a part (a bigger mesh is thinned out): the 3D view draws every one each frame.
const KEEP_TRIANGLES: usize = 12_000;

impl PartForm {
    /// The pads of the footprint as the form builds it: (x, y, w, h).
    fn pad_boxes(&self) -> Vec<(f64, f64, f64, f64)> {
        let Ok(def) = self.to_def() else { return vec![] };
        let Ok(fp) = def.footprint() else { return vec![] };
        fp.pads.iter().map(|p| { let (w, h) = p.shape.size(); (p.pos.x, p.pos.y, w, h) }).collect()
    }

    /// Makes `model3d` from the STEP as read: turned about the axes, stood on the board, centred over the pads.
    pub(super) fn step_apply(&mut self) {
        let Some(base) = &self.step_base else {
            self.model3d = None;
            return;
        };
        let mut m = base.clone();
        for (axis, q) in self.step_rot.iter().enumerate() {
            m = m.turned(axis, *q);
        }
        let pads = self.pad_boxes();
        if let Some((lo, hi)) = m.bounds() {
            let target = if pads.is_empty() {
                (0.0, 0.0)
            } else {
                let (x0, x1) = pads.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
                let (y0, y1) = pads.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
            };
            let d = [
                if self.step_center { target.0 - (lo[0] + hi[0]) / 2.0 } else { 0.0 },
                if self.step_center { target.1 - (lo[1] + hi[1]) / 2.0 } else { 0.0 },
                if self.step_drop { -lo[2] } else { 0.0 },
            ];
            m = m.shifted(d);
        }
        self.model3d = Some(m);
    }

    /// For the command line and the tests.
    pub fn load_step(&mut self, path: &std::path::Path) {
        self.step_load(path);
    }

    /// Reads a STEP file as the part's 3D shape (the message tells what went wrong when it cannot be read).
    pub(super) fn step_load(&mut self, path: &std::path::Path) {
        let result = std::fs::read(path).map_err(|e| format!("読み込めません: {e}")).and_then(|b| pcb_core::step::read_step(&b));
        match result {
            Ok(mesh) => {
                let mesh = thin_out(mesh, KEEP_TRIANGLES);
                self.step_file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.step_msg = String::new();
                self.step_base = Some(mesh);
                self.step_rot = [0; 3];
                self.step_apply();
                if let Some((lo, hi)) = self.model3d.as_ref().and_then(|m| m.bounds())
                    && self.height <= 0.0
                {
                    self.height = ((hi[2] - lo[2]) * 100.0).round() / 100.0;
                }
            }
            Err(e) => self.step_msg = e,
        }
    }

    pub(super) fn step3d_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("3D の形 (メーカーの STEP ファイル)").id_salt("part_step3d").default_open(self.step_base.is_some()).show(ui, |ui| {
            crate::icons::help_small(
                ui,
                "3D の形について",
                "メーカーが配っている部品の STEP ファイル (.step / .stp) を読み込むと、基板の「3Dで見る」で、この部品が実物に近い形で表示されます。形のためだけのデータで、足やパッドの位置は、上のパッケージの寸法で決まります。ファイルは自分でダウンロードしたものを使い、メーカーの使用条件を確かめてください。大きなファイルは、三角形を間引いて保存します",
            );
            ui.horizontal(|ui| {
                if ui.button("STEP を読み込む…").on_hover_text("メーカーの STEP ファイルを選びます。ソリッド (立体) の形だけを読みます。色があれば、色も使います").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("STEP", &["step", "stp", "STEP", "STP"]).pick_file()
                {
                    self.step_load(&p);
                }
                if self.step_base.is_some() && ui.button("3D の形を外す").on_hover_text("この部品の3Dの形を消して、基板の3Dでは推測の箱に戻します").clicked() {
                    self.step_base = None;
                    self.step_file.clear();
                    self.model3d = None;
                }
            });
            if !self.step_msg.is_empty() {
                ui.colored_label(Color32::from_rgb(230, 80, 70), &self.step_msg);
            }
            let Some(m) = self.model3d.clone() else {
                ui.small("読み込むと、ここに上から見た図と横から見た図が出ます");
                return;
            };
            let (lo, hi) = m.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
            ui.small(format!("{}  三角形 {}  大きさ {:.2} × {:.2} × {:.2} mm", self.step_file, m.triangles.len(), hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]));
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.label("向き:");
                for (axis, name) in ["X", "Y", "Z"].iter().enumerate() {
                    if ui.small_button(format!("{name} 軸に90°回す")).on_hover_text(format!("{name} 軸のまわりに、部品を90°回します。STEP の上下が、基板の上下と違うときに使います。横から見た図で、本体が基板の面の上にあれば正しい向きです")).clicked() {
                        self.step_rot[axis] = (self.step_rot[axis] + 1) % 4;
                        changed = true;
                    }
                }
                if ui.small_button("向きを戻す").on_hover_text("回した向きを、ファイルのままに戻します").clicked() {
                    self.step_rot = [0; 3];
                    changed = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                changed |= ui.checkbox(&mut self.step_center, "本体の中心を、パッドの中心に合わせる").on_hover_text("オンにすると、本体の中心 (上から見て) が、パッドの並びの中心に来ます。オフだと、STEP ファイルの原点のままです").changed();
                changed |= ui.checkbox(&mut self.step_drop, "本体の底を、基板の面に付ける").on_hover_text("オンにすると、いちばん低い所が基板の面の高さになります。足が基板にささる部品は、足の先が基板を突き抜けるので、オフにして上下を合わせることもできます").changed();
            });
            if changed {
                self.step_apply();
            }
            if let Some(m) = &self.model3d {
                let pads = self.pad_boxes();
                ui.horizontal(|ui| {
                    mesh_view(ui, m, &pads, false);
                    mesh_view(ui, m, &pads, true);
                });
                ui.small("左: 上から (赤い枠がパッド)  右: 横から (緑の線が基板の面)");
            }
        });
    }
}

/// The mesh with at most about `keep` triangles: the vertices are snapped to a grid that gets coarser until it fits, and
/// the triangles that fall flat are dropped. A preview needs the outline, not every facet.
pub(crate) fn thin_out(mesh: Mesh3d, keep: usize) -> Mesh3d {
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

/// A small picture of the mesh from above (`front` false) or from the front: filled facets, far ones first.
fn mesh_view(ui: &mut egui::Ui, m: &Mesh3d, pads: &[(f64, f64, f64, f64)], front: bool) {
    let size = Vec2::new(176.0, 150.0);
    let (resp, painter) = ui.allocate_painter(size, Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(34, 38, 46));
    let Some((lo, hi)) = m.bounds() else { return };
    // the picture's horizontal is x; the vertical is y (top view) or z (front view)
    let v = |p: [f32; 3]| (f64::from(p[0]), if front { f64::from(p[2]) } else { f64::from(p[1]) });
    let (mut x0, mut x1) = (lo[0], hi[0]);
    let (mut y0, mut y1) = if front { (lo[2], hi[2]) } else { (lo[1], hi[1]) };
    for p in pads {
        x0 = x0.min(p.0 - p.2 / 2.0);
        x1 = x1.max(p.0 + p.2 / 2.0);
        if !front {
            y0 = y0.min(p.1 - p.3 / 2.0);
            y1 = y1.max(p.1 + p.3 / 2.0);
        }
    }
    if front {
        y0 = y0.min(-0.2);
    }
    let scale = ((f64::from(r.width()) - 12.0) / (x1 - x0).max(1e-6)).min((f64::from(r.height()) - 12.0) / (y1 - y0).max(1e-6));
    let to = |x: f64, y: f64| egui::pos2(r.center().x + ((x - (x0 + x1) / 2.0) * scale) as f32, r.center().y - ((y - (y0 + y1) / 2.0) * scale) as f32);
    let mut order: Vec<(f64, usize, f32)> = vec![];
    for (i, t) in m.triangles.iter().enumerate() {
        let p = |k: usize| m.vertices[t[k] as usize].map(f64::from);
        let (a, b, c) = (p(0), p(1), p(2));
        let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        // facing the viewer: +z for the top view, -y for the front view
        let facing = if front { -n[1] / len } else { n[2] / len };
        if facing <= 0.0 {
            continue;
        }
        let depth = if front { -(a[1] + b[1] + c[1]) } else { a[2] + b[2] + c[2] };
        order.push((depth, i, facing as f32));
    }
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut mesh = egui::Mesh::default();
    for (_, i, facing) in order {
        let t = &m.triangles[i];
        let col = m.palette.get(m.tri_color[i] as usize).copied().unwrap_or([140, 140, 140]);
        let k = 0.45 + 0.6 * facing;
        let color = Color32::from_rgb((f32::from(col[0]) * k).min(255.0) as u8, (f32::from(col[1]) * k).min(255.0) as u8, (f32::from(col[2]) * k).min(255.0) as u8);
        for &corner in t {
            let (x, y) = v(m.vertices[corner as usize]);
            mesh.colored_vertex(to(x, y), color);
        }
        let n = mesh.vertices.len() as u32;
        mesh.add_triangle(n - 3, n - 2, n - 1);
    }
    // one mesh, not polygons: a polygon is feathered at its corners, and a long thin facet grows spikes
    painter.add(egui::Shape::mesh(mesh));
    if front {
        // the board face
        painter.line_segment([to(x0, 0.0), to(x1, 0.0)], Stroke::new(1.5, Color32::from_rgb(60, 170, 90)));
    } else {
        for p in pads {
            let rect = Rect::from_two_pos(to(p.0 - p.2 / 2.0, p.1 - p.3 / 2.0), to(p.0 + p.2 / 2.0, p.1 + p.3 / 2.0));
            painter.rect_stroke(rect, 0.0, Stroke::new(1.3, Color32::from_rgb(235, 70, 60)), egui::StrokeKind::Inside);
        }
    }
}
