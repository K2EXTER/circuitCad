//! Breakout stubs for fine-pitch ICs: the way a person leaves a QFN or a fine-pitch SOP before routing the rest. The pins in the
//! middle of a side go straight out, the pins at the two ends of the side turn outward after a short way, and the nearer a pin is to
//! the end the shorter its way before the turn, so that no wire of the end pins runs across the way out of the middle ones.
//! The router then goes on from the end of each stub (copper of its own net is free to use).

use crate::drc::check;
use crate::model::*;
use std::collections::BTreeSet;

/// A row of pads with a pitch of at most this (mm) is a fine-pitch row.
const FINE_PITCH: f64 = 0.8;
/// The fewest pads of a row that get stubs.
const MIN_ROW: usize = 3;
/// How far (mm) beyond the pad's edge the pin at the very end turns, and how much further each pin nearer the middle goes.
const BASE: f64 = 0.35;
const STEP: f64 = 0.5;
/// How far (mm) a stub runs outward after its turn.
const RUN: f64 = 0.8;
/// How far (mm) beyond the pad's edge the pins in the middle go straight.
const MID_LEN: f64 = 0.5;
/// Parts with fewer pads than this get none.
const MIN_PADS: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct BreakoutReport {
    /// Stubs drawn.
    pub stubs: usize,
    /// The parts that got stubs.
    pub parts: Vec<String>,
    /// The parts whose stubs were taken away again because the DRC found more than before.
    pub dropped: Vec<String>,
}

/// Draw the stubs of every fine-pitch row of every IC (a part of `MIN_PADS` pads or more) for the pins that need wiring: pads on a net
/// that is not a pour and reaches another part, with no copper on them yet. `width` is the track width (cut to fit the pitch). A part
/// whose stubs make the DRC find more than before gets none.
pub fn breakout(board: &mut Board, width: f64) -> BreakoutReport {
    let mut report = BreakoutReport::default();
    let poured: BTreeSet<String> = board.zones.iter().map(|z| z.net.clone()).filter(|n| !n.is_empty()).collect();
    let refs: Vec<String> = board.parts.iter().map(|p| p.refdes.clone()).collect();
    for r in refs {
        let pads: Vec<Pad> = board.pads.iter().filter(|p| p.refdes == r).cloned().collect();
        if pads.len() < MIN_PADS || board.is_locked(&r) {
            continue;
        }
        let (x0, x1) = (pads.iter().map(|p| p.pos.x).fold(f64::MAX, f64::min), pads.iter().map(|p| p.pos.x).fold(f64::MIN, f64::max));
        let (y0, y1) = (pads.iter().map(|p| p.pos.y).fold(f64::MAX, f64::min), pads.iter().map(|p| p.pos.y).fold(f64::MIN, f64::max));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (hw, hh) = (((x1 - x0) / 2.0).max(0.1), ((y1 - y0) / 2.0).max(0.1));
        // the rows: pads on the edge, grouped by the side they face and the layer they are on
        let mut rows: std::collections::BTreeMap<(u8, String), Vec<&Pad>> = Default::default();
        for p in &pads {
            if p.drill.is_some() {
                continue;
            }
            let (dx, dy) = ((p.pos.x - cx) / hw, (p.pos.y - cy) / hh);
            if dx.abs().max(dy.abs()) < 0.6 {
                continue;
            }
            let side = if dx.abs() >= dy.abs() { if dx > 0.0 { 0 } else { 1 } } else if dy > 0.0 { 2 } else { 3 };
            rows.entry((side, format!("{:?}", p.side))).or_default().push(p);
        }
        let before = check(board).len();
        let mark = board.tracks.len();
        let mut drawn = 0;
        let mut made: Vec<(String, Track)> = vec![];
        let gap = board.rules.min_clearance + 0.02;
        for ((side, _), mut row) in rows {
            let vertical = side < 2; // the pads stand in a column: the stubs go out along x
            row.sort_by(|a, b| if vertical { a.pos.y.total_cmp(&b.pos.y) } else { a.pos.x.total_cmp(&b.pos.x) });
            let n = row.len();
            if n < MIN_ROW {
                continue;
            }
            let mut gaps: Vec<f64> = row.windows(2).map(|w| if vertical { w[1].pos.y - w[0].pos.y } else { w[1].pos.x - w[0].pos.x }).collect();
            gaps.sort_by(f64::total_cmp);
            let pitch = gaps[gaps.len() / 2];
            if pitch <= 0.0 || pitch > FINE_PITCH {
                continue;
            }
            let w = width.min(pitch * 0.45);
            let normal: (f64, f64) = match side {
                0 => (1.0, 0.0),
                1 => (-1.0, 0.0),
                2 => (0.0, 1.0),
                _ => (0.0, -1.0),
            };
            let along = (normal.1.abs(), normal.0.abs());
            for (k, p) in row.iter().enumerate() {
                let needs = !p.net.is_empty()
                    && !poured.contains(&p.net)
                    && board.pads.iter().filter(|q| q.net == p.net).map(|q| q.refdes.as_str()).collect::<BTreeSet<_>>().len() >= 2
                    && !board.tracks.iter().any(|t| t.side == p.side && (t.a.dist(p.pos) < 0.05 || t.b.dist(p.pos) < 0.05));
                if !needs {
                    continue;
                }
                let (pw, ph) = p.shape.size();
                let half = if normal.0 != 0.0 { pw } else { ph } / 2.0;
                let rel = k as f64 - (n as f64 - 1.0) / 2.0;
                let d_end = k.min(n - 1 - k) as f64;
                let a = p.pos;
                // the ways out, the best first: the pins in the middle go straight; the others turn outward, with a shorter run when the long one
                // would come too near another net
                let mut ways: Vec<Vec<Point>> = vec![];
                if rel.abs() < 0.75 {
                    ways.push(vec![a, Point::new(p.pos.x + normal.0 * (half + MID_LEN), p.pos.y + normal.1 * (half + MID_LEN))]);
                } else {
                    let h = half + BASE + STEP * d_end;
                    let b = Point::new(p.pos.x + normal.0 * h, p.pos.y + normal.1 * h);
                    let dir = rel.signum();
                    for run in [RUN, 0.5, 0.3] {
                        ways.push(vec![a, b, Point::new(b.x + along.0 * dir * run, b.y + along.1 * dir * run)]);
                    }
                    ways.push(vec![a, b]);
                }
                let fits = |way: &Vec<Point>| -> bool {
                    way.windows(2).all(|s| {
                        let pads_ok = board.pads.iter().filter(|q| !(q.refdes == p.refdes && q.pin == p.pin)).filter(|q| q.drill.is_some() || q.side == p.side).all(|q| {
                            let (qw, qh) = q.shape.size();
                            let d = crate::drc::geo::seg_rect(s[0], s[1], q.pos, qw / 2.0, qh / 2.0) - w / 2.0;
                            // another net keeps its distance; the same net either touches or stays away (a hair of a gap is a slit)
                            if q.net == p.net && !q.net.is_empty() { d >= gap || d <= 0.0 } else { d >= gap }
                        });
                        let tracks_ok = made.iter().filter(|m| m.0 != p.net && m.1.side == p.side).all(|m| crate::drc::geo::seg_seg(s[0], s[1], m.1.a, m.1.b) - (w + m.1.width) / 2.0 >= gap);
                        pads_ok && tracks_ok
                    })
                };
                let Some(way) = ways.iter().find(|w| fits(w)) else { continue };
                for s in way.windows(2) {
                    let t = Track { side: p.side, a: s[0], b: s[1], width: w };
                    made.push((p.net.clone(), t.clone()));
                    board.tracks.push(t);
                }
                drawn += 1;
            }
        }
        if drawn == 0 {
            continue;
        }
        if check(board).len() > before {
            board.tracks.truncate(mark);
            report.dropped.push(r);
        } else {
            report.stubs += drawn;
            report.parts.push(r);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;
    use crate::package::PackageSpec;

    /// A 20-pin quad package with a pitch of 0.5 mm, every pin wired to a resistor of its own.
    fn quad() -> Board {
        let mut b = Board::new("qfn", 40.0, 40.0);
        let mut fp = PackageSpec::Quad { pins: 20, pitch: 0.5, span: 4.0, pad_len: 0.8, pad_width: 0.25 }.build("QUAD20").unwrap();
        fp.name = "QUAD20".into();
        b.footprints.push(fp);
        b.footprints.push(PackageSpec::Chip2 { pitch: 1.0, pad_len: 0.6, pad_width: 0.6 }.build("R0402").unwrap());
        let mut parts = vec![NetlistPart { refdes: "U1".into(), value: "Q".into(), footprint: "QUAD20".into() }];
        let mut nets = vec![];
        for k in 1..=20 {
            parts.push(NetlistPart { refdes: format!("R{k}"), value: "1k".into(), footprint: "R0402".into() });
            nets.push(NetlistNet { name: format!("N{k}"), pins: vec![("U1".into(), k.to_string()), (format!("R{k}"), "1".into())] });
        }
        import(&mut b, &parts, &nets);
        // the resistors stand far from the package, one after another on a line below it
        for (k, p) in b.parts.clone().into_iter().filter(|p| p.refdes != "U1").enumerate() {
            b.place_part(&p.refdes, Point::new(2.0 + (k % 10) as f64 * 3.5, 25.0 + (k / 10) as f64 * 4.0), 0);
        }
        b.place_part("U1", Point::new(20.0, 12.0), 0);
        b
    }

    #[test]
    fn the_pins_of_a_fine_pitch_package_get_stubs_that_keep_the_drc_clean() {
        let mut b = quad();
        let drc = crate::drc::check(&b).len();
        let r = breakout(&mut b, 0.2);
        assert!(r.stubs >= 10, "{r:?}");
        assert_eq!(r.parts, vec!["U1".to_string()]);
        assert!(crate::drc::check(&b).len() <= drc);
        // the pins in the middle of a side go straight out (one piece), the ones at the ends turn (two pieces)
        assert!(b.tracks.len() > r.stubs);
    }

    #[test]
    fn a_coarse_pitch_package_gets_none() {
        let mut b = quad();
        for p in b.pads.iter_mut().filter(|p| p.refdes == "U1") {
            p.pos = Point::new(20.0 + (p.pos.x - 20.0) * 3.0, 12.0 + (p.pos.y - 12.0) * 3.0);
        }
        assert_eq!(breakout(&mut b, 0.2).stubs, 0);
    }
}
