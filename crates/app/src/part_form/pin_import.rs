//! The window that turns a pin table, cut out of the data sheet with a rectangle, into the pin box of the form.

use datasheet::Table;
use eframe::egui;

/// What the window gives to the form: lines of "number name", and how to put them in the pin box.
pub struct PinImport {
    pub lines: String,
    /// The page (from 0) the table was on.
    pub page: usize,
    /// Add to the pins already in the box (a number that is there stays as it is) instead of replacing them.
    pub append: bool,
}

pub enum Action {
    None,
    Cancel,
    Apply { append: bool },
}

pub struct TablePanel {
    pub page: usize,
    table: Table,
    num_col: usize,
    name_col: usize,
    /// The columns were guessed (false: no column looked like pin numbers, and the first two were taken).
    guessed: bool,
    /// The pins as lines; a person may edit them before they go in.
    pub text: String,
}

impl TablePanel {
    pub fn new(page: usize, table: Table) -> Self {
        let guess = datasheet::guess_columns(&table);
        let (num_col, name_col) = guess.unwrap_or((0, 1.min(table.columns.saturating_sub(1))));
        let mut p = TablePanel { page, table, num_col, name_col, guessed: guess.is_some(), text: String::new() };
        p.recompute();
        p
    }

    fn recompute(&mut self) {
        if self.num_col < self.table.columns && self.name_col < self.table.columns {
            self.text = datasheet::pin_lines(&datasheet::pins_from(&self.table, self.num_col, self.name_col));
        }
    }

    fn lines(&self) -> usize {
        self.text.lines().filter(|l| !l.trim().is_empty()).count()
    }

    /// "列2: 1 | 2 | 3": a column with the first cells in it, to tell the columns apart.
    fn column_label(&self, c: usize) -> String {
        let samples: Vec<String> = self.table.cells.iter().map(|r| r[c].as_str()).filter(|s| !s.is_empty()).take(3).map(|s| s.chars().take(8).collect()).collect();
        format!("列{}: {}", c + 1, samples.join(" | "))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Action {
        let mut action = Action::None;
        if self.table.columns == 0 {
            ui.colored_label(egui::Color32::from_rgb(230, 80, 70), "囲んだ中に文字がありませんでした。表をもう一度、ドラッグで囲んでください (画像の表は読めません)");
            if ui.button("閉じる").clicked() {
                action = Action::Cancel;
            }
            return action;
        }
        ui.small(format!("p.{} の表を読みました ({} 行 × {} 列)", self.page + 1, self.table.cells.len(), self.table.columns));
        if !self.guessed {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 60), "ピン番号の列が見つかりませんでした。下で、番号の列と名前の列を選んでください");
        }
        let labels: Vec<String> = (0..self.table.columns).map(|c| self.column_label(c)).collect();
        let (mut num, mut name) = (self.num_col, self.name_col);
        egui::Grid::new("pin_import_cols").num_columns(2).show(ui, |ui| {
            let choices = [
                ("番号の列", &mut num, "ピン番号 (1, 2, 3 …) が並んでいる列。パッケージごとに列があるときは、使うパッケージの列を選びます。番号と名前が1つのセルにあるときは、名前の列にも同じ列を選びます"),
                ("名前の列", &mut name, "ピンの名前 (VCC、GND、1IN+ …) が並んでいる列"),
            ];
            for (label, col, tip) in choices {
                ui.label(label);
                let shown = labels.get(*col).cloned().unwrap_or_default();
                egui::ComboBox::from_id_salt(label).selected_text(shown).width(260.0).show_ui(ui, |ui| {
                    for (c, text) in labels.iter().enumerate() {
                        ui.selectable_value(col, c, text);
                    }
                }).response.on_hover_text(tip);
                ui.end_row();
            }
        });
        let changed = (num, name) != (self.num_col, self.name_col);
        (self.num_col, self.name_col) = (num, name);
        if changed {
            self.recompute();
        }
        ui.label(format!("ピン ({} 行。直せます)", self.lines()));
        ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(10).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace))
            .on_hover_text("1行に「番号 名前」。読み違いがあれば、ここで直してから入れます。向き (left/right) は、入れたあとに「④ ピン」で書き足せます");
        egui::CollapsingHeader::new("読み取った表").id_salt("pin_import_raw").show(ui, |ui| {
            egui::ScrollArea::both().max_height(160.0).show(ui, |ui| {
                egui::Grid::new("pin_import_grid").striped(true).show(ui, |ui| {
                    for r in self.table.cells.iter().take(40) {
                        for c in r {
                            ui.small(c.chars().take(14).collect::<String>());
                        }
                        ui.end_row();
                    }
                });
            });
        });
        ui.horizontal(|ui| {
            let ok = self.lines() > 0;
            if ui.add_enabled(ok, egui::Button::new("ピン表に置き換える")).on_hover_text("「④ ピン」のピン表を、この内容に置き換えます。いまの内容は消えます").clicked() {
                action = Action::Apply { append: false };
            }
            if ui.add_enabled(ok, egui::Button::new("ピン表のあとに足す")).on_hover_text("いまのピン表に、この内容を足します。同じ番号の行は、いまのままにします").clicked() {
                action = Action::Apply { append: true };
            }
            if ui.button("やめる").clicked() {
                action = Action::Cancel;
            }
        });
        action
    }
}

/// The pin box after `lines` are put in: replaced, or added to (a pin number already there is not added again).
pub fn merged(current: &str, lines: &str, append: bool) -> String {
    if !append {
        return lines.to_string();
    }
    let number = |l: &str| l.split_whitespace().next().map(str::to_string);
    let have: Vec<String> = current.lines().filter_map(number).collect();
    let mut out: Vec<&str> = current.lines().filter(|l| !l.trim().is_empty()).collect();
    out.extend(lines.lines().filter(|l| number(l).is_some_and(|n| !have.contains(&n))));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_and_adding_pins() {
        assert_eq!(merged("1 IN\n2 GND", "1 A\n2 B\n3 C", false), "1 A\n2 B\n3 C");
        // only the numbers that are not there yet are added
        assert_eq!(merged("1 IN\n2 GND", "1 A\n2 B\n3 C", true), "1 IN\n2 GND\n3 C");
        assert_eq!(merged("", "1 A", true), "1 A");
    }

    #[test]
    fn a_panel_guesses_columns_and_follows_the_choice() {
        let rows: Vec<Vec<String>> = [["Name", "No.", "Alt"], ["VIN", "1", "9"], ["GND", "2", "8"], ["OUT", "3", "7"]].iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect();
        let mut p = TablePanel::new(2, Table { cells: rows, columns: 3 });
        assert!(p.guessed);
        assert_eq!(p.text, "1 VIN\n2 GND\n3 OUT");
        p.num_col = 2;
        p.recompute();
        assert_eq!(p.text, "7 OUT\n8 GND\n9 VIN");
        assert_eq!(p.lines(), 3);
        assert!(p.column_label(1).starts_with("列2: No."));
    }

    #[test]
    fn an_empty_table_is_not_a_panel_with_pins() {
        let p = TablePanel::new(0, Table::default());
        assert_eq!(p.lines(), 0);
    }
}
