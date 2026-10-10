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
}
