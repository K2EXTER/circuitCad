//! Design-rule checks run before export.
//!
//! Copper clearance works on geometry only: copper items that touch on a layer
//! (directly, or through a via / through-hole pad) form one electrical group,
//! and only items in *different* groups must keep `min_clearance` apart.

use crate::geom::inside_polygon;
use crate::model::*;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub message: String,
    pub at: Point,
}

fn v(message: String, at: Point) -> Violation {
    Violation { message, at }
}

// ---------------------------------------------------------------- geometry

#[derive(Clone, Copy, Debug)]
pub(crate) enum Geo {
    /// Segment `a`-`b` swept by a disc of radius `r` (r > 0: track, round pad, oval pad, via).
    Cap { a: Point, b: Point, r: f64 },
    /// Axis-aligned rectangle.
    Rect { c: Point, hw: f64, hh: f64 },
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

pub(crate) fn point_seg(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

fn segs_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    // touching / collinear cases: an endpoint lying on the other segment
    point_seg(c, a, b) < 1e-12 || point_seg(d, a, b) < 1e-12 || point_seg(a, c, d) < 1e-12 || point_seg(b, c, d) < 1e-12
}

fn seg_seg(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segs_intersect(a, b, c, d) {
        return 0.0;
    }
    point_seg(a, c, d).min(point_seg(b, c, d)).min(point_seg(c, a, b)).min(point_seg(d, a, b))
}

pub(crate) fn point_rect(p: Point, c: Point, hw: f64, hh: f64) -> f64 {
    let dx = ((p.x - c.x).abs() - hw).max(0.0);
    let dy = ((p.y - c.y).abs() - hh).max(0.0);
    dx.hypot(dy)
}

fn seg_rect(a: Point, b: Point, c: Point, hw: f64, hh: f64) -> f64 {
    if point_rect(a, c, hw, hh) == 0.0 || point_rect(b, c, hw, hh) == 0.0 {
        return 0.0;
    }
    let k = [
        Point::new(c.x - hw, c.y - hh),
        Point::new(c.x + hw, c.y - hh),
        Point::new(c.x + hw, c.y + hh),
        Point::new(c.x - hw, c.y + hh),
    ];
    for i in 0..4 {
        if segs_intersect(a, b, k[i], k[(i + 1) % 4]) {
            return 0.0;
        }
    }
    k.iter()
        .map(|&q| point_seg(q, a, b))
        .fold(point_rect(a, c, hw, hh).min(point_rect(b, c, hw, hh)), f64::min)
}

/// Edge-to-edge distance (0 when touching or overlapping).
fn dist(x: &Geo, y: &Geo) -> f64 {
    match (*x, *y) {
        (Geo::Cap { a, b, r }, Geo::Cap { a: c, b: d, r: s }) => (seg_seg(a, b, c, d) - r - s).max(0.0),
        (Geo::Cap { a, b, r }, Geo::Rect { c, hw, hh }) | (Geo::Rect { c, hw, hh }, Geo::Cap { a, b, r }) => {
            (seg_rect(a, b, c, hw, hh) - r).max(0.0)
        }
        (Geo::Rect { c: c1, hw: w1, hh: h1 }, Geo::Rect { c: c2, hw: w2, hh: h2 }) => {
            let dx = ((c1.x - c2.x).abs() - w1 - w2).max(0.0);
            let dy = ((c1.y - c2.y).abs() - h1 - h2).max(0.0);
            dx.hypot(dy)
        }
    }
}

pub(crate) fn shape_geo(pos: Point, shape: PadShape) -> Geo {
    match shape {
        PadShape::Circle { d } => Geo::Cap { a: pos, b: pos, r: d / 2.0 },
        PadShape::Rect { w, h } => Geo::Rect { c: pos, hw: w / 2.0, hh: h / 2.0 },
        PadShape::Oval { w, h } => {
            let r = w.min(h) / 2.0;
            let off = (w.max(h) / 2.0) - r;
            let (da, db) = if w >= h {
                (Point::new(pos.x - off, pos.y), Point::new(pos.x + off, pos.y))
            } else {
                (Point::new(pos.x, pos.y - off), Point::new(pos.x, pos.y + off))
            };
            Geo::Cap { a: da, b: db, r }
        }
    }
}

fn center(g: &Geo) -> Point {
    match *g {
        Geo::Cap { a, b, .. } => Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        Geo::Rect { c, .. } => c,
    }
}

// ---------------------------------------------------------------- copper items

struct Cu {
    geo: Geo,
    top: bool,
    bottom: bool,
}

impl Cu {
    fn on(&self, s: Side) -> bool {
        match s {
            Side::Top => self.top,
            Side::Bottom => self.bottom,
        }
    }
}

fn copper_items(board: &Board) -> Vec<Cu> {
    let mut out = vec![];
    for t in &board.tracks {
        out.push(Cu {
            geo: Geo::Cap { a: t.a, b: t.b, r: t.width / 2.0 },
            top: t.side == Side::Top,
            bottom: t.side == Side::Bottom,
        });
    }
    for p in &board.pads {
        let both = p.drill.is_some();
        out.push(Cu {
            geo: shape_geo(p.pos, p.shape),
            top: both || p.side == Side::Top,
            bottom: both || p.side == Side::Bottom,
        });
    }
    for via in &board.vias {
        out.push(Cu { geo: Geo::Cap { a: via.pos, b: via.pos, r: via.diameter / 2.0 }, top: true, bottom: true });
    }
    out
}

fn find(parent: &mut Vec<usize>, i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = i;
    while parent[c] != r {
        let n = parent[c];
        parent[c] = r;
        c = n;
    }
    r
}

/// Union-find group id for every copper item.
fn groups(items: &[Cu]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..items.len()).collect();
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            let shared = [Side::Top, Side::Bottom].iter().any(|&s| items[i].on(s) && items[j].on(s));
            if shared && dist(&items[i].geo, &items[j].geo) <= 1e-9 {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    (0..items.len()).map(|i| find(&mut parent, i)).collect()
}

// ---------------------------------------------------------------- checks

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Top => "表面",
        Side::Bottom => "裏面",
    }
}

pub fn check(board: &Board) -> Vec<Violation> {
    let r = &board.rules;
    let mut out = vec![];

    if board.outline.len() < 3 {
        out.push(v("基板の外形が3点未満で、面になっていません。「基板外形」ツールか「矩形に設定」で外形を決めましょう".into(), Point::default()));
        return out;
    }

    for t in &board.tracks {
        if t.width + 1e-9 < r.min_track_width {
            out.push(v(format!("配線の幅 {:.3}mm が最小 {:.3}mm より細いです。配線を太くするか、発注先の最小幅に合わせたルールを選びましょう", t.width, r.min_track_width), t.a));
        }
        for p in [t.a, t.b] {
            if !inside_polygon(&board.outline, p) {
                out.push(v("配線の端が基板の外にあります。基板の内側に引き直すか、外形を広げましょう".into(), p));
            }
        }
    }

    let mut drills: Vec<(Point, f64, f64)> = vec![];
    for p in &board.pads {
        if let Some(d) = p.drill {
            let (w, h) = p.shape.size();
            drills.push((p.pos, d, (w.min(h) - d) / 2.0));
        }
    }
    for via in &board.vias {
        drills.push((via.pos, via.drill, (via.diameter - via.drill) / 2.0));
    }
    for (pos, d, ring) in &drills {
        if *d + 1e-9 < r.min_drill {
            out.push(v(format!("穴の径 {:.3}mm が最小 {:.3}mm より小さいです。穴を大きくするか、発注先に合ったルールを選びましょう", d, r.min_drill), *pos));
        }
        if *ring + 1e-9 < r.min_annular_ring {
            out.push(v(format!("穴のまわりの銅 (アニュラリング) {:.3}mm が最小 {:.3}mm より細いです。パッド/ビアの直径を大きくするか、穴を小さくしましょう", ring, r.min_annular_ring), *pos));
        }
        if !inside_polygon(&board.outline, *pos) {
            out.push(v("穴が基板の外にあります。基板の内側に動かすか、外形を広げましょう".into(), *pos));
        }
    }
    for h in &board.holes {
        if !inside_polygon(&board.outline, h.pos) {
            out.push(v("取り付け穴が基板の外にあります。基板の内側に動かすか、外形を広げましょう".into(), h.pos));
        }
    }

    for z in &board.zones {
        if z.polygon.len() < 3 {
            out.push(v(format!("ゾーン({})の頂点が3点未満です", z.net), Point::default()));
            continue;
        }
        for p in &z.polygon {
            if !inside_polygon(&board.outline, *p) {
                out.push(v(format!("ゾーン({})が基板外形の外にあります", z.net), *p));
                break;
            }
        }
    }

    out.extend(clearance(board));
    out.extend(shorts(board));
    out
}

/// Copper-to-copper and copper-to-board-edge clearance.
pub fn clearance(board: &Board) -> Vec<Violation> {
    let r = &board.rules;
    let items = copper_items(board);
    let grp = connectivity(board).items;
    let mut out = vec![];

    for side in [Side::Top, Side::Bottom] {
        let on: Vec<usize> = (0..items.len()).filter(|&i| items[i].on(side)).collect();
        for (k, &i) in on.iter().enumerate() {
            for &j in &on[k + 1..] {
                if grp[i] == grp[j] {
                    continue;
                }
                let d = dist(&items[i].geo, &items[j].geo);
                if d + 1e-9 < r.min_clearance {
                    let (p, q) = (center(&items[i].geo), center(&items[j].geo));
                    out.push(v(
                        format!("{} 銅箔どうしの間隔 {:.3}mm が最小 {:.3}mm より狭いです。配線やパッドを離しましょう (別のネットが近づきすぎ)", side_name(side), d, r.min_clearance),
                        Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
                    ));
                }
            }
        }
    }

    let n = board.outline.len();
    for it in &items {
        for e in 0..n {
            let edge = Geo::Cap { a: board.outline[e], b: board.outline[(e + 1) % n], r: 0.0 };
            let d = dist(&it.geo, &edge);
            if d + 1e-9 < r.edge_clearance {
                out.push(v(format!("銅箔と基板の端の間隔 {:.3}mm が最小 {:.3}mm より狭いです。端から離しましょう (切り出しで銅が欠けたり短絡したりします)", d, r.edge_clearance), center(&it.geo)));
                break;
            }
        }
    }
    out
}

/// Electrical grouping of all copper: touching items and zone-connected items share a group.
pub struct Connectivity {
    /// Group id of every copper item in the order tracks, pads, vias.
    pub items: Vec<usize>,
    pub n_tracks: usize,
    pub n_pads: usize,
    /// Net name of a group, taken from the pads it contains.
    net_of_group: HashMap<usize, String>,
}

impl Connectivity {
    pub fn track_group(&self, i: usize) -> usize {
        self.items[i]
    }
    pub fn pad_group(&self, i: usize) -> usize {
        self.items[self.n_tracks + i]
    }
    pub fn via_group(&self, i: usize) -> usize {
        self.items[self.n_tracks + self.n_pads + i]
    }
    pub fn group_net(&self, g: usize) -> Option<&str> {
        self.net_of_group.get(&g).map(|s| s.as_str())
    }
    pub fn track_net(&self, i: usize) -> Option<&str> {
        self.group_net(self.track_group(i))
    }
    pub fn pad_net(&self, i: usize) -> Option<&str> {
        self.group_net(self.pad_group(i))
    }
    pub fn via_net(&self, i: usize) -> Option<&str> {
        self.group_net(self.via_group(i))
    }
}

fn anchors_inside(g: &Geo, poly: &[Point]) -> bool {
    match g {
        Geo::Cap { a, b, .. } => inside_polygon(poly, *a) || inside_polygon(poly, *b),
        Geo::Rect { c, .. } => inside_polygon(poly, *c),
    }
}

fn pad_nets(board: &Board, parent: &mut Vec<usize>, n_tracks: usize) -> HashMap<usize, String> {
    let mut m = HashMap::new();
    for (j, p) in board.pads.iter().enumerate() {
        if !p.net.is_empty() {
            let r = find(parent, n_tracks + j);
            m.entry(r).or_insert_with(|| p.net.clone());
        }
    }
    m
}

pub fn connectivity(board: &Board) -> Connectivity {
    let items = copper_items(board);
    let (n_tracks, n_pads) = (board.tracks.len(), board.pads.len());
    let mut parent = groups(&items);
    // pads of the same part and pin (a package tab, an exposed pad) are one connection
    let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
    for (j, p) in board.pads.iter().enumerate().filter(|(_, p)| !p.refdes.is_empty() && !p.pin.is_empty()) {
        match seen.get(&(p.refdes.as_str(), p.pin.as_str())) {
            Some(&first) => {
                let (a, b) = (find(&mut parent, first), find(&mut parent, n_tracks + j));
                parent[a] = b;
            }
            None => {
                seen.insert((p.refdes.as_str(), p.pin.as_str()), n_tracks + j);
            }
        }
    }
    // a zone joins every item of its net that has an anchor point inside it
    for z in board.zones.iter().filter(|z| !z.net.is_empty() && z.polygon.len() >= 3) {
        let nets = pad_nets(board, &mut parent, n_tracks);
        let mut members = vec![];
        for (i, it) in items.iter().enumerate() {
            if it.on(z.side) && anchors_inside(&it.geo, &z.polygon) {
                let r = find(&mut parent, i);
                if nets.get(&r) == Some(&z.net) {
                    members.push(i);
                }
            }
        }
        for w in members.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    let net_of_group = pad_nets(board, &mut parent, n_tracks);
    let all: Vec<usize> = (0..items.len()).map(|i| find(&mut parent, i)).collect();
    let net_of_group = net_of_group.into_iter().map(|(k, v)| (find(&mut parent, k), v)).collect();
    Connectivity { items: all, n_tracks, n_pads, net_of_group }
}

/// Group id for every entry of `board.pads`, from copper connectivity.
pub(crate) fn pad_groups(board: &Board) -> Vec<usize> {
    let c = connectivity(board);
    (0..board.pads.len()).map(|i| c.pad_group(i)).collect()
}

/// Pads of different nets that end up in one copper group.
pub fn shorts(board: &Board) -> Vec<Violation> {
    let grp = pad_groups(board);
    let mut seen: std::collections::BTreeMap<usize, (Vec<String>, Point)> = Default::default();
    for (p, g) in board.pads.iter().zip(grp) {
        if p.net.is_empty() {
            continue;
        }
        let e = seen.entry(g).or_insert_with(|| (vec![], p.pos));
        if !e.0.contains(&p.net) {
            e.0.push(p.net.clone());
        }
    }
    seen.into_values()
        .filter(|(nets, _)| nets.len() > 1)
        .map(|(nets, at)| v(format!("ネット短絡: {}", nets.join(" と ")), at))
        .collect()
}

/// Smallest silk line width and text height most fabs print legibly (mm).
pub const MIN_SILK_WIDTH: f64 = 0.15;
pub const MIN_SILK_TEXT_HEIGHT: f64 = 0.8;

/// Silk-screen problems that are not copper rule violations but spoil the printed board:
/// silk over an exposed pad (the fab trims it away, or the pad solders badly), lines too thin to
/// print, and text too small to read.
pub fn silk_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for l in &board.silk {
        if l.width + 1e-9 < MIN_SILK_WIDTH {
            out.push(v(format!("シルクの線の太さ {:.2}mm が細すぎます (印刷できるのは {:.2}mm 以上が目安)。太くしましょう", l.width, MIN_SILK_WIDTH), l.a));
        }
        let line = Geo::Cap { a: l.a, b: l.b, r: l.width / 2.0 };
        for p in &board.pads {
            let exposed = p.drill.is_some() || p.side == l.side;
            if exposed && dist(&line, &shape_geo(p.pos, p.shape)) < 1e-9 {
                out.push(v(
                    format!("シルク (白い線) が {} のパッドに重なっています。パッドの上のシルクは製造時に削られ、はんだ付けにも邪魔です。シルクをずらしましょう", if p.refdes.is_empty() { "穴/パッド".to_string() } else { format!("{}.{}", p.refdes, p.pin) }),
                    p.pos,
                ));
                break;
            }
        }
    }
    for t in &board.texts {
        if t.height + 1e-9 < MIN_SILK_TEXT_HEIGHT {
            out.push(v(format!("シルクの文字「{}」の高さ {:.2}mm が小さすぎて読めません ({:.1}mm 以上が目安)", t.text, t.height, MIN_SILK_TEXT_HEIGHT), t.pos));
        }
    }
    out
}

/// The narrowest strip of solder mask (mm) that fabs keep between two pad openings. Narrower than this and
/// the strip peels off during manufacturing, so the solder is not held back during reflow.
pub const MIN_MASK_DAM: f64 = 0.1;

/// Solder mask dams: between the openings of two pads of different nets the mask must leave a strip (the
/// "dam"). The openings are the pads grown by `rules.mask_expansion` on each side, so
/// dam = gap between the pad edges - 2 x expansion. Too narrow = the fab drops it and the solder can bridge.
pub fn mask_dam_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    let e = board.rules.mask_expansion;
    let exposed = |p: &Pad, side: Side| p.drill.is_some() || p.side == side;
    for (i, a) in board.pads.iter().enumerate() {
        for b in &board.pads[i + 1..] {
            if !a.net.is_empty() && a.net == b.net {
                continue;
            }
            // two pads of the same part and pin (a pad split in pieces) are one opening
            if !a.refdes.is_empty() && a.refdes == b.refdes && a.pin == b.pin {
                continue;
            }
            if ![Side::Top, Side::Bottom].into_iter().any(|s| exposed(a, s) && exposed(b, s)) {
                continue;
            }
            let (sa, sb) = (a.shape.size(), b.shape.size());
            let reach = sa.0.max(sa.1) / 2.0 + sb.0.max(sb.1) / 2.0 + 2.0 * e + MIN_MASK_DAM;
            if (a.pos.x - b.pos.x).abs() > reach || (a.pos.y - b.pos.y).abs() > reach {
                continue;
            }
            let gap = dist(&shape_geo(a.pos, a.shape), &shape_geo(b.pos, b.shape));
            let dam = gap - 2.0 * e;
            if gap < 1e-9 {
                continue; // touching pads are the DRC's job (clearance / short)
            }
            if dam + 1e-9 < MIN_MASK_DAM {
                let name = |p: &Pad| if p.refdes.is_empty() { "穴/パッド".to_string() } else { format!("{}.{}", p.refdes, p.pin) };
                out.push(v(
                    format!(
                        "{} と {} のパッドの間のソルダーマスクの帯が {:.2}mm しかありません ({:.2}mm 以上が目安)。{}パッドを離す、小さくする、または発注先のマスクの設定を確かめましょう",
                        name(a),
                        name(b),
                        dam.max(0.0),
                        MIN_MASK_DAM,
                        if dam <= 0.0 { "マスクの開口がつながり、はんだがブリッジして短絡することがあります。" } else { "製造時に帯が剥がれて、はんだがブリッジすることがあります。" }
                    ),
                    Point::new((a.pos.x + b.pos.x) / 2.0, (a.pos.y + b.pos.y) / 2.0),
                ));
            }
        }
    }
    out
}

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

/// Thermal vias: a heat pad needs vias in it that carry the heat through the board to copper on the back (about one
/// per 2.5 mm², at least two). Without them the heat has only the small top-side copper to spread in.
pub fn thermal_via_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for (i, area) in heat_pads(board) {
        let p = &board.pads[i];
        let (w, h) = p.shape.size();
        let inside = |c: Point| match p.shape {
            PadShape::Circle { d } => c.dist(p.pos) <= d / 2.0,
            _ => (c.x - p.pos.x).abs() <= w / 2.0 && (c.y - p.pos.y).abs() <= h / 2.0,
        };
        let have = board.vias.iter().filter(|v| inside(v.pos)).count();
        let need = ((area / 2.5).ceil() as usize).clamp(2, 9);
        if have < need {
            out.push(v(
                format!(
                    "{}.{} は放熱パッド (面積 {area:.1}mm²) ですが、中に熱を裏面へ逃がすビア (サーマルビア) が {have} 個しかありません ({need} 個以上が目安)。パッドの中にビアを格子状に置いて (穴 0.3mm くらい)、裏面の同じネットの銅 (ゾーン) につなぐと、熱が基板全体に広がります",
                    p.refdes, p.pin
                ),
                p.pos,
            ));
        }
    }
    out
}

/// A copper pour piece thinner than this (mm, as an equivalent strip width) is a sliver.
pub const MIN_POUR_WIDTH: f64 = 0.2;

/// Problems of the pours that the clearance rules do not see, found on the copper that will really be made
/// (`zonefill`): slivers (thin strips of copper left between cut-outs, which peel off in manufacturing), and
/// islands (pieces of the pour that touch no pad or via of its net, so they are connected to nothing and only
/// act as an antenna).
pub fn zone_checks(board: &Board) -> Vec<Violation> {
    use crate::geom::{inside_polygon, signed_area};
    let mut out = vec![];
    if board.zones.is_empty() {
        return out;
    }
    let conn = connectivity(board);
    for z in &board.zones {
        let side = if z.side == Side::Top { "表面" } else { "裏面" };
        let name = if z.net.is_empty() { "ネットなし".to_string() } else { z.net.clone() };
        for shape in crate::zonefill::zone_fill(board, z, &conn) {
            let perimeter = |c: &[Point]| (0..c.len()).map(|i| c[i].dist(c[(i + 1) % c.len()])).sum::<f64>();
            let area = signed_area(&shape.outer).abs() - shape.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>();
            let edge = perimeter(&shape.outer) + shape.holes.iter().map(|h| perimeter(h)).sum::<f64>();
            let at = shape.outer[0];
            if edge > 0.0 && 2.0 * area / edge < MIN_POUR_WIDTH {
                out.push(v(
                    format!("{side}のゾーン ({name}) に、細い銅のかけら (スリバー、幅 約 {:.2}mm) があります。細すぎる銅は製造中にはがれて、ショートのもとになります。ゾーンの形を変える、配線やパッドの位置を動かす、クリアランスを小さくして、細い所をなくしましょう", 2.0 * area / edge),
                    at,
                ));
                continue;
            }
            if z.net.is_empty() {
                continue;
            }
            let inside = |p: Point| inside_polygon(&shape.outer, p) && !shape.holes.iter().any(|h| inside_polygon(h, p));
            let touches_pad = board.pads.iter().any(|p| p.net == z.net && (p.drill.is_some() || p.side == z.side) && inside(p.pos));
            let touches_via = board.vias.iter().enumerate().any(|(k, via)| conn.via_net(k) == Some(z.net.as_str()) && inside(via.pos));
            if !touches_pad && !touches_via {
                out.push(v(
                    format!("{side}のゾーン ({name}) に、どこにもつながっていない銅の島 ({area:.1} mm²) があります。電気的には何ともつながらず、ノイズを拾うアンテナになります。島にビアかパッドをつなぐ、島を消す (ゾーンの形を変える)、のどちらかにしましょう"),
                    at,
                ));
            }
        }
    }
    out
}

/// Farther than this (mm) from an IC supply pin, a bypass capacitor stops doing its job: the
/// trace between them adds inductance that the fast current spikes cannot get through.
pub const MAX_DECOUPLING_DISTANCE: f64 = 8.0;

fn is_ground_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n.starts_with("GND") || n.ends_with("GND") || n == "VSS" || n == "0"
}

fn is_supply_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n.starts_with("VCC") || n.starts_with("VDD") || n.starts_with("VBUS") || n.starts_with('+') || n == "V+" || (n.contains('V') && n.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// ICs (refdes `U…` with 5+ pads) whose supply pin has no bypass capacitor (a `C…` part joining the
/// same net to ground) close by. Skipped when the board has no capacitor on that net at all and
/// no ground net, so a half-finished board is not nagged.
pub fn decoupling_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for part in board.parts.iter().filter(|p| p.refdes.starts_with('U')) {
        let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
        if pads.len() < 5 {
            continue;
        }
        for sp in pads.iter().filter(|p| is_supply_net(&p.net)) {
            // capacitors that bridge this supply net and a ground net
            let mut nearest: Option<f64> = None;
            for c in board.parts.iter().filter(|c| c.refdes.starts_with('C')) {
                let cp: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == c.refdes).collect();
                if !cp.iter().any(|p| is_ground_net(&p.net)) {
                    continue;
                }
                for p in cp.iter().filter(|p| p.net == sp.net) {
                    let d = ((p.pos.x - sp.pos.x).powi(2) + (p.pos.y - sp.pos.y).powi(2)).sqrt();
                    nearest = Some(nearest.map_or(d, |n: f64| n.min(d)));
                }
            }
            let at = sp.pos;
            let pin = format!("{}.{} ({})", part.refdes, sp.pin, sp.net);
            match nearest {
                None => out.push(v(
                    format!("{pin} の電源ピンに、GND との間のバイパスコンデンサ (0.1µF 前後) が見当たりません。ICは切り替えの瞬間に大きな電流を引くため、近くにコンデンサがないと電源が揺れて誤動作します。電源ピンのすぐそばに 0.1µF を足しましょう"),
                    at,
                )),
                Some(d) if d > MAX_DECOUPLING_DISTANCE => out.push(v(
                    format!("{pin} から一番近いバイパスコンデンサまで {d:.1} mm あります。電源ピンから離れるほど配線のインダクタンスが増えて効きが悪くなります。{:.0} mm 以内、できれば数 mm の距離に置きましょう", MAX_DECOUPLING_DISTANCE),
                    at,
                )),
                _ => {}
            }
        }
    }
    out
}

/// Where two tracks of the same layer meet at an angle sharper than 90 degrees. The wedge of
/// copper-free space between them traps etchant ("acid trap") and the corner can over-etch; it also
/// makes a fast signal reflect. Use 45-degree bends (an interior angle of 135 degrees) instead.
pub fn acute_angle_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    let tracks = &board.tracks;
    for (i, t) in tracks.iter().enumerate() {
        for u in tracks.iter().skip(i + 1).filter(|u| u.side == t.side) {
            for (p, q) in [(t.a, t.b), (t.b, t.a)] {
                // `p` is the shared end; `q` is the far end of t; the far end of u is whichever end is not at p
                let far = if (u.a.x - p.x).hypot(u.a.y - p.y) < 1e-6 {
                    u.b
                } else if (u.b.x - p.x).hypot(u.b.y - p.y) < 1e-6 {
                    u.a
                } else {
                    continue;
                };
                let (v1, v2) = ((q.x - p.x, q.y - p.y), (far.x - p.x, far.y - p.y));
                let (l1, l2) = (v1.0.hypot(v1.1), v2.0.hypot(v2.1));
                if l1 < 1e-6 || l2 < 1e-6 {
                    continue;
                }
                let cos = ((v1.0 * v2.0 + v1.1 * v2.1) / (l1 * l2)).clamp(-1.0, 1.0);
                let deg = cos.acos().to_degrees();
                // a straight continuation is 180; a 45-degree bend is 135; 90 is the sharpest we accept
                if deg < 89.0 && deg > 1.0 {
                    out.push(v(
                        format!("配線が {deg:.0}° の鋭角で曲がっています。鋭角の隙間には、基板をつくる薬品がたまって銅が余計に削れる (酸のたまり)・高速な信号が反射する、といった問題が出ます。曲がり角は 135° (45° の折れ) か 90° にしましょう"),
                        p,
                    ));
                }
            }
        }
    }
    out
}

/// A board with ground pads but no ground copper pour: worth a hint on a 2-layer board, where a
/// ground fill gives every signal a short return path and cuts noise.
pub fn missing_ground_fill(board: &Board) -> bool {
    let has_ground = board.pads.iter().any(|p| is_ground_net(&p.net));
    let has_fill = board.zones.iter().any(|z| is_ground_net(&z.net));
    has_ground && !has_fill && board.pads.iter().filter(|p| !p.net.is_empty()).count() >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(side: Side, a: (f64, f64), b: (f64, f64), w: f64) -> Track {
        Track { side, a: Point::new(a.0, a.1), b: Point::new(b.0, b.1), width: w }
    }

    fn board() -> Board {
        Board::new("t", 40.0, 40.0)
    }

    #[test]
    fn parallel_tracks_too_close() {
        let mut b = board();
        // gap = 0.35 - 0.25 = 0.10 < 0.15
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (10.0, 10.35), (20.0, 10.35), 0.25));
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn parallel_tracks_far_enough() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (10.0, 10.5), (20.0, 10.5), 0.25)); // gap 0.25
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn different_layers_do_not_interact() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Bottom, (10.0, 10.1), (20.0, 10.1), 0.25));
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn touching_items_are_one_net() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 10.0), (20.0, 20.0), 0.25));
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn via_joins_layers_so_crossing_other_net_is_flagged() {
        let mut b = board();
        // top track A -> via -> bottom track A'; bottom track B passes 0.05 from the via ring
        b.tracks.push(track(Side::Top, (10.0, 10.0), (15.0, 10.0), 0.25));
        b.vias.push(Via { pos: Point::new(15.0, 10.0), diameter: 0.6, drill: 0.3 });
        b.tracks.push(track(Side::Bottom, (15.0, 10.0), (15.0, 20.0), 0.25));
        b.tracks.push(track(Side::Bottom, (15.45, 5.0), (15.45, 12.0), 0.2));
        // gap between via edge (x=15.3) and B edge (x=15.35) is 0.05
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn rect_pad_to_track() {
        let mut b = board();
        b.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top));
        // pad edge x=10.5, track edge x=10.6 -> 0.1
        b.tracks.push(track(Side::Top, (10.725, 5.0), (10.725, 15.0), 0.25));
        assert_eq!(clearance(&b).len(), 1);
        b.tracks[0] = track(Side::Top, (10.85, 5.0), (10.85, 15.0), 0.25); // gap 0.225
        assert!(clearance(&b).is_empty());
    }

    #[test]
    fn copper_near_board_edge() {
        let mut b = board();
        b.tracks.push(track(Side::Top, (0.3, 10.0), (0.3, 20.0), 0.25)); // edge gap ~0.175 < 0.3
        assert_eq!(clearance(&b).len(), 1);
    }

    #[test]
    fn silk_over_a_pad_thin_silk_and_tiny_text_are_flagged() {
        let mut b = board();
        b.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top));
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(9.0, 10.0), b: Point::new(11.0, 10.0), width: 0.15, refdes: String::new() });
        // far away and on the other side: fine
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(20.0, 10.0), b: Point::new(22.0, 10.0), width: 0.15, refdes: String::new() });
        b.silk.push(SilkLine { side: Side::Bottom, a: Point::new(9.0, 10.0), b: Point::new(11.0, 10.0), width: 0.15, refdes: String::new() });
        assert_eq!(silk_checks(&b).len(), 1, "{:?}", silk_checks(&b));
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(30.0, 10.0), b: Point::new(32.0, 10.0), width: 0.08, refdes: String::new() });
        b.texts.push(SilkText { side: Side::Top, pos: Point::new(5.0, 5.0), height: 0.5, text: "R1".into(), refdes: "R1".into() });
        let v = silk_checks(&b);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().any(|x| x.message.contains("パッドに重なって")) && v.iter().any(|x| x.message.contains("細すぎ")) && v.iter().any(|x| x.message.contains("小さすぎ")));
    }

    #[test]
    fn narrow_mask_dams_between_different_nets_are_flagged() {
        let pad = |x: f64, net: &str| {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 0.6, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            p
        };
        let mut b = board();
        // gap 0.2 mm, mask opens 0.05 on each side: a 0.1 mm dam is just enough
        b.pads = vec![pad(10.0, "A"), pad(10.8, "B")];
        assert!(mask_dam_checks(&b).is_empty());
        // gap 0.15 mm: the dam is 0.05 mm
        b.pads = vec![pad(10.0, "A"), pad(10.75, "B")];
        let v = mask_dam_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("0.05mm") && v[0].message.contains("剥がれ"));
        // gap 0.08 mm: the openings merge
        b.pads = vec![pad(10.0, "A"), pad(10.68, "B")];
        assert!(mask_dam_checks(&b)[0].message.contains("つながり"));
        // the same net does not need a dam; pads on opposite sides do not share a mask
        b.pads = vec![pad(10.0, "A"), pad(10.75, "A")];
        assert!(mask_dam_checks(&b).is_empty());
        let mut other_side = pad(10.75, "B");
        other_side.side = Side::Bottom;
        b.pads = vec![pad(10.0, "A"), other_side];
        assert!(mask_dam_checks(&b).is_empty());
    }

    /// The footprints that come with the program keep the mask strip between their own pads (a false alarm on
    /// every board that uses them would teach the wrong lesson).
    #[test]
    fn built_in_footprints_have_mask_dams() {
        let mut bad = vec![];
        for name in crate::footprints::names() {
            let Some(fp) = crate::footprints::get(&name) else { continue };
            let mut b = board();
            for (i, p) in fp.pads.iter().enumerate() {
                let mut pad = Pad::new(Point::new(20.0 + p.pos.x, 20.0 + p.pos.y), p.shape, p.drill, Side::Top);
                pad.refdes = "U1".into();
                pad.pin = p.pin.clone();
                pad.net = format!("N{i}");
                b.pads.push(pad);
            }
            if let Some(first) = mask_dam_checks(&b).first() {
                bad.push(format!("{name}: {}", first.message));
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn pour_slivers_and_islands_are_found() {
        let mut b = board();
        let z = |x0: f64, y0: f64, x1: f64, y1: f64| Zone::new("GND", Side::Top, vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]);
        // a plain pour with a GND pad in it: nothing to report
        let mut gnd = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        gnd.net = "GND".into();
        b.pads.push(gnd);
        b.zones.push(z(5.0, 5.0, 25.0, 15.0));
        assert!(zone_checks(&b).is_empty(), "{:?}", zone_checks(&b));
        // a track of another net cuts the pour in two: the far piece touches no GND pad, it is an island
        b.tracks.push(Track { side: Side::Top, a: Point::new(18.0, 4.0), b: Point::new(18.0, 16.0), width: 0.25 });
        let v = zone_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("島"), "{v:?}");
        // a pour that is only a thin strip is a sliver
        let mut b = board();
        b.zones.push(z(5.0, 5.0, 25.0, 5.12));
        let v = zone_checks(&b);
        assert!(v.iter().any(|x| x.message.contains("スリバー")), "{v:?}");
    }

    #[test]
    fn a_heat_pad_needs_thermal_vias() {
        let mut b = board();
        // a small 3-pin part with a big tab (like a SOT-223)
        for (x, y, w, h, pin) in [(10.0, 10.0, 2.0, 1.5, "1"), (10.0, 12.3, 2.0, 1.5, "2"), (16.0, 11.0, 2.0, 3.8, "3")] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
            p.refdes = "U1".into();
            p.pin = pin.into();
            b.pads.push(p);
        }
        assert_eq!(heat_pads(&b).len(), 1, "only the tab is a heat pad");
        let v = thermal_via_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("U1.3") && v[0].message.contains("0 個"), "{v:?}");
        // vias in the tab: 7.6 mm2 asks for 4
        for dy in [-1.2, -0.4, 0.4] {
            b.vias.push(Via { pos: Point::new(16.0, 11.0 + dy), diameter: 0.6, drill: 0.3 });
        }
        assert_eq!(thermal_via_checks(&b).len(), 1, "three are not enough");
        b.vias.push(Via { pos: Point::new(16.0, 12.8), diameter: 0.6, drill: 0.3 });
        assert!(thermal_via_checks(&b).is_empty());
        // a plain two-pad resistor with big pads is not a heat pad
        let mut c = board();
        for (x, pin) in [(10.0, "1"), (14.0, "2")] {
            let mut p = Pad::new(Point::new(x, 10.0), PadShape::Rect { w: 1.5, h: 3.0 }, None, Side::Top);
            p.refdes = "R1".into();
            p.pin = pin.into();
            c.pads.push(p);
        }
        assert!(heat_pads(&c).is_empty());
    }

    fn ic_board(cap_x: Option<f64>) -> Board {
        let mut b = Board::new("t", 40.0, 40.0);
        b.parts.push(Part { refdes: "U1".into(), value: "74HC00".into(), footprint: "x".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        for i in 0..8 {
            let mut p = Pad::new(Point::new(10.0 + i as f64 * 0.5, 10.0), PadShape::Circle { d: 1.0 }, Some(0.6), Side::Top);
            p.refdes = "U1".into();
            p.pin = (i + 1).to_string();
            p.net = match i {
                0 => "VCC".into(),
                1 => "GND".into(),
                _ => String::new(),
            };
            b.pads.push(p);
        }
        if let Some(x) = cap_x {
            b.parts.push(Part { refdes: "C1".into(), value: "100n".into(), footprint: "C_0805".into(), pos: Point::new(x, 10.0), rot: 0, side: Side::Top });
            for (pin, net, dx) in [("1", "VCC", 0.0), ("2", "GND", 2.0)] {
                let mut p = Pad::new(Point::new(x + dx, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
                p.refdes = "C1".into();
                p.pin = pin.into();
                p.net = net.into();
                b.pads.push(p);
            }
        }
        b
    }

    #[test]
    fn supply_pin_needs_a_bypass_capacitor_nearby() {
        // no capacitor at all
        let none = decoupling_checks(&ic_board(None));
        assert_eq!(none.len(), 1, "{none:?}");
        assert!(none[0].message.contains("U1.1") && none[0].message.contains("見当たりません"));
        // a capacitor 2 mm away is fine
        assert!(decoupling_checks(&ic_board(Some(12.0))).is_empty());
        // 20 mm away is too far
        let far = decoupling_checks(&ic_board(Some(30.0)));
        assert_eq!(far.len(), 1, "{far:?}");
        assert!(far[0].message.contains("mm あります"));
    }

    #[test]
    fn acute_corners_are_flagged_but_45_degree_bends_are_not() {
        let mut b = board();
        // 135 degree interior angle (a 45 degree bend)
        b.tracks.push(track(Side::Top, (10.0, 10.0), (20.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 10.0), (25.0, 15.0), 0.25));
        // a plain 90 degree corner
        b.tracks.push(track(Side::Top, (30.0, 10.0), (35.0, 10.0), 0.25));
        b.tracks.push(track(Side::Top, (35.0, 10.0), (35.0, 15.0), 0.25));
        assert!(acute_angle_checks(&b).is_empty(), "{:?}", acute_angle_checks(&b));
        // 45 degree interior angle: doubling back
        b.tracks.push(track(Side::Top, (10.0, 30.0), (20.0, 30.0), 0.25));
        b.tracks.push(track(Side::Top, (20.0, 30.0), (10.0, 35.0), 0.25));
        let v = acute_angle_checks(&b);
        assert_eq!(v.len(), 1, "{v:?}");
        // the same shape on different layers does not touch
        let mut c = board();
        c.tracks.push(track(Side::Top, (10.0, 30.0), (20.0, 30.0), 0.25));
        c.tracks.push(track(Side::Bottom, (20.0, 30.0), (10.0, 35.0), 0.25));
        assert!(acute_angle_checks(&c).is_empty());
    }

    #[test]
    fn a_ground_fill_is_suggested_only_when_there_is_none() {
        let mut b = ic_board(Some(12.0));
        assert!(missing_ground_fill(&b));
        b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(30.0, 1.0), Point::new(30.0, 30.0)]));
        assert!(!missing_ground_fill(&b));
    }
}
