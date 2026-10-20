use super::*;

/// Farther than this (mm) from an IC supply pin, a bypass capacitor stops doing its job: the
/// trace between them adds inductance that the fast current spikes cannot get through.
pub const MAX_DECOUPLING_DISTANCE: f64 = 8.0;

pub fn is_ground_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n.starts_with("GND") || n.ends_with("GND") || n == "VSS" || n == "0"
}

pub fn is_supply_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    // "V5", "V12", "V3V3" (a V and a digit), as well as "3V3" / "5V" and the VCC family
    let v_digit = n.strip_prefix('V').is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
    n.starts_with("VCC") || n.starts_with("VDD") || n.starts_with("VBUS") || n.starts_with('+') || n == "V+" || v_digit || (n.contains('V') && n.chars().next().is_some_and(|c| c.is_ascii_digit()))
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
