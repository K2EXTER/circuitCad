//! The "ピン定義" window (candidates W1, W2): the signals on the microcontroller board's pins (`sch_core::pindefs`), the C header
//! for Arduino written from them, and a firmware file checked against the schematic.

use super::*;
use sch_core::pindefs::{check_firmware, header_text, pin_defs, PinFinding};

impl SchEditor {
    /// The window's contents: the list, the two buttons, and the last check's findings.
    pub(super) fn pins_contents(&mut self, ui: &mut egui::Ui, check: &mut Option<(String, Vec<PinFinding>)>) {
        let defs = pin_defs(&self.sch);
        ui.label(egui::RichText::new(i18n::tr("マイコンボードのピン (D13・A0・GPIO2 の名前のピン) につながる信号です。部品のピンの名前か、ピンヘッダの「ピンの名前」から読みます。ファーム (Arduino のスケッチ) で使うピン番号を、回路図から書き出して、手で写す間違いをなくします")).small());
        if defs.is_empty() {
            ui.colored_label(crate::theme::WARN, i18n::tr("マイコンボードのピンが見つかりません。Arduino などのモジュールの部品を置くか、ケーブルでつなぐピンヘッダの各ピンに、ボードのピンの名前 (D22・A0 など) を付けます (部品の設定の「ピンの名前」)"));
            return;
        }
        egui::ScrollArea::vertical().max_height(260.0).id_salt("pin_defs").show(ui, |ui| {
            egui::Grid::new("pin_defs_grid").striped(true).num_columns(4).show(ui, |ui| {
                for h in ["定数", "ピン", "番号", "つながる先"] {
                    ui.strong(i18n::tr(h));
                }
                ui.end_row();
                for d in &defs {
                    ui.monospace(&d.name);
                    ui.label(format!("{} ({})", d.pin, d.at));
                    ui.monospace(&d.value);
                    ui.label(d.others.join("、"));
                    ui.end_row();
                }
            });
        });
        ui.horizontal(|ui| {
            if ui
                .button(i18n::tr("Arduino 用のヘッダ (.h) を書き出す…"))
                .on_hover_text(i18n::tr("ピンごとに #define の行を並べた C のヘッダを書き出します。スケッチで #include すると、ピン番号の代わりに名前で使えます。回路図を変えたら、書き出し直します (このファイルは手で直さず、回路図を直します)"))
                .clicked()
            {
                let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
                if let Some(p) = rfd::FileDialog::new().add_filter("C ヘッダ", &["h"]).set_file_name(format!("{stem}_pins.h")).save_file() {
                    self.status = match std::fs::write(&p, header_text(&self.sch, &stem)) {
                        Ok(()) => i18n::trf!("ピン定義を書き出しました: {}", p.display()),
                        Err(e) => i18n::trf!("ピン定義を書き出せませんでした: {}。保存先のフォルダに書き込めるか確認してください", e),
                    };
                }
            }
            if ui
                .button(i18n::tr("ファームのファイルと照合…"))
                .on_hover_text(i18n::tr("スケッチ (.ino) やヘッダ (.h・.cpp) の #define と const int のピン番号を読み、回路図とずれている所を出します (例: LED_PIN は 13 番だが、回路図では D12)"))
                .clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("ファーム", &["ino", "h", "hpp", "c", "cpp"]).pick_file()
            {
                *check = Some(match std::fs::read_to_string(&p) {
                    Ok(text) => (p.display().to_string(), check_firmware(&self.sch, &text)),
                    Err(e) => (p.display().to_string(), vec![PinFinding { message: i18n::trf!("ファイルを読めませんでした: {}", e), hint: String::new(), line: 0, wrong: true }]),
                });
            }
        });
        if let Some((file, findings)) = check.as_ref() {
            ui.separator();
            ui.label(i18n::trf!("照合: {}", file));
            if findings.iter().all(|f| !f.wrong) {
                ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図とずれているピンはありません"));
            }
            for f in findings {
                let colour = if f.wrong { crate::theme::ERROR } else { ui.visuals().weak_text_color() };
                ui.label(egui::RichText::new(format!("{} {}", if f.wrong { "✖" } else { "・" }, i18n::tr_dyn(&f.message))).color(colour)).on_hover_text(i18n::tr_dyn(&f.hint));
            }
        }
    }
}
