//! Reading the SPICE text that library parts carry: `.model` cards and flat `.subckt` bodies.
//! Supported elements: R C L V I E G D Q M S B. Anything else is reported, never silently ignored.

use crate::expr::Expr;
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
pub(crate) fn logical_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+')
            && let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(rest.trim());
                continue;
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
        t @ ("NPN" | "PNP" | "LPNP") => Ok((
            name,
            Model::Bjt(BjtModel {
                is: get("IS", 1e-16),
                bf: get("BF", 100.0),
                br: get("BR", 1.0),
                nf: get("NF", 1.0),
                nr: get("NR", 1.0),
                rb: get("RB", 0.0),
                rc: get("RC", 0.0),
                re: get("RE", 0.0),
                vaf: p.get("VAF").or(p.get("VA")).copied().filter(|v| *v > 0.0).unwrap_or(f64::INFINITY),
                var: p.get("VAR").or(p.get("VB")).copied().filter(|v| *v > 0.0).unwrap_or(f64::INFINITY),
                cje: get("CJE", 0.0),
                vje: p.get("VJE").or(p.get("PE")).copied().filter(|v| *v > 0.0).unwrap_or(0.75),
                mje: p.get("MJE").or(p.get("ME")).copied().filter(|v| *v > 0.0 && *v < 0.95).unwrap_or(0.33),
                cjc: get("CJC", 0.0),
                vjc: p.get("VJC").or(p.get("PC")).copied().filter(|v| *v > 0.0).unwrap_or(0.75),
                mjc: p.get("MJC").or(p.get("MC")).copied().filter(|v| *v > 0.0 && *v < 0.95).unwrap_or(0.33),
                fc: get("FC", 0.5).clamp(0.0, 0.95),
                tf: get("TF", 0.0),
                tr: get("TR", 0.0),
                pnp: t != "NPN",
            }),
        )),
        "SW" => {
            let d = SwitchModel::default();
            Ok((name, Model::Switch(SwitchModel { ron: get("RON", d.ron), roff: get("ROFF", d.roff), vt: get("VT", d.vt), vw: get("VW", d.vw) })))
        }
        // LTspice's voltage-controlled switch: Von / Voff, or Vt with the hysteresis Vh
        "VSWITCH" => {
            let d = SwitchModel::default();
            let (vt, vw) = match (p.get("VON"), p.get("VOFF")) {
                (Some(on), Some(off)) => ((on + off) / 2.0, ((on - off).abs() / 2.0).max(1e-3)),
                _ => (get("VT", d.vt), get("VH", d.vw).abs().max(1e-3)),
            };
            Ok((name, Model::Switch(SwitchModel { ron: get("RON", d.ron), roff: get("ROFF", d.roff), vt, vw })))
        }
        other => Err(format!("未対応のモデル種別 {other} (D / NPN / PNP / NMOS / PMOS / SW / VSWITCH のみ)")),
    }
}

/// Collect every `.model` line in a text block.
pub fn parse_models(text: &str) -> Result<HashMap<String, Model>, String> {
    let mut m = HashMap::new();
    for l in logical_lines(text) {
        if l.to_ascii_lowercase().starts_with(".model") {
            match parse_model(&l) {
                Ok((n, model)) => {
                    m.insert(n.to_ascii_lowercase(), model);
                }
                // a card of a kind this simulator does not know (JFET, VDMOS ...) matters only if an element uses it
                Err(e) if e.contains("未対応のモデル種別") => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(m)
}

/// The area factor of a diode / transistor line: `AREA=x` or a bare number after the model name (default 1).
fn area_of(words: &[&str]) -> f64 {
    words
        .iter()
        .find_map(|w| match w.to_ascii_uppercase().strip_prefix("AREA=") {
            Some(v) => parse_value(v),
            None => parse_value(w).filter(|_| !w.contains('=')),
        })
        .filter(|a| *a > 0.0)
        .unwrap_or(1.0)
}

/// Parameters on `.model` cards that this simulator does not use, by model name (the learner is told: the result
/// is then an approximation of the real part).
pub fn ignored_model_params(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out = vec![];
    for l in logical_lines(text) {
        let mut it = l.split_whitespace();
        if !it.next().is_some_and(|d| d.eq_ignore_ascii_case(".model")) {
            continue;
        }
        let name = it.next().unwrap_or("").to_string();
        let rest: Vec<&str> = it.collect();
        let rest = rest.join(" ");
        let (ty, plist) = match rest.find('(') {
            Some(i) => (rest[..i].trim().to_ascii_uppercase(), rest[i..].to_string()),
            None => {
                let mut sp = rest.splitn(2, ' ');
                (sp.next().unwrap_or("").to_ascii_uppercase(), sp.next().unwrap_or("").to_string())
            }
        };
        let used: &[&str] = match ty.as_str() {
            "D" => &["IS", "N", "RS", "BV", "IBV"],
            "NPN" | "PNP" | "LPNP" => &["IS", "BF", "BR", "NF", "NR", "RB", "RC", "RE", "VAF", "VA", "VAR", "VB", "CJE", "VJE", "PE", "MJE", "ME", "CJC", "VJC", "PC", "MJC", "MC", "FC", "TF", "TR"],
            "NMOS" | "PMOS" => &["KP", "VTO", "LAMBDA", "CGS", "CGD"],
            "SW" | "VSWITCH" => &["RON", "ROFF", "VT", "VW", "VH", "VON", "VOFF"],
            _ => continue,
        };
        let ignored: Vec<String> = plist
            .replace(['(', ')', ','], " ")
            .split_whitespace()
            .filter_map(|w| w.split_once('=').map(|(k, _)| k.to_ascii_uppercase()))
            .filter(|k| !used.contains(&k.as_str()))
            .collect();
        if !ignored.is_empty() {
            out.push((name, ignored));
        }
    }
    out
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
    // vendor-style text (.param, {expr}, nested X calls) is first turned into one plain .subckt
    if let Some(flat) = crate::spice_flat::flatten(text, sub_name, &lines)? {
        return add_flat_subckt(c, inst, sub_name, &flat, nodes);
    }
    add_flat_subckt(c, inst, sub_name, text, nodes)
}

fn add_flat_subckt(c: &mut Circuit, inst: &str, sub_name: &str, text: &str, nodes: &[usize]) -> Result<(), String> {
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
        if head.eq_ignore_ascii_case(".ends") || head.eq_ignore_ascii_case(".end") {
            return check_current_refs(c, inst);
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
                if head.starts_with(['C', 'c']) {
                    // initial voltage: C1 a b 100u IC=3.2
                    if let Some(ic) = t[4..].iter().find_map(|w| w.to_ascii_uppercase().strip_prefix("IC=").map(str::to_string)) {
                        c.set_initial_voltage(&name, val(&ic)?);
                    }
                }
                // temperature coefficients: TC=a,b  or  TC1=a TC2=b (1/°C, 1/°C²)
                let (mut tc1, mut tc2) = (0.0, 0.0);
                for w in t[4..].iter() {
                    let up = w.to_ascii_uppercase();
                    if let Some(v) = up.strip_prefix("TC=") {
                        let mut it = v.split(',');
                        tc1 = it.next().and_then(parse_value).unwrap_or(0.0);
                        tc2 = it.next().and_then(parse_value).unwrap_or(0.0);
                    } else if let Some(v) = up.strip_prefix("TC1=") {
                        tc1 = parse_value(v).unwrap_or(0.0);
                    } else if let Some(v) = up.strip_prefix("TC2=") {
                        tc2 = parse_value(v).unwrap_or(0.0);
                    }
                }
                if tc1 != 0.0 || tc2 != 0.0 {
                    c.set_tempco(&name, tc1, tc2);
                }
                // saturation of an inductor: ISAT=<A> LDROP=<0..1>  (the inductance has fallen by LDROP at ISAT; default 0.3)
                if head.starts_with(['L', 'l']) {
                    let find = |key: &str| t[4..].iter().find_map(|w| w.to_ascii_uppercase().strip_prefix(key).and_then(parse_value));
                    if let Some(isat) = find("ISAT=") {
                        let drop = find("LDROP=").unwrap_or(0.3);
                        if !(isat > 0.0 && drop > 0.0 && drop < 1.0) {
                            return Err(format!("{name}: ISAT は 0 より大きい電流、LDROP は 0〜1 の間 (例: 0.3 = 30% 下がる) にしてください"));
                        }
                        c.set_saturation(&name, Circuit::saturation_scale(isat, drop));
                    }
                }
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
                    Some(Model::Diode(m)) => {
                        let mut m = m.clone();
                        let area = area_of(&t[4..]);
                        m.is *= area;
                        m.rs /= area;
                        c.add(Element::Diode { name, a, k, model: m })
                    }
                    _ => return Err(format!("{name}: ダイオードモデル {} が見つかりません", t[3])),
                }
            }
            'Q' => {
                need(5)?;
                let (cc, b, e) = (local(c, t[1]), local(c, t[2]), local(c, t[3]));
                // a fourth node (substrate) is ignored: the model name is then the sixth word
                let mi = if t.len() >= 6 && !models.contains_key(&t[4].to_ascii_lowercase()) && models.contains_key(&t[5].to_ascii_lowercase()) { 5 } else { 4 };
                match models.get(&t[mi].to_ascii_lowercase()) {
                    Some(Model::Bjt(m)) => {
                        let mut m = m.clone();
                        let area = area_of(&t[mi + 1..]);
                        m.is *= area;
                        m.rb /= area;
                        c.add(Element::Bjt { name, c: cc, b, e, model: m })
                    }
                    _ => return Err(format!("{name}: トランジスタモデル {} が見つかりません (JFET や VDMOS など、未対応の種類のモデルかもしれません)", t[mi])),
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
            'B' => {
                need(4)?;
                let (p, n) = (local(c, t[1]), local(c, t[2]));
                // B1 out 0 V=expression   or   I=expression (the expression may contain spaces)
                // options after the expression (`Rpar=25k tripdt=30n`) are not used: drop them
                let mut words: Vec<&str> = t[3..].to_vec();
                while words.len() > 1 && words.last().is_some_and(|w| w.contains('=') && !w.contains(['<', '>', '(', ')', '+', '*', '/']) && w.chars().next().is_some_and(|c| c.is_ascii_alphabetic())) {
                    words.pop();
                }
                let rest = words.join(" ");
                let (kind, text) = rest
                    .split_once('=')
                    .map(|(k, e)| (k.trim().to_ascii_uppercase(), e.trim().to_string()))
                    .ok_or_else(|| format!("{name}: B 素子は「B名前 ノード+ ノード- V=式」または「I=式」の形で書いてください"))?;
                if kind != "V" && kind != "I" {
                    return Err(format!("{name}: B 素子の「{kind}=」は使えません。V=式 (電圧) か I=式 (電流) にしてください"));
                }
                let mut node_of = |nm: &str| local(c, nm);
                let branch_of = |nm: &str| format!("{inst}.{nm}");
                let expr = Expr::parse(&text, &mut node_of, &branch_of).map_err(|e| format!("{name}: {e}"))?;
                c.add(Element::BSource { name, p, n, voltage: kind == "V", expr });
            }
            other => return Err(format!("{name}: 素子 {other} は未対応です (R C L V I E G F H D Q M S B と X のみ。A は XSPICE の部品、J は JFET です)")),
        }
    }
    Err(format!(".subckt {sub_name} に .ends がありません"))
}

/// Every `I(name)` read by a B source of instance `inst` must name a V source, an inductor, an E
/// source or a voltage-type B source of the same instance.
fn check_current_refs(c: &Circuit, inst: &str) -> Result<(), String> {
    let prefix = format!("{inst}.");
    for e in c.elements.iter().filter(|e| e.name().starts_with(&prefix)) {
        let Element::BSource { name, expr, .. } = e else { continue };
        let mut refs = vec![];
        expr.branches(&mut refs);
        for r in refs {
            let ok = c.elements.iter().any(|x| {
                x.name().eq_ignore_ascii_case(&r)
                    && matches!(x, Element::VSource { .. } | Element::Inductor { .. } | Element::Vcvs { .. } | Element::BSource { voltage: true, .. })
            });
            if !ok {
                let short = r.strip_prefix(&prefix).unwrap_or(&r);
                return Err(format!("{name}: 式の I({short}) は電流を読める素子ではありません。V 電源・コイル・E 素子・V=式の B 素子の名前を書いてください (R や G、I 電源は不可。電流を読みたいときは 0V の電圧源を直列に入れます)"));
            }
        }
    }
    Ok(())
}
