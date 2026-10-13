use super::*;

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
