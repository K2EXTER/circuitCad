//! Characteristic impedance of a track from its width, and the width for a wanted impedance: a rule of thumb from the textbook
//! closed forms (Hammerstad-Jensen for a microstrip, the IPC-2141 forms for a stripline and for a pair), good to about ±10 %.
//! A maker's own figure (from its field solver and its real stack-up) is the one to trust; this is for choosing a width to start from.
//!
//! The layer stack is `Stackup`: the permittivity of the board, the distance from an outer layer to the plane it runs over, and the
//! copper thickness. The defaults are those of a plain 1.6 mm two-layer board; a maker's page has the real ones.

use crate::model::{Board, Side};

/// What the impedance depends on besides the track.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Stackup {
    /// Relative permittivity of the board material (FR-4 is about 4.2 to 4.6, and falls a little with frequency).
    pub er: f64,
    /// Distance (mm) from a track on an outer layer to the copper it runs over: the other layer of a two-layer board, or the plane
    /// next to the outer layer of a four-layer one.
    pub height: f64,
    /// Distance (mm) between the two planes an inner layer lies between (a stripline), the track in the middle of them.
    pub plane_gap: f64,
    /// Copper thickness (µm): 35 for "1 oz".
    pub copper_um: f64,
}

impl Default for Stackup {
    fn default() -> Self {
        Self { er: 4.5, height: 1.53, plane_gap: 0.36, copper_um: 35.0 }
    }
}

/// How a track sits: on an outer layer over one plane (microstrip), or between two planes on an inner layer (stripline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Microstrip,
    Stripline,
}

impl Kind {
    /// The kind for a track on `side` of `board` (a layer between the outer ones is a stripline).
    pub fn of(side: Side) -> Kind {
        match side {
            Side::Inner(_) => Kind::Stripline,
            _ => Kind::Microstrip,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Microstrip => "外層 (マイクロストリップ)",
            Kind::Stripline => "内層 (ストリップライン)",
        }
    }
}

/// Characteristic impedance (Ω) of a single track of `width` mm.
pub fn single_ended(s: &Stackup, kind: Kind, width: f64) -> f64 {
    let t = s.copper_um / 1000.0;
    match kind {
        Kind::Microstrip => {
            let h = s.height;
            // the copper's own thickness makes the track act wider
            let dw = if width / h <= 1.0 / (2.0 * std::f64::consts::PI) {
                t / std::f64::consts::PI * (1.0 + (4.0 * std::f64::consts::PI * width / t).ln())
            } else {
                t / std::f64::consts::PI * (1.0 + (2.0 * h / t).ln())
            };
            let u = (width + dw) / h;
            let er_eff = (s.er + 1.0) / 2.0 + (s.er - 1.0) / 2.0 * (1.0 + 12.0 / u).powf(-0.5) + if u < 1.0 { 0.04 * (1.0 - u).powi(2) * (s.er - 1.0) / 2.0 } else { 0.0 };
            if u <= 1.0 {
                60.0 / er_eff.sqrt() * (8.0 / u + u / 4.0).ln()
            } else {
                120.0 * std::f64::consts::PI / (er_eff.sqrt() * (u + 1.393 + 0.667 * (u + 1.444).ln()))
            }
        }
        Kind::Stripline => {
            let b = s.plane_gap;
            60.0 / s.er.sqrt() * (4.0 * b / (0.67 * std::f64::consts::PI * (0.8 * width + t))).ln()
        }
    }
}

/// Differential impedance (Ω) of a pair of tracks of `width` mm, `space` mm apart (edge to edge), driven in opposite directions.
pub fn differential(s: &Stackup, kind: Kind, width: f64, space: f64) -> f64 {
    let z0 = single_ended(s, kind, width);
    match kind {
        Kind::Microstrip => 2.0 * z0 * (1.0 - 0.48 * (-0.96 * space / s.height).exp()),
        Kind::Stripline => 2.0 * z0 * (1.0 - 0.347 * (-2.9 * space / s.plane_gap).exp()),
    }
}

/// The widest and the thinnest width the closed forms are asked about: outside them the answer is not worth much.
const WIDTH_RANGE: (f64, f64) = (0.03, 20.0);

/// The width (mm) for a wanted single-ended impedance (Ω), or `None` when no width in a sensible range gives it (the stack-up
/// cannot reach that impedance).
pub fn width_for(s: &Stackup, kind: Kind, target: f64) -> Option<f64> {
    solve(|w| single_ended(s, kind, w), target)
}

/// The width (mm) of each track of a pair `space` mm apart for a wanted differential impedance (Ω).
pub fn width_for_differential(s: &Stackup, kind: Kind, space: f64, target: f64) -> Option<f64> {
    solve(|w| differential(s, kind, w, space), target)
}

/// The edge-to-edge space (mm) of a pair of tracks of `width` mm for a wanted differential impedance (Ω). The impedance rises with the
/// space, up to twice the single-ended value, so a target above that is out of reach.
pub fn space_for_differential(s: &Stackup, kind: Kind, width: f64, target: f64) -> Option<f64> {
    // increasing in the space
    let f = |sp: f64| differential(s, kind, width, sp);
    let (mut lo, mut hi) = (0.01, 20.0);
    if target < f(lo) || target > f(hi) {
        return None;
    }
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if f(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// `f` falls as the width grows: the width where it is `target`, by bisection.
fn solve(f: impl Fn(f64) -> f64, target: f64) -> Option<f64> {
    let (mut lo, mut hi) = WIDTH_RANGE;
    if !(f(hi)..=f(lo)).contains(&target) {
        return None;
    }
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if f(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// A track of a net with a wanted impedance whose width is not the one that gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImpedanceIssue {
    pub net: String,
    pub target: f64,
    /// The impedance of the track that is furthest from the target, and its width.
    pub worst_z: f64,
    pub worst_width: f64,
    pub at: crate::model::Point,
}

/// The tracks of the nets that have a wanted impedance (in their class) whose impedance is more than `tolerance` (a fraction, 0.1 = 10 %)
/// off it.
pub fn check_targets(board: &Board, tolerance: f64) -> Vec<ImpedanceIssue> {
    let conn = crate::drc::connectivity(board);
    let mut out: Vec<ImpedanceIssue> = vec![];
    for class in &board.net_classes {
        let Some(target) = class.impedance else { continue };
        for net in &class.nets {
            let mut worst: Option<(f64, f64, crate::model::Point)> = None;
            for (i, t) in board.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net.as_str())) {
                let _ = i;
                let z = single_ended(&board.stackup, Kind::of(t.side), t.width);
                if (z - target).abs() / target > tolerance && worst.is_none_or(|w| (z - target).abs() > (w.0 - target).abs()) {
                    worst = Some((z, t.width, t.a));
                }
            }
            if let Some((z, w, at)) = worst {
                out.push(ImpedanceIssue { net: net.clone(), target, worst_z: z, worst_width: w, at });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fifty_ohm_microstrip_on_1_6_mm_fr4_is_about_three_mm_wide() {
        let s = Stackup::default();
        let w = width_for(&s, Kind::Microstrip, 50.0).unwrap();
        // the usual figure for a plain two-layer board
        assert!((2.6..=3.4).contains(&w), "{w}");
        assert!((single_ended(&s, Kind::Microstrip, w) - 50.0).abs() < 0.01, "the inverse gives back what was asked");
    }

    #[test]
    fn a_wider_track_has_a_lower_impedance_and_a_closer_pair_too() {
        let s = Stackup::default();
        assert!(single_ended(&s, Kind::Microstrip, 0.2) > single_ended(&s, Kind::Microstrip, 1.0));
        assert!(single_ended(&s, Kind::Stripline, 0.15) > single_ended(&s, Kind::Stripline, 0.3));
        assert!(differential(&s, Kind::Microstrip, 0.3, 0.15) < differential(&s, Kind::Microstrip, 0.3, 0.5));
        // a pair far apart is two single tracks
        let (z0, zd) = (single_ended(&s, Kind::Microstrip, 0.3), differential(&s, Kind::Microstrip, 0.3, 50.0));
        assert!((zd - 2.0 * z0).abs() < 0.5);
    }

    #[test]
    fn a_thin_core_makes_a_narrow_fifty_ohm_track() {
        // 0.2 mm to the plane (a four-layer board): about 0.35 mm for 50 ohm
        let s = Stackup { height: 0.2, ..Default::default() };
        let w = width_for(&s, Kind::Microstrip, 50.0).unwrap();
        assert!((0.3..=0.45).contains(&w), "{w}");
    }

    #[test]
    fn a_ninety_ohm_pair_is_found_by_width_and_by_space() {
        let s = Stackup { height: 0.2, ..Default::default() };
        let w = width_for_differential(&s, Kind::Microstrip, 0.2, 90.0).unwrap();
        assert!((differential(&s, Kind::Microstrip, w, 0.2) - 90.0).abs() < 0.01);
        let sp = space_for_differential(&s, Kind::Microstrip, 0.25, 90.0).unwrap();
        assert!((differential(&s, Kind::Microstrip, 0.25, sp) - 90.0).abs() < 0.01);
        // not reachable: above twice the single-ended impedance of such a track
        assert!(space_for_differential(&s, Kind::Microstrip, 1.0, 400.0).is_none());
    }

    #[test]
    fn an_impossible_impedance_has_no_width() {
        // a 1.5 ohm line is not a track on this board
        assert!(width_for(&Stackup::default(), Kind::Microstrip, 1.5).is_none());
    }

    fn board_with_class(width: f64, target: f64) -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        for x in [10.0, 30.0] {
            let mut p = crate::model::Pad::new(crate::model::Point::new(x, 15.0), crate::model::PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = "CLK".into();
            b.pads.push(p);
        }
        b.tracks.push(crate::model::Track { side: Side::Top, a: crate::model::Point::new(10.0, 15.0), b: crate::model::Point::new(30.0, 15.0), width });
        b.net_classes.push(crate::model::NetClass { name: "高速".into(), nets: vec!["CLK".into()], track_width: width, impedance: Some(target), ..Default::default() });
        b
    }

    #[test]
    fn a_track_off_the_wanted_impedance_is_found_and_one_on_it_is_not() {
        let w50 = width_for(&Stackup::default(), Kind::Microstrip, 50.0).unwrap();
        assert!(check_targets(&board_with_class(w50, 50.0), 0.1).is_empty());
        let off = check_targets(&board_with_class(0.25, 50.0), 0.1);
        assert_eq!(off.len(), 1);
        assert!(off[0].worst_z > 90.0, "a 0.25 mm track on 1.53 mm of FR-4 is far above 50 ohm: {:?}", off[0]);
        // the order check says so
        let checks = crate::fab::preflight(&board_with_class(0.25, 50.0), None);
        assert!(checks.iter().any(|c| c.level == crate::fab::Level::Warn && c.title.contains("インピーダンス")), "{:?}", checks.iter().map(|c| &c.title).collect::<Vec<_>>());
    }

    #[test]
    fn the_stackup_is_written_only_when_it_is_not_the_default() {
        let mut b = Board::new("t", 10.0, 10.0);
        assert!(!serde_json::to_string(&b).unwrap().contains("stackup"));
        b.stackup.er = 4.2;
        let back: Board = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(back.stackup.er, 4.2);
    }
}
