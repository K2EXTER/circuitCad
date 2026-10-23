//! How much current a copper track can carry (IPC-2221 formula), the track width that goes with a
//! current, and a per-net report against the tracks actually drawn.

use crate::drc::Connectivity;
use crate::model::*;

/// Conditions behind a width / current figure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CopperSpec {
    /// Copper thickness in micrometres (35 = 1 oz, the usual outer layer of a cheap board).
    pub thickness_um: f64,
    /// Allowed temperature rise above the surroundings (degrees C).
    pub rise_c: f64,
    /// Outer layers cool better than inner ones (k = 0.048 instead of 0.024).
    pub external: bool,
}

impl Default for CopperSpec {
    fn default() -> Self {
        Self { thickness_um: 35.0, rise_c: 10.0, external: true }
    }
}

const MIL_MM: f64 = 0.0254;

impl CopperSpec {
    fn k(&self) -> f64 {
        if self.external { 0.048 } else { 0.024 }
    }

    fn thickness_mil(&self) -> f64 {
        self.thickness_um / 1000.0 / MIL_MM
    }
}

/// Smallest track width (mm) that carries `amps` within the temperature rise: IPC-2221,
/// I = k * dT^0.44 * A^0.725 with A the cross-section in square mil.
pub fn width_for_current(amps: f64, spec: &CopperSpec) -> f64 {
    if !(amps > 0.0) || !(spec.rise_c > 0.0) || !(spec.thickness_um > 0.0) {
        return 0.0;
    }
    let area_mil2 = (amps / (spec.k() * spec.rise_c.powf(0.44))).powf(1.0 / 0.725);
    area_mil2 / spec.thickness_mil() * MIL_MM
}

/// Current (A) a track of `width_mm` carries within the temperature rise (the inverse of [`width_for_current`]).
pub fn current_for_width(width_mm: f64, spec: &CopperSpec) -> f64 {
    if !(width_mm > 0.0) || !(spec.rise_c > 0.0) || !(spec.thickness_um > 0.0) {
        return 0.0;
    }
    let area_mil2 = width_mm / MIL_MM * spec.thickness_mil();
    spec.k() * spec.rise_c.powf(0.44) * area_mil2.powf(0.725)
}

/// Round a width up to the next 0.05 mm, a size that is easy to read and to draw.
pub fn round_up_width(w: f64) -> f64 {
    ((w / 0.05) - 1e-9).ceil().max(1.0) * 0.05
}

/// One net with a declared current, and how its tracks compare with what that current needs.
#[derive(Clone, Debug, PartialEq)]
pub struct NetWidth {
    pub net: String,
    pub amps: f64,
    /// Width this current needs (mm), not rounded.
    pub needed: f64,
    /// Thinnest track of the net (mm); `None` when the net has no track yet.
    pub thinnest: Option<f64>,
    /// Where the thinnest track starts.
    pub at: Option<Point>,
}

impl NetWidth {
    pub fn too_thin(&self) -> bool {
        self.thinnest.is_some_and(|w| w + 1e-9 < self.needed)
    }
}

/// The longest (mm) a thin stub on a pad can be and still count as a neck.
const NECK_MAX: f64 = 1.2;

/// Is track `i` a neck: a short stub that leaves a pad and runs into a wider track of the same net (the necks of a trunk that joins two
/// pads, the thin lead of a fan-out)? Such a stub is thin on purpose, carries only its pad's share, and is not what limits the net.
pub fn is_neck(board: &Board, conn: &Connectivity, i: usize) -> bool {
    let t = &board.tracks[i];
    if t.a.dist(t.b) > NECK_MAX {
        return false;
    }
    let on_pad = |p: Point| {
        board.pads.iter().any(|q| {
            let (w, h) = q.shape.size();
            (q.drill.is_some() || q.side == t.side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
        })
    };
    // the end of the stub lies on a wider track of the net (on its copper, not only at its end: a trunk's round end covers the necks' ends)
    let dist_to = |u: &Track, p: Point| {
        let (dx, dy) = (u.b.x - u.a.x, u.b.y - u.a.y);
        let l2 = dx * dx + dy * dy;
        let s = if l2 < 1e-18 { 0.0 } else { (((p.x - u.a.x) * dx + (p.y - u.a.y) * dy) / l2).clamp(0.0, 1.0) };
        p.dist(Point::new(u.a.x + s * dx, u.a.y + s * dy))
    };
    let joins_wider = |p: Point| {
        board.tracks.iter().enumerate().any(|(k, u)| k != i && u.side == t.side && u.width > t.width + 0.05 && conn.track_net(k) == conn.track_net(i) && dist_to(u, p) <= u.width / 2.0 + 0.02)
    };
    (on_pad(t.a) && joins_wider(t.b)) || (on_pad(t.b) && joins_wider(t.a))
}

/// For every net with a current in `board.net_currents`: the needed width against the tracks drawn (the necks of a trunk are left out).
pub fn net_widths(board: &Board, conn: &Connectivity, spec: &CopperSpec) -> Vec<NetWidth> {
    board
        .net_currents
        .iter()
        .filter(|(_, a)| **a > 0.0)
        .map(|(net, amps)| {
            let mut thinnest: Option<(f64, Point)> = None;
            for (i, t) in board.tracks.iter().enumerate() {
                if conn.track_net(i) == Some(net.as_str()) && thinnest.is_none_or(|(w, _)| t.width < w) && !is_neck(board, conn, i) {
                    thinnest = Some((t.width, t.a));
                }
            }
            NetWidth { net: net.clone(), amps: *amps, needed: width_for_current(*amps, spec), thinnest: thinnest.map(|t| t.0), at: thinnest.map(|t| t.1) }
        })
        .collect()
}

/// Declares the given `(net, amps)` as the board's net currents, for the nets that have pads on the board
/// (the simulation's current of every other net is dropped). The old declarations are replaced.
/// Returns how many nets got a current.
pub fn set_net_currents(board: &mut Board, currents: &[(String, f64)]) -> usize {
    let on_board: std::collections::HashSet<&str> = board.pads.iter().filter(|p| !p.net.is_empty()).map(|p| p.net.as_str()).collect();
    let new: Vec<(String, f64)> = currents.iter().filter(|(n, a)| *a > 0.0 && on_board.contains(n.as_str())).cloned().collect();
    board.net_currents.clear();
    for (n, a) in &new {
        board.net_currents.insert(n.clone(), *a);
    }
    new.len()
}

/// Sets every track of `net` to at least `width` (never thinner than it is). Returns how many tracks were widened.
pub fn widen_net(board: &mut Board, conn: &Connectivity, net: &str, width: f64) -> usize {
    let mut n = 0;
    // the necks of a trunk stay as thin as they are
    let necks: Vec<bool> = (0..board.tracks.len()).map(|i| conn.track_net(i) == Some(net) && is_neck(board, conn, i)).collect();
    for (i, t) in board.tracks.iter_mut().enumerate() {
        if conn.track_net(i) == Some(net) && t.width + 1e-9 < width && !necks[i] {
            t.width = width;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ipc_2221_values() {
        let s = CopperSpec::default();
        // 1 A, 10 C rise, 1 oz outer layer: about 0.3 mm (12 mil)
        let w = width_for_current(1.0, &s);
        assert!((0.25..0.35).contains(&w), "{w}");
        // 3 A needs a bit over 4x the width of 1 A (width grows faster than the current: A ~ I^1.38)
        let w3 = width_for_current(3.0, &s);
        assert!((1.1..1.5).contains(&w3), "{w3}");
        // inner layers need about twice the width
        let inner = width_for_current(1.0, &CopperSpec { external: false, ..s });
        assert!((inner / w - 2.5).abs() < 0.5, "{inner} vs {w}");
        // 2 oz copper halves the width
        let oz2 = width_for_current(1.0, &CopperSpec { thickness_um: 70.0, ..s });
        assert!((oz2 / w - 0.5).abs() < 0.05);
    }

    #[test]
    fn width_and_current_are_inverse() {
        let s = CopperSpec::default();
        for a in [0.1, 0.5, 2.0, 5.0] {
            assert!((current_for_width(width_for_current(a, &s), &s) - a).abs() < 1e-9);
        }
        assert_eq!(width_for_current(0.0, &s), 0.0);
        assert_eq!(current_for_width(-1.0, &s), 0.0);
    }

    #[test]
    fn widths_round_up_to_a_clean_size() {
        assert!((round_up_width(0.31) - 0.35).abs() < 1e-9);
        assert!((round_up_width(0.30) - 0.30).abs() < 1e-9);
        assert!((round_up_width(0.01) - 0.05).abs() < 1e-9);
    }

    #[test]
    fn report_finds_thin_tracks_of_a_loaded_net_and_widening_fixes_them() {
        let mut b = Board::new("t", 40.0, 40.0);
        let mut pa = Pad::new(Point::new(5.0, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        pa.net = "VIN".into();
        let mut pb = Pad::new(Point::new(25.0, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        pb.net = "VIN".into();
        b.pads.push(pa);
        b.pads.push(pb);
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(15.0, 10.0), width: 0.25 });
        b.tracks.push(Track { side: Side::Top, a: Point::new(15.0, 10.0), b: Point::new(25.0, 10.0), width: 0.5 });
        b.net_currents.insert("VIN".into(), 2.0);
        let spec = CopperSpec::default();
        let conn = crate::drc::connectivity(&b);
        let r = net_widths(&b, &conn, &spec);
        assert_eq!(r.len(), 1);
        assert!(r[0].too_thin() && r[0].thinnest == Some(0.25), "{:?}", r[0]);
        let need = round_up_width(r[0].needed);
        assert!(need > 0.5, "2 A needs more than the 0.5 mm track: {need}");
        assert_eq!(widen_net(&mut b, &conn, "VIN", need), 2);
        let after = net_widths(&b, &crate::drc::connectivity(&b), &spec);
        assert!(!after[0].too_thin(), "{:?}", after[0]);
    }

    #[test]
    fn preflight_warns_about_thin_tracks_of_a_loaded_net() {
        let mut b = Board::new("t", 40.0, 40.0);
        for x in [5.0, 25.0] {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
            p.net = "VIN".into();
            b.pads.push(p);
        }
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(25.0, 10.0), width: 0.25 });
        let has = |b: &Board, level: crate::fab::Level| crate::fab::preflight(b, None).iter().any(|c| c.level == level && c.title.contains("配線") && c.title.contains("電流"));
        assert!(!has(&b, crate::fab::Level::Warn), "no current declared, nothing to say");
        b.net_currents.insert("VIN".into(), 2.0);
        assert!(has(&b, crate::fab::Level::Warn));
        let conn = crate::drc::connectivity(&b);
        widen_net(&mut b, &conn, "VIN", round_up_width(width_for_current(2.0, &CopperSpec::default())));
        assert!(!has(&b, crate::fab::Level::Warn));
        assert!(crate::fab::preflight(&b, None).iter().any(|c| c.level == crate::fab::Level::Ok && c.title.contains("電流を設定したネット")));
    }

    #[test]
    fn simulated_currents_replace_the_old_ones_and_skip_nets_not_on_the_board() {
        let mut b = Board::new("t", 50.0, 50.0);
        let mut p = Pad::new(Point::new(1.0, 1.0), PadShape::Circle { d: 1.6 }, None, Side::Top);
        p.net = "VIN".into();
        b.pads.push(p);
        b.net_currents.insert("OLD".into(), 9.0);
        let n = set_net_currents(&mut b, &[("VIN".into(), 1.5), ("GHOST".into(), 2.0), ("VIN2".into(), 0.0)]);
        assert_eq!(n, 1);
        assert_eq!(b.net_currents.len(), 1);
        assert_eq!(b.net_currents["VIN"], 1.5);
    }
}
