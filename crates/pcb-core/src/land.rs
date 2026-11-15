//! Land patterns from a package's dimensions (the dimension table of its data sheet), in the way of IPC-7351 at its nominal density:
//! the pad reaches past the lead's toe and heel by fillet goals, widened by the tolerances of the part, of the board and of placing
//! it (added as a root of a sum of squares, as the standard does).

/// Fillet goals (mm) for gull-wing leads at the nominal density: toe, heel, side.
const TOE: f64 = 0.35;
const HEEL: f64 = 0.35;
const SIDE: f64 = 0.03;
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
    let ok = |r: (f64, f64)| r.0 > 0.0 && r.1 >= r.0;
    if !ok(span) || !ok(foot) || !ok(lead_w) {
        return None;
    }
    let (cl, ct, cw) = (span.1 - span.0, foot.1 - foot.0, lead_w.1 - lead_w.0);
    let tol = |c: f64| (c * c + FAB * FAB + PLACE * PLACE).sqrt();
    // outside: the shortest span plus the toe fillets
    let z = span.0 + 2.0 * TOE + tol(cl);
    // inside: between the heels. Its tolerance from those of the span and of the feet, as a root of squares
    let s_min = span.0 - 2.0 * foot.1;
    let cs = (cl * cl + 2.0 * ct * ct).sqrt();
    let s_max = (span.1 - 2.0 * foot.0).min(s_min + cs);
    let g = s_max - 2.0 * HEEL - tol(cs);
    let x = lead_w.0 + 2.0 * SIDE + tol(cw);
    (g > 0.0 && z > g).then_some(GullWing { row: (z + g) / 2.0, pad_l: (z - g) / 2.0, pad_w: x })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn sizes_that_cannot_be_lands_give_none() {
        assert!(gull_wing((0.0, 6.0), (0.4, 1.27), (0.31, 0.51)).is_none());
        assert!(gull_wing((2.0, 2.2), (0.9, 1.2), (0.3, 0.5)).is_none(), "feet longer than half the span");
    }
}
