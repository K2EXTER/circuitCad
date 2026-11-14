//! Words of a page and a search over them.

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask,
    interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{BezPath, Point, Rect as KRect};

/// A rectangle on a page in points, origin at the top left of the page as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
}

/// A run of letters with no gap, and where it is on the page.
#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub rect: Rect,
    /// Where the letters sit on the line (points from the top), to tell which words share a line.
    baseline: f32,
    /// Font size (points).
    pub(crate) em: f32,
    /// Where each character of `text` starts (points from the left of the page).
    xs: Vec<f32>,
}

/// Characters that end a name inside a word (Japanese data sheets run a list of part numbers together).
fn is_separator(c: char) -> bool {
    c.is_whitespace() || ",;/()[]{}<>:|\u{3001}\u{3002}\u{FF0C}\u{FF1B}\u{FF0F}\u{FF08}\u{FF09}\u{300C}\u{300D}\u{30FB}".contains(c)
}

impl Word {
    /// The piece of the word at horizontal place `x` between separators: the part number under the pointer in
    /// "LM393B,LM2903B,LM193". None when the place is on a separator.
    pub fn token_at(&self, x: f32) -> Option<&str> {
        self.token_span_at(x).map(|t| t.0)
    }

    /// [`Word::token_at`] with where the token starts and ends on the line (points from the left of the page): a tick
    /// label of a graph is placed by its own centre, not the centre of the run of labels it was read with.
    pub fn token_span_at(&self, x: f32) -> Option<(&str, f32, f32)> {
        let chars: Vec<(usize, char)> = self.text.char_indices().collect();
        let n = chars.len();
        if n == 0 || self.xs.len() != n {
            return Some((self.text.as_str(), self.rect.x0, self.rect.x1));
        }
        let i = self.xs.iter().rposition(|&s| s <= x).unwrap_or(0);
        if is_separator(chars[i].1) {
            return None;
        }
        let (mut a, mut b) = (i, i);
        while a > 0 && !is_separator(chars[a - 1].1) {
            a -= 1;
        }
        while b + 1 < n && !is_separator(chars[b + 1].1) {
            b += 1;
        }
        let end = chars.get(b + 1).map_or(self.text.len(), |c| c.0);
        let x1 = self.xs.get(b + 1).copied().unwrap_or(self.rect.x1);
        Some((&self.text[chars[a].0..end], self.xs[a], x1))
    }

    pub(crate) fn baseline(&self) -> f32 {
        self.baseline
    }

    pub(crate) fn em(&self) -> f32 {
        self.em
    }

    /// The pieces of the word between separators.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.text.split(is_separator).filter(|t| !t.is_empty())
    }
}

/// One letter as the page draws it.
struct Letter {
    text: String,
    x: f64,
    baseline: f64,
    em: f64,
    advance: f64,
}

#[derive(Default)]
struct Collector {
    letters: Vec<Letter>,
}

impl Device<'_> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'_>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, '_>, props: DrawProps<'_>, _: &DrawMode) {
        for g in run.glyphs() {
            let t = props.transform * g.transform();
            let p = t * Point::ZERO;
            // the glyph space is 1/1000 of the font size
            let unit = ((t * Point::new(0.0, 1.0)) - p).hypot() * 1000.0;
            let adv = match &**g {
                Glyph::Outline(o) => o.advance_width().map(f64::from),
                Glyph::Type3(_) => None,
            };
            let advance = match adv {
                Some(a) => ((t * Point::new(a, 0.0)) - p).hypot(),
                None => unit * 0.5,
            };
            let text = match g.as_unicode() {
                Some(BfString::Char(c)) => c.to_string(),
                Some(BfString::String(s)) => s,
                None => continue,
            };
            self.letters.push(Letter { text, x: p.x, baseline: p.y, em: unit, advance });
        }
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_image(&mut self, _: Image<'_, '_>, _: ImageDrawProps<'_>) {}
}

/// Reads the words of a page.
pub(crate) fn read_words(page: &Page<'_>) -> Vec<Word> {
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(
        page.initial_transform(true).to_kurbo(),
        KRect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut c = Collector::default();
    interpret_page(page, &mut ctx, &mut c);
    group(&c.letters)
}

/// Joins letters that follow each other on one baseline into words.
fn group(letters: &[Letter]) -> Vec<Word> {
    let mut out: Vec<Word> = vec![];
    // the open word and the right edge of its last letter
    let mut cur: Option<(Word, f64)> = None;
    for l in letters {
        if l.text.chars().all(|c| c.is_whitespace()) {
            out.extend(cur.take().map(|(w, _)| w));
            continue;
        }
        let em = l.em.max(1.0);
        let joins = cur.as_ref().is_some_and(|(w, right)| {
            let same_line = (l.baseline - f64::from(w.baseline)).abs() <= 0.3 * em;
            let gap = l.x - right;
            same_line && (-0.6 * em..=0.2 * em).contains(&gap)
        });
        if !joins {
            out.extend(cur.take().map(|(w, _)| w));
        }
        let rect = Rect { x0: l.x as f32, y0: (l.baseline - 0.8 * em) as f32, x1: (l.x + l.advance) as f32, y1: (l.baseline + 0.2 * em) as f32 };
        // a letter that is several characters (a ligature) spreads them over its width
        let count = l.text.chars().count().max(1);
        let starts = (0..count).map(|k| (l.x + l.advance * k as f64 / count as f64) as f32);
        match cur.as_mut() {
            Some((w, right)) => {
                w.text.push_str(&l.text);
                w.xs.extend(starts);
                w.rect = w.rect.union(rect);
                *right = l.x + l.advance;
            }
            None => cur = Some((Word { text: l.text.clone(), rect, baseline: l.baseline as f32, em: em as f32, xs: starts.collect() }, l.x + l.advance)),
        }
    }
    out.extend(cur.take().map(|(w, _)| w));
    out
}

/// Lower case, full width letters to half width, ligatures and the many dashes to plain ones, so that a search
/// finds "fine" written with the one-glyph "fi" ligature, and a full width "Pin" by "pin".
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{FF01}'..='\u{FF5E}' => out.extend(char::from_u32(c as u32 - 0xFEE0).unwrap_or(c).to_lowercase()),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => out.push('-'),
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{00A0}' | '\u{3000}' => out.push(' '),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// What to look for: one or more words, which must follow each other on a page.
pub struct Query {
    terms: Vec<String>,
}

impl Query {
    /// None for a query with nothing in it.
    pub fn new(text: &str) -> Option<Query> {
        let terms: Vec<String> = normalize(text).split_whitespace().map(String::from).collect();
        if terms.is_empty() { None } else { Some(Query { terms }) }
    }
}

/// A place where the query was found.
#[derive(Clone, Debug)]
pub struct Hit {
    pub page: usize,
    /// The found words (the first to the last, when several).
    pub rect: Rect,
    pub text: String,
    /// The line around it: the words on the same baseline, left to right, cut to a readable length.
    pub context: String,
    /// The line is a row of a table of contents (it runs on in dots to a page number), not the heading itself.
    pub in_contents: bool,
}

pub(crate) fn search(page: usize, words: &[Word], q: &Query) -> Vec<Hit> {
    let norm: Vec<String> = words.iter().map(|w| normalize(&w.text)).collect();
    let n = q.terms.len();
    let mut hits = vec![];
    for i in 0..(words.len() + 1).saturating_sub(n) {
        // a single term may be anywhere in its word ("SOIC" in "SOIC-8"); with several, the first may only end
        // with its term and the last may only start with its term
        let ok = (0..n).all(|k| {
            let w = &norm[i + k];
            let t = q.terms[k].as_str();
            match (n, k) {
                (1, _) => w.contains(t),
                (_, 0) => w.ends_with(t),
                (_, k) if k == n - 1 => w.starts_with(t),
                _ => w == t,
            }
        });
        if !ok {
            continue;
        }
        let found = &words[i..i + n];
        let rect = found.iter().skip(1).fold(found[0].rect, |r, w| r.union(w.rect));
        let text = found.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        let (context, in_contents) = line_around(words, &words[i]);
        hits.push(Hit { page, rect, text, context, in_contents });
    }
    hits
}

fn line_around(words: &[Word], at: &Word) -> (String, bool) {
    let mut row: Vec<&Word> = words.iter().filter(|w| (w.baseline - at.baseline).abs() <= 0.3 * at.em.max(1.0)).collect();
    row.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    let line = row.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    // dot leaders: "....", or the same in the ellipsis or the Japanese middle dot
    let in_contents = ["....", "……", "・・・・"].iter().any(|d| line.contains(d));
    let chars: Vec<char> = line.chars().collect();
    const SHOWN: usize = 70;
    if chars.len() <= SHOWN {
        return (line, in_contents);
    }
    let before: usize = row.iter().take_while(|w| w.rect.x0 < at.rect.x0).map(|w| w.text.chars().count() + 1).sum();
    let from = before.saturating_sub(25).min(chars.len() - SHOWN);
    let cut: String = chars[from..from + SHOWN].iter().collect();
    (format!("{}{cut}{}", if from > 0 { "…" } else { "" }, if from + SHOWN < chars.len() { "…" } else { "" }), in_contents)
}

/// The unit written right after `at` on its line ("0.65" then "mm"), if there is one.
pub(crate) fn unit_after(words: &[Word], at: &Word) -> Option<String> {
    let em = at.em.max(1.0);
    words
        .iter()
        .filter(|w| (w.baseline - at.baseline).abs() <= 0.3 * em && w.rect.x0 >= at.rect.x1 - 1.0 && w.rect.x0 - at.rect.x1 <= 2.0 * em)
        .min_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0))
        .filter(|w| crate::value::is_unit_word(&w.text))
        .map(|w| w.text.trim_matches(['(', ')', '[', ']']).to_string())
}
