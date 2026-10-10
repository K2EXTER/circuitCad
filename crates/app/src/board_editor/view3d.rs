//! "3Dで見る" window: the board as the fab will make it (the picture rebuilt from the exported
//! fabrication files, put on the top and bottom faces) with solids for every part. A small
//! software renderer: perspective projection, back-face culling and far-to-near drawing order.
//! Hovering a part names it, clicking selects it in the board editor.

use super::*;
use pcb_core::gerber_view::Preview;
use pcb_core::model3d::{part_boxes, part_total_height, BodyShape, PartBox, BOARD_THICKNESS};

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
    /// `(refdes, value, footprint, height above the board)` for every part, in `Board::parts` order.
    parts: Vec<(String, String, String, f64)>,
    /// The tallest part on each side, for case checks.
    tallest: [Option<(String, f64)>; 2],
}

pub(super) struct View3d {
    pub(super) open: bool,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
    spin: bool,
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
        Self { open: false, yaw: 0.0, pitch: 0.9, zoom: 1.0, pan: Vec2::ZERO, spin: false, show_parts: true, show_names: false, mask: true, silk: true, scene: None, error: None, stale: true }
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
    let k = k.clamp(0.0, 1.3);
    Color32::from_rgb((c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8)
}

/// Brightness for a face whose normal (view space) is `n`: lit from above-left of the viewer.
fn light(n: [f32; 3]) -> f32 {
    let l = [-0.35f32, 0.45, 0.82];
    0.45 + 0.6 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
}

type Face3 = (Vec<[f64; 3]>, [f32; 3]);

/// The faces of a solid (corners, outward normal): a box has six, a cylinder a ring of sides and two caps.
fn solid_faces(b: &PartBox) -> Vec<Face3> {
    let (l, h) = (b.lo, b.hi);
    match b.shape {
        BodyShape::Box => vec![
            (vec![[l[0], l[1], h[2]], [h[0], l[1], h[2]], [h[0], h[1], h[2]], [l[0], h[1], h[2]]], [0.0, 0.0, 1.0]),
            (vec![[l[0], l[1], l[2]], [l[0], h[1], l[2]], [h[0], h[1], l[2]], [h[0], l[1], l[2]]], [0.0, 0.0, -1.0]),
            (vec![[l[0], l[1], l[2]], [h[0], l[1], l[2]], [h[0], l[1], h[2]], [l[0], l[1], h[2]]], [0.0, -1.0, 0.0]),
            (vec![[h[0], h[1], l[2]], [l[0], h[1], l[2]], [l[0], h[1], h[2]], [h[0], h[1], h[2]]], [0.0, 1.0, 0.0]),
            (vec![[l[0], h[1], l[2]], [l[0], l[1], l[2]], [l[0], l[1], h[2]], [l[0], h[1], h[2]]], [-1.0, 0.0, 0.0]),
            (vec![[h[0], l[1], l[2]], [h[0], h[1], l[2]], [h[0], h[1], h[2]], [h[0], l[1], h[2]]], [1.0, 0.0, 0.0]),
        ],
        BodyShape::Cylinder(axis) => {
            let a = axis as usize;
            let (i, j) = ((a + 1) % 3, (a + 2) % 3);
            let (ci, cj) = ((l[i] + h[i]) / 2.0, (l[j] + h[j]) / 2.0);
            let (ri, rj) = ((h[i] - l[i]) / 2.0, (h[j] - l[j]) / 2.0);
            const N: usize = 16;
            let at = |k: usize, along: f64| {
                let t = std::f64::consts::TAU * k as f64 / N as f64;
                let mut p = [0.0; 3];
                p[a] = along;
                p[i] = ci + ri * t.cos();
                p[j] = cj + rj * t.sin();
                p
            };
            let mut faces = vec![];
            for k in 0..N {
                let tm = std::f64::consts::TAU * (k as f64 + 0.5) / N as f64;
                let mut n = [0.0f32; 3];
                n[i] = tm.cos() as f32;
                n[j] = tm.sin() as f32;
                faces.push((vec![at(k, l[a]), at(k + 1, l[a]), at(k + 1, h[a]), at(k, h[a])], n));
            }
            let (mut up, mut down) = ([0.0f32; 3], [0.0f32; 3]);
            up[a] = 1.0;
            down[a] = -1.0;
            faces.push(((0..N).map(|k| at(k, h[a])).collect(), up));
            faces.push(((0..N).rev().map(|k| at(k, l[a])).collect(), down));
            faces
        }
    }
}

/// Is `p` inside the polygon (even-odd)?
fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// A drawn face: how near, its outline, colour and the part it belongs to.
struct Face {
    w: f32,
    pts: Vec<Pos2>,
    color: Color32,
    part: usize,
    dark: Color32,
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
        let parts: Vec<(String, String, String, f64)> = board.parts.iter().map(|p| (p.refdes.clone(), p.value.clone(), p.footprint.clone(), part_total_height(&p.refdes, &p.footprint))).collect();
        let boxes = part_boxes(board);
        let mut tallest: [Option<(String, f64)>; 2] = [None, None];
        for b in &boxes {
            let side = b.bottom as usize;
            let height = if b.bottom { -b.lo[2] } else { b.hi[2] - BOARD_THICKNESS };
            if tallest[side].as_ref().is_none_or(|t| height > t.1) {
                tallest[side] = Some((b.refdes.clone(), height));
            }
        }
        Ok(Scene { pv, tex, outline, tris, lo, hi, boxes, parts, tallest })
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

    /// Every visible face of the solids on one side of the board, far to near.
    fn part_faces(&self, cam: &Cam, bottom: bool) -> Vec<Face> {
        let mut faces: Vec<Face> = vec![];
        for b in self.boxes.iter().filter(|b| b.bottom == bottom) {
            for (corners, normal) in solid_faces(b) {
                let nv = cam.dir(normal);
                if nv[2] <= 1e-4 {
                    continue;
                }
                let proj: Vec<(Pos2, f32)> = corners.iter().map(|p| cam.project(*p)).collect();
                faces.push(Face {
                    w: proj.iter().map(|p| p.1).sum::<f32>() / proj.len() as f32,
                    pts: proj.iter().map(|p| p.0).collect(),
                    color: shade(b.color, light(nv)),
                    dark: shade(b.color, 0.4),
                    part: b.part,
                });
            }
        }
        faces.sort_by(|a, b| a.w.total_cmp(&b.w));
        faces
    }

    fn draw_faces(faces: &[Face], hover: Option<usize>, selected: Option<usize>, painter: &egui::Painter) {
        for f in faces {
            let (fill, edge) = if hover == Some(f.part) {
                // brighter, with a yellow outline
                let c = f.color;
                (Color32::from_rgb(c.r().saturating_add(50), c.g().saturating_add(50), c.b().saturating_add(30)), Stroke::new(1.6, Color32::from_rgb(255, 230, 90)))
            } else if selected == Some(f.part) {
                (f.color, Stroke::new(1.6, Color32::from_rgb(90, 220, 255)))
            } else {
                (f.color, Stroke::new(0.6, f.dark))
            };
            painter.add(egui::Shape::convex_polygon(f.pts.clone(), fill, edge));
        }
    }

    /// Draws the scene far to near; returns the part under `pointer`, if any.
    fn draw(&self, cam: &Cam, v: &View3d, pointer: Option<Pos2>, selected: Option<usize>, painter: &egui::Painter) -> Option<usize> {
        let above = cam.pitch.sin() > 0.0;
        let (near_side, far_side) = (!above, above); // `bottom` flag of the side facing the viewer / hidden behind the board
        let far = if v.show_parts { self.part_faces(cam, far_side) } else { vec![] };
        let near = if v.show_parts { self.part_faces(cam, near_side) } else { vec![] };
        // the part under the pointer: the nearest face that contains it
        let hover = pointer.and_then(|p| near.iter().rev().find(|f| inside(&f.pts, p)).map(|f| f.part));
        // far side parts, then the far face, the edge, the near face, and the near side parts
        Self::draw_faces(&far, None, selected, painter);
        self.face(cam, far_side, painter);
        self.walls(cam, painter);
        self.face(cam, near_side, painter);
        Self::draw_faces(&near, hover, selected, painter);
        if v.show_names {
            for (i, (name, ..)) in self.parts.iter().enumerate() {
                if let Some(b) = self.boxes.iter().filter(|b| b.part == i && b.bottom == near_side).max_by(|a, b| a.hi[2].total_cmp(&b.hi[2])) {
                    let z = if near_side { b.lo[2] } else { b.hi[2] };
                    let (pos, _) = cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, z]);
                    painter.text(pos, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), Color32::WHITE);
                }
            }
        }
        hover
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
        let mut want_help = false;
        let mut pick: Option<usize> = None;
        let selected = if let Some(Item::Part(i)) = self.selected { Some(i) } else { None };
        let v = &mut self.view3d;
        egui::Window::new("3Dで見る").open(&mut open).default_size([900.0, 680.0]).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("上から").on_hover_text("真上から見ます (部品の配置・配線の確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                if ui.button("斜め").on_hover_text("斜め上から見ます (部品の高さや立体感)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.9, Vec2::ZERO);
                }
                if ui.button("横から").on_hover_text("真横から見ます (部品の高さや、ケースに入る高さの確認)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, 0.12, Vec2::ZERO);
                }
                if ui.button("裏から").on_hover_text("基板を裏返して真下から見ます (裏面の部品と配線)").clicked() {
                    (v.yaw, v.pitch, v.pan) = (0.0, -std::f32::consts::FRAC_PI_2, Vec2::ZERO);
                }
                ui.separator();
                ui.checkbox(&mut v.show_parts, "部品").on_hover_text("部品の本体を表示します。形と高さはフットプリントからの目安です");
                ui.checkbox(&mut v.show_names, "部品名").on_hover_text("部品番号 (R1 など) を表示します");
                if ui.checkbox(&mut v.mask, "ソルダーレジスト").on_hover_text("基板の緑色の保護膜。オフにすると、銅箔の配線がそのまま見えます").changed() {
                    retex = true;
                }
                if ui.checkbox(&mut v.silk, "シルク").on_hover_text("白い印刷 (部品の枠や文字)").changed() {
                    retex = true;
                }
                ui.checkbox(&mut v.spin, "自動回転").on_hover_text("ゆっくり回して、いろいろな向きから見せます");
                if ui.button("更新").on_hover_text("基板エディタで変更した内容を反映します").clicked() {
                    stale = true;
                }
                if ui.small_button("操作").on_hover_text("回す・動かす・拡大の操作を開きます").clicked() {
                    want_help = true;
                }
            });
            // (the rotate / pan / zoom operations are in the guide)
            if let Some(e) = &v.error {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return;
            }
            let Some(scene) = &v.scene else { return };
            let (w, h) = (scene.hi.x - scene.lo.x, scene.hi.y - scene.lo.y);
            let mut info = format!("基板 {w:.1} × {h:.1} × {BOARD_THICKNESS} mm");
            if let Some((r, t)) = &scene.tallest[0] {
                info += &format!(" / 表側の最も高い部品 {r} (基板の面から {t:.1} mm)");
            }
            if let Some((r, t)) = &scene.tallest[1] {
                info += &format!(" / 裏側 {r} ({t:.1} mm)");
            }
            ui.small(info);
            let size = ui.available_size().max(Vec2::new(300.0, 240.0));
            let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
            painter.rect_filled(resp.rect, 4.0, Color32::from_rgb(34, 38, 46));
            if resp.dragged_by(egui::PointerButton::Primary) {
                let d = resp.drag_delta();
                v.yaw -= d.x * 0.01;
                v.pitch = (v.pitch + d.y * 0.01).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            }
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                v.pan += resp.drag_delta();
            }
            if resp.double_clicked() {
                (v.yaw, v.pitch, v.zoom, v.pan) = (0.0, 0.9, 1.0, Vec2::ZERO);
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.2, 8.0);
                }
            }
            if v.spin {
                v.yaw += ui.input(|i| i.stable_dt) * 0.5;
                ui.ctx().request_repaint();
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
                origin: resp.rect.center() + v.pan,
            };
            let hover = scene.draw(&cam, v, resp.hover_pos(), selected, &painter);
            if let Some(i) = hover {
                if let Some((r, val, fp, hh)) = scene.parts.get(i) {
                    egui::Area::new(egui::Id::new("view3d_tip")).order(egui::Order::Tooltip).fixed_pos(resp.hover_pos().unwrap_or_default() + Vec2::new(14.0, 14.0)).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.strong(r);
                            ui.label(format!("値: {}", if val.is_empty() { "—" } else { val }));
                            ui.label(format!("フットプリント: {}", if fp.is_empty() { "—" } else { fp }));
                            ui.label(format!("高さの目安: {hh:.1} mm"));
                        });
                    });
                }
                if resp.clicked_by(egui::PointerButton::Primary) {
                    pick = Some(i);
                }
            }
        });
        if retex && let Some(s) = &mut self.view3d.scene {
            s.tex = Scene::textures(&s.pv, ctx, self.view3d.mask, self.view3d.silk);
        }
        if let Some(i) = pick {
            self.selected = Some(Item::Part(i));
            self.multi.clear();
            if let Some(p) = self.board.parts.get(i) {
                self.status = format!("3D表示から {} を選びました (基板エディタで選択中)", p.refdes);
            }
        }
        if want_help {
            self.help_request = Some(crate::help::HelpTab::View3d);
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

    fn solid_box(shape: BodyShape) -> PartBox {
        PartBox { refdes: "U1".into(), part: 0, lo: [0.0, 0.0, 1.6], hi: [4.0, 4.0, 4.0], color: [30, 30, 30], bottom: false, shape }
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
    fn a_box_shows_three_faces_and_a_cylinder_its_visible_half() {
        let c = cam(0.5, 0.9);
        let faces = |b: &PartBox| solid_faces(b).iter().filter(|(_, n)| c.dir(*n)[2] > 1e-4).count();
        assert_eq!(faces(&solid_box(BodyShape::Box)), 3);
        // 16 side facets, about half face the viewer, plus the top cap
        let cyl = faces(&solid_box(BodyShape::Cylinder(2)));
        assert!((8..=10).contains(&cyl), "{cyl}");
        // a cylinder lying along x has a cap at each end and a ring of sides
        assert_eq!(solid_faces(&solid_box(BodyShape::Cylinder(0))).len(), 18);
        // brighter when facing the light than when turned away
        assert!(light([-0.3, 0.4, 0.85]) > light([0.9, -0.3, 0.1]));
    }

    #[test]
    fn point_in_polygon_picks_the_face_under_the_pointer() {
        let sq = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(10.0, 10.0), Pos2::new(0.0, 10.0)];
        assert!(inside(&sq, Pos2::new(5.0, 5.0)) && !inside(&sq, Pos2::new(11.0, 5.0)));
    }

    #[test]
    fn the_window_builds_a_scene_with_part_info_and_the_tallest_part() {
        let ctx = egui::Context::default();
        let mut b = Board::new("round", 40.0, 25.0);
        b.outline = pcb_core::outline::rounded_rect(0.0, 0.0, 40.0, 25.0, 5.0);
        for k in 0..2 {
            let mut pad = Pad::new(Point::new(20.0 + 2.54 * k as f64, 12.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            pad.refdes = "J1".into();
            b.pads.push(pad);
        }
        b.parts.push(pcb_core::Part { refdes: "J1".into(), value: "conn".into(), footprint: "PinHeader_1x2".into(), pos: Point::new(20.0, 12.0), rot: 0, side: Side::Top });
        let s = Scene::build(&b, &ctx, true, true).unwrap();
        assert!(s.boxes.len() >= 3, "plastic and pins");
        assert!(s.tris.len() > 10, "{}", s.tris.len());
        assert_eq!(s.parts[0].0, "J1");
        let (r, t) = s.tallest[0].clone().unwrap();
        assert_eq!(r, "J1");
        assert!(t > 8.0, "the pins make the header the tall part: {t}");
        assert!(s.tallest[1].is_none());
        // a board without an outline is explained
        b.outline.clear();
        assert!(Scene::build(&b, &ctx, true, true).is_err());
    }
}
