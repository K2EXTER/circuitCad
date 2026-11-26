//! The sketch's text as code: its words in colours (comments, strings, numbers, types, Arduino's functions), its lines numbered
//! with the line a mistake is on marked, and set out again by its braces (`format`). Used by the sketch box and its own window.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};

/// What a piece of the text is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Plain,
    Comment,
    Text,
    Number,
    /// `if`, `for`, `return`, `HIGH`, `OUTPUT`…
    Keyword,
    /// `int`, `float`, `byte`…
    Type,
    /// A name called (`digitalWrite(`), Arduino's own in its own colour.
    Call,
    Arduino,
    /// `#include` and the rest of its line.
    Directive,
}

const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "return", "break", "continue", "switch", "case", "default", "const", "static", "volatile", "true",
    "false", "HIGH", "LOW", "INPUT", "OUTPUT", "INPUT_PULLUP", "LED_BUILTIN", "CHANGE", "RISING", "FALLING", "DEC", "HEX", "BIN",
    "sizeof", "struct", "enum", "new", "delete", "class", "public", "private", "nullptr", "NULL",
];
const TYPES: &[&str] = &[
    "void", "int", "long", "short", "unsigned", "signed", "float", "double", "bool", "boolean", "byte", "char", "word", "String",
    "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t", "int64_t", "size_t",
];
const ARDUINO: &[&str] = &[
    "setup", "loop", "pinMode", "digitalWrite", "digitalRead", "analogRead", "analogWrite", "analogReference", "delay",
    "delayMicroseconds", "millis", "micros", "map", "constrain", "random", "randomSeed", "tone", "noTone", "pulseIn", "shiftOut",
    "shiftIn", "attachInterrupt", "detachInterrupt", "digitalPinToInterrupt", "Serial", "Wire", "SPI", "EEPROM", "abs", "min", "max",
    "sq", "sqrt", "pow", "sin", "cos", "bitRead", "bitWrite", "bitSet", "bitClear", "highByte", "lowByte",
];

/// The pieces of `text` and what each is.
fn pieces(text: &str) -> Vec<(std::ops::Range<usize>, Kind)> {
    let b = text.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let start = i;
        let c = b[i];
        let kind = if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            Kind::Comment
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            Kind::Comment
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            Kind::Text
        } else if c == b'#' && text[..i].rsplit('\n').next().is_some_and(|l| l.trim().is_empty()) {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            Kind::Directive
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            Kind::Number
        } else if ident(c) {
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
            let word = &text[start..i];
            let called = text[i..].trim_start_matches([' ', '\t']).starts_with('(');
            if TYPES.contains(&word) {
                Kind::Type
            } else if KEYWORDS.contains(&word) || word.len() > 1 && word.starts_with('A') && word[1..].bytes().all(|d| d.is_ascii_digit()) {
                Kind::Keyword
            } else if ARDUINO.contains(&word) {
                Kind::Arduino
            } else if called {
                Kind::Call
            } else {
                Kind::Plain
            }
        } else {
            // a character of its own (whole, not a byte of one)
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            Kind::Plain
        };
        out.push((start..i, kind));
    }
    out
}

fn colour(kind: Kind, dark: bool) -> Color32 {
    let pick = |d: (u8, u8, u8), l: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { l };
        Color32::from_rgb(r, g, b)
    };
    match kind {
        Kind::Plain => pick((220, 220, 220), (30, 30, 30)),
        Kind::Comment => pick((120, 150, 120), (90, 120, 90)),
        Kind::Text => pick((230, 160, 110), (170, 70, 20)),
        Kind::Number => pick((180, 220, 150), (30, 120, 60)),
        Kind::Keyword => pick((200, 150, 230), (130, 40, 160)),
        Kind::Type => pick((110, 190, 230), (20, 100, 170)),
        Kind::Call => pick((230, 220, 150), (120, 100, 0)),
        Kind::Arduino => pick((240, 190, 90), (180, 90, 0)),
        Kind::Directive => pick((150, 170, 230), (60, 80, 170)),
    }
}

/// The text laid out in its colours, the line `marked` (from 1: a mistake) on a red ground; `wrap` the width it wraps at.
pub fn layout(text: &str, font: FontId, dark: bool, marked: Option<usize>, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    // where each line starts, to mark one
    let marked_range = marked.and_then(|n| {
        let start = text.split_inclusive('\n').take(n.saturating_sub(1)).map(str::len).sum::<usize>();
        let len = text[start.min(text.len())..].split('\n').next()?.len();
        (n >= 1 && start <= text.len()).then_some(start..start + len)
    });
    let mark = if dark { Color32::from_rgb(90, 30, 30) } else { Color32::from_rgb(255, 215, 215) };
    for (r, kind) in pieces(text) {
        // a piece split where the marked line starts and ends
        let mut cuts = vec![r.start, r.end];
        if let Some(m) = &marked_range {
            cuts.extend([m.start, m.end].into_iter().filter(|c| *c > r.start && *c < r.end));
        }
        cuts.sort_unstable();
        for w in cuts.windows(2) {
            let bg = if marked_range.as_ref().is_some_and(|m| w[0] >= m.start && w[1] <= m.end && m.start < m.end) { mark } else { Color32::TRANSPARENT };
            job.append(&text[w[0]..w[1]], 0.0, TextFormat { font_id: font.clone(), color: colour(kind, dark), background: bg, ..Default::default() });
        }
    }
    job
}

/// The sketch set out again by its braces: two spaces a level, a line starting with `}` a level out, the lines of a block
/// comment and of `#` directives as they were (a directive at the left edge); the spaces at the ends of lines gone, two empty
/// lines at most in a row, one line end at the end. Only the space at the start of a line changes: the words stay as written.
pub fn format(text: &str) -> String {
    let mut out = String::new();
    let mut depth: i32 = 0;
    let mut in_comment = false;
    let mut empties = 0;
    for line in text.replace('\t', "  ").lines() {
        let t = line.trim();
        if in_comment {
            // the inside of a block comment as written (only its ends trimmed)
            out.push_str(line.trim_end());
            out.push('\n');
            in_comment = !t.contains("*/");
            continue;
        }
        if t.is_empty() {
            empties += 1;
            if empties <= 2 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        empties = 0;
        // the braces outside strings and comments, and whether a block comment is left open
        let (mut opens, mut closes_first, mut closes) = (0, 0, 0);
        let mut seen_other = false;
        let bytes = t.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'/' if bytes.get(i + 1) == Some(&b'/') => break,
                b'/' if bytes.get(i + 1) == Some(&b'*') => match t[i + 2..].find("*/") {
                    Some(k) => i += k + 3,
                    None => {
                        in_comment = true;
                        break;
                    }
                },
                q @ (b'"' | b'\'') => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != q {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'{' => {
                    opens += 1;
                    seen_other = true;
                }
                b'}' => {
                    if opens > 0 {
                        opens -= 1;
                    } else if !seen_other {
                        closes_first += 1;
                    } else {
                        closes += 1;
                    }
                }
                b' ' => {}
                _ => seen_other = true,
            }
            i += 1;
        }
        let level = if t.starts_with('#') { 0 } else { (depth - closes_first).max(0) };
        for _ in 0..level {
            out.push_str("  ");
        }
        out.push_str(t);
        out.push('\n');
        depth = (depth - closes_first - closes + opens).max(0);
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// The sketch's text box: its words in colours, the line `marked` on a red ground. `rows` high, as wide as given.
pub fn code_box(ui: &mut egui::Ui, text: &mut String, rows: usize, width: f32, marked: Option<usize>) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| ui.fonts_mut(|f| f.layout_job(layout(buf.as_str(), font.clone(), dark, marked, wrap)));
    ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(rows).desired_width(width).layouter(&mut layouter))
}

/// The same with the line numbers beside it, in a box that scrolls both ways (the lines are not wrapped): the sketch's window.
pub fn numbered_box(ui: &mut egui::Ui, text: &mut String, marked: Option<usize>) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let lines = text.lines().count().max(1) + usize::from(text.ends_with('\n'));
    let rows = ((ui.available_height() / ui.text_style_height(&egui::TextStyle::Monospace)) as usize).max(10);
    let numbers: String = (1..=lines).map(|n| format!("{n:>3}")).collect::<Vec<_>>().join("\n");
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(numbers).font(font.clone()).color(Color32::GRAY));
                let mut layouter =
                    |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| ui.fonts_mut(|f| f.layout_job(layout(buf.as_str(), font.clone(), dark, marked, f32::INFINITY)));
                ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(rows).desired_width(f32::INFINITY).layouter(&mut layouter))
            })
            .inner
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_get_their_kinds() {
        let text = "#include <Wire.h>\n// on\nint n = 0x1F; // x\nvoid loop() { digitalWrite(13, HIGH); go(); Serial.println(\"a{\"); n = A0; }";
        let of = |word: &str| pieces(text).into_iter().find(|(r, _)| &text[r.clone()] == word).map(|(_, k)| k);
        assert_eq!(of("#include <Wire.h>"), Some(Kind::Directive));
        assert_eq!(of("// on"), Some(Kind::Comment));
        assert_eq!(of("int"), Some(Kind::Type));
        assert_eq!(of("0x1F"), Some(Kind::Number));
        assert_eq!(of("digitalWrite"), Some(Kind::Arduino));
        assert_eq!(of("go"), Some(Kind::Call));
        assert_eq!(of("HIGH"), Some(Kind::Keyword));
        assert_eq!(of("A0"), Some(Kind::Keyword));
        assert_eq!(of("\"a{\""), Some(Kind::Text));
        assert_eq!(of("n"), Some(Kind::Plain));
        // every byte is in exactly one piece, Japanese too
        let jp = "// 点滅\nint x;";
        let p = pieces(jp);
        assert_eq!(p.first().unwrap().0.start, 0);
        assert_eq!(p.last().unwrap().0.end, jp.len());
        assert!(p.windows(2).all(|w| w[0].0.end == w[1].0.start));
    }

    #[test]
    fn the_marked_line_is_on_its_own_ground() {
        let job = layout("int a;\nint b;\n", FontId::monospace(14.0), true, Some(2), f32::INFINITY);
        let marked: String = job.sections.iter().filter(|s| s.format.background != Color32::TRANSPARENT).map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
        assert_eq!(marked, "int b;");
    }

    #[test]
    fn format_sets_out_the_lines_by_their_braces() {
        let messy = "#include <Wire.h>\nvoid setup() {\npinMode(13, OUTPUT);   \n    }\n\n\n\n\nvoid loop() {\n\tif (x) {\ndigitalWrite(13, HIGH); // {\n  } else { y(); }\nSerial.print(\"}\");\n/* a\n   comment { */\n}\n\n";
        let want = "#include <Wire.h>\nvoid setup() {\n  pinMode(13, OUTPUT);\n}\n\n\nvoid loop() {\n  if (x) {\n    digitalWrite(13, HIGH); // {\n  } else { y(); }\n  Serial.print(\"}\");\n  /* a\n   comment { */\n}\n";
        assert_eq!(format(messy), want);
        // once set out, it stays so
        assert_eq!(format(want), want);
    }
}
