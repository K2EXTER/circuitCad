use super::*;

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

/// A board with ground pads but no ground copper pour: worth a hint on a 2-layer board, where a
/// ground fill gives every signal a short return path and cuts noise.
pub fn missing_ground_fill(board: &Board) -> bool {
    let has_ground = board.pads.iter().any(|p| is_ground_net(&p.net));
    let has_fill = board.zones.iter().any(|z| is_ground_net(&z.net));
    has_ground && !has_fill && board.pads.iter().filter(|p| !p.net.is_empty()).count() >= 4
}
