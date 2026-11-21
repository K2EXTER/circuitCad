//! The "ピン定義" window (candidates W1, W2): the signals on the microcontroller board's pins (`sch_core::pindefs`), written as a C
//! header for Arduino, a Zephyr devicetree overlay, or a chip's pins as an FPGA's constraint file; and a firmware file checked
//! against the schematic. The file written and the firmware checked are kept with the schematic for the ERC.

use super::*;
use sch_core::pindefs::{check_firmware, chip_signals, firmware_problems, mark_written, pin_defs, Constraints, PinFile, PinFinding};

/// The formats in the menu: the Arduino header, the Zephyr overlay, then the FPGA constraint files.
fn format_name(k: usize) -> String {
    match k {
        0 => i18n::tr("Arduino 用のヘッダ (.h)").to_string(),
        1 => i18n::tr("Zephyr の devicetree overlay (.overlay)").to_string(),
        k => Constraints::ALL.get(k - 2).map(|c| format!("FPGA: {} (.{})", c.tools(), c.ext())).unwrap_or_default(),
    }
}

impl SchEditor {
    /// The window's contents: the list, the two buttons, and the last check's findings.
    pub(super) fn pins_contents(&mut self, ui: &mut egui::Ui, check: &mut Option<(String, Vec<PinFinding>)>, format: &mut usize, part: &mut String) {
        let defs = pin_defs(&self.sch);
        // what the ERC would say: the file written last is out of date, the firmware drives another pin
        for p in firmware_problems(&self.sch, self.path.as_ref().and_then(|p| p.parent())) {
            ui.colored_label(crate::theme::WARN, format!("▲ {}", i18n::tr_dyn(&p.message))).on_hover_text(i18n::tr_dyn(&p.hint));
        }
        ui.label(egui::RichText::new(i18n::tr("マイコンボードのピン (D13・A0・GPIO2 の名前のピン) につながる信号です。部品のピンの名前か、ピンヘッダの「ピンの名前」から読みます。ファーム (Arduino のスケッチ) で使うピン番号を、回路図から書き出して、手で写す間違いをなくします")).small());
        let fpga = *format >= 2;
        if defs.is_empty() && !fpga {
            ui.colored_label(crate::theme::WARN, i18n::tr("マイコンボードのピンが見つかりません。Arduino などのモジュールの部品を置くか、ケーブルでつなぐピンヘッダの各ピンに、ボードのピンの名前 (D22・A0 など) を付けます (部品の設定の「ピンの名前」)。FPGA のピンは、下の形式で FPGA を選びます"));
        }
        ui.horizontal(|ui| {
            ui.label(i18n::tr("書き出す形式"));
            egui::ComboBox::from_id_salt("pin_format").selected_text(format_name(*format)).show_ui(ui, |ui| {
                for k in 0..2 + Constraints::ALL.len() {
                    ui.selectable_value(format, k, format_name(k));
                }
            })
            .response
            .on_hover_text(i18n::tr("Arduino はスケッチで #include するヘッダ、Zephyr はボードの devicetree に重ねる overlay (zephyr,user の …-gpios)。FPGA は、選んだ部品のピン番号 (パッケージのピン) とネットの名前を、そのツールの制約ファイルに書きます"));
            if fpga {
                // the chips with pins of their own (an FPGA, a CPLD): the bigger first
                let mut chips: Vec<(usize, String)> = self.sch.flat().components.iter().filter_map(|c| c.custom.as_ref().filter(|x| x.pins.len() >= 8).map(|x| (x.pins.len(), c.refdes.clone()))).collect();
                chips.sort_by_key(|c| std::cmp::Reverse(c.0));
                if part.is_empty() || !chips.iter().any(|c| c.1 == *part) {
                    *part = chips.first().map(|c| c.1.clone()).unwrap_or_default();
                }
                egui::ComboBox::from_id_salt("pin_part").selected_text(part.clone()).show_ui(ui, |ui| {
                    for (n, r) in &chips {
                        ui.selectable_value(part, r.clone(), format!("{r} ({n})"));
                    }
                })
                .response
                .on_hover_text(i18n::tr("ピンを書き出す部品 (FPGA など)。電源・GND のピンと、ほかの部品につながらないピンは外します"));
            }
        });
        if fpga {
            match chip_signals(&self.sch, part) {
                Ok(sig) => {
                    egui::ScrollArea::vertical().max_height(260.0).id_salt("chip_pins").show(ui, |ui| {
                        egui::Grid::new("chip_pins_grid").striped(true).num_columns(3).show(ui, |ui| {
                            for h in ["ポート", "ピン", "ネット"] {
                                ui.strong(i18n::tr(h));
                            }
                            ui.end_row();
                            for (port, pin, net) in &sig {
                                ui.monospace(port);
                                ui.monospace(pin);
                                ui.label(net);
                                ui.end_row();
                            }
                        });
                    });
                }
                Err(e) => {
                    ui.colored_label(crate::theme::WARN, i18n::tr_dyn(&e));
                }
            }
        }
        if !fpga && !defs.is_empty() {
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
        }
        ui.horizontal(|ui| {
            if ui
                .button(i18n::tr("書き出す…"))
                .on_hover_text(i18n::tr("選んだ形式でピン定義を書き出します。ファームで読むと、ピン番号の代わりに名前で使えます。回路図を変えたら書き出し直します (このファイルは手で直さず、回路図を直します)。書き出したことは回路図に残り、そのあとピンが変わると ERC が知らせます"))
                .clicked()
            {
                let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "board".into());
                let (kind, ext, name) = match *format {
                    0 => (PinFile::Header, "h", format!("{stem}_pins.h")),
                    1 => (PinFile::Zephyr, "overlay", format!("{stem}.overlay")),
                    k => {
                        let c = Constraints::ALL[(k - 2).min(Constraints::ALL.len() - 1)];
                        (PinFile::Fpga { part: part.clone(), format: c }, c.ext(), format!("{stem}.{}", c.ext()))
                    }
                };
                match kind.text(&self.sch, &stem) {
                    Err(e) => self.status = i18n::tr_dyn(&e).to_string(),
                    Ok(text) => {
                        if let Some(p) = rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(name).save_file() {
                            self.status = match std::fs::write(&p, text) {
                                Ok(()) => {
                                    mark_written(&mut self.sch, &p.display().to_string(), &kind);
                                    i18n::trf!("ピン定義を書き出しました: {}", p.display())
                                }
                                Err(e) => i18n::trf!("ピン定義を書き出せませんでした: {}。保存先のフォルダに書き込めるか確認してください", e),
                            };
                        }
                    }
                }
            }
            if ui
                .button(i18n::tr("ファームのファイルと照合…"))
                .on_hover_text(i18n::tr("スケッチ (.ino) やヘッダ (.h・.cpp) の #define と const int のピン番号を読み、回路図とずれている所を出します (例: LED_PIN は 13 番だが、回路図では D12)"))
                .clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("ファーム", &["ino", "h", "hpp", "c", "cpp"]).pick_file()
            {
                // kept with the schematic (relative to its folder when it is inside it): the ERC checks it again
                let dir = self.path.as_ref().and_then(|x| x.parent()).map(|d| d.to_path_buf());
                self.sch.firmware_link.firmware = dir.and_then(|d| p.strip_prefix(&d).ok().map(|r| r.display().to_string())).unwrap_or_else(|| p.display().to_string());
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
