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
        low.starts_with(['x', 'f', 'h'])
            || low.starts_with(".param")
            || low.contains("params:")
            || l.contains('{')
            || (!l.starts_with('*') && l.contains('\''))
            || (!l.starts_with(['*', '.']) && (low.contains("poly") || low.contains("value") || low.contains("table")))
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

fn assignments(tokens: &[String]) -> Result<Vec<(String, String)>, String> {
    tokens
        .iter()
        .filter(|t| !t.eq_ignore_ascii_case("params:"))
        .map(|t| {
            t.split_once('=')
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .ok_or_else(|| format!("「{t}」は 名前=値 の形ではありません"))
        })
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
            out.push(c);
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
            out.push(c);
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

fn node_count(letter: char) -> Option<usize> {
    Some(match letter {
        'R' | 'C' | 'L' | 'V' | 'I' | 'D' | 'B' => 2,
        'Q' => 3,
        'E' | 'G' | 'M' | 'S' => 4,
        _ => return None,
    })
}

struct Ctx<'a> {
    defs: &'a HashMap<String, Def>,
    /// Lower-case names of the `.model` cards (to tell a transistor's substrate node from its model name).
    models: &'a std::collections::HashSet<String>,
}

impl Ctx<'_> {
    /// Body lines of `def` with parameters resolved and nested calls inlined.
    fn expand(&self, def: &Def, path: &str, ports: &HashMap<String, String>, mut env: Env, call_args: &[(String, String)], depth: usize) -> Result<Vec<String>, String> {
        if depth > MAX_DEPTH {
            return Err("サブサーキットの入れ子が深すぎます (自分自身を呼んでいませんか)".into());
        }
        let outer = env.clone();
        for (k, v) in &def.defaults {
            let val = eval_expr(v, &env)?;
            env.insert(k.clone(), val);
        }
        for (k, v) in call_args {
            let val = eval_expr(v, &outer)?;
            env.insert(k.clone(), val);
        }
        // the subcircuit's own .param lines, in any order (a name may be used before the line that defines it)
        let mut pending: Vec<(String, String)> = vec![];
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
            let line = subst_braces(raw, &env)?;
            let t = split_top(&line);
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
            if matches!(letter, 'E' | 'G' | 'F' | 'H') {
                out.push(self.controlled(&line, &t, letter, path, ports, &prefix)?);
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
            if letter == 'B' {
                let rest = t[n + 1..].join(" ");
                u.truncate(n + 1);
                u.push(rename_expr(&rest, ports, &prefix, path));
            }
            out.push(u.join(" "));
        }
        Ok(out)
    }
}

impl Ctx<'_> {
    /// An E / G / F / H line as a behavioural (B) source: `VALUE={expr}`, `POLY(n)` (linear terms, or any degree
    /// with one control) and the current-controlled F / H sources.
    fn controlled(&self, line: &str, t: &[String], letter: char, path: &str, ports: &HashMap<String, String>, prefix: &str) -> Result<String, String> {
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
        if matches!(letter, 'E' | 'G')
            && let Some(pos) = low.find("table")
            && let Some(eq) = low[pos..].find('=').map(|e| e + pos)
        {
            let x = rename_expr(line[pos + 5..eq].trim(), ports, prefix, path);
            let mut pts: Vec<(f64, f64)> = vec![];
            for chunk in line[eq + 1..].split(')') {
                if let Some(open) = chunk.find('(') {
                    let nums: Vec<f64> = chunk[open + 1..].split([',', ' ']).filter(|s| !s.is_empty()).filter_map(parse_value).collect();
                    if nums.len() == 2 {
                        pts.push((nums[0], nums[1]));
                    }
                }
            }
            if pts.len() < 2 {
                return Err(format!("{head}: TABLE の点 (x,y) が 2 つ以上ありません"));
            }
            pts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let xs = format!("({x})");
            let mut expr = format!("({:e})", pts[pts.len() - 1].1);
            for k in (0..pts.len() - 1).rev() {
                let ((x0, y0), (x1, y1)) = (pts[k], pts[k + 1]);
                let seg = if x1 > x0 { format!("({y0:e})+({:e})*({xs}-({x0:e}))", (y1 - y0) / (x1 - x0)) } else { format!("({y0:e})") };
                expr = format!("if({xs}<({x1:e}),{seg},{expr})");
            }
            expr = format!("if({xs}<({:e}),({:e}),{expr})", pts[0].0, pts[0].1);
            return Ok(format!("{name} {p} {n} {}={expr}", if out_is_voltage { "V" } else { "I" }));
        }
        // VALUE=expr
        if matches!(letter, 'E' | 'G')
            && let Some(pos) = low.find("value")
            && low[pos + 5..].trim_start().starts_with('=')
        {
            let eq = pos + 5 + low[pos + 5..].find('=').unwrap();
            let expr = rename_expr(line[eq + 1..].trim(), ports, prefix, path);
            return Ok(format!("{name} {p} {n} {}={expr}", if out_is_voltage { "V" } else { "I" }));
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
        let coeffs: Vec<f64> = t[idx..].iter().map(|w| parse_value(w).ok_or_else(|| format!("{head}: 係数「{w}」を数値にできません"))).collect::<Result<_, _>>()?;
        let expr = if poly_n.is_none() {
            // plain gain
            let g = coeffs.first().ok_or_else(|| format!("{head}: 利得がありません"))?;
            format!("({g:e})*{}", xs[0])
        } else if count == 1 {
            // p0 + p1 x + p2 x^2 + ...
            let x = &xs[0];
            let terms: Vec<String> = coeffs.iter().enumerate().map(|(k, c)| format!("({c:e})*{}", if k == 0 { "1".to_string() } else { vec![x.as_str(); k].join("*") })).collect();
            terms.join("+")
        } else if coeffs.len() == count + 1 {
            let mut terms = vec![format!("({:e})", coeffs[0])];
            terms.extend(xs.iter().zip(&coeffs[1..]).map(|(x, c)| format!("({c:e})*{x}")));
            terms.join("+")
        } else {
            return Err(format!("{head}: POLY({count}) の高次の項 (かけ算の項) は未対応です。1 次の係数だけの POLY か、1 つの制御の多項式にしてください"));
        };
        Ok(format!("{name} {p} {n} {}={expr}", if out_is_voltage { "V" } else { "I" }))
    }
}

/// A flat text for `.subckt sub_name`, or `None` when the text needs no flattening.
pub fn flatten(text: &str, sub_name: &str, lines: &[String]) -> Result<Option<String>, String> {
    if !needs_flatten(text) {
        return Ok(None);
    }
    let mut defs: HashMap<String, Def> = HashMap::new();
    let mut global: Env = HashMap::new();
    let mut models = vec![];
    let mut model_cards = vec![];
    let mut cur: Option<(String, Def)> = None;
    for l in lines {
        let low = l.to_ascii_lowercase();
        let word = low.split_whitespace().next().unwrap_or("");
        match (word, cur.as_mut()) {
            (".subckt", None) => {
                let t = split_top(l);
                let name = t.get(1).ok_or(".subckt に名前がありません")?.clone();
                let split = t.iter().skip(2).position(|w| w.contains('=') || w.eq_ignore_ascii_case("params:")).map_or(t.len(), |p| p + 2);
                cur = Some((name, Def { ports: t[2..split].to_vec(), defaults: assignments(&t[split..])?, body: vec![] }));
            }
            (".ends" | ".end", Some(_)) => {
                let (name, d) = cur.take().unwrap();
                defs.insert(name.to_ascii_lowercase(), d);
            }
            (".model", c) => {
                model_cards.push(l.clone());
                if !l.contains(['{', '\'']) {
                    models.push(l.clone());
                }
                if let Some((_, d)) = c {
                    d.body.push(l.clone());
                }
            }
            (".param", None) => {
                for (k, v) in assignments(&split_top(&l[6..]))? {
                    let val = eval_expr(&v, &global)?;
                    global.insert(k, val);
                }
            }
            (_, Some((_, d))) => d.body.push(l.clone()),
            _ => {}
        }
    }
    let top = defs.get(&sub_name.to_ascii_lowercase()).ok_or_else(|| format!(".subckt {sub_name} が見つかりません"))?;
    let ports: HashMap<String, String> = top.ports.iter().map(|p| (p.to_ascii_lowercase(), p.clone())).collect();
    let model_names: std::collections::HashSet<String> = model_cards.iter().filter_map(|m| m.split_whitespace().nth(1).map(str::to_ascii_lowercase)).collect();
    let body = Ctx { defs: &defs, models: &model_names }.expand(top, "", &ports, global, &[], 0)?;
    let mut s = models.join("\n");
    s.push_str(&format!("\n.subckt {sub_name} {}\n{}\n.ends {sub_name}\n", top.ports.join(" "), body.join("\n")));
    Ok(Some(s))
}
