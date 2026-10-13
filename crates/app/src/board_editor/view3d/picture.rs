use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Quality {
    /// While the view is moved: fast and a little blurry.
    Draft,
    /// Rendered `n` times over and averaged (smooth edges).
    Fine(usize),
}

/// Index into a picture of `size` samples for the pointer at `p` over the window `rect` (the picture is stretched
/// to the window, so a draft has fewer samples than pixels).
pub(super) fn sample_at(rect: Rect, size: [usize; 2], p: Pos2) -> Option<usize> {
    let (u, v) = ((p.x - rect.min.x) / rect.width().max(1.0), (p.y - rect.min.y) / rect.height().max(1.0));
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return None;
    }
    let (x, y) = ((u * size[0] as f32) as usize, (v * size[1] as f32) as usize);
    (x < size[0] && y < size[1]).then_some(y * size[0] + x)
}

/// One picture of the scene at `ss` x supersampling: the pixels, the part under each, and the size.
/// `hover` and `selected` get an outline (pass `None` for a clean picture to save).
pub(super) fn snapshot(scene: &Scene, rect: Rect, cam: &Cam, show_parts: bool, q: Quality, hover: Option<usize>, selected: Option<usize>) -> (Vec<[u8; 4]>, Vec<u32>, usize, usize) {
    // a draft is drawn at half the size (a quarter of the pixels, no averaging); a fine picture at `n` times, then averaged
    let (s, ss) = match q {
        Quality::Draft => (0.5f32, 1),
        Quality::Fine(n) => (n as f32, n),
    };
    let local = Cam { scale: cam.scale * s, origin: Pos2::new((cam.origin.x - rect.min.x) * s, (cam.origin.y - rect.min.y) * s), ..*cam };
    let (w, h) = ((rect.width().round().max(1.0) * s).round().max(1.0) as usize, (rect.height().round().max(1.0) * s).round().max(1.0) as usize);
    let buf = scene.render(&local, w, h, show_parts, hover);
    let (mut pixels, ids, w, h) = buf.resolve(ss);
    if let Some(s) = selected {
        outline_part(&mut pixels, &ids, w, h, s as u32, [90, 220, 255]);
    }
    if let Some(hv) = hover {
        outline_part(&mut pixels, &ids, w, h, hv as u32, [255, 230, 90]);
    }
    (pixels, ids, w, h)
}
