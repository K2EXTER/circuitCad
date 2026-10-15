//! Differential pairs: two nets that carry one signal as a difference and must be routed side by side,
//! with the same length. This module measures a pair (length, skew, how well the two lines are coupled),
//! checks it, and evens out the lengths with a serpentine ("meander") on the shorter line.

use crate::drc::{check, connectivity, point_seg, seg_seg};
use crate::model::*;
use serde::{Deserialize, Serialize};

fn default_gap() -> f64 {
    0.2
}

fn default_skew() -> f64 {
    0.5
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffPair {
    pub name: String,
    /// The two nets (the "+" and "-" line).
    pub p: String,
    pub n: String,
    /// Wanted distance between the edges of the two lines (mm) where they run together.
    #[serde(default = "default_gap")]
    pub gap: f64,
    /// Longest allowed difference in length (mm).
    #[serde(default = "default_skew")]
    pub max_skew: f64,
}

impl DiffPair {
    pub fn new(name: &str, p: &str, n: &str) -> Self {
        Self { name: name.into(), p: p.into(), n: n.into(), gap: default_gap(), max_skew: default_skew() }
    }
}

/// Guess pairs from net names: `USB_DP`/`USB_DM`, `D+`/`D-`, `TX_P`/`TX_N`, `CLK+`/`CLK-` ...
pub fn suggest_pairs(nets: &[String]) -> Vec<DiffPair> {
    const ENDS: [(&str, &str); 6] = [("_P", "_N"), ("+", "-"), ("_DP", "_DM"), ("P", "N"), ("_H", "_L"), ("_A", "_B")];
    let mut out: Vec<DiffPair> = vec![];
    for a in nets {
        for (pe, ne) in ENDS {
            let Some(stem) = a.strip_suffix(pe) else { continue };
            let b = format!("{stem}{ne}");
            // "P"/"N" alone is too loose to trust unless the stem ends in a separator or a digit
            if (pe == "P") && !(stem.ends_with(|c: char| c.is_ascii_digit() || c == '_' || c == '-')) {
                continue;
            }
            if nets.contains(&b) && !out.iter().any(|d| d.p == *a) {
                let name = if stem.is_empty() { a.clone() } else { stem.trim_end_matches(['_', '-']).to_string() };
                out.push(DiffPair::new(&name, a, &b));
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct PairReport {
    pub len_p: f64,
    pub len_n: f64,
    pub vias_p: usize,
    pub vias_n: usize,
    /// |len_p - len_n|
    pub skew: f64,
    /// Length of the part where the two lines run side by side (counted on P).
    pub coupled: f64,
    /// Tightest and widest edge gap measured along the coupled part (`None` when nothing runs together).
    pub gap_min: Option<f64>,
    pub gap_max: Option<f64>,
}

fn seg_len(t: &Track) -> f64 {
    t.a.dist(t.b)
}

/// Track length of a net (mm) and the number of vias on it.
pub fn net_length(board: &Board, net: &str) -> (f64, usize) {
    let conn = connectivity(board);
    let len = board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).map(|(_, t)| seg_len(t)).sum();
    let vias = (0..board.vias.len()).filter(|&i| conn.via_net(i) == Some(net)).count();
    (len, vias)
}

/// Is the line `c`-`d` roughly parallel to `a`-`b` (within about 10 degrees)?
fn parallel(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (u, v) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y));
    let (lu, lv) = (u.0.hypot(u.1), v.0.hypot(v.1));
    if lu < 1e-9 || lv < 1e-9 {
        return false;
    }
    let cross = (u.0 * v.1 - u.1 * v.0).abs() / (lu * lv);
    let dot = (u.0 * v.0 + u.1 * v.1).abs() / (lu * lv);
    cross < 0.17 && dot > 0.9
}

pub fn pair_report(board: &Board, pair: &DiffPair) -> PairReport {
    let conn = connectivity(board);
    let of = |net: &str| -> Vec<&Track> { board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).map(|(_, t)| t).collect() };
    let (tp, tn) = (of(&pair.p), of(&pair.n));
    let vias = |net: &str| (0..board.vias.len()).filter(|&i| conn.via_net(i) == Some(net)).count();
    let (len_p, len_n): (f64, f64) = (0.0 + tp.iter().map(|t| seg_len(t)).sum::<f64>(), 0.0 + tn.iter().map(|t| seg_len(t)).sum::<f64>());
    let (mut coupled, mut gmin, mut gmax): (f64, Option<f64>, Option<f64>) = (0.0, None, None);
    let reach = pair.gap * 4.0 + 1.0;
    for a in &tp {
        // how much of this segment has a partner line next to it
        let mut best: Option<(f64, f64)> = None; // (partner length alongside, edge gap)
        for b in tn.iter().filter(|b| b.side == a.side && parallel(a.a, a.b, b.a, b.b)) {
            let centre = seg_seg(a.a, a.b, b.a, b.b);
            let gap = centre - (a.width + b.width) / 2.0;
            if gap > reach {
                continue;
            }
            // overlap along the direction of `a`
            let l = seg_len(a).max(1e-9);
            let (ux, uy) = ((a.b.x - a.a.x) / l, (a.b.y - a.a.y) / l);
            let proj = |p: Point| (p.x - a.a.x) * ux + (p.y - a.a.y) * uy;
            let (lo, hi) = (proj(b.a).min(proj(b.b)).max(0.0), proj(b.a).max(proj(b.b)).min(l));
            let overlap = (hi - lo).max(0.0);
            if overlap > 1e-6 && best.is_none_or(|x| overlap > x.0) {
                best = Some((overlap, gap));
            }
        }
        if let Some((o, g)) = best {
            coupled += o;
            gmin = Some(gmin.map_or(g, |m: f64| m.min(g)));
            gmax = Some(gmax.map_or(g, |m: f64| m.max(g)));
        }
    }
    PairReport { len_p, len_n, vias_p: vias(&pair.p), vias_n: vias(&pair.n), skew: (len_p - len_n).abs(), coupled, gap_min: gmin, gap_max: gmax }
}

/// Problems of all pairs, in Japanese with the cause and the fix.
pub fn check_pairs(board: &Board) -> Vec<crate::drc::Violation> {
    let mut out = vec![];
    for d in &board.diff_pairs {
        let r = pair_report(board, d);
        let at = board.pads.iter().find(|p| p.net == d.p).map_or(Point::default(), |p| p.pos);
        let v = |message: String| crate::drc::Violation { message, at };
        for net in [&d.p, &d.n] {
            if !board.pads.iter().any(|p| p.net == *net) {
                out.push(v(format!("差動ペア {} の「{net}」というネットが基板にありません。ペアの設定を見直しましょう", d.name)));
            }
        }
        if (r.len_p > 0.0) != (r.len_n > 0.0) {
            out.push(v(format!("差動ペア {} は片方だけ配線されています。もう片方も、並べて配線しましょう (ペアで自動配線が使えます)", d.name)));
            continue;
        }
        if r.skew > d.max_skew + 1e-9 {
            out.push(v(format!(
                "差動ペア {} の長さの差が {:.2}mm あります (許容 {:.2}mm)。短いほうに蛇行を足して長さを合わせましょう。差があると、2本の信号が届く時刻がずれて、差動の利点が減ります",
                d.name, r.skew, d.max_skew
            )));
        }
        if let (Some(lo), Some(hi)) = (r.gap_min, r.gap_max) {
            let tol = 0.05_f64.max(d.gap * 0.25);
            if lo + tol < d.gap || hi > d.gap + tol {
                out.push(v(format!(
                    "差動ペア {} の2本の間隔が {:.2}〜{:.2}mm で、目標の {:.2}mm からずれています。並んで走る部分は、間隔を一定にしましょう (ずれると、差動のインピーダンスが変わり、ノイズにも弱くなります)",
                    d.name, lo.max(0.0), hi, d.gap
                )));
            }
        }
        if r.len_p > 0.0 && r.coupled < 0.5 * r.len_p.min(r.len_n) {
            out.push(v(format!(
                "差動ペア {} は、2本が並んで走っている部分が少ないです (並走 {:.1}mm / 全長 {:.1}mm)。離れた配線では、ノイズを同じように拾えず、差動の利点が出ません。並べて引き直しましょう",
                d.name, r.coupled, r.len_p
            )));
        }
    }
    out
}

/// Add a serpentine to the shorter line of the pair so that the lengths match. Returns the length added (mm).
/// Fails (and leaves the board as it was) when there is no straight run long enough, or the serpentine would break a rule.
pub fn tune_length(board: &mut Board, pair: &DiffPair) -> Result<f64, String> {
    let r = pair_report(board, pair);
    let delta = r.skew;
    if delta < 0.01 {
        return Err("長さはもう合っています".into());
    }
    if r.len_p == 0.0 || r.len_n == 0.0 {
        return Err("両方の線が配線されてから、長さを合わせます".into());
    }
    let short = if r.len_p < r.len_n { &pair.p } else { &pair.n };
    crate::meander::tune_net(board, short, delta, &crate::meander::MeanderOptions::default())
}

/// Length of a polyline of tracks (for tests and the panel).
pub fn total_length(tracks: &[Track]) -> f64 {
    tracks.iter().map(seg_len).sum()
}

/// Distance from a point to the nearest track of a net (for placing marks).
pub fn nearest_on_net(board: &Board, net: &str, p: Point) -> Option<Point> {
    let conn = connectivity(board);
    board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).min_by(|(_, a), (_, b)| point_seg(p, a.a, a.b).total_cmp(&point_seg(p, b.a, b.b))).map(|(_, t)| t.a)
}
