//! How much a part's heat is helped by the copper it sits on: the copper joined to its pads near it, and the
//! thermal resistance (θ, K/W) that copper brings down.
//!
//! A part's θ to the air is given for the smallest pads (the datasheet's "minimum pad", or the guess from the
//! package's rated power). Copper joined to its pins spreads the heat into a bigger surface: a conductance `G`
//! (W/K) in parallel, `θ = 1 / (1/θ_part + G)`. `G` grows with the copper added around the pads, less than in
//! proportion (the far copper is cooler: heat has to spread through 35 µm of copper to reach it):
//! `G = k · A^n`, fitted to one datasheet curve, NDT3055L (onsemi, SOT-223, 2 oz copper):
//! 110 K/W on the minimum pad, 95 K/W on 0.066 in², 42 K/W on 1 in². The pads of SOT-223's tab and pin 2
//! (10.6 mm²) are the minimum pad, so `A` is what lies beyond them. Thinner copper spreads worse:
//! `G` is scaled by `√(t / 70 µm)` (a wide copper plate's fin conductance goes as √(k·t·h)).
//! This is an estimate: the air, the board's other parts and its inner layers all move it. Measure the real
//! board when it matters (docs/code-notes/pcb-core/heat_copper.md).

use crate::drc::CopperCtx;
use crate::model::{Board, Point};
use std::collections::BTreeMap;

/// Only the copper within this distance (mm) of a part's middle counts: the far copper of a long net is too cool to help.
pub const REACH_MM: f64 = 20.0;

/// The datasheet points: (copper area in mm², θ in K/W) on 2 oz (70 µm) copper, and the minimum pad's area and θ.
const REF_THICKNESS_UM: f64 = 70.0;
const REF_PAD_MM2: f64 = 10.6;
const REF_MIN_THETA: f64 = 110.0;
const REF_POINTS: [(f64, f64); 2] = [(0.066 * 645.16, 95.0), (645.16, 42.0)];

/// `(k, n)` of `G = k · A^n` (W/K, A in mm² beyond the pads) on 70 µm copper, from the datasheet points.
fn fit() -> (f64, f64) {
    let g = |theta: f64| 1.0 / theta - 1.0 / REF_MIN_THETA;
    let [(a1, t1), (a2, t2)] = REF_POINTS;
    let (a1, a2) = (a1 - REF_PAD_MM2, a2 - REF_PAD_MM2);
    let n = (g(t2) / g(t1)).ln() / (a2 / a1).ln();
    (g(t2) / a2.powf(n), n)
}

/// The conductance (W/K) that `extra_mm2` of copper beyond a part's own pads adds, on copper `thickness_um` thick.
pub fn copper_conductance(extra_mm2: f64, thickness_um: f64) -> f64 {
    if !(extra_mm2 > 0.0) || !(thickness_um > 0.0) {
        return 0.0;
    }
    let (k, n) = fit();
    k * extra_mm2.powf(n) * (thickness_um / REF_THICKNESS_UM).sqrt()
}

/// A part's θ with the copper's conductance `g` (W/K) beside its own `theta` (K/W).
pub fn theta_with_copper(theta: f64, g: f64) -> f64 {
    if !(theta > 0.0) {
        return theta;
    }
    1.0 / (1.0 / theta + g.max(0.0))
}

/// The copper of one net of a part.
#[derive(Clone, Debug, PartialEq)]
pub struct NetCopper {
    pub net: String,
    /// All the copper joined to the part's pads of this net within `REACH_MM` (mm², the layers added).
    pub area: f64,
    /// What lies beyond the part's own pads (mm²).
    pub extra: f64,
}

/// The copper a part's heat can spread into.
#[derive(Clone, Debug, PartialEq)]
pub struct PartCopper {
    pub refdes: String,
    /// The part's nets with copper beyond its pads, the most first.
    pub nets: Vec<NetCopper>,
    /// The conductance (W/K) of all of it: each net is a path of its own, so they add.
    pub conductance: f64,
}

impl PartCopper {
    pub fn extra(&self) -> f64 {
        self.nets.iter().map(|n| n.extra).sum()
    }
}

/// The copper of every part of the board (by reference, in order), on copper `thickness_um` thick. Parts whose pads
/// have no net have nothing beyond their pads.
pub fn part_copper(board: &Board, thickness_um: f64) -> Vec<PartCopper> {
    let ctx = CopperCtx::new(board, |_| true);
    // the pads of each part, by net
    let mut parts: BTreeMap<&str, BTreeMap<&str, Vec<usize>>> = BTreeMap::new();
    for (i, p) in board.pads.iter().enumerate() {
        if !p.refdes.is_empty() {
            parts.entry(p.refdes.as_str()).or_default().entry(p.net.as_str()).or_default().push(i);
        }
    }
    let work: Vec<(&str, BTreeMap<&str, Vec<usize>>)> = parts.into_iter().collect();
    crate::par::par_map(&work, |(refdes, nets)| {
        let all: Vec<Point> = nets.values().flatten().map(|&i| board.pads[i].pos).collect();
        let centre = Point::new(all.iter().map(|p| p.x).sum::<f64>() / all.len() as f64, all.iter().map(|p| p.y).sum::<f64>() / all.len() as f64);
        let mut out: Vec<NetCopper> = nets
            .iter()
            .filter(|(net, _)| !net.is_empty())
            .map(|(net, pads)| {
                let area = ctx.joined_area(pads, centre, REACH_MM, false);
                let own = ctx.joined_area(pads, centre, REACH_MM, true);
                NetCopper { net: net.to_string(), area, extra: (area - own).max(0.0) }
            })
            .filter(|n| n.extra > 1e-6)
            .collect();
        out.sort_by(|a, b| b.extra.total_cmp(&a.extra));
        let conductance = out.iter().map(|n| copper_conductance(n.extra, thickness_um)).sum();
        PartCopper { refdes: refdes.to_string(), nets: out, conductance }
    })
}

/// Every part's copper conductance (W/K) by reference, for the simulator's heat (`sim_core::heaters_from_schematic`),
/// with the copper beyond its pads (mm²): only the parts that have some.
pub fn conductances(board: &Board, thickness_um: f64) -> BTreeMap<String, (f64, f64)> {
    part_copper(board, thickness_um).into_iter().filter(|p| p.conductance > 0.0).map(|p| (p.refdes.clone(), (p.conductance, p.extra()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Pad, PadShape, Side, Track, Zone};

    #[test]
    fn the_fit_goes_through_the_datasheet_points() {
        for (area, theta) in REF_POINTS {
            let g = copper_conductance(area - REF_PAD_MM2, REF_THICKNESS_UM);
            let got = theta_with_copper(REF_MIN_THETA, g);
            assert!((got - theta).abs() < 1e-6, "{area} mm²: {got} K/W, the datasheet says {theta}");
        }
        // no copper beyond the pads: the part's own θ
        assert_eq!(theta_with_copper(110.0, copper_conductance(0.0, 35.0)), 110.0);
        // 1 oz spreads less than 2 oz: 1 in² gives about 51 K/W instead of 42
        let one_oz = theta_with_copper(110.0, copper_conductance(645.16 - REF_PAD_MM2, 35.0));
        assert!((48.0..55.0).contains(&one_oz), "{one_oz}");
    }

    fn sot223(b: &mut Board) {
        let pad = |x: f64, y: f64, w: f64, h: f64, pin: &str, net: &str| {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
            p.refdes = "Q1".into();
            p.pin = pin.into();
            p.net = net.into();
            p
        };
        b.pads.push(pad(26.85, 32.3, 2.0, 1.5, "1", "G"));
        b.pads.push(pad(26.85, 30.0, 2.0, 1.5, "2", "DRN"));
        b.pads.push(pad(26.85, 27.7, 2.0, 1.5, "3", "S"));
        b.pads.push(pad(33.15, 30.0, 2.0, 3.8, "2", "DRN"));
    }

    #[test]
    fn a_pour_under_the_tab_brings_theta_down_and_far_copper_does_not_count() {
        let mut b = Board::new("t", 100.0, 60.0);
        sot223(&mut b);
        let bare = part_copper(&b, 70.0);
        assert_eq!(bare.len(), 1);
        assert!(bare[0].nets.is_empty() && bare[0].conductance == 0.0, "{bare:?}");

        // about 1 in² of drain copper around the tab
        let mut z = Zone::new("DRN", Side::Top, vec![Point::new(30.0, 17.3), Point::new(55.4, 17.3), Point::new(55.4, 42.7), Point::new(30.0, 42.7)]);
        z.clearance = 0.3;
        b.zones.push(z);
        let poured = part_copper(&b, 70.0);
        let drn = &poured[0].nets[0];
        assert_eq!(drn.net, "DRN");
        // the reach of 20 mm from the part's middle (x = 30) keeps the half disc to its right that lies within
        // the pour's 25.4 mm height: about 434 mm² of the 645
        assert!(drn.extra > 400.0 && drn.extra < 450.0, "{drn:?}");
        let theta = theta_with_copper(110.0, poured[0].conductance);
        assert!((45.0..60.0).contains(&theta), "{theta}");

        // a long thin track far out adds little
        let mut t = Board::new("t", 100.0, 60.0);
        sot223(&mut t);
        t.tracks.push(Track { side: Side::Top, a: Point::new(33.15, 30.0), b: Point::new(95.0, 30.0), width: 0.3 });
        let tr = part_copper(&t, 70.0);
        assert!(tr[0].nets[0].extra < 20.0 * 0.3 + 1.0, "{:?}", tr[0]);
    }
}
