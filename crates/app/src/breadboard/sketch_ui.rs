//! The sketch of each microcontroller board on the breadboard (B6 ⑤): written in the proto tab's left panel, saved with the sheet
//! (`SchEditor::sketches`), compiled as it is typed (`sim_core::sketch`), and run beside the circuit while the power is on
//! (`live.rs`): the serial monitor shows what it prints. A board may instead have its sketch built by arduino-cli and run as the
//! chip's real instructions (`avr_sim`, W5 (b)): libraries, `tone`, interrupts and the rest of Arduino's C work there.

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

/// A number for the sketches as they are now (and which boards run the real chip): a change of one starts the run again.
pub fn sketches_key(sch: &SchEditor) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sch.sketches.hash(&mut h);
    sch.sketch_real.hash(&mut h);
    h.finish()
}

/// A sketch being built by arduino-cli for the real chip on another thread: the chip's program when it is done (or why not).
pub type RealBuild = std::sync::Arc<std::sync::Mutex<Option<Result<avr_sim::Image, String>>>>;

/// The board arduino-cli builds for: the Nano (ATmega328P at 16 MHz).
pub const NANO_FQBN: &str = "arduino:avr:nano";

/// A number for a sketch's text (the builds are kept by it).
pub fn text_key(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl BreadboardView {
    /// The build of a sketch for the real chip: started on another thread the first time it is asked for (None while it goes on).
    pub(super) fn real_build(&mut self, text: &str) -> Option<Result<avr_sim::Image, String>> {
        let slot = self.real_builds.entry(text_key(text)).or_insert_with(|| {
            let slot = RealBuild::default();
            let (to, text) = (slot.clone(), text.to_string());
            std::thread::spawn(move || {
                let r = sch_core::firmware::compile_sketch(&text, NANO_FQBN).and_then(|elf| avr_sim::read_elf(&elf));
                *to.lock().unwrap() = Some(r);
            });
            slot
        });
        slot.lock().unwrap().clone()
    }

    /// A sketch is being built for the real chip, or a board waits for its program (a screenshot waits for them).
    pub fn real_busy(&self) -> bool {
        self.real_builds.values().any(|b| b.lock().unwrap().is_none()) || self.live.as_ref().is_some_and(|lv| lv.mcus.iter().any(|m| m.waiting))
    }

    /// The build of a sketch if one was started (Some(None) while it goes on).
    fn real_build_seen(&self, text: &str) -> Option<Option<Result<avr_sim::Image, String>>> {
        self.real_builds.get(&text_key(text)).map(|b| b.lock().unwrap().clone())
    }

    /// The boards waiting for their real chip's program get it when its build is done (each frame while the power is on).
    pub(super) fn give_real_programs(&mut self, ctx: &egui::Context, sch: &SchEditor) {
        let waiting: Vec<String> = self.live.as_ref().map(|lv| lv.mcus.iter().filter(|m| m.waiting).map(|m| m.refdes.clone()).collect()).unwrap_or_default();
        for refdes in waiting {
            let text = sketch_of(sch, &refdes);
            match self.real_build(&text) {
                None => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
                Some(Ok(img)) => {
                    if let Some(lv) = self.live.as_mut()
                        && let Err(e) = lv.give_program(&refdes, &img)
                    {
                        self.real_builds.insert(text_key(&text), std::sync::Arc::new(std::sync::Mutex::new(Some(Err(e)))));
                    }
                }
                // the build failed: the board stays still (the sketch box says why)
                Some(Err(_)) => {
                    if let Some(m) = self.live.as_mut().and_then(|lv| lv.mcus.iter_mut().find(|m| m.refdes == refdes)) {
                        m.waiting = false;
                    }
                }
            }
        }
    }

    /// The sketch boxes, one per board: the text, whether it compiles (the line and why when not), and while it runs the serial
    /// monitor and a run-time mistake.
    pub(super) fn sketch_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        for refdes in boards(&sch.sch) {
            crate::widgets::group_gap(ui);
            ui.label(egui::RichText::new(i18n::trf!("スケッチ ({refdes})")).strong()).on_hover_text(i18n::tr(
                "このマイコンで動かすプログラム (Arduino の C の小さな部分)。setup() は電源を入れたとき 1 回、loop() はそのあとずっと繰り返します。使えるもの: int・long・float・bool の変数と配列、if・for・while・関数、pinMode・digitalWrite・digitalRead・analogRead・analogWrite・delay・millis・Serial.print / println・map・constrain・random。電源を入れると、ブレッドボードの回路と一緒に動きます",
            ));
            let mut text = sketch_of(sch, &refdes);
            // as wide as the panel is, not wider (a box asking for all the width made the panel grow)
            let edit = ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(10).desired_width(ui.available_width()));
            if edit.changed() {
                sch.sketches.insert(refdes.clone(), text.clone());
            }
            let mut real = sch.sketch_real.contains(&refdes);
            if ui
                .checkbox(&mut real, i18n::tr("本物のマイコンの命令で動かす (arduino-cli)"))
                .on_hover_text(i18n::tr("スケッチを arduino-cli でコンパイルし、Nano のマイコン (ATmega328P) の本物の命令を 1 クロックずつ動かします (simavr)。ライブラリ・tone・割り込み・EEPROM など Arduino の C がそのまま動き、命令にかかる時間も本物どおりです。arduino-cli (Arduino IDE 2 に入っています) と、ボードの種類 arduino:avr が要ります。初めてのコンパイルは数十秒かかります。切ると、circuitCad の読み方 (Arduino の C の小さな部分) で動かします"))
                .changed()
            {
                if real {
                    sch.sketch_real.insert(refdes.clone());
                } else {
                    sch.sketch_real.remove(&refdes);
                }
            }
            // whether it compiles, and the way back to the sample on the same row
            ui.horizontal_wrapped(|ui| {
                if real {
                    match self.real_build_seen(&text) {
                        _ if sch_core::firmware::find_tool("arduino-cli").is_none() => {
                            ui.colored_label(crate::theme::error(), i18n::trf!("arduino-cli が見つかりません。{}", i18n::tr(sch_core::firmware::install_hint("arduino-cli"))));
                        }
                        None => {
                            ui.label(egui::RichText::new(i18n::tr("電源を入れるか「コンパイル」を押すと、arduino-cli がコンパイルします")).small().color(NOTE));
                            if ui.small_button(i18n::tr("コンパイル")).on_hover_text(i18n::tr("いまのスケッチを arduino-cli でコンパイルします (電源を入れる前に、誤りが無いか確かめられます)")).clicked() {
                                self.real_build(&text);
                            }
                        }
                        Some(None) => {
                            ui.spinner();
                            ui.label(i18n::tr("コンパイルしています… (初めては数十秒かかります)"));
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                        }
                        Some(Some(Ok(img))) => {
                            ui.colored_label(crate::theme::ok(), i18n::trf!("✔ コンパイルできました (プログラム {} バイト)。電源を入れると動きます", img.flash.len()));
                        }
                        Some(Some(Err(e))) => {
                            ui.colored_label(crate::theme::error(), i18n::tr_dyn(&e));
                        }
                    }
                } else {
                    match sim_core::sketch::compile(&text) {
                        Ok(_) => {
                            ui.colored_label(crate::theme::ok(), i18n::tr("✔ 読めました。電源を入れると動きます"));
                        }
                        Err(e) => {
                            ui.colored_label(crate::theme::error(), i18n::trf!("{} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
                        }
                    }
                }
                if sch.sketches.contains_key(&refdes) && ui.small_button(i18n::tr("点滅の見本に戻す")).on_hover_text(i18n::tr("スケッチを、LED (D13) を点滅させる見本に戻します")).clicked() {
                    sch.sketches.remove(&refdes);
                }
            });
            let Some(m) = self.live.as_ref().and_then(|lv| lv.mcus.iter().find(|m| m.refdes == refdes)) else { continue };
            if let Some(e) = m.machine.as_ref().and_then(|x| x.error.as_ref()) {
                ui.colored_label(crate::theme::error(), i18n::trf!("止まりました: {} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
            }
            if m.waiting {
                ui.label(egui::RichText::new(i18n::tr("コンパイルが終わると動き出します (それまでピンは入力のまま)")).small().color(NOTE));
            }
            if let Some(why) = m.real.as_ref().and_then(|n| n.avr.stopped()) {
                ui.colored_label(crate::theme::error(), i18n::trf!("止まりました: {}", i18n::tr_dyn(why)));
            }
            let serial = m.machine.as_ref().map(|x| &x.serial).or(m.real.as_ref().map(|n| &n.serial));
            if let Some(serial) = serial {
                ui.label(egui::RichText::new(i18n::tr("シリアルモニタ (Serial.print の出力)")).small().color(NOTE));
                let tail: Vec<&str> = serial.lines().rev().take(12).collect();
                let shown: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                egui::Frame::NONE.fill(ui.visuals().extreme_bg_color).inner_margin(4.0).show(ui, |ui| {
                    ui.label(egui::RichText::new(if shown.is_empty() { "—".to_string() } else { shown }).monospace().small());
                });
            }
            // the real chip reads what is typed here (Serial.read); the built-in reader has no Serial.read
            let real_running = self.live.as_ref().and_then(|lv| lv.mcus.iter().find(|m| m.refdes == refdes)).is_some_and(|m| m.real.is_some());
            if real_running {
                let mut sent = None;
                ui.horizontal(|ui| {
                    let text = self.serial_input.entry(refdes.clone()).or_default();
                    let w = (ui.available_width() - 50.0).max(60.0);
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(w).hint_text(i18n::tr("送る文字")).font(egui::TextStyle::Monospace));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let send = ui.small_button(i18n::tr("送る")).on_hover_text(i18n::tr("打った文字に改行を付けて、マイコンのシリアル (Serial.read で読めます) に送ります。Enter でも送れます"));
                    if (send.clicked() || enter) && !text.is_empty() {
                        sent = Some(std::mem::take(text));
                        r.request_focus();
                    }
                });
                if let Some(t) = sent
                    && let Some(n) = self.live.as_mut().and_then(|lv| lv.mcus.iter_mut().find(|m| m.refdes == refdes)).and_then(|m| m.real.as_mut())
                {
                    n.send_text(&format!("{t}\n"));
                }
            }
        }
    }
}
