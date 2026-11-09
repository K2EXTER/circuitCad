//! The title block (title, revision, date, author, note): a window to fill it in, and the box drawn at the corner of the exported sheet.

use super::*;

impl SchEditor {
    /// The box at the lower right of the exported picture. Nothing is drawn while the block is empty.
    pub(super) fn draw_title_block(&self, painter: &egui::Painter, rect: Rect) {
        let t = &self.sch.title_block;
        if t.is_empty() {
            return;
        }
        let ink = Color32::from_gray(40);
        let rows: Vec<(&str, &str, f32)> = vec![("", t.title.as_str(), 18.0), ("版", t.revision.as_str(), 13.0), ("日付", t.date.as_str(), 13.0), ("作者", t.author.as_str(), 13.0), ("メモ", t.note.as_str(), 13.0)]
            .into_iter()
            .filter(|r| !r.1.is_empty())
            .collect();
        let width = 300.0;
        let height: f32 = rows.iter().map(|r| r.2 + 10.0).sum::<f32>() + 6.0;
        let frame = Rect::from_min_size(rect.right_bottom() - Vec2::new(width + 8.0, height + 8.0), Vec2::new(width, height));
        painter.rect_stroke(frame, 0.0, Stroke::new(1.5, ink), egui::StrokeKind::Inside);
        let mut y = frame.top() + 3.0;
        for (label, text, size) in rows {
            let font = egui::FontId::proportional(size);
            if label.is_empty() {
                painter.text(Pos2::new(frame.left() + 8.0, y + 2.0), Align2::LEFT_TOP, text, font, ink);
            } else {
                painter.text(Pos2::new(frame.left() + 8.0, y + 2.0), Align2::LEFT_TOP, label, font.clone(), Color32::from_gray(110));
                painter.text(Pos2::new(frame.left() + 56.0, y + 2.0), Align2::LEFT_TOP, text, font, ink);
            }
            y += size + 10.0;
            painter.line_segment([Pos2::new(frame.left(), y - 2.0), Pos2::new(frame.right(), y - 2.0)], Stroke::new(0.8, Color32::from_gray(160)));
        }
    }

    pub fn open_title_block(&mut self) {
        self.title_open = true;
    }

    /// File → 図枠: the fields of the title block.
    pub fn title_block_window(&mut self, ctx: &egui::Context) {
        if !self.title_open {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::tr("図枠")).open(&mut open).default_pos(crate::widgets::beside_panel()).resizable(false).show(ctx, |ui| {
            ui.label(i18n::tr("書き出した図の右下に、枠で入ります。空の欄は出ません。")).on_hover_text(i18n::tr("回路図のファイルにも一緒に保存されます。印刷した図が、どの回路のどの版か分かるようにしておくと、あとで見返すときに迷いません"));
            let t = &mut self.sch.title_block;
            egui::Grid::new("title_block_grid").num_columns(2).show(ui, |ui| {
                for (label, field, hint) in [
                    ("タイトル", &mut t.title, "回路の名前。例: 5 V 電源"),
                    ("版", &mut t.revision, "作り直すたびに上げる印。例: A、1.2"),
                    ("日付", &mut t.date, "例: 2026-11-08"),
                    ("作者", &mut t.author, "あなたの名前"),
                    ("メモ", &mut t.note, "ひとこと。例: 試作 1 回目"),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(field).desired_width(220.0)).on_hover_text(hint);
                    ui.end_row();
                }
            });
        });
        self.title_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_block_is_drawn_in_the_svg_only_when_filled_in() {
        let mut e = SchEditor::default();
        e.sch.labels.push(sch_core::Label { pos: (0, 0), text: "VOUT".into() });
        assert!(!e.sheet_svg().unwrap().contains("試作"), "an empty block draws nothing");
        e.sch.title_block.title = "5 V 電源".into();
        e.sch.title_block.note = "試作 1 回目".into();
        let svg = e.sheet_svg().unwrap();
        assert!(svg.contains(">5 V 電源</text>") && svg.contains(">試作 1 回目</text>") && svg.contains(">メモ</text>"), "{svg}");
        assert!(!svg.contains(">版</text>"), "an empty field has no row");
    }

    #[test]
    fn the_title_block_survives_saving_and_an_empty_one_is_not_written() {
        let mut s = sch_core::Schematic::default();
        assert!(!serde_json::to_string(&s).unwrap().contains("title_block"));
        s.title_block.author = "me".into();
        let back: sch_core::Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.title_block.author, "me");
    }
}
