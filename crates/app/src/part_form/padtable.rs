//! The table of the extra pads and holes (shield legs, locating holes, slots) in the "④ 部品の形" tab. As in the pin table
//! (`pintable.rs`), the text in `extras` stays the stored form: rows are read from it and written back when a row is edited.

use super::*;

/// What a row is. `Raw` keeps a line this table does not understand (a comment, a typo) as it is.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ExtraKind {
    /// 脚: a plated hole with a pad (a shield leg).
    #[default]
    Leg,
    /// パッド: a surface mount pad.
    Pad,
    /// 穴: a hole without copper.
    Hole,
    Raw,
}

/// One row. Numbers are text so that a half-typed number is kept as it is.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ExtraRow {
    pub kind: ExtraKind,
    pub pin: String,
    pub x: String,
    pub y: String,
    pub w: String,
    pub h: String,
    pub drill: String,
    pub slot: String,
    pub vertical: bool,
    /// The whole line of a `Raw` row.
    pub raw: String,
}

/// The rows of an extras text (empty lines are left out).
pub(super) fn rows_of(text: &str) -> Vec<ExtraRow> {
    text.lines().filter(|l| !l.trim().is_empty()).map(row_of).collect()
}

fn row_of(line: &str) -> ExtraRow {
    let raw = || ExtraRow { kind: ExtraKind::Raw, raw: line.trim().to_string(), ..ExtraRow::default() };
    if line.trim_start().starts_with('#') {
        return raw();
    }
    let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
    // a trailing v / h says which way a slot runs
    let (t, vertical) = match t.last().map(|s| s.to_ascii_lowercase()) {
        Some(o) if matches!(o.as_str(), "v" | "縦") => (&t[..t.len() - 1], true),
        Some(o) if matches!(o.as_str(), "h" | "横") => (&t[..t.len() - 1], false),
        _ => (&t[..], false),
    };
    let at = |i: usize| t.get(i).copied().unwrap_or("").to_string();
    match t.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("脚" | "leg" | "pth" | "th") => ExtraRow { kind: ExtraKind::Leg, pin: at(1), x: at(2), y: at(3), w: at(4), h: at(5), drill: at(6), slot: at(7), vertical, raw: String::new() },
        Some("パッド" | "pad" | "smd") => ExtraRow { kind: ExtraKind::Pad, pin: at(1), x: at(2), y: at(3), w: at(4), h: at(5), ..ExtraRow::default() },
        Some("穴" | "hole" | "npth") => ExtraRow { kind: ExtraKind::Hole, x: at(1), y: at(2), drill: at(3), slot: at(4), vertical, ..ExtraRow::default() },
        _ => raw(),
    }
}

/// The extras text of the rows.
pub(super) fn text_of(rows: &[ExtraRow]) -> String {
    rows.iter().map(line_of).collect::<Vec<_>>().join("\n")
}

fn line_of(r: &ExtraRow) -> String {
    // a slot length, or "0" when only the direction is given
    let slot = |r: &ExtraRow| match (r.slot.is_empty(), r.vertical) {
        (true, false) => String::new(),
        (true, true) => " 0 v".to_string(),
        (false, false) => format!(" {}", r.slot),
        (false, true) => format!(" {} v", r.slot),
    };
    match r.kind {
        ExtraKind::Raw => r.raw.clone(),
        ExtraKind::Leg => format!("脚 {} {} {} {} {} {}{}", r.pin, r.x, r.y, r.w, r.h, r.drill, slot(r)),
        ExtraKind::Pad => format!("パッド {} {} {} {} {}", r.pin, r.x, r.y, r.w, r.h),
        ExtraKind::Hole => format!("穴 {} {} {}{}", r.x, r.y, r.drill, slot(r)),
    }
}

fn kind_label(k: ExtraKind) -> &'static str {
    match k {
        ExtraKind::Leg => "脚 (めっき穴)",
        ExtraKind::Pad => "面実装パッド",
        ExtraKind::Hole => "穴 (めっきなし)",
        ExtraKind::Raw => "(読めない行)",
    }
}

impl PartForm {
    /// The rows follow the text: read again when something else (an example button, the paste box) changed it.
    fn sync_extra_rows(&mut self) {
        if self.extra_rows_of.as_deref() != Some(self.extras.as_str()) {
            self.extra_rows = rows_of(&self.extras);
            self.extra_rows_of = Some(self.extras.clone());
        }
    }

    /// The table of the extra pads and holes. Writes `extras`.
    pub(super) fn extras_table_ui(&mut self, ui: &mut egui::Ui) {
        self.sync_extra_rows();
        let mut rows = self.extra_rows.clone();
        let n = rows.len();
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        if n > 0 {
            ui.small(i18n::tr("座標は部品の中心から (右が +X、上が +Y)、単位は mm。ピン名は「③ ピン」の番号と同じにします"));
            egui::Grid::new("extras_table").num_columns(10).spacing([6.0, 4.0]).striped(true).show(ui, |ui| {
                for h in ["種類", "ピン名", "X", "Y", "幅", "高さ", "穴径", "長穴の長さ", "縦", ""] {
                    ui.small(h);
                }
                ui.end_row();
                for i in 0..n {
                    let kind = rows[i].kind;
                    if kind == ExtraKind::Raw {
                        ui.small(kind_label(kind)).on_hover_text(i18n::tr("この表で読めない行です (# で始まるコメントなど)。「文字で編集」で直せます"));
                        ui.add(egui::TextEdit::singleline(&mut rows[i].raw).desired_width(330.0).min_size(egui::vec2(330.0, 0.0)).font(egui::TextStyle::Monospace));
                        for _ in 0..7 {
                            ui.label("");
                        }
                    } else {
                        let mut k = kind;
                        egui::ComboBox::from_id_salt(("extra_kind", i)).selected_text(kind_label(kind)).width(110.0).show_ui(ui, |ui| {
                            for c in [ExtraKind::Leg, ExtraKind::Pad, ExtraKind::Hole] {
                                ui.selectable_value(&mut k, c, kind_label(c));
                            }
                        }).response.on_hover_text(i18n::tr("脚はめっきのある穴と銅のパッド (USBのシェル脚など)、面実装パッドは穴のない銅のパッド、穴はめっきなしの穴 (位置決め・ネジ穴) です"));
                        rows[i].kind = k;
                        let has_pin = k != ExtraKind::Hole;
                        let has_size = k != ExtraKind::Hole;
                        let has_drill = k != ExtraKind::Pad;
                        let field = |ui: &mut egui::Ui, v: &mut String, on: bool, w: f32, tip: &str| {
                            ui.add_enabled(on, egui::TextEdit::singleline(v).desired_width(w).min_size(egui::vec2(w, 0.0)).font(egui::TextStyle::Monospace)).on_hover_text(tip);
                        };
                        field(ui, &mut rows[i].pin, has_pin, 44.0, i18n::tr("ピン名 (「③ ピン」の番号と同じ。例: SH)"));
                        field(ui, &mut rows[i].x, true, 46.0, i18n::tr("X (mm)。部品の中心から右が +"));
                        field(ui, &mut rows[i].y, true, 46.0, i18n::tr("Y (mm)。部品の中心から上が +"));
                        field(ui, &mut rows[i].w, has_size, 46.0, i18n::tr("パッドの幅 (mm)"));
                        field(ui, &mut rows[i].h, has_size, 46.0, i18n::tr("パッドの高さ (mm)"));
                        field(ui, &mut rows[i].drill, has_drill, 46.0, i18n::tr("穴径 (mm)"));
                        field(ui, &mut rows[i].slot, has_drill, 46.0, i18n::tr("長穴にするときの長さ (mm)。丸い穴なら空"));
                        ui.add_enabled(has_drill, egui::Checkbox::without_text(&mut rows[i].vertical)).on_hover_text(i18n::tr("長穴を縦向きにします (オフなら横向き)"));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new("↑").small()).on_hover_text(i18n::tr("1つ上へ")).clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("↓").small()).on_hover_text(i18n::tr("1つ下へ")).clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.add(egui::Button::new(i18n::tr("消す")).small()).on_hover_text(i18n::tr("この行を消します")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
        }
        if let Some((a, b)) = swap {
            rows.swap(a, b);
        }
        if let Some(i) = remove {
            rows.remove(i);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("足す:"));
            for (label, tip, kind, defaults) in [
                ("＋ 脚 (めっき穴)", "コネクタの金属の外側 (シェル) をはんだ付けする脚など。穴と、まわりの銅のパッドです", ExtraKind::Leg, ["SH", "-6.5", "0", "2.0", "3.0", "1.2"]),
                ("＋ 面実装パッド", "面実装 (SMD) タイプの固定用パッドなど。穴のない銅のパッドです", ExtraKind::Pad, ["SH", "-4", "-2", "1.5", "2.0", ""]),
                ("＋ 穴 (めっきなし)", "部品の突起が入る位置決めの穴、ネジ穴。回路にはつながりません", ExtraKind::Hole, ["", "0", "-3", "", "", "1.0"]),
            ] {
                if ui.small_button(label).on_hover_text(i18n::trf!("{tip}\n入る数字は例です。データシートの推奨ランドパターンの寸法に直してください")).clicked() {
                    let [pin, x, y, w, h, drill] = defaults.map(String::from);
                    rows.push(ExtraRow { kind, pin, x, y, w, h, drill, ..ExtraRow::default() });
                }
            }
        });
        if rows != self.extra_rows {
            self.extras = text_of(&rows);
            self.extra_rows = rows;
            self.extra_rows_of = Some(self.extras.clone());
        }
    }
}
