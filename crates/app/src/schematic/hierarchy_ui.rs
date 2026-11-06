//! The screen side of the sheet hierarchy (`sch-core/src/hierarchy.rs`): put a sheet symbol on the sheet, open the sheet it stands
//! for and come back, and the ports of a child sheet (drawn as flags, placed with the label tool, edited when selected).

use super::*;
use sch_core::{PortDir, SheetPort};

const PORT_FILL: Color32 = Color32::from_rgb(255, 236, 196);
const PORT_EDGE: Color32 = Color32::from_rgb(200, 120, 20);
const PORT_TEXT: Color32 = Color32::from_rgb(120, 60, 0);

fn dir_mark(d: PortDir) -> &'static str {
    match d {
        PortDir::In => "入力",
        PortDir::Out => "出力",
        PortDir::Io => "入出力",
    }
}

impl SchEditor {
    /// Put the port named by the label text at `at` (the label tool with "ポート" on).
    pub(super) fn place_port(&mut self, at: Pt, dir: PortDir) {
        let name = self.label_text.trim().to_string();
        if name.is_empty() {
            self.status = "ポートの名前が空です。左の欄に、ポートの名前 (IN、OUT、EN など) を書きます".into();
        } else if self.sch.ports.iter().any(|p| p.name == name) {
            self.status = format!("このシートには、ポート {name} がもうあります。別の名前にしてください (同じ名前では、記号のピンが1つにまとまります)");
        } else {
            self.sch.ports.push(SheetPort { pos: at, name: name.clone(), dir });
            self.sch.refresh_sheet_symbols();
            self.status = format!("ポート {name} を置きました。このシートを記号にして別のシートに置くと、ピンになります");
        }
    }

    /// Open the sheet a symbol stands for (double click on it); "親へ戻る" goes back.
    pub(super) fn enter_sheet(&mut self, k: usize) {
        let from = self.sch.active_sheet;
        if k != from && k < self.sch.sheet_count() {
            self.show_sheet(k);
            self.sheet_trail.push(from);
            self.status = format!("{} を開きました。上の「親へ戻る」で、もとのシートに戻れます", self.sch.sheet_name(k));
        }
    }

    /// Back to the sheet a symbol was opened from.
    pub(super) fn leave_sheet(&mut self) {
        if let Some(k) = self.sheet_trail.pop() {
            self.show_sheet(k);
        }
    }

    /// Put a symbol of sheet `k` on the sheet being edited, below what is there, and select it.
    pub(super) fn place_sheet_symbol(&mut self, k: usize) {
        let y = self.sch.components.iter().flat_map(|c| c.pins()).map(|(_, p)| p.1).max().map_or(10, |y| y + 10);
        match self.sch.add_sheet_symbol(k, (12, y)) {
            Some(i) => {
                self.selected = Some(Sel::Comp(i));
                self.multi.clear();
                let c = &self.sch.components[i];
                self.status = format!("{} を置きました。ピンに線かラベルをつなぎます。ダブルクリックで、そのシートの中を開けます", c.refdes);
            }
            None => self.status = "このシートの記号は、このシートには置けません (自分自身は入れられません)".into(),
        }
    }

    pub(super) fn draw_ports(&self, painter: &egui::Painter, rect: Rect) {
        for (i, port) in self.sch.ports.iter().enumerate() {
            let sel = self.is_sel(Sel::Port(i));
            let p = self.to_screen(rect, (port.pos.0 as f32, port.pos.1 as f32));
            let text = match port.dir {
                PortDir::In => format!("> {}", port.name),
                PortDir::Out => format!("{} >", port.name),
                PortDir::Io => format!("<> {}", port.name),
            };
            let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), PORT_TEXT);
            let (w, h) = (galley.size().x, 8.0);
            let pts = vec![p, p + Vec2::new(6.0, -h), p + Vec2::new(12.0 + w, -h), p + Vec2::new(12.0 + w, h), p + Vec2::new(6.0, h)];
            painter.add(egui::Shape::convex_polygon(pts, PORT_FILL, egui::Stroke::new(if sel { 2.5 } else { 1.2 }, if sel { Color32::from_rgb(0, 80, 220) } else { PORT_EDGE })));
            painter.galley(p + Vec2::new(9.0, -galley.size().y / 2.0), galley, PORT_TEXT);
        }
    }

    /// The label tool's port switch (shown with several sheets: a port is the entrance of a sheet used as a child).
    pub(super) fn port_tool_ui(&mut self, ui: &mut egui::Ui) {
        if self.sch.sheet_count() < 2 {
            return;
        }
        let mut on = self.label_port.is_some();
        if ui.checkbox(&mut on, "ポートにする (子シートの出入口)").on_hover_text("オンにすると、このシートの出入口 (ポート) を置きます。このシートを記号にして別のシートに置くと、ポートが記号のピンになります (シートのタブを右クリック → 記号として置く)。同じシートを何回も使えて、直すのは1か所です").changed() {
            self.label_port = on.then_some(PortDir::Io);
        }
        if let Some(dir) = self.label_port.as_mut() {
            ui.horizontal(|ui| {
                for d in [PortDir::In, PortDir::Out, PortDir::Io] {
                    ui.selectable_value(dir, d, dir_mark(d)).on_hover_text(match d {
                        PortDir::In => "信号が入ってくるポート。記号の左側にピンが出ます",
                        PortDir::Out => "信号が出ていくポート。記号の右側にピンが出ます",
                        PortDir::Io => "入りも出もするポート (電源・双方向の線など)。記号の左側にピンが出ます",
                    });
                }
            });
        }
    }

    /// The settings of the selected port: its name (the pin of the symbol) and its direction.
    pub(super) fn port_editor_ui(&mut self, ui: &mut egui::Ui) {
        let Some(Sel::Port(i)) = self.selected else { return };
        let Some(port) = self.sch.ports.get(i).cloned() else { return };
        ui.label(egui::RichText::new("選択中のポート").strong());
        let mut name = port.name.clone();
        let mut dir = port.dir;
        ui.horizontal(|ui| {
            ui.label("名前");
            ui.text_edit_singleline(&mut name).on_hover_text("ポートの名前。このシートを記号にしたとき、同じ名前のピンになります");
        });
        ui.horizontal(|ui| {
            for d in [PortDir::In, PortDir::Out, PortDir::Io] {
                ui.selectable_value(&mut dir, d, dir_mark(d));
            }
        });
        let taken = self.sch.ports.iter().enumerate().any(|(k, p)| k != i && p.name == name);
        if taken {
            ui.colored_label(crate::theme::WARN, "同じ名前のポートが、このシートにあります");
        }
        if name != port.name || dir != port.dir {
            self.sch.ports[i].name = name;
            self.sch.ports[i].dir = dir;
            self.sch.refresh_sheet_symbols();
        }
        if ui.button("ポートを削除").on_hover_text("このポートを消します。記号のピンも消えます (Delete キーでも同じです)").clicked() {
            self.delete_selected();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two sheets: the first (shown) is the parent, the second a child.
    fn editor() -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = Schematic::default();
        e.sch.add_sheet();
        e.sch.switch_sheet(0);
        e
    }

    #[test]
    fn a_port_is_put_once_per_name_and_gives_the_symbol_a_pin() {
        let mut e = editor();
        e.show_sheet(1);
        e.label_text = "IN".into();
        e.place_port((4, 4), PortDir::In);
        e.place_port((4, 8), PortDir::In);
        assert_eq!(e.sch.ports.len(), 1, "the same name twice is refused");
        e.label_text = "OUT".into();
        e.place_port((20, 4), PortDir::Out);
        e.show_sheet(0);
        e.place_sheet_symbol(1);
        let ch = &e.sch.components[0];
        assert_eq!(ch.refdes, "CH1");
        assert_eq!(e.selected, Some(Sel::Comp(0)));
        let pins: Vec<&str> = ch.custom.as_ref().unwrap().pins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(pins, ["IN", "OUT"]);
    }

    #[test]
    fn deleting_a_port_takes_the_pin_off_the_symbols() {
        let mut e = editor();
        e.show_sheet(1);
        e.label_text = "IN".into();
        e.place_port((4, 4), PortDir::In);
        e.show_sheet(0);
        e.place_sheet_symbol(1);
        e.show_sheet(1);
        e.selected = Some(Sel::Port(0));
        e.delete_selected();
        assert!(e.sch.ports.is_empty());
        e.show_sheet(0);
        assert!(e.sch.components[0].custom.as_ref().unwrap().pins.is_empty());
    }

    #[test]
    fn opening_a_symbol_and_going_back_returns_to_the_parent() {
        let mut e = editor();
        e.place_sheet_symbol(1);
        e.enter_sheet(1);
        assert_eq!(e.sch.active_sheet, 1);
        assert_eq!(e.sheet_trail, [0]);
        e.leave_sheet();
        assert_eq!(e.sch.active_sheet, 0);
        assert!(e.sheet_trail.is_empty());
    }

    #[test]
    fn a_sheet_symbol_of_the_sheet_itself_is_refused() {
        let mut e = editor();
        e.place_sheet_symbol(0);
        assert!(e.sch.components.is_empty());
        assert!(e.status.contains("置けません"), "{}", e.status);
    }
}
