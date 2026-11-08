//! Board snippets: the selected parts with the tracks and vias between them, saved as a file (`.ccbsnip`) and put on another board that
//! has the same parts (`pcb_core::snippet`). The parts are matched by reference designator and footprint.

use super::*;
use pcb_core::snippet::{self, Applied, BoardSnippet};
use std::path::Path;

const EXT: &str = "ccbsnip";

/// The sentence that tells what a put snippet did (and what it could not do).
fn summary(done: &Applied) -> String {
    let mut s = format!("スニペットを置きました: 部品 {} 個を動かし、配線 {} 本・ビア {} 個を足しました", done.moved.len(), done.tracks_added, done.vias_added);
    if done.tracks_removed > 0 {
        s += &format!(" (動かした部品につながっていた古い配線 {} 本は消しました)", done.tracks_removed);
    }
    let note = |label: &str, list: &[String]| if list.is_empty() { String::new() } else { format!(" / {label}: {}", list.join("・")) };
    s += &note("この基板にない部品", &done.missing);
    s += &note("フットプリントが違うので動かさなかった部品", &done.other_footprint);
    s += &note("固定されていて動かさなかった部品", &done.locked);
    s
}

impl BoardEditor {
    /// Writes the selected parts, tracks and vias to `path`.
    pub(super) fn save_snippet_to(&mut self, path: &Path) -> Result<usize, String> {
        let (mut parts, mut tracks, mut vias) = (vec![], vec![], vec![]);
        for it in self.selection_items() {
            match it {
                Item::Part(i) => parts.push(i),
                Item::Track(i) => tracks.push(i),
                Item::Via(i) => vias.push(i),
                _ => {}
            }
        }
        let snip = snippet::capture(&self.board, &parts, &tracks, &vias);
        if snip.is_empty() {
            return Err("保存する部品・配線・ビアを選んでください (選択ツールで囲むか、Shift+クリックで選びます)".into());
        }
        let text = serde_json::to_string_pretty(&snip).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("書き込めません: {e}"))?;
        Ok(snip.parts.len())
    }

    /// Reads a snippet file and puts it with its middle at the middle of the view. The board is changed only when something moves or is added.
    pub(super) fn load_snippet_from(&mut self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("読み込めません: {e}"))?;
        let snip: BoardSnippet = serde_json::from_str(&text).map_err(|e| format!("基板のスニペットのファイルではありません: {e}"))?;
        if snip.is_empty() {
            return Err("中身が空のスニペットです".into());
        }
        let center = Point { x: self.pan.x as f64, y: self.pan.y as f64 };
        let mut trial = self.board.clone();
        let done = snippet::apply(&mut trial, &snip, center);
        if done.moved.is_empty() && done.tracks_added == 0 && done.vias_added == 0 {
            return Err(format!("この基板に置けるものがありませんでした。{}", summary(&done)));
        }
        self.checkpoint();
        self.board = trial;
        Ok(summary(&done))
    }

    /// "選択を保存…": asks where, then writes the selection as a snippet file.
    pub(super) fn save_snippet_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("基板のスニペット", &[EXT]).set_file_name(format!("snippet.{EXT}")).save_file() {
            self.status = match self.save_snippet_to(&p) {
                Ok(n) => format!("スニペットを保存しました (部品 {n} 個): {}", p.display()),
                Err(e) => format!("保存失敗: {e}"),
            };
        }
    }

    /// "スニペットを置く…": asks which file, then puts it at the middle of the view.
    pub(super) fn load_snippet_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("基板のスニペット", &[EXT]).pick_file() {
            self.status = match self.load_snippet_from(&p) {
                Ok(s) => s,
                Err(e) => format!("読み込み失敗: {e}"),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcb_core::model::{Part, Side};

    fn editor(x: f64) -> BoardEditor {
        let mut e = BoardEditor::default();
        for (r, dx) in [("C1", 0.0), ("C2", 10.0)] {
            e.board.set_part(Part { refdes: r.into(), value: "100n".into(), footprint: "C_0603".into(), pos: Point::new(x + dx, 20.0), rot: 0, side: Side::Top }).unwrap();
        }
        e
    }

    #[test]
    fn a_saved_selection_is_put_on_another_board_and_can_be_undone() {
        let mut a = editor(10.0);
        a.multi = vec![Item::Part(0), Item::Part(1)];
        let path = std::env::temp_dir().join("ccad_board_snippet_test.ccbsnip");
        assert_eq!(a.save_snippet_to(&path), Ok(2));

        let mut b = editor(40.0);
        b.pan = Vec2::new(100.0, 50.0);
        let said = b.load_snippet_from(&path).unwrap();
        assert!(said.contains("部品 2 個"), "{said}");
        let c1 = b.board.parts.iter().find(|p| p.refdes == "C1").unwrap().pos;
        assert!((c1.x - 95.0).abs() < 0.2 && (c1.y - 50.0).abs() < 0.2, "{c1:?}: the middle of the two parts is at the middle of the view");
        b.undo();
        assert_eq!(b.board.parts.iter().find(|p| p.refdes == "C1").unwrap().pos.x, 40.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn nothing_selected_and_nothing_to_match_are_refused_without_a_change() {
        let mut a = editor(10.0);
        let path = std::env::temp_dir().join("ccad_board_snippet_none.ccbsnip");
        assert!(a.save_snippet_to(&path).is_err());
        a.multi = vec![Item::Part(0)];
        a.save_snippet_to(&path).unwrap();
        let mut other = BoardEditor::default();
        assert!(other.load_snippet_from(&path).unwrap_err().contains("置けるものがありません"));
        assert!(!other.dirty());
        let _ = std::fs::remove_file(path);
    }
}
