//! "3Dで見る" window: the board as the fab will make it (the picture rebuilt from the exported
//! fabrication files, put on the top and bottom faces) with a body box for every part. A small
//! software renderer: perspective projection, back-face culling and far-to-near drawing order.

use super::*;
use pcb_core::gerber_view::Preview;
use pcb_core::model3d::{part_boxes, PartBox, BOARD_THICKNESS};

/// What the window shows, built from the board when it is opened or refreshed.
struct Scene {
    pv: Preview,
    tex: [egui::TextureHandle; 2],
    /// Outline in counter-clockwise order, and its triangles.
    outline: Vec<Point>,
    tris: Vec<[Point; 3]>,
    lo: Point,
    hi: Point,
    boxes: Vec<PartBox>,
}

pub(super) struct View3d {
    pub(super) open: bool,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    show_parts: bool,
    show_names: bool,
    mask: bool,
    silk: bool,
    scene: Option<Scene>,
    error: Option<String>,
    /// Rebuild the scene on the next frame (set when the window opens).
    stale: bool,
}

impl Default for View3d {
    fn default() -> Self {
        Self { open: false, yaw: 0.0, pitch: 0.9, zoom: 1.0, show_parts: true, show_names: false, mask: true, silk: true, scene: None, error: None, stale: true }
    }
}

/// The camera: where world points land on the screen.
#[derive(Clone, Copy)]
pub(super) struct Cam {
    pub yaw: f32,
    pub pitch: f32,
    pub scale: f32,
    pub center: [f32; 3],
    pub dist: f32,
    pub origin: Pos2,
}

impl Cam {
    /// A direction in view space: (right, up, towards the viewer). Used for lighting and culling.
    pub(super) fn dir(&self, v: [f32; 3]) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let (x1, y1) = (v[0] * cy - v[1] * sy, v[0] * sy + v[1] * cy);
        [x1, y1 * sp + v[2] * cp, -y1 * cp + v[2] * sp]
    }

    /// Screen position of a world point and how near the viewer it is (bigger = nearer).
    pub(super) fn project(&self, p: [f64; 3]) -> (Pos2, f32) {
        let rel = [p[0] as f32 - self.center[0], p[1] as f32 - self.center[1], p[2] as f32 - self.center[2]];
        let [x, up, w] = self.dir(rel);
        let f = self.dist / (self.dist - w).max(self.dist * 0.2);
        (Pos2::new(self.origin.x + x * f * self.scale, self.origin.y - up * f * self.scale), w)
    }
}

fn shade(c: [u8; 3], k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.2);
    Color32::from_rgb((c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8)
}

/// Brightness for a face whose normal (view space) is `n`: lit from above-left of the viewer.
fn light(n: [f32; 3]) -> f32 {
    let l = [-0.35f32, 0.45, 0.82];
    0.45 + 0.6 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

/// The six faces of a box as (corners, outward normal).
fn box_faces(b: &PartBox) -> [([[f64; 3]; 4], [f32; 3]); 6] {
    let (l, h) = (b.lo, b.hi);
    [
        ([[l[0], l[1], h[2]], [h[0], l[1], h[2]], [h[0], h[1], h[2]], [l[0], h[1], h[2]]], [0.0, 0.0, 1.0]),
        ([[l[0], l[1], l[2]], [l[0], h[1], l[2]], [h[0], h[1], l[2]], [h[0], l[1], l[2]]], [0.0, 0.0, -1.0]),
        ([[l[0], l[1], l[2]], [h[0], l[1], l[2]], [h[0], l[1], h[2]], [l[0], l[1], h[2]]], [0.0, -1.0, 0.0]),
        ([[h[0], h[1], l[2]], [l[0], h[1], l[2]], [l[0], h[1], h[2]], [h[0], h[1], h[2]]], [0.0, 1.0, 0.0]),
        ([[l[0], h[1], l[2]], [l[0], l[1], l[2]], [l[0], l[1], h[2]], [l[0], h[1], h[2]]], [-1.0, 0.0, 0.0]),
        ([[h[0], l[1], l[2]], [h[0], h[1], l[2]], [h[0], h[1], h[2]], [h[0], l[1], h[2]]], [1.0, 0.0, 0.0]),
    ]
}

impl Scene {
    fn build(board: &Board, ctx: &egui::Context, mask: bool, silk: bool) -> Result<Scene, String> {
        let files = export::fab_files(board);
        let pv = Preview::from_files(&files, 1000)?;
        let mut outline = board.outline.clone();
        if pcb_core::geom::signed_area(&outline) < 0.0 {
            outline.reverse();
        }
        let tris = triangulate(&outline);
        if tris.is_empty() {
            return Err("基板の外形が面になっていません。外形を決めてから開いてください".into());
        }
        let (lo, hi) = board.bounds().ok_or("基板の外形がありません")?;
        let tex = Self::textures(&pv, ctx, mask, silk);
        Ok(Scene { pv, tex, outline, tris, lo, hi, boxes: part_boxes(board) })
    }

    fn textures(pv: &Preview, ctx: &egui::Context, mask: bool, silk: bool) -> [egui::TextureHandle; 2] {
        let make = |bottom: bool, name: &str| {
            let rgba: Vec<u8> = pv.render(bottom, mask, silk).iter().flatten().copied().collect();
            ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied([pv.frame.w, pv.frame.h], &rgba), egui::TextureOptions::LINEAR)
        };
        [make(false, "board3d_top"), make(true, "board3d_bottom")]
    }

    /// The top or bottom face: the board picture mapped onto the outline triangles.
    fn face(&self, cam: &Cam, bottom: bool, painter: &egui::Painter) {
        let (w, h) = (self.hi.x - self.lo.x, self.hi.y - self.lo.y);
        let z = if bottom { 0.0 } else { BOARD_THICKNESS };
        let mut mesh = egui::Mesh::with_texture(self.tex[bottom as usize].id());
        for t in &self.tris {
            let base = mesh.vertices.len() as u32;
            for p in t {
                let (pos, _) = cam.project([p.x, p.y, z]);
                // the bottom picture is the view from the back: mirrored left to right
                let u = ((p.x - self.lo.x) / w) as f32;
                let uv = egui::pos2(if bottom { 1.0 - u } else { u }, ((self.hi.y - p.y) / h) as f32);
                mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
            }
            mesh.indices.extend([base, base + 1, base + 2]);
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    /// The edge of the board: one wall per outline segment, the ones facing the viewer, far to near.
    fn walls(&self, cam: &Cam, painter: &egui::Painter) {
        let n = self.outline.len();
        let mut walls: Vec<(f32, [Pos2; 4], f32)> = vec![];
        for i in 0..n {
            let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            // counter-clockwise outline: the outside is to the right of the direction of travel
            let normal = [(dy / len) as f32, (-dx / len) as f32, 0.0];
            let nv = cam.dir(normal);
            if nv[2] <= 1e-4 {
                continue;
            }
            let pts = [[a.x, a.y, 0.0], [b.x, b.y, 0.0], [b.x, b.y, BOARD_THICKNESS], [a.x, a.y, BOARD_THICKNESS]].map(|p| cam.project(p));
            walls.push((pts.iter().map(|p| p.1).sum::<f32>() / 4.0, pts.map(|p| p.0), light(nv)));
        }
        walls.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, pts, k) in walls {
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), shade([176, 150, 92], k), Stroke::NONE));
        }
    }

    fn boxes(&self, cam: &Cam, bottom: bool, names: bool, painter: &egui::Painter) {
        let mut list: Vec<(f32, &PartBox)> = self.boxes.iter().filter(|b| b.bottom == bottom).map(|b| (cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, (b.lo[2] + b.hi[2]) / 2.0]).1, b)).collect();
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, b) in list {
            let mut faces: Vec<(f32, Vec<Pos2>, f32)> = vec![];
            for (corners, normal) in box_faces(b) {
                let nv = cam.dir(normal);
                if nv[2] <= 1e-4 {
                    continue;
                }
                let pts = corners.map(|p| cam.project(p));
                faces.push((pts.iter().map(|p| p.1).sum::<f32>() / 4.0, pts.iter().map(|p| p.0).collect(), light(nv)));
            }
            faces.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, pts, k) in faces {
                painter.add(egui::Shape::convex_polygon(pts, shade(b.color, k), Stroke::new(0.6, shade(b.color, 0.35))));
            }
            if names {
                let z = if bottom { b.lo[2] } else { b.hi[2] };
                let (pos, _) = cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, z]);
                painter.text(pos, egui::Align2::CENTER_CENTER, &b.refdes, egui::FontId::proportional(11.0), Color32::WHITE);
            }
        }
    }

    fn draw(&self, cam: &Cam, show_parts: bool, names: bool, painter: &egui::Painter) {
        let above = cam.pitch.sin() > 0.0;
        if above {
            if show_parts {
                self.boxes(cam, true, names, painter);
            }
            self.face(cam, true, painter);
            self.walls(cam, painter);
            self.face(cam, false, painter);
            if show_parts {
                self.boxes(cam, false, names, painter);
            }
        } else {
            if show_parts {
                self.boxes(cam, false, names, painter);
            }
            self.face(cam, false, painter);
            self.walls(cam, painter);
            self.face(cam, true, painter);
            if show_parts {
                self.boxes(cam, true, names, painter);
            }
        }
    }
}

impl BoardEditor {
    /// Opens (and rebuilds) the 3D window.
    pub fn open_view3d(&mut self) {
        self.view3d.open = true;
        self.view3d.stale = true;
    }

    pub fn view3d_window(&mut self, ctx: &egui::Context) {
        if !self.view3d.open {
            return;
        }
        if self.view3d.stale {
            self.view3d.stale = false;
            match Scene::build(&self.board, ctx, self.view3d.mask, self.view3d.silk) {
                Ok(s) => {
                    self.view3d.scene = Some(s);
                    self.view3d.error = None;
                }
                Err(e) => {
                    self.view3d.scene = None;
                    self.view3d.error = Some(e);
                }
            }
        }
        let mut open = true;
        let mut retex = false;
        let mut stale = false;
        let v = &mut self.view3d;
        egui::Window::new("3Dで見る").open(&mut open).default_size([860.0, 640.0]).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("上から").on_hover_text("真上から見ます (部品の配置・配線の確認)").clicked() {
                    (v.yaw, v.pitch) = (0.0, std::f32::consts::FRAC_PI_2);
                }
                if ui.button("斜め").on_hover_text("斜め上から見ます (部品の高さや立体感)").clicked() {
                    (v.yaw, v.pitch) = (0.0, 0.9);
                }
                if ui.button("横から").on_hover_text("真横から見ます (部品の高さや、ケースに入る高さの確認)").clicked() {
                    (v.yaw, v.pitch) = (0.0, 0.12);
                }
                if ui.button("裏から").on_hover_text("基板を裏返して真下から見ます (裏面の部品と配線)").clicked() {
                    (v.yaw, v.pitch) = (0.0, -std::f32::consts::FRAC_PI_2);
                }
                ui.separator();
                ui.checkbox(&mut v.show_parts, "部品").on_hover_text("部品の本体を箱で表示します。高さはフットプリントからの目安です");
                ui.checkbox(&mut v.show_names, "部品名").on_hover_text("部品番号 (R1 など) を表示します");
                if ui.checkbox(&mut v.mask, "ソルダーレジスト").on_hover_text("基板の緑色の保護膜。オフにすると、銅箔の配線がそのまま見えます").changed() {
                    retex = true;
                }
                if ui.checkbox(&mut v.silk, "シルク").on_hover_text("白い印刷 (部品の枠や文字)").changed() {
                    retex = true;
                }
                if ui.button("更新").on_hover_text("基板エディタで変更した内容を反映します").clicked() {
                    stale = true;
                }
            });
            ui.small("ドラッグ: 回す / ホイール: 拡大縮小 / ダブルクリック: 初期の向き。出来上がりの絵は、書き出す製造ファイルから作っています");
            if let Some(e) = &v.error {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return;
            }
            let Some(scene) = &v.scene else { return };
            let size = ui.available_size().max(Vec2::new(300.0, 240.0));
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            painter.rect_filled(resp.rect, 4.0, Color32::from_rgb(34, 38, 46));
            if resp.dragged() {
                let d = resp.drag_delta();
                v.yaw -= d.x * 0.01;
                v.pitch = (v.pitch + d.y * 0.01).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            }
            if resp.double_clicked() {
                (v.yaw, v.pitch, v.zoom) = (0.0, 0.9, 1.0);
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.2, 8.0);
                }
            }
            // the longest side sets the size; the camera stands back three times the board's diagonal
            let longest = (scene.hi.x - scene.lo.x).max(scene.hi.y - scene.lo.y) as f32;
            let extent = ((scene.hi.x - scene.lo.x).hypot(scene.hi.y - scene.lo.y)) as f32;
            let cam = Cam {
                yaw: v.yaw,
                pitch: v.pitch,
                scale: 0.95 * resp.rect.width().min(resp.rect.height()) / (longest * 1.15).max(1.0) * v.zoom,
                center: [((scene.lo.x + scene.hi.x) / 2.0) as f32, ((scene.lo.y + scene.hi.y) / 2.0) as f32, (BOARD_THICKNESS / 2.0) as f32],
                dist: extent * 3.0,
                origin: resp.rect.center(),
            };
            scene.draw(&cam, v.show_parts, v.show_names, &painter);
        });
        if retex && let Some(s) = &mut self.view3d.scene {
            s.tex = Scene::textures(&s.pv, ctx, self.view3d.mask, self.view3d.silk);
        }
        if stale {
            self.view3d.stale = true;
        }
        if !open {
            self.view3d.open = false;
            self.view3d.scene = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(yaw: f32, pitch: f32) -> Cam {
        Cam { yaw, pitch, scale: 10.0, center: [0.0, 0.0, 0.0], dist: 300.0, origin: Pos2::new(100.0, 100.0) }
    }

    #[test]
    fn straight_down_shows_the_board_as_a_map_and_higher_points_are_nearer() {
        let c = cam(0.0, std::f32::consts::FRAC_PI_2);
        let (p, w0) = c.project([0.0, 0.0, 0.0]);
        let (q, w1) = c.project([5.0, 3.0, 0.0]);
        // +x goes right, +y goes up the screen (smaller y)
        assert!(q.x > p.x && q.y < p.y);
        let (_, wz) = c.project([0.0, 0.0, 10.0]);
        assert!(wz > w0 && (w1 - w0).abs() < 1e-4);
        // the top face (normal +z) faces the viewer from above and not from below
        assert!(c.dir([0.0, 0.0, 1.0])[2] > 0.99);
        assert!(cam(0.0, -std::f32::consts::FRAC_PI_2).dir([0.0, 0.0, 1.0])[2] < -0.99);
    }

    #[test]
    fn from_the_side_the_height_is_the_vertical_axis() {
        let c = cam(0.0, 0.0);
        let (p, _) = c.project([0.0, 0.0, 0.0]);
        let (q, _) = c.project([0.0, 0.0, 4.0]);
        assert!(q.y < p.y && (q.x - p.x).abs() < 1e-4);
        // the viewer stands on the -y side: the -y wall faces them, the +y wall does not
        assert!(c.dir([0.0, -1.0, 0.0])[2] > 0.99 && c.dir([0.0, 1.0, 0.0])[2] < -0.99);
    }

    #[test]
    fn a_box_shows_three_faces_from_a_corner_view() {
        let b = PartBox { refdes: "U1".into(), lo: [0.0, 0.0, 1.6], hi: [4.0, 4.0, 4.0], color: [30, 30, 30], bottom: false };
        let c = cam(0.5, 0.9);
        let visible = box_faces(&b).iter().filter(|(_, n)| c.dir(*n)[2] > 1e-4).count();
        assert_eq!(visible, 3);
        // brighter when facing the light than when turned away
        assert!(light([-0.3, 0.4, 0.85]) > light([0.9, -0.3, 0.1]));
    }

    #[test]
    fn the_window_builds_a_scene_for_a_board_with_a_curved_outline() {
        let ctx = egui::Context::default();
        let mut b = Board::new("round", 40.0, 25.0);
        b.outline = pcb_core::outline::rounded_rect(0.0, 0.0, 40.0, 25.0, 5.0);
        let mut pad = Pad::new(Point::new(20.0, 12.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        pad.refdes = "J1".into();
        b.pads.push(pad);
        b.parts.push(pcb_core::Part { refdes: "J1".into(), value: String::new(), footprint: "PinHeader_1x2".into(), pos: Point::new(20.0, 12.0), rot: 0, side: Side::Top });
        let s = Scene::build(&b, &ctx, true, true).unwrap();
        assert_eq!(s.boxes.len(), 1);
        assert!(s.tris.len() > 10, "{}", s.tris.len());
        // a board without an outline is explained
        b.outline.clear();
        assert!(Scene::build(&b, &ctx, true, true).is_err());
    }
}
