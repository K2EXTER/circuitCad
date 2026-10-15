//! Length matching: a serpentine (meander) in a straight run of a net makes it longer, so that nets that must arrive
//! together (a bus, a clock and its data, the two lines of a differential pair) have the same length.
//!
//! A run of a net is replaced by a run with bumps on one side of it. The bumps are as wide as they are far apart, so the
//! facing edges keep the clearance; their height is worked out so that the added length is what was asked for (the
//! corners can be square, cut at 45° or rounded, which shortens the path a little, so the height is adjusted until
//! the length is right). Every attempt is checked with the DRC and thrown away if it makes things worse.

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
    if style == MeanderStyle::Square || pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for k in 1..pts.len() - 1 {
        let (p, v, q) = (pts[k - 1], pts[k], pts[k + 1]);
        let (l1, l2) = (v.dist(p), v.dist(q));
        let d = d.min(0.45 * l1.min(l2));
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
