//! Land patterns from a package's dimensions (the dimension table of its data sheet), in the way of IPC-7351 at its nominal density:
//! the pad reaches past the lead's toe and heel by fillet goals, widened by the tolerances of the part, of the board and of placing
//! it (added as a root of a sum of squares, as the standard does).

/// Fillet goals (mm) for gull-wing leads at the nominal density: toe, heel, side.
const TOE: f64 = 0.35;
const HEEL: f64 = 0.35;
const SIDE: f64 = 0.03;
/// For flat terminals under the body's edge (QFN, SON, DFN), IPC-7351's nominal goals: the toe reaches out, the heel and the
/// sides take none (the side a little in).
const NL_TOE: f64 = 0.40;
const NL_HEEL: f64 = 0.0;
const NL_SIDE: f64 = -0.04;
/// Board (fabrication) and placement tolerances (mm).
const FAB: f64 = 0.10;
const PLACE: f64 = 0.05;

/// The lands of a row of gull-wing leads (SOIC, SSOP, TSSOP, SOT-23 …): from the lead span tip to tip (`span`, the data sheet's E or
/// HE), the length of a lead's foot (`foot`, L) and its width (`lead_w`, b), each as (min, max) in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GullWing {
    /// From the centre of the pads of one row to the centre of the pads of the other.
    pub row: f64,
    /// Pad length (across the rows) and width (along the row).
    pub pad_l: f64,
    pub pad_w: f64,
}

/// See `GullWing`. `None` when a size is not positive or the pads would overlap across the body.
pub fn gull_wing(span: (f64, f64), foot: (f64, f64), lead_w: (f64, f64)) -> Option<GullWing> {
    lands(span, foot, lead_w, (TOE, HEEL, SIDE))
}

/// The lands of a row of flat terminals of a no-lead package (QFN, SON, DFN): `span` is the body's size across the rows (D or E:
/// the terminals end at its edge), `foot` the terminal's length (L) and `lead_w` its width (b), each (min, max) in mm. The same
/// sums as `gull_wing` with the no-lead fillet goals. The exposed pad is not here: its land is the data sheet's D2 × E2.
pub fn no_lead(span: (f64, f64), foot: (f64, f64), lead_w: (f64, f64)) -> Option<GullWing> {
    lands(span, foot, lead_w, (NL_TOE, NL_HEEL, NL_SIDE))
}

/// The lands of a two-terminal chip (resistor, capacitor, inductor: 0402, 0603 …, 候補 Q3) from the body's length `len` (L), its
/// terminal's length on the underside `term` (the data sheet's a, b, e, I2 …) and its width `width` (W), each (min, max) in mm.
/// IPC-7351's nominal goals: a toe of 0.35 for chips of 1.6 mm (0603) and longer, 0.15 for smaller ones, no heel or side.
/// `row` is the distance between the two pads' centres, `pad_l` a pad's length along it, `pad_w` across.
pub fn chip(len: (f64, f64), term: (f64, f64), width: (f64, f64)) -> Option<GullWing> {
    let toe = if len.1 >= 1.5 { 0.35 } else { 0.15 };
    lands(len, term, width, (toe, 0.0, 0.0))
}

fn lands(span: (f64, f64), foot: (f64, f64), lead_w: (f64, f64), (toe, heel, side): (f64, f64, f64)) -> Option<GullWing> {
    let ok = |r: (f64, f64)| r.0 > 0.0 && r.1 >= r.0;
    if !ok(span) || !ok(foot) || !ok(lead_w) {
        return None;
    }
    let (cl, ct, cw) = (span.1 - span.0, foot.1 - foot.0, lead_w.1 - lead_w.0);
    let tol = |c: f64| (c * c + FAB * FAB + PLACE * PLACE).sqrt();
    // outside: the shortest span plus the toe fillets
    let z = span.0 + 2.0 * toe + tol(cl);
    // inside: between the heels. Its tolerance from those of the span and of the feet, as a root of squares
    let s_min = span.0 - 2.0 * foot.1;
    let cs = (cl * cl + 2.0 * ct * ct).sqrt();
    let s_max = (span.1 - 2.0 * foot.0).min(s_min + cs);
    let g = s_max - 2.0 * heel - tol(cs);
    let x = lead_w.0 + 2.0 * side + tol(cw);
    (g > 0.0 && z > g).then_some(GullWing { row: (z + g) / 2.0, pad_l: (z - g) / 2.0, pad_w: x })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yageo's RT0603 (L 1.60 ±0.10, W 0.80 ±0.10, the bottom terminal I2 0.25 ±0.15): near the usual library land (pads about
    /// 0.8–0.9 × 0.95 mm, 1.55–1.65 mm apart). Its RT0402 (L 1.00 ±0.10, I2 0.25 ±0.10): the terminal's wide tolerance draws the
    /// pads in, 0.8 mm apart (the library's are 1.0 mm apart).
    #[test]
    fn chip_lands() {
        let g = chip((1.5, 1.7), (0.1, 0.4), (0.7, 0.9)).unwrap();
        assert!((g.row - 1.56).abs() < 0.03 && (g.pad_l - 0.87).abs() < 0.03 && (g.pad_w - 0.93).abs() < 0.03, "{g:?}");
        let g = chip((0.9, 1.1), (0.15, 0.35), (0.45, 0.55)).unwrap();
        assert!((g.row - 0.81).abs() < 0.02 && (g.pad_l - 0.62).abs() < 0.02 && (g.pad_w - 0.6).abs() < 0.02, "{g:?}");
        assert!(chip((1.6, 1.6), (0.9, 0.9), (0.8, 0.8)).is_none(), "terminals that meet leave no gap");
    }

    /// SOIC-8 (JEDEC MS-012: E 5.80–6.20, L 0.40–1.27, b 0.31–0.51): near the usual library land (pads about 0.6 × 1.5–2.2 mm, rows
    /// 4.7–5.4 mm apart).
    #[test]
    fn soic8_lands_are_like_the_usual_ones() {
        let g = gull_wing((5.80, 6.20), (0.40, 1.27), (0.31, 0.51)).unwrap();
        assert!((g.pad_w - 0.6).abs() < 0.05, "{g:?}");
        assert!((4.6..=5.5).contains(&g.row) && (1.4..=2.3).contains(&g.pad_l), "{g:?}");
        // the pads reach past the leads' tips
        assert!(g.row / 2.0 + g.pad_l / 2.0 > 6.2 / 2.0);
    }

    /// A 3 × 3 mm QFN-16 at 0.5 mm (JEDEC MO-220 WEED: E 2.90–3.10, L 0.30–0.50, b 0.18–0.30): IPC's calculator gives pads about
    /// 1.0 × 0.26 mm (outer ends 3.9 mm apart); TI's own land is shorter (0.6 × 0.24, 2.8 between opposite pads' centres).
    #[test]
    fn qfn16_lands_are_like_the_makers() {
        let g = no_lead((2.90, 3.10), (0.30, 0.50), (0.18, 0.30)).unwrap();
        assert!((0.9..=1.1).contains(&g.pad_l) && (0.2..=0.32).contains(&g.pad_w) && (2.75..=3.0).contains(&g.row), "{g:?}");
        assert!((g.row + g.pad_l - 3.93).abs() < 0.05, "{g:?}");
        // the toe reaches out past the body; the heel stays inside
        assert!(g.row / 2.0 + g.pad_l / 2.0 > 3.1 / 2.0);
        assert!(g.row / 2.0 - g.pad_l / 2.0 < 2.9 / 2.0 - 0.3);
    }

    #[test]
    fn sizes_that_cannot_be_lands_give_none() {
        assert!(gull_wing((0.0, 6.0), (0.4, 1.27), (0.31, 0.51)).is_none());
        assert!(gull_wing((2.0, 2.2), (0.9, 1.2), (0.3, 0.5)).is_none(), "feet longer than half the span");
    }
}
