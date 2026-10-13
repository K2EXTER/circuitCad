//! Reads the fabrication files back and draws them as a picture of the finished board.
//!
//! This is a deliberately small RS-274X reader: exactly what the writer in `gerber.rs` produces
//! (round / rectangular / obround apertures, lines, flashes, regions, dark and clear polarity)
//! plus Excellon drill files. The point is the check: what the fab will see is rebuilt from the
//! files themselves, not from the design in memory.

use crate::model::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aperture {
    Circle(f64),
    Rect(f64, f64),
    Oval(f64, f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Flash { ap: Aperture, at: Point, clear: bool },
    Line { a: Point, b: Point, width: f64, clear: bool },
    Region { pts: Vec<Point>, clear: bool },
}

/// Parse a Gerber file into drawing primitives, in drawing order.
pub fn parse_gerber(text: &str) -> Result<Vec<Prim>, String> {
    let mut aps: Vec<(u32, Aperture)> = vec![];
    let mut cur: Option<Aperture> = None;
    let mut at = Point::default();
    let mut clear = false;
    let mut region: Option<Vec<Point>> = None;
    let mut out = vec![];
    let mut decimals = 6u32;

    let flush = |region: &mut Option<Vec<Point>>, out: &mut Vec<Prim>, clear: bool| {
        if let Some(pts) = region.take()
            && pts.len() >= 3 {
                out.push(Prim::Region { pts, clear });
            }
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(body) = line.strip_prefix('%') {
            let body = body.trim_end_matches('%').trim_end_matches('*');
            if let Some(fs) = body.strip_prefix("FSLAX") {
                // FSLAX46Y46: two digits for the integer part, then the decimals
                decimals = fs.chars().nth(1).and_then(|c| c.to_digit(10)).ok_or("FS の書式を読めません")?;
            } else if let Some(ad) = body.strip_prefix("ADD") {
                let (num, rest) = ad.split_at(ad.find(|c: char| !c.is_ascii_digit()).ok_or("ADD の書式が不正です")?);
                let code: u32 = num.parse().map_err(|_| "アパーチャ番号が不正です")?;
                let (kind, params) = rest.split_at(1);
                let nums: Vec<f64> = params.trim_start_matches(',').split('X').map(|s| s.parse::<f64>().map_err(|_| format!("アパーチャの寸法が不正です: {s}"))).collect::<Result<_, _>>()?;
                let ap = match (kind, nums.as_slice()) {
                    ("C", [d, ..]) => Aperture::Circle(*d),
                    ("R", [w, h, ..]) => Aperture::Rect(*w, *h),
                    ("O", [w, h, ..]) => Aperture::Oval(*w, *h),
                    _ => return Err(format!("未対応のアパーチャです: {rest}")),
                };
                aps.push((code, ap));
            } else if body == "LPD" || body == "LPC" {
                flush(&mut region, &mut out, clear);
                clear = body == "LPC";
            }
            continue;
        }
        let cmd = line.trim_end_matches('*');
        if cmd.starts_with("G04") || cmd == "G01" || cmd == "M02" || cmd == "M00" {
            continue;
        }
        if cmd == "G36" {
            region = Some(vec![]);
            continue;
        }
        if cmd == "G37" {
            flush(&mut region, &mut out, clear);
            continue;
        }
        // Dnn (select aperture), or X..Y..D01/D02/D03
        if let Some(code) = cmd.strip_prefix('D').and_then(|c| c.parse::<u32>().ok()) {
            if code >= 10 {
                cur = Some(aps.iter().find(|(c, _)| *c == code).ok_or_else(|| format!("D{code} は定義されていません"))?.1);
            }
            continue;
        }
        if cmd.starts_with('X') || cmd.starts_with('Y') {
            let scale = 10f64.powi(decimals as i32);
            let num = |c: char, from: &str| -> Option<f64> {
                let i = from.find(c)? + 1;
                let end = from[i..].find(|ch: char| !(ch.is_ascii_digit() || ch == '-' || ch == '+')).map_or(from.len(), |e| i + e);
                from[i..end].parse::<f64>().ok().map(|v| v / scale)
            };
            let d_pos = cmd.rfind('D').ok_or_else(|| format!("D 命令がありません: {cmd}"))?;
            let (coords, op) = cmd.split_at(d_pos);
            let to = Point::new(num('X', coords).unwrap_or(at.x), num('Y', coords).unwrap_or(at.y));
            match op {
                "D02" => {
                    if let Some(pts) = region.as_mut() {
                        // a new contour inside a region
                        if pts.len() >= 3 {
                            out.push(Prim::Region { pts: std::mem::take(pts), clear });
                        }
                        pts.clear();
                        pts.push(to);
                    }
                }
                "D01" => {
                    if let Some(pts) = region.as_mut() {
                        pts.push(to);
                    } else {
                        let Some(Aperture::Circle(w)) = cur else { return Err("線は円形のアパーチャで描きます".into()) };
                        out.push(Prim::Line { a: at, b: to, width: w, clear });
                    }
                }
                "D03" => {
                    let ap = cur.ok_or("アパーチャが選ばれていません")?;
                    out.push(Prim::Flash { ap, at: to, clear });
                }
                other => return Err(format!("未対応の命令です: {other}")),
            }
            at = to;
            continue;
        }
        return Err(format!("読めない行です: {cmd}"));
    }
    Ok(out)
}

/// Drill hits `(diameter, position)` from an Excellon file (a slot gives its start point).
pub fn parse_excellon(text: &str) -> Result<Vec<(f64, Point)>, String> {
    Ok(parse_excellon_slots(text)?.into_iter().map(|(d, p, _)| (d, p)).collect())
}

/// Drill hits `(diameter, position, other end of a slot)` from an Excellon file; slots are `X..Y..G85X..Y..`.
pub fn parse_excellon_slots(text: &str) -> Result<Vec<(f64, Point, Option<Point>)>, String> {
    let mut tools: Vec<(u32, f64)> = vec![];
    let mut cur = None;
    let mut out = vec![];
    let mut in_header = true;
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with(';') {
            continue;
        }
        if l == "%" {
            in_header = false;
            continue;
        }
        if let Some(rest) = l.strip_prefix('T') {
            if let Some((n, d)) = rest.split_once('C') {
                if in_header {
                    tools.push((n.parse().map_err(|_| "ドリル番号が不正です")?, d.parse().map_err(|_| "ドリル径が不正です")?));
                    continue;
                }
            } else if let Ok(n) = rest.parse::<u32>() {
                cur = if n == 0 { None } else { tools.iter().find(|(t, _)| *t == n).map(|(_, d)| *d) };
                continue;
            }
        }
        if l.starts_with('X') {
            let d = cur.ok_or("ドリルが選ばれていません")?;
            let xy = |t: &str| -> Result<Point, String> {
                let t = t.strip_prefix('X').ok_or("X 座標がありません")?;
                let y = t.find('Y').ok_or("Y 座標がありません")?;
                Ok(Point::new(t[..y].parse().map_err(|_| "X 座標が不正です")?, t[y + 1..].parse().map_err(|_| "Y 座標が不正です")?))
            };
            match l.split_once("G85") {
                Some((a, b)) => out.push((d, xy(a)?, Some(xy(b)?))),
                None => out.push((d, xy(l)?, None)),
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ rasterising

/// A view of the board: millimetres to pixels. Pixel (i, j) has its centre at
/// `(x0 + (i + 0.5) / px, y1 - (j + 0.5) / px)`.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub x0: f64,
    pub y1: f64,
    pub px: f64,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    /// A frame around `lo`..`hi` with a small margin; at most `max_side` pixels on the long side.
    pub fn around(lo: Point, hi: Point, max_side: usize) -> Frame {
        let (w_mm, h_mm) = ((hi.x - lo.x) + 2.0, (hi.y - lo.y) + 2.0);
        let px = (max_side as f64 / w_mm.max(h_mm)).clamp(2.0, 40.0);
        Frame { x0: lo.x - 1.0, y1: hi.y + 1.0, px, w: (w_mm * px).ceil() as usize, h: (h_mm * px).ceil() as usize }
    }

    fn to_mm(self, i: usize, j: usize) -> Point {
        Point::new(self.x0 + (i as f64 + 0.5) / self.px, self.y1 - (j as f64 + 0.5) / self.px)
    }

    fn pixel_range(&self, lo: f64, hi: f64, x_axis: bool) -> (usize, usize) {
        let (a, b) = if x_axis { ((lo - self.x0) * self.px, (hi - self.x0) * self.px) } else { ((self.y1 - hi) * self.px, (self.y1 - lo) * self.px) };
        let max = if x_axis { self.w } else { self.h } as f64;
        (a.floor().clamp(0.0, max) as usize, b.ceil().clamp(0.0, max) as usize)
    }
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

/// Draw `prims` into a coverage buffer (255 = covered). Clear polarity erases.
pub fn rasterize(prims: &[Prim], f: &Frame) -> Vec<u8> {
    let mut buf = vec![0u8; f.w * f.h];
    fn fill(buf: &mut [u8], f: &Frame, lo: Point, hi: Point, clear: bool, inside: &dyn Fn(Point) -> bool) {
        let (i0, i1) = f.pixel_range(lo.x, hi.x, true);
        let (j0, j1) = f.pixel_range(lo.y, hi.y, false);
        for j in j0..j1 {
            for i in i0..i1 {
                if inside(f.to_mm(i, j)) {
                    buf[j * f.w + i] = if clear { 0 } else { 255 };
                }
            }
        }
    }
    for prim in prims {
        match prim {
            Prim::Line { a, b, width, clear } => {
                let r = width / 2.0;
                let (lo, hi) = (Point::new(a.x.min(b.x) - r, a.y.min(b.y) - r), Point::new(a.x.max(b.x) + r, a.y.max(b.y) + r));
                let (a, b) = (*a, *b);
                fill(&mut buf, f, lo, hi, *clear, &move |p| seg_dist(p, a, b) <= r);
            }
            Prim::Flash { ap, at, clear } => {
                let c = *at;
                match *ap {
                    Aperture::Circle(d) => {
                        let r = d / 2.0;
                        fill(&mut buf, f, Point::new(c.x - r, c.y - r), Point::new(c.x + r, c.y + r), *clear, &move |p| p.dist(c) <= r);
                    }
                    Aperture::Rect(w, h) => {
                        fill(&mut buf, f, Point::new(c.x - w / 2.0, c.y - h / 2.0), Point::new(c.x + w / 2.0, c.y + h / 2.0), *clear, &move |p| (p.x - c.x).abs() <= w / 2.0 && (p.y - c.y).abs() <= h / 2.0);
                    }
                    Aperture::Oval(w, h) => {
                        let r = w.min(h) / 2.0;
                        let off = w.max(h) / 2.0 - r;
                        let (a, b) = if w >= h { (Point::new(c.x - off, c.y), Point::new(c.x + off, c.y)) } else { (Point::new(c.x, c.y - off), Point::new(c.x, c.y + off)) };
                        fill(&mut buf, f, Point::new(c.x - w / 2.0, c.y - h / 2.0), Point::new(c.x + w / 2.0, c.y + h / 2.0), *clear, &move |p| seg_dist(p, a, b) <= r);
                    }
                }
            }
            Prim::Region { pts, clear } => {
                let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
                let (ymin, ymax) = (ys.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                let (j0, j1) = f.pixel_range(ymin, ymax, false);
                for j in j0..j1 {
                    let y = f.y1 - (j as f64 + 0.5) / f.px;
                    let mut xs: Vec<f64> = vec![];
                    for k in 0..pts.len() {
                        let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
                        if (p.y > y) != (q.y > y) {
                            xs.push(p.x + (y - p.y) * (q.x - p.x) / (q.y - p.y));
                        }
                    }
                    xs.sort_by(|a, b| a.total_cmp(b));
                    for pair in xs.chunks(2) {
                        if let [x0, x1] = pair {
                            let (i0, i1) = f.pixel_range(*x0, *x1, true);
                            for i in i0..i1 {
                                // pixel centres inside the span
                                let cx = f.x0 + (i as f64 + 0.5) / f.px;
                                if cx >= *x0 && cx <= *x1 {
                                    buf[j * f.w + i] = if *clear { 0 } else { 255 };
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    buf
}

// ------------------------------------------------------------------ the board picture

/// The layers of a board, rebuilt from its fabrication files.
pub struct Preview {
    /// The picture's pixels (what `render` returns).
    pub frame: Frame,
    /// The layers are drawn `SUPERSAMPLE` times finer than `frame` and averaged by `render`, so edges are smooth.
    fine: Frame,
    copper: [Vec<u8>; 2],
    mask: [Vec<u8>; 2],
    silk: [Vec<u8>; 2],
    board: Vec<u8>,
    holes: Vec<u8>,
    /// `(diameter, count, plated)` for the drill summary.
    pub drills: Vec<(f64, usize, bool)>,
    pub size_mm: (f64, f64),
}

/// Samples per picture pixel on each axis (the layers are drawn this much finer, then averaged).
const SUPERSAMPLE: usize = 2;

fn find<'a>(files: &'a [(String, String)], ext: &str) -> Result<&'a str, String> {
    files.iter().find(|(n, _)| n.to_ascii_uppercase().ends_with(ext)).map(|(_, t)| t.as_str()).ok_or_else(|| format!("{ext} のファイルがありません"))
}

impl Preview {
    /// `files` are `(name, contents)` as produced by `export::fab_files`.
    pub fn from_files(files: &[(String, String)], max_side: usize) -> Result<Preview, String> {
        let outline = parse_gerber(find(files, ".GKO")?)?;
        let mut pts: Vec<Point> = vec![];
        for p in &outline {
            if let Prim::Line { a, .. } = p {
                pts.push(*a);
            }
        }
        if pts.len() < 3 {
            return Err("外形のファイルから基板の形を読めません".into());
        }
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        for p in &pts {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let frame = Frame::around(lo, hi, max_side);
        let fine = Frame { px: frame.px * SUPERSAMPLE as f64, w: frame.w * SUPERSAMPLE, h: frame.h * SUPERSAMPLE, ..frame };
        let board = rasterize(&[Prim::Region { pts, clear: false }], &fine);
        let layer = |ext: &str| -> Result<Vec<u8>, String> { Ok(rasterize(&parse_gerber(find(files, ext)?)?, &fine)) };

        let mut hits = parse_excellon_slots(find(files, "-PTH.DRL")?)?.into_iter().map(|(d, p, e)| (d, p, e, true)).collect::<Vec<_>>();
        hits.extend(parse_excellon_slots(find(files, "-NPTH.DRL")?)?.into_iter().map(|(d, p, e)| (d, p, e, false)));
        // a slot is a fat line with round ends
        let holes = rasterize(
            &hits
                .iter()
                .map(|(d, p, e, _)| match e {
                    Some(e) => Prim::Line { a: *p, b: *e, width: *d, clear: false },
                    None => Prim::Flash { ap: Aperture::Circle(*d), at: *p, clear: false },
                })
                .collect::<Vec<_>>(),
            &fine,
        );
        let mut drills: Vec<(f64, usize, bool)> = vec![];
        for (d, _, _, plated) in &hits {
            match drills.iter_mut().find(|(x, _, pl)| (x - d).abs() < 1e-6 && pl == plated) {
                Some(e) => e.1 += 1,
                None => drills.push((*d, 1, *plated)),
            }
        }
        drills.sort_by(|a, b| a.0.total_cmp(&b.0));

        Ok(Preview {
            frame,
            fine,
            copper: [layer(".GTL")?, layer(".GBL")?],
            mask: [layer(".GTS")?, layer(".GBS")?],
            silk: [layer(".GTO")?, layer(".GBO")?],
            board,
            holes,
            drills,
            size_mm: (hi.x - lo.x, hi.y - lo.y),
        })
    }

    /// RGBA pixels, row by row. `bottom` looks at the board from the back (mirrored). Each pixel is the
    /// average of `SUPERSAMPLE` x `SUPERSAMPLE` samples, so the edges of tracks, pads and the outline are smooth.
    pub fn render(&self, bottom: bool, show_mask: bool, show_silk: bool) -> Vec<[u8; 4]> {
        let (w, h) = (self.frame.w, self.frame.h);
        let fw = self.fine.w;
        let side = bottom as usize;
        let mut img = vec![[0u8; 4]; w * h];
        for j in 0..h {
            for i in 0..w {
                // premultiplied sums over the samples of this pixel
                let mut acc = [0u32; 4];
                for dj in 0..SUPERSAMPLE {
                    for di in 0..SUPERSAMPLE {
                        let fi = i * SUPERSAMPLE + di;
                        let src = (j * SUPERSAMPLE + dj) * fw + if bottom { fw - 1 - fi } else { fi };
                        if self.board[src] == 0 {
                            continue;
                        }
                        let mut c = [14u8, 70, 44]; // bare board (green)
                        if self.copper[side][src] != 0 {
                            let open = self.mask[side][src] != 0;
                            c = if !show_mask || open { [214, 176, 58] } else { [38, 128, 76] };
                        }
                        if show_silk && self.silk[side][src] != 0 {
                            c = [238, 238, 232];
                        }
                        if self.holes[src] != 0 {
                            c = [22, 22, 22];
                        }
                        for k in 0..3 {
                            acc[k] += c[k] as u32;
                        }
                        acc[3] += 1;
                    }
                }
                if acc[3] > 0 {
                    let n = acc[3];
                    let total = (SUPERSAMPLE * SUPERSAMPLE) as u32;
                    img[j * w + i] = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (255 * n / total) as u8];
                }
            }
        }
        img
    }

    /// Is there copper on `side` at this millimetre position (for tests and probing)?
    pub fn copper_at(&self, side: crate::model::Side, p: Point) -> bool {
        let i = ((p.x - self.fine.x0) * self.fine.px).floor();
        let j = ((self.fine.y1 - p.y) * self.fine.px).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.fine.w || j as usize >= self.fine.h {
            return false;
        }
        let k = j as usize * self.fine.w + i as usize;
        self.copper[(side == crate::model::Side::Bottom) as usize][k] != 0
    }

    pub fn hole_at(&self, p: Point) -> bool {
        let i = ((p.x - self.fine.x0) * self.fine.px).floor();
        let j = ((self.fine.y1 - p.y) * self.fine.px).floor();
        i >= 0.0 && j >= 0.0 && (i as usize) < self.fine.w && (j as usize) < self.fine.h && self.holes[j as usize * self.fine.w + i as usize] != 0
    }
}
