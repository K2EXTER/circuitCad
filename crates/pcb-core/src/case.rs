//! A plain box for the board to go in (the inside size of the case) and what does not fit: the board's footprint on the floor of
//! the box (turned a quarter if that helps), and the parts taller than the room above and below the board. No STEP is needed:
//! these are the numbers that the case's drawing gives (`dxf_in` takes the outline from the drawing itself).

use crate::model::*;
use crate::model3d::part_height_on;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseSpec {
    /// Inside width and depth of the box in mm (0: the floor is not checked).
    pub inner_w: f64,
    pub inner_d: f64,
    /// Room from the board's surface to the lid (top) and to the floor (bottom) in mm (0: not checked).
    pub top: f64,
    pub bottom: f64,
    /// Gap to keep between the board's edge and the wall in mm.
    pub clearance: f64,
}

impl Default for CaseSpec {
    fn default() -> Self {
        CaseSpec { inner_w: 0.0, inner_d: 0.0, top: 0.0, bottom: 0.0, clearance: 0.5 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseReport {
    /// The board's size (the box round its outline and parts).
    pub board_w: f64,
    pub board_d: f64,
    /// Is the floor checked, does the board go in, and is it turned a quarter to go in?
    pub floor_checked: bool,
    pub fits: bool,
    pub turned: bool,
    /// Room left on each side (width, depth) in the turn that was chosen (negative: how much too big).
    pub spare_w: f64,
    pub spare_d: f64,
    /// Parts above the room: (reference, side, height, how much over).
    pub too_tall: Vec<(String, Side, f64, f64)>,
}

impl CaseReport {
    pub fn ok(&self) -> bool {
        (!self.floor_checked || self.fits) && self.too_tall.is_empty()
    }
}

pub fn check(board: &Board, spec: &CaseSpec) -> CaseReport {
    let (lo, hi) = board.bounds().unwrap_or_default();
    let (bw, bd) = (hi.x - lo.x, hi.y - lo.y);
    let floor_checked = spec.inner_w > 0.0 && spec.inner_d > 0.0;
    let (aw, ad) = (spec.inner_w - 2.0 * spec.clearance, spec.inner_d - 2.0 * spec.clearance);
    let straight = (aw - bw, ad - bd);
    let turned_sp = (aw - bd, ad - bw);
    let min = |s: (f64, f64)| s.0.min(s.1);
    let turned = floor_checked && min(straight) < 0.0 && min(turned_sp) >= 0.0;
    let (spare_w, spare_d) = if turned || (floor_checked && min(turned_sp) > min(straight)) { turned_sp } else { straight };
    let fits = floor_checked && spare_w >= 0.0 && spare_d >= 0.0;
    let mut too_tall = vec![];
    for p in &board.parts {
        let room = if p.side == Side::Bottom { spec.bottom } else { spec.top };
        if room <= 0.0 {
            continue;
        }
        let h = part_height_on(board, p);
        if h > room + 1e-9 {
            too_tall.push((p.refdes.clone(), p.side, h, h - room));
        }
    }
    too_tall.sort_by(|a, b| b.3.total_cmp(&a.3));
    CaseReport { board_w: bw, board_d: bd, floor_checked, fits, turned, spare_w, spare_d, too_tall }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_goes_in_a_box_or_does_not_and_a_quarter_turn_can_save_it() {
        let b = Board::new("x", 60.0, 40.0);
        let spec = |w, d| CaseSpec { inner_w: w, inner_d: d, ..Default::default() };
        let r = check(&b, &spec(70.0, 50.0));
        assert!(r.fits && !r.turned && (r.spare_w - 9.0).abs() < 1e-9, "{r:?}");
        let r = check(&b, &spec(45.0, 65.0));
        assert!(r.fits && r.turned, "turned a quarter it goes in: {r:?}");
        let r = check(&b, &spec(55.0, 45.0));
        assert!(!r.fits && r.spare_w < 0.0, "{r:?}");
        assert!(!r.ok());
        // nothing given: nothing is checked
        assert!(check(&b, &CaseSpec::default()).ok());
    }

    #[test]
    fn a_part_taller_than_the_room_is_named_with_the_excess() {
        let mut b = Board::new("x", 30.0, 20.0);
        // a pin header: 2.5 mm body + 6 mm of pins sticks out
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: "PinHeader_1x4".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        let r = check(&b, &CaseSpec { top: 5.0, ..Default::default() });
        assert_eq!(r.too_tall.len(), 1, "{r:?}");
        assert!(r.too_tall[0].3 > 0.0);
        let r = check(&b, &CaseSpec { top: 50.0, ..Default::default() });
        assert!(r.too_tall.is_empty());
        let r = check(&b, &CaseSpec { bottom: 1.0, ..Default::default() });
        assert!(r.too_tall.is_empty(), "the part is on the top: the bottom room does not matter");
    }
}
