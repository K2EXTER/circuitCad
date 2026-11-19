//! The words of a sketch: numbers, names, strings, operators. Comments (`//`, `/* */`) and `#include` lines are skipped; a
//! `#define NAME value` becomes a token of its own.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    /// `#define NAME` and the words that follow it on the line.
    Define(String, Vec<(Tok, u32)>),
    /// An operator or punctuation: `+`, `+=`, `++`, `(`, `{`, `;`, `&&`, `<<`, `.`, ...
    Op(&'static str),
}

const OPS: [&str; 41] = [
    "<<=", ">>=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "+", "-", "*", "/", "%", "=", "<", ">", "!", "~", "&",
    "|", "^", "(", ")", "{", "}", "[", "]", ";", ",",
];
const MORE_OPS: [&str; 3] = [".", "?", ":"];

/// The tokens and the line (from 1) each starts on.
pub fn lex(src: &str) -> Result<Vec<(Tok, u32)>, (u32, String)> {
    let b: Vec<char> = src.chars().collect();
    let mut out = vec![];
    let (mut i, mut line) = (0usize, 1u32);
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                if b[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c == '#' {
            // a preprocessor line: #define NAME value, the rest (#include) ignored
            let start = i;
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            let text: String = b[start..i].iter().collect();
            let mut words = text.trim_start_matches('#').trim_start().splitn(2, char::is_whitespace);
            if words.next() == Some("define") {
                let rest = words.next().unwrap_or("").trim();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("").to_string();
                if name.is_empty() {
                    return Err((line, "#define の後に名前がありません".into()));
                }
                let value = lex(parts.next().unwrap_or(""))?.into_iter().map(|(t, _)| (t, line)).collect();
                out.push((Tok::Define(name, value), line));
            }
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            if c == '0' && matches!(b.get(i + 1), Some('x') | Some('X')) {
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let text: String = b[start + 2..i].iter().collect();
                out.push((Tok::Int(i64::from_str_radix(&text, 16).map_err(|_| (line, format!("16 進の数「0x{text}」が読めません")))?), line));
            } else if c == '0' && matches!(b.get(i + 1), Some('b') | Some('B')) {
                i += 2;
                while i < b.len() && (b[i] == '0' || b[i] == '1') {
                    i += 1;
                }
                let text: String = b[start + 2..i].iter().collect();
                out.push((Tok::Int(i64::from_str_radix(&text, 2).map_err(|_| (line, format!("2 進の数「0b{text}」が読めません")))?), line));
            } else {
                let mut float = false;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.' || ((b[i] == 'e' || b[i] == 'E') && float)) {
                    float |= b[i] == '.';
                    i += 1;
                }
                let text: String = b[start..i].iter().collect();
                // suffixes: 10UL, 3.0f
                while i < b.len() && matches!(b[i], 'u' | 'U' | 'l' | 'L' | 'f' | 'F') {
                    i += 1;
                }
                if float {
                    out.push((Tok::Float(text.parse().map_err(|_| (line, format!("数「{text}」が読めません")))?), line));
                } else {
                    out.push((Tok::Int(text.parse().map_err(|_| (line, format!("数「{text}」が大きすぎるか読めません")))?), line));
                }
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push((Tok::Ident(b[start..i].iter().collect()), line));
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\n' {
                    return Err((line, "文字列の \" が閉じていません".into()));
                }
                if b[i] == '\\' && i + 1 < b.len() {
                    i += 1;
                    s.push(match b[i] {
                        'n' => '\n',
                        't' => '\t',
                        x => x,
                    });
                } else {
                    s.push(b[i]);
                }
                i += 1;
            }
            i += 1;
            out.push((Tok::Str(s), line));
        } else if c == '\'' {
            // a character: its code
            let ch = b.get(i + 1).copied().ok_or((line, "' が閉じていません".to_string()))?;
            let (ch, len) = if ch == '\\' { (match b.get(i + 2) { Some('n') => '\n', Some('t') => '\t', Some(x) => *x, None => '\\' }, 4) } else { (ch, 3) };
            if b.get(i + len - 1) != Some(&'\'') {
                return Err((line, "文字は '1' のように 1 文字を ' で囲みます".into()));
            }
            out.push((Tok::Int(ch as i64), line));
            i += len;
        } else {
            let rest: String = b[i..(i + 3).min(b.len())].iter().collect();
            match OPS.iter().chain(MORE_OPS.iter()).find(|op| rest.starts_with(**op)) {
                Some(op) => {
                    out.push((Tok::Op(op), line));
                    i += op.len();
                }
                None => return Err((line, format!("「{c}」はスケッチでは使えない文字です"))),
            }
        }
    }
    Ok(out)
}
