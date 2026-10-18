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

/// The text of a SPICE file: UTF-8 when it is, otherwise Latin-1 / Windows-1252 (vendor files write the micro sign and
/// the ohm sign as single bytes, which would turn `10µ` into an unreadable number).
pub fn decode_spice_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| if b == 0xB5 { 'u' } else { b as char }).collect(),
    }
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
        // `.params` is accepted by LTspice as `.param`
        let line = match line.get(..8) {
            Some(h) if h.eq_ignore_ascii_case(".params ") => format!(".param {}", &line[8..]),
            _ => line.to_string(),
        };
        out.push(squash_equals(&space_before_paren(&line)));
    }
    out
}

/// `POLY (2)` and `TABLE (` are written with a blank before the parenthesis by some vendors.
fn space_before_paren(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let low = line.to_ascii_lowercase();
    let mut i = 0;
    while i < line.len() {
        let rest = &low[i..];
        // `POLY 3 a b ...` (no parentheses): the count is the number after the word
        if rest.starts_with("poly") && rest[4..].starts_with(char::is_whitespace) && !line[..i].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            let after = rest[4..].trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && after[digits.len()..].starts_with(char::is_whitespace) {
                out.push_str(&format!("{}({digits})", &line[i..i + 4]));
                i += 4 + (rest[4..].len() - after.len()) + digits.len();
                continue;
            }
        }
        let word = ["poly", "table"].iter().find(|w| rest.starts_with(**w) && rest[w.len()..].starts_with(char::is_whitespace));
        match word {
            Some(w) if rest[w.len()..].trim_start().starts_with('(') && !line[..i].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') => {
                out.push_str(&line[i..i + w.len()]);
                i += w.len() + (rest[w.len()..].len() - rest[w.len()..].trim_start().len());
            }
            _ => {
                let ch = line[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// `KP= {X}` and `W = 1u` become `KP={X}` and `W=1u` (vendors put blanks around the `=` of a parameter); comparison
/// operators (`==`, `<=`, `>=`, `!=`) are left alone.
fn squash_equals(line: &str) -> String {
    let ch: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        if c.is_whitespace() {
            let mut j = i;
            while j < ch.len() && ch[j].is_whitespace() {
                j += 1;
            }
            let single_eq = |k: usize| ch.get(k) == Some(&'=') && ch.get(k + 1) != Some(&'=') && !matches!(out.chars().last(), Some('<' | '>' | '!' | '='));
            if !single_eq(j) {
                out.push(' ');
            }
            i = j;
        } else if c == '=' && ch.get(i + 1) != Some(&'=') && !matches!(out.chars().last(), Some('<' | '>' | '!' | '=')) {
            out.push('=');
            i += 1;
            while i < ch.len() && ch[i].is_whitespace() {
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `NAME(K=V K=V)` / `NAME K=V K=V` parameter list.
fn params(s: &str) -> Result<HashMap<String, f64>, String> {
    let cleaned = s.replace(['(', ')', ','], " ");
    let mut m = HashMap::new();
    for tok in cleaned.split_whitespace() {
        // a bare word is a flag (`noiseless`): not used
        let Some((k, val)) = tok.split_once('=') else { continue };
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
            Model::Diode(DiodeModel {
                is: get("IS", 1e-14),
                n: get("N", 1.0),
                rs: get("RS", 0.0),
                bv: get("BV", f64::INFINITY),
                ibv: get("IBV", 1e-3),
                cj0: p.get("CJO").or(p.get("CJ0")).or(p.get("CJ")).copied().filter(|v| *v > 0.0).unwrap_or(0.0),
                vj: p.get("VJ").copied().filter(|v| *v > 0.0).unwrap_or(1.0),
                m: p.get("M").copied().filter(|v| *v > 0.0 && *v < 0.95).unwrap_or(0.5),
                fc: get("FC", 0.5).clamp(0.0, 0.95),
                tt: get("TT", 0.0),
                isr: get("ISR", 0.0),
                nr: get("NR", 2.0),
                eg: p.get("EG").copied().filter(|v| *v > 0.0).unwrap_or(1.11),
                xti: get("XTI", 3.0),
            }),
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
                cgso: get("CGSO", 0.0),
                cgdo: get("CGDO", 0.0),
                jfet: false,
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
                ise: p.get("ISE").or(p.get("C2")).copied().filter(|v| *v > 0.0).unwrap_or(0.0),
                ne: p.get("NE").copied().filter(|v| *v > 0.0).unwrap_or(1.5),
                isc: p.get("ISC").or(p.get("C4")).copied().filter(|v| *v > 0.0).unwrap_or(0.0),
                nc: p.get("NC").copied().filter(|v| *v > 0.0).unwrap_or(2.0),
                ikf: p.get("IKF").or(p.get("IK")).copied().filter(|v| *v > 0.0).unwrap_or(f64::INFINITY),
                ikr: p.get("IKR").copied().filter(|v| *v > 0.0).unwrap_or(f64::INFINITY),
                eg: p.get("EG").copied().filter(|v| *v > 0.0).unwrap_or(1.11),
                xti: get("XTI", 3.0),
                xtb: get("XTB", 1.5),
                pnp: t != "NPN",
            }),
        )),
        // junction FET (level 1): the same square law as the MOSFET with kp = 2*beta; the pinch-off is always negative in
        // the n-channel orientation the solver works in
        t @ ("NJF" | "PJF") => Ok((
            name,
            Model::Mos(MosModel {
                kp: 2.0 * get("BETA", 1e-4),
                vto: -get("VTO", 2.0).abs(),
                lambda: get("LAMBDA", 0.0),
                pmos: t == "PJF",
                cgs: get("CGS", 0.0),
                cgd: get("CGD", 0.0),
                cgso: 0.0,
                cgdo: 0.0,
                jfet: true,
            }),
        )),
        // a power MOSFET (ngspice / LTspice VDMOS): the square law with the whole-device Kp; the series resistances and the
        // body diode of the card are not modelled
        "VDMOS" => Ok((
            name,
            Model::Mos(MosModel {
                kp: get("KP", 1.0),
                vto: get("VTO", 0.0),
                lambda: get("LAMBDA", 0.0),
                pmos: plist.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == "pchan"),
                cgs: get("CGS", 0.0),
                cgd: p.get("CGDMAX").or(p.get("CGD")).copied().unwrap_or(0.0),
                cgso: 0.0,
                cgdo: 0.0,
                jfet: false,
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

/// Like `parse_models`, but a `.model` card that cannot be read does not stop the others: a library file holds
/// dozens of unrelated cards, and only the ones an element uses matter. The unreadable ones come back with their error.
pub fn parse_models_lenient(text: &str) -> (HashMap<String, Model>, HashMap<String, String>) {
    let (mut m, mut bad) = (HashMap::new(), HashMap::new());
    for l in logical_lines(text) {
        if l.to_ascii_lowercase().starts_with(".model") {
            match parse_model(&l) {
                Ok((n, model)) => {
                    m.insert(n.to_ascii_lowercase(), model);
                }
                Err(e) if e.contains("未対応のモデル種別") => {}
                Err(e) => {
                    if let Some(n) = l.split_whitespace().nth(1) {
                        bad.insert(n.to_ascii_lowercase(), e);
                    }
                }
            }
        }
    }
    (m, bad)
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

/// `(W, L, M)` of a MOSFET line (`W=`, `L=`, `M=` words; W and L default to 100 um, M to 1).
fn mos_dims(words: &[&str]) -> (f64, f64, f64) {
    let find = |key: &str| words.iter().find_map(|w| w.to_ascii_uppercase().strip_prefix(key).and_then(parse_value)).filter(|v| *v > 0.0);
    (find("W=").unwrap_or(100e-6), find("L=").unwrap_or(100e-6), find("M=").unwrap_or(1.0))
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
            "D" => &["IS", "N", "RS", "BV", "IBV", "CJO", "CJ0", "CJ", "VJ", "M", "FC", "TT", "ISR", "NR", "EG", "XTI"],
            "NPN" | "PNP" | "LPNP" => &["IS", "BF", "BR", "NF", "NR", "RB", "RC", "RE", "VAF", "VA", "VAR", "VB", "CJE", "VJE", "PE", "MJE", "ME", "CJC", "VJC", "PC", "MJC", "MC", "FC", "TF", "TR", "ISE", "C2", "NE", "ISC", "C4", "NC", "IKF", "IK", "IKR", "EG", "XTI", "XTB"],
            "NMOS" | "PMOS" => &["KP", "VTO", "LAMBDA", "CGS", "CGD", "CGSO", "CGDO"],
            "NJF" | "PJF" => &["BETA", "VTO", "LAMBDA", "CGS", "CGD"],
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
    let (models, bad_models) = parse_models_lenient(text);
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
        .take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:") && !w.eq_ignore_ascii_case("optional:"))
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
                // `R1 a b MODEL 1k` (LTspice / PSpice model form): the value is the word after the model name
                let vi = if parse_value(t[3]).is_none() && t.len() >= 5 && parse_value(t[4]).is_some() { 4 } else { 3 };
                let (a, b, x) = (local(c, t[1]), local(c, t[2]), val(t[vi])?);
                if head.starts_with(['C', 'c']) {
                    // initial voltage: C1 a b 100u IC=3.2
                    if let Some(ic) = t[vi + 1..].iter().find_map(|w| w.to_ascii_uppercase().strip_prefix("IC=").map(str::to_string)) {
                        c.set_initial_voltage(&name, val(&ic)?);
                    }
                }
                // temperature coefficients: TC=a,b  or  TC1=a TC2=b (1/°C, 1/°C²)
                let (mut tc1, mut tc2) = (0.0, 0.0);
                for w in t[vi + 1..].iter() {
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
                    let find = |key: &str| t[vi + 1..].iter().find_map(|w| w.to_ascii_uppercase().strip_prefix(key).and_then(parse_value));
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
            'Z' => {
                // internal line written by the flattener: Z<name> kind q qb vlow vhigh vt vh set_low reset_low in...
                need(11)?;
                let kind = match t[1] {
                    "dff" => crate::SeqKind::Dff,
                    "tff" => crate::SeqKind::Tff,
                    "jkff" => crate::SeqKind::Jkff,
                    "srff" => crate::SeqKind::Srff,
                    "dlatch" => crate::SeqKind::Dlatch,
                    "srlatch" => crate::SeqKind::Srlatch,
                    "schmitt" => crate::SeqKind::Schmitt,
                    other => return Err(format!("{name}: 記憶つき部品の種類 {other} が分かりません")),
                };
                let (q, qb) = (local(c, t[2]), local(c, t[3]));
                let (vlow, vhigh, vt, vh) = (val(t[4])?, val(t[5])?, val(t[6])?, val(t[7])?);
                let levels = crate::SeqLevels { vlow, vhigh, vt, vh, set_low: t[8] == "1", reset_low: t[9] == "1" };
                let ins: Vec<usize> = t[10..].iter().map(|n| local(c, n)).collect();
                c.add(Element::Seq { name, kind, ins, q, qb, levels });
            }
            'K' => {
                // K name L1 L2 k  (two coupled inductors; the names are those of this subcircuit)
                need(4)?;
                if t.len() > 4 && parse_value(t[4]).is_none() && !t[4].contains('=') {
                    return Err(format!("{name}: 3 つ以上のインダクタの結合 (K) は未対応です。2 つずつの K に分けてください"));
                }
                let k = val(t[3])?;
                if !(k > 0.0 && k <= 1.0) {
                    return Err(format!("{name}: 結合係数 {k} は 0 より大きく 1 以下にしてください"));
                }
                c.add(Element::Mutual { name, l1: format!("{inst}.{}", t[1]), l2: format!("{inst}.{}", t[2]), k });
            }
            'V' | 'I' => {
                need(3)?;
                let (p, n) = (local(c, t[1]), local(c, t[2]));
                // `Vsense a b` with no value is a 0 V source (a current probe)
                let src = parse_source_full(&if t.len() > 3 { t[3..].join(" ") } else { "0".to_string() }).map_err(|e| format!("{name}: {e}"))?;
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
                        m.cj0 *= area;
                        c.add(Element::Diode { name, a, k, model: m })
                    }
                    _ => return Err(bad_models.get(&t[3].to_ascii_lowercase()).map_or_else(|| format!("{name}: ダイオードモデル {} が見つかりません", t[3]), |e| format!("{name}: モデル {} を読めません: {e}", t[3]))),
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
                    _ => return Err(bad_models.get(&t[mi].to_ascii_lowercase()).map_or_else(|| format!("{name}: トランジスタモデル {} が見つかりません (JFET や VDMOS など、未対応の種類のモデルかもしれません)", t[mi]), |e| format!("{name}: モデル {} を読めません: {e}", t[mi]))),
                }
            }
            'J' => {
                need(5)?;
                let (d, g, s) = (local(c, t[1]), local(c, t[2]), local(c, t[3]));
                match models.get(&t[4].to_ascii_lowercase()) {
                    Some(Model::Mos(m)) if m.jfet => {
                        let mut m = m.clone();
                        m.kp *= area_of(&t[5..]);
                        c.add_mosfet(&name, d, g, s, m);
                    }
                    _ => return Err(bad_models.get(&t[4].to_ascii_lowercase()).map_or_else(|| format!("{name}: JFET モデル {} が見つかりません (NJF / PJF の .model が要ります)", t[4]), |e| format!("{name}: モデル {} を読めません: {e}", t[4]))),
                }
            }
            'M' => {
                need(5)?;
                let (d, g, s) = (local(c, t[1]), local(c, t[2]), local(c, t[3]));
                // D G S model (no bulk: power MOSFET cards) or D G S B model
                let mi = if models.contains_key(&t[4].to_ascii_lowercase()) || bad_models.contains_key(&t[4].to_ascii_lowercase()) || t.len() < 6 { 4 } else { 5 };
                match models.get(&t[mi].to_ascii_lowercase()) {
                    Some(Model::Mos(m)) => {
                        // level-1 transconductance is KP * W / L (and M parallel devices); W and L default to 100 um
                        let mut m = m.clone();
                        let (w, l, mult) = mos_dims(&t[(mi + 1).min(t.len())..]);
                        m.kp *= mult * w / l;
                        // the overlap capacitances per metre of width become fixed gate capacitances
                        m.cgs = (m.cgs + m.cgso * w) * mult;
                        m.cgd = (m.cgd + m.cgdo * w) * mult;
                        c.add_mosfet(&name, d, g, s, m);
                    }
                    _ => return Err(bad_models.get(&t[mi].to_ascii_lowercase()).map_or_else(|| format!("{name}: MOSFETモデル {} が見つかりません", t[mi]), |e| format!("{name}: モデル {} を読めません: {e}", t[mi]))),
                }
            }
            'S' => {
                need(6)?;
                let (a, b, cp, cn) = (local(c, t[1]), local(c, t[2]), local(c, t[3]), local(c, t[4]));
                match models.get(&t[5].to_ascii_lowercase()) {
                    Some(Model::Switch(m)) => c.add(Element::Switch { name, a, b, cp, cn, model: m.clone() }),
                    _ => return Err(bad_models.get(&t[5].to_ascii_lowercase()).map_or_else(|| format!("{name}: スイッチモデル {} が見つかりません", t[5]), |e| format!("{name}: モデル {} を読めません: {e}", t[5]))),
                }
            }
            'B' => {
                need(4)?;
                let (p, n) = (local(c, t[1]), local(c, t[2]));
                // B1 out 0 V=expression   or   I=expression (the expression may contain spaces)
                // options after the expression (`Rpar=25k Cpar=50f tripdt=30n`): Rpar and Cpar are a resistor and a capacitor
                // across the source (a node fed only by a current source would float without Rpar); the others are not used
                let mut words: Vec<&str> = t[3..].to_vec();
                let mut opts: Vec<&str> = vec![];
                while words.len() > 1 && words.last().is_some_and(|w| w.contains('=') && !w.contains(['<', '>', '(', ')', '+', '*', '/']) && w.chars().next().is_some_and(|c| c.is_ascii_alphabetic())) {
                    opts.extend(words.pop());
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
                let opt = |key: &str| opts.iter().find_map(|w| w.to_ascii_uppercase().strip_prefix(key).and_then(parse_value)).filter(|v| *v > 0.0);
                if let Some(r) = opt("RPAR=") {
                    c.add(Element::Resistor { name: format!("{name}.Rpar"), a: p, b: n, r });
                }
                if let Some(cp) = opt("CPAR=") {
                    c.add(Element::Capacitor { name: format!("{name}.Cpar"), a: p, b: n, c: cp });
                }
                c.add(Element::BSource { name, p, n, voltage: kind == "V", expr });
            }
            other => return Err(format!("{name}: 素子 {other} は未対応です (R C L V I E G F H D Q J M S B と X のみ。A は XSPICE の部品です)")),
        }
    }
    Err(format!(".subckt {sub_name} に .ends がありません"))
}

/// Every `I(name)` read by a B source of instance `inst` must name a V source, an inductor, an E
/// source or a voltage-type B source of the same instance.
fn check_current_refs(c: &Circuit, inst: &str) -> Result<(), String> {
    let prefix = format!("{inst}.");
    for e in c.elements.iter().filter(|e| e.name().starts_with(&prefix)) {
        if let Element::Mutual { name, l1, l2, .. } = e {
            for l in [l1, l2] {
                if !c.elements.iter().any(|x| matches!(x, Element::Inductor { name, .. } if name.eq_ignore_ascii_case(l))) {
                    return Err(format!("{name}: 結合するインダクタ {} がありません (K は同じ .subckt の中の L を 2 つ指します)", l.strip_prefix(&prefix).unwrap_or(l)));
                }
            }
        }
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
