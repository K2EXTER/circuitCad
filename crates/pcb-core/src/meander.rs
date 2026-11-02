//! Length matching: a serpentine (meander) in a straight run of a net makes it longer, so that nets that must arrive
//! together (a bus, a clock and its data, the two lines of a differential pair) have the same length.
//!
//! A run of a net is replaced by a run with bumps on one side of it. The bumps are as wide as they are far apart, so the
//! facing edges keep the clearance; their height is worked out so that the added length is what was asked for (the
//! corners can be square, cut at 45° or rounded, which shortens the path a little, so the height is adjusted until
//! the length is right). Every attempt is checked with the DRC and thrown away if it makes things worse.
//!
//! The two lines of a differential pair are bent together ([`tune_pair`]): the lines of a coupled run are replaced by bumps
//! that nest, one line wrapped round the other, so the gap between them stays what it was. A line that is offset from
//! another by `d` has the same length on a bump (what one gains on the outer corners it loses on the inner ones), so
//! the bumps add the same length to both lines and the pair keeps its skew; they alternate sides so that the line on the
//! outside changes from bump to bump.

use crate::drc::{check, connectivity};
use crate::model::*;

/// The shape of the corners of the bumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MeanderStyle {
    /// Square corners.
    Square,
    /// Corners cut at 45°.
    Chamfer,
    /// Round corners.
    Round,
}

#[derive(Clone, Copy, Debug)]
pub struct MeanderOptions {
    pub style: MeanderStyle,
    /// The tallest bump (mm) the search tries first; a few more are allowed when the length does not fit.
    pub height_max: f64,
}

impl Default for MeanderOptions {
    fn default() -> Self {
        Self { style: MeanderStyle::Square, height_max: 2.0 }
    }
}

fn seg_len(t: &Track) -> f64 {
    t.a.dist(t.b)
}

/// Track length of a net (mm).
pub fn length_of(board: &Board, net: &str) -> f64 {
    let conn = connectivity(board);
    board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).map(|(_, t)| seg_len(t)).sum()
}

fn poly_len(pts: &[Point]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Replace the 90° corners inside `pts` by a cut (`Chamfer`) or an arc (`Round`) of size `d` (smaller where the
/// pieces next to it are short).
fn treat_corners(pts: &[Point], d: f64, style: MeanderStyle) -> Vec<Point> {
    treat_corners_sized(pts, &vec![d; pts.len()], style)
}

/// [`treat_corners`] with a size of its own for each corner (`sizes[k]` for `pts[k]`).
fn treat_corners_sized(pts: &[Point], sizes: &[f64], style: MeanderStyle) -> Vec<Point> {
    if style == MeanderStyle::Square || pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for k in 1..pts.len() - 1 {
        let (p, v, q) = (pts[k - 1], pts[k], pts[k + 1]);
        let (l1, l2) = (v.dist(p), v.dist(q));
        let d = sizes[k].min(0.45 * l1.min(l2));
        if d < 1e-6 {
            out.push(v);
            continue;
        }
        let (u1, u2) = (((p.x - v.x) / l1, (p.y - v.y) / l1), ((q.x - v.x) / l2, (q.y - v.y) / l2));
        let (a, b) = (Point::new(v.x + u1.0 * d, v.y + u1.1 * d), Point::new(v.x + u2.0 * d, v.y + u2.1 * d));
        match style {
            MeanderStyle::Chamfer => out.extend([a, b]),
            _ => {
                // the arc's centre lies inside the corner, d from both legs
                let c = Point::new(v.x + (u1.0 + u2.0) * d, v.y + (u1.1 + u2.1) * d);
                let (a1, a2) = ((a.y - c.y).atan2(a.x - c.x), (b.y - c.y).atan2(b.x - c.x));
                let mut sweep = a2 - a1;
                while sweep > std::f64::consts::PI {
                    sweep -= std::f64::consts::TAU;
                }
                while sweep < -std::f64::consts::PI {
                    sweep += std::f64::consts::TAU;
                }
                let steps = ((sweep.abs() / 15f64.to_radians()).ceil() as usize).max(2);
                out.push(a);
                for i in 1..steps {
                    let ang = a1 + sweep * i as f64 / steps as f64;
                    out.push(Point::new(c.x + d * ang.cos(), c.y + d * ang.sin()));
                }
                out.push(b);
            }
        }
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// The run `t` with `n` bumps of height `h` and width `s` on side `side` (+1 / -1) of it, bumps in the middle.
fn bumps(t: &Track, n: usize, h: f64, s: f64, margin: f64, side: f64, style: MeanderStyle) -> Option<Vec<Point>> {
    let l = seg_len(t);
    let (ux, uy) = ((t.b.x - t.a.x) / l, (t.b.y - t.a.y) / l);
    let (nx, ny) = (-uy * side, ux * side);
    let at = |along: f64, off: f64| Point::new(t.a.x + ux * along + nx * off, t.a.y + uy * along + ny * off);
    let total = n as f64 * 2.0 * s - s;
    let start = (l - total) / 2.0;
    if start < margin {
        return None;
    }
    let mut pts = vec![t.a, at(start, 0.0)];
    for k in 0..n {
        let x = start + k as f64 * 2.0 * s;
        pts.extend([at(x, 0.0), at(x, h), at(x + s, h), at(x + s, 0.0)]);
    }
    pts.push(t.b);
    pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
    // a corner of the cut or arc is at most a third of a bump: the legs and tops stay straight
    Some(treat_corners(&pts, (0.45 * s).min(0.45 * h), style))
}

/// Replace the straight track `t` by a serpentine that is `extra` mm longer. `None` when it does not fit.
pub fn meander_track(t: &Track, extra: f64, side: f64, clearance: f64, opts: &MeanderOptions) -> Option<Vec<Track>> {
    let l = seg_len(t);
    let w = t.width;
    // each bump is `s` wide and the bumps are `s` apart: the facing edges are s - w apart
    let s = (3.0 * w).max(2.0 * w + clearance + 0.1);
    let margin = 1.0_f64.max(2.0 * w);
    let n_max = ((l - 2.0 * margin) / (2.0 * s)).floor();
    if n_max < 1.0 || extra <= 0.0 {
        return None;
    }
    let n_max = n_max as usize;
    // for n bumps: the height that gives `extra`, found by correcting the height until the length is right
    let solve = |n: usize| -> Option<Vec<Point>> {
        let mut h = extra / (2.0 * n as f64);
        for _ in 0..12 {
            if h <= 0.0 {
                return None;
            }
            let pts = bumps(t, n, h, s, margin, side, opts.style)?;
            let got = poly_len(&pts) - l;
            if (got - extra).abs() < 0.005 {
                return (h >= 0.5 * w && h <= 1.3 * opts.height_max).then_some(pts);
            }
            h += (extra - got) / (2.0 * n as f64);
        }
        None
    };
    let n0 = ((extra / (2.0 * opts.height_max)).ceil() as usize).clamp(1, n_max);
    // first the fewest bumps that stay low enough, then fewer and taller ones
    let order = (n0..=n_max).chain((1..n0).rev());
    let pts = order.into_iter().find_map(solve)?;
    Some(pts.windows(2).filter(|w| w[0].dist(w[1]) > 1e-9).map(|w| Track { side: t.side, a: w[0], b: w[1], width: t.width }).collect())
}

/// Make net `net` `extra` mm longer with a serpentine in one of its longest straight runs. Returns the length added.
/// Fails (and leaves the board as it was) when no run is long enough or the serpentine would break a rule.
pub fn tune_net(board: &mut Board, net: &str, extra: f64, opts: &MeanderOptions) -> Result<f64, String> {
    if extra < 0.01 {
        return Err("長さはもう合っています".into());
    }
    let conn = connectivity(board);
    let mut cand: Vec<usize> = (0..board.tracks.len()).filter(|&i| conn.track_net(i) == Some(net)).collect();
    if cand.is_empty() {
        return Err(format!("ネット {net} はまだ配線されていません。配線してから長さを合わせます"));
    }
    cand.sort_by(|&a, &b| seg_len(&board.tracks[b]).total_cmp(&seg_len(&board.tracks[a])));
    let before = check(board).len();
    let len_before = length_of(board, net);
    for i in cand.into_iter().take(4) {
        for side in [1.0, -1.0] {
            let Some(new_tracks) = meander_track(&board.tracks[i], extra, side, board.rules.min_clearance, opts) else { continue };
            let mut trial = board.clone();
            trial.tracks.remove(i);
            trial.tracks.extend(new_tracks);
            if check(&trial).len() > before {
                continue;
            }
            let got = length_of(&trial, net) - len_before;
            if (got - extra).abs() < 0.05_f64.max(extra * 0.02) {
                *board = trial;
                return Ok(got);
            }
        }
    }
    Err("蛇行を入れられる、まっすぐで長い配線が見つかりませんでした (近くの配線とぶつかるか、短すぎます)。配線を少し直線にするか、周りをあけてからもう一度試してください".into())
}

/// What [`match_nets`] did.
#[derive(Clone, Debug, Default)]
pub struct MatchReport {
    /// The length every net was brought to (mm).
    pub target: f64,
    /// Per net: its length before, and the length added or why it could not be done.
    pub nets: Vec<(String, f64, Result<f64, String>)>,
}

/// Bring every one of `nets` to the length of the longest of them (or to `target`, when given and longer).
pub fn match_nets(board: &mut Board, nets: &[String], target: Option<f64>, opts: &MeanderOptions) -> MatchReport {
    let lens: Vec<f64> = nets.iter().map(|n| length_of(board, n)).collect();
    let longest = lens.iter().copied().fold(0.0, f64::max);
    let target = target.map_or(longest, |t| t.max(longest));
    let mut report = MatchReport { target, nets: vec![] };
    for (n, len) in nets.iter().zip(lens) {
        let res = if len == 0.0 {
            Err(format!("ネット {n} はまだ配線されていません"))
        } else if target - len < 0.01 {
            Ok(0.0)
        } else {
            tune_net(board, n, target - len, opts)
        };
        report.nets.push((n.clone(), len, res));
    }
    report
}

// ------------------------------------------------------------------ the two lines of a pair, bent together

/// Two parallel straight runs `p` and `n` (the two lines of a coupled pair) in a frame of their own: `along` is the distance
/// along `p`, `off` the distance from `p` towards `n`, `d` the centre-to-centre distance of the lines.
struct PairFrame {
    origin: Point,
    u: (f64, f64),
    m: (f64, f64),
    d: f64,
    /// The stretch (along) where both runs are.
    lo: f64,
    hi: f64,
    /// The ends of `n` (along), low to high.
    n_ends: (f64, f64),
    /// The length of `p`.
    len_p: f64,
}

impl PairFrame {
    fn new(p: &Track, n: &Track) -> Option<Self> {
        if p.side != n.side {
            return None;
        }
        let (lp, ln) = (seg_len(p), seg_len(n));
        if lp < 1e-6 || ln < 1e-6 {
            return None;
        }
        let u = ((p.b.x - p.a.x) / lp, (p.b.y - p.a.y) / lp);
        let un = ((n.b.x - n.a.x) / ln, (n.b.y - n.a.y) / ln);
        if (u.0 * un.1 - u.1 * un.0).abs() > 1e-3 {
            return None;
        }
        let off_of = |q: Point| -u.1 * (q.x - p.a.x) + u.0 * (q.y - p.a.y);
        let along_of = |q: Point| u.0 * (q.x - p.a.x) + u.1 * (q.y - p.a.y);
        let (oa, ob) = (off_of(n.a), off_of(n.b));
        if (oa - ob).abs() > 0.02 || oa.abs() < 1e-3 {
            return None;
        }
        let (m, d) = if oa > 0.0 { ((-u.1, u.0), oa) } else { ((u.1, -u.0), -oa) };
        let (a1, a2) = (along_of(n.a), along_of(n.b));
        let n_ends = (a1.min(a2), a1.max(a2));
        Some(Self { origin: p.a, u, m, d, lo: n_ends.0.max(0.0), hi: n_ends.1.min(lp), n_ends, len_p: lp })
    }

    fn at(&self, along: f64, off: f64) -> Point {
        Point::new(self.origin.x + self.u.0 * along + self.m.0 * off, self.origin.y + self.u.1 * along + self.m.1 * off)
    }
}

/// How the bumps of a pair are laid out: width `s` of the inner bump, distance `pitch` between the starts of two bumps, the
/// free stretch `margin` kept at each end, and the most bumps that fit.
struct PairLayout {
    s: f64,
    pitch: f64,
    margin: f64,
    n_max: usize,
}

fn pair_layout(f: &PairFrame, w: f64, clearance: f64, style: MeanderStyle) -> PairLayout {
    let d = f.d;
    let s = (3.0 * w).max(2.0 * w + clearance + 0.1).max(2.0 * d + 2.0 * w);
    // the outer lines of two bumps on the same side face each other across 2 pitches; the baseline between a bump
    // and the next (on the other side) must be long enough for the corners
    let baseline = if style == MeanderStyle::Square { w + clearance } else { (w + clearance).max(s) };
    let pitch = ((s + 2.0 * d + w + clearance) / 2.0).max(s + d + baseline);
    let margin = 1.0_f64.max(2.0 * w);
    let room = f.hi - f.lo - 2.0 * margin - s - 2.0 * d;
    let n_max = if room < 0.0 { 0 } else { (room / pitch).floor() as usize + 1 };
    PairLayout { s, pitch, margin, n_max }
}

/// The two polylines (in the order of `along`) of `n` nested bumps of depth `depth`, the first towards `n` when `sigma0` is 1.0
/// (away from it when -1.0), the next on the other side, and so on. `None` when they do not fit.
fn pair_bumps(f: &PairFrame, l: &PairLayout, n: usize, depth: f64, sigma0: f64, style: MeanderStyle) -> Option<(Vec<Point>, Vec<Point>)> {
    let d = f.d;
    let total = (n as f64 - 1.0) * l.pitch + l.s + 2.0 * d;
    let free = (f.hi - f.lo - total) / 2.0;
    if n == 0 || free < l.margin {
        return None;
    }
    let x0 = f.lo + free + d;
    // a corner is cut or rounded by `base` on the line inside the turn and by `base + ext` on the line outside it
    // (concentric arcs; parallel cuts), so the lines stay `d` apart round the corner
    let ext = match style {
        MeanderStyle::Square => 0.0,
        MeanderStyle::Chamfer => (2.0 - std::f64::consts::SQRT_2) * d,
        MeanderStyle::Round => d,
    };
    let base = (0.45 * l.s.min(depth) - ext).max(0.0);
    let mut p_pts = vec![f.at(0.0, 0.0)];
    let mut n_pts = vec![f.at(f.n_ends.0, d)];
    let (mut p_size, mut n_size) = (vec![0.0], vec![0.0]);
    for k in 0..n {
        let x = x0 + k as f64 * l.pitch;
        let sigma = if k % 2 == 0 { sigma0 } else { -sigma0 };
        // `p` is inside the turn of the legs' corners (j = 0, 3) and outside the turn of the top corners (j = 1, 2) when the
        // bump goes towards `n` (sigma > 0); the other way round for `n`; and everything swaps when the bump goes away from `n`
        let (p_inner, n_inner) = if sigma > 0.0 {
            (vec![(x, 0.0), (x, depth), (x + l.s, depth), (x + l.s, 0.0)], vec![(x - d, d), (x - d, d + depth), (x + l.s + d, d + depth), (x + l.s + d, d)])
        } else {
            (vec![(x - d, 0.0), (x - d, -depth), (x + l.s + d, -depth), (x + l.s + d, 0.0)], vec![(x, d), (x, d - depth), (x + l.s, d - depth), (x + l.s, d)])
        };
        for (j, (a, o)) in p_inner.iter().enumerate() {
            p_pts.push(f.at(*a, *o));
            // the line on the bump's side is outside at the top corners and inside at the foot corners
            let p_on_bump_side = sigma < 0.0;
            let outside = if j == 1 || j == 2 { p_on_bump_side } else { !p_on_bump_side };
            p_size.push(base + if outside { ext } else { 0.0 });
        }
        for (j, (a, o)) in n_inner.iter().enumerate() {
            n_pts.push(f.at(*a, *o));
            let n_on_bump_side = sigma > 0.0;
            let outside = if j == 1 || j == 2 { n_on_bump_side } else { !n_on_bump_side };
            n_size.push(base + if outside { ext } else { 0.0 });
        }
    }
    p_pts.push(f.at(f.len_p, 0.0));
    n_pts.push(f.at(f.n_ends.1, d));
    p_size.push(0.0);
    n_size.push(0.0);
    Some((treat_corners_sized(&p_pts, &p_size, style), treat_corners_sized(&n_pts, &n_size, style)))
}

/// The tracks that replace the straight runs `p` and `n`, bent together so that each is `extra` mm longer; `n_bumps` bumps, the
/// first towards `n` when `sigma0` is 1.0. `None` when it does not fit (the depth must stay between half a track width and the
/// tallest bump) or the two lines would not get the same length.
fn meander_pair(p: &Track, n: &Track, extra: f64, clearance: f64, opts: &MeanderOptions, n_bumps: usize, sigma0: f64) -> Option<(Vec<Track>, Vec<Track>)> {
    let f = PairFrame::new(p, n)?;
    let w = p.width.max(n.width);
    let l = pair_layout(&f, w, clearance, opts.style);
    if n_bumps == 0 || n_bumps > l.n_max || extra <= 0.0 {
        return None;
    }
    let straight = (f.len_p, f.n_ends.1 - f.n_ends.0);
    let mut depth = extra / (2.0 * n_bumps as f64);
    for _ in 0..12 {
        if depth <= 0.0 {
            return None;
        }
        let (pp, np) = pair_bumps(&f, &l, n_bumps, depth, sigma0, opts.style)?;
        let (got_p, got_n) = (poly_len(&pp) - straight.0, poly_len(&np) - straight.1);
        let got = (got_p + got_n) / 2.0;
        if (got - extra).abs() < 0.005 {
            let fits = depth >= 0.5 * w && depth <= 1.3 * opts.height_max;
            let alike = (got_p - got_n).abs() < 0.05;
            let tracks = |pts: &[Point], t: &Track| -> Vec<Track> { pts.windows(2).filter(|s| s[0].dist(s[1]) > 1e-9).map(|s| Track { side: t.side, a: s[0], b: s[1], width: t.width }).collect() };
            return (fits && alike).then(|| (tracks(&pp, p), tracks(&np, n)));
        }
        depth += (extra - got) / (2.0 * n_bumps as f64);
    }
    None
}

/// Make both lines of `pair` `extra` mm longer with bumps that nest, in the longest stretch where they run side by side
/// (the gap between them stays; the skew of the pair does not change). Returns the length added to each line. Fails (and
/// leaves the board as it was) when no such stretch is long enough or the bumps would break a rule.
pub fn tune_pair(board: &mut Board, pair: &crate::diffpair::DiffPair, extra: f64, opts: &MeanderOptions) -> Result<f64, String> {
    if extra < 0.01 {
        return Err("長さはもう合っています".into());
    }
    let conn = connectivity(board);
    let of = |net: &str| -> Vec<usize> { (0..board.tracks.len()).filter(|&i| conn.track_net(i) == Some(net)).collect() };
    let (tp, tn) = (of(&pair.p), of(&pair.n));
    // the runs that lie side by side, the longest stretch first
    let mut runs: Vec<(f64, usize, usize)> = vec![];
    for &i in &tp {
        for &j in &tn {
            if let Some(f) = PairFrame::new(&board.tracks[i], &board.tracks[j]) {
                runs.push((f.hi - f.lo, i, j));
            }
        }
    }
    runs.sort_by(|a, b| b.0.total_cmp(&a.0));
    if runs.is_empty() {
        return Err("2本が並んで走るまっすぐな所がありません".into());
    }
    let before = check(board).len();
    let (len_p0, len_n0) = (length_of(board, &pair.p), length_of(board, &pair.n));
    let tol = 0.05_f64.max(extra * 0.02);
    for &(_, i, j) in runs.iter().take(3) {
        let (tp_i, tn_j) = (board.tracks[i].clone(), board.tracks[j].clone());
        let Some(f) = PairFrame::new(&tp_i, &tn_j) else { continue };
        let l = pair_layout(&f, tp_i.width.max(tn_j.width), board.rules.min_clearance, opts.style);
        if l.n_max == 0 {
            continue;
        }
        // first the fewest bumps that stay low enough, then fewer and taller ones
        let n0 = ((extra / (2.0 * opts.height_max)).ceil() as usize).clamp(1, l.n_max);
        let counts: Vec<usize> = (n0..=l.n_max).chain((1..n0).rev()).take(6).collect();
        for n_bumps in counts {
            for sigma0 in [1.0, -1.0] {
                let Some((new_p, new_n)) = meander_pair(&tp_i, &tn_j, extra, board.rules.min_clearance, opts, n_bumps, sigma0) else { continue };
                let mut trial = board.clone();
                let (hi_i, lo_i) = (i.max(j), i.min(j));
                trial.tracks.remove(hi_i);
                trial.tracks.remove(lo_i);
                trial.tracks.extend(new_p);
                trial.tracks.extend(new_n);
                if check(&trial).len() > before {
                    continue;
                }
                let (got_p, got_n) = (length_of(&trial, &pair.p) - len_p0, length_of(&trial, &pair.n) - len_n0);
                if (got_p - extra).abs() < tol && (got_n - extra).abs() < tol {
                    *board = trial;
                    return Ok((got_p + got_n) / 2.0);
                }
            }
        }
    }
    Err("2本をいっしょに曲げる蛇行を入れられる、並んだまっすぐな配線が見つかりませんでした (近くの配線とぶつかるか、短すぎます)".into())
}
