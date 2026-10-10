//! A small software rasteriser for the 3D view: a depth buffer, perspective-correct textures and flat-shaded
//! solids. Every pixel shows the nearest surface, so faces never show through each other, and the board
//! picture (copper, mask, silk) is sampled from the picture of the fabrication files with the same mapping the
//! parts use. The result is an image (and, for picking, the number of the part under every pixel).

use eframe::egui::Pos2;

/// "No part here" in the picking buffer.
pub(super) const NO_PART: u32 = u32::MAX;

/// What a triangle is painted with.
#[derive(Clone, Copy)]
pub(super) enum Paint {
    /// The picture of the board (index 0 = top, 1 = bottom), looked up by the (u, v) of the corners.
    Texture(usize),
    /// One colour. `part` is what a click on it selects.
    Flat { color: [u8; 3], part: u32 },
}

/// One corner: screen position (in buffer pixels), nearness `q` (1 / distance along the view axis: bigger is
/// nearer, and linear in screen space), and the texture position.
#[derive(Clone, Copy)]
pub(super) struct Vert {
    pub pos: Pos2,
    pub q: f32,
    pub uv: [f32; 2],
}

/// The picture of one side of the board, for sampling.
pub(super) struct Picture<'a> {
    pub w: usize,
    pub h: usize,
    pub rgba: &'a [[u8; 4]],
}

impl Picture<'_> {
    /// Bilinear sample at (u, v) in 0..1 (clamped); fully transparent pixels count as bare board.
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        let x = (u * self.w as f32 - 0.5).clamp(0.0, self.w as f32 - 1.0);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, self.h as f32 - 1.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |i: usize, j: usize| {
            let p = self.rgba[j * self.w + i];
            let a = p[3] as f32 / 255.0;
            // outside the board the picture is transparent: show the bare board instead of black
            [p[0] as f32 * a + 14.0 * (1.0 - a), p[1] as f32 * a + 70.0 * (1.0 - a), p[2] as f32 * a + 44.0 * (1.0 - a)]
        };
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0.0; 3];
        for k in 0..3 {
            out[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
        }
        out
    }
}

pub(super) struct Buffers {
    pub w: usize,
    pub h: usize,
    color: Vec<[u8; 3]>,
    depth: Vec<f32>,
    id: Vec<u32>,
}

impl Buffers {
    pub fn new(w: usize, h: usize, background: [u8; 3]) -> Buffers {
        Buffers { w, h, color: vec![background; w * h], depth: vec![0.0; w * h], id: vec![NO_PART; w * h] }
    }

    /// Fills a triangle (either winding), keeping what is nearer. Textures are interpolated perspective-correct.
    pub fn triangle(&mut self, v: [Vert; 3], paint: Paint, pics: &[Picture; 2]) {
        let area = edge(v[0].pos, v[1].pos, v[2].pos);
        if area.abs() < 1e-6 {
            return;
        }
        let (min_x, max_x) = (v.iter().map(|p| p.pos.x).fold(f32::MAX, f32::min).floor().max(0.0), v.iter().map(|p| p.pos.x).fold(f32::MIN, f32::max).ceil().min(self.w as f32 - 1.0));
        let (min_y, max_y) = (v.iter().map(|p| p.pos.y).fold(f32::MAX, f32::min).floor().max(0.0), v.iter().map(|p| p.pos.y).fold(f32::MIN, f32::max).ceil().min(self.h as f32 - 1.0));
        if min_x > max_x || min_y > max_y {
            return;
        }
        let eps = -1e-4;
        for y in min_y as usize..=max_y as usize {
            for x in min_x as usize..=max_x as usize {
                let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(v[1].pos, v[2].pos, p) / area;
                let w1 = edge(v[2].pos, v[0].pos, p) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < eps || w1 < eps || w2 < eps {
                    continue;
                }
                let q = w0 * v[0].q + w1 * v[1].q + w2 * v[2].q;
                let i = y * self.w + x;
                if q <= self.depth[i] {
                    continue;
                }
                self.depth[i] = q;
                match paint {
                    Paint::Flat { color, part } => {
                        self.color[i] = color;
                        self.id[i] = part;
                    }
                    Paint::Texture(side) => {
                        let u = (w0 * v[0].uv[0] * v[0].q + w1 * v[1].uv[0] * v[1].q + w2 * v[2].uv[0] * v[2].q) / q;
                        let t = (w0 * v[0].uv[1] * v[0].q + w1 * v[1].uv[1] * v[1].q + w2 * v[2].uv[1] * v[2].q) / q;
                        let c = pics[side].sample(u, t);
                        self.color[i] = [c[0] as u8, c[1] as u8, c[2] as u8];
                        self.id[i] = NO_PART;
                    }
                }
            }
        }
    }

    /// A one-pixel line that is hidden by anything nearer (a small bias keeps it on top of its own face).
    pub fn line(&mut self, a: Vert, b: Vert, color: [u8; 3]) {
        let (dx, dy) = (b.pos.x - a.pos.x, b.pos.y - a.pos.y);
        let n = dx.abs().max(dy.abs()).ceil() as usize;
        if n == 0 || n > 20_000 {
            return;
        }
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let (x, y) = ((a.pos.x + dx * t).floor(), (a.pos.y + dy * t).floor());
            if x < 0.0 || y < 0.0 || x >= self.w as f32 || y >= self.h as f32 {
                continue;
            }
            let i = y as usize * self.w + x as usize;
            let q = a.q + (b.q - a.q) * t;
            if q * 1.002 >= self.depth[i] && self.depth[i] > 0.0 {
                self.color[i] = color;
            }
        }
    }

    /// Averages `ss` x `ss` samples into one pixel. Returns the pixels and, per pixel, the part at the middle sample.
    pub fn resolve(&self, ss: usize) -> (Vec<[u8; 4]>, Vec<u32>, usize, usize) {
        let (w, h) = (self.w / ss, self.h / ss);
        let mut img = vec![[0u8; 4]; w * h];
        let mut ids = vec![NO_PART; w * h];
        for j in 0..h {
            for i in 0..w {
                let mut acc = [0u32; 3];
                for dj in 0..ss {
                    for di in 0..ss {
                        let c = self.color[(j * ss + dj) * self.w + i * ss + di];
                        for k in 0..3 {
                            acc[k] += c[k] as u32;
                        }
                    }
                }
                let n = (ss * ss) as u32;
                img[j * w + i] = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255];
                ids[j * w + i] = self.id[(j * ss + ss / 2) * self.w + i * ss + ss / 2];
            }
        }
        (img, ids, w, h)
    }
}

fn edge(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Paints a one-pixel-wide ring of `color` around the pixels whose part is `part` (the part under the pointer, or the
/// selected one).
pub(super) fn outline_part(img: &mut [[u8; 4]], ids: &[u32], w: usize, h: usize, part: u32, color: [u8; 3]) {
    let mut marks = vec![];
    for j in 1..h.saturating_sub(1) {
        for i in 1..w.saturating_sub(1) {
            if ids[j * w + i] == part && [ids[j * w + i - 1], ids[j * w + i + 1], ids[(j - 1) * w + i], ids[(j + 1) * w + i]].iter().any(|n| *n != part) {
                marks.push(j * w + i);
            }
        }
    }
    for i in marks {
        img[i] = [color[0], color[1], color[2], 255];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pics() -> [Picture<'static>; 2] {
        static PIX: [[u8; 4]; 4] = [[200, 0, 0, 255], [0, 200, 0, 255], [0, 0, 200, 255], [200, 200, 0, 255]];
        [Picture { w: 2, h: 2, rgba: &PIX }, Picture { w: 2, h: 2, rgba: &PIX }]
    }

    fn v(x: f32, y: f32, q: f32) -> Vert {
        Vert { pos: Pos2::new(x, y), q, uv: [0.0, 0.0] }
    }

    #[test]
    fn the_nearer_triangle_wins_whatever_the_drawing_order() {
        let p = pics();
        let red = Paint::Flat { color: [255, 0, 0], part: 1 };
        let blue = Paint::Flat { color: [0, 0, 255], part: 2 };
        for near_first in [true, false] {
            let mut b = Buffers::new(20, 20, [0, 0, 0]);
            let near = [v(2.0, 2.0, 2.0), v(18.0, 2.0, 2.0), v(2.0, 18.0, 2.0)];
            let far = [v(0.0, 0.0, 1.0), v(20.0, 0.0, 1.0), v(0.0, 20.0, 1.0)];
            if near_first {
                b.triangle(near, red, &p);
                b.triangle(far, blue, &p);
            } else {
                b.triangle(far, blue, &p);
                b.triangle(near, red, &p);
            }
            let (img, ids, w, _) = b.resolve(1);
            assert_eq!(img[5 * w + 5], [255, 0, 0, 255], "near_first = {near_first}");
            assert_eq!(ids[5 * w + 5], 1);
            // outside the near triangle, inside the far one
            assert_eq!(img[10 * w + 1], [0, 0, 255, 255]);
            // outside both
            assert_eq!(img[18 * w + 18], [0, 0, 0, 255]);
        }
    }

    #[test]
    fn both_windings_are_filled_and_a_texture_is_mapped_by_uv() {
        let p = pics();
        let mut b = Buffers::new(20, 20, [0, 0, 0]);
        // the left half of the picture is red/blue, the right half green/yellow: map the whole picture on a square
        let corner = |x: f32, y: f32, u: f32, t: f32| Vert { pos: Pos2::new(x, y), q: 1.0, uv: [u, t] };
        b.triangle([corner(0.0, 0.0, 0.0, 0.0), corner(20.0, 0.0, 1.0, 0.0), corner(0.0, 20.0, 0.0, 1.0)], Paint::Texture(0), &p);
        b.triangle([corner(20.0, 20.0, 1.0, 1.0), corner(0.0, 20.0, 0.0, 1.0), corner(20.0, 0.0, 1.0, 0.0)], Paint::Texture(0), &p);
        let (img, ids, w, _) = b.resolve(1);
        assert!(img[3 * w + 3][0] > 150 && img[3 * w + 3][1] < 60, "top left is red: {:?}", img[3 * w + 3]);
        assert!(img[3 * w + 16][1] > 150 && img[3 * w + 16][0] < 60, "top right is green: {:?}", img[3 * w + 16]);
        assert!(img[16 * w + 3][2] > 150, "bottom left is blue: {:?}", img[16 * w + 3]);
        assert_eq!(ids[3 * w + 3], NO_PART);
    }

    #[test]
    fn supersampling_smooths_an_edge_and_the_outline_marks_the_part() {
        let p = pics();
        let mut b = Buffers::new(40, 40, [0, 0, 0]);
        b.triangle([v(0.0, 0.0, 1.0), v(40.0, 0.0, 1.0), v(0.0, 40.0, 1.0)], Paint::Flat { color: [200, 200, 200], part: 7 }, &p);
        let (mut img, ids, w, h) = b.resolve(2);
        // along the diagonal some pixels are only partly covered
        assert!(img.iter().any(|c| c[0] > 0 && c[0] < 200), "an anti-aliased edge has in-between values");
        outline_part(&mut img, &ids, w, h, 7, [255, 230, 90]);
        assert!(img.iter().any(|c| *c == [255, 230, 90, 255]));
    }
}
