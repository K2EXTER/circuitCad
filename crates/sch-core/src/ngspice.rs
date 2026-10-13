//! Rewrites a SPICE deck into ngspice's dialect.
//!
//! * Behavioural-source (`B`) expressions: ours has `if(c,a,b)`, `step(x,w)`, `limit(x,lo,hi)`,
//!   `temp` and a natural `log`; ngspice has `ternary_fcn`, `temper`, no `step`/`limit`, and its
//!   `log` is base 10.
//! * Voltage-controlled switches: ours moves smoothly between `roff` and `ron` over `vt +- vw`
//!   (that smoothness is what lets Newton's method settle latches and comparators); ngspice's `SW`
//!   model has no `vw` and switches sharply. Each `S` element becomes a `B` current source with
//!   the same smooth conductance.

use std::collections::HashMap;

/// A switch model as our engine reads it (defaults as in `SwitchModel::default`).
#[derive(Clone, Copy, Debug)]
struct Sw {
    ron: f64,
    roff: f64,
    vt: f64,
    vw: f64,
}

/// SPICE number with an engineering suffix ("1G", "4.7k", "100meg", "5m").
fn spice_number(t: &str) -> Option<f64> {
    let t = t.trim().to_ascii_lowercase();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '+' || c == '-' || c == 'e')).unwrap_or(t.len());
    // an 'e' just before the suffix ("1e" + "meg") is not an exponent
    let (num, suf) = match t[..end].parse::<f64>() {
        Ok(_) => t.split_at(end),
        Err(_) => t.split_at(t[..end].trim_end_matches('e').len()),
    };
    let v: f64 = num.parse().ok()?;
    let mult = if suf.starts_with("meg") {
        1e6
    } else {
        match suf.chars().next() {
            Some('f') => 1e-15,
            Some('p') => 1e-12,
            Some('n') => 1e-9,
            Some('u') => 1e-6,
            Some('m') => 1e-3,
            Some('k') => 1e3,
            Some('g') => 1e9,
            Some('t') => 1e12,
            _ => 1.0,
        }
    };
    Some(v * mult)
}

/// `.model NAME SW(RON=.. ROFF=.. VT=.. VW=..)` -> (lower-case name, parameters).
fn switch_model(line: &str) -> Option<(String, Sw)> {
    let mut t = line.split_whitespace();
    if !t.next()?.eq_ignore_ascii_case(".model") {
        return None;
    }
    let name = t.next()?.to_ascii_lowercase();
    let mut words = t.flat_map(|w| w.split(['(', ')', ',']).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>());
    if !words.next()?.eq_ignore_ascii_case("sw") {
        return None;
    }
    let mut m = Sw { ron: 1.0, roff: 1e9, vt: 0.0, vw: 0.01 };
    for w in words {
        let Some((k, v)) = w.split_once('=') else { continue };
        let Some(x) = spice_number(v) else { continue };
        match k.to_ascii_lowercase().as_str() {
            "ron" => m.ron = x,
            "roff" => m.roff = x,
            "vt" => m.vt = x,
            "vw" => m.vw = x,
            _ => {}
        }
    }
    Some((name, m))
}

/// `Sxxx a b cp cn MODEL` -> a `B` current source with the switch's smooth conductance.
fn switch_to_b(line: &str, models: &HashMap<String, Sw>) -> Option<String> {
    let t: Vec<&str> = line.split_whitespace().collect();
    if t.len() < 6 || !t[0].starts_with(['S', 's']) {
        return None;
    }
    let m = models.get(&t[5].to_ascii_lowercase())?;
    let (gon, goff) = (1.0 / m.ron.max(1e-9), 1.0 / m.roff.max(1e-9));
    let w = m.vw.abs().max(1e-6);
    let s = format!("min(max((V({},{})-({:e}))/{:e},0),1)", t[3], t[4], m.vt - w, 2.0 * w);
    Some(format!("B{} {} {} I=V({},{})*exp({:e}+({:e})*({s}*{s}*(3-2*{s})))", t[0], t[1], t[2], t[1], t[2], goff.ln(), gon.ln() - goff.ln()))
}

/// Convert a deck or a part's model text: switches first, then every `Bxxx ... V=<expr>` / `I=<expr>` line.
pub fn convert_text(text: &str) -> String {
    let models: HashMap<String, Sw> = text.lines().filter_map(switch_model).collect();
    let lines: Vec<String> = text
        .lines()
        .filter(|l| switch_model(l).is_none())
        .map(|l| switch_to_b(l, &models).unwrap_or_else(|| l.to_string()))
        .map(|l| convert_line(&l))
        .collect();
    lines.join("\n") + if text.ends_with('\n') { "\n" } else { "" }
}

fn convert_line(line: &str) -> String {
    if !line.trim_start().starts_with(['B', 'b']) {
        return line.to_string();
    }
    // the expression starts after the first whitespace-delimited `V=` / `I=`
    let mut pos = 0;
    for tok in line.split_inclusive(char::is_whitespace) {
        let t = tok.trim_start().to_ascii_lowercase();
        if t.starts_with("v=") || t.starts_with("i=") {
            let at = pos + (tok.len() - tok.trim_start().len()) + 2;
            return format!("{}{}", &line[..at], convert_expr(&line[at..]));
        }
        pos += tok.len();
    }
    line.to_string()
}

/// Rewrite one expression.
pub fn convert_expr(e: &str) -> String {
    let c: Vec<char> = e.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            // a number with its suffix ("4.7k", "1e-3"): copy as is
            let s = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                if (c[i] == 'e' || c[i] == 'E') && matches!(c.get(i + 1), Some('-' | '+')) {
                    i += 1;
                }
                i += 1;
            }
            out.extend(&c[s..i]);
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let s = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let name: String = c[s..i].iter().collect();
            if c.get(i) == Some(&'(') {
                let close = matching(&c, i);
                let inner: String = c[i + 1..close].iter().collect();
                out.push_str(&call(&name, &inner));
                i = close + 1;
            } else if name.eq_ignore_ascii_case("temp") {
                out.push_str("temper");
            } else {
                out.push_str(&name);
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// Index of the `)` that closes the `(` at `open` (or the end when unbalanced).
fn matching(c: &[char], open: usize) -> usize {
    let mut depth = 0;
    for (k, ch) in c.iter().enumerate().skip(open) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    c.len().saturating_sub(1)
}

fn split_args(s: &str) -> Vec<String> {
    let (mut args, mut depth, mut cur) = (vec![], 0, String::new());
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    args.push(cur);
    args
}

fn call(name: &str, inner: &str) -> String {
    let lower = name.to_ascii_lowercase();
    // node and branch references are names, not expressions
    if lower == "v" || lower == "i" {
        return format!("{name}({inner})");
    }
    let a: Vec<String> = split_args(inner).iter().map(|x| convert_expr(x.trim())).collect();
    match (lower.as_str(), a.len()) {
        ("if", 3) => format!("ternary_fcn({},{},{})", a[0], a[1], a[2]),
        ("limit", 3) => format!("min(max({},{}),{})", a[0], a[1], a[2]),
        ("log", 1) => format!("ln({})", a[0]),
        ("step", 1 | 2) => {
            // smooth 0 -> 1 across [-w, w]: s*s*(3-2s) with s = clamp((x+w)/(2w))
            let w = a.get(1).map_or("1e-3".to_string(), |w| format!("abs({w})"));
            let s = format!("min(max((({}) + {w})/(2*{w}),0),1)", a[0]);
            format!("({s}*{s}*(3-2*{s}))")
        }
        _ => format!("{name}({})", a.join(",")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_if_limit_log_and_temp() {
        assert_eq!(convert_expr("if(V(soc)>0,-I(Bv),0)"), "ternary_fcn(V(soc)>0,-I(Bv),0)");
        assert_eq!(convert_expr("limit(50*V(a,b),0.005,V(vp)-0.2)"), "min(max(50*V(a,b),0.005),V(vp)-0.2)");
        assert_eq!(convert_expr("log(2)+0.01*temp"), "ln(2)+0.01*temper");
        // names inside V()/I() stay as they are, even when they look like keywords
        assert_eq!(convert_expr("V(temp)*I(step)"), "V(temp)*I(step)");
    }

    #[test]
    fn step_becomes_a_smooth_clamped_polynomial() {
        let s = convert_expr("step(V(a)-4,0.1)");
        assert!(!s.contains("step"), "{s}");
        assert!(s.contains("min(max(((V(a)-4) + abs(0.1))/(2*abs(0.1)),0),1)"), "{s}");
        // nested calls and a default width work too
        assert!(!convert_expr("V(on)*min(0.1*(1-step(V(x),0.01)),2)").contains("step("));
        assert!(convert_expr("step(V(a))").contains("(2*1e-3)"));
    }

    #[test]
    fn spice_numbers_read_suffixes() {
        assert_eq!(spice_number("1G"), Some(1e9));
        assert_eq!(spice_number("100meg"), Some(1e8));
        assert_eq!(spice_number("4.7k"), Some(4700.0));
        assert_eq!(spice_number("5m"), Some(5e-3));
        assert_eq!(spice_number("1e-3"), Some(1e-3));
        assert_eq!(spice_number("0.005"), Some(0.005));
        assert_eq!(spice_number("x"), None);
    }

    #[test]
    fn a_switch_becomes_a_smooth_b_source_and_its_model_goes() {
        let t = "Scr vcc rcmp thres ctl SWC\nRsp a b 1k\n.model SWC SW(RON=100 ROFF=1G VT=0 VW=0.005)\n";
        let o = convert_text(t);
        assert!(!o.to_ascii_lowercase().contains(".model") && !o.starts_with("Scr"), "{o}");
        assert!(o.contains("BScr vcc rcmp I=V(vcc,rcmp)*exp("), "{o}");
        assert!(o.contains("min(max((V(thres,ctl)-(-5e-3))/1e-2,0),1)"), "{o}");
        // 1/roff and ln(gon/goff) as numbers: ln(1e-9) = -20.72..., ln(1e-2/1e-9) = 16.11...
        assert!(o.contains("exp(-2.0723") && o.contains("(1.6118"), "{o}");
        assert!(o.contains("Rsp a b 1k"));
        // a switch whose model is not in the text is left alone, and other models stay
        assert_eq!(convert_text("S1 a b c d NOPE\n.model D1 D(IS=1e-14)\n"), "S1 a b c d NOPE\n.model D1 D(IS=1e-14)\n");
    }

    #[test]
    fn only_b_lines_after_the_equals_sign_are_touched() {
        let t = "Bpull soc 0 I=if(time<1u,1000*(V(soc)-0.2),0)\nRtemp a b 1k\nBt t gnd V=0.01*temp\n";
        let o = convert_text(t);
        assert_eq!(o, "Bpull soc 0 I=ternary_fcn(time<1u,1000*(V(soc)-0.2),0)\nRtemp a b 1k\nBt t gnd V=0.01*temper\n");
    }
}
