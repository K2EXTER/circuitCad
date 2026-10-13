//! Rewrites the behavioural-source (`B`) expressions of a part's `.subckt` into ngspice's dialect.
//!
//! Our expressions have `if(c,a,b)`, `step(x,w)`, `limit(x,lo,hi)`, `temp` and a natural `log`;
//! ngspice has `ternary_fcn`, `temper`, no `step`/`limit`, and its `log` is base 10.

/// Convert every `Bxxx ... V=<expr>` / `I=<expr>` line of a SPICE text.
pub fn convert_text(text: &str) -> String {
    text.lines().map(convert_line).collect::<Vec<_>>().join("\n") + if text.ends_with('\n') { "\n" } else { "" }
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
    fn only_b_lines_after_the_equals_sign_are_touched() {
        let t = "Bpull soc 0 I=if(time<1u,1000*(V(soc)-0.2),0)\nRtemp a b 1k\nBt t gnd V=0.01*temp\n";
        let o = convert_text(t);
        assert_eq!(o, "Bpull soc 0 I=ternary_fcn(time<1u,1000*(V(soc)-0.2),0)\nRtemp a b 1k\nBt t gnd V=0.01*temper\n");
    }
}
