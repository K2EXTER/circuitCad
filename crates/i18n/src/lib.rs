//! The English translation of the screen text.
//!
//! The Japanese text stays in the code as it is and is the key of a table (`en/*.tsv`, one `日本語<TAB>English` per line).
//! In English mode a text with an entry is shown in English and a text without one stays Japanese, so the table can grow
//! a little at a time. Nothing changes in Japanese mode (the default).
//!
//! - `tr("ファイル")` for a fixed text.
//! - `trf!("部品が{n}個あります", n = 3)` for a formatted one: the table keeps the format string itself as the key, and the
//!   English side uses the same placeholders (`{n}`, `{:.2}`, `{}` …; `{@2}` is "the second placeholder of the Japanese",
//!   for a sentence that needs another order).
//! - `tr_dyn(&text)` for a text made somewhere else (an ERC finding, a status line): it is looked up as it is, and else
//!   matched against every formatted key, so `format!` in the core crates needs no change.
//!
//! Details and the tools to list the missing entries: `docs/code-notes/i18n.md`.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }

    /// "ja" / "en" (and a few spellings); anything else is `None`.
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ja" | "jp" | "japanese" | "日本語" => Some(Lang::Ja),
            "en" | "english" => Some(Lang::En),
            _ => None,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    LANG.store(if l == Lang::En { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::En } else { Lang::Ja }
}

pub fn is_ja() -> bool {
    lang() == Lang::Ja
}

/// `CCAD_LANG=en` (or `ja`) chooses the language; returns what it chose.
pub fn lang_from_env() -> Option<Lang> {
    std::env::var("CCAD_LANG").ok().as_deref().and_then(Lang::parse)
}

#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Lit(String),
    /// The text between the braces: "", "n", ":.2", "x:>4", "@2".
    Ph(String),
}

/// Splits a Rust format string into literal text and placeholders (`{{` and `}}` are literal braces).
fn parse_format(s: &str) -> Option<Vec<Seg>> {
    let mut segs = vec![];
    let mut lit = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                lit.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                lit.push('}');
            }
            '{' => {
                let mut tok = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('{') | None => return None,
                        Some(c) => tok.push(c),
                    }
                }
                if !lit.is_empty() {
                    segs.push(Seg::Lit(std::mem::take(&mut lit)));
                }
                segs.push(Seg::Ph(tok));
            }
            '}' => return None,
            c => lit.push(c),
        }
    }
    if !lit.is_empty() {
        segs.push(Seg::Lit(lit));
    }
    Some(segs)
}

/// Placeholders side by side in the Japanese ("{}{}") cannot be told apart in the finished text; when the English has the same
/// ones side by side in the same order, both sides treat the run as one placeholder (the joined value goes in as it is).
fn merge_runs(ja: Vec<Seg>, en: Vec<Seg>) -> (Vec<Seg>, Vec<Seg>) {
    fn runs(segs: &[Seg]) -> Vec<(usize, usize)> {
        let mut out = vec![];
        let mut i = 0;
        while i < segs.len() {
            let mut j = i;
            while j < segs.len() && matches!(segs[j], Seg::Ph(_)) {
                j += 1;
            }
            if j - i >= 2 {
                out.push((i, j));
            }
            i = j.max(i + 1);
        }
        out
    }
    let mut ja = ja;
    let mut en = en;
    for (a, b) in runs(&ja).into_iter().rev() {
        let run = ja[a..b].to_vec();
        if let Some(p) = (0..=en.len().saturating_sub(run.len())).find(|&p| en[p..].starts_with(&run)) {
            let tok = run.iter().map(|s| if let Seg::Ph(t) = s { t.as_str() } else { "" }).collect::<Vec<_>>().join("\u{1}");
            en.splice(p..p + run.len(), [Seg::Ph(tok.clone())]);
            ja.splice(a..b, [Seg::Ph(tok)]);
        }
    }
    (ja, en)
}

fn has_placeholder(segs: &[Seg]) -> bool {
    segs.iter().any(|s| matches!(s, Seg::Ph(_)))
}

/// The values of the placeholders in a finished text, found by its literal parts (the leftmost match of each).
/// Two placeholders side by side cannot be told apart and give `None`.
fn capture<'a>(segs: &[Seg], s: &'a str) -> Option<Vec<&'a str>> {
    let mut vals = vec![];
    let mut pos = 0;
    let mut pending = false;
    for (i, seg) in segs.iter().enumerate() {
        match seg {
            Seg::Ph(_) => {
                if pending {
                    return None;
                }
                pending = true;
            }
            Seg::Lit(l) => {
                if pending {
                    let at = if i + 1 == segs.len() {
                        // the last literal is the end of the text
                        if !s[pos..].ends_with(l.as_str()) {
                            return None;
                        }
                        s.len() - l.len()
                    } else {
                        pos + s[pos..].find(l.as_str())?
                    };
                    vals.push(&s[pos..at]);
                    pos = at + l.len();
                    pending = false;
                } else {
                    if !s[pos..].starts_with(l.as_str()) {
                        return None;
                    }
                    pos += l.len();
                }
            }
        }
    }
    if pending {
        vals.push(&s[pos..]);
    } else if pos != s.len() {
        return None;
    }
    Some(vals)
}

/// Puts the values (in the order of the Japanese placeholders) into the English segments.
fn fill(ja: &[Seg], en: &[Seg], vals: &[&str]) -> Option<String> {
    let ja_ph: Vec<&str> = ja.iter().filter_map(|s| if let Seg::Ph(t) = s { Some(t.as_str()) } else { None }).collect();
    let mut used: HashMap<&str, usize> = HashMap::new();
    let mut out = String::new();
    for seg in en {
        match seg {
            Seg::Lit(l) => out.push_str(l),
            Seg::Ph(t) => {
                let idx = if let Some(n) = t.strip_prefix('@') {
                    n.parse::<usize>().ok()?.checked_sub(1)?
                } else {
                    let k = used.entry(t.as_str()).or_insert(0);
                    let idx = ja_ph.iter().enumerate().filter(|(_, p)| **p == t.as_str()).nth(*k)?.0;
                    *k += 1;
                    idx
                };
                let v = vals.get(idx)?;
                out.push_str(lookup_exact(v).unwrap_or(v));
            }
        }
    }
    Some(out)
}

struct Template {
    ja: Vec<Seg>,
    en: Vec<Seg>,
    /// How much fixed text the key has: a longer one is tried first, a key with almost none is never matched freely.
    lit_len: usize,
}

#[derive(Default)]
struct Table {
    exact: HashMap<String, String>,
    templates: Vec<Template>,
    by_key: HashMap<String, usize>,
    /// The templates tried by `tr_dyn`, the one with the most fixed text first.
    free_order: Vec<usize>,
}

const TABLES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/tables.rs"));

/// One line of a table: `\n`, `\t` and `\\` are escapes. An empty English side means "not translated yet".
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The entries of one table file: (line number, Japanese, English). Comments (`#`), blank lines and untranslated keys are left out.
pub fn parse_table(text: &str) -> Vec<(usize, String, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|(i, l)| {
            let (ja, en) = l.split_once('\t')?;
            let en = en.trim_end_matches('\r');
            (!en.trim().is_empty()).then(|| (i + 1, unescape(ja), unescape(en)))
        })
        .collect()
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Table::default();
        for (_, text) in TABLES {
            for (_, ja, en) in parse_table(text) {
                if let (Some(js), Some(es)) = (parse_format(&ja), parse_format(&en))
                    && has_placeholder(&js)
                    && !t.by_key.contains_key(&ja)
                {
                    let (js, es) = merge_runs(js, es);
                    let lit_len = js.iter().map(|s| if let Seg::Lit(l) = s { l.chars().count() } else { 0 }).sum();
                    t.by_key.insert(ja.clone(), t.templates.len());
                    t.templates.push(Template { ja: js, en: es, lit_len });
                }
                t.exact.entry(ja).or_insert(en);
            }
        }
        // the longest fixed text first, so "{}個の部品を消しました" wins over "{}個"
        t.free_order = (0..t.templates.len()).filter(|&i| t.templates[i].lit_len >= 2).collect();
        t.free_order.sort_by_key(|&i| std::cmp::Reverse(t.templates[i].lit_len));
        t
    })
}

fn lookup_exact(s: &str) -> Option<&'static str> {
    table().exact.get(s).map(|e| e.as_str())
}

fn has_japanese(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3040..=0x30FF | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF))
}

static MISSING: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);

/// Starts noting the Japanese texts that had no English (`missing()` gives them). Off by default.
pub fn record_missing() {
    *MISSING.lock().unwrap() = Some(BTreeSet::new());
}

pub fn missing() -> Vec<String> {
    MISSING.lock().unwrap().as_ref().map(|s| s.iter().cloned().collect()).unwrap_or_default()
}

fn note_missing(s: &str) {
    if has_japanese(s)
        && let Some(set) = MISSING.lock().unwrap().as_mut()
        && set.len() < 100_000
    {
        set.insert(s.to_string());
    }
}

/// A fixed text in the chosen language (the Japanese itself when it has no entry).
pub fn tr(s: &str) -> &str {
    if is_ja() {
        return s;
    }
    match lookup_exact(s) {
        Some(e) => e,
        None => {
            note_missing(s);
            s
        }
    }
}

/// A fixed text whose English depends on where it is (配線 is a wire on the schematic and a track on the board): the
/// table key `場所|日本語` is tried first, then the text alone.
pub fn tr_ctx<'a>(place: &str, s: &'a str) -> &'a str {
    if is_ja() {
        return s;
    }
    lookup_exact(&format!("{place}|{s}")).unwrap_or_else(|| tr(s))
}

/// A text made by `format!(fmt, …)` in Japanese, shown in the chosen language. Used by `trf!`.
pub fn from_template(fmt: &str, ja: String) -> String {
    if is_ja() {
        return ja;
    }
    let t = table();
    if let Some(&i) = t.by_key.get(fmt) {
        let tp = &t.templates[i];
        if let Some(en) = capture(&tp.ja, &ja).and_then(|v| fill(&tp.ja, &tp.en, &v)) {
            return en;
        }
    } else if let Some(e) = lookup_exact(&ja) {
        return e.to_string();
    }
    note_missing(fmt);
    ja
}

/// Any text, made anywhere: looked up as it is, else matched against the formatted keys. The answers are kept, as the same
/// text is usually drawn every frame.
pub fn tr_dyn(s: &str) -> String {
    if is_ja() || !has_japanese(s) {
        return s.to_string();
    }
    if let Some(e) = lookup_exact(s) {
        return e.to_string();
    }
    static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
    if let Some(hit) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(s).cloned()) {
        return hit.unwrap_or_else(|| s.to_string());
    }
    let t = table();
    let found = t
        .free_order
        .iter()
        .map(|&i| &t.templates[i])
        .find_map(|t| capture(&t.ja, s).and_then(|v| fill(&t.ja, &t.en, &v)))
        .or_else(|| tr_marked(s))
        .or_else(|| tr_lines(s));
    if found.is_none() {
        note_missing(s);
    }
    let mut cache = CACHE.lock().unwrap();
    let c = cache.get_or_insert_with(HashMap::new);
    if c.len() > 20_000 {
        c.clear();
    }
    c.insert(s.to_string(), found.clone());
    found.unwrap_or_else(|| s.to_string())
}

/// A text with a mark put in front of a known one ("⚠ ① 基本", "● 基板パターン"): the mark is kept and the rest translated.
fn tr_marked(s: &str) -> Option<String> {
    s.char_indices().filter(|&(i, c)| c == ' ' && i <= 12).find_map(|(i, _)| {
        let (mark, rest) = (&s[..=i], &s[i + 1..]);
        (!has_japanese(mark) && !rest.is_empty()).then(|| lookup_exact(rest).map(|e| format!("{mark}{e}"))).flatten()
    })
}

/// A text of several lines translated line by line, when every line with Japanese has an entry.
fn tr_lines(s: &str) -> Option<String> {
    if !s.contains('\n') {
        return None;
    }
    let lines: Vec<String> = s.split('\n').map(|l| if has_japanese(l) { Some(tr_dyn(l)).filter(|e| e != l) } else { Some(l.to_string()) }).collect::<Option<_>>()?;
    Some(lines.join("\n"))
}

/// `format!` whose result is shown in the chosen language: `trf!("部品が{n}個", n = 3)`.
#[macro_export]
macro_rules! trf {
    ($fmt:literal $($rest:tt)*) => {
        $crate::from_template($fmt, format!($fmt $($rest)*))
    };
}

/// A problem of one table entry (for the test that checks every table, and the tool that lists them).
pub fn check_entry(ja: &str, en: &str) -> Option<String> {
    let (Some(js), Some(es)) = (parse_format(ja), parse_format(en)) else {
        return Some("波かっこ { } の対応がとれていません".into());
    };
    let jp: Vec<&str> = js.iter().filter_map(|s| if let Seg::Ph(t) = s { Some(t.as_str()) } else { None }).collect();
    let ep: Vec<&str> = es.iter().filter_map(|s| if let Seg::Ph(t) = s { Some(t.as_str()) } else { None }).collect();
    for p in &ep {
        if let Some(n) = p.strip_prefix('@') {
            if n.parse::<usize>().ok().filter(|&n| n >= 1 && n <= jp.len()).is_none() {
                return Some(format!("{{{p}}} の番号が日本語の置き場所の数 ({}) を超えています", jp.len()));
            }
        } else if ep.iter().filter(|q| *q == p).count() > jp.iter().filter(|q| *q == p).count() {
            return Some(format!("英語の {{{p}}} が日本語にありません"));
        }
    }
    let has_at = ep.iter().any(|p| p.starts_with('@'));
    if !has_at {
        for p in &jp {
            if jp.iter().filter(|q| *q == p).count() != ep.iter().filter(|q| *q == p).count() {
                return Some(format!("日本語の {{{p}}} が英語にありません"));
            }
        }
    }
    None
}

/// Every table file with its entries, for tools and tests.
pub fn tables() -> impl Iterator<Item = (&'static str, &'static str)> {
    TABLES.iter().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_and_fill() {
        let ja = parse_format("部品が{n}個、ネットが{}本").unwrap();
        let en = parse_format("{n} parts and {} nets").unwrap();
        let v = capture(&ja, "部品が12個、ネットが3本").unwrap();
        assert_eq!(v, vec!["12", "3"]);
        assert_eq!(fill(&ja, &en, &v).unwrap(), "12 parts and 3 nets");
        let en2 = parse_format("{@2} nets, {@1} parts").unwrap();
        assert_eq!(fill(&ja, &en2, &v).unwrap(), "3 nets, 12 parts");
        assert!(capture(&ja, "部品が12個").is_none());
        let side = parse_format("{a}{b}だけ").unwrap();
        assert!(capture(&side, "12だけ").is_none(), "two placeholders side by side are ambiguous");
        // ...unless the English keeps them side by side in the same order
        let (j2, e2) = merge_runs(parse_format("外: {}{}").unwrap(), parse_format("Outside: {}{}").unwrap());
        let v = capture(&j2, "外: D1, D2 ほか").unwrap();
        assert_eq!(fill(&j2, &e2, &v).unwrap(), "Outside: D1, D2 ほか");
        let esc = parse_format("{{x}} は {v:.2} V").unwrap();
        assert_eq!(capture(&esc, "{x} は 1.50 V").unwrap(), vec!["1.50"]);
    }

    #[test]
    fn entries_are_consistent() {
        assert_eq!(check_entry("{n}個", "{n} pcs"), None);
        assert!(check_entry("{n}個", "pcs").is_some());
        assert!(check_entry("{n}個", "{m} pcs").is_some());
        assert_eq!(check_entry("{}と{}", "{@2} and {@1}"), None);
        let mut bad = vec![];
        let mut seen: HashMap<String, (String, &str, usize)> = HashMap::new();
        for (file, text) in tables() {
            for (line, ja, en) in parse_table(text) {
                if let Some(p) = check_entry(&ja, &en) {
                    bad.push(format!("{file}:{line}: {p}: {ja}"));
                }
                if let Some((e0, f0, l0)) = seen.get(&ja)
                    && *e0 != en
                {
                    bad.push(format!("{file}:{line}: 同じ日本語に別の英語 ({f0}:{l0}): {ja}"));
                }
                seen.entry(ja).or_insert((en, file, line));
            }
        }
        assert!(bad.is_empty(), "表の行に問題があります:\n{}", bad.join("\n"));
    }

    #[test]
    fn english_mode_uses_the_tables() {
        set_lang(Lang::En);
        assert_eq!(tr("ファイル"), "File");
        assert_eq!(tr("表に無い文"), "表に無い文", "no entry: the Japanese stays");
        // a formatted key, through the macro and found again from the finished text
        let tpl = table().templates.iter().find(|t| t.ja.iter().filter(|s| matches!(s, Seg::Ph(_))).count() == 1 && t.lit_len >= 6);
        if let Some(t) = tpl {
            let ja: String = t.ja.iter().map(|s| if let Seg::Lit(l) = s { l.as_str() } else { "42" }).collect();
            let en = tr_dyn(&ja);
            assert_ne!(en, ja, "{ja}");
            assert!(en.contains("42"), "{en}");
        }
        set_lang(Lang::Ja);
    }

    #[test]
    fn japanese_mode_changes_nothing() {
        // the language is global; this test only reads it in its default state
        if is_ja() {
            assert_eq!(tr("ファイル"), "ファイル");
            assert_eq!(trf!("{}個", 3), "3個");
            assert_eq!(tr_dyn("何か"), "何か");
        }
    }
}
