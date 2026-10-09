//! Plain-language hints for a failed analysis: what probably went wrong and how to fix it.

use crate::circuit::{Circuit, Element};
use crate::dc::SimError;

fn find(p: &mut [usize], mut i: usize) -> usize {
    while p[i] != i {
        p[i] = p[p[i]];
        i = p[i];
    }
    i
}

/// Voltage-defined branches (sources, inductors at DC, controlled voltage sources) that close a
/// loop together: the loop's voltages cannot all be satisfied, so the matrix has no solution.
fn voltage_loop(c: &Circuit) -> Option<Vec<String>> {
    let mut p: Vec<usize> = (0..c.names.len()).collect();
    let mut members: Vec<(usize, usize, &str)> = vec![];
    for e in &c.elements {
        let (a, b) = match e {
            Element::VSource { p, n, .. } | Element::Vcvs { p, n, .. } => (*p, *n),
            Element::BSource { p, n, voltage: true, .. } => (*p, *n),
            Element::Inductor { a, b, .. } => (*a, *b),
            _ => continue,
        };
        members.push((a, b, e.name()));
        let (ra, rb) = (find(&mut p, a), find(&mut p, b));
        if ra == rb {
            let root = ra;
            let names = members.iter().filter(|(x, _, _)| find(&mut p, *x) == root).map(|(_, _, n)| n.to_string()).collect();
            return Some(names);
        }
        p[ra] = rb;
    }
    None
}

/// Suggestions for why `err` happened on `c`. Never empty for the errors it knows.
pub fn diagnose(c: &Circuit, err: &SimError) -> Vec<String> {
    let mut out = vec![];
    match err {
        SimError::NoGround => out.push("GND記号を置いて、電源のマイナス側など基準にしたい点につないでください。電圧は「GNDからいくつ高いか」で決まるので、基準がないと計算できません".into()),
        SimError::Singular => {
            match voltage_loop(c) {
                Some(names) => out.push(format!(
                    "電圧源・コイルがループになっています ({})。電源を並列につないだり、電源とコイルだけで輪を作ると、電圧が決まらず矛盾します。間に小さな抵抗を入れるか、つなぎ方を見直しましょう",
                    names.join(", ")
                )),
                None => out.push("方程式が解けません。電源を短絡していないか、部品の端子が同じネットにまとまっていないかを確認しましょう".into()),
            }
        }
        SimError::NoConvergence => {
            let has_source = c.elements.iter().any(|e| matches!(e, Element::VSource { .. } | Element::ISource { .. }));
            if !has_source {
                out.push("電源が1つもありません。電圧源か電流源を置きましょう".into());
            }
            let nonlinear = c.elements.iter().any(|e| matches!(e, Element::Diode { .. } | Element::Bjt { .. } | Element::Mosfet { .. } | Element::BSource { .. } | Element::Switch { .. }));
            if nonlinear {
                out.push("ダイオードやトランジスタなどの非線形部品は、計算を繰り返して答えを探します。向き(アノード/カソード、C/B/E)や、電流制限抵抗の有無を確認しましょう。抵抗なしで電源に直接つなぐと電流が無限大になって収束しません".into());
                out.push("発振回路や、2つの状態を持つ回路(555・フリップフロップなど)は、DCでは1点に定まらないことがあります。「過渡解析」で時間を追って見てください".into());
            } else {
                out.push("部品の値が極端に大きい/小さくないか(0Ωや1TΩなど)を確認しましょう".into());
            }
        }
        SimError::Invalid(_) => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_voltage_sources_are_named() {
        let mut c = Circuit::default();
        let a = c.node("a");
        c.elements.push(Element::vdc("V1", a, 0, 5.0));
        c.elements.push(Element::vdc("V2", a, 0, 3.0));
        let h = diagnose(&c, &SimError::Singular);
        assert!(h[0].contains("V1") && h[0].contains("V2"), "{h:?}");
    }

    #[test]
    fn no_ground_and_no_source_have_advice() {
        let c = Circuit::default();
        assert!(diagnose(&c, &SimError::NoGround)[0].contains("GND"));
        assert!(diagnose(&c, &SimError::NoConvergence).iter().any(|m| m.contains("電源が1つもありません")));
    }
}
