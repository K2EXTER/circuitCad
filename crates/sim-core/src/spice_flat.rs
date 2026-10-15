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
        low.starts_with('x') || low.starts_with(".param") || low.contains("params:") || l.contains('{') || (!l.starts_with('*') && l.contains('\''))
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
        out.push_str(&format!("{:e}", eval_expr(&inner, env)?));
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
        let prefix = if path.is_empty() { String::new() } else { format!("{path}.") };
        let mut out = vec![];
        for raw in &def.body {
            let low = raw.to_ascii_lowercase();
            if low.starts_with(".param") {
                for (k, v) in assignments(&split_top(&raw[6..]))? {
                    // an argument given by the caller wins over the subcircuit's own .param
                    if !call_args.iter().any(|(a, _)| *a == k) {
                        let val = eval_expr(&v, &env)?;
                        env.insert(k, val);
                    }
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
            let Some(n) = node_count(letter) else {
                out.push(line);
                continue;
            };
            if t.len() < n + 1 {
                out.push(line);
                continue;
            }
            let mut u = t.clone();
            if !path.is_empty() {
                u[0] = format!("{head}.{path}");
            }
            for w in u.iter_mut().skip(1).take(n) {
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

/// A flat text for `.subckt sub_name`, or `None` when the text needs no flattening.
pub fn flatten(text: &str, sub_name: &str, lines: &[String]) -> Result<Option<String>, String> {
    if !needs_flatten(text) {
        return Ok(None);
    }
    let mut defs: HashMap<String, Def> = HashMap::new();
    let mut global: Env = HashMap::new();
    let mut models = vec![];
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
            (".ends", Some(_)) => {
                let (name, d) = cur.take().unwrap();
                defs.insert(name.to_ascii_lowercase(), d);
            }
            (".model", c) => {
                models.push(l.clone());
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
    let body = Ctx { defs: &defs }.expand(top, "", &ports, global, &[], 0)?;
    let mut s = models.join("\n");
    s.push_str(&format!("\n.subckt {sub_name} {}\n{}\n.ends {sub_name}\n", top.ports.join(" "), body.join("\n")));
    Ok(Some(s))
}
