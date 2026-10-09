//! Reading the SPICE text that library parts carry: `.model` cards and flat `.subckt` bodies.
//! Supported elements: R C L V I E G D Q M S. Anything else is reported, never silently ignored.

use crate::circuit::{BjtModel, Circuit, DiodeModel, Element, MosModel, SwitchModel};
use crate::value::parse_value;
use crate::waveform::parse_source_full;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Model {
    Diode(DiodeModel),
    Bjt(BjtModel),
    Mos(MosModel),
    Switch(SwitchModel),
}

/// Join continuation lines (`+`), drop comments and blank lines.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            if let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(rest.trim());
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

/// `NAME(K=V K=V)` / `NAME K=V K=V` parameter list.
fn params(s: &str) -> Result<HashMap<String, f64>, String> {
    let cleaned = s.replace(['(', ')', ','], " ");
    let mut m = HashMap::new();
    for tok in cleaned.split_whitespace() {
        let (k, val) = tok.split_once('=').ok_or_else(|| format!("パラメータ {tok} が K=V の形式ではありません"))?;
        let x = parse_value(val).ok_or_else(|| format!("パラメータ値 {val} を数値にできません"))?;
        m.insert(k.to_ascii_uppercase(), x);
    }
    Ok(m)
}

/// Parse one `.model NAME TYPE(params)` line.
pub fn parse_model(line: &str) -> Result<(String, Model), String> {
    let mut it = line.split_whitespace();
    it.next(); // .model
    let name = it.next().ok_or(".model に名前がありません")?.to_string();
    let rest_all: Vec<&str> = it.collect();
    let rest = rest_all.join(" ");
    let (ty, plist) = match rest.find('(') {
        Some(i) => (rest[..i].trim().to_string(), rest[i..].to_string()),
        None => {
            let mut sp = rest.splitn(2, ' ');
            (sp.next().unwrap_or("").to_string(), sp.next().unwrap_or("").to_string())
        }
    };
    let p = params(&plist)?;
    let get = |k: &str, d: f64| p.get(k).copied().unwrap_or(d);
    match ty.to_ascii_uppercase().as_str() {
        "D" => Ok((
            name,
            Model::Diode(DiodeModel { is: get("IS", 1e-14), n: get("N", 1.0), rs: get("RS", 0.0), bv: get("BV", f64::INFINITY), ibv: get("IBV", 1e-3) }),
        )),
        t @ ("NMOS" | "PMOS") => Ok((
            name,
            Model::Mos(MosModel {
                kp: get("KP", 2e-5),
                vto: get("VTO", if t == "NMOS" { 1.0 } else { -1.0 }),
                lambda: get("LAMBDA", 0.0),
                pmos: t == "PMOS",
                cgs: get("CGS", 0.0),
                cgd: get("CGD", 0.0),
            }),
        )),
        t @ ("NPN" | "PNP") => Ok((
            name,
            Model::Bjt(BjtModel {
                is: get("IS", 1e-16),
                bf: get("BF", 100.0),
                br: get("BR", 1.0),
                nf: get("NF", 1.0),
                nr: get("NR", 1.0),
                rb: get("RB", 0.0),
                pnp: t == "PNP",
            }),
        )),
        "SW" => {
            let d = SwitchModel::default();
            Ok((name, Model::Switch(SwitchModel { ron: get("RON", d.ron), roff: get("ROFF", d.roff), vt: get("VT", d.vt), vw: get("VW", d.vw) })))
        }
        other => Err(format!("未対応のモデル種別 {other} (D / NPN / PNP / NMOS / PMOS / SW のみ)")),
    }
}

/// Collect every `.model` line in a text block.
pub fn parse_models(text: &str) -> Result<HashMap<String, Model>, String> {
    let mut m = HashMap::new();
    for l in logical_lines(text) {
        if l.to_ascii_lowercase().starts_with(".model") {
            let (n, model) = parse_model(&l)?;
            m.insert(n.to_ascii_lowercase(), model);
        }
    }
    Ok(m)
}

/// Instantiate one `.model` part (a bare diode/transistor model) on `nodes`.
pub fn add_model_instance(
    c: &mut Circuit,
    inst: &str,
    element: &str,
    model_name: &str,
    text: &str,
    nodes: &[usize],
) -> Result<(), String> {
    let models = parse_models(text)?;
    let m = models.get(&model_name.to_ascii_lowercase()).ok_or_else(|| format!("モデル {model_name} が見つかりません"))?;
    match (element.to_ascii_uppercase().as_str(), m) {
        ("D", Model::Diode(dm)) if nodes.len() == 2 => {
            c.add(Element::Diode { name: inst.into(), a: nodes[0], k: nodes[1], model: dm.clone() });
            Ok(())
        }
        ("Q", Model::Bjt(bm)) if nodes.len() == 3 => {
            c.add(Element::Bjt { name: inst.into(), c: nodes[0], b: nodes[1], e: nodes[2], model: bm.clone() });
            Ok(())
        }
        ("M", Model::Mos(mm)) if nodes.len() >= 3 => {
            // drain, gate, source (a fourth bulk node, if any, is ignored: bulk is tied to the source)
            c.add_mosfet(inst, nodes[0], nodes[1], nodes[2], mm.clone());
            Ok(())
        }
        ("S", Model::Switch(sm)) if nodes.len() == 4 => {
            // switch terminals a, b, then the control pair (+, -)
            c.add(Element::Switch { name: inst.into(), a: nodes[0], b: nodes[1], cp: nodes[2], cn: nodes[3], model: sm.clone() });
            Ok(())
        }
        (e, _) => Err(format!("{inst}: 素子記号 {e} とモデル {model_name} の組み合わせ/ピン数が未対応です")),
    }
}

/// Instantiate a `.subckt` on `nodes` (ordered like the subcircuit's ports). Internal nodes and
/// element names are prefixed with `inst.`.
pub fn add_subckt_instance(c: &mut Circuit, inst: &str, sub_name: &str, text: &str, nodes: &[usize]) -> Result<(), String> {
    let lines = logical_lines(text);
    let models = parse_models(text)?;
    let start = lines
        .iter()
        .position(|l| {
            let mut t = l.split_whitespace();
            t.next().is_some_and(|d| d.eq_ignore_ascii_case(".subckt")) && t.next().is_some_and(|n| n.eq_ignore_ascii_case(sub_name))
        })
        .ok_or_else(|| format!(".subckt {sub_name} が見つかりません"))?;
    let ports: Vec<&str> = lines[start]
        .split_whitespace()
        .skip(2)
        .take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:"))
        .collect();
    if ports.len() != nodes.len() {
        return Err(format!("{inst}: ポート数 {} と接続数 {} が一致しません", ports.len(), nodes.len()));
    }
    let port_node: HashMap<String, usize> = ports.iter().map(|p| p.to_ascii_lowercase()).zip(nodes.iter().copied()).collect();

    let local = |c: &mut Circuit, n: &str| -> usize {
        if n == "0" {
            return 0;
        }
        match port_node.get(&n.to_ascii_lowercase()) {
            Some(&i) => i,
            None => c.node(&format!("{inst}.{n}")),
        }
    };
    let val = |s: &str| parse_value(s).ok_or_else(|| format!("{inst}: 値 {s} を数値にできません"));

    for l in &lines[start + 1..] {
        let t: Vec<&str> = l.split_whitespace().collect();
        let head = t[0];
        if head.eq_ignore_ascii_case(".ends") {
            return Ok(());
        }
        if head.starts_with('.') {
            continue; // .model handled above, other dot-cards (.param, .options) ignored
        }
        let name = format!("{inst}.{head}");
        let need = |n: usize| if t.len() >= n { Ok(()) } else { Err(format!("{name}: 引数が足りません")) };
        match head.chars().next().unwrap().to_ascii_uppercase() {
            'R' | 'C' | 'L' => {
                need(4)?;
                let (a, b, x) = (local(c, t[1]), local(c, t[2]), val(t[3])?);
                c.add(match head.chars().next().unwrap().to_ascii_uppercase() {
                    'R' => Element::Resistor { name, a, b, r: x },
                    'C' => Element::Capacitor { name, a, b, c: x },
                    _ => Element::Inductor { name, a, b, l: x },
                });
            }
            'V' | 'I' => {
                need(4)?;
                let (p, n) = (local(c, t[1]), local(c, t[2]));
                let src = parse_source_full(&t[3..].join(" ")).map_err(|e| format!("{name}: {e}"))?;
                c.add(if head.to_ascii_uppercase().starts_with('V') {
                    Element::VSource { name, p, n, wave: src.wave, ac: src.ac }
                } else {
                    Element::ISource { name, from: p, to: n, wave: src.wave, ac: src.ac }
                });
            }
            'E' | 'G' => {
                need(6)?;
                let (p, n, cp, cn, g) = (local(c, t[1]), local(c, t[2]), local(c, t[3]), local(c, t[4]), val(t[5])?);
                c.add(if head.to_ascii_uppercase().starts_with('E') {
                    Element::Vcvs { name, p, n, cp, cn, gain: g }
                } else {
                    Element::Vccs { name, p, n, cp, cn, gm: g }
                });
            }
            'D' => {
                need(4)?;
                let (a, k) = (local(c, t[1]), local(c, t[2]));
                match models.get(&t[3].to_ascii_lowercase()) {
                    Some(Model::Diode(m)) => c.add(Element::Diode { name, a, k, model: m.clone() }),
                    _ => return Err(format!("{name}: ダイオードモデル {} が見つかりません", t[3])),
                }
            }
            'Q' => {
                need(5)?;
                let (cc, b, e) = (local(c, t[1]), local(c, t[2]), local(c, t[3]));
                match models.get(&t[4].to_ascii_lowercase()) {
                    Some(Model::Bjt(m)) => c.add(Element::Bjt { name, c: cc, b, e, model: m.clone() }),
                    _ => return Err(format!("{name}: トランジスタモデル {} が見つかりません", t[4])),
                }
            }
            'M' => {
                need(6)?;
                let (d, g, s) = (local(c, t[1]), local(c, t[2]), local(c, t[3]));
                match models.get(&t[5].to_ascii_lowercase()) {
                    Some(Model::Mos(m)) => c.add_mosfet(&name, d, g, s, m.clone()),
                    _ => return Err(format!("{name}: MOSFETモデル {} が見つかりません", t[5])),
                }
            }
            'S' => {
                need(6)?;
                let (a, b, cp, cn) = (local(c, t[1]), local(c, t[2]), local(c, t[3]), local(c, t[4]));
                match models.get(&t[5].to_ascii_lowercase()) {
                    Some(Model::Switch(m)) => c.add(Element::Switch { name, a, b, cp, cn, model: m.clone() }),
                    _ => return Err(format!("{name}: スイッチモデル {} が見つかりません", t[5])),
                }
            }
            other => return Err(format!("{name}: 素子 {other} は未対応です (R C L V I E G D Q M S のみ)")),
        }
    }
    Err(format!(".subckt {sub_name} に .ends がありません"))
}
