use super::*;

/// A surface-mount pad this big (mm²) on a part with several pins is a heat pad (the exposed pad of a QFN or
/// ESOP, the tab of a SOT-223): the part gets rid of its heat through it.
pub const HEAT_PAD_AREA: f64 = 4.0;

/// The heat pads of the board: (index into `board.pads`, area in mm²). A pad counts when it is surface-mount,
/// at least `HEAT_PAD_AREA`, on a part with three or more pads, and more than twice as big as any other pad of it.
pub fn heat_pads(board: &Board) -> Vec<(usize, f64)> {
    let area = |p: &Pad| {
        let (w, h) = p.shape.size();
        match p.shape {
            PadShape::Circle { d } => std::f64::consts::PI * d * d / 4.0,
            _ => w * h,
        }
    };
    let mut out = vec![];
    for (i, p) in board.pads.iter().enumerate() {
        if p.drill.is_some() || p.refdes.is_empty() || area(p) < HEAT_PAD_AREA {
            continue;
        }
        let others: Vec<f64> = board.pads.iter().enumerate().filter(|(j, q)| *j != i && q.refdes == p.refdes).map(|(_, q)| area(q)).collect();
        if others.len() >= 2 && others.iter().all(|a| area(p) > 2.0 * a) {
            out.push((i, area(p)));
        }
    }
    out
}

/// A pour that touches a heat pad spreads its heat when the copper piece it sits in is at least this big (mm²,
/// about 10 × 10 mm); then half the thermal vias are enough. A small piece (cut up by other nets' copper, or
/// a pour that is only an island around the pad) spreads little, so the full number stays.
pub const POUR_SPREAD_AREA: f64 = 100.0;

/// How many thermal vias a heat pad of `area` mm² should have, given the copper (mm²) of the same-net pour
/// piece it sits in (0 = no pour): about one per 2.5 mm², at least two; half that with a big pour (at least one).
pub fn thermal_via_need(area: f64, pour_area: f64) -> usize {
    let base = ((area / 2.5).ceil() as usize).clamp(2, 9);
    if pour_area >= POUR_SPREAD_AREA { base.div_ceil(2).max(1) } else { base }
}

/// The actual copper of every pour (what `zonefill` draws), to see how much copper a pad sits in.
pub(super) struct PourFills {
    fills: Vec<(Side, String, Vec<crate::zonefill::FillShape>)>,
}

impl PourFills {
    fn new(board: &Board) -> Self {
        if board.zones.is_empty() {
            return Self { fills: vec![] };
        }
        let conn = connectivity(board);
        Self { fills: board.zones.iter().map(|z| (z.side, z.net.clone(), crate::zonefill::zone_fill(board, z, &conn))).collect() }
    }

    /// Copper (mm²) of the piece of a same-net pour on the pad's side that the pad sits in; 0 when no pour touches it.
    fn area_at(&self, pad: &Pad) -> f64 {
        use crate::geom::{inside_polygon, signed_area};
        if pad.net.is_empty() {
            return 0.0;
        }
        let mut best = 0.0f64;
        for (side, net, shapes) in &self.fills {
            if *side != pad.side || *net != pad.net {
                continue;
            }
            for sh in shapes {
                if inside_polygon(&sh.outer, pad.pos) && !sh.holes.iter().any(|h| inside_polygon(h, pad.pos)) {
                    best = best.max(signed_area(&sh.outer).abs() - sh.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>());
                }
            }
        }
        best
    }
}

/// Thermal vias: a heat pad needs vias in it that carry the heat through the board to copper on the back (about one
/// per 2.5 mm², at least two; half that when a big pour of the pad's net touches it). Without them the heat has
/// only the small top-side copper to spread in.
pub fn thermal_via_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    let heat = heat_pads(board);
    if heat.is_empty() {
        return out;
    }
    let pours = PourFills::new(board);
    for (i, area) in heat {
        let p = &board.pads[i];
        let (w, h) = p.shape.size();
        let inside = |c: Point| match p.shape {
            PadShape::Circle { d } => c.dist(p.pos) <= d / 2.0,
            _ => (c.x - p.pos.x).abs() <= w / 2.0 && (c.y - p.pos.y).abs() <= h / 2.0,
        };
        let have = board.vias.iter().filter(|v| inside(v.pos)).count();
        let pour = pours.area_at(p);
        let need = thermal_via_need(area, pour);
        if have < need {
            let pour_note = if pour >= POUR_SPREAD_AREA {
                format!("接しているベタ: 約 {pour:.0}mm² (広いので、目安をベタなしの半分にしています)")
            } else if pour > 0.0 {
                format!("接しているベタ: 約 {pour:.0}mm² ({POUR_SPREAD_AREA:.0}mm² に足りず、熱があまり広がらないので、目安はそのままです。ベタが他のネットの銅で細かく切られていないか確認してください)")
            } else {
                format!("接しているベタ: なし (同じネットのベタ {POUR_SPREAD_AREA:.0}mm² 以上が接していれば、目安は半分になります)")
            };
            out.push(v(
                format!(
                    "{}.{} は放熱パッドです (面積 {area:.1}mm²)。
推奨: サーマルビア {need} 個以上 (いま {have} 個)
{pour_note}
意味: サーマルビアは、部品の熱を裏面の銅へ逃がす穴です。パッドの中に格子状に置き (穴 0.3mm くらい)、裏面の同じネットの銅 (ゾーン) につなぐと、熱が基板全体に広がります。基板エディタの「サーマルビアを置く」で自動で置けます",
                    p.refdes, p.pin
                ),
                p.pos,
            ));
        }
    }
    out
}

/// Put the missing thermal vias into the heat pads: a grid of 0.3 mm vias inside each pad that has fewer than the
/// check asks for. The vias join the pad's copper (and so its net); joining the copper on the back is up to the
/// user's pour. Returns how many vias were added; nothing is added when that would make the DRC worse.
pub fn add_thermal_vias(board: &mut Board) -> usize {
    let drill = board.rules.min_drill.max(0.3);
    let dia = drill + 0.3;
    let gap = dia + board.rules.min_clearance;
    let margin = dia / 2.0 + 0.1;
    let before = check(board).len();
    let backup = board.vias.len();
    let pours = PourFills::new(board);
    for (i, area) in heat_pads(board) {
        let p = board.pads[i].clone();
        let (w, h) = p.shape.size();
        let inside = |c: Point, m: f64| match p.shape {
            PadShape::Circle { d } => c.dist(p.pos) <= d / 2.0 - m,
            _ => (c.x - p.pos.x).abs() <= w / 2.0 - m && (c.y - p.pos.y).abs() <= h / 2.0 - m,
        };
        let have = board.vias.iter().filter(|v| inside(v.pos, 0.0)).count();
        let need = thermal_via_need(area, pours.area_at(&p));
        if have >= need {
            continue;
        }
        let (iw, ih) = ((w - 2.0 * margin).max(0.0), (h - 2.0 * margin).max(0.0));
        // a grid of about the pad's proportions with enough cells, as tight as the via spacing allows
        let fit = |len: f64| (len / gap).floor() as usize + 1;
        let (max_x, max_y) = (fit(iw), fit(ih));
        let nx = (((need as f64) * iw.max(0.1) / ih.max(0.1)).sqrt().ceil() as usize).clamp(1, max_x);
        let ny = need.div_ceil(nx).clamp(1, max_y);
        let at = |k: usize, n: usize, len: f64| if n == 1 { 0.0 } else { -len / 2.0 + len * k as f64 / (n - 1) as f64 };
        let mut have = have;
        'grid: for iy in 0..ny {
            for ix in 0..nx {
                if have >= need {
                    break 'grid;
                }
                let c = Point::new(p.pos.x + at(ix, nx, iw), p.pos.y + at(iy, ny, ih));
                let near = board.vias.iter().any(|v| v.pos.dist(c) < gap) || board.pads.iter().enumerate().any(|(j, q)| j != i && q.drill.is_some() && q.pos.dist(c) < gap);
                if inside(c, margin - 0.1) && !near {
                    board.vias.push(Via { pos: c, diameter: dia, drill });
                    have += 1;
                }
            }
        }
    }
    if check(board).len() > before {
        board.vias.truncate(backup);
        return 0;
    }
    board.vias.len() - backup
}
