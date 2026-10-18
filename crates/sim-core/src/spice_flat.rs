//! Flattening of vendor-style SPICE text before it is read element by element: `.param`,
//! `{expr}` / `'expr'` values, `.subckt ... params:` defaults and nested `X` calls are resolved
//! into one plain `.subckt` that `spice_in` can read. Plain text that needs none of this is not touched.

use crate::expr::Expr;
use crate::value::parse_value;
use std::cell::Cell;
use std::collections::HashMap;

type Env = HashMap<String, f64>;

struct Def {
    ports: Vec<String>,
    defaults: Vec<(String, String)>,
    body: Vec<String>,
}

const MAX_DEPTH: usize = 12;

/// Does `text` use anything this module resolves?
pub fn needs_flatten(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        let low = l.to_ascii_lowercase();
        low.starts_with(['x', 'f', 'h', 'a', 'u', 't', 'o'])
            || low.starts_with(".param")
            || low.starts_with(".func")
            || low.contains("params:")
            || l.contains('{')
            || (!l.starts_with('*') && l.contains('\''))
            || (!l.starts_with(['*', '.']) && (low.contains("poly") || low.contains("value") || low.contains("table") || low.contains("idt") || low.contains("ddt") || low.contains("tbl(")))
    })
}

/// Normalise `a = b` to `a=b` outside braces, then split on whitespace outside braces/quotes/parentheses.
fn split_top(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut norm = String::new();
    let (mut depth, mut quote) = (0i32, false);
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '{' | '(' => depth += 1,
            '}' | ')' => depth -= 1,
            '\'' => quote = !quote,
            _ => {}
        }
        if c.is_whitespace() && depth <= 0 && !quote {
            let prev = norm.trim_end().chars().last();
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if prev == Some('=') || next == Some(&'=') {
                continue;
            }
        }
        norm.push(c);
    }
    let (mut out, mut cur) = (vec![], String::new());
    let (mut depth, mut quote) = (0i32, false);
    for c in norm.chars() {
        match c {
            '{' | '(' => depth += 1,
            '}' | ')' => depth -= 1,
            '\'' => quote = !quote,
            _ => {}
        }
        if c.is_whitespace() && depth <= 0 && !quote {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Position of `word` in `text` (from `from` on) where it starts a word: `TABLE` but not `VAKTABLE`.
fn find_word(text: &str, from: usize, word: &str) -> Option<usize> {
    let mut at = from;
    while let Some(p) = text.get(at..)?.find(word) {
        let pos = at + p;
        if text[..pos].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_')) {
            return Some(pos);
        }
        at = pos + word.len();
    }
    None
}

/// Old PSpice writes the parameters of a `.subckt` header (and of an `X` call) as one braced group:
/// `.subckt CAP 1 2 {TTest=27 Cnom=.1U}`. Opens such a group into separate `name=value` words.
fn unbrace(tokens: Vec<String>) -> Vec<String> {
    tokens
        .into_iter()
        .flat_map(|t| match t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            Some(inner) if inner.contains('=') && inner.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) => split_top(inner),
            _ => vec![t],
        })
        .collect()
}

fn assignments(tokens: &[String]) -> Result<Vec<(String, String)>, String> {
    tokens
        .iter()
        .filter(|t| !t.eq_ignore_ascii_case("params:") && !t.eq_ignore_ascii_case("optional:"))
        // a bare word is a flag (`SENS`, `noiseless`); the commas some vendors put between `name=value` pairs are dropped
        .filter_map(|t| t.split_once('=').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().trim_end_matches(',').trim().to_string())))
        // `DPWR=$G_DPWR`: a global net (PSpice's digital supply), not a parameter
        .filter(|(_, v)| !v.starts_with('$'))
        // `{LEN=???}`: a placeholder for "the caller must give this one" (no default)
        .filter(|(_, v)| v != "???")
        .map(Ok)
        .collect()
}

/// Value of a constant expression with parameter names taken from `env`.
fn eval_expr(s: &str, env: &Env) -> Result<f64, String> {
    let mut s = s.trim();
    for (a, b) in [('{', '}'), ('\'', '\'')] {
        if s.len() >= 2 && s.starts_with(a) && s.ends_with(b) {
            s = s[1..s.len() - 1].trim();
        }
    }
    if let Some(v) = parse_value(s) {
        return Ok(v);
    }
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || c == '.' {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == '+' || b[j] == '-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    out.extend(&b[i..j]);
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            while i < b.len() && b[i].is_ascii_alphabetic() {
                out.push(b[i]);
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let st = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let id: String = b[st..i].iter().collect();
            let low = id.to_ascii_lowercase();
            let call = b[i..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
            if call || matches!(low.as_str(), "pi" | "temp" | "time") {
                out.push_str(&low);
            } else if let Some(v) = env.get(&low) {
                out.push_str(&format!("({v:e})"));
            } else {
                return Err(format!("パラメータ「{id}」の値が分かりません。.param で定義されているか確かめてください"));
            }
        } else {
            // braces inside an expression (`({vcc1}-0.5)*{speed1}`) only group, like parentheses
            out.push(match c {
                '{' => '(',
                '}' => ')',
                other => other,
            });
            i += 1;
        }
    }
    let bad = Cell::new(false);
    let e = Expr::parse(
        &out.to_ascii_lowercase(),
        &mut |_| {
            bad.set(true);
            0
        },
        &|n| {
            bad.set(true);
            n.to_string()
        },
    )
    .map_err(|e| format!("パラメータの式を読めません: {e}"))?;
    if bad.get() {
        return Err(format!("パラメータの式「{s}」に電圧や電流が含まれています。数値だけの式にしてください"));
    }
    let v = e.compile(&|_| None).eval(&[], 0.0).0;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("パラメータの式「{s}」の結果が数になりません"))
    }
}

/// The text of an expression with the parameter names replaced by their values; V(..) / I(..) arguments, function
/// names and names that are not parameters stay as they are.
fn substitute_params(s: &str, env: &Env) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || c == '.' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '.' || ((b[i] == '+' || b[i] == '-') && matches!(b[i - 1], 'e' | 'E') && b[..i].iter().rev().take_while(|c| c.is_ascii_alphanumeric() || **c == '.').any(|c| c.is_ascii_digit()))) {
                out.push(b[i]);
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let st = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let id: String = b[st..i].iter().collect();
            let low = id.to_ascii_lowercase();
            let call = b[i..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
            if call && matches!(low.as_str(), "v" | "i") {
                // copy the whole argument list
                let open = i + b[i..].iter().position(|c| *c == '(').unwrap_or(0);
                let close = b[open..].iter().position(|c| *c == ')').map_or(b.len() - 1, |p| open + p);
                out.extend(&b[st..=close]);
                i = close + 1;
            } else if !call && env.contains_key(&low) {
                out.push_str(&format!("({:e})", env[&low]));
            } else {
                out.push_str(&id);
            }
        } else {
            // braces inside an expression (`({vcc1}-0.5)*{speed1}`) only group, like parentheses
            out.push(match c {
                '{' => '(',
                '}' => ')',
                other => other,
            });
            i += 1;
        }
    }
    out
}

/// Replace every `{expr}` / `'expr'` in a line by its value.
fn subst_braces(line: &str, env: &Env) -> Result<String, String> {
    let b: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let (open, close) = match b[i] {
            '{' => ('{', '}'),
            '\'' => ('\'', '\''),
            c => {
                out.push(c);
                i += 1;
                continue;
            }
        };
        let (mut depth, mut j) = (0, i);
        let mut end = None;
        while j < b.len() {
            if b[j] == open && (open == '{' || j == i) {
                depth += 1;
            } else if b[j] == close {
                depth -= 1;
                if depth == 0 {
                    end = Some(j);
                    break;
                }
            }
            j += 1;
        }
        let Some(end) = end else { return Err(format!("「{line}」の {open} が閉じていません")) };
        let inner: String = b[i + 1..end].iter().collect();
        let low = inner.to_ascii_lowercase();
        if low.contains("v(") || low.contains("i(") || low.contains("time") {
            // a behavioural expression: only the parameter names are replaced
            out.push_str(&substitute_params(&inner, env));
        } else {
            out.push_str(&format!("{:e}", eval_expr(&inner, env)?));
        }
        i = end + 1;
    }
    Ok(out)
}

fn rename_node(n: &str, ports: &HashMap<String, String>, prefix: &str) -> String {
    if n == "0" {
        return "0".into();
    }
    match ports.get(&n.to_ascii_lowercase()) {
        Some(p) => p.clone(),
        None => format!("{prefix}{n}"),
    }
}

/// Rewrite `V(a)`, `V(a,b)` and `I(name)` inside a behavioural-source expression.
fn rename_expr(expr: &str, ports: &HashMap<String, String>, prefix: &str, path: &str) -> String {
    let b: Vec<char> = expr.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let prev_ident = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == '_');
        let is_call = matches!(b[i], 'v' | 'V' | 'i' | 'I') && !prev_ident && b.get(i + 1) == Some(&'(');
        if is_call && let Some(close) = b[i..].iter().position(|&c| c == ')') {
            let inner: String = b[i + 2..i + close].iter().collect();
            let args: Vec<String> = inner
                .split(',')
                .map(|a| {
                    let a = a.trim();
                    if b[i].eq_ignore_ascii_case(&'v') {
                        rename_node(a, ports, prefix)
                    } else if path.is_empty() {
                        a.to_string()
                    } else {
                        format!("{a}.{path}")
                    }
                })
                .collect();
            out.push(b[i]);
            out.push('(');
            out.push_str(&args.join(","));
            out.push(')');
            i += close + 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// A piecewise-linear curve of the expression `x` through `pts` (flat beyond the ends), as nested `if(...)`.
fn pwl_expr(x: &str, pts: &mut [(f64, f64)]) -> String {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let xs = format!("({x})");
    let mut expr = format!("({:e})", pts[pts.len() - 1].1);
    for k in (0..pts.len() - 1).rev() {
        let ((x0, y0), (x1, y1)) = (pts[k], pts[k + 1]);
        let seg = if x1 > x0 { format!("({y0:e})+({:e})*({xs}-({x0:e}))", (y1 - y0) / (x1 - x0)) } else { format!("({y0:e})") };
        expr = format!("if({xs}<({x1:e}),{seg},{expr})");
    }
    format!("if({xs}<({:e}),({:e}),{expr})", pts[0].0, pts[0].1)
}

fn node_count(letter: char) -> Option<usize> {
    Some(match letter {
        'R' | 'C' | 'L' | 'V' | 'I' | 'D' | 'B' => 2,
        'Q' | 'J' => 3,
        'E' | 'G' | 'M' | 'S' => 4,
        _ => return None,
    })
}

struct Ctx<'a> {
    defs: &'a HashMap<String, Def>,
    /// Lower-case model name -> (lower-case type, the rest of the card): to tell what an XSPICE `a` device is.
    model_kinds: &'a HashMap<String, (String, String)>,
    /// Lower-case names of the `.model` cards (to tell a transistor's substrate node from its model name).
    models: &'a std::collections::HashSet<String>,
}

/// The XSPICE model type that `gate_device` reads for a PSpice gate kind.
fn kind_to_model(kind: &str) -> &'static str {
    match kind {
        "and" => "d_and",
        "nand" => "d_nand",
        "or" => "d_or",
        "nor" => "d_nor",
        "xor" => "d_xor",
        "xnor" => "d_xnor",
        "buf" => "d_buffer",
        _ => "d_inverter",
    }
}

impl Ctx<'_> {
    /// Body lines of `def` with parameters resolved and nested calls inlined.
    fn expand(&self, def: &Def, path: &str, ports: &HashMap<String, String>, mut env: Env, call_args: &[(String, String)], depth: usize) -> Result<Vec<String>, String> {
        if depth > MAX_DEPTH {
            return Err("サブサーキットの入れ子が深すぎます (自分自身を呼んでいませんか)".into());
        }
        let outer = env.clone();
        for (k, v) in call_args {
            let val = eval_expr(v, &outer)?;
            env.insert(k.clone(), val);
        }
        // the header defaults and the subcircuit's own .param lines, in any order (a name may be used before the line that
        // defines it, and vendors write `vcc1={vcc}` in the header with `.param vcc=5` in the body)
        let mut pending: Vec<(String, String)> = def.defaults.iter().filter(|(k, _)| !call_args.iter().any(|(a, _)| a == k)).cloned().collect();
        for raw in def.body.iter().filter(|l| l.to_ascii_lowercase().starts_with(".param")) {
            for (k, v) in assignments(&split_top(&raw[6..]))? {
                if !call_args.iter().any(|(a, _)| *a == k) {
                    pending.push((k, v));
                }
            }
        }
        loop {
            let before = pending.len();
            pending.retain(|(k, v)| match eval_expr(v, &env) {
                Ok(x) => {
                    env.insert(k.clone(), x);
                    false
                }
                Err(_) => true,
            });
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
        if let Some((_, v)) = pending.first() {
            return Err(eval_expr(v, &env).unwrap_err());
        }
        let prefix = if path.is_empty() { String::new() } else { format!("{path}.") };
        let mut out = vec![];
        for raw in &def.body {
            let low = raw.to_ascii_lowercase();
            if low.starts_with(".param") {
                continue;
            }
            if low.starts_with(".model") && raw.contains(['{', '\'']) {
                // a model card with parameters: its values come from this instance (an unusable card is left out)
                if let Ok(l) = subst_braces(raw, &env) {
                    out.push(l);
                }
                continue;
            }
            if raw.starts_with('.') {
                continue;
            }
            let opened;
            let raw = if raw.trim_start().starts_with(['X', 'x']) && raw.contains("{") {
                // `X1 a b SUB {P=1 Q=2}`: the parameters of the call in one brace group
                opened = unbrace(split_top(raw)).join(" ");
                opened.as_str()
            } else {
                raw
            };
            let line = subst_braces(raw, &env)?;
            let mut t = split_top(&line);
            // the control nodes as one word, `(cp,cn)` or `cp,cn` (E / G / S): split into two
            if t.len() >= 5 && matches!(t[0].chars().next().map(|c| c.to_ascii_uppercase()), Some('E' | 'G' | 'S')) {
                let w = t[3].trim_matches(|c| c == '(' || c == ')').to_string();
                if let Some((a, b)) = w.split_once(',').filter(|(a, b)| !a.is_empty() && !b.is_empty() && !b.contains(',')) {
                    let (a, b) = (a.trim().to_string(), b.trim().to_string());
                    t.splice(3..4, [a, b]);
                }
            }
            let line = if t.len() > 0 { t.join(" ") } else { line };
            let head = t[0].as_str();
            let letter = head.chars().next().unwrap().to_ascii_uppercase();
            if letter == 'X' {
                let split = t.iter().skip(1).position(|w| w.contains('=') || w.eq_ignore_ascii_case("params:")).map_or(t.len(), |p| p + 1);
                if split < 3 {
                    return Err(format!("{head}: 「X名前 ノード… サブサーキット名」の形で書いてください"));
                }
                let sub = &t[split - 1];
                let def2 = self.defs.get(&sub.to_ascii_lowercase()).ok_or_else(|| {
                    format!("{head} が呼ぶ .subckt {sub} が見つかりません。メーカーのファイルでは別の .subckt として入っているので、その中身も貼ってください")
                })?;
                let nodes: Vec<String> = t[1..split - 1].iter().map(|n| rename_node(n, ports, &prefix)).collect();
                if nodes.len() != def2.ports.len() {
                    return Err(format!("{head}: .subckt {sub} の端子は {} 本ですが、{} 本つながっています", def2.ports.len(), nodes.len()));
                }
                let port_map = def2.ports.iter().map(|p| p.to_ascii_lowercase()).zip(nodes).collect();
                let args = assignments(&t[split..])?;
                let sub_path = if path.is_empty() { head.to_string() } else { format!("{head}.{path}") };
                out.extend(self.expand(def2, &sub_path, &port_map, env.clone(), &args, depth + 1)?);
                continue;
            }
            if letter == 'A' {
                out.extend(self.a_device(&line, &t, path, ports, &prefix)?);
                continue;
            }
            if letter == 'U' {
                out.extend(self.u_device(&t, path, ports, &prefix)?);
                continue;
            }
            if letter == 'R' && t.len() >= 4 && t[3].get(..2).is_some_and(|p| p.eq_ignore_ascii_case("r=")) {
                // `R1 a b R = {expr}`: a resistance given by an expression (may read V() / I(); `Freq` is 0 outside an AC run)
                let rest = t[3..].join(" ");
                let e = rest.split_once('=').map_or("", |(_, e)| e);
                let e = substitute_params(e, &env);
                let e = rename_expr(&e, ports, &prefix, path);
                let e = find_replace_word(&e, "freq", "0");
                let (a, b) = (rename_node(&t[1], ports, &prefix), rename_node(&t[2], ports, &prefix));
                let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                out.push(format!("B_{head}{suffix} {a} {b} I=V({a},{b})/({e})"));
                continue;
            }
            if letter == 'I' && t.len() >= 4 && line.to_ascii_lowercase().contains("tbl(") {
                // `I1 p n TBL(v0 i0 v1 i1 ...)`: a current that depends on the voltage across the source (a supply-current curve)
                let low = line.to_ascii_lowercase();
                let at = low.find("tbl(").unwrap_or(0) + 4;
                let list = &line[at..];
                let nums: Vec<f64> = list[..list.find(')').unwrap_or(list.len())].split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()).filter_map(parse_value).collect();
                if nums.len() < 4 || nums.len() % 2 != 0 {
                    return Err(format!("{head}: TBL(電圧 電流 電圧 電流 …) は、点 (電圧, 電流) を 2 つ以上並べます"));
                }
                let mut pts: Vec<(f64, f64)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
                let (a, b) = (rename_node(&t[1], ports, &prefix), rename_node(&t[2], ports, &prefix));
                let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                out.push(format!("B_{head}{suffix} {a} {b} I={}", pwl_expr(&format!("V({a},{b})"), &mut pts)));
                continue;
            }
            if letter == 'O' {
                out.extend(self.lossy_line(&t, path, ports, &prefix, &env)?);
                continue;
            }
            if letter == 'T' && t.len() == 6 && self.model_kinds.get(&t[5].to_ascii_lowercase()).is_some_and(|(ty, _)| ty == "trn" || ty == "ltra") {
                // `T1 a1 b1 a2 b2 MODEL` with `.model MODEL TRN(len= r= l= c=)` (PSpice): the lossy line
                out.extend(self.lossy_line(&t, path, ports, &prefix, &env)?);
                continue;
            }
            if letter == 'T' {
                out.extend(Self::transmission_line(&t, path, ports, &prefix)?);
                continue;
            }
            // a source whose value follows the circuit temperature (`V1 a b {temp*0.01}`): kept as an expression, so
            // that a temperature sweep moves it (`temp` is in degrees C)
            if matches!(letter, 'V' | 'I') {
                let rt = split_top(raw);
                if rt.len() == 4 && rt[3].starts_with('{') && rt[3].to_ascii_lowercase().contains("temp") {
                    let expr = substitute_params(rt[3].trim_start_matches('{').trim_end_matches('}'), &env);
                    let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                    out.push(format!("B_{head}{suffix} {} {} {}={expr}", rename_node(&rt[1], ports, &prefix), rename_node(&rt[2], ports, &prefix), letter));
                    continue;
                }
            }
            if letter == 'K' && t.len() >= 4 {
                // the inductors it couples get the same instance path as the inductors themselves
                let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                let rest: Vec<String> = t[1..].iter().map(|w| if parse_value(w).is_none() && !w.contains('=') { format!("{w}{suffix}") } else { w.clone() }).collect();
                out.push(format!("{head}{suffix} {}", rest.join(" ")));
                continue;
            }
            if matches!(letter, 'E' | 'G' | 'F' | 'H') {
                out.push(self.controlled(raw, &env, &line, &t, letter, path, ports, &prefix)?);
                continue;
            }
            let Some(mut n) = node_count(letter) else {
                out.push(line);
                continue;
            };
            let four_node_q = letter == 'Q' && t.len() >= 6 && self.models.contains(&t[5].to_ascii_lowercase()) && !self.models.contains(&t[4].to_ascii_lowercase());
            if four_node_q {
                n = 4;
            }
            if t.len() < n + 1 {
                out.push(line);
                continue;
            }
            let mut u = t.clone();
            if four_node_q {
                // the substrate node is not modelled: drop it
                u.remove(4);
            }
            if !path.is_empty() {
                u[0] = format!("{head}.{path}");
            }
            let kept = if four_node_q { 3 } else { n };
            for w in u.iter_mut().skip(1).take(kept) {
                *w = rename_node(w, ports, &prefix);
            }
            if letter == 'C' && t.get(3).is_some_and(|w| w.len() > 2 && w.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("q="))) {
                // charge-type capacitor: the expression after Q= names nodes of this subcircuit
                let rest = t[3..].join(" ");
                u.truncate(3);
                let e = rest.split_once('=').map_or("", |(_, e)| e);
                u.push(format!("Q={}", rename_expr(&substitute_params(e, &env), ports, &prefix, path)));
            }
            if letter == 'B' {
                // options after the expression (`Rpar=1 Cpar=5n tripdt=5n NoJacob`, `ymax=1e300`) stay as they are: a parameter
                // that happens to have the name of an option must not be put into it
                let mut toks: Vec<String> = t[n + 1..].to_vec();
                let mut opts: Vec<String> = vec![];
                while toks.len() > 1 && toks.last().is_some_and(|w| is_b_option(w)) {
                    opts.insert(0, toks.pop().unwrap());
                }
                let rest = toks.join(" ");
                u.truncate(n + 1);
                u.push(rename_expr(&substitute_params(&rest, &env), ports, &prefix, path));
                u.extend(opts);
            }
            out.push(u.join(" "));
        }
        Ok(extract_integrals(out))
    }
}

impl Ctx<'_> {
    /// A combinational logic gate written as an `A` device, in LTspice's form (`A in1 in2 in3 in4 vp vm out out2 AND ...`) or
    /// ngspice's XSPICE form (`a1 [in1 in2] out model` with a `d_and` ... `.model`). The gate becomes one behavioural
    /// voltage source: logic levels are `vlow`..`vhigh` (default 0 and 1 V, like LTspice), true above `vt` (their middle),
    /// with a smooth transition so that Newton's iteration has a slope to follow. Delays are not modelled; inputs tied to
    /// ground are ignored (an unused pin). `None` when the device is not a gate of these kinds.
    fn gate_device(&self, line: &str, t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str, levels: Option<(f64, f64)>) -> Result<Option<Vec<String>>, String> {
        let head = t[0].as_str();
        let gate_of = |w: &str| match w.to_ascii_lowercase().as_str() {
            "and" | "d_and" => Some("and"),
            "nand" | "d_nand" => Some("nand"),
            "or" | "d_or" => Some("or"),
            "nor" | "d_nor" => Some("nor"),
            "xor" | "d_xor" => Some("xor"),
            "xnor" | "d_xnor" => Some("xnor"),
            "buf" | "d_buffer" => Some("buf"),
            "inv" | "d_inverter" => Some("inv"),
            _ => None,
        };
        // the outputs: (node, inverted?). An LTspice device has the true output on pin 7 and the inverted one on pin 6
        // (`A a b 0 0 0 y 0 0 AND` is a NAND); the other forms have the one output they name
        let (kind, inputs, outs, params): (&str, Vec<String>, Vec<(String, bool)>, String) = if t.len() >= 10 && gate_of(&t[9]).is_some() {
            (gate_of(&t[9]).unwrap(), t[1..=5].to_vec(), vec![(t[7].clone(), false), (t[6].clone(), true)], t[10..].join(" "))
        } else {
            // XSPICE: the words after the name; the last one is the model, whose type says what the others are
            let rest = line[head.len()..].trim();
            let words: Vec<&str> = rest.split_whitespace().collect();
            let Some(model) = words.last() else { return Ok(None) };
            let Some((ty, _)) = self.model_kinds.get(&model.to_ascii_lowercase()) else { return Ok(None) };
            let kind = match ty.as_str() {
                "d_open_c" => "openc",
                "d_tristate" => "tri",
                other => match gate_of(other) {
                    Some(k) => k,
                    None => return Ok(None),
                },
            };
            let (ins, out): (Vec<String>, String) = match (kind, rest.find('['), rest.find(']')) {
                // [in1 in2 ..] out model
                (_, Some(0), Some(b)) if b > 0 => {
                    let tail: Vec<&str> = rest[b + 1..].split_whitespace().collect();
                    if tail.len() != 2 {
                        return Ok(None);
                    }
                    (rest[1..b].split_whitespace().map(str::to_string).collect(), tail[0].to_string())
                }
                ("tri", _, _) if words.len() == 4 => (vec![words[0].to_string(), words[1].to_string()], words[2].to_string()),
                (_, _, _) if words.len() == 3 && !rest.contains('[') => (vec![words[0].to_string()], words[1].to_string()),
                _ => return Ok(None),
            };
            (kind, ins, vec![(out, false)], String::new())
        };
        let param = |key: &str| params.split_whitespace().find_map(|w| w.to_ascii_lowercase().strip_prefix(&format!("{key}=")).and_then(parse_value));
        let (vlow, vhigh) = levels.unwrap_or((param("vlow").unwrap_or(0.0), param("vhigh").unwrap_or(1.0)));
        // LTspice names the switching level `Ref` (gates) or `Vt`
        let vt = param("vt").or_else(|| param("ref")).unwrap_or((vlow + vhigh) / 2.0);
        let w = ((vhigh - vlow).abs() * 0.05).max(1e-6);
        let ins: Vec<String> = inputs.iter().filter(|n| n.as_str() != "0").map(|n| rename_node(n, ports, prefix)).collect();
        let s = |n: &str| format!("(0.5+0.5*tanh((V({n})-({vt:e}))/({w:e})))");
        let terms: Vec<String> = ins.iter().map(|n| s(n)).collect();
        let and = || if terms.is_empty() { "0".to_string() } else { terms.join("*") };
        let or = || if terms.is_empty() { "0".to_string() } else { format!("(1-{})", terms.iter().map(|x| format!("(1-{x})")).collect::<Vec<_>>().join("*")) };
        let xor = || terms.iter().skip(1).fold(terms.first().cloned().unwrap_or_else(|| "0".into()), |acc, x| format!("(({acc})+({x})-2*({acc})*({x}))"));
        let logic = match kind {
            "and" => and(),
            "nand" => format!("(1-{})", and()),
            "or" => or(),
            "nor" => format!("(1-{})", or()),
            "xor" => xor(),
            "xnor" => format!("(1-{})", xor()),
            "buf" => terms.first().cloned().unwrap_or_else(|| "0".into()),
            "inv" => format!("(1-{})", terms.first().cloned().unwrap_or_else(|| "0".into())),
            _ => String::new(),
        };
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let mut lines = vec![];
        for (k, (out, inverted)) in outs.iter().filter(|(o, _)| o != "0").enumerate() {
            let outn = rename_node(out, ports, prefix);
            let nm = if k == 0 { format!("B_{head}{suffix}") } else { format!("B_{head}_{k}{suffix}") };
            // open collector: pulls the output to the low level (through 100 ohm) when the input is false, floats when true
            if kind == "openc" {
                let t0 = terms.first().cloned().unwrap_or_else(|| "0".into());
                lines.push(format!("{nm} {outn} 0 I=(1-{t0})*(V({outn})-({vlow:e}))/100"));
                continue;
            }
            // tri-state buffer: drives the input's level through 100 ohm while enabled, floats otherwise
            if kind == "tri" {
                let (d, en) = (terms.first().cloned().unwrap_or_else(|| "0".into()), terms.get(1).cloned().unwrap_or_else(|| "1".into()));
                lines.push(format!("{nm} {outn} 0 I={en}*(V({outn})-({vlow:e}+({:e})*{d}))/100", vhigh - vlow));
                continue;
            }
            let logic = if *inverted { format!("(1-{logic})") } else { logic.clone() };
            lines.push(format!("{nm} {outn} 0 V={vlow:e}+({:e})*{logic}", vhigh - vlow));
        }
        Ok(Some(lines))
    }

    /// A flip-flop, latch or Schmitt trigger written as an `A` device (LTspice `DFLOP` / `SRFLOP` / `JKFLOP` / `SCHMITT` with
    /// pins `in1 in2 clk set reset qbar q`, ngspice `d_dff` / `d_tff` / `d_jkff` / `d_srff` / `d_dlatch` / `d_srlatch`). It becomes
    /// a `Z` line (see `Element::Seq`): the stored state is updated between time steps, set / reset are active high.
    fn seq_device(&self, line: &str, t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Option<Vec<String>>, String> {
        let head = t[0].as_str();
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let nd = |n: &str| if n.eq_ignore_ascii_case("null") { "0".to_string() } else { rename_node(n, ports, prefix) };
        // (kind, input nodes in the order of SeqKind, q, qb, level parameters)
        let (kind, ins, q, qb, params): (&str, Vec<String>, String, String, String) = if t.len() >= 10 {
            let ty = t[9].to_ascii_uppercase();
            let (kind, pins): (&str, Vec<usize>) = match ty.as_str() {
                "DFLOP" => ("dff", vec![1, 3, 4, 5]),
                "SRFLOP" => ("srff", vec![1, 2, 3, 4, 5]),
                "JKFLOP" => ("jkff", vec![1, 2, 3, 4, 5]),
                "SCHMITT" => ("schmitt", vec![1]),
                _ => return Ok(None),
            };
            (kind, pins.iter().map(|p| nd(&t[*p])).collect(), nd(&t[7]), nd(&t[6]), t[10..].join(" "))
        } else {
            let rest: Vec<&str> = line[head.len()..].split_whitespace().collect();
            let Some(model) = rest.last() else { return Ok(None) };
            let Some((ty, _)) = self.model_kinds.get(&model.to_ascii_lowercase()) else { return Ok(None) };
            let (kind, n_in) = match ty.as_str() {
                "d_dff" => ("dff", 4),
                "d_tff" => ("tff", 4),
                "d_jkff" => ("jkff", 5),
                "d_srff" => ("srff", 5),
                "d_dlatch" => ("dlatch", 4),
                "d_srlatch" => ("srlatch", 5),
                _ => return Ok(None),
            };
            if rest.len() != n_in + 2 + 1 {
                return Err(format!("{head}: {ty} は「入力 {n_in} 本、出力 q と qb」の形で書きます (使わない端子は NULL)"));
            }
            (kind, rest[..n_in].iter().map(|n| nd(n)).collect(), nd(rest[n_in]), nd(rest[n_in + 1]), String::new())
        };
        let param = |key: &str| params.split_whitespace().find_map(|w| w.to_ascii_lowercase().strip_prefix(&format!("{key}=")).and_then(parse_value));
        let (vlow, vhigh) = (param("vlow").unwrap_or(0.0), param("vhigh").unwrap_or(1.0));
        let vt = param("vt").or_else(|| param("ref")).unwrap_or((vlow + vhigh) / 2.0);
        let vh = param("vh").unwrap_or(0.0).abs();
        Ok(Some(vec![format!("Z{head}{suffix} {kind} {q} {qb} {vlow:e} {vhigh:e} {vt:e} {vh:e} 0 0 {}", ins.join(" "))]))
    }

    /// A lossless transmission line `Tname a1 b1 a2 b2 Z0=50 TD=1n` (or `F=` and `NL=`, default 0.25 wavelength: `TD = NL / F`),
    /// as a ladder of `LINE_SECTIONS` L-C sections. It works in every analysis, but only below about `N / (pi * TD)` (the
    /// ladder rings and delays slightly differently from a true line); the return nodes `b1` and `b2` are joined.
    fn transmission_line(t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        if t.len() < 5 {
            return Err(format!("{head}: 伝送線路は「T名前 入力+ 入力- 出力+ 出力- Z0=特性インピーダンス TD=遅れ時間」の形で書きます"));
        }
        let key = |k: &str| t[5..].iter().find_map(|w| w.to_ascii_lowercase().strip_prefix(&format!("{k}=")).and_then(parse_value));
        let z0 = key("z0").unwrap_or(50.0);
        let td = match (key("td"), key("f")) {
            (Some(td), _) => td,
            (None, Some(f)) if f > 0.0 => key("nl").unwrap_or(0.25) / f,
            _ => return Err(format!("{head}: 伝送線路の遅れ時間が分かりません。TD=10n のように書くか、F= (周波数) と NL= (波長の何倍か) を書いてください")),
        };
        if z0 <= 0.0 || td <= 0.0 {
            return Err(format!("{head}: 伝送線路の Z0 と TD は正の値にしてください"));
        }
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let nodes = [rename_node(&t[1], ports, prefix), rename_node(&t[2], ports, prefix), rename_node(&t[3], ports, prefix), rename_node(&t[4], ports, prefix)];
        Ok(Self::ladder(head, &nodes, &suffix, prefix, z0 * td, td / z0, 0.0, 0.0))
    }

    /// A transmission line as `LINE_SECTIONS` sections of series `r` + `l` and shunt `c` + `g` (the totals over the line).
    #[allow(clippy::too_many_arguments)]
    fn ladder(head: &str, nodes: &[String; 4], suffix: &str, prefix: &str, l: f64, c: f64, r: f64, g: f64) -> Vec<String> {
        const LINE_SECTIONS: usize = 40;
        let [a1, b1, a2, b2] = nodes;
        let n = LINE_SECTIONS;
        let (l, c, r, g) = (l / n as f64, c / n as f64, r / n as f64, g / n as f64);
        let node = |i: usize| if i == 0 { a1.clone() } else if i == n { a2.clone() } else { format!("{prefix}{head}_n{i}") };
        let mut lines = Vec::new();
        for i in 0..n {
            if r > 0.0 {
                let mid = format!("{prefix}{head}_m{i}");
                lines.push(format!("R{head}_{i}{suffix} {} {mid} {r:e}", node(i)));
                lines.push(format!("L{head}_{i}{suffix} {mid} {} {l:e}", node(i + 1)));
            } else {
                lines.push(format!("L{head}_{i}{suffix} {} {} {l:e}", node(i), node(i + 1)));
            }
        }
        for i in 0..=n {
            let k = if i == 0 || i == n { 0.5 } else { 1.0 };
            lines.push(format!("C{head}_{i}{suffix} {} {b1} {:e}", node(i), c * k));
            if g > 0.0 {
                lines.push(format!("R{head}_g{i}{suffix} {} {b1} {:e}", node(i), 1.0 / (g * k)));
            }
        }
        if b1 != b2 {
            lines.push(format!("R{head}_ret{suffix} {b1} {b2} 1e-6"));
        }
        lines
    }

    /// A lossy line `O name a1 b1 a2 b2 MODEL` with `.model MODEL LTRA(R= L= C= G= LEN=)` (per unit length), as the same ladder.
    fn lossy_line(&self, t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str, env: &Env) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        if t.len() < 6 {
            return Err(format!("{head}: 損失のある線路は「O名前 入力+ 入力- 出力+ 出力- モデル名」の形で書きます"));
        }
        let (ty, rest) = self.model_kinds.get(&t[5].to_ascii_lowercase()).ok_or_else(|| format!("{head}: 線路のモデル {} が見つかりません", t[5]))?;
        if ty != "ltra" && ty != "trn" {
            return Err(format!("{head}: 線路のモデル {} は LTRA / TRN ではありません", t[5]));
        }
        let inner = rest.split_once('(').map_or(rest.as_str(), |(_, r)| r.trim_end().trim_end_matches(')'));
        let val = |k: &str| -> Result<f64, String> {
            let found = split_top(&squash_eq_text(inner)).into_iter().find_map(|w| w.split_once('=').filter(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.to_string()));
            match found {
                Some(v) => eval_expr(&v, env),
                None => Ok(0.0),
            }
        };
        let (r, l, c, g, len) = (val("r")?, val("l")?, val("c")?, val("g")?, val("len")?);
        if l <= 0.0 || c <= 0.0 || len <= 0.0 {
            return Err(format!("{head}: 線路のモデルには L と C と LEN (正の値) が必要です"));
        }
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let nodes = [rename_node(&t[1], ports, prefix), rename_node(&t[2], ports, prefix), rename_node(&t[3], ports, prefix), rename_node(&t[4], ports, prefix)];
        Ok(Self::ladder(head, &nodes, &suffix, prefix, l * len, c * len, r * len, g * len))
    }

    /// A PSpice flip-flop or latch: `U dff(n) dpwr dgnd preb clrb clk d q qb ...` (`jkff`: `... clk j k q qb`, `dltch`: `... gate d q qb`,
    /// `srff`: `... gate s r q qb`, a gated SR latch). Preset / clear are active low; the pins are repeated once per bit.
    /// `$D_HI` (inactive) and `$D_LO` (active) are the digital constants. Delays are ignored.
    fn u_seq_device(&self, t: &[String], name: &str, count: usize, path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        // (kind, data pins before q / qb, SeqKind input order as indices into [preb clrb clk a b])
        let (kind, n_dat) = match name {
            "dff" => ("dff", 1),
            "jkff" => ("jkff", 2),
            "dltch" => ("dlatch", 1),
            _ => ("srlatch", 2),
        };
        // the pins: PREBAR CLRBAR CLK/GATE, then the data pins (D, or J K, or S R) of every bit, then Q of every bit, then QBAR of every bit
        let bits = count.max(1);
        let total = 3 + n_dat * bits + 2 * bits;
        if t.len() < 4 + total {
            return Err(format!("{head}: {name}({bits}) は「電源 GND PREBAR CLRBAR {} とビットごとの Q QBAR」の形で書きます", if kind == "dff" { "CLK D…" } else if kind == "jkff" { "CLK J… K…" } else if kind == "dlatch" { "GATE D…" } else { "GATE S… R…" }));
        }
        let mut lines = Vec::new();
        let hi = format!("__hi{suffix}_{head}");
        let mut need_hi = false;
        let mut node = |n: &str| -> String {
            match n.to_ascii_lowercase().as_str() {
                "$d_hi" => {
                    need_hi = true;
                    hi.clone()
                }
                "$d_lo" => "0".to_string(),
                _ => rename_node(n, ports, prefix),
            }
        };
        let p = &t[4..4 + total];
        let (preb, clrb, clk) = (node(&p[0]), node(&p[1]), node(&p[2]));
        for b in 0..bits {
            let d = |k: usize| node(&p[3 + k * bits + b]);
            let ins_data: Vec<String> = (0..n_dat).map(d).collect();
            let q = rename_node(&p[3 + n_dat * bits + b], ports, prefix);
            let qb = rename_node(&p[3 + n_dat * bits + bits + b], ports, prefix);
            let ins = match kind {
                "dff" | "dlatch" => vec![ins_data[0].clone(), clk.clone(), preb.clone(), clrb.clone()],
                _ => vec![ins_data[0].clone(), ins_data[1].clone(), clk.clone(), preb.clone(), clrb.clone()],
            };
            let bit = if bits > 1 { format!("_{b}") } else { String::new() };
            lines.push(format!("Z{head}{bit}{suffix} {kind} {q} {qb} 0 5 2.5 0 1 1 {}", ins.join(" ")));
        }
        if need_hi {
            lines.push(format!("V{hi} {hi} 0 DC 5"));
        }
        Ok(lines)
    }

    /// A PSpice digital gate: `Uname type(n) dpwr dgnd in1 .. inN out delay-model io-model ...` (`nand(2)`, `inv`, `buf` ...). The
    /// supply pins are ignored: the levels are 0 and 5 V (true above 2.5 V), like `gate_device` otherwise. Anything with a
    /// memory (flip-flops, latches, counters) is reported.
    fn u_device(&self, t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        let ty = t.get(1).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let (name, count) = match ty.split_once('(') {
            Some((n, c)) => (n.to_string(), c.trim_end_matches(')').parse::<usize>().unwrap_or(1)),
            None => (ty.clone(), 1),
        };
        let kind = match name.as_str() {
            "and" | "nand" | "or" | "nor" | "xor" | "buf" | "inv" => name.as_str(),
            "nxor" => "xnor",
            "bufa" => "buf",
            "inva" => "inv",
            "dff" | "jkff" | "dltch" | "srff" => return self.u_seq_device(t, &name, count, path, ports, prefix),
            "pullup" | "pulldn" => {
                // `U pulldn(n) dpwr dgnd pin1 .. pinN`: a 1 k resistor from every pin to 0 V (or to 5 V for a pull-up)
                let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                let pins = t.get(4..).unwrap_or(&[]);
                let mut lines: Vec<String> = pins.iter().take(count.max(1)).enumerate().map(|(i, p)| {
                    let rail = if name == "pullup" { format!("__hi{suffix}_{head}") } else { "0".to_string() };
                    format!("R{head}_{i}{suffix} {} {rail} 1k", rename_node(p, ports, prefix))
                }).collect();
                if name == "pullup" && !lines.is_empty() {
                    lines.push(format!("V__hi{suffix}_{head} __hi{suffix}_{head} 0 DC 5"));
                }
                return Ok(lines);
            }
            _ => return Err(format!("{head}: PSpice のデジタル部品「{ty}」は未対応です (AND / OR / バッファ / インバータ / フリップフロップ / ラッチだけ読めます。カウンタなどは、アナログ回路としては表せません)")),
        };
        let n_in = if matches!(kind, "buf" | "inv") { 1 } else { count };
        if t.len() < 4 + n_in + 1 {
            return Err(format!("{head}: U 素子は「U名前 種類(入力数) 電源 GND 入力… 出力 …」の形で書きます"));
        }
        let (ins, out) = (&t[4..4 + n_in], &t[4 + n_in]);
        let line = format!("{head} [{}] {out} __gate", ins.join(" "));
        let mut tmp: HashMap<String, (String, String)> = HashMap::new();
        tmp.insert("__gate".into(), (kind_to_model(kind).into(), String::new()));
        let helper = Ctx { defs: self.defs, model_kinds: &tmp, models: self.models };
        helper.gate_device(&line, &[head.to_string()], path, ports, prefix, Some((0.0, 5.0)))?.ok_or_else(|| format!("{head}: ゲートを読めません"))
    }

    /// An LTspice `A` (XSPICE) device. Only the OTA of the "universal op-amp" is read: a transconductance with a
    /// current limit and an output capacitance. The others (SCHMITT, SRFLOP, DFLOP, AND ... have a memory or a
    /// time delay) are reported.
    fn a_device(&self, line: &str, t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        if let Some(lines) = self.gate_device(line, t, path, ports, prefix, None)? {
            return Ok(lines);
        }
        if let Some(lines) = self.seq_device(line, t, path, ports, prefix)? {
            return Ok(lines);
        }
        // `A in out MODEL` with `.model MODEL s_xfer(num_coeff=[..] den_coeff=[..])`: a Laplace transfer function
        if t.len() == 4
            && let Some((ty, rest)) = self.model_kinds.get(&t[3].to_ascii_lowercase())
            && ty == "s_xfer"
        {
            return Self::s_xfer(t, rest, path, ports, prefix);
        }
        // `A in out MODEL` with `.model MODEL PWL2 xy_array=[x0 y0 x1 y1 ...]` (the XSPICE piecewise-linear block): V(out) = f(V(in))
        if t.len() == 4
            && let Some((ty, rest)) = self.model_kinds.get(&t[3].to_ascii_lowercase())
            && (ty == "pwl2" || ty == "pwl")
            && let Some((_, arr)) = rest.split_once('[')
        {
            let nums: Vec<f64> = arr.split(']').next().unwrap_or("").split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
            if nums.len() >= 4 && nums.len() % 2 == 0 {
                let mut pts: Vec<(f64, f64)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
                let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
                let x = format!("V({})", rename_node(&t[1], ports, prefix));
                return Ok(vec![format!("B_{head}{suffix} {} 0 V={}", rename_node(&t[2], ports, prefix), pwl_expr(&x, &mut pts))]);
            }
        }
        // `A a b cp cn VSWITCHMODEL`: an ngspice-style switch written with an A name (it is an S switch)
        if t.len() == 6
            && self.model_kinds.get(&t[5].to_ascii_lowercase()).is_some_and(|(ty, _)| ty == "vswitch" || ty == "sw")
        {
            let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
            let n = |i: usize| rename_node(&t[i], ports, prefix);
            return Ok(vec![format!("S_{head}{suffix} {} {} {} {} {}", n(1), n(2), n(3), n(4), t[5])]);
        }
        // A name n1 ... n8 MODEL key=value ...
        let kind = t.get(9).map(|k| k.to_ascii_uppercase()).unwrap_or_default();
        if t.len() < 10 {
            return Err(format!("{head}: A 素子は「A名前 ノード × 8 種類 パラメータ…」の形で書きます"));
        }
        if kind == "MODULATOR" {
            return Self::modulator(t, path, ports, prefix);
        }
        if kind != "OTA" {
            return Err(format!("{head}: XSPICE の部品「{kind}」は未対応です (SCHMITT・SRFLOP・DFLOP・AND・OR のように、記憶や遅れを持つ部品は、1 つの式にできません)。回路図用の別のモデルを探すか、部品作成フォームの「かんたん作成」を使ってください"));
        }
        let param = |key: &str| t[10..].iter().find_map(|w| w.to_ascii_uppercase().strip_prefix(&format!("{key}=")).and_then(parse_value));
        let (g, vref, iout, cout) = (param("G").unwrap_or(1.0), param("REF").unwrap_or(0.0), param("IOUT").unwrap_or(1e30), param("COUT").unwrap_or(0.0));
        // the first node is the one that pulls the output down (LTspice's universal op-amps list the inverting input first)
        let (inp, inn, out) = (rename_node(&t[1], ports, prefix), rename_node(&t[2], ports, prefix), rename_node(&t[7], ports, prefix));
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let mut lines = vec![format!("B_{head}{suffix} 0 {out} I=limit(({g:e})*(V({inn},{inp})-({vref:e})),{:e},{iout:e})", -iout)];
        if cout > 0.0 {
            lines.push(format!("C_{head}{suffix} {out} 0 {cout:e}"));
        }
        Ok(lines)
    }

    /// An LTspice `MODULATOR`: `A fm am - - - - out common MODULATOR mark=Hz space=Hz`. The output is a sine of amplitude `V(am)`
    /// (1 V when the pin is grounded) whose frequency is `space + (mark - space) * V(fm)`. The phase is the integral of the
    /// frequency, kept as the voltage of a 1 F capacitor (so the frequency may change over time without a phase jump); that
    /// capacitor starts at 0 (`IC=0`), so only a transient analysis gives a meaningful output. Steps are not shortened to follow the
    /// sine: use a time step well below the period.
    fn modulator(t: &[String], path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        let param = |key: &str| t[10..].iter().find_map(|w| w.to_ascii_lowercase().strip_prefix(&format!("{key}=")).and_then(parse_value));
        let (Some(mark), space) = (param("mark"), param("space").unwrap_or(0.0)) else {
            return Err(format!("{head}: MODULATOR には mark=周波数 (と space=周波数) が必要です"));
        };
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let node = |i: usize| rename_node(&t[i], ports, prefix);
        let (fm, am, out, common) = (node(1), node(2), node(7), node(8));
        let ph = format!("{prefix}{head}_ph");
        let amp = if am == "0" { "1".to_string() } else { format!("V({am})") };
        Ok(vec![
            format!("B_{head}_f{suffix} 0 {ph} I={space:e}+({:e})*V({fm})", mark - space),
            format!("C_{head}_ph{suffix} {ph} 0 1 IC=0"),
            format!("R_{head}_ph{suffix} {ph} 0 1e9"),
            format!("B_{head}{suffix} {out} {common} V={amp}*sin(6.283185307179586*V({ph}))"),
        ])
    }

    /// XSPICE `s_xfer`: H(s) = num(s) / den(s), the coefficients highest power first, with `gain`, `in_offset` and `denorm_freq`.
    /// Built as a chain of integrators (controllable canonical form): the nodes X0..X(n-1) are 1 F capacitors charged by the
    /// next one (X(k)' = X(k+1)), X(n) is solved from den(s) X = u, and the output is the weighted sum of the X(k) (so the
    /// transfer function is exact). A 1 GΩ leak keeps a DC analysis well defined (a pure integrator has no DC solution).
    fn s_xfer(t: &[String], rest: &str, path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<Vec<String>, String> {
        let head = t[0].as_str();
        let low = rest.to_ascii_lowercase();
        let array = |key: &str| -> Option<Vec<f64>> {
            let at = low.find(&format!("{key}="))? + key.len() + 1;
            let r = &low[at..];
            let inner = r.trim_start().strip_prefix('[')?;
            Some(inner.split(']').next()?.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()).filter_map(parse_value).collect())
        };
        let scalar = |key: &str, d: f64| -> f64 {
            low.find(&format!("{key}=")).and_then(|a| low[a + key.len() + 1..].split(|c: char| c.is_whitespace() || c == ')').next().and_then(parse_value)).unwrap_or(d)
        };
        let (Some(num), Some(den)) = (array("num_coeff"), array("den_coeff")) else {
            return Err(format!("{head}: s_xfer には num_coeff=[…] と den_coeff=[…] が必要です"));
        };
        let (gain, offset, denorm) = (scalar("gain", 1.0), scalar("in_offset", 0.0), scalar("denorm_freq", 1.0));
        let n = den.len().saturating_sub(1);
        if n < 1 || num.is_empty() || num.len() > den.len() || den[0] == 0.0 || denorm <= 0.0 {
            return Err(format!("{head}: s_xfer の分子は分母より高い次数にできません (分母は 1 次以上で、最高次の係数が 0 でないこと)"));
        }
        // coefficients by power of s (ascending), with s -> s / denorm
        let asc = |c: &[f64]| -> Vec<f64> { c.iter().rev().enumerate().map(|(k, v)| v / denorm.powi(k as i32)).collect() };
        let (d, nu) = (asc(&den), asc(&num));
        let suffix = if path.is_empty() { String::new() } else { format!(".{path}") };
        let (inp, out) = (rename_node(&t[1], ports, prefix), rename_node(&t[2], ports, prefix));
        let x = |k: usize| format!("{prefix}{head}_x{k}");
        let mut lines = vec![];
        for k in 0..n {
            lines.push(format!("B_{head}_i{k}{suffix} 0 {} I=V({})", x(k), x(k + 1)));
            lines.push(format!("C_{head}_c{k}{suffix} {} 0 1 IC=0", x(k)));
            lines.push(format!("R_{head}_r{k}{suffix} {} 0 1e9", x(k)));
        }
        let dn = d[n];
        let sum: Vec<String> = (0..n).map(|k| format!("({:e})*V({})", d[k], x(k))).collect();
        lines.push(format!("B_{head}_n{suffix} {} 0 V=((V({inp})+({offset:e})){})/({dn:e})", x(n), if sum.is_empty() { String::new() } else { format!("-({})", sum.join("+")) }));
        let y: Vec<String> = nu.iter().enumerate().map(|(k, c)| format!("({c:e})*V({})", x(k))).collect();
        lines.push(format!("B_{head}_y{suffix} {out} 0 V=({gain:e})*({})", y.join("+")));
        Ok(lines)
    }

    /// An E / G / F / H line as a behavioural (B) source: `VALUE={expr}`, `POLY(n)` (linear terms, or any degree
    /// with one control) and the current-controlled F / H sources.
    #[allow(clippy::too_many_arguments)]
    fn controlled(&self, raw: &str, env: &Env, line: &str, t: &[String], letter: char, path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<String, String> {
        let head = t[0].as_str();
        let low = line.to_ascii_lowercase();
        let name = format!("B_{head}{}", if path.is_empty() { String::new() } else { format!(".{path}") });
        let out_is_voltage = matches!(letter, 'E' | 'H');
        if t.len() < 4 {
            return Err(format!("{head}: 引数が足りません"));
        }
        let (p, n) = (rename_node(&t[1], ports, prefix), rename_node(&t[2], ports, prefix));
        let rename_ctl = |s: &str| if path.is_empty() { s.to_string() } else { format!("{s}.{path}") };
        // TABLE {expr} = (x1,y1) (x2,y2) ...: a piecewise-linear curve of an expression
        let raw_low = raw.to_ascii_lowercase();
        // `G n+ n- cp cn table(x0 y0 x1 y1 ...)`: the curve of the control voltage
        if matches!(letter, 'E' | 'G')
            && let Some((pos, skip)) = [("table(", 6), ("table=(", 7)].iter().find_map(|(k, n)| find_word(&low, head.len(), k).map(|p| (p, *n)))
            && t.len() >= 6
        {
            let list = &line[pos + skip..];
            let nums: Vec<f64> = list[..list.find(')').unwrap_or(list.len())].split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
            if nums.len() < 4 || !nums.len().is_multiple_of(2) {
                return Err(format!("{head}: table(x0 y0 x1 y1 ...) は、点 (x y) を 2 つ以上並べます"));
            }
            let mut pts: Vec<(f64, f64)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
            let x = format!("V({},{})", rename_node(&t[3], ports, prefix), rename_node(&t[4], ports, prefix));
            return Ok(format!("{name} {p} {n} {}={}", if out_is_voltage { "V" } else { "I" }, pwl_expr(&x, &mut pts)));
        }
        if matches!(letter, 'E' | 'G')
            && let Some(pos) = find_word(&raw_low, head.len(), "table")
        {
            // `TABLE {expr} = (x,y) ...`, `TABLE {expr} ((x,y)(x,y))` or `TABLE expr = ...`
            let after = raw[pos + 5..].trim_start();
            // `TABLE = {expr} (…)`: a stray = before the braces
            let after = match after.strip_prefix('=') {
                Some(r) if r.trim_start().starts_with('{') => r.trim_start(),
                _ => after,
            };
            let (expr_raw, rest) = if let Some(inner) = after.strip_prefix('{') {
                let (mut depth, mut end) = (1, None);
                for (i, c) in inner.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let end = end.ok_or_else(|| format!("{head}: TABLE の {{ が閉じていません"))?;
                (&inner[..end], &inner[end + 1..])
            } else {
                let eq = after.find('=').ok_or_else(|| format!("{head}: TABLE の式と点 (x,y) の区切りの = がありません"))?;
                (&after[..eq], &after[eq + 1..])
            };
            let x = rename_expr(&substitute_params(expr_raw.trim(), env), ports, prefix, path);
            let rest = subst_braces(rest, env)?;
            let mut pts: Vec<(f64, f64)> = vec![];
            for chunk in rest.split(')') {
                if let Some(open) = chunk.rfind('(') {
                    let nums: Vec<f64> = chunk[open + 1..].split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
                    if nums.len() == 2 {
                        pts.push((nums[0], nums[1]));
                    }
                }
            }
            if pts.is_empty() && !rest.contains('(') {
                // the points without parentheses: `x y x y ...` (one pair per continuation line)
                let nums: Vec<f64> = rest.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
                pts = nums.chunks_exact(2).map(|c| (c[0], c[1])).collect();
            }
            if pts.len() < 2 {
                // unbalanced or odd brackets (`(0 0 (1 10)`): take the numbers in order, two by two
                let nums: Vec<f64> = rest.split(|c: char| c == ',' || c == '(' || c == ')' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
                pts = nums.chunks_exact(2).map(|c| (c[0], c[1])).collect();
            }
            if pts.len() < 2 {
                return Err(format!("{head}: TABLE の点 (x,y) が 2 つ以上ありません"));
            }
            let expr = pwl_expr(&x, &mut pts);
            return Ok(format!("{name} {p} {n} {}={expr}", if out_is_voltage { "V" } else { "I" }));
        }
        // VALUE=expr
        if matches!(letter, 'E' | 'G')
            && let Some(pos) = low.find("value")
            && !low[pos + 5..].trim().is_empty()
        {
            // `VALUE={expr}` and `VALUE { expr }` (the braces are gone by now)
            let after = line[pos + 5..].trim_start();
            let after = after.strip_prefix('=').unwrap_or(after).trim();
            // options after the expression (`Rpar=1 Cpar=5n NoJacob`) are handed on untouched
            let mut words: Vec<&str> = after.split_whitespace().collect();
            let mut opts: Vec<&str> = vec![];
            while words.len() > 1 && words.last().is_some_and(|w| is_b_option(w)) {
                opts.insert(0, words.pop().unwrap());
            }
            let cut = words.last().map_or(after.len(), |w| w.as_ptr() as usize + w.len() - after.as_ptr() as usize);
            let expr = rename_expr(&substitute_params(after[..cut].trim(), env), ports, prefix, path);
            let tail = if opts.is_empty() { String::new() } else { format!(" {}", opts.join(" ")) };
            return Ok(format!("{name} {p} {n} {}={expr}{tail}", if out_is_voltage { "V" } else { "I" }));
        }
        // the controls: voltage pairs for E / G, source names for F / H
        let mut idx = 3;
        let mut poly_n: Option<usize> = None;
        if t[idx].to_ascii_lowercase().starts_with("poly") {
            let digits: String = t[idx].chars().filter(|c| c.is_ascii_digit()).collect();
            poly_n = Some(digits.parse().map_err(|_| format!("{head}: POLY(n) の n を読めません"))?);
            idx += 1;
        }
        let count = poly_n.unwrap_or(1);
        let mut xs: Vec<String> = vec![];
        for _ in 0..count {
            if matches!(letter, 'E' | 'G') {
                let first = t.get(idx).ok_or_else(|| format!("{head}: 制御するノードが足りません"))?;
                if first.starts_with('(') {
                    let inner = first.trim_matches(|c| c == '(' || c == ')');
                    let mut it = inner.split(',');
                    let (a, b) = (it.next().unwrap_or("0").trim(), it.next().unwrap_or("0").trim());
                    xs.push(format!("V({},{})", rename_node(a, ports, prefix), rename_node(b, ports, prefix)));
                    idx += 1;
                } else {
                    let b = t.get(idx + 1).ok_or_else(|| format!("{head}: 制御するノードが足りません"))?;
                    xs.push(format!("V({},{})", rename_node(first, ports, prefix), rename_node(b, ports, prefix)));
                    idx += 2;
                }
            } else {
                let ctl = t.get(idx).ok_or_else(|| format!("{head}: 制御する電源の名前がありません"))?;
                xs.push(format!("I({})", rename_ctl(ctl)));
                idx += 1;
            }
        }
        // the coefficients may be in brackets and separated by commas (`POLY(2) V1 V2 (0,7E-5,7E-5)`)
        let coeffs: Vec<f64> = t[idx..]
            .iter()
            .flat_map(|w| w.split([',', '(', ')']).filter(|p| !p.is_empty()))
            .map(|w| parse_value(w).ok_or_else(|| format!("{head}: 係数「{w}」を数値にできません")))
            .collect::<Result<_, _>>()?;
        // several coefficients without the word POLY: the old SPICE2 form, a polynomial in the one control (p0 + p1 x + ...)
        if poly_n.is_none() && coeffs.len() > 1 {
            poly_n = Some(1);
        }
        let expr = if poly_n.is_none() {
            // plain gain
            let g = coeffs.first().ok_or_else(|| format!("{head}: 利得がありません"))?;
            format!("({g:e})*{}", xs[0])
        } else if count == 1 {
            // p0 + p1 x + p2 x^2 + ...
            let x = &xs[0];
            let terms: Vec<String> = coeffs.iter().enumerate().map(|(k, c)| format!("({c:e})*{}", if k == 0 { "1".to_string() } else { vec![x.as_str(); k].join("*") })).collect();
            terms.join("+")
        } else {
            // c0 + c1 x1 + .. + cn xn + the second-order terms x1x1, x1x2, .. x2x2 ... (monomials of one degree in
            // non-decreasing order of the variable numbers), then the third order, and so on
            let mut terms = vec![format!("({:e})", coeffs[0])];
            let mut k = 1;
            let mut degree = 1;
            while k < coeffs.len() && degree <= 6 {
                let mut tuple = vec![0usize; degree];
                loop {
                    if k >= coeffs.len() {
                        break;
                    }
                    terms.push(format!("({:e})*{}", coeffs[k], tuple.iter().map(|i| xs[*i].as_str()).collect::<Vec<_>>().join("*")));
                    k += 1;
                    // next non-decreasing tuple
                    let mut pos = degree;
                    while pos > 0 && tuple[pos - 1] == count - 1 {
                        pos -= 1;
                    }
                    if pos == 0 {
                        break;
                    }
                    let v = tuple[pos - 1] + 1;
                    for t in tuple.iter_mut().skip(pos - 1) {
                        *t = v;
                    }
                }
                degree += 1;
            }
            terms.join("+")
        };
        Ok(format!("{name} {p} {n} {}={expr}", if out_is_voltage { "V" } else { "I" }))
    }
}

/// A flat text for `.subckt sub_name`, or `None` when the text needs no flattening.
pub fn flatten(text: &str, sub_name: &str, lines: &[String]) -> Result<Option<String>, String> {
    if !needs_flatten(text) {
        return Ok(None);
    }
    let expanded = expand_funcs(lines);
    let lines = &expanded[..];
    let mut defs: HashMap<String, Def> = HashMap::new();
    let mut global: Env = HashMap::new();
    let mut models = vec![];
    let mut model_cards = vec![];
    // a .subckt may be defined inside another one: a stack of the blocks being read (definitions are global)
    let mut stack: Vec<(String, Def)> = vec![];
    for l in lines {
        let low = l.to_ascii_lowercase();
        let word = low.split_whitespace().next().unwrap_or("");
        match word {
            ".subckt" => {
                let t = unbrace(split_top(l));
                let name = t.get(1).ok_or(".subckt に名前がありません")?.clone();
                let split = t.iter().skip(2).position(|w| w.contains('=') || w.eq_ignore_ascii_case("params:") || w.eq_ignore_ascii_case("optional:")).map_or(t.len(), |p| p + 2);
                stack.push((name, Def { ports: t[2..split].to_vec(), defaults: assignments(&t[split..])?, body: vec![] }));
            }
            ".ends" | ".end" if !stack.is_empty() => {
                let (name, d) = stack.pop().unwrap();
                defs.insert(name.to_ascii_lowercase(), d);
            }
            ".model" => {
                model_cards.push(l.clone());
                if !l.contains(['{', '\'']) {
                    models.push(l.clone());
                }
                if let Some((_, d)) = stack.last_mut() {
                    d.body.push(l.clone());
                }
            }
            ".param" if stack.is_empty() => {
                for (k, v) in assignments(&split_top(&l[6..]))? {
                    let val = eval_expr(&v, &global)?;
                    global.insert(k, val);
                }
            }
            _ => {
                if let Some((_, d)) = stack.last_mut() {
                    d.body.push(l.clone());
                }
            }
        }
    }
    let top = defs.get(&sub_name.to_ascii_lowercase()).ok_or_else(|| format!(".subckt {sub_name} が見つかりません"))?;
    let ports: HashMap<String, String> = top.ports.iter().map(|p| (p.to_ascii_lowercase(), p.clone())).collect();
    let model_names: std::collections::HashSet<String> = model_cards.iter().filter_map(|m| m.split_whitespace().nth(1).map(str::to_ascii_lowercase)).collect();
    let model_kinds: HashMap<String, (String, String)> = model_cards
        .iter()
        .filter_map(|m| {
            let mut it = m.split_whitespace();
            it.next()?;
            let name = it.next()?.to_ascii_lowercase();
            let rest = m.splitn(3, char::is_whitespace).nth(2).unwrap_or("").trim().to_string();
            let ty: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>().to_ascii_lowercase();
            Some((name, (ty, rest)))
        })
        .collect();
    let body = Ctx { defs: &defs, model_kinds: &model_kinds, models: &model_names }.expand(top, "", &ports, global, &[], 0)?;
    let mut s = models.join("\n");
    s.push_str(&format!("\n.subckt {sub_name} {}\n{}\n.ends {sub_name}\n", top.ports.join(" "), body.join("\n")));
    Ok(Some(s))
}

/// `.func name(a, b) {body}` user functions: every call `name(x, y)` in the other lines is replaced by the body with the
/// arguments put in (in parentheses). A function may call another one; the expansion repeats a few times for that.
fn expand_funcs(lines: &[String]) -> Vec<String> {
    let mut funcs: Vec<(String, Vec<String>, String)> = vec![];
    for l in lines {
        let t = l.trim_start();
        if !t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case(".func")) {
            continue;
        }
        let rest = t[5..].trim_start();
        let Some(open) = rest.find('(') else { continue };
        let Some(close) = rest[open..].find(')').map(|c| c + open) else { continue };
        let name = rest[..open].trim().to_ascii_lowercase();
        let args: Vec<String> = rest[open + 1..close].split(',').map(|a| a.trim().to_ascii_lowercase()).filter(|a| !a.is_empty()).collect();
        let mut body = rest[close + 1..].trim();
        body = body.strip_prefix('=').unwrap_or(body).trim();
        if let Some(inner) = body.strip_prefix('{').and_then(|b| b.strip_suffix('}')) {
            body = inner.trim();
        }
        if !name.is_empty() && !body.is_empty() {
            funcs.push((name, args, body.to_string()));
        }
    }
    if funcs.is_empty() {
        return lines.to_vec();
    }
    lines
        .iter()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with('*') || t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case(".func")) {
                return l.clone();
            }
            let mut text = l.clone();
            for _ in 0..6 {
                let next = expand_calls(&text, &funcs);
                if next == text {
                    break;
                }
                text = next;
            }
            text
        })
        .collect()
}

fn expand_calls(text: &str, funcs: &[(String, Vec<String>, String)]) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_alphabetic() || b[i] == '_' {
            let st = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let id: String = b[st..i].iter().collect();
            let mut j = i;
            while j < b.len() && b[j] == ' ' {
                j += 1;
            }
            let func = funcs.iter().find(|f| f.0 == id.to_ascii_lowercase());
            if let (Some((_, params, body)), true) = (func, j < b.len() && b[j] == '(') {
                // the argument list up to the matching parenthesis
                let (mut depth, mut k, mut args, mut cur) = (0, j, vec![], String::new());
                while k < b.len() {
                    match b[k] {
                        '(' => {
                            depth += 1;
                            if depth > 1 {
                                cur.push('(');
                            }
                        }
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            cur.push(')');
                        }
                        ',' if depth == 1 => args.push(std::mem::take(&mut cur)),
                        c => cur.push(c),
                    }
                    k += 1;
                }
                if depth == 0 && k < b.len() {
                    if !cur.trim().is_empty() || !args.is_empty() {
                        args.push(cur);
                    }
                    if args.len() == params.len() {
                        out.push('(');
                        out.push_str(&put_args(body, params, &args));
                        out.push(')');
                        i = k + 1;
                        continue;
                    }
                }
            }
            out.push_str(&id);
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `body` with each parameter name replaced by its argument (in parentheses).
fn put_args(body: &str, params: &[String], args: &[String]) -> String {
    let b: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_alphabetic() || b[i] == '_' {
            let st = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let id: String = b[st..i].iter().collect();
            match params.iter().position(|p| *p == id.to_ascii_lowercase()) {
                Some(k) => out.push_str(&format!("({})", args[k].trim())),
                None => out.push_str(&id),
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `idt(x, ic)` and `idtmod(x, ic, m [, offset])` (time integrals, LTspice) inside a B source: each one becomes a 1 F capacitor
/// charged by a current source `x` (starting at `ic`), and the call is replaced by the capacitor voltage (for `idtmod` wrapped
/// into `[offset, offset + m)`). A leak resistor of 1 GΩ keeps the node from floating in a DC analysis (where the integral is
/// meaningless; a transient analysis starts from `ic`).
fn extract_integrals(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let low = line.to_ascii_lowercase();
        if !(low.starts_with(['b']) && (low.contains("idt") || low.contains("ddt"))) {
            out.push(line);
            continue;
        }
        let name = line.split_whitespace().next().unwrap_or("B").to_string();
        let mut text = line.clone();
        let mut extra = vec![];
        let mut k = 0;
        loop {
            let low = text.to_ascii_lowercase();
            // the first call of idt( or idtmod( that starts a word
            let Some((at, wlen)) = ["idtmod(", "idt(", "ddt(", "sdt("].iter().filter_map(|w| find_word(&low, 0, w).map(|p| (p, w.len()))).min_by_key(|(p, _)| *p) else { break };
            let open = at + wlen - 1;
            let mut depth = 0;
            let mut args: Vec<String> = vec![];
            let mut cur = String::new();
            let mut end = None;
            for (i, c) in text[open..].char_indices() {
                match c {
                    '(' => {
                        depth += 1;
                        if depth > 1 {
                            cur.push(c);
                        }
                    }
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(open + i);
                            break;
                        }
                        cur.push(c);
                    }
                    ',' if depth == 1 => args.push(std::mem::take(&mut cur)),
                    _ => cur.push(c),
                }
            }
            let Some(end) = end else { break };
            args.push(cur);
            let is_mod = wlen == 7;
            if text[at..].starts_with("ddt(") || text[at..].starts_with("DDT(") || text[at..].to_ascii_lowercase().starts_with("ddt(") {
                // ddt(x): the current of a capacitor whose charge is x, read through a 0 V source that holds its node at 0
                let node = format!("{name}_ddt{k}");
                extra.push(format!("V_{node} {node} 0 DC 0"));
                extra.push(format!("C_{node} {node} 0 Q={}", args[0].trim()));
                text.replace_range(at..=end, &format!("(-I(V_{node}))"));
                k += 1;
                continue;
            }
            let node = format!("{name}_idt{k}");
            let ic = args.get(1).and_then(|a| eval_expr(a, &Env::new()).ok()).unwrap_or(0.0);
            extra.push(format!("B_{node} 0 {node} I={}", args[0].trim()));
            extra.push(format!("C_{node} {node} 0 1 IC={ic:e}"));
            extra.push(format!("R_{node} {node} 0 1e9"));
            let repl = if is_mod && args.len() >= 3 {
                let m = args[2].trim();
                let off = args.get(3).map_or("0", |a| a.trim());
                format!("(V({node})-({m})*floor((V({node})-({off}))/({m})))")
            } else {
                format!("V({node})")
            };
            text.replace_range(at..=end, &repl);
            k += 1;
        }
        out.push(text);
        out.extend(extra);
    }
    out
}

/// `text` with every whole word `word` (case-insensitive) replaced by `with`.
fn find_replace_word(text: &str, word: &str, with: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_alphabetic() || b[i] == '_' {
            let st = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let id: String = b[st..i].iter().collect();
            if id.eq_ignore_ascii_case(word) {
                out.push_str(with);
            } else {
                out.push_str(&id);
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `a = b` -> `a=b` (the blanks vendors put around `=` in a model card).
fn squash_eq_text(s: &str) -> String {
    crate::spice_in::squash_equals_pub(s)
}

/// `name=value` (a single `=`, a plain name that is not `V` / `I`) or the bare word `NoJacob`: an option of a B source.
fn is_b_option(w: &str) -> bool {
    if w.eq_ignore_ascii_case("nojacob") {
        return true;
    }
    match w.split_once('=') {
        Some((k, v)) => !v.starts_with('=') && !k.is_empty() && k.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.eq_ignore_ascii_case("v") && !k.eq_ignore_ascii_case("i"),
        None => false,
    }
}
