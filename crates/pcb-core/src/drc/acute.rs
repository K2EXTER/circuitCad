use super::*;

/// Where two tracks of the same layer meet at an angle sharper than 90 degrees. The wedge of
/// copper-free space between them traps etchant ("acid trap") and the corner can over-etch; it also
/// makes a fast signal reflect. Use 45-degree bends (an interior angle of 135 degrees) instead.
/// A corner that lies on a pad or a via is not reported: the pad's copper fills the point of the wedge
/// (several tracks meeting under a part's pad is how a pad is normally wired).
pub fn acute_angle_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    let tracks = &board.tracks;
    let covered = |p: Point, side: Side| {
        board.pads.iter().any(|pd| {
            (pd.drill.is_some() || pd.side == side)
                && match pd.shape {
                    PadShape::Circle { d } => p.dist(pd.pos) <= d / 2.0 + 1e-6,
                    PadShape::Rect { w, h } | PadShape::Oval { w, h } => (p.x - pd.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - pd.pos.y).abs() <= h / 2.0 + 1e-6,
                }
        }) || board.vias.iter().any(|via| p.dist(via.pos) <= via.diameter / 2.0 + 1e-6)
    };
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
                if covered(p, t.side) {
                    continue;
                }
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
