//! Which tracks are long enough to act as transmission lines (S1): a net's one-way delay along its longest copper way from pad to pad,
//! against the rise time of the edge that drives it. Past about 1/6 of the rise time the far end sees the edge before the reflection
//! from it has died away, and the edge rings (overshoot, undershoot, a false second edge on a clock): the net wants a termination (a
//! series resistor of about 22-47 Ω at the driver, say) or a shorter track. Below it the track is a plain wire.
//!
//! The delay per mm is the field solver's (`fieldsolve`: the layer stack, the copper, the mask) for the track's layer and width, so a
//! microstrip on FR-4 is about 6 ps/mm and a stripline about 7 ps/mm. The rise time is the net class's (`NetClass.rise_ns`) or, for a
//! signal net that has none, `DEFAULT_RISE_NS` (the fast edge of a modern microcontroller's pin); rails (by name, pour or declared
//! current) and the classes of power, sensing and analog nets are left out unless their class gives a rise time.

use crate::drc::{CopperGraph, PadRef};
use crate::fieldsolve::{Section, Solved};
use crate::impedance::Kind;
use crate::model::{Board, Point, Role, Side};

/// The rise time (ns) assumed for a signal net whose class gives none: the fast edge of a modern microcontroller's pin (an STM32 or
/// RP2040 pin at its fast setting is 1-3 ns; 74HC logic about 6 ns, 74LVC and 74AC about 1-2 ns).
pub const DEFAULT_RISE_NS: f64 = 2.0;
/// The part of the rise time past which a track acts as a line (the usual rule of thumb: delay > tr / 6).
pub const FRACTION: f64 = 1.0 / 6.0;
/// The most pads of a net whose ways are all tried (each pair is one search); a net with more uses the first ones.
const MAX_PADS: usize = 16;

/// Where a net's rise time comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiseFrom {
    /// The net class gives it.
    Class,
    /// The rule of thumb `DEFAULT_RISE_NS`.
    Default,
}

/// One net's longest way and what its delay means.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCheck {
    pub net: String,
    pub rise_ns: f64,
    pub from: RiseFrom,
    /// The longest way (mm) along the copper between two pads of the net, and its one-way delay (ps).
    pub length: f64,
    pub delay_ps: f64,
    /// The pads at its ends ("U1.5").
    pub ends: (String, String),
    /// The length (mm) a way of this net may have before it acts as a line (with the delay per mm of this way).
    pub critical_mm: f64,
    /// The middle of the way (where the editor points).
    pub at: Point,
}

impl LineCheck {
    /// The delay as a part of the rise time (1/6 = 0.167 is the line).
    pub fn ratio(&self) -> f64 {
        self.delay_ps / (self.rise_ns * 1000.0)
    }
    /// Does the way act as a transmission line (reflections can ring)?
    pub fn is_line(&self) -> bool {
        self.ratio() > FRACTION
    }
}

/// The rise time of a net and where it comes from; None for a net that is not checked (a rail, a pour, a declared current, a power,
/// sensing or analog class) unless its class gives one.
pub fn rise_of(board: &Board, net: &str) -> Option<(f64, RiseFrom)> {
    let class = board.class_of(net);
    if let Some(r) = class.and_then(|c| c.rise_ns).filter(|r| *r > 0.0) {
        return Some((r, RiseFrom::Class));
    }
    let rail = crate::drc::is_ground_net(net) || crate::drc::is_supply_net(net) || crate::drc::is_negative_supply_net(net);
    let poured = board.zones.iter().any(|z| z.net == net);
    let current = board.net_currents.get(net).is_some_and(|a| *a > 0.0);
    let quiet_role = class.is_some_and(|c| matches!(c.role, Role::Power | Role::Sense | Role::Analog));
    (!(rail || poured || current || quiet_role)).then_some((DEFAULT_RISE_NS, RiseFrom::Default))
}

/// The one-way delay (ps/mm) of a track of `width` on `side` (the field solver; the same question again is answered from memory).
pub fn delay_ps_mm(board: &Board, side: Side, width: f64) -> f64 {
    match crate::fieldsolve::solve(&board.stackup, &Section::single(Kind::of(side), width.max(0.01))) {
        Solved::Single(m) => m.delay_ps_mm,
        Solved::Pair { odd, .. } => odd.delay_ps_mm,
    }
}

/// Every checked net that has a copper way between two of its pads, by name.
pub fn check(board: &Board) -> Vec<LineCheck> {
    let mut nets: std::collections::BTreeMap<&str, Vec<PadRef>> = Default::default();
    for p in board.pads.iter().filter(|p| !p.net.is_empty() && !p.refdes.is_empty()) {
        let r = PadRef { refdes: p.refdes.clone(), pin: p.pin.clone() };
        let list = nets.entry(p.net.as_str()).or_default();
        if !list.contains(&r) {
            list.push(r);
        }
    }
    nets.retain(|_, pads| pads.len() >= 2);
    if nets.is_empty() || board.tracks.is_empty() {
        return vec![];
    }
    let conn = crate::drc::connectivity(board);
    let graph = CopperGraph::build(board);
    let mut out = vec![];
    for (net, pads) in nets {
        let Some((rise_ns, from)) = rise_of(board, net) else { continue };
        let tracks: Vec<&crate::model::Track> = board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).map(|(_, t)| t).collect();
        if tracks.is_empty() {
            continue;
        }
        let pads = &pads[..pads.len().min(MAX_PADS)];
        // the longest way by delay (a way on a slower layer can be the longer in time though shorter in mm)
        let mut best: Option<(f64, f64, usize, usize, Point)> = None;
        for i in 0..pads.len() {
            for j in i + 1..pads.len() {
                let Some(way) = graph.path(&pads[i], &pads[j]) else { continue };
                let mut delay = 0.0;
                for w in way.points.windows(2) {
                    let ((sa, a), (sb, b)) = (w[0], w[1]);
                    let len = a.dist(b);
                    if sa != sb || len < 1e-9 {
                        continue;
                    }
                    delay += len * delay_ps_mm(board, sa, width_along(&tracks, sa, a, b));
                }
                if best.is_none_or(|b| delay > b.1) {
                    let at = way.points.get(way.points.len() / 2).map_or(Point::default(), |p| p.1);
                    best = Some((way.length, delay, i, j, at));
                }
            }
        }
        let Some((length, delay_ps, i, j, at)) = best else { continue };
        let per_mm = if length > 1e-9 && delay_ps > 0.0 { delay_ps / length } else { delay_ps_mm(board, tracks[0].side, tracks[0].width) };
        out.push(LineCheck {
            net: net.to_string(),
            rise_ns,
            from,
            length,
            delay_ps,
            ends: (pads[i].to_string(), pads[j].to_string()),
            critical_mm: rise_ns * 1000.0 * FRACTION / per_mm,
            at,
        });
    }
    out
}

/// The nets whose longest way acts as a transmission line, the furthest over first.
pub fn long_lines(board: &Board) -> Vec<LineCheck> {
    let mut v: Vec<LineCheck> = check(board).into_iter().filter(LineCheck::is_line).collect();
    v.sort_by(|a, b| b.ratio().total_cmp(&a.ratio()));
    v
}

/// The width of the track of the net that the stretch a-b lies along (the nearest one on that layer).
fn width_along(tracks: &[&crate::model::Track], side: Side, a: Point, b: Point) -> f64 {
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    tracks
        .iter()
        .filter(|t| t.side == side)
        .min_by(|x, y| seg_dist(mid, x.a, x.b).total_cmp(&seg_dist(mid, y.a, y.b)))
        .map_or(0.25, |t| t.width)
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let s = if l2 < 1e-18 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    p.dist(Point::new(a.x + s * dx, a.y + s * dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    /// Two pads `len` mm apart on net `net`, joined by one straight track on the top.
    fn board(net: &str, len: f64) -> Board {
        let mut b = Board::new("t", len + 20.0, 30.0);
        for (k, x) in [(1, 10.0), (2, 10.0 + len)] {
            let mut p = Pad::new(Point::new(x, 15.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            p.refdes = format!("U{k}");
            p.pin = "1".into();
            b.pads.push(p);
        }
        b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 15.0), b: Point::new(10.0 + len, 15.0), width: 0.25 });
        b
    }

    #[test]
    fn a_microstrip_on_fr4_is_about_six_ps_per_mm() {
        let b = board("CLK", 10.0);
        let d = delay_ps_mm(&b, Side::Top, 0.25);
        // εeff of a thin track on 1.5 mm of FR-4 (εr 4.5) with a mask is about 3.2: 3.336 × √3.2 = 6.0 ps/mm
        assert!((5.5..=6.5).contains(&d), "{d}");
        assert!(delay_ps_mm(&b, Side::Inner(1), 0.15) > d, "a stripline is all in the board: slower");
    }

    #[test]
    fn a_long_clock_track_acts_as_a_line_and_a_short_one_does_not() {
        // 2 ns / 6 = 333 ps: about 55 mm of microstrip
        let short = check(&board("CLK", 30.0));
        assert_eq!(short.len(), 1);
        assert!(!short[0].is_line(), "{:?}", short[0]);
        assert!((50.0..=62.0).contains(&short[0].critical_mm), "{:?}", short[0]);
        assert_eq!(short[0].from, RiseFrom::Default);
        assert_eq!(short[0].ends, ("U1.1".to_string(), "U2.1".to_string()));
        let long = long_lines(&board("CLK", 80.0));
        assert_eq!(long.len(), 1);
        assert!((long[0].length - 80.0).abs() < 1e-6 && long[0].delay_ps > 400.0, "{:?}", long[0]);
    }

    #[test]
    fn a_class_rise_time_decides_and_rails_are_left_out_unless_it_gives_one() {
        // a slow edge (74HC at 5 V is about 6 ns): 80 mm is still a wire
        let mut b = board("CLK", 80.0);
        b.net_classes.push(NetClass { name: "遅い".into(), nets: vec!["CLK".into()], rise_ns: Some(6.0), ..Default::default() });
        let c = check(&b);
        assert_eq!((c[0].from, c[0].is_line()), (RiseFrom::Class, false));
        // a rail, a poured net and a net of a power class are not looked at
        assert!(check(&board("VCC", 80.0)).is_empty());
        assert!(check(&board("GND", 80.0)).is_empty());
        let mut p = board("MOTOR", 80.0);
        p.net_classes.push(NetClass { name: "電源".into(), nets: vec!["MOTOR".into()], role: Role::Power, ..Default::default() });
        assert!(check(&p).is_empty());
        // unless the class gives a rise time
        p.net_classes[0].rise_ns = Some(1.0);
        assert!(check(&p)[0].is_line());
    }

    /// The order check tells of a long line: for information with the rule of thumb, as a warning when the class gives the rise time.
    #[test]
    fn the_order_check_tells_of_a_long_line() {
        let find = |b: &Board| crate::fab::preflight(b, None).into_iter().find(|c| c.title.contains("伝送線路")).map(|c| (c.level, c.detail));
        let mut b = board("CLK", 80.0);
        let (level, detail) = find(&b).expect("a long clock is told of");
        assert_eq!(level, crate::fab::Level::Info);
        assert!(detail.contains("U1.1") && detail.contains("ダンピング抵抗"), "{detail}");
        b.net_classes.push(NetClass { name: "クロック".into(), nets: vec!["CLK".into()], rise_ns: Some(1.0), ..Default::default() });
        assert_eq!(find(&b).map(|f| f.0), Some(crate::fab::Level::Warn));
        assert!(find(&board("CLK", 30.0)).is_none(), "a short one is not");
    }

    #[test]
    fn an_unrouted_net_has_no_row() {
        let mut b = board("CLK", 80.0);
        b.tracks.clear();
        assert!(check(&b).is_empty());
    }
}
