//! Snippets: the selection saved as a file (`.ccsnip`, the same pieces the clipboard holds), to be used in other circuits.

use super::*;

const EXT: &str = "ccsnip";

impl SchEditor {
    /// Writes the selection to `path`. A part brings the net labels on its pins, as with copy.
    pub(super) fn save_snippet_to(&mut self, path: &Path) -> Result<(), String> {
        if !self.copy_selection() {
            return Err("保存するものを選んでください".into());
        }
        let text = serde_json::to_string_pretty(&self.clipboard).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("書き込めません: {e}"))
    }

    /// Reads a snippet file and pastes it on free ground of this sheet.
    pub(super) fn load_snippet_from(&mut self, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("読み込めません: {e}"))?;
        let f: sch_core::Fragment = serde_json::from_str(&text).map_err(|e| format!("スニペットのファイルではありません: {e}"))?;
        if f.is_empty() {
            return Err("中身が空のスニペットです".into());
        }
        self.clipboard = f;
        self.paste_clipboard();
        Ok(())
    }

    pub(super) fn snippet_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let has = self.has_selection();
            if ui
                .add_enabled(has, egui::Button::new("選択を保存…"))
                .on_hover_text("選んだ部品・線・ラベルをファイル (スニペット) に保存します。電源回路やLED点灯回路など、よく使う部分を別の回路でも使えます")
                .clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("スニペット", &[EXT]).set_file_name(format!("snippet.{EXT}")).save_file()
            {
                self.status = match self.save_snippet_to(&p) {
                    Ok(()) => format!("スニペットを保存しました: {}", p.display()),
                    Err(e) => format!("保存失敗: {e}"),
                };
            }
            if ui
                .button("スニペットを置く…")
                .on_hover_text("保存したスニペットを選んで、いまの回路の空いた所に置きます。部品番号は新しくなります。置いたあと、ドラッグで動かせます")
                .clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("スニペット", &[EXT]).pick_file()
                && let Err(e) = self.load_snippet_from(&p)
            {
                self.status = format!("読み込み失敗: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_selection_is_pasted_into_another_sheet() {
        let mut a = SchEditor::default();
        a.sch = Schematic::default();
        a.sch.add(Kind::Resistor, (20, 20), 0);
        let pin = a.sch.components[0].pins()[0].1;
        a.sch.labels.push(Label { pos: pin, text: "IN".into() });
        a.set_selection(vec![Sel::Comp(0)]);
        let path = std::env::temp_dir().join("ccad_snippet_test.ccsnip");
        a.save_snippet_to(&path).unwrap();

        let mut b = SchEditor::default();
        b.sch = Schematic::default();
        b.sch.add(Kind::Resistor, (20, 20), 0);
        b.load_snippet_from(&path).unwrap();
        assert_eq!(b.sch.components.len(), 2);
        assert_ne!(b.sch.components[0].refdes, b.sch.components[1].refdes, "the pasted part has a new number");
        assert_eq!(b.sch.labels.len(), 1, "the label on the pin came along");
        assert!(b.load_snippet_from(&std::env::temp_dir().join("no_such.ccsnip")).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn saving_without_a_selection_is_refused() {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        assert!(e.save_snippet_to(&std::env::temp_dir().join("never.ccsnip")).is_err());
    }
}
