use super::*;

/// A pour that loses at least this share of its area to later pours of other nets is reported.
pub const COVERED_SHARE: f64 = 0.5;

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
    out.extend(split_pour_checks(board));
    out.extend(covered_pour_checks(board));
    for z in &board.zones {
        let side = side_name(z.side);
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

/// The pads of a net that has a pour, joined only by the pour's real copper: when the copper is in several pieces (the pour is cut apart
/// by other nets' tracks and pads) and a piece has pads that no track or via joins to the rest, they are not connected to it, whatever the
/// outline of the pour says. One finding per net, at the pads that are left out.
pub fn split_pour_checks(board: &Board) -> Vec<Violation> {
    pour_splits(board)
        .into_iter()
        .map(|s| {
            let names: Vec<String> = s.left.iter().take(4).map(|&j| format!("{}-{}", board.pads[j].refdes, board.pads[j].pin)).collect();
            v(
                format!("ネット {} のベタが、他のネットの配線やパッドで分断されていて、{} が、残りの {} とつながっていません。配線かビアで、つなぎましょう (ベタの見た目は広がっていても、銅はつながっていません)", s.net, names.join("・"), s.net),
                board.pads[s.left[0]].pos,
            )
        })
        .collect()
}

/// A net whose pads are not all joined by its copper: the pads (numbers in `board.pads`) of the pieces that are left out, and the pads
/// of the piece that is kept (the one with the most pads).
pub struct PourSplit {
    pub net: String,
    pub left: Vec<usize>,
    pub main: Vec<usize>,
}

/// An airwire for each net split like that: from the nearest left-out pad to a pad of the kept piece (the yellow line to draw).
pub fn pour_split_airwires(board: &Board) -> Vec<crate::ratsnest::Airwire> {
    pour_splits(board)
        .into_iter()
        .filter_map(|s| {
            let a = &board.pads[s.left[0]];
            let b = s.main.iter().map(|&j| &board.pads[j]).min_by(|x, y| x.pos.dist(a.pos).total_cmp(&y.pos.dist(a.pos)))?;
            Some(crate::ratsnest::Airwire { net: s.net.clone(), a: a.pos, b: b.pos })
        })
        .collect()
}

fn pour_splits(board: &Board) -> Vec<PourSplit> {
    use crate::geom::inside_polygon;
    if !board.zones.iter().any(|z| !z.net.is_empty() && z.has_area()) {
        return vec![];
    }
    let base = connectivity_without_pours(board);
    let items = copper_items(board);
    let mut parent: HashMap<usize, usize> = HashMap::new();
    fn root(parent: &mut HashMap<usize, usize>, g: usize) -> usize {
        let mut r = g;
        while let Some(&p) = parent.get(&r) {
            if p == r {
                break;
            }
            r = p;
        }
        r
    }
    let mut nets: Vec<&str> = board.zones.iter().filter(|z| !z.net.is_empty() && z.has_area()).map(|z| z.net.as_str()).collect();
    nets.sort_unstable();
    nets.dedup();
    let fills = crate::zonefill::merged_fills(board, &base);
    let mut out: Vec<PourSplit> = vec![];
    for net in nets {
        // the copper groups of the net that hold a pad (a group of tracks alone is nothing to connect)
        let group_of = |i: usize| base.items[i];
        let mut pad_groups: Vec<(usize, usize)> = vec![]; // (group, pad number)
        for (j, p) in board.pads.iter().enumerate() {
            if p.net == net {
                let g = group_of(base.n_tracks + j);
                if !pad_groups.iter().any(|(x, _)| *x == g) {
                    pad_groups.push((g, j));
                }
            }
        }
        if pad_groups.len() < 2 {
            continue;
        }
        parent.clear();
        for (g, _) in &pad_groups {
            parent.insert(*g, *g);
        }
        // every piece of the pour of this net joins the groups that have copper in it
        for (side, name, shapes) in fills.iter().filter(|(_, n, _)| n == net) {
            let _ = name;
            for shape in shapes {
                let inside = |p: Point| inside_polygon(&shape.outer, p) && !shape.holes.iter().any(|h| inside_polygon(h, p));
                let mut first: Option<usize> = None;
                for (i, it) in items.iter().enumerate() {
                    if !it.on(*side) || base.group_net(group_of(i)) != Some(net) || !anchors_inside(&it.geo, &shape.outer) || !inside(center(&it.geo)) && !matches!(it.geo, Geo::Cap { .. }) {
                        continue;
                    }
                    let g = group_of(i);
                    if !parent.contains_key(&g) {
                        parent.insert(g, g);
                    }
                    match first {
                        None => first = Some(g),
                        Some(f) => {
                            let (a, b) = (root(&mut parent, f), root(&mut parent, g));
                            if a != b {
                                parent.insert(a, b);
                            }
                        }
                    }
                }
            }
        }
        let mut comps: Vec<(usize, Vec<usize>)> = vec![];
        for (g, j) in &pad_groups {
            let r = root(&mut parent, *g);
            match comps.iter_mut().find(|(x, _)| *x == r) {
                Some((_, pads)) => pads.push(*j),
                None => comps.push((r, vec![*j])),
            }
        }
        if comps.len() > 1 {
            // the pieces that are left out: all but the one with the most pads
            comps.sort_by_key(|(_, pads)| std::cmp::Reverse(pads.len()));
            let left: Vec<usize> = comps[1..].iter().flat_map(|(_, p)| p.iter().copied()).collect();
            out.push(PourSplit { net: net.to_string(), left, main: comps[0].1.clone() });
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

/// A pour whose outline is mostly covered by a pour of another net that comes after it on the same layer (the later one wins where
/// they overlap): the copper the outline promises is not there, and the vias and pads that were counting on it join nothing.
fn covered_pour_checks(board: &Board) -> Vec<Violation> {
    crate::zonefill::pours_covered_by_later(board)
        .into_iter()
        .filter(|c| c.lost >= COVERED_SHARE * c.area)
        .filter_map(|c| {
            let z = &board.zones[c.zone];
            let name = if z.net.is_empty() { "ネットなし".to_string() } else { z.net.clone() };
            let at = z.region().first().map(|s| s.outer[0])?;
            Some(v(
                format!(
                    "{}のゾーン ({name}) の {:.0}% ({:.1} / {:.1} mm²) が、あとから置いたゾーン ({}) に上書きされています。輪郭どおりの銅は残らないので、ビアやパッドをつないでも放熱や電流の道になりません。上書きしているゾーンの形を変える (その部分をくり抜く)、このゾーンを後に置く、のどちらかにしましょう",
                    side_name(z.side),
                    100.0 * c.lost / c.area,
                    c.lost,
                    c.area,
                    c.by.join("・")
                ),
                at,
            ))
        })
        .collect()
}
