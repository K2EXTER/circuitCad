//! Line up the chosen parts, as the align buttons of a drawing program do. The edges and centres are those of each
//! part's pad box (the box round its pads), so two parts "left aligned" have their leftmost pads on one vertical line.
//! Locked parts stay where they are. Only part positions change (tracks are not moved).

use crate::autoplace::extent;
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Left edges on the leftmost one's.
    Left,
    /// Right edges on the rightmost one's.
    Right,
    /// Upper edges on the uppermost one's (the editor's y axis points up).
    Top,
    /// Lower edges on the lowest one's.
    Bottom,
    /// Centres on one vertical line (the middle of the whole group).
    CenterX,
    /// Centres on one horizontal line.
    CenterY,
    /// Equal gaps between neighbours from left to right; the outer two stay (needs 3 parts).
    SpaceX,
    /// Equal gaps from bottom to top; the outer two stay (needs 3 parts).
    SpaceY,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlignReport {
    pub moved: usize,
    /// Locked parts that were left out of the move.
    pub locked: usize,
}

type Box2 = (String, Point, Point);

/// Move the parts `refdes` as `how` says. Fewer than two parts (three for the equal gaps) change nothing and give
/// an error that says what to select.
pub fn align_parts(board: &mut Board, refdes: &[String], how: Align) -> Result<AlignReport, String> {
    let need = if matches!(how, Align::SpaceX | Align::SpaceY) { 3 } else { 2 };
    let mut boxes: Vec<Box2> = refdes.iter().filter_map(|r| extent(board, r).map(|(lo, hi)| (r.clone(), lo, hi))).collect();
    boxes.sort_by(|a, b| a.0.cmp(&b.0));
    boxes.dedup_by(|a, b| a.0 == b.0);
    if boxes.len() < need {
        return Err(if need == 3 {
            "等間隔にそろえるには、部品を 3 個以上選んでください".into()
        } else {
            "整列するには、部品を 2 個以上選んでください (Shift を押しながらクリック、または範囲選択)".into()
        });
    }
    let min = |f: fn(&Box2) -> f64| boxes.iter().map(f).fold(f64::MAX, f64::min);
    let max = |f: fn(&Box2) -> f64| boxes.iter().map(f).fold(f64::MIN, f64::max);
    let (lo_x, hi_x) = (min(|b| b.1.x), max(|b| b.2.x));
    let (lo_y, hi_y) = (min(|b| b.1.y), max(|b| b.2.y));
    // how far each part should move: (dx, dy)
    let mut moves: Vec<(String, f64, f64)> = vec![];
    match how {
        Align::Left => moves.extend(boxes.iter().map(|b| (b.0.clone(), lo_x - b.1.x, 0.0))),
        Align::Right => moves.extend(boxes.iter().map(|b| (b.0.clone(), hi_x - b.2.x, 0.0))),
        Align::Top => moves.extend(boxes.iter().map(|b| (b.0.clone(), 0.0, hi_y - b.2.y))),
        Align::Bottom => moves.extend(boxes.iter().map(|b| (b.0.clone(), 0.0, lo_y - b.1.y))),
        Align::CenterX => moves.extend(boxes.iter().map(|b| (b.0.clone(), (lo_x + hi_x) / 2.0 - (b.1.x + b.2.x) / 2.0, 0.0))),
        Align::CenterY => moves.extend(boxes.iter().map(|b| (b.0.clone(), 0.0, (lo_y + hi_y) / 2.0 - (b.1.y + b.2.y) / 2.0))),
        Align::SpaceX | Align::SpaceY => {
            let x = how == Align::SpaceX;
            let lo = |b: &Box2| if x { b.1.x } else { b.1.y };
            let hi = |b: &Box2| if x { b.2.x } else { b.2.y };
            boxes.sort_by(|a, b| (lo(a) + hi(a)).total_cmp(&(lo(b) + hi(b))).then(a.0.cmp(&b.0)));
            let span = hi(&boxes[boxes.len() - 1]) - lo(&boxes[0]);
            let size: f64 = boxes.iter().map(|b| hi(b) - lo(b)).sum();
            let gap = (span - size) / (boxes.len() as f64 - 1.0);
            let mut at = lo(&boxes[0]);
            for b in &boxes {
                let d = at - lo(b);
                moves.push((b.0.clone(), if x { d } else { 0.0 }, if x { 0.0 } else { d }));
                at += hi(b) - lo(b) + gap;
            }
        }
    }
    let mut report = AlignReport::default();
    for (r, dx, dy) in moves {
        if board.is_locked(&r) {
            report.locked += 1;
            continue;
        }
        let (dx, dy) = ((dx * 1000.0).round() / 1000.0, (dy * 1000.0).round() / 1000.0);
        if dx == 0.0 && dy == 0.0 {
            continue;
        }
        if let Some(p) = board.parts.iter().find(|p| p.refdes == r).cloned() {
            board.place_part(&r, Point::new(p.pos.x + dx, p.pos.y + dy), p.rot);
            report.moved += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;

    fn board() -> Board {
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
        let mut b = Board::new("t", 60.0, 40.0);
        let net = NetlistNet { name: "A".into(), pins: vec![("R1".into(), "1".into()), ("R2".into(), "1".into())] };
        import(&mut b, &[part("R1", "R_0805"), part("R2", "R_0805"), part("C1", "C_0805"), part("D1", "LED_0805")], &[net]);
        for (r, x, y) in [("R1", 10.0, 10.0), ("R2", 20.0, 14.0), ("C1", 31.0, 12.0), ("D1", 50.0, 18.0)] {
            b.place_part(r, Point::new(x, y), 0);
        }
        b
    }

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    fn bx(b: &Board, r: &str) -> (Point, Point) {
        extent(b, r).unwrap()
    }

    #[test]
    fn edges_and_centres_meet() {
        let all = names(&["R1", "R2", "C1", "D1"]);
        let mut b = board();
        let leftmost = bx(&b, "R1").0.x;
        align_parts(&mut b, &all, Align::Left).unwrap();
        let xs: Vec<f64> = all.iter().map(|r| bx(&b, r).0.x).collect();
        assert!(xs.iter().all(|x| (x - xs[0]).abs() < 1e-9), "{xs:?}");
        assert!((xs[0] - leftmost).abs() < 1e-9, "the leftmost one stays: {xs:?}");
        let mut b = board();
        align_parts(&mut b, &all, Align::Top).unwrap();
        let tops: Vec<f64> = all.iter().map(|r| bx(&b, r).1.y).collect();
        assert!(tops.iter().all(|y| (y - tops[0]).abs() < 1e-9), "{tops:?}");
        let mut b = board();
        align_parts(&mut b, &all, Align::CenterY).unwrap();
        let cs: Vec<f64> = all.iter().map(|r| (bx(&b, r).0.y + bx(&b, r).1.y) / 2.0).collect();
        assert!(cs.iter().all(|y| (y - cs[0]).abs() < 1e-9), "{cs:?}");
    }

    #[test]
    fn equal_gaps_keep_the_outer_parts() {
        let all = names(&["R1", "R2", "C1", "D1"]);
        let mut b = board();
        let (first, last) = (bx(&b, "R1"), bx(&b, "D1"));
        align_parts(&mut b, &all, Align::SpaceX).unwrap();
        assert_eq!(bx(&b, "R1").0, first.0);
        assert_eq!(bx(&b, "D1").0, last.0);
        let mut order = all.clone();
        order.sort_by(|p, q| bx(&b, p).0.x.total_cmp(&bx(&b, q).0.x));
        let gaps: Vec<f64> = order.windows(2).map(|w| bx(&b, &w[1]).0.x - bx(&b, &w[0]).1.x).collect();
        assert!(gaps.iter().all(|g| (g - gaps[0]).abs() < 2e-3), "{gaps:?}");
    }

    #[test]
    fn locked_parts_stay_and_too_few_parts_are_refused() {
        let mut b = board();
        b.locked_parts.push("R2".into());
        let r = align_parts(&mut b, &names(&["R1", "R2", "C1"]), Align::Bottom).unwrap();
        assert_eq!(r.locked, 1);
        assert_eq!(bx(&b, "R2").0.y, bx(&board(), "R2").0.y);
        assert!(align_parts(&mut b, &names(&["R1"]), Align::Left).is_err());
        assert!(align_parts(&mut b, &names(&["R1", "C1"]), Align::SpaceX).is_err());
        assert!(align_parts(&mut b, &names(&["R1", "R1"]), Align::Left).is_err(), "the same part twice is one part");
    }
}
