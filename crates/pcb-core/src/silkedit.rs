//! Editing the silkscreen by hand: free texts (a board name, a version, a note), the size / turn / visibility of the part designators, and
//! the width of silk lines. Every size stays within what the fab prints (`Rules::min_silk_width`, `Rules::min_silk_text`, set with the fab
//! preset): a smaller value is refused with the reason, so a board never asks for silk the fab cannot make.

use crate::model::*;

/// The biggest silk text (mm, the height of the letters): bigger is allowed by the fabs, but is never what was meant.
pub const MAX_TEXT_HEIGHT: f64 = 10.0;
/// The widest silk line (mm).
pub const MAX_LINE_WIDTH: f64 = 2.0;

/// Is the text at `i` one that is edited as a free text: a text of no part, or a pin name written by `pinlabels` (they move and change like
/// free texts; a designator is changed with `set_label`).
pub fn is_free_text(board: &Board, i: usize) -> bool {
    board.texts.get(i).is_some_and(|t| t.refdes.is_empty() || t.refdes.contains('#'))
}

/// A text the font can print: not empty, and only letters, digits and the symbols of the stroke font (lower case prints as capitals).
pub fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("文字が空です。印刷する文字を入れてください".into());
    }
    let bad: String = text.chars().filter(|c| !crate::strokefont::supported(*c)).collect();
    if !bad.is_empty() {
        return Err(format!("「{bad}」はシルクの文字 (英字・数字・記号の一部) で書けません。英数字で書いてください (日本語は印刷できません)"));
    }
    Ok(())
}

/// A text height the fab prints: not below the fab's smallest (`Rules::min_silk_text`) and not above `MAX_TEXT_HEIGHT`.
pub fn check_height(board: &Board, h: f64) -> Result<(), String> {
    let min = board.rules.min_silk_text;
    if !h.is_finite() || h + 1e-9 < min {
        return Err(format!("文字の高さ {h:.2} mm は、発注先が印刷できる {min:.2} mm より小さいです。{min:.2} mm 以上にしてください (下限は発注先の設定で変わります)"));
    }
    if h > MAX_TEXT_HEIGHT + 1e-9 {
        return Err(format!("文字の高さ {h:.2} mm は大きすぎます ({MAX_TEXT_HEIGHT} mm まで)"));
    }
    Ok(())
}

/// A silk line width the fab prints: not below the fab's thinnest (`Rules::min_silk_width`) and not above `MAX_LINE_WIDTH`.
pub fn check_width(board: &Board, w: f64) -> Result<(), String> {
    let min = board.rules.min_silk_width;
    if !w.is_finite() || w + 1e-9 < min {
        return Err(format!("シルクの線の太さ {w:.3} mm は、発注先が印刷できる {min:.3} mm より細いです。{min:.3} mm 以上にしてください"));
    }
    if w > MAX_LINE_WIDTH + 1e-9 {
        return Err(format!("シルクの線の太さ {w:.2} mm は太すぎます ({MAX_LINE_WIDTH} mm まで)"));
    }
    Ok(())
}

/// Put a free text on the silk; returns its index in `Board::texts`.
pub fn add_text(board: &mut Board, text: &str, pos: Point, height: f64, rot: u8, side: Side) -> Result<usize, String> {
    check_text(text)?;
    check_height(board, height)?;
    board.texts.push(SilkText { side: side.outer(), pos, height, text: text.trim().to_string(), refdes: String::new(), rot: rot % 4 });
    Ok(board.texts.len() - 1)
}

/// What to change in a text (`None`: as it is).
#[derive(Clone, Debug, Default)]
pub struct TextChange {
    pub text: Option<String>,
    pub height: Option<f64>,
    pub rot: Option<u8>,
    pub side: Option<Side>,
    pub pos: Option<Point>,
}

/// Change a free text (see `is_free_text`); nothing changes when a value is refused.
pub fn set_text(board: &mut Board, i: usize, c: &TextChange) -> Result<(), String> {
    if !is_free_text(board, i) {
        return Err(format!("{i} 番の文字は自由な文字ではありません (部品番号は、部品の文字の設定で変えます)"));
    }
    if let Some(t) = &c.text {
        check_text(t)?;
    }
    if let Some(h) = c.height {
        check_height(board, h)?;
    }
    let t = &mut board.texts[i];
    if let Some(s) = &c.text {
        t.text = s.trim().to_string();
    }
    if let Some(h) = c.height {
        t.height = h;
    }
    if let Some(r) = c.rot {
        t.rot = r % 4;
    }
    if let Some(s) = c.side {
        t.side = s.outer();
    }
    if let Some(p) = c.pos {
        t.pos = p;
    }
    Ok(())
}

/// Take a free text away.
pub fn remove_text(board: &mut Board, i: usize) -> Result<(), String> {
    if !is_free_text(board, i) {
        return Err(format!("{i} 番の文字は自由な文字ではありません (部品番号は消さずに、隠す設定にします)"));
    }
    board.texts.remove(i);
    Ok(())
}

/// Change the designator of a part: its height, its turn, or whether it is printed (`None`: as it is). The part is placed again where it
/// is, so the text stands above it as before (with the offset the user gave it).
pub fn set_label(board: &mut Board, refdes: &str, height: Option<f64>, rot: Option<u8>, hidden: Option<bool>) -> Result<(), String> {
    let Some(part) = board.parts.iter().find(|p| p.refdes == refdes).cloned() else {
        return Err(format!("部品 {refdes} は基板にありません"));
    };
    if let Some(h) = height {
        check_height(board, h)?;
    }
    if let Some(hide) = hidden {
        board.hidden_labels.retain(|r| r != refdes);
        if hide {
            board.hidden_labels.push(refdes.to_string());
        }
    }
    // the size and turn are kept on the text, which placing the part again reads back (a hidden designator gets them when shown again)
    if let Some(t) = board.texts.iter_mut().find(|t| t.refdes == refdes) {
        if let Some(h) = height {
            t.height = h;
        }
        if let Some(r) = rot {
            t.rot = r % 4;
        }
    }
    board.place_part(refdes, part.pos, part.rot);
    Ok(())
}

/// The same height for every designator; returns how many changed.
pub fn set_all_labels(board: &mut Board, height: f64) -> Result<usize, String> {
    check_height(board, height)?;
    let names: Vec<String> = board.texts.iter().filter(|t| !t.refdes.is_empty() && !t.refdes.contains('#') && (t.height - height).abs() > 1e-9).map(|t| t.refdes.clone()).collect();
    for r in &names {
        set_label(board, r, Some(height), None, None)?;
    }
    Ok(names.len())
}

/// Change the width of a silk line that belongs to no part (the outlines of parts come from their footprints).
pub fn set_line_width(board: &mut Board, i: usize, width: f64) -> Result<(), String> {
    check_width(board, width)?;
    match board.silk.get_mut(i) {
        Some(l) if l.refdes.is_empty() => {
            l.width = width;
            Ok(())
        }
        Some(_) => Err(format!("{i} 番の線は部品の外形です (部品の形から作られるので、太さは変えられません)")),
        None => Err(format!("{i} 番のシルクの線はありません")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top }).unwrap();
        b
    }

    #[test]
    fn a_free_text_is_put_changed_and_taken_away_within_the_fabs_limits() {
        let mut b = board();
        let i = add_text(&mut b, "v1.2 rev", Point::new(5.0, 5.0), 1.5, 1, Side::Top).unwrap();
        assert_eq!((b.texts[i].text.as_str(), b.texts[i].height, b.texts[i].rot), ("v1.2 rev", 1.5, 1));
        // too small for the fab, letters the font has not, and an empty text are refused, and nothing changes
        assert!(set_text(&mut b, i, &TextChange { height: Some(0.5), ..Default::default() }).unwrap_err().contains("0.80"));
        assert!(set_text(&mut b, i, &TextChange { text: Some("基板".into()), ..Default::default() }).unwrap_err().contains("基板"));
        assert!(add_text(&mut b, "  ", Point::default(), 1.0, 0, Side::Top).is_err());
        assert_eq!(b.texts[i].height, 1.5);
        // the fab's limit comes from the rules: a fab that prints smaller lets it through
        b.rules.min_silk_text = 0.5;
        set_text(&mut b, i, &TextChange { height: Some(0.5), text: Some("OK".into()), side: Some(Side::Bottom), ..Default::default() }).unwrap();
        assert_eq!((b.texts[i].text.as_str(), b.texts[i].side), ("OK", Side::Bottom));
        // a designator is not a free text
        let d = b.texts.iter().position(|t| t.refdes == "R1").unwrap();
        assert!(set_text(&mut b, d, &TextChange { text: Some("X".into()), ..Default::default() }).is_err());
        assert!(remove_text(&mut b, d).is_err());
        remove_text(&mut b, i).unwrap();
        assert!(b.texts.iter().all(|t| t.refdes == "R1"));
    }

    #[test]
    fn a_designator_keeps_its_size_and_turn_when_the_part_moves_and_can_be_hidden() {
        let mut b = board();
        set_label(&mut b, "R1", Some(1.6), Some(1), None).unwrap();
        b.place_part("R1", Point::new(10.0, 10.0), 0);
        let t = b.texts.iter().find(|t| t.refdes == "R1").unwrap();
        assert_eq!((t.height, t.rot), (1.6, 1));
        // the text stands above the part, clear of it, for its new size
        let (_, hi) = crate::labels::body_box(&b, "R1").unwrap();
        assert!(t.pos.y - t.size().1 / 2.0 > hi.y - 1e-9, "{:?} {:?}", t.pos, hi);
        assert!(set_label(&mut b, "R1", Some(0.3), None, None).is_err());
        set_label(&mut b, "R1", None, None, Some(true)).unwrap();
        assert!(b.texts.iter().all(|t| t.refdes != "R1"));
        b.place_part("R1", Point::new(12.0, 10.0), 0);
        assert!(b.texts.iter().all(|t| t.refdes != "R1"), "a hidden designator stays hidden when the part moves");
        set_label(&mut b, "R1", None, None, Some(false)).unwrap();
        assert!(b.texts.iter().any(|t| t.refdes == "R1"));
        assert_eq!(set_all_labels(&mut b, 1.2).unwrap(), 1);
        assert_eq!(b.texts.iter().find(|t| t.refdes == "R1").unwrap().height, 1.2);
    }

    #[test]
    fn a_free_silk_line_gets_a_width_the_fab_prints_and_a_parts_outline_does_not_change() {
        let mut b = board();
        b.silk.push(SilkLine { side: Side::Top, a: Point::new(1.0, 1.0), b: Point::new(5.0, 1.0), width: 0.15, refdes: String::new() });
        let i = b.silk.len() - 1;
        set_line_width(&mut b, i, 0.4).unwrap();
        assert_eq!(b.silk[i].width, 0.4);
        assert!(set_line_width(&mut b, i, 0.05).unwrap_err().contains("0.150"));
        let part_line = b.silk.iter().position(|l| l.refdes == "R1").unwrap();
        assert!(set_line_width(&mut b, part_line, 0.3).is_err());
    }
}
