//! A board in a closed case (candidate H10): how much the case raises the temperature the board sits in. The heat of the parts leaves
//! the board partly by convection into the air inside and partly by radiation straight to the walls; the air gives its share to the
//! inner walls by convection; all of it then crosses the wall (its thickness over its conductivity) and leaves the outer surface by
//! convection and radiation. The air inside is a passage, not a store: its heat capacity is a hundredth of the board's.
//!
//! The board's map (`thermal`) is then solved with the temperature it sees inside as its surroundings: between the air and the wall,
//! by the share of its heat that goes by convection. Where the room to the lid or the floor is thin, the air carries the heat across
//! by conduction (k / gap) when that beats convection (`ThermalSpec::gap_top_mm` / `gap_bottom_mm`).
//!
//! Over time the case is far slower than the board (its walls hold tens of J/K and lose heat through a few K/W: tens of minutes), so the
//! run over time adds the case's own first-order rise on top of the board's (quasi-static).

use crate::thermal::{h_air, pressure_at, HeatSource, Orientation, ThermalMap, ThermalSpec, ThermalTransient, TraceHeat};

/// What the case is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CaseMaterial {
    /// ABS resin: conducts 0.17 W/m·K, 1.05 g/cm³ × 1.4 J/g·K, emissivity 0.9.
    #[default]
    Plastic,
    /// Anodised aluminium: 200 W/m·K, 2.7 g/cm³ × 0.9 J/g·K, emissivity 0.8 (bare aluminium shines, 0.1: anodise or paint it).
    Aluminium,
}

impl CaseMaterial {
    /// (conductivity W/m·K, heat capacity per volume J/m³·K, emissivity of the outside).
    fn props(self) -> (f64, f64, f64) {
        match self {
            CaseMaterial::Plastic => (0.17, 1.05e6 * 1.4, 0.9),
            CaseMaterial::Aluminium => (200.0, 2.7e6 * 0.9, 0.8),
        }
    }
}

/// The case round the board.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CaseHeat {
    /// Inside width and depth (mm).
    pub inner_w: f64,
    pub inner_d: f64,
    /// Room from the board's top face to the lid and from its bottom face to the floor (mm).
    pub top_mm: f64,
    pub bottom_mm: f64,
    pub wall_mm: f64,
    pub material: CaseMaterial,
}

impl Default for CaseHeat {
    fn default() -> Self {
        CaseHeat { inner_w: 0.0, inner_d: 0.0, top_mm: 15.0, bottom_mm: 5.0, wall_mm: 2.0, material: CaseMaterial::Plastic }
    }
}

/// The case worked out for a heat.
#[derive(Clone, Debug, PartialEq)]
pub struct CaseAir {
    /// The heat that crosses the case (W).
    pub power: f64,
    /// The inner wall, the air inside, and what the board sees (°C).
    pub wall_c: f64,
    pub air_c: f64,
    pub board_ambient_c: f64,
    /// The heat resistances (K/W): inside air to the wall, through the wall, from the outside to the room.
    pub r_air: f64,
    pub r_wall: f64,
    pub r_out: f64,
    /// The heat transfer coefficients inside (air to wall) and outside (convection + radiation), W/m²·K.
    pub h_in: f64,
    pub h_out: f64,
    /// The inner surface (m²).
    pub area: f64,
    /// The case's heat capacity (J/K, its walls) and its time to warm (s).
    pub cth: f64,
    pub tau: f64,
}

/// The board's thickness (mm) inside the case's height.
const BOARD_MM: f64 = 1.6;

/// The case for `power` W from the board, the room at `ambient_c`: its walls, the air inside and the temperature the board sees.
/// `convection_share` is the part of the board's heat that goes into the air (the rest radiates to the walls; about a half for a board
/// a few tens of kelvin above its surroundings).
pub fn case_air(power: f64, ambient_c: f64, case: &CaseHeat, orientation: Orientation, altitude_m: f64, convection_share: f64) -> Result<CaseAir, String> {
    if !(case.inner_w > 0.0 && case.inner_d > 0.0) {
        return Err("ケースの内寸 (幅・奥行) を入れてください".into());
    }
    let (w, d, h) = (case.inner_w * 1e-3, case.inner_d * 1e-3, (case.top_mm + case.bottom_mm + BOARD_MM).max(1.0) * 1e-3);
    let area = 2.0 * (w * d + w * h + d * h);
    let t = case.wall_mm.max(0.1) * 1e-3;
    let (k, rho_c, eps) = case.material.props();
    let outer = |x: f64| x + 2.0 * t;
    let area_out = 2.0 * (outer(w) * outer(d) + outer(w) * outer(h) + outer(d) * outer(h));
    let pressure = pressure_at(altitude_m);
    // the characteristic length of the box's faces: its height standing, area over perimeter lying flat
    let length = match orientation {
        Orientation::Standing => outer(d),
        Orientation::Flat => outer(w) * outer(d) / (2.0 * (outer(w) + outer(d))),
    };
    let share = convection_share.clamp(0.0, 1.0);
    let r_wall = t / (k * area);
    // h follows the temperature differences: a few rounds
    let (mut d_out, mut d_in) = (10.0, 5.0);
    let (mut h_out, mut h_in, mut r_out, mut r_air) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..30 {
        // outside: the faces stand and lie as the box does; the mean of a face up, a face down and four standing
        let (cu, ru) = h_air(d_out, ambient_c, length, Some(true), eps, pressure);
        let (cd, _) = h_air(d_out, ambient_c, length, Some(false), eps, pressure);
        let (cs, _) = h_air(d_out, ambient_c, outer(h).max(0.01), None, eps, pressure);
        h_out = (cu + cd + 4.0 * cs) / 6.0 + ru;
        r_out = 1.0 / (h_out * area_out);
        // inside: the air to the walls by convection only (radiation goes from the board straight to the walls)
        let (ci, _) = h_air(d_in, ambient_c + d_out, h.max(0.005), None, 0.0, pressure);
        h_in = ci;
        r_air = 1.0 / (h_in * area);
        let (o, i) = (power * r_out, share * power * r_air);
        if (o - d_out).abs() < 1e-4 && (i - d_in).abs() < 1e-4 {
            break;
        }
        (d_out, d_in) = (o.max(0.01), i.max(0.01));
    }
    let wall_c = ambient_c + power * (r_out + r_wall);
    let air_c = wall_c + share * power * r_air;
    // the board's surroundings: the air for its convection, the wall for its radiation
    let board_ambient_c = share * air_c + (1.0 - share) * wall_c;
    let cth = rho_c * area * t;
    Ok(CaseAir { power, wall_c, air_c, board_ambient_c, r_air, r_wall, r_out, h_in, h_out, area, cth, tau: cth * (r_out + r_wall) })
}

/// The board's map inside the case: the case worked out for all the heat (parts and wiring), and the map solved with what the board
/// sees inside as its surroundings and the room to the lid and floor as its gaps. The board's share of heat by convection is taken from
/// the open board's h (its convection over convection and radiation), and the case worked out again with it.
pub fn thermal_map_in_case(board: &crate::model::Board, sources: &[HeatSource], traces: &[TraceHeat], copper_um: f64, spec: &ThermalSpec, case: &CaseHeat) -> Result<(ThermalMap, CaseAir), String> {
    let power: f64 = sources.iter().map(|s| s.power).sum::<f64>() + traces.iter().map(|t| t.power.max(0.0)).sum::<f64>();
    let inside = |amb: f64| ThermalSpec { ambient_c: amb, gap_top_mm: case.top_mm, gap_bottom_mm: case.bottom_mm, ..*spec };
    let mut share = 0.5;
    let mut air = case_air(power, spec.ambient_c, case, spec.orientation, spec.altitude_m, share)?;
    let mut map = crate::thermal::thermal_map_with(board, sources, traces, copper_um, &inside(air.board_ambient_c))?;
    for _ in 0..3 {
        // the board's own split between convection and radiation, at its mean rise
        let mean_rise = (map.parts.iter().map(|p| p.pad_temp).sum::<f64>() / map.parts.len().max(1) as f64 - map.ambient_c).max(1.0);
        let (c, r) = h_air(mean_rise, map.ambient_c, 0.05, Some(true), spec.emissivity, pressure_at(spec.altitude_m));
        let s = c / (c + r);
        if (s - share).abs() < 0.01 {
            break;
        }
        share = s;
        air = case_air(power, spec.ambient_c, case, spec.orientation, spec.altitude_m, share)?;
        map = crate::thermal::thermal_map_with(board, sources, traces, copper_um, &inside(air.board_ambient_c))?;
    }
    Ok((map, air))
}

/// The run over time in the case: the board's own run (from the case's steady inside temperature, `tr`) shifted by the case's slow
/// rise, `(board_ambient − room)·(1 − e^(−t/τ))`, less the steady shift it was run with. Quasi-static: the case is far slower than the
/// board, so the board follows its surroundings.
pub fn transient_in_case(mut tr: ThermalTransient, air: &CaseAir, room_c: f64) -> ThermalTransient {
    let full = air.board_ambient_c - room_c;
    let at = |t: f64| -full * (-t / air.tau.max(1e-9)).exp();
    for (_, v) in tr.parts.iter_mut() {
        for (x, t) in v.iter_mut().zip(&tr.times) {
            *x += at(*t);
        }
    }
    for (t, layers) in tr.frames.iter_mut() {
        let shift = at(*t) as f32;
        for l in layers.iter_mut() {
            for x in l.iter_mut().filter(|x| x.is_finite()) {
                *x += shift;
            }
        }
    }
    tr.ambient_c = room_c;
    tr
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sealed ABS box of 100 × 60 × 30 mm (2 mm walls) with 1 W inside: the outside takes h ≈ 10 W/m²K (radiation 4σT³ε ≈ 5.5 with
    /// ε 0.9, natural convection ≈ 4 at a few kelvin), so the wall runs 4.5 K above the room and what the board sees about 6.7 K. The
    /// common rule of thumb for sealed boxes (5–6 W/m²K over the outer area, ≈ 7.5 K for the wall here) is more careful than this. The
    /// wall itself is a small part, so aluminium is only a little cooler; the walls hold about 60 J/K and take minutes to warm.
    #[test]
    fn a_sealed_box_adds_its_outside_and_its_air() {
        let c = CaseHeat { inner_w: 100.0, inner_d: 60.0, top_mm: 15.0, bottom_mm: 13.4, wall_mm: 2.0, material: CaseMaterial::Plastic };
        let a = case_air(1.0, 25.0, &c, Orientation::Flat, 0.0, 0.5).unwrap();
        let rise = a.board_ambient_c - 25.0;
        assert!((5.0..12.0).contains(&rise), "{a:?}");
        assert!(a.air_c > a.board_ambient_c && a.board_ambient_c > a.wall_c && a.wall_c > 25.0, "{a:?}");
        assert!(a.r_wall < 0.1 * (a.r_out + a.r_air), "{a:?}");
        let alu = case_air(1.0, 25.0, &CaseHeat { material: CaseMaterial::Aluminium, ..c }, Orientation::Flat, 0.0, 0.5).unwrap();
        assert!(alu.board_ambient_c < a.board_ambient_c && a.board_ambient_c - alu.board_ambient_c < 0.15 * rise, "{alu:?}");
        assert!((40.0..90.0).contains(&a.cth) && (120.0..900.0).contains(&a.tau), "{a:?}");
        // twice the heat: a little less than twice the rise (h grows with the difference)
        let b = case_air(2.0, 25.0, &c, Orientation::Flat, 0.0, 0.5).unwrap();
        assert!(b.board_ambient_c - 25.0 < 2.0 * rise && b.board_ambient_c - 25.0 > 1.6 * rise);
        assert!(case_air(1.0, 25.0, &CaseHeat::default(), Orientation::Flat, 0.0, 0.5).is_err());
    }

    /// The board in its case: a 0.5 W part on a 60 × 40 mm board in a 70 × 50 × 20 mm ABS box runs hotter than in the open by about what
    /// the case adds to its surroundings, and the case sees all the heat; the run over time ends where the map in the case is and starts
    /// from the room.
    #[test]
    fn the_board_in_its_case_runs_hotter_by_what_the_case_adds() {
        use crate::model::{Board, Pad, PadShape, Point, Side};
        let mut b = Board::new("t", 60.0, 40.0);
        for dx in [-1.6, 1.6] {
            let mut p = Pad::new(Point::new(30.0 + dx, 20.0), PadShape::Rect { w: 1.6, h: 1.2 }, None, Side::Top);
            p.refdes = "R1".into();
            b.pads.push(p);
        }
        let src = [HeatSource::new("R1", 0.5, 120.0, 0.0, 0.0)];
        let spec = ThermalSpec { cell_mm: 1.0, ..Default::default() };
        let case = CaseHeat { inner_w: 70.0, inner_d: 50.0, top_mm: 12.0, bottom_mm: 6.0, ..Default::default() };
        let open = crate::thermal::thermal_map(&b, &src, 35.0, &spec).unwrap();
        let (inside, air) = thermal_map_in_case(&b, &src, &[], 35.0, &spec, &case).unwrap();
        assert!((air.power - 0.5).abs() < 1e-12);
        let shift = air.board_ambient_c - 25.0;
        let (j_open, j_in) = (open.parts[0].junction, inside.parts[0].junction);
        assert!(shift > 1.0 && j_in > j_open + 0.7 * shift && j_in < j_open + 1.5 * shift, "open {j_open}, in the case {j_in}, the case adds {shift}");
        let tr = crate::thermal::thermal_transient(&b, &src, &[], 35.0, &ThermalSpec { ambient_c: air.board_ambient_c, gap_top_mm: 12.0, gap_bottom_mm: 6.0, ..spec }, 20.0 * air.tau, 4).unwrap();
        let tr = transient_in_case(tr, &air, 25.0);
        let (first, last) = (tr.parts[0].1[0], *tr.parts[0].1.last().unwrap());
        assert!((first - 25.0).abs() < 0.5, "starts from the room: {first}");
        assert!((last - j_in).abs() < 0.03 * (j_in - 25.0), "ends on the map in the case: {last} against {j_in}");
    }

}
