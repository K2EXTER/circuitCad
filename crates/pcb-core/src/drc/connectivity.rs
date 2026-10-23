use super::*;

pub(super) struct Cu {
    pub(super) geo: Geo,
    /// Copper layers the item is on (a through hole or via is on all of them).
    pub(super) layers: Vec<Side>,
}

impl Cu {
    pub(super) fn on(&self, s: Side) -> bool {
        self.layers.contains(&s)
    }
}

pub(super) fn copper_items(board: &Board) -> Vec<Cu> {
    let all = board.copper_layers();
    let mut out = vec![];
    for t in &board.tracks {
        out.push(Cu { geo: Geo::Cap { a: t.a, b: t.b, r: t.width / 2.0 }, layers: vec![t.side] });
    }
    for p in &board.pads {
        let layers = if p.drill.is_some() { all.clone() } else { vec![p.side] };
        out.push(Cu { geo: shape_geo(p.pos, p.shape), layers });
    }
    for via in &board.vias {
        out.push(Cu { geo: Geo::Cap { a: via.pos, b: via.pos, r: via.diameter / 2.0 }, layers: all.clone() });
    }
    out
}

pub(super) fn find(parent: &mut [usize], i: usize) -> usize {
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

/// The box (x0, x1, y0, y1) around a shape.
pub(super) fn bbox(g: &Geo) -> (f64, f64, f64, f64) {
    match *g {
        Geo::Cap { a, b, r } => (a.x.min(b.x) - r, a.x.max(b.x) + r, a.y.min(b.y) - r, a.y.max(b.y) + r),
        Geo::Rect { c, hw, hh } => (c.x - hw, c.x + hw, c.y - hh, c.y + hh),
    }
}

/// Every pair of items whose boxes, each grown by `margin`, overlap (so the pairs that can be within `margin` of each
/// other, and no others), as (i, j) with i before j in the order of the boxes' left edges. A sweep along x: only the
/// items whose boxes overlap in x are compared in y.
pub(super) fn near_pairs(items: &[Cu], margin: f64, mut each: impl FnMut(usize, usize)) {
    let boxes: Vec<(f64, f64, f64, f64)> = items.iter().map(|it| bbox(&it.geo)).collect();
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0.total_cmp(&boxes[b].0));
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if boxes[j].0 > boxes[i].1 + margin {
                break;
            }
            if boxes[j].2 > boxes[i].3 + margin || boxes[i].2 > boxes[j].3 + margin {
                continue;
            }
            each(i, j);
        }
    }
}

/// Union-find group id for every copper item.
pub(super) fn groups(items: &[Cu]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..items.len()).collect();
    near_pairs(items, 1e-9, |i, j| {
        let shared = items[i].layers.iter().any(|&s| items[j].on(s));
        if shared && dist(&items[i].geo, &items[j].geo) <= 1e-9 {
            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
            parent[a] = b;
        }
    });
    (0..items.len()).map(|i| find(&mut parent, i)).collect()
}

// ---------------------------------------------------------------- checks

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

/// Does the item lie in the pour's outline: its centre or an end inside, or its edge crossing the outline (a pad that sticks out of
/// a hand-drawn pour is still joined to it).
pub(super) fn anchors_inside(g: &Geo, poly: &[Point]) -> bool {
    let anchored = match g {
        Geo::Cap { a, b, .. } => inside_polygon(poly, *a) || inside_polygon(poly, *b),
        Geo::Rect { c, .. } => inside_polygon(poly, *c),
    };
    anchored
        || (0..poly.len()).any(|i| {
            let edge = Geo::Cap { a: poly[i], b: poly[(i + 1) % poly.len()], r: 0.0 };
            super::geo::dist(&edge, g) == 0.0
        })
}

pub(super) fn pad_nets(board: &Board, parent: &mut [usize], n_tracks: usize) -> HashMap<usize, String> {
    let mut m = HashMap::new();
    for (j, p) in board.pads.iter().enumerate() {
        if !p.net.is_empty() {
            let r = find(parent, n_tracks + j);
            m.entry(r).or_insert_with(|| p.net.clone());
        }
    }
    // a via placed for a net names its group too (a pad of the group is what counts when they differ: that is a short)
    for (k, v) in board.vias.iter().enumerate() {
        if !v.net.is_empty() {
            let r = find(parent, n_tracks + board.pads.len() + k);
            m.entry(r).or_insert_with(|| v.net.clone());
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
