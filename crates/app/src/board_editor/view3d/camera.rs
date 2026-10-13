use super::*;

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

/// The camera for a window of `rect`: the longest side of the board fills most of it at zoom 1.
pub(super) fn camera(scene: &Scene, rect: Rect, yaw: f32, pitch: f32, zoom: f32, pan: Vec2) -> Cam {
    let longest = (scene.hi.x - scene.lo.x).max(scene.hi.y - scene.lo.y) as f32;
    // the camera stands back three times the board's diagonal
    let extent = ((scene.hi.x - scene.lo.x).hypot(scene.hi.y - scene.lo.y)) as f32;
    Cam {
        yaw,
        pitch,
        scale: 0.95 * rect.width().min(rect.height()) / (longest * 1.15).max(1.0) * zoom,
        center: [((scene.lo.x + scene.hi.x) / 2.0) as f32, ((scene.lo.y + scene.hi.y) / 2.0) as f32, (BOARD_THICKNESS / 2.0) as f32],
        dist: extent * 3.0,
        origin: rect.center() + pan,
    }
}

/// Where to put the camera to look at part `part` closely: `(pitch, zoom, pan)`. A part on the far face turns the
/// board over (looks from below) so it is seen, not hidden behind the board.
pub(super) fn focus_on(scene: &Scene, part: usize, rect: Rect, yaw: f32, pitch: f32) -> Option<(f32, f32, Vec2)> {
    let mut boxes = scene.boxes.iter().filter(|b| b.part == part);
    let first = boxes.next()?;
    let (mut lo, mut hi) = (first.lo, first.hi);
    let mut bottom = first.bottom;
    for b in boxes {
        for k in 0..3 {
            lo[k] = lo[k].min(b.lo[k]);
            hi[k] = hi[k].max(b.hi[k]);
        }
        bottom |= b.bottom;
    }
    let pitch = if bottom && pitch.sin() > 0.0 {
        -0.9
    } else if !bottom && pitch.sin() < 0.0 {
        0.9
    } else {
        pitch
    };
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() as f32;
    let base = camera(scene, rect, yaw, pitch, 1.0, Vec2::ZERO).scale;
    let zoom = (0.4 * rect.width().min(rect.height()) / diag.max(2.0) / base).clamp(0.2, MAX_ZOOM);
    let at = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let (pos, _) = camera(scene, rect, yaw, pitch, zoom, Vec2::ZERO).project(at);
    Some((pitch, zoom, rect.center() - pos))
}
