//! Setting the scale of a drawing cut out of the data sheet: two clicks on the ends of a dimension line and its length (mm or inch).
//! The small picture in the form takes the clicks, and "大きく開く" shows it in a window of its own that zooms with the wheel, pans with
//! a drag, shows a magnified loupe under the pointer, and lets the two points be dragged into place.

use super::solidcraft::Drawing;
use super::*;

/// How the picture lies on the screen: screen = origin + pixel × scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PickView {
    pub origin: Pos2,
    pub scale: f32,
}

impl PickView {
    /// The picture of `size` pixels fitted into `rect`, magnified by `zoom` and moved by `pan` (screen points).
    pub fn fit(rect: Rect, size: [usize; 2], zoom: f32, pan: Vec2) -> PickView {
        let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
        let scale = (rect.width() / w).min(rect.height() / h) * zoom;
        PickView { origin: rect.center() + pan - Vec2::new(w, h) * scale / 2.0, scale }
    }

    pub fn to_screen(self, p: [f32; 2]) -> Pos2 {
        self.origin + Vec2::new(p[0], p[1]) * self.scale
    }

    pub fn to_pixel(self, s: Pos2) -> [f32; 2] {
        let p = (s - self.origin) / self.scale;
        [p.x, p.y]
    }
}

/// The zoom and pan after the wheel turned the zoom from `zoom` to `new_zoom` with the pointer at `at`: the pixel under the pointer
/// stays under it.
pub(super) fn zoom_about(rect: Rect, size: [usize; 2], zoom: f32, pan: Vec2, new_zoom: f32, at: Pos2) -> Vec2 {
    let before = PickView::fit(rect, size, zoom, pan);
    let pixel = before.to_pixel(at);
    let after = PickView::fit(rect, size, new_zoom, pan);
    pan + (at - after.to_screen(pixel))
}

/// The largest zoom of the big window (pixels of the cut are already 2 or 3 times the printed size).
const MAX_ZOOM: f32 = 12.0;

/// The picture with its two points: clicks set them (a third click starts again), and in the big window the wheel zooms, a drag pans,
/// a drag that starts on a point moves it, and a loupe magnifies what is under the pointer.
fn pick_area(ui: &mut egui::Ui, d: &mut Drawing, size: Vec2, big: bool) {
    let tex = d.texture(ui.ctx());
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 2.0, Color32::from_gray(245));
    let (zoom, pan) = if big { (d.zoom, d.pan) } else { (1.0, Vec2::ZERO) };
    let view = PickView::fit(rect, d.size, zoom, pan);
    if big
        && resp.hovered()
        && let Some(at) = resp.hover_pos()
    {
        let scroll = crate::prefs::zoom_scroll(ui);
        if scroll != 0.0 {
            let nz = (d.zoom * (1.0 + scroll * 0.002)).clamp(1.0, MAX_ZOOM);
            d.pan = zoom_about(rect, d.size, d.zoom, d.pan, nz, at);
            d.zoom = nz;
        }
    }
    let view = if big { PickView::fit(rect, d.size, d.zoom, d.pan) } else { view };
    let img_rect = Rect::from_min_max(view.to_screen([0.0, 0.0]), view.to_screen([d.size[0] as f32, d.size[1] as f32]));
    painter.image(tex.id(), img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // a drag that starts on a point moves it; elsewhere (big window) it pans
    if resp.drag_started()
        && let Some(at) = resp.interact_pointer_pos()
    {
        d.dragging = [d.a, d.b].iter().enumerate().filter_map(|(k, p)| p.map(|p| (k, view.to_screen(p).distance(at)))).filter(|(_, dist)| *dist < 10.0).min_by(|x, y| x.1.total_cmp(&y.1)).map(|(k, _)| k as u8);
    }
    if resp.dragged()
        && let Some(at) = resp.interact_pointer_pos()
    {
        match d.dragging {
            Some(0) => d.a = Some(view.to_pixel(at)),
            Some(_) => d.b = Some(view.to_pixel(at)),
            None if big => d.pan += resp.drag_delta(),
            None => {}
        }
    }
    if resp.drag_stopped() {
        d.dragging = None;
    }
    if resp.clicked()
        && let Some(at) = resp.interact_pointer_pos()
    {
        let p = view.to_pixel(at);
        match (d.a, d.b) {
            (Some(_), None) => d.b = Some(p),
            _ => {
                d.a = Some(p);
                d.b = None;
            }
        }
    }
    let red = Color32::from_rgb(230, 40, 40);
    if let (Some(a), Some(b)) = (d.a, d.b) {
        painter.line_segment([view.to_screen(a), view.to_screen(b)], Stroke::new(1.5, red));
    }
    for (k, p) in [d.a, d.b].into_iter().enumerate() {
        if let Some(p) = p {
            let s = view.to_screen(p);
            painter.circle_stroke(s, if big { 6.0 } else { 4.0 }, Stroke::new(1.5, red));
            if big {
                painter.line_segment([s - Vec2::X * 10.0, s + Vec2::X * 10.0], Stroke::new(1.0, red));
                painter.line_segment([s - Vec2::Y * 10.0, s + Vec2::Y * 10.0], Stroke::new(1.0, red));
                painter.text(s + Vec2::new(8.0, -8.0), egui::Align2::LEFT_BOTTOM, if k == 0 { "1" } else { "2" }, crate::theme::canvas_font(&painter, 13.0), red);
            }
        }
    }
    // the loupe: 5 times what is under the pointer, beside it, with a cross on the very pixel
    if big
        && let Some(at) = resp.hover_pos()
    {
        const LOUPE: f32 = 130.0;
        const MAG: f32 = 5.0;
        let half_px = LOUPE / 2.0 / (view.scale * MAG);
        let c = view.to_pixel(at);
        let uv = Rect::from_min_max(Pos2::new((c[0] - half_px) / d.size[0] as f32, (c[1] - half_px) / d.size[1] as f32), Pos2::new((c[0] + half_px) / d.size[0] as f32, (c[1] + half_px) / d.size[1] as f32));
        // beside the pointer, on the side with room
        let dx = if at.x + 20.0 + LOUPE < rect.right() { 20.0 } else { -20.0 - LOUPE };
        let dy = if at.y + 20.0 + LOUPE < rect.bottom() { 20.0 } else { -20.0 - LOUPE };
        let loupe = Rect::from_min_size(at + Vec2::new(dx, dy), Vec2::splat(LOUPE));
        let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drawing_loupe")));
        top.rect_filled(loupe, 2.0, Color32::from_gray(245));
        top.image(tex.id(), loupe, uv, Color32::WHITE);
        top.rect_stroke(loupe, 2.0, Stroke::new(1.5, Color32::from_gray(60)), egui::StrokeKind::Outside);
        top.line_segment([loupe.center() - Vec2::X * 12.0, loupe.center() + Vec2::X * 12.0], Stroke::new(1.0, red));
        top.line_segment([loupe.center() - Vec2::Y * 12.0, loupe.center() + Vec2::Y * 12.0], Stroke::new(1.0, red));
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if !big {
        resp.on_hover_text(i18n::tr("寸法の分かる線 (寸法線の両端の矢印など) の、片方の端をクリックし、もう片方の端をクリックします。3 回目のクリックでやり直します。小さくて選びにくいときは「大きく開く」"));
    }
}

/// The length between the two points: the number and its unit.
fn length_row(ui: &mut egui::Ui, d: &mut Drawing) {
    ui.horizontal(|ui| {
        ui.label(i18n::tr("2 点の間の長さ"));
        crate::widgets::unless(ui, (!d.b.is_some()).then_some("先に図の上で 2 点目をクリックしてください"), egui::DragValue::new(&mut d.dist).range(0.0..=500.0).speed(if d.inch { 0.001 } else { 0.05 }).max_decimals(4).suffix(if d.inch { " in" } else { " mm" }))
            .on_hover_text(i18n::tr("クリックした 2 点の、本当の長さ (図の寸法の数字)。これで図の縮尺が決まります"));
        ui.selectable_value(&mut d.inch, false, "mm").on_hover_text(i18n::tr("図の寸法の数字がミリメートルのとき"));
        ui.selectable_value(&mut d.inch, true, "inch").on_hover_text(i18n::tr("図の寸法の数字がインチのとき (1 inch = 25.4 mm。「0.100」のような数字や、mm の横の [ ] の中の数字)。数字はそのまま、単位だけが変わります"));
        if d.inch && d.dist > 0.0 {
            ui.small(format!("= {:.3} mm", d.dist_mm()));
        }
    });
}

/// The drawing in the form: the small picture, the length, and the button to the big window (drawn by `big_window`).
pub(super) fn calibrate_ui(ui: &mut egui::Ui, d: &mut Drawing) {
    let width = ui.available_width().min(480.0);
    let size = Vec2::new(width, width * d.size[1] as f32 / d.size[0].max(1) as f32);
    pick_area(ui, d, size, false);
    ui.horizontal(|ui| {
        if ui.button(i18n::tr("大きく開く")).on_hover_text(i18n::tr("図を別の窓で大きく表示します。ホイールで拡大、ドラッグで移動、点はドラッグで動かせます。ポインタの下は拡大鏡で見えます")).clicked() {
            d.big = true;
        }
        length_row(ui, d);
    });
}

/// The big window of a drawing, while it is open.
pub(super) fn big_window(ctx: &egui::Context, d: &mut Drawing) {
    if !d.big {
        return;
    }
    let mut open = true;
    egui::Window::new(i18n::trf!("{} の縮尺を合わせる", i18n::tr(d.view.label()))).id(egui::Id::new(("drawing_big", d.view.label()))).open(&mut open).default_size([900.0, 640.0]).resizable(true).show(ctx, |ui| {
        ui.small(i18n::tr("寸法線の両端を 1 回ずつクリック (3 回目でやり直し)。ホイール: 拡大・縮小  ドラッグ: 移動  点の上からドラッグ: 点を動かす"));
        ui.horizontal(|ui| {
            length_row(ui, d);
            if ui.small_button(i18n::tr("全体を表示")).on_hover_text(i18n::tr("拡大と移動を元に戻して、図の全体を表示します")).clicked() {
                (d.zoom, d.pan) = (1.0, Vec2::ZERO);
            }
            if ui.button(i18n::tr("閉じる")).clicked() {
                d.big = false;
            }
        });
        let size = ui.available_size().max(Vec2::new(300.0, 200.0));
        pick_area(ui, d, size, true);
    });
    if !open {
        d.big = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_fits_and_zooming_keeps_the_pixel_under_the_pointer() {
        let rect = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(400.0, 300.0));
        // 200 x 100 pixels: twice as wide as tall, so the width decides (scale 2)
        let v = PickView::fit(rect, [200, 100], 1.0, Vec2::ZERO);
        assert_eq!(v.scale, 2.0);
        assert_eq!(v.to_screen([100.0, 50.0]), rect.center());
        assert_eq!(v.to_pixel(v.to_screen([12.0, 34.0])), [12.0, 34.0]);
        // zoom 4 times at a point off the centre: that pixel stays under the pointer
        let at = Pos2::new(180.0, 120.0);
        let pixel = v.to_pixel(at);
        let pan = zoom_about(rect, [200, 100], 1.0, Vec2::ZERO, 4.0, at);
        let w = PickView::fit(rect, [200, 100], 4.0, pan);
        let back = w.to_screen(pixel);
        assert!((back - at).length() < 1e-3, "{back:?}");
        assert_eq!(w.scale, 8.0);
    }
}
