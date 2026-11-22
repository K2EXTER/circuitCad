//! The sketch of each microcontroller board on the breadboard (B6 ⑤): written in the proto tab's left panel, saved with the sheet
//! (`SchEditor::sketches`), compiled as it is typed (`sim_core::sketch`), and run beside the circuit while the power is on
//! (`live.rs`): the serial monitor shows what it prints.

use super::*;
use sch_core::Schematic;

/// What a new board's sketch starts as: the built-in LED (D13) blinking.
pub const BLINK: &str = "// LED_BUILTIN (D13) を 0.5 秒ごとに点けたり消したりします\nvoid setup() {\n  pinMode(LED_BUILTIN, OUTPUT);\n}\n\nvoid loop() {\n  digitalWrite(LED_BUILTIN, HIGH);\n  delay(500);\n  digitalWrite(LED_BUILTIN, LOW);\n  delay(500);\n}\n";

/// The microcontroller boards on the sheet: the parts whose model is a board the sketch runs on.
pub fn boards(sch: &Schematic) -> Vec<String> {
    sch.components.iter().filter(|c| c.custom.as_ref().and_then(|s| s.sim.as_ref()).is_some_and(|m| m.name == "ARDUINO_NANO")).map(|c| c.refdes.clone()).collect()
}

/// The text of a board's sketch (the blink one until the person writes their own).
pub fn sketch_of(sch: &SchEditor, refdes: &str) -> String {
    sch.sketches.get(refdes).cloned().unwrap_or_else(|| BLINK.to_string())
}

/// The nets of a board's pins D0..D13, A0..A7 on the breadboard (by the pins' names in its symbol).
pub fn pin_nets(sch: &Schematic, refdes: &str, nets: &BoardNets) -> Vec<Option<String>> {
    let sym = sch.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref());
    (0..sim_core::sketch::PINS as u8)
        .map(|p| {
            let name = sim_core::sketch::pin_name(p);
            let number = sym?.pins.iter().find(|q| q.name == name)?.number.clone();
            nets.pins.get(&(refdes.to_string(), number)).cloned()
        })
        .collect()
}

/// A number for the sketches as they are now: a change of one starts the run again.
pub fn sketches_key(sch: &SchEditor) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sch.sketches.hash(&mut h);
    h.finish()
}

impl BreadboardView {
    /// The sketch boxes, one per board: the text, whether it compiles (the line and why when not), and while it runs the serial
    /// monitor and a run-time mistake.
    pub(super) fn sketch_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        for refdes in boards(&sch.sch) {
            ui.separator();
            ui.label(egui::RichText::new(i18n::trf!("スケッチ ({refdes})")).strong()).on_hover_text(i18n::tr(
                "このマイコンで動かすプログラム (Arduino の C の小さな部分)。setup() は電源を入れたとき 1 回、loop() はそのあとずっと繰り返します。使えるもの: int・long・float・bool の変数と配列、if・for・while・関数、pinMode・digitalWrite・digitalRead・analogRead・analogWrite・delay・millis・Serial.print / println・map・constrain・random。電源を入れると、ブレッドボードの回路と一緒に動きます",
            ));
            let mut text = sketch_of(sch, &refdes);
            // as wide as the panel is, not wider (a box asking for all the width made the panel grow)
            let edit = ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(10).desired_width(ui.available_width()));
            if edit.changed() {
                sch.sketches.insert(refdes.clone(), text.clone());
            }
            match sim_core::sketch::compile(&text) {
                Ok(_) => {
                    ui.colored_label(crate::theme::ok(), i18n::tr("✔ 読めました。電源を入れると動きます"));
                }
                Err(e) => {
                    ui.colored_label(crate::theme::error(), i18n::trf!("{} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
                }
            }
            if sch.sketches.contains_key(&refdes) && ui.small_button(i18n::tr("点滅の見本に戻す")).on_hover_text(i18n::tr("スケッチを、LED (D13) を点滅させる見本に戻します")).clicked() {
                sch.sketches.remove(&refdes);
            }
            let Some(m) = self.live.as_ref().and_then(|lv| lv.mcus.iter().find(|m| m.refdes == refdes)) else { continue };
            if let Some(e) = m.machine.as_ref().and_then(|x| x.error.as_ref()) {
                ui.colored_label(crate::theme::error(), i18n::trf!("止まりました: {} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
            }
            if let Some(x) = &m.machine {
                ui.label(egui::RichText::new(i18n::tr("シリアルモニタ (Serial.print の出力)")).small().color(NOTE));
                let tail: Vec<&str> = x.serial.lines().rev().take(12).collect();
                let shown: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                egui::Frame::NONE.fill(ui.visuals().extreme_bg_color).inner_margin(4.0).show(ui, |ui| {
                    ui.label(egui::RichText::new(if shown.is_empty() { "—".to_string() } else { shown }).monospace().small());
                });
            }
        }
    }
}
