use super::*;

/// Farther than this (mm) from an IC supply pin, a bypass capacitor stops doing its job: the
/// trace between them adds inductance that the fast current spikes cannot get through.
pub const MAX_DECOUPLING_DISTANCE: f64 = 8.0;

pub use design_spec::netnames::{is_ground_net, is_negative_supply_net, is_supply_net};

/// ICs (refdes `U…` with 5+ pads) whose supply has no bypass capacitor (a `C…` part joining the same net to ground or to another supply)
/// close by. A supply net counts once per IC: inputs tied high (a reset or preset pin on V5) are not supply pins, so
/// the nearest capacitor to any pad of the IC on that net is what counts. Skipped when the board has no capacitor on
/// that net at all and no ground net, so a half-finished board is not nagged.
pub fn decoupling_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for part in board.parts.iter().filter(|p| p.refdes.starts_with('U')) {
        let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
        if pads.len() < 5 {
            continue;
        }
        let mut nets: Vec<&str> = pads.iter().filter(|p| is_supply_net(&p.net) || is_negative_supply_net(&p.net)).map(|p| p.net.as_str()).collect();
        nets.sort_unstable();
        nets.dedup();
        for net in nets {
            let on_net: Vec<&&Pad> = pads.iter().filter(|p| p.net == net).collect();
            // the capacitors that bridge this supply net and a ground net: the pair (IC pad, capacitor pad) that is closest
            let mut nearest: Option<(f64, &Pad)> = None;
            for c in board.parts.iter().filter(|c| c.refdes.starts_with('C')) {
                let cp: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == c.refdes).collect();
                // the other side of the capacitor is the return: a ground, or another supply (a single-supply IC whose 0 V is the negative rail
                // has its capacitor between VCC and that rail)
                if !cp.iter().any(|p| p.net != net && (is_ground_net(&p.net) || is_supply_net(&p.net) || is_negative_supply_net(&p.net))) {
                    continue;
                }
                for p in cp.iter().filter(|p| p.net == net) {
                    for sp in &on_net {
                        let d = ((p.pos.x - sp.pos.x).powi(2) + (p.pos.y - sp.pos.y).powi(2)).sqrt();
                        if nearest.is_none_or(|(n, _)| d < n) {
                            nearest = Some((d, **sp));
                        }
                    }
                }
            }
            // reported at the IC's pad that is nearest to a capacitor (or its first pad on the net when there is none)
            let sp = nearest.map_or(*on_net[0], |(_, sp)| sp);
            let at = sp.pos;
            let pin = format!("{}.{} ({})", part.refdes, sp.pin, sp.net);
            match nearest.map(|(d, _)| d) {
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
