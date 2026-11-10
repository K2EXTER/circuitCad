//! The pin table of the "③ ピン" tab: one row per pin (number, name, side, the terminal of the SPICE model it is). The text in
//! `pins_text` (and `sim_pins`) stays the stored form, so the templates, the data sheet import and the saved parts keep working;
//! the rows are read from the text and written back to it when a row is edited.

use super::*;

/// Shown for a terminal that has no pin yet (also what is written into `sim_pins`, so the form says what is missing).
const OPEN: &str = "?";

const SIDES: [(&str, &str); 5] = [("", "自動"), ("left", "左"), ("right", "右"), ("top", "上"), ("bottom", "下")];

/// One row of the table. Fields are text so that a half-typed row is kept as it is.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PinRow {
    pub number: String,
    pub name: String,
    pub side: String,
}

/// The rows of a pin text (a line with only a number gives a row with an empty name).
pub(super) fn rows_of(text: &str) -> Vec<PinRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let t: Vec<&str> = l.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            let side = t.get(2).and_then(|s| parse_side(s)).map_or("", side_word);
            PinRow { number: t.first().copied().unwrap_or("").into(), name: t.get(1).copied().unwrap_or("").into(), side: side.into() }
        })
        .collect()
}

/// The pin text of the rows (a row with nothing in it is left out).
pub(super) fn text_of(rows: &[PinRow]) -> String {
    rows.iter()
        .filter(|r| !(r.number.is_empty() && r.name.is_empty()))
        .map(|r| if r.side.is_empty() { format!("{} {}", r.number, r.name) } else { format!("{} {} {}", r.number, r.name, r.side) })
        .collect::<Vec<_>>()
        .join("\n")
}

fn side_word(s: PinSide) -> &'static str {
    match s {
        PinSide::Left => "left",
        PinSide::Right => "right",
        PinSide::Top => "top",
        PinSide::Bottom => "bottom",
    }
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Spaces and commas would split a field into two on the way back to the text.
fn clean(s: &mut String) {
    s.retain(|c| !c.is_whitespace() && c != ',');
}

impl PartForm {
    /// The rows follow the text: read again when something else (a template, the data sheet import, the paste box) changed it.
    fn sync_pin_rows(&mut self) {
        if self.pin_rows_of.as_deref() != Some(self.pins_text.as_str()) {
            self.pin_rows = rows_of(&self.pins_text);
            self.pin_rows_of = Some(self.pins_text.clone());
        }
    }

    /// The table. Writes `pins_text`, and `sim_pins` for the model terminal column.
    pub(super) fn pin_table_ui(&mut self, ui: &mut egui::Ui) {
        self.sync_pin_rows();
        let roles = guess_spice(&self.sim_text).map(|g| g.roles).unwrap_or_default();
        let mut cur = tokens(&self.sim_pins);
        if !roles.is_empty() {
            cur.resize(roles.len().max(cur.len()), OPEN.into());
        }
        let mut rows = self.pin_rows.clone();
        let mut sim = cur.clone();
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let n = rows.len();
        let numbers: Vec<String> = rows.iter().map(|r| r.number.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            crate::icons::help_label(ui, "ピン", "1行が1本のピンです。番号はパッケージのパッドの番号 (データシートのピン番号)、名前は回路図に出る名前です。向きは記号のどちらの辺に出すかで、「自動」なら前半が左、後半が右になります。NC は未接続のピンです");
            if !roles.is_empty() {
                ui.small(i18n::tr("「モデルの端子」は、貼った SPICE モデルの端子が、どのピンにあたるかです (選ばないピンは、シミュレーションではどこにもつながりません)"));
            }
        });
        egui::Grid::new("pin_table").num_columns(if roles.is_empty() { 5 } else { 6 }).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
            ui.small("");
            ui.small(i18n::tr("番号"));
            ui.small(i18n::tr("名前"));
            ui.small(i18n::tr("向き"));
            if !roles.is_empty() {
                ui.small(i18n::tr("モデルの端子"));
            }
            ui.small("");
            ui.end_row();
            for i in 0..n {
                ui.small(format!("{}", i + 1));
                let twice = !rows[i].number.is_empty() && numbers.iter().filter(|x| **x == rows[i].number).count() > 1;
                let before = rows[i].number.clone();
                // min_size: a text box in a grid column (not the last) otherwise stays as narrow as the column's first frame
                let r = ui
                    .add(egui::TextEdit::singleline(&mut rows[i].number).desired_width(52.0).min_size(egui::vec2(52.0, 0.0)).font(egui::TextStyle::Monospace))
                    .on_hover_text(if twice { i18n::tr("同じ番号のピンがあります。番号は重ならないようにします") } else { i18n::tr("パッドの番号 (データシートのピン番号。BGA なら A1 のような名前)。同じ番号は2つ使えません") });
                if r.changed() {
                    clean(&mut rows[i].number);
                    // the model terminals follow the renamed pin
                    if !before.is_empty() {
                        for s in sim.iter_mut().filter(|s| **s == before) {
                            *s = rows[i].number.clone();
                        }
                    }
                }
                let name_edit = ui
                    .add(egui::TextEdit::singleline(&mut rows[i].name).desired_width(130.0).min_size(egui::vec2(130.0, 0.0)).font(egui::TextStyle::Monospace))
                    .on_hover_text(i18n::tr("ピンの名前 (IN・GND・OUT・IN+ など)。回路図の部品に出ます"));
                if name_edit.changed() {
                    clean(&mut rows[i].name);
                }
                let shown = SIDES.iter().find(|(k, _)| *k == rows[i].side).map_or("自動", |(_, l)| *l);
                egui::ComboBox::from_id_salt(("pin_side", i))
                    .selected_text(i18n::tr(shown))
                    .width(56.0)
                    .show_ui(ui, |ui| {
                        for (k, label) in SIDES {
                            ui.selectable_value(&mut rows[i].side, k.to_string(), i18n::tr(label));
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("記号のどちらの辺にこのピンを出すか。「自動」なら、前半が左、後半が右です"));
                if !roles.is_empty() {
                    let mine = if rows[i].number.is_empty() { None } else { sim.iter().position(|s| *s == rows[i].number) };
                    let label = |j: usize| i18n::trf!("端子 {} ({})", j + 1, roles.get(j).map_or("", |s| s.as_str()));
                    let mut pick = mine;
                    egui::ComboBox::from_id_salt(("pin_term", i))
                        .selected_text(mine.map_or(i18n::tr("(使わない)").to_string(), label))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut pick, None, i18n::tr("(使わない)"));
                            for j in 0..roles.len() {
                                ui.selectable_value(&mut pick, Some(j), label(j));
                            }
                        })
                        .response
                        .on_hover_text(i18n::tr("このピンが、モデルのどの端子にあたるか。同じ端子を2つのピンに選ぶと、前に選んだピンは「使わない」に戻ります"));
                    if pick != mine && !rows[i].number.is_empty() {
                        if let Some(j) = mine {
                            sim[j] = OPEN.into();
                        }
                        if let Some(j) = pick {
                            sim[j] = rows[i].number.clone();
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("↑").small()).on_hover_text(i18n::tr("1つ上へ")).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("↓").small()).on_hover_text(i18n::tr("1つ下へ")).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.add(egui::Button::new(i18n::tr("消す")).small()).on_hover_text(i18n::tr("このピンを消します")).clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
        if let Some((a, b)) = swap {
            rows.swap(a, b);
        }
        if let Some(i) = remove {
            let gone = rows.remove(i).number;
            if !gone.is_empty() {
                for s in sim.iter_mut().filter(|s| **s == gone) {
                    *s = OPEN.into();
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("＋ ピンを足す")).on_hover_text(i18n::tr("表の最後に、空のピンを1行足します。番号は、いまの最大の番号の次になります")).clicked() {
                let next = rows.iter().filter_map(|r| r.number.parse::<u32>().ok()).max().map_or(1, |m| m + 1);
                // when every pin is on one side (a header), the new one goes there too instead of splitting the box
                let side = rows.first().map(|r| r.side.clone()).filter(|s| !s.is_empty() && rows.iter().all(|r| r.side == *s)).unwrap_or_default();
                rows.push(PinRow { number: next.to_string(), side, ..PinRow::default() });
            }
            if !roles.is_empty() && ui.button(i18n::tr("名前で合わせる")).on_hover_text(i18n::tr("ピンの名前 (か番号) が、モデルの端子の名前と同じものを、自動で選びます。合うものがない端子は、選び直しになります")).clicked() {
                for (j, role) in roles.iter().enumerate() {
                    sim[j] = rows.iter().find(|p| p.name.eq_ignore_ascii_case(role) || p.number.eq_ignore_ascii_case(role)).map_or(OPEN.into(), |p| p.number.clone());
                }
            }
        });
        if !roles.is_empty() {
            let unused: Vec<&str> = rows.iter().filter(|p| !p.number.is_empty() && !sim.contains(&p.number)).map(|p| p.name.as_str()).collect();
            if !unused.is_empty() {
                ui.small(i18n::trf!("モデルに使わないピン: {}", unused.join(" "))).on_hover_text(i18n::tr("これらのピンは、シミュレーションでは何にもつながりません (基板では、パッドとしてそのまま使えます)。つながない部品なら、回路図で「使わないピン (NC)」にします"));
            }
        }
        if rows != self.pin_rows {
            self.pins_text = text_of(&rows);
            self.pin_rows = rows;
            self.pin_rows_of = Some(self.pins_text.clone());
        }
        if sim != cur {
            if !roles.is_empty() {
                sim.truncate(roles.len());
            }
            self.sim_pins = sim.join(" ");
        }
    }
}
