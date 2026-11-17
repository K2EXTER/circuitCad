//! 「PDF の熱の表から読む」: the thermal resistances of the open data sheet (`datasheet::thermal`) into the ratings: θJA (the
//! self-heating run's θ of the part on its smallest pads), θjc, and the temperature the rated power starts to fall from.
//! A θJA measured on a large board (JEDEC 2s2p, 1 in² of copper) is kept in the free rows instead: the board's copper is counted
//! from the board, so the part's own θ must be the one on its smallest pads.

use super::PartForm;
use super::graph_reader::short;
use datasheet::thermal::{ThermalRow, To, on_large_copper};

/// What was taken from the rows, and a line for each figure (read or why not).
#[derive(Debug, Default, PartialEq)]
pub struct HeatRead {
    pub ja: Option<f64>,
    pub jc: Option<f64>,
    pub derate_from: Option<f64>,
    /// Free rows (`name | value | condition`) for figures that are not the ratings' own (θJA on a large board, θJL).
    pub rows: Vec<String>,
    pub lines: Vec<(bool, String)>,
}

/// The thermal table's row of a kind, else the one from a derating. A row with a value per package (3 or more) is not taken.
fn best(rows: &[ThermalRow], to: To) -> Option<&ThermalRow> {
    let one = |r: &&ThermalRow| r.to == to && r.columns < 3;
    rows.iter().filter(one).find(|r| !r.from_derating).or_else(|| rows.iter().find(one))
}

/// A row of the kind with a value per package, when that is all there is.
fn per_package(rows: &[ThermalRow], to: To) -> Option<&ThermalRow> {
    rows.iter().find(|r| r.to == to && r.columns >= 3)
}

/// Where a row came from, for the line.
fn source(r: &ThermalRow) -> String {
    if r.from_derating {
        format!("p.{} の許容損失の低減 ({}) の逆数", r.page + 1, r.text)
    } else {
        format!("p.{} の熱の表 ({})", r.page + 1, r.text)
    }
}

pub fn heat_from_rows(rows: &[ThermalRow]) -> HeatRead {
    let mut h = HeatRead::default();
    match best(rows, To::Ambient) {
        Some(r) if on_large_copper(&r.text) => {
            h.rows.push(format!("RθJA | {} K/W | {}", short(r.k_per_w), r.text));
            h.lines.push((false, format!("θJA {} K/W は大きい基板 (銅を広げた基板) で測った値なので、部品だけの θ には入れず「そのほかの情報」に残しました。基板の銅の効きは、基板から計算します: {}", short(r.k_per_w), source(r))));
        }
        Some(r) => {
            h.ja = Some(r.k_per_w);
            h.lines.push((true, format!("θJA {} K/W (接合部 → 空気): {}。自己発熱の計算で、この部品の θ になります", short(r.k_per_w), source(r))));
        }
        None => match per_package(rows, To::Ambient) {
            Some(r) => h.lines.push((false, format!("θJA の行 (p.{}) は値がパッケージごとに {} 列あり、どれがこの部品か決められません。表の見出しで部品のパッケージの列を見て、手で入れてください: {}", r.page + 1, r.columns, r.text))),
            None => h.lines.push((false, "θJA (Thermal Resistance, Junction-to-Ambient・RθJA) も、許容損失の低減 (Derate above … mW/°C) も見つかりません。θ はパッケージから推定します".into())),
        },
    }
    match (best(rows, To::Case), per_package(rows, To::Case)) {
        (Some(r), _) => {
            h.jc = Some(r.k_per_w);
            h.lines.push((true, format!("θjc {} K/W (接合部 → ケース): {}", short(r.k_per_w), source(r))));
        }
        (None, Some(r)) => h.lines.push((false, format!("θjc の行 (p.{}) は値がパッケージごとに {} 列あり、決められません: {}", r.page + 1, r.columns, r.text))),
        (None, None) => {}
    }
    if let Some(r) = best(rows, To::Lead) {
        h.rows.push(format!("RθJL | {} K/W | {}", short(r.k_per_w), r.text));
        h.lines.push((true, format!("θJL {} K/W (接合部 → リード) は「そのほかの情報」に入れました (自己発熱には使いません): {}", short(r.k_per_w), source(r))));
    }
    // the temperature the power starts to fall from: the derating to the air
    if let Some((r, t)) = rows.iter().filter(|r| r.from_derating && r.to == To::Ambient).find_map(|r| r.derate_from.map(|t| (r, t))) {
        h.derate_from = Some(t);
        h.lines.push((true, format!("電力を下げ始める温度 {} °C: p.{} の「{}」", short(t), r.page + 1, r.text)));
    }
    h
}

impl PartForm {
    /// 「PDF の熱の表から読む」: the open PDF's thermal figures into the ratings fields (the ones found; the others stay).
    pub(super) fn fill_heat_from_pdf(&mut self) {
        let h = heat_from_rows(&self.sheet.thermal());
        if let Some(v) = h.ja {
            self.rt_ja = short(v);
        }
        if let Some(v) = h.jc {
            self.rt_jc = short(v);
        }
        if let Some(t) = h.derate_from {
            self.rt_derate = short(t);
        }
        add_rows(&mut self.rt_rows, &h.rows);
        self.rt_heat_msg = h.lines;
    }
}

/// Adds free rows (`name | value | condition`) whose name is not there yet.
fn add_rows(text: &mut String, rows: &[String]) {
    let name = |l: &str| l.split('|').next().unwrap_or("").trim().to_string();
    for row in rows {
        if !text.lines().any(|l| name(l) == name(row)) {
            if !text.trim().is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(to: To, v: f64, text: &str, derating: Option<f64>) -> ThermalRow {
        ThermalRow { page: 1, to, k_per_w: v, text: text.into(), from_derating: derating.is_some(), derate_from: derating, columns: 1 }
    }

    #[test]
    fn the_table_goes_before_the_derating_and_a_large_board_is_kept_aside() {
        // onsemi's TIP120: deratings first on the page, then the table
        let rows = [
            row(To::Case, 1.923, "derate above 25°c 0.52 w/°c", Some(25.0)),
            row(To::Ambient, 62.5, "derate above 25°c 0.016 w/°c", Some(25.0)),
            row(To::Case, 1.92, "thermal resistance, junction-to-case 1.92 °c/w", None),
            row(To::Ambient, 62.5, "thermal resistance, junction-to-ambient 62.5 °c/w", None),
        ];
        let h = heat_from_rows(&rows);
        assert_eq!((h.ja, h.jc, h.derate_from), (Some(62.5), Some(1.92), Some(25.0)));
        assert!(h.lines.iter().all(|l| l.0) && h.lines[0].1.contains("熱の表"), "{:?}", h.lines);
        // a θJA on JEDEC's 2s2p board is not the part's own θ; a lead's goes to the rows
        let rows = [row(To::Ambient, 45.0, "junction-to-ambient (jedec 2s2p) 45 °c/w", None), row(To::Lead, 28.0, "rθjl 28", None)];
        let h = heat_from_rows(&rows);
        assert_eq!(h.ja, None);
        assert_eq!(h.rows.len(), 2);
        assert!(!h.lines[0].0 && h.lines[0].1.contains("大きい基板"));
        // only a derating: θJA from it
        let h = heat_from_rows(&[row(To::Ambient, 200.0, "derate above 25°c 5.0 mw/°c", Some(25.0))]);
        assert_eq!(h.ja, Some(200.0));
        assert!(h.lines[0].1.contains("逆数"));
        assert!(!heat_from_rows(&[]).lines[0].0, "nothing found says so");
        // TI's columns per package: not taken, and said why
        let mut ti = row(To::Ambient, 43.7, "rθja 97.1 96.5 98.18 51.5 44 43.7 °c/w", None);
        ti.columns = 6;
        let h = heat_from_rows(&[ti]);
        assert_eq!(h.ja, None);
        assert!(h.lines[0].1.contains("パッケージごと"), "{:?}", h.lines);
    }

    #[test]
    fn the_rows_are_added_once() {
        let mut text = "RθJL | 30 K/W | old".to_string();
        add_rows(&mut text, &["RθJL | 28 K/W | new".into(), "RθJA | 45 K/W | 2s2p".into()]);
        assert_eq!(text, "RθJL | 30 K/W | old\nRθJA | 45 K/W | 2s2p");
    }
}
