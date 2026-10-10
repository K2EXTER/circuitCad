//! "3Dで見る" window: the board as the fab will make it (the picture rebuilt from the exported
//! fabrication files, put on the top and bottom faces) with solids for every part. Drawn by a small
//! software rasteriser (`raster3d`): perspective projection, back-face culling and a depth buffer, so what is
//! nearer always hides what is behind it. Hovering a part names it, clicking selects it in the board editor.

use super::raster3d::{outline_part, Buffers, Paint, Picture, Vert, NO_PART};
use super::*;
use pcb_core::gerber_view::Preview;
use pcb_core::model3d::{part_boxes, part_total_height, BodyShape, PartBox, BOARD_THICKNESS};

/// What the window shows, built from the board when it is opened or refreshed.
struct Scene {
    pv: Preview,
    /// The board picture of the top and of the bottom (what the faces are painted with).
    img: [Vec<[u8; 4]>; 2],
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
    /// The part under the pointer in the last picture (drawn brighter in the next one).
    hover: Option<usize>,
    /// Changes whenever the scene or its pictures change, so a kept picture is not reused for another.
    generation: u32,
    /// The last picture, kept while nothing that shows in it changes.
    cache: Option<Cached>,
}

/// A finished picture of the scene: what it was drawn for, the image, and the part at each pixel.
struct Cached {
    key: PictureKey,
    tex: egui::TextureHandle,
    ids: Vec<u32>,
    size: [usize; 2],
}

#[derive(PartialEq)]
struct PictureKey {
    camera: [u32; 5],
    size: [usize; 2],
    parts: bool,
    hover: Option<usize>,
    selected: Option<usize>,
    generation: u32,
}

impl Default for View3d {
    fn default() -> Self {
        Self { open: false, yaw: 0.0, pitch: 0.9, zoom: 1.0, pan: Vec2::ZERO, spin: false, show_parts: true, show_names: false, mask: true, silk: true, scene: None, error: None, stale: true, hover: None, generation: 0, cache: None }
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
            const N: usize = 32;
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

fn shade3(c: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.3);
    [(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8]
}

impl Scene {
    fn build(board: &Board, ctx: &egui::Context, mask: bool, silk: bool) -> Result<Scene, String> {
        let _ = ctx;
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
        let img = Self::images(&pv, mask, silk);
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
        Ok(Scene { pv, img, outline, tris, lo, hi, boxes, parts, tallest })
    }

    fn images(pv: &Preview, mask: bool, silk: bool) -> [Vec<[u8; 4]>; 2] {
        [pv.render(false, mask, silk), pv.render(true, mask, silk)]
    }

    /// The whole scene into a depth-buffered image of `w` x `h` samples. `cam` must already be in sample units
    /// (its origin relative to the image). `hover` is drawn brighter.
    fn render(&self, cam: &Cam, w: usize, h: usize, show_parts: bool, hover: Option<usize>) -> Buffers {
        let mut buf = Buffers::new(w, h, [34, 38, 46]);
        let pics = [
            Picture { w: self.pv.frame.w, h: self.pv.frame.h, rgba: &self.img[0] },
            Picture { w: self.pv.frame.w, h: self.pv.frame.h, rgba: &self.img[1] },
        ];
        let vert = |p: [f64; 3], uv: [f32; 2]| {
            let (pos, depth) = cam.project(p);
            Vert { pos, q: 1.0 / (cam.dist - depth).max(cam.dist * 0.2), uv }
        };
        // the top and bottom faces: the board picture mapped by the picture's own frame (it has a margin around the
        // outline), so the copper lies exactly where the parts' pads are
        let fr = self.pv.frame;
        for bottom in [false, true] {
            let up = if bottom { -1.0 } else { 1.0 };
            if cam.dir([0.0, 0.0, up])[2] <= 1e-4 {
                continue;
            }
            let z = if bottom { 0.0 } else { BOARD_THICKNESS };
            for t in &self.tris {
                let corner = |p: &Point| {
                    let u = ((p.x - fr.x0) * fr.px / fr.w as f64) as f32;
                    let v = ((fr.y1 - p.y) * fr.px / fr.h as f64) as f32;
                    // the bottom picture is the view from the back: mirrored left to right
                    vert([p.x, p.y, z], [if bottom { 1.0 - u } else { u }, v])
                };
                buf.triangle([corner(&t[0]), corner(&t[1]), corner(&t[2])], Paint::Texture(bottom as usize), &pics);
            }
        }
        // the edge of the board
        let n = self.outline.len();
        for i in 0..n {
            let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            // counter-clockwise outline: the outside is to the right of the direction of travel
            let nv = cam.dir([(dy / len) as f32, (-dx / len) as f32, 0.0]);
            if nv[2] <= 1e-4 {
                continue;
            }
            let c = |x: f64, y: f64, z: f64| vert([x, y, z], [0.0, 0.0]);
            let (p0, p1, p2, p3) = (c(a.x, a.y, 0.0), c(b.x, b.y, 0.0), c(b.x, b.y, BOARD_THICKNESS), c(a.x, a.y, BOARD_THICKNESS));
            let paint = Paint::Flat { color: shade3([176, 150, 92], light(nv)), part: NO_PART };
            buf.triangle([p0, p1, p2], paint, &pics);
            buf.triangle([p0, p2, p3], paint, &pics);
        }
        if !show_parts {
            return buf;
        }
        for b in &self.boxes {
            for (corners, normal) in solid_faces(b) {
                let nv = cam.dir(normal);
                if nv[2] <= 1e-4 {
                    continue;
                }
                let mut color = shade3(b.color, light(nv));
                if hover == Some(b.part) {
                    color = [color[0].saturating_add(50), color[1].saturating_add(50), color[2].saturating_add(30)];
                }
                let vs: Vec<Vert> = corners.iter().map(|p| vert(*p, [0.0, 0.0])).collect();
                let paint = Paint::Flat { color, part: b.part as u32 };
                for k in 1..vs.len().saturating_sub(1) {
                    buf.triangle([vs[0], vs[k], vs[k + 1]], paint, &pics);
                }
                // a thin dark edge round every face, so the shape reads even where two faces are lit alike
                let dark = shade3(b.color, 0.4);
                for k in 0..vs.len() {
                    buf.line(vs[k], vs[(k + 1) % vs.len()], dark);
                }
            }
        }
        buf
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
            self.view3d.generation += 1;
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
            // the picture: drawn at twice the size and averaged (release builds), and kept while nothing in it changes
            let ss = if cfg!(debug_assertions) { 1 } else { 2 };
            let size_px = [resp.rect.width().round().max(1.0) as usize, resp.rect.height().round().max(1.0) as usize];
            let key = PictureKey {
                camera: [v.yaw.to_bits(), v.pitch.to_bits(), v.zoom.to_bits(), v.pan.x.to_bits(), v.pan.y.to_bits()],
                size: size_px,
                parts: v.show_parts,
                hover: v.hover,
                selected,
                generation: v.generation,
            };
            if v.cache.as_ref().is_none_or(|c| c.key != key) {
                let s = ss as f32;
                let local = Cam { scale: cam.scale * s, origin: Pos2::new((cam.origin.x - resp.rect.min.x) * s, (cam.origin.y - resp.rect.min.y) * s), ..cam };
                let buf = scene.render(&local, size_px[0] * ss, size_px[1] * ss, v.show_parts, v.hover);
                let (mut pixels, ids, w, h) = buf.resolve(ss);
                if let Some(s) = selected {
                    outline_part(&mut pixels, &ids, w, h, s as u32, [90, 220, 255]);
                }
                if let Some(hv) = v.hover {
                    outline_part(&mut pixels, &ids, w, h, hv as u32, [255, 230, 90]);
                }
                let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
                let tex = ui.ctx().load_texture("board3d_view", egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR);
                v.cache = Some(Cached { key, tex, ids, size: [w, h] });
            }
            let mut hover = None;
            if let Some(c) = &v.cache {
                painter.image(c.tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if let Some(p) = resp.hover_pos() {
                    let (x, y) = ((p.x - resp.rect.min.x) as usize, (p.y - resp.rect.min.y) as usize);
                    if x < c.size[0] && y < c.size[1] {
                        let id = c.ids[y * c.size[0] + x];
                        hover = (id != NO_PART && v.show_parts).then_some(id as usize);
                    }
                }
            }
            if hover != v.hover {
                // the part under the pointer changed: draw it brighter in the next picture
                v.hover = hover;
                ui.ctx().request_repaint();
            }
            if v.show_names {
                let near_side = cam.pitch.sin() <= 0.0;
                for (i, (name, ..)) in scene.parts.iter().enumerate() {
                    if let Some(b) = scene.boxes.iter().filter(|b| b.part == i && b.bottom == near_side).max_by(|a, b| a.hi[2].total_cmp(&b.hi[2])) {
                        let z = if near_side { b.lo[2] } else { b.hi[2] };
                        let (pos, _) = cam.project([(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, z]);
                        painter.text(pos, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), Color32::WHITE);
                    }
                }
            }
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
            s.img = Scene::images(&s.pv, self.view3d.mask, self.view3d.silk);
            self.view3d.generation += 1;
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
        // 32 side facets, about half face the viewer, plus the top cap
        let cyl = faces(&solid_box(BodyShape::Cylinder(2)));
        assert!((16..=18).contains(&cyl), "{cyl}");
        // a cylinder lying along x has a cap at each end and a ring of sides
        assert_eq!(solid_faces(&solid_box(BodyShape::Cylinder(0))).len(), 34);
        // brighter when facing the light than when turned away
        assert!(light([-0.3, 0.4, 0.85]) > light([0.9, -0.3, 0.1]));
    }

    /// The board picture lies where the parts' pads are: looking straight down, the copper of a pad is under the
    /// pad's position in the 3D scene (the picture has a margin around the outline, which the mapping must account for).
    #[test]
    fn the_board_picture_and_the_part_positions_agree() {
        let ctx = egui::Context::default();
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, name) in [(8.0, "R1"), (30.0, "R2")] {
            for dx in [-1.0, 1.0] {
                let mut pad = Pad::new(Point::new(x + dx, 12.0), PadShape::Rect { w: 1.2, h: 1.6 }, None, Side::Top);
                pad.refdes = name.into();
                b.pads.push(pad);
            }
            b.parts.push(pcb_core::Part { refdes: name.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, 12.0), rot: 0, side: Side::Top });
        }
        let s = Scene::build(&b, &ctx, false, false).unwrap();
        let (w, h) = (800usize, 500usize);
        let cam = Cam { yaw: 0.0, pitch: std::f32::consts::FRAC_PI_2, scale: 16.0, center: [20.0, 12.5, 0.8], dist: 300.0, origin: Pos2::new(w as f32 / 2.0, h as f32 / 2.0) };
        let (img, _, w, _) = s.render(&cam, w, h, false, None).resolve(1);
        let at = |x: f64, y: f64| {
            let (p, _) = cam.project([x, y, BOARD_THICKNESS]);
            img[p.y.round() as usize * w + p.x.round() as usize]
        };
        let gold = |c: [u8; 4]| c[0] > 180 && c[1] > 140 && c[2] < 120;
        for pad in &b.pads {
            assert!(gold(at(pad.pos.x, pad.pos.y)), "no copper under the pad at {:?}: {:?}", pad.pos, at(pad.pos.x, pad.pos.y));
        }
        // between the two pads of a part and away from them it is bare board
        assert!(!gold(at(8.0, 12.0)) && !gold(at(20.0, 20.0)));
        // the copper of a pad ends where the pad ends (0.6 mm from its centre, but not 1 mm)
        assert!(gold(at(7.0 + 0.5, 12.0)) && !gold(at(7.0 - 1.0, 12.0)));
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
