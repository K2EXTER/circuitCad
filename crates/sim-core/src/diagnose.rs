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

/// The reference designators of the 555 timers in `c` (found from the names of their internal comparator nodes).
fn timers_555(c: &Circuit) -> Vec<String> {
    let mut v: Vec<String> = c
        .names
        .iter()
        .filter_map(|n| n.strip_suffix(".rcmp"))
        .filter(|u| c.names.iter().any(|n| n.strip_suffix(".scmp") == Some(*u)))
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    v
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
            if let Some(t) = crate::dc::last_convergence_trouble() {
                out.push(t);
            }
            let has_source = c.elements.iter().any(|e| matches!(e, Element::VSource { .. } | Element::ISource { .. }));
            if !has_source {
                out.push("電源が1つもありません。電圧源か電流源を置きましょう".into());
            }
            // a 555 inside (its comparators are the nodes "U1.rcmp" / "U1.scmp"): say so, the usual hints are too general
            for u in timers_555(c) {
                out.push(format!(
                    "{u} は 555 タイマーです。555 で作る点滅 (無安定) 回路は、出力が High と Low を行き来し続けるので、DC では 1 点に定まりません (これは異常ではありません)。「過渡解析を実行」で、コンデンサの充放電と出力の波形を見ましょう"
                ));
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
    fn a_555_that_has_no_operating_point_is_named() {
        let mut c = Circuit::default();
        c.node("U3.rcmp");
        c.node("U3.scmp");
        c.node("U7.rcmp");
        let h = diagnose(&c, &SimError::NoConvergence);
        assert!(h.iter().any(|m| m.starts_with("U3 は 555") && m.contains("過渡解析")), "{h:?}");
        assert!(!h.iter().any(|m| m.starts_with("U7")), "only a 555 has both comparators: {h:?}");
    }

    #[test]
    fn no_ground_and_no_source_have_advice() {
        let c = Circuit::default();
        assert!(diagnose(&c, &SimError::NoGround)[0].contains("GND"));
        assert!(diagnose(&c, &SimError::NoConvergence).iter().any(|m| m.contains("電源が1つもありません")));
    }

    #[test]
    fn a_circuit_without_an_operating_point_names_the_node_that_would_not_settle() {
        // KCL at "a": V/1 + V^2 + 1 = 0 has no real root, so Newton's iteration can only wander
        let mut c = Circuit::default();
        let a = c.node("a");
        c.elements.push(Element::Resistor { name: "R1".into(), a, b: 0, r: 1.0 });
        let expr = crate::Expr::parse("V(a)*V(a)+1", &mut |_: &str| a, &|s: &str| s.to_string()).unwrap();
        c.elements.push(Element::BSource { name: "B1".into(), p: a, n: 0, voltage: false, expr });
        let err = crate::dc_op(&c).unwrap_err();
        assert_eq!(err, SimError::NoConvergence);
        let h = diagnose(&c, &err);
        assert!(h[0].contains("ノード a ") && (h[0].contains("発散") || h[0].contains("落ち着かなかった")), "{h:?}");
    }
}
