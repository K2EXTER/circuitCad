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
    /// Side by side from the leftmost one's place, in the order they stand, `ROW_GAP` between, centres on the first one's
    /// horizontal line.
    RowX,
    /// One under another from the uppermost one's place, in the order they stand, `ROW_GAP` between, centres on the first one's
    /// vertical line.
    RowY,
    /// In one row (or column) in the order of the pads they are wired to on the other parts, its middle across from theirs: series
    /// resistors in the order of a connector's pins, LED resistors in the order of a driver's outputs. A row when those pads lie
    /// along a row, else a column; on the line of the chosen parts' middle, within the board's outline.
    NetOrder,
}

/// The gap between parts lined up by `RowX`, `RowY` and `NetOrder` (mm).
pub const ROW_GAP: f64 = 1.0;

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
        Align::RowX | Align::RowY => {
            let x = how == Align::RowX;
            let order = sorted(&boxes, &|b: &Box2| if x { (b.1.x, -b.2.y) } else { (-b.2.y, b.1.x) });
            line(&boxes, &order, x, None, &mut moves);
        }
        Align::NetOrder => {
            let key = wired_to(board, &boxes);
            let spread = |f: &dyn Fn(&Point) -> f64| key.iter().map(f).fold(f64::MIN, f64::max) - key.iter().map(f).fold(f64::MAX, f64::min);
            let x = spread(&|k| k.x) >= spread(&|k| k.y);
            // (a row runs left to right; a column top to bottom, the y axis pointing up)
            let order = sorted(&boxes, &|b: &Box2| {
                let k = key[boxes.iter().position(|c| c.0 == b.0).unwrap_or(0)];
                if x { (k.x, -k.y) } else { (-k.y, k.x) }
            });
            let size = |b: &Box2| if x { b.2.x - b.1.x } else { b.2.y - b.1.y };
            let long: f64 = boxes.iter().map(size).sum::<f64>() + ROW_GAP * (boxes.len() as f64 - 1.0);
            let mid = key.iter().map(|k| if x { k.x } else { k.y }).sum::<f64>() / key.len() as f64;
            // the line's start: its middle across from the wired pads' middle, kept within the outline where it fits
            let mut start = if x { mid - long / 2.0 } else { mid + long / 2.0 };
            if let Some((lo, hi)) = outline_box(board) {
                start = if x { start.min(hi.x - long).max(lo.x) } else { start.max(lo.y + long).min(hi.y) };
            }
            // across the line: the middle of the chosen parts
            let across = boxes.iter().map(|b| if x { (b.1.y + b.2.y) / 2.0 } else { (b.1.x + b.2.x) / 2.0 }).sum::<f64>() / boxes.len() as f64;
            line(&boxes, &order, x, Some((start, across)), &mut moves);
        }
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

/// The indices of `boxes` sorted by `key`.
fn sorted(boxes: &[Box2], key: &dyn Fn(&Box2) -> (f64, f64)) -> Vec<usize> {
    let mut v: Vec<usize> = (0..boxes.len()).collect();
    v.sort_by(|a, b| key(&boxes[*a]).partial_cmp(&key(&boxes[*b])).unwrap_or(std::cmp::Ordering::Equal).then(boxes[*a].0.cmp(&boxes[*b].0)));
    v
}

/// The parts of `boxes` one after another in `order`, `ROW_GAP` between: along x (left to right) or down y. From `at` (where the
/// line starts along it, and the line across it the centres go on), else from the first one's place and centre.
fn line(boxes: &[Box2], order: &[usize], x: bool, at: Option<(f64, f64)>, moves: &mut Vec<(String, f64, f64)>) {
    let Some(&first) = order.first() else { return };
    let f = &boxes[first];
    let (mut along, across) = at.unwrap_or(if x { (f.1.x, (f.1.y + f.2.y) / 2.0) } else { (f.2.y, (f.1.x + f.2.x) / 2.0) });
    for &i in order {
        let b = &boxes[i];
        let (cx, cy) = ((b.1.x + b.2.x) / 2.0, (b.1.y + b.2.y) / 2.0);
        if x {
            moves.push((b.0.clone(), along - b.1.x, across - cy));
            along += b.2.x - b.1.x + ROW_GAP;
        } else {
            moves.push((b.0.clone(), across - cx, along - b.2.y));
            along -= b.2.y - b.1.y + ROW_GAP;
        }
    }
}

/// For each part of `boxes`: the middle of the pads it is wired to on the parts not chosen, by nets of few pads only (a supply or
/// ground runs everywhere and tells nothing of the order), on the chips and connectors (3 pads and more) where there are any (a
/// resistor's other end goes to its LED), else on any part; its own middle when there are none.
fn wired_to(board: &Board, boxes: &[Box2]) -> Vec<Point> {
    let chosen = |r: &str| boxes.iter().any(|b| b.0 == r);
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut pads_of: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for p in board.pads.iter().filter(|p| !p.net.is_empty()) {
        *count.entry(p.net.as_str()).or_default() += 1;
    }
    for p in &board.pads {
        *pads_of.entry(p.refdes.as_str()).or_default() += 1;
    }
    let ground = |n: &str| matches!(n.to_ascii_uppercase().as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND" | "PGND");
    boxes
        .iter()
        .map(|b| {
            let mine: Vec<&str> = board.pads.iter().filter(|p| p.refdes == b.0 && !p.net.is_empty() && !ground(&p.net) && count[p.net.as_str()] <= 4).map(|p| p.net.as_str()).collect();
            let on = |big: bool| -> Vec<Point> {
                board.pads.iter().filter(|p| !chosen(&p.refdes) && !p.refdes.is_empty() && mine.contains(&p.net.as_str()) && (!big || pads_of[p.refdes.as_str()] > 2)).map(|p| p.pos).collect()
            };
            let mut to = on(true);
            if to.is_empty() {
                to = on(false);
            }
            if to.is_empty() {
                Point::new((b.1.x + b.2.x) / 2.0, (b.1.y + b.2.y) / 2.0)
            } else {
                Point::new(to.iter().map(|p| p.x).sum::<f64>() / to.len() as f64, to.iter().map(|p| p.y).sum::<f64>() / to.len() as f64)
            }
        })
        .collect()
}

/// The box round the board's outline (None without one).
fn outline_box(board: &Board) -> Option<(Point, Point)> {
    if board.outline.len() < 3 {
        return None;
    }
    let lo = Point::new(board.outline.iter().map(|p| p.x).fold(f64::MAX, f64::min), board.outline.iter().map(|p| p.y).fold(f64::MAX, f64::min));
    let hi = Point::new(board.outline.iter().map(|p| p.x).fold(f64::MIN, f64::max), board.outline.iter().map(|p| p.y).fold(f64::MIN, f64::max));
    Some((lo, hi))
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

    /// Four resistors wired one to each pin of a header, standing in no order: "つながる順" puts them in a row (or column) in the
    /// order of the header's pins, `ROW_GAP` apart, within the outline; "横に詰めて" keeps the order they stand in.
    #[test]
    fn parts_line_up_in_the_order_of_the_pins_they_are_wired_to() {
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
        let mut b = Board::new("t", 80.0, 60.0);
        let rs = ["R1", "R2", "R3", "R4"];
        let nets: Vec<NetlistNet> = (0..4).map(|i| NetlistNet { name: format!("N{i}"), pins: vec![("J1".into(), format!("{}", i + 1)), (rs[i].into(), "1".into())] }).collect();
        let mut parts = vec![part("J1", "PinHeader_1x4")];
        parts.extend(rs.iter().map(|r| part(r, "R_0805")));
        import(&mut b, &parts, &nets);
        b.place_part("J1", Point::new(40.0, 50.0), 0);
        for (r, x, y) in [("R1", 60.0, 20.0), ("R2", 15.0, 25.0), ("R3", 50.0, 12.0), ("R4", 30.0, 18.0)] {
            b.place_part(r, Point::new(x, y), 0);
        }
        let pin = |b: &Board, r: &str| b.pads.iter().find(|p| p.refdes == "J1" && p.pin == r).unwrap().pos;
        let along_x = (pin(&b, "1").x - pin(&b, "4").x).abs() >= (pin(&b, "1").y - pin(&b, "4").y).abs();
        let all = names(&rs);
        align_parts(&mut b, &all, Align::NetOrder).unwrap();
        let mid = |r: &str| {
            let (lo, hi) = bx(&b, r);
            Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
        };
        let key = |p: Point| if along_x { p.x } else { -p.y };
        let mut by_place = all.clone();
        by_place.sort_by(|p, q| key(mid(p)).total_cmp(&key(mid(q))));
        let mut by_pin = all.clone();
        by_pin.sort_by(|p, q| key(pin(&b, &p[1..])).total_cmp(&key(pin(&b, &q[1..]))));
        assert_eq!(by_place, by_pin, "in the order of the header's pins");
        let across: Vec<f64> = all.iter().map(|r| if along_x { mid(r).y } else { mid(r).x }).collect();
        assert!(across.iter().all(|c| (c - across[0]).abs() < 1e-6), "on one line: {across:?}");
        let gaps: Vec<f64> = by_place.windows(2).map(|w| if along_x { bx(&b, &w[1]).0.x - bx(&b, &w[0]).1.x } else { bx(&b, &w[0]).0.y - bx(&b, &w[1]).1.y }).collect();
        assert!(gaps.iter().all(|g| (g - ROW_GAP).abs() < 2e-3), "{gaps:?}");
        // side by side in the order they stand (left to right), centres on the first one's line
        let mut b = board();
        let all = names(&["R1", "R2", "C1", "D1"]);
        align_parts(&mut b, &all, Align::RowX).unwrap();
        let order: Vec<f64> = all.iter().map(|r| bx(&b, r).0.x).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        let gaps: Vec<f64> = all.windows(2).map(|w| bx(&b, &w[1]).0.x - bx(&b, &w[0]).1.x).collect();
        assert!(gaps.iter().all(|g| (g - ROW_GAP).abs() < 2e-3), "{gaps:?}");
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
