//! A pin table cut out of a page: the words inside a rectangle become rows and columns, and two of the columns
//! (the pin number and the pin name) become the pins of a part. The columns are guessed, and a person can change them.

use crate::text::{Rect, Word};

/// Rows of cells; every row has `columns` cells ("" where a row has nothing in a column).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Table {
    pub cells: Vec<Vec<String>>,
    pub columns: usize,
}

struct Cell {
    text: String,
    x0: f32,
    x1: f32,
}

/// Reads the words whose middle is inside `area` as a table. Words close together on a line are one cell; the
/// columns are the stretches of the page across which cells of some row lie.
pub fn read_table(words: &[Word], area: Rect) -> Table {
    let mut inside: Vec<&Word> = words
        .iter()
        .filter(|w| {
            let (cx, cy) = ((w.rect.x0 + w.rect.x1) / 2.0, (w.rect.y0 + w.rect.y1) / 2.0);
            area.x0 <= cx && cx <= area.x1 && area.y0 <= cy && cy <= area.y1
        })
        .collect();
    inside.sort_by(|a, b| a.baseline().total_cmp(&b.baseline()).then(a.rect.x0.total_cmp(&b.rect.x0)));
    // rows: words whose baselines are close
    let mut rows: Vec<Vec<&Word>> = vec![];
    for w in inside {
        match rows.last_mut() {
            Some(r) if (w.baseline() - r[0].baseline()).abs() <= 0.5 * w.em().max(r[0].em()).max(1.0) => r.push(w),
            _ => rows.push(vec![w]),
        }
    }
    // cells: words with a small gap between them
    let row_cells: Vec<Vec<Cell>> = rows
        .into_iter()
        .map(|mut r| {
            r.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
            let mut cells: Vec<Cell> = vec![];
            for w in r {
                match cells.last_mut() {
                    Some(c) if w.rect.x0 - c.x1 <= 0.6 * w.em().max(1.0) => {
                        c.text.push(' ');
                        c.text.push_str(&w.text);
                        c.x1 = c.x1.max(w.rect.x1);
                    }
                    _ => cells.push(Cell { text: w.text.clone(), x0: w.rect.x0, x1: w.rect.x1 }),
                }
            }
            cells
        })
        .collect();
    // columns: the stretches where the cells of the body rows lie, merged when they overlap. Only rows with many
    // cells count: a caption or a heading that lies across several columns would join them.
    let most = row_cells.iter().map(Vec::len).max().unwrap_or(0);
    let dense = (most * 6).div_ceil(10).max(2).min(most);
    let mut spans: Vec<(f32, f32)> = row_cells.iter().filter(|r| r.len() >= dense).flatten().map(|c| (c.x0, c.x1)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut columns: Vec<(f32, f32)> = vec![];
    for (x0, x1) in spans {
        match columns.last_mut() {
            Some(c) if x0 <= c.1 => c.1 = c.1.max(x1),
            _ => columns.push((x0, x1)),
        }
    }
    let mut cells: Vec<Vec<String>> = vec![];
    for r in &row_cells {
        let mut line = vec![String::new(); columns.len()];
        for c in r {
            let mid = (c.x0 + c.x1) / 2.0;
            // the column the middle of the cell is in, or the nearest one (a heading cell lies between)
            let i = (0..columns.len())
                .min_by(|&a, &b| distance(columns[a], mid).total_cmp(&distance(columns[b], mid)))
                .unwrap_or(0);
            if !line[i].is_empty() {
                line[i].push(' ');
            }
            line[i].push_str(&c.text);
        }
        cells.push(line);
    }
    Table { cells, columns: columns.len() }
}

/// How far `x` is from the stretch (0 inside it).
fn distance((a, b): (f32, f32), x: f32) -> f32 {
    if x < a { a - x } else if x > b { x - b } else { 0.0 }
}

/// "1", "12", "A3", "5-8", "2, 3": what a pin number cell looks like.
pub fn is_pin_number(s: &str) -> bool {
    !expand_numbers(s).is_empty() && s.split([' ', ',', '/', '\u{3001}']).filter(|t| !t.is_empty()).all(|t| single_number(t) || range(t).is_some())
}

/// "A3" or "12": up to two letters, then 1 to 3 digits.
fn single_number(t: &str) -> bool {
    let letters = t.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let digits = &t[letters..];
    letters <= 2 && (1..=3).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit())
}

/// "5-8" -> (5, 8).
fn range(t: &str) -> Option<(u32, u32)> {
    let (a, b) = t.split_once(['-', '\u{2013}', '\u{2212}', '~'])?;
    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
    (a < b && b - a < 64).then_some((a, b))
}

/// The pin numbers a cell stands for: "1" -> [1]; "2, 3" -> [2, 3]; "5-8" -> [5, 6, 7, 8].
fn expand_numbers(s: &str) -> Vec<String> {
    let mut out = vec![];
    for t in s.split([' ', ',', '/', '\u{3001}']).filter(|t| !t.is_empty()) {
        if single_number(t) {
            out.push(t.to_string());
        } else if let Some((a, b)) = range(t) {
            out.extend((a..=b).map(|n| n.to_string()));
        }
    }
    out
}

/// A pin name: one short word that starts with a letter or a digit and has a letter ("VCC", "1IN-", "RESET/GPIO0").
/// A cell of several words is a description.
fn is_pin_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 16
        && !s.chars().any(char::is_whitespace)
        && s.chars().any(|c| c.is_alphabetic())
        && s.chars().next().is_some_and(|c| c.is_alphanumeric())
        && !is_pin_number(s)
}

/// "1, 5, 8 NIC" -> ("1, 5, 8", "NIC"); "2 -IN" -> ("2", "-IN"): a cell that has the pin numbers and the name in it.
fn split_number_name(cell: &str) -> Option<(String, String)> {
    let tokens: Vec<&str> = cell.split_whitespace().collect();
    let n = tokens.iter().take_while(|t| is_pin_number(t.trim_end_matches(','))).count();
    if n == 0 || tokens.len() != n + 1 {
        return None;
    }
    // "-IN", "+IN" and "V-" are names though they do not start with a letter or a digit
    let name = tokens[n];
    (name.len() <= 16 && name.chars().any(|c| c.is_alphabetic())).then(|| (tokens[..n].join(" "), name.to_string()))
}

/// Which column holds the pin numbers and which the names: (number column, name column). Where the pin numbers and
/// the names are in one cell, both are that column. A column counts when most of its cells look right; of the
/// columns that count about equally, the leftmost wins (a table has the number of the main package first).
pub fn guess_columns(t: &Table) -> Option<(usize, usize)> {
    // (column, how many of its cells look right)
    let pick = |f: &dyn Fn(&str) -> bool, skip: Option<usize>| -> Option<(usize, usize)> {
        let scores: Vec<(usize, usize)> = (0..t.columns).filter(|c| Some(*c) != skip).map(|c| (c, t.cells.iter().filter(|r| f(&r[c])).count())).collect();
        let top = scores.iter().map(|s| s.1).max()?;
        if top < 2 {
            return None;
        }
        scores.iter().find(|s| s.1 * 10 >= top * 7).copied()
    };
    let both = pick(&|c| split_number_name(c).is_some(), None);
    let apart = pick(&is_pin_number, None).and_then(|num| pick(&is_pin_name, Some(num.0)).map(|name| (num, name)));
    match (apart, both) {
        // pieces of a drawing can look like two columns; a column of "number name" cells is the better evidence
        (Some((num, name)), Some(b)) if b.1 <= num.1.min(name.1) => Some((num.0, name.0)),
        (Some((num, name)), None) => Some((num.0, name.0)),
        (_, Some(b)) => Some((b.0, b.0)),
        (None, None) => None,
    }
}

/// The pins of the table: (number, name), in the order of the table, a number once. A row without a pin number or
/// without a name (a header, the second line of a wrapped description) is left out. With the same column for both,
/// each cell is split into numbers and name.
pub fn pins_from(t: &Table, num_col: usize, name_col: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for r in &t.cells {
        let (Some(num), Some(name)) = (r.get(num_col), r.get(name_col)) else { continue };
        let (num, name) = if num_col == name_col {
            let Some(pair) = split_number_name(num) else { continue };
            pair
        } else {
            let first = name.split_whitespace().next().unwrap_or("").to_string();
            (num.clone(), first)
        };
        let name = name.replace(['\u{2212}', '\u{2013}'], "-");
        let name = name.trim_matches([',', ';']);
        if name.is_empty() || !name.chars().any(|c| c.is_alphabetic()) || !is_pin_number(&num) {
            continue;
        }
        for n in expand_numbers(&num) {
            if !out.iter().any(|(m, _)| *m == n) {
                out.push((n, name.to_string()));
            }
        }
    }
    if out.iter().all(|(n, _)| n.chars().all(|c| c.is_ascii_digit())) {
        out.sort_by_key(|(n, _)| n.parse::<u32>().unwrap_or(0));
    }
    out
}

/// The pins as the form's pin box takes them: one "number name" per line.
pub fn pin_lines(pins: &[(String, String)]) -> String {
    pins.iter().map(|(n, name)| format!("{n} {name}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_pin_numbers_from_other_cells() {
        for ok in ["1", "12", "A3", "5-8", "2, 3", "2 3", "10/11"] {
            assert!(is_pin_number(ok), "{ok}");
        }
        for no in ["", "IN", "Pin", "VCC", "1234", "出力", "I/O", "SOIC (8)"] {
            assert!(!is_pin_number(no), "{no}");
        }
        assert_eq!(expand_numbers("5-8"), ["5", "6", "7", "8"]);
        assert_eq!(expand_numbers("2, 3"), ["2", "3"]);
    }

    #[test]
    fn tells_pin_names_from_other_cells() {
        for ok in ["VCC", "1IN-", "GND", "RESET/GPIO0", "V+"] {
            assert!(is_pin_name(ok), "{ok}");
        }
        for no in ["", "1", "12", "-", "OUT A", "Output of comparator 1"] {
            assert!(!is_pin_name(no), "{no}");
        }
    }

    fn table(rows: &[&[&str]]) -> Table {
        Table { cells: rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect(), columns: rows[0].len() }
    }

    #[test]
    fn guesses_the_columns_and_reads_the_pins() {
        // name | number (SOIC) | number (DSG) | I/O | description, with a header and a wrapped line
        let t = table(&[
            &["Name", "SOIC", "DSG", "I/O", "Description"],
            &["1OUT", "1", "1", "O", "Output of comparator 1"],
            &["1IN\u{2212}", "2", "2", "I", "Negative input"],
            &["GND", "4", "4", "-", "Ground"],
            &["", "", "", "", "second line of text"],
            &["VCC", "8", "8", "-", "Power"],
        ]);
        assert_eq!(guess_columns(&t), Some((1, 0)));
        let pins = pins_from(&t, 1, 0);
        assert_eq!(pins.iter().map(|(n, m)| format!("{n}:{m}")).collect::<Vec<_>>(), ["1:1OUT", "2:1IN-", "4:GND", "8:VCC"]);
        assert_eq!(pin_lines(&pins), "1 1OUT\n2 1IN-\n4 GND\n8 VCC");
        // the other number column and a column that is not names
        assert_eq!(pins_from(&t, 2, 0).len(), 4);
        assert!(pins_from(&t, 0, 4).is_empty());
    }

    #[test]
    fn number_first_tables_and_lists_of_numbers() {
        let t = table(&[&["Pin", "Name"], &["1", "VIN"], &["2, 3", "GND"], &["4-5", "NC"], &["6", "OUT"]]);
        assert_eq!(guess_columns(&t), Some((0, 1)));
        let pins = pins_from(&t, 0, 1);
        assert_eq!(pin_lines(&pins), "1 VIN\n2 GND\n3 GND\n4 NC\n5 NC\n6 OUT");
    }

    #[test]
    fn pin_number_and_name_in_one_cell() {
        let t = table(&[&["PIN NAME", "DESCRIPTION"], &["1, 5, 8 NIC", "Not internally connected"], &["2 \u{2212}IN", "Inverting input"], &["4 V-", "Negative supply"], &["6 OUT", "Output"]]);
        assert_eq!(guess_columns(&t), Some((0, 0)));
        assert_eq!(pin_lines(&pins_from(&t, 0, 0)), "1 NIC\n2 -IN\n4 V-\n5 NIC\n6 OUT\n8 NIC");
    }

    #[test]
    fn the_leftmost_of_several_package_columns_is_the_number() {
        // a later package column has a number in more rows (the first has blanks for the pins it does not have)
        let t = table(&[&["NAME", "D", "FK", "I/O"], &["1A", "1", "2", "I"], &["1Y", "2", "3", "O"], &["2A", "3", "4", "I"], &["2Y", "4", "6", "O"], &["NC", "", "1", "-"]]);
        assert_eq!(guess_columns(&t), Some((1, 0)));
    }

    #[test]
    fn no_pin_table_when_there_are_no_two_columns() {
        assert_eq!(guess_columns(&table(&[&["a b", "c d"], &["e f", "g h"]])), None);
        assert_eq!(guess_columns(&Table::default()), None);
    }
}
