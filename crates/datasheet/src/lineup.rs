//! Lineup tables: one row per part number, the way capacitor, inductor and resistor series list their values (Nichicon's
//! "Dimensions" table: capacitance, case size, tan δ, leakage, impedance, ripple current, part number). A row is a line
//! with one part number and at least three numbers; the columns are the places where most rows have a cell, and each
//! column is named by the header words above the first row. [`electrolytic`] reads an aluminium electrolytic capacitor's
//! values out of such rows (`ccad lineup`, the part form's lineup picker).

use crate::suggest::looks_like_part_number;
use crate::text::Word;

/// One page's lineup table.
#[derive(Clone, Debug, PartialEq)]
pub struct Lineup {
    /// From 0.
    pub page: usize,
    /// The header words of each column, top to bottom ("Rated Ripple （mArms） （105℃/100kHz）"); the part number's
    /// column is one of them.
    pub headers: Vec<String>,
    pub rows: Vec<LineupRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineupRow {
    /// The part number, "" for a table without them (Rubycon's: the part number is made from the values).
    pub part: String,
    /// One per header, "" where the row has nothing in that column.
    pub cells: Vec<String>,
    /// The number written once beside the row's group of rows (the rated voltage "6.3", "16 (1C)"), "" without.
    pub group: String,
}

/// A case size "8×11.5" (diameter × length, or width × length).
pub fn case_size(s: &str) -> Option<(f64, f64)> {
    let s = s.trim().trim_end_matches([',', ';']);
    let (a, b) = s.split_once(['×', 'x', 'X', '*'])?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn number(s: &str) -> Option<f64> {
    s.trim().trim_end_matches(['*', '※', ',']).replace(',', "").parse::<f64>().ok().filter(|v| v.is_finite())
}

fn is_cell_value(s: &str) -> bool {
    number(s).is_some() || case_size(s).is_some()
}

fn center(w: &Word) -> (f32, f32) {
    ((w.rect.x0 + w.rect.x1) / 2.0, (w.rect.y0 + w.rect.y1) / 2.0)
}

/// The words of a page grouped into lines (top to bottom, each left to right).
fn lines(words: &[Word]) -> Vec<Vec<&Word>> {
    let mut ws: Vec<&Word> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
    ws.sort_by(|a, b| center(a).1.total_cmp(&center(b).1));
    let mut out: Vec<Vec<&Word>> = vec![];
    let mut last = f32::MIN;
    for w in ws {
        let y = center(w).1;
        let tol = (w.em() * 0.35).clamp(1.5, 4.0);
        if (y - last).abs() > tol || out.is_empty() {
            out.push(vec![]);
            last = y;
        }
        out.last_mut().unwrap().push(w);
    }
    for l in &mut out {
        l.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    }
    out
}

fn is_row_part(t: &str) -> bool {
    t.len() >= 6 && looks_like_part_number(t) && case_size(t).is_none() && t.chars().any(|c| c.is_ascii_alphabetic())
}

/// A row found on a line: the line, its part number word (None in a table without them), its value words.
type Found<'a> = (usize, Option<&'a Word>, Vec<&'a Word>);

/// The lineup table of one page, if it has one (the first of `lineups_on`).
pub fn lineup_on(page: usize, words: &[Word]) -> Option<Lineup> {
    lineups_on(page, words).into_iter().next()
}

/// The lineup tables of one page (at least three rows each). Rows are lines with one part number and three or more
/// numbers; without such lines, a capacitor's table without part numbers: a row starts at a number followed by a case
/// size ("100  5×11  150  0.90  3.6"), so that two tables side by side (Rubycon's) are two rows on a line, and the
/// rows are sorted into tables by where they start.
pub fn lineups_on(page: usize, words: &[Word]) -> Vec<Lineup> {
    let ls = lines(words);
    let mut rows: Vec<Found> = vec![];
    for (i, l) in ls.iter().enumerate() {
        let parts: Vec<&&Word> = l.iter().filter(|w| is_row_part(w.text.trim())).collect();
        let values: Vec<&Word> = l.iter().copied().filter(|w| is_cell_value(&w.text)).collect();
        if parts.len() == 1 && values.len() >= 3 {
            rows.push((i, Some(parts[0]), values));
        }
    }
    if rows.len() >= 3 {
        return build(page, &ls, rows).into_iter().collect();
    }
    // segments: from a number right before a case size to the next such number
    let mut segs: Vec<Found> = vec![];
    for (i, l) in ls.iter().enumerate() {
        let starts: Vec<usize> = (0..l.len().saturating_sub(1)).filter(|&k| number(&l[k].text).is_some() && case_size(&l[k + 1].text).is_some()).collect();
        for (n, &k) in starts.iter().enumerate() {
            let end = starts.get(n + 1).copied().unwrap_or(l.len());
            let values: Vec<&Word> = l[k..end].iter().copied().filter(|w| is_cell_value(&w.text)).collect();
            if values.len() >= 3 {
                segs.push((i, None, values));
            }
        }
    }
    // tables: segments starting within 25 points of each other
    let mut tables: Vec<(f32, Vec<Found>)> = vec![];
    for s in segs {
        let x = s.2[0].rect.x0;
        match tables.iter_mut().find(|t| (t.0 - x).abs() < 25.0) {
            Some(t) => t.1.push(s),
            None => tables.push((x, vec![s])),
        }
    }
    tables.sort_by(|a, b| a.0.total_cmp(&b.0));
    tables.into_iter().filter(|t| t.1.len() >= 3).filter_map(|t| build(page, &ls, t.1)).collect()
}

/// One table from its rows: the columns, the header and the groups.
fn build<'a>(page: usize, ls: &[Vec<&'a Word>], rows: Vec<Found<'a>>) -> Option<Lineup> {
    // columns: the centres of the cells, grouped where they lie close; a group with cells in fewer than half the rows is
    // no column (the rated voltage written once beside a group of rows)
    let mut xs: Vec<(f32, f32, f32)> = rows.iter().flat_map(|r| r.2.iter().chain(r.1.iter()).map(|w| (center(w).0, w.rect.x0, w.rect.x1))).collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut groups: Vec<Vec<(f32, f32, f32)>> = vec![];
    for x in xs {
        match groups.last_mut() {
            Some(g) if x.0 - g.last().unwrap().0 < 8.0 => g.push(x),
            _ => groups.push(vec![x]),
        }
    }
    let need = rows.len().div_ceil(2);
    // (centre, left, right)
    let cols: Vec<(f32, f32, f32)> = groups
        .iter()
        .filter(|g| g.len() >= need)
        .map(|g| {
            let c = g.iter().map(|x| x.0).sum::<f32>() / g.len() as f32;
            (c, g.iter().map(|x| x.1).fold(f32::MAX, f32::min), g.iter().map(|x| x.2).fold(f32::MIN, f32::max))
        })
        .collect();
    if cols.len() < 3 {
        return None;
    }
    let col_of = |x: f32| -> Option<usize> {
        let (i, d) = cols.iter().enumerate().map(|(i, c)| (i, (c.0 - x).abs())).min_by(|a, b| a.1.total_cmp(&b.1))?;
        // a word between two columns or far outside the table is in none
        let half = cols[i].2 - cols[i].1;
        (d <= half / 2.0 + 10.0).then_some(i)
    };
    let (left, right) = (cols[0].1, cols[cols.len() - 1].2);
    // the header: the lines right above the first row (no gap of more than 20 points between them, 80 points at most),
    // inside the table's width
    let first = rows.iter().map(|r| r.0).min()?;
    let top_of = |l: &Vec<&Word>| l.iter().map(|w| w.rect.y0).fold(f32::MAX, f32::min);
    let bottom_of = |l: &Vec<&Word>| l.iter().map(|w| w.rect.y1).fold(f32::MIN, f32::max);
    let top = top_of(&ls[first]);
    let mut headers = vec![String::new(); cols.len()];
    let mut above = top;
    for l in ls[..first].iter().rev() {
        if above - bottom_of(l) > 20.0 || top - top_of(l) > 80.0 {
            break;
        }
        above = top_of(l);
        // bands: halfway between the columns' centres, the outer ones 20 points beyond the cells
        let mut on_line: Vec<(usize, &Word)> = vec![];
        for w in l {
            let x = center(w).0;
            if x < left - 20.0 || x > right + 20.0 {
                continue;
            }
            let i = (0..cols.len()).find(|&i| i + 1 == cols.len() || x < (cols[i].0 + cols[i + 1].0) / 2.0).unwrap_or(0);
            on_line.push((i, w));
        }
        // the lines are read bottom up: put this line's words before what is there
        for (i, h) in headers.iter_mut().enumerate() {
            let words: Vec<&str> = on_line.iter().filter(|(c, _)| *c == i).map(|(_, w)| w.text.trim()).collect();
            if !words.is_empty() {
                *h = if h.is_empty() { words.join(" ") } else { format!("{} {h}", words.join(" ")) };
            }
        }
    }
    let mut rows: Vec<(f32, LineupRow)> = rows
        .iter()
        .map(|(_, part, values)| {
            let mut cells = vec![String::new(); cols.len()];
            for w in values.iter().chain(part.iter()) {
                if let Some(i) = col_of(center(w).0)
                    && cells[i].is_empty()
                {
                    cells[i] = w.text.trim().to_string();
                }
            }
            (center(values[0]).1, LineupRow { part: part.map_or(String::new(), |p| p.text.trim().to_string()), cells, group: String::new() })
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    // groups: the first column (the capacitance) goes up within a group and starts again at the next; the number
    // written left of the table (up to 60 points) within a group's height names it
    let labels: Vec<(f32, &str)> = ls
        .iter()
        .flatten()
        .filter(|w| {
            let x = center(w).0;
            x < left - 3.0 && x > left - 60.0 && number(&w.text).is_some()
        })
        .map(|w| (center(w).1, w.text.trim()))
        .collect();
    let mut start = 0;
    for i in 1..=rows.len() {
        let restart = i == rows.len() || number(&rows[i].1.cells[0]).zip(number(&rows[i - 1].1.cells[0])).is_some_and(|(a, b)| a < b);
        if restart {
            let (y0, y1) = (rows[start].0 - 4.0, rows[i - 1].0 + 4.0);
            if let Some(l) = labels.iter().find(|l| l.0 >= y0 && l.0 <= y1) {
                for r in &mut rows[start..i] {
                    r.1.group = l.1.to_string();
                }
            }
            start = i;
        }
    }
    Some(Lineup { page, headers, rows: rows.into_iter().map(|r| r.1).collect() })
}

/// The lineup tables of all pages. A page without a header (the table runs on) takes the one before it when it has as
/// many columns.
pub fn lineups(pages: &[&[Word]]) -> Vec<Lineup> {
    let mut out: Vec<Lineup> = vec![];
    for (p, words) in pages.iter().enumerate() {
        for mut l in lineups_on(p, words) {
            if l.headers.iter().all(|h| h.is_empty())
                && let Some(prev) = out.last()
                && prev.headers.len() == l.headers.len()
            {
                l.headers = prev.headers.clone();
            }
            out.push(l);
        }
    }
    out
}

/// One aluminium electrolytic capacitor of a lineup table.
#[derive(Clone, Debug, PartialEq)]
pub struct ElcapRow {
    pub page: usize,
    pub part: String,
    pub cap_uf: f64,
    /// Rated voltage: from a column, else the number beside the row's group, else from the part number (the EIA code
    /// "1C" = 16 V, or Rubycon's leading "16").
    pub volts: Option<f64>,
    /// Case diameter and length (mm).
    pub d_mm: f64,
    pub l_mm: f64,
    pub tan_d: Option<f64>,
    pub leak_ua: Option<f64>,
    /// (temperature in °C, frequency in Hz, impedance or ESR in Ω), as many as the table has.
    pub impedance: Vec<(f64, f64, f64)>,
    /// Rated ripple current (A rms) and the frequency and temperature it is given at.
    pub ripple_a: Option<f64>,
    pub ripple_freq_hz: f64,
    pub ripple_temp_c: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Cap,
    Volts,
    Case,
    TanD,
    Leak,
    /// (temperature, frequency)
    Z(f64, f64),
    /// (in mA, frequency, temperature)
    Ripple(bool, f64, f64),
    Other,
}

fn squash(s: &str) -> String {
    crate::text::normalize(s).to_lowercase().replace([' ', '（', '）', '(', ')'], "")
}

/// The temperature in a header ("20℃/ 100kHz" → 20, "－10℃/" → −10).
pub fn temperature_in(h: &str) -> Option<f64> {
    let h: String = h.chars().map(|c| match c {
        '－' | '−' | '–' | '—' => '-',
        '˚' | '°' | 'º' => '℃',
        c => c,
    }).collect();
    let mut h = h.replace("℃c", "℃").replace("℃C", "℃");
    // the OCR reads the degree sign as an O: "20OC", "105oc"
    for oc in ["OC", "oc", "Oc", "oC"] {
        while let Some(i) = h.find(oc).filter(|&i| h[..i].ends_with(|c: char| c.is_ascii_digit())) {
            h.replace_range(i..i + 2, "℃");
        }
    }
    let at = h.find('℃')?;
    let before = &h[..at];
    let start = before.rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).map(|i| i + before[i..].chars().next().unwrap().len_utf8()).unwrap_or(0);
    number(&before[start..])
}

/// The frequency in a header ("100kHz" → 100 000, "120Hz" → 120).
pub fn frequency_in(h: &str) -> Option<f64> {
    let low = h.to_lowercase().replace(' ', "");
    let at = low.find("hz")?;
    let (head, k) = match low[..at].strip_suffix('k') {
        Some(head) => (head, 1e3),
        None => match low[..at].strip_suffix('m') {
            Some(head) => (head, 1e6),
            None => (&low[..at], 1.0),
        },
    };
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map(|i| i + head[i..].chars().next().unwrap().len_utf8()).unwrap_or(0);
    number(&head[start..]).map(|v| v * k)
}

fn role(header: &str, cells: &[&str]) -> Role {
    let h = squash(header);
    let decimals = cells.iter().filter(|c| c.contains('.')).count() * 2 > cells.len();
    if h.contains("ripple") || h.contains("リップル") {
        // in mA when the header says so, or (its unit lost) when the values are what mA would be: a table's ripple
        // currents in A are below 20, in mA mostly above
        let mut vals: Vec<f64> = cells.iter().filter_map(|c| number(c)).collect();
        vals.sort_by(|a, b| a.total_cmp(b));
        let big = vals.get(vals.len() / 2).is_some_and(|m| *m >= 20.0);
        let ma = h.contains("ma") || (!h.contains("arms") && !h.contains("(a") && big);
        return Role::Ripple(ma, frequency_in(header).unwrap_or(100e3), temperature_in(header).unwrap_or(105.0));
    }
    if cells.iter().filter(|c| case_size(c).is_some()).count() * 2 > cells.len() {
        return Role::Case;
    }
    if h.contains("leakage") || h.contains("漏れ") || h.contains("µa") || h.contains("μa") {
        return Role::Leak;
    }
    // (before tan δ: "capacitance" has "tan" in it)
    if h.contains("capacitance") || h.contains("µf") || h.contains("μf") || h.contains("容量") {
        return Role::Cap;
    }
    if h.contains("tan") {
        return Role::TanD;
    }
    if h.contains("impedance") || h.contains("esr") || h.contains("インピーダンス") || ((h.contains("khz") || h.contains('ω')) && decimals) {
        return Role::Z(temperature_in(header).unwrap_or(20.0), frequency_in(header).unwrap_or(100e3));
    }
    if h.contains("voltage") || h.contains("電圧") || h == "v" || h == "wv" {
        return Role::Volts;
    }
    Role::Other
}

/// Rated voltage from a capacitor's part number: the EIA code after the series letters (Nichicon "UHE1C331MPD",
/// Panasonic "EEUFR1C101": "1C" = 16 V), or the number it starts with (Rubycon "16ZLH1000MEFC").
pub fn volts_of_part(part: &str) -> Option<f64> {
    const EIA: &[(&str, f64)] = &[
        ("0G", 4.0), ("0J", 6.3), ("1A", 10.0), ("1C", 16.0), ("1E", 25.0), ("1V", 35.0), ("1H", 50.0), ("1J", 63.0), ("1K", 80.0),
        ("2A", 100.0), ("2C", 160.0), ("2D", 200.0), ("2E", 250.0), ("2V", 350.0), ("2G", 400.0), ("2W", 450.0), ("2H", 500.0),
    ];
    let p = part.trim().to_ascii_uppercase();
    let lead: String = p.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if !lead.is_empty() {
        return number(&lead);
    }
    let letters = p.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let code = p.get(letters..letters + 2)?;
    EIA.iter().find(|e| e.0 == code).map(|e| e.1)
}

/// The aluminium electrolytic capacitors of the lineup tables: rows with a capacitance and a case size.
pub fn electrolytic(tables: &[Lineup]) -> Vec<ElcapRow> {
    let mut out = vec![];
    for t in tables {
        let roles: Vec<Role> = (0..t.headers.len())
            .map(|i| {
                let cells: Vec<&str> = t.rows.iter().map(|r| r.cells[i].as_str()).filter(|c| !c.is_empty()).collect();
                if t.rows.iter().all(|r| r.cells[i].is_empty() || r.cells[i] == r.part) && !cells.is_empty() {
                    return Role::Other;
                }
                role(&t.headers[i], &cells)
            })
            .collect();
        // no capacitance header: the first number column left of the case size
        let cap_col = roles.iter().position(|r| *r == Role::Cap).or_else(|| {
            let case = roles.iter().position(|r| *r == Role::Case)?;
            (0..case).rev().find(|&i| roles[i] == Role::Other && t.rows.iter().any(|r| number(&r.cells[i]).is_some()))
        });
        let (Some(cap_col), Some(case_col)) = (cap_col, roles.iter().position(|r| *r == Role::Case)) else { continue };
        for r in &t.rows {
            let (Some(cap_uf), Some((d, l))) = (number(&r.cells[cap_col]), case_size(&r.cells[case_col])) else { continue };
            let mut row = ElcapRow {
                page: t.page,
                part: r.part.clone(),
                cap_uf,
                volts: None,
                d_mm: d,
                l_mm: l,
                tan_d: None,
                leak_ua: None,
                impedance: vec![],
                ripple_a: None,
                ripple_freq_hz: 100e3,
                ripple_temp_c: 105.0,
            };
            for (i, role) in roles.iter().enumerate() {
                let Some(v) = number(&r.cells[i]) else { continue };
                match *role {
                    Role::Volts => row.volts = Some(v),
                    Role::TanD => row.tan_d = Some(v),
                    Role::Leak => row.leak_ua = Some(v),
                    Role::Z(t, f) => row.impedance.push((t, f, v)),
                    Role::Ripple(ma, f, t) => {
                        row.ripple_a = Some(if ma { v / 1000.0 } else { v });
                        row.ripple_freq_hz = f;
                        row.ripple_temp_c = t;
                    }
                    _ => {}
                }
            }
            if row.volts.is_none() {
                row.volts = number(&r.group).or_else(|| volts_of_part(&row.part));
            }
            out.push(row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words of one line at height `y`: (left, text) pairs, 6 points per character.
    fn line(y: f32, cells: &[(f32, &str)]) -> Vec<Word> {
        cells.iter().map(|(x, t)| Word::for_test(t, *x, y - 7.0, *x + 5.0 * t.chars().count() as f32, y)).collect()
    }

    /// Nichicon UHE's Dimensions table (page 3 of e-uhe.pdf, CAT.8100M) as its words lie: the header over two columns
    /// of impedance, the rated voltage written once beside its group of rows (and on a row's line).
    fn uhe_page() -> Vec<Word> {
        let mut w = vec![];
        w.extend(line(102.0, &[(337.0, "Impedance（Ω）"), (280.0, "Leakage"), (303.0, "Current")]));
        w.extend(line(108.0, &[(125.0, "Rated"), (410.0, "Rated"), (431.0, "Ripple"), (49.0, "Rated"), (70.0, "Voltage")]));
        w.extend(line(111.0, &[(354.0, "max."), (189.0, "Case"), (207.0, "Size"), (289.0, "（µA）")]));
        w.extend(line(116.0, &[(250.0, "tan"), (262.0, "δ"), (115.0, "Capacitance"), (413.0, "（mArms）"), (488.0, "Part"), (503.0, "Number")]));
        w.extend(line(121.0, &[(186.0, "φD×L（mm）"), (284.0, "at"), (291.0, "20℃"), (335.0, "20℃/"), (368.0, "－10℃/")]));
        w.extend(line(125.0, &[(123.0, "（µF）"), (401.0, "（105℃/100kHz）")]));
        w.extend(line(128.5, &[(331.0, "100kHz"), (368.0, "100kHz")]));
        let rows = [
            (142.9, ["330", "8×11.5", "0.16", "52.8", "0.13", "0.52", "640", "UHE1C331MPD"]),
            (156.3, ["470", "8×15", "0.16", "75.2", "0.087", "0.35", "840", "UHE1C471MPD"]),
            (169.7, ["1000", "10×20", "0.16", "160", "0.046", "0.18", "1400", "UHE1C102MPD"]),
            (183.0, ["2200", "12.5×25", "0.18", "352", "0.027", "0.089", "2230", "UHE1C222MHD"]),
            (196.4, ["2200", "18×15", "0.18", "352", "0.043", "0.11", "2210", "UHE1C222MHD6"]),
        ];
        let xs = [127.0, 192.0, 252.0, 293.0, 334.0, 371.0, 423.0, 481.0];
        for (y, cells) in rows {
            let c: Vec<(f32, &str)> = xs.iter().copied().zip(cells.iter().copied()).collect();
            w.extend(line(y, &c));
        }
        // the group's voltage, once, on a row's line and under it
        w.extend(line(196.4, &[(67.0, "16")]));
        w.extend(line(207.4, &[(60.0, "（1C）")]));
        // the table's title (left of the columns) and the page's, well above
        w.extend(line(87.5, &[(52.0, "Dimensions")]));
        w.extend(line(58.1, &[(42.0, "UHE"), (300.0, "ELECTROLYTIC")]));
        w
    }

    #[test]
    fn the_rows_and_columns_of_a_capacitor_lineup() {
        let page = uhe_page();
        let l = lineup_on(2, &page).unwrap();
        assert_eq!(l.rows.len(), 5, "{l:?}");
        assert_eq!(l.headers.len(), 8, "{:?}", l.headers);
        assert!(l.headers[0].contains("Capacitance") && l.headers[1].contains("Case"), "{:?}", l.headers);
        assert!(l.headers[6].contains("Ripple") && l.headers[6].contains("100kHz"), "{:?}", l.headers);
        assert!(l.headers[5].contains("－10℃"), "{:?}", l.headers);
        assert_eq!(l.rows[4].part, "UHE1C222MHD6");
        assert_eq!(l.rows[4].cells[0], "2200", "the voltage beside the group is not a cell: {:?}", l.rows[4]);
    }

    #[test]
    fn electrolytic_values_from_the_lineup() {
        let page = uhe_page();
        let rows = electrolytic(&lineups(&[&[], &[], &page]));
        assert_eq!(rows.len(), 5);
        let r = &rows[2];
        assert_eq!((r.part.as_str(), r.cap_uf, r.volts, r.d_mm, r.l_mm), ("UHE1C102MPD", 1000.0, Some(16.0), 10.0, 20.0));
        assert_eq!((r.tan_d, r.leak_ua, r.ripple_a), (Some(0.16), Some(160.0), Some(1.4)));
        assert_eq!(r.impedance, vec![(20.0, 100e3, 0.046), (-10.0, 100e3, 0.18)]);
        assert_eq!((r.ripple_freq_hz, r.ripple_temp_c, r.page), (100e3, 105.0, 2));
    }

    #[test]
    fn rated_voltage_from_the_part_number() {
        assert_eq!(volts_of_part("UHE1C331MPD"), Some(16.0));
        assert_eq!(volts_of_part("EEUFR1H101"), Some(50.0));
        assert_eq!(volts_of_part("UHE0J681MPD"), Some(6.3));
        assert_eq!(volts_of_part("16ZLH1000MEFC10X16"), Some(16.0));
        assert_eq!(volts_of_part("ABC"), None);
    }

    #[test]
    fn temperatures_and_frequencies_in_headers() {
        assert_eq!(temperature_in("－10℃/ 100kHz"), Some(-10.0));
        assert_eq!(temperature_in("（105℃/100kHz）"), Some(105.0));
        assert_eq!(temperature_in("at 20˚C"), Some(20.0));
        assert_eq!(frequency_in("（105℃/100kHz）"), Some(100e3));
        assert_eq!(frequency_in("120Hz"), Some(120.0));
    }
}

#[cfg(test)]
mod real_pdf {
    /// Nichicon UHE (e-uhe.pdf, CAT.8100M; skipped without the file in microohm-meter/): every row of its six pages.
    #[test]
    fn nichicon_uhe() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../microohm-meter/e-uhe.pdf");
        let Ok(ds) = crate::Datasheet::open_file(&path) else { return };
        let rows = ds.electrolytic_rows();
        assert_eq!(rows.len(), 212);
        let r = rows.iter().find(|r| r.part == "UHE1H471MPD").unwrap();
        assert_eq!((r.cap_uf, r.volts, r.d_mm, r.l_mm, r.ripple_a), (470.0, Some(50.0), 10.0, 30.5, Some(1.69)));
        assert_eq!(r.impedance, vec![(20.0, 100e3, 0.043), (-10.0, 100e3, 0.17)]);
        // the leakage column says 0.01 CV in every row
        for r in &rows {
            let cv = 0.01 * r.cap_uf * r.volts.unwrap();
            assert!((r.leak_ua.unwrap() - cv).abs() < 0.06, "{r:?}");
        }
    }
}
