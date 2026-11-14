//! Puts a fitted model into a `.part.json` without rewriting the rest of the file: only the values of `sim.text`,
//! `sim.limits`, `sim.origin`, `description` and `datasheet` are replaced (or added at the end of their object), in
//! the file's own layout (two spaces a level, one item a line). The part files are written by hand, by the app and by
//! scripts in slightly different number and field layouts, and a full rewrite would change all of that.

use super::Fitted;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Writes `f` into `<dir>/<part>.part.json`, which must already exist (the symbol, pins and footprint come from it).
pub fn write_part(dir: &Path, f: &Fitted) -> Result<PathBuf, String> {
    let path = dir.join(format!("{}.part.json", f.part));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e} (先に部品を作ってから、モデルを書き込みます)", path.display()))?;
    let out = edit_part(&text, f)?;
    serde_json::from_str::<crate::PartDef>(&out).map_err(|e| format!("書き換えた部品が読めません ({e})"))?;
    if out != text {
        std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(path)
}

/// The part file's text with the model put in.
pub fn edit_part(text: &str, f: &Fitted) -> Result<String, String> {
    let mut t = text.to_string();
    t = set(&t, &["sim", "text"], &json!(f.text))?;
    t = set(&t, &["sim", "limits"], &json!(f.limits))?;
    t = set(&t, &["sim", "origin"], &json!({"kind": "equivalent", "source": f.source}))?;
    if !f.description.is_empty() {
        t = set(&t, &["description"], &json!(f.description))?;
    }
    if !f.url.is_empty() {
        t = set(&t, &["datasheet"], &json!(f.url))?;
    }
    Ok(t)
}

/// One object member: where its key starts and where its value starts and ends (byte offsets).
struct Member {
    key: String,
    key_at: usize,
    value: (usize, usize),
}

/// Sets the member at `path` (objects only) to `v`.
fn set(text: &str, path: &[&str], v: &Value) -> Result<String, String> {
    let s = text.as_bytes();
    let mut at = skip_ws(s, 0);
    for (depth, key) in path.iter().enumerate() {
        let (members, close) = object(s, at)?;
        match members.iter().find(|m| m.key == *key) {
            Some(m) if depth + 1 == path.len() => {
                let indent = line_indent(s, m.key_at);
                return Ok(format!("{}{}{}", &text[..m.value.0], pretty(v, indent, newline(text)), &text[m.value.1..]));
            }
            Some(m) => at = m.value.0,
            None if depth + 1 == path.len() => {
                let nl = newline(text);
                let indent = members.first().map(|m| line_indent(s, m.key_at)).unwrap_or(line_indent(s, at) + 2);
                let insert = members.last().map(|m| m.value.1).unwrap_or(at + 1);
                let lead = if members.is_empty() { "" } else { "," };
                let tail = if members.is_empty() { format!("{nl}{}", " ".repeat(line_indent(s, close))) } else { String::new() };
                let item = format!("{lead}{nl}{}{}: {}{tail}", " ".repeat(indent), json!(key), pretty(v, indent, nl));
                return Ok(format!("{}{}{}", &text[..insert], item, &text[insert..]));
            }
            None => return Err(format!("部品のファイルに \"{key}\" がありません")),
        }
    }
    Err("書き換える場所がありません".into())
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Python's `json.dumps(v, ensure_ascii=False, indent=2)` for a value whose key line is indented by `indent`.
fn pretty(v: &Value, indent: usize, nl: &str) -> String {
    let pad = |n: usize| " ".repeat(n);
    match v {
        Value::Array(a) if !a.is_empty() => {
            let items: Vec<String> = a.iter().map(|x| format!("{}{}", pad(indent + 2), pretty(x, indent + 2, nl))).collect();
            format!("[{nl}{}{nl}{}]", items.join(&format!(",{nl}")), pad(indent))
        }
        Value::Object(o) if !o.is_empty() => {
            let items: Vec<String> = o.iter().map(|(k, x)| format!("{}{}: {}", pad(indent + 2), json!(k), pretty(x, indent + 2, nl))).collect();
            format!("{{{nl}{}{nl}{}}}", items.join(&format!(",{nl}")), pad(indent))
        }
        _ => v.to_string(),
    }
}

/// The number of spaces at the start of the line holding byte `at`.
fn line_indent(s: &[u8], at: usize) -> usize {
    let start = s[..at].iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
    s[start..].iter().take_while(|&&c| c == b' ').count()
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn bad(i: usize) -> String {
    format!("部品のファイルの JSON が読めません ({i} 文字目のあたり)")
}

/// The members of the object starting at `i` and the offset of its closing brace.
fn object(s: &[u8], i: usize) -> Result<(Vec<Member>, usize), String> {
    if s.get(i) != Some(&b'{') {
        return Err(bad(i));
    }
    let mut members = vec![];
    let mut j = skip_ws(s, i + 1);
    if s.get(j) == Some(&b'}') {
        return Ok((members, j));
    }
    loop {
        let key_at = j;
        let key_end = string_end(s, j)?;
        let key: String = serde_json::from_slice(&s[j..key_end]).map_err(|_| bad(j))?;
        j = skip_ws(s, key_end);
        if s.get(j) != Some(&b':') {
            return Err(bad(j));
        }
        let vs = skip_ws(s, j + 1);
        let ve = value_end(s, vs)?;
        members.push(Member { key, key_at, value: (vs, ve) });
        j = skip_ws(s, ve);
        match s.get(j) {
            Some(b',') => j = skip_ws(s, j + 1),
            Some(b'}') => return Ok((members, j)),
            _ => return Err(bad(j)),
        }
    }
}

fn string_end(s: &[u8], i: usize) -> Result<usize, String> {
    if s.get(i) != Some(&b'"') {
        return Err(bad(i));
    }
    let mut j = i + 1;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            b'"' => return Ok(j + 1),
            _ => j += 1,
        }
    }
    Err(bad(i))
}

/// The end (exclusive) of the value starting at `i`.
fn value_end(s: &[u8], i: usize) -> Result<usize, String> {
    match s.get(i) {
        Some(b'"') => string_end(s, i),
        Some(b'{') | Some(b'[') => {
            let mut depth = 0;
            let mut j = i;
            while j < s.len() {
                match s[j] {
                    b'"' => {
                        j = string_end(s, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            Err(bad(i))
        }
        Some(_) => Ok(i + s[i..].iter().position(|c| matches!(c, b',' | b'}' | b']') || c.is_ascii_whitespace()).unwrap_or(s.len() - i)),
        None => Err(bad(i)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted() -> Fitted {
        Fitted {
            part: "X".into(),
            source: "S".into(),
            url: "u".into(),
            description: "d".into(),
            text: ".model X D(IS=1)".into(),
            limits: vec!["a".into(), "b \"q\"".into()],
            rows: vec![],
            notes: vec![],
        }
    }

    #[test]
    fn only_the_model_fields_change_and_missing_ones_are_added_at_the_end() {
        let before = "{\n  \"name\": \"X\",\n  \"description\": \"old\",\n  \"pins\": [1.0, 2],\n  \"sim\": {\n    \"text\": \".model X D\",\n    \"limits\": []\n  }\n}\n";
        let after = edit_part(before, &fitted()).unwrap();
        assert_eq!(
            after,
            "{\n  \"name\": \"X\",\n  \"description\": \"d\",\n  \"pins\": [1.0, 2],\n  \"sim\": {\n    \"text\": \".model X D(IS=1)\",\n    \"limits\": [\n      \"a\",\n      \"b \\\"q\\\"\"\n    ],\n    \"origin\": {\n      \"kind\": \"equivalent\",\n      \"source\": \"S\"\n    }\n  },\n  \"datasheet\": \"u\"\n}\n"
        );
        // a second write changes nothing
        assert_eq!(edit_part(&after, &fitted()).unwrap(), after);
    }

    #[test]
    fn crlf_files_keep_their_line_ends() {
        let before = "{\r\n  \"sim\": {\r\n    \"text\": \"\"\r\n  }\r\n}";
        let after = edit_part(before, &fitted()).unwrap();
        assert!(!after.replace("\r\n", "").contains('\n'), "{after:?}");
    }
}
