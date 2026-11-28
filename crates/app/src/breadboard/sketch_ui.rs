//! The sketch of each microcontroller board on the breadboard (B6 ⑤): written in the proto tab's left panel, saved with the sheet
//! (`SchEditor::sketches`), compiled as it is typed (`sim_core::sketch`), and run beside the circuit while the power is on
//! (`live.rs`): the serial monitor shows what it prints. A board may instead have its sketch built by arduino-cli and run as the
//! chip's real instructions (`avr_sim`, W5 (b)): libraries, `tone`, interrupts and the rest of Arduino's C work there.

use super::*;
use sch_core::Schematic;

/// What a new board's sketch starts as: the built-in LED (D13) blinking.
pub const BLINK: &str = "// LED_BUILTIN (D13) を 0.5 秒ごとに点けたり消したりします\nvoid setup() {\n  pinMode(LED_BUILTIN, OUTPUT);\n}\n\nvoid loop() {\n  digitalWrite(LED_BUILTIN, HIGH);\n  delay(500);\n  digitalWrite(LED_BUILTIN, LOW);\n  delay(500);\n}\n";

/// The microcontroller boards on the sheet: the parts whose model is a board the sketch runs on (Nano, Mega).
pub fn boards(sch: &Schematic) -> Vec<String> {
    sch.components.iter().filter(|c| model_board(c).is_some()).map(|c| c.refdes.clone()).collect()
}

pub(super) fn model_board(c: &sch_core::Component) -> Option<sim_core::sketch::Board> {
    c.custom.as_ref().and_then(|s| s.sim.as_ref()).and_then(|m| sim_core::sketch::Board::from_model(&m.name))
}

/// Which board a part is (the Nano when it is none).
pub fn board_of(sch: &Schematic, refdes: &str) -> sim_core::sketch::Board {
    sch.components.iter().find(|c| c.refdes == refdes).and_then(model_board).unwrap_or_default()
}

/// The text of a board's sketch (the blink one until the person writes their own).
pub fn sketch_of(sch: &SchEditor, refdes: &str) -> String {
    sch.sketches.get(refdes).cloned().unwrap_or_else(|| {
        // LED_BUILTIN is D13 on the Arduino boards, PB1 on the ATtiny85
        let led = board_of(&sch.sch, refdes).pin_name(board_of(&sch.sch, refdes).led());
        BLINK.replace("(D13)", &format!("({led})"))
    })
}

/// The nets of a board's pins (D0.., A0..) on the breadboard (by the pins' names in its symbol).
pub fn pin_nets(sch: &Schematic, refdes: &str, nets: &BoardNets) -> Vec<Option<String>> {
    let sym = sch.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref());
    let board = board_of(sch, refdes);
    (0..board.pins() as u8)
        .map(|p| {
            let name = board.pin_name(p);
            // "PB5/RESET": the pin by its first name
            let number = sym?.pins.iter().find(|q| q.name.split('/').next() == Some(name.as_str()))?.number.clone();
            nets.pins.get(&(refdes.to_string(), number)).cloned()
        })
        .collect()
}

/// The nets of a board's supply pins (VCC, GND) when it has no USB of its own (`Board::supply_pins`: the ATtiny85).
pub fn supply_nets(sch: &Schematic, refdes: &str, nets: &BoardNets) -> Option<(Option<String>, Option<String>)> {
    let (vcc, gnd) = board_of(sch, refdes).supply_pins()?;
    let sym = sch.components.iter().find(|c| c.refdes == refdes).and_then(|c| c.custom.as_ref())?;
    let net = |name: &str| {
        let number = sym.pins.iter().find(|q| q.name == name)?.number.clone();
        nets.pins.get(&(refdes.to_string(), number)).cloned()
    };
    Some((net(vcc), net(gnd)))
}

/// The speed of `Serial.begin(…)` in a sketch (9600 when it says none or not as a number).
pub fn serial_baud(text: &str) -> u32 {
    text.split("Serial.begin(").nth(1).and_then(|r| r.split(')').next()).and_then(|n| n.trim().parse().ok()).unwrap_or(9600)
}

/// A number for the sketches as they are now (and which boards run the real chip): a change of one starts the run again.
pub fn sketches_key(sch: &SchEditor) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sch.sketches.hash(&mut h);
    sch.sketch_real.hash(&mut h);
    h.finish()
}

/// A board's sketch as it was before its changes: one step per change made with the mouse up and no box being typed in (a drag of
/// a block, a number dragged, a name typed: each one step).
#[derive(Default)]
pub struct SketchHistory {
    committed: Option<String>,
    undo: Vec<String>,
    redo: Vec<String>,
}

/// The most steps kept for a sketch.
const SKETCH_UNDO_MAX: usize = 100;

/// A sketch being built by arduino-cli for the real chip on another thread: the chip's program when it is done (or why not).
pub type RealBuild = std::sync::Arc<std::sync::Mutex<Option<Result<avr_sim::Image, String>>>>;

/// A number for a sketch's text and its board (the builds are kept by it).
pub fn text_key(text: &str, board: sim_core::sketch::Board) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, board).hash(&mut h);
    h.finish()
}

impl BreadboardView {
    /// Takes the board's sketch as it is now into its history when a change has settled (the mouse up, no box typed in).
    fn track_sketch(&mut self, ctx: &egui::Context, refdes: &str, text: &str) {
        let h = self.sketch_history.entry(refdes.to_string()).or_default();
        let Some(before) = h.committed.as_ref() else {
            h.committed = Some(text.to_string());
            return;
        };
        let settled = !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input();
        if before != text && settled {
            h.undo.push(std::mem::replace(h.committed.as_mut().unwrap(), text.to_string()));
            if h.undo.len() > SKETCH_UNDO_MAX {
                h.undo.remove(0);
            }
            h.redo.clear();
        }
    }

    /// 元に戻す (`back`) or やり直す the last change of a board's sketch.
    pub(super) fn sketch_undo(&mut self, sch: &mut SchEditor, refdes: &str, back: bool) {
        let now = sketch_of(sch, refdes);
        let h = self.sketch_history.entry(refdes.to_string()).or_default();
        // a change not yet taken (made this frame) is a step of its own
        if h.committed.as_ref().is_some_and(|c| *c != now) {
            h.undo.push(h.committed.take().unwrap());
            h.redo.clear();
        }
        let (from, to) = if back { (&mut h.undo, &mut h.redo) } else { (&mut h.redo, &mut h.undo) };
        let Some(text) = from.pop() else { return };
        to.push(now);
        h.committed = Some(text.clone());
        sch.sketches.insert(refdes.to_string(), text);
        sch.status = i18n::tr(if back { "スケッチを元に戻しました" } else { "スケッチをやり直しました" }).to_string();
    }

    /// The 元に戻す and やり直す buttons of a board's sketch: Some(true) to undo, Some(false) to redo.
    fn sketch_undo_buttons(&self, ui: &mut egui::Ui, sch: &SchEditor, refdes: &str) -> Option<bool> {
        let now = sketch_of(sch, refdes);
        let (can_back, can_redo) = self.sketch_history.get(refdes).map_or((false, false), |h| (!h.undo.is_empty() || h.committed.as_ref().is_some_and(|c| *c != now), !h.redo.is_empty()));
        let mut act = None;
        ui.horizontal(|ui| {
            if crate::icons::icon_button_key(ui, crate::icons::Icon::Undo, "元に戻す", "Ctrl+Z", false, can_back, "スケッチの直前の変更 (ブロックを置いた・動かした・消した・値を変えた) を取り消します (Ctrl+Z)").clicked() {
                act = Some(true);
            }
            if crate::icons::icon_button_key(ui, crate::icons::Icon::Redo, "やり直す", "Ctrl+Y", false, can_redo, "取り消した変更をもう一度します (Ctrl+Y)").clicked() {
                act = Some(false);
            }
        });
        act
    }

    /// The keys of 元に戻す (Ctrl+Z) and やり直す (Ctrl+Y, Ctrl+Shift+Z) while the pointer is over the blocks (`area`) and nothing is
    /// being typed (a box being typed in has its own). Taken here: the stripboard's own Ctrl+Z does not undo its layout too.
    fn sketch_undo_keys(ui: &mut egui::Ui, area: egui::Rect) -> Option<bool> {
        if !ui.rect_contains_pointer(area) || ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        let shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        ui.input_mut(|i| {
            if i.consume_key(shift, egui::Key::Z) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) {
                Some(false)
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                Some(true)
            } else {
                None
            }
        })
    }

    /// The line the board's sketch runs at now, while the power is on and circuitCad reads it (the real chip's is not known).
    fn running_line(&self, refdes: &str) -> Option<u32> {
        let m = self.live.as_ref()?.mcus.iter().find(|m| m.refdes == refdes)?;
        m.machine.as_ref().filter(|x| x.error.is_none())?.line()
    }

    /// The build of a sketch for the real chip: started on another thread the first time it is asked for (None while it goes on).
    pub(super) fn real_build(&mut self, text: &str, board: sim_core::sketch::Board) -> Option<Result<avr_sim::Image, String>> {
        let slot = self.real_builds.entry(text_key(text, board)).or_insert_with(|| {
            let slot = RealBuild::default();
            let (to, text) = (slot.clone(), text.to_string());
            std::thread::spawn(move || {
                let r = sch_core::firmware::compile_sketch(&text, board.fqbn()).and_then(|elf| avr_sim::read_elf(&elf));
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
    fn real_build_seen(&self, text: &str, board: sim_core::sketch::Board) -> Option<Option<Result<avr_sim::Image, String>>> {
        self.real_builds.get(&text_key(text, board)).map(|b| b.lock().unwrap().clone())
    }

    /// The boards waiting for their real chip's program get it when its build is done (each frame while the power is on).
    pub(super) fn give_real_programs(&mut self, ctx: &egui::Context, sch: &SchEditor) {
        let waiting: Vec<String> = self.live.as_ref().map(|lv| lv.mcus.iter().filter(|m| m.waiting).map(|m| m.refdes.clone()).collect()).unwrap_or_default();
        for refdes in waiting {
            let text = sketch_of(sch, &refdes);
            let board = board_of(&sch.sch, &refdes);
            match self.real_build(&text, board) {
                None => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
                Some(Ok(img)) => {
                    if let Some(lv) = self.live.as_mut() {
                        if let Err(e) = lv.give_program(&refdes, &img) {
                            self.real_builds.insert(text_key(&text, board), std::sync::Arc::new(std::sync::Mutex::new(Some(Err(e)))));
                        } else if let Some(chip) = lv.mcus.iter_mut().find(|m| m.refdes == refdes).and_then(|m| m.real.as_mut()) {
                            // a chip without a UART sends its Serial in software: read back at the speed the sketch begins it at
                            chip.set_soft_baud(serial_baud(&text));
                        }
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
            let board = board_of(&sch.sch, &refdes);
            crate::widgets::group_gap(ui);
            let mut text = sketch_of(sch, &refdes);
            // the title, and on its right what is done to the whole sketch (the way back to the sample in a menu: it wipes the text)
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(i18n::trf!("スケッチ ({refdes})")).strong()).on_hover_text(i18n::tr(
                    "このマイコンで動かすプログラム (Arduino の C の小さな部分)。setup() は電源を入れたとき 1 回、loop() はそのあとずっと繰り返します。使えるもの: int・long・float・bool の変数と配列、if・for・while・関数、pinMode・digitalWrite・digitalRead・analogRead・analogWrite・delay・millis・Serial.print / println・map・constrain・random。電源を入れると、ブレッドボードの回路と一緒に動きます",
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("…", |ui| {
                        let led = board.pin_name(board.led());
                        let why_not = (!sch.sketches.contains_key(&refdes)).then_some("いまのスケッチが見本のままです");
                        let back = crate::widgets::unless(ui, why_not, egui::Button::new(i18n::tr("点滅の見本に戻す")));
                        if back.on_hover_text(i18n::trf!("スケッチを消して、LED ({led}) を点滅させる見本に戻します")).clicked() {
                            sch.sketches.remove(&refdes);
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("ほかの操作 (見本に戻す)"));
                    if ui.small_button(i18n::tr("整形")).on_hover_text(i18n::tr("{ } の深さに合わせて行の頭の字下げをそろえます (2 文字ずつ)。言葉は変えません")).clicked() {
                        let f = super::code_edit::format(&text);
                        if f != text {
                            text = f.clone();
                            sch.sketches.insert(refdes.clone(), f);
                        }
                    }
                    if ui.small_button(i18n::tr("別窓で開く")).on_hover_text(i18n::tr("スケッチを大きな窓で書きます (行番号つき)。ブレッドボードを見ながら書けます")).clicked() {
                        self.sketch_window = Some(refdes.clone());
                    }
                });
            });
            let real = sch.sketch_real.contains(&refdes);
            // written as code or as blocks (the blocks are the code read back: a sketch they cannot show stays code)
            let blocks = mode_bar(ui, sch, &refdes, &text);
            if let Some((mut bs, lines)) = blocks {
                let running = self.running_line(&refdes);
                let mut act = self.sketch_undo_buttons(ui, sch, &refdes);
                let shown = egui::ScrollArea::vertical()
                    .id_salt(("blocks_box", &refdes))
                    .max_height(BLOCKS_PANEL_HEIGHT)
                    .show(ui, |ui| super::blocks_ui::edit_blocks(ui, &mut bs, board, &refdes, &lines, running));
                if shown.inner {
                    text = blocks_code(&bs);
                    sch.sketches.insert(refdes.clone(), text.clone());
                }
                act = act.or_else(|| Self::sketch_undo_keys(ui, shown.inner_rect));
                if let Some(back) = act {
                    self.sketch_undo(sch, &refdes, back);
                }
                text = sketch_of(sch, &refdes);
            } else {
                let marked = self.mistake_line(&text, board, real);
                // as wide as the panel is, not wider; as tall as SKETCH_ROWS lines at most (a long sketch scrolls inside: the serial
                // monitor below stays in sight while it runs; the window is for writing at length)
                let max_h = ui.text_style_height(&egui::TextStyle::Monospace) * SKETCH_ROWS as f32 + 8.0;
                let edit = egui::ScrollArea::vertical()
                    .id_salt(("sketch_box", &refdes))
                    .max_height(max_h)
                    .show(ui, |ui| super::code_edit::code_box(ui, &mut text, 10, ui.available_width(), marked))
                    .inner;
                if edit.changed() {
                    sch.sketches.insert(refdes.clone(), text.clone());
                }
            }
            // how it runs: circuitCad's own reading, or the chip's real instructions
            let real_tip = i18n::tr("スケッチを arduino-cli でコンパイルし、ボードのマイコン (Nano は ATmega328P、Mega は ATmega2560、ATtiny85) の本物の命令を 1 クロックずつ動かします (simavr)。ライブラリ・tone・割り込み・EEPROM など Arduino の C がそのまま動き、命令にかかる時間も本物どおりです。arduino-cli (Arduino IDE 2 に入っています) と、ボードの種類 (arduino:avr、ATtiny85 は ATTinyCore) が要ります。初めてのコンパイルは数十秒かかります。切ると、circuitCad の読み方 (Arduino の C の小さな部分) で動かします");
            let own_tip = i18n::tr("circuitCad がスケッチを読んで動かします (Arduino の C の小さな部分: 変数・if・for・関数・pinMode・digitalWrite・analogRead・delay・Serial.print など)。arduino-cli が無くても、すぐに動きます。ライブラリ・割り込み・命令にかかる時間は扱いません");
            for (k, r) in crate::widgets::segment_bar(ui, &[("circuitCad で読む", None), ("本物の命令で動かす", None)], Some(usize::from(real))).into_iter().enumerate() {
                if r.on_hover_text(if k == 0 { own_tip } else { real_tip }).clicked() && (k == 1) != real {
                    if k == 1 {
                        sch.sketch_real.insert(refdes.clone());
                    } else {
                        sch.sketch_real.remove(&refdes);
                    }
                }
            }
            let real = sch.sketch_real.contains(&refdes);
            ui.horizontal_wrapped(|ui| self.compile_status(ui, &text, board, real));
            self.serial_ui(ui, &refdes, &text, SERIAL_PANEL_HEIGHT);
            self.track_sketch(ui.ctx(), &refdes, &text);
        }
        self.sketch_window_ui(ui.ctx(), sch);
    }

    /// The line of the sketch's mistake (from 1), if it does not compile: circuitCad's reading says it, arduino-cli's message
    /// names it as "スケッチ:12:5: error".
    fn mistake_line(&self, text: &str, board: sim_core::sketch::Board, real: bool) -> Option<usize> {
        if real {
            let Some(Some(Err(e))) = self.real_build_seen(text, board) else { return None };
            e.lines().find_map(|l| l.split("スケッチ:").nth(1)?.split(':').next()?.parse().ok())
        } else {
            sim_core::sketch::compile(text, board).err().map(|e| e.line as usize)
        }
    }

    /// Whether the sketch compiles (circuitCad's reading, or arduino-cli for the real chip): the line and why when not.
    fn compile_status(&mut self, ui: &mut egui::Ui, text: &str, board: sim_core::sketch::Board, real: bool) {
        if real {
            match self.real_build_seen(text, board) {
                _ if sch_core::firmware::find_tool("arduino-cli").is_none() => {
                    ui.colored_label(crate::theme::error(), i18n::trf!("arduino-cli が見つかりません。{}", i18n::tr(sch_core::firmware::install_hint("arduino-cli"))));
                }
                None => {
                    ui.label(egui::RichText::new(i18n::tr("電源を入れるか「コンパイル」を押すと、arduino-cli がコンパイルします")).small().color(NOTE));
                    if ui.small_button(i18n::tr("コンパイル")).on_hover_text(i18n::tr("いまのスケッチを arduino-cli でコンパイルします (電源を入れる前に、誤りが無いか確かめられます)")).clicked() {
                        self.real_build(text, board);
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
            match sim_core::sketch::compile(text, board) {
                Ok(_) => {
                    ui.colored_label(crate::theme::ok(), i18n::tr("✔ 読めました。電源を入れると動きます"));
                }
                Err(e) => {
                    ui.colored_label(crate::theme::error(), i18n::trf!("{} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
                }
            }
        }
    }

    /// While it runs: a mistake met on the way, the serial monitor, and (on a real chip with a UART) the line to send it.
    /// `text`: the sketch (the speed its `Serial.begin` says goes in the monitor's title); `height`: how tall the monitor is.
    fn serial_ui(&mut self, ui: &mut egui::Ui, refdes: &str, text: &str, height: f32) {
        let refdes = refdes.to_string();
        let Some(m) = self.live.as_ref().and_then(|lv| lv.mcus.iter().find(|m| m.refdes == refdes)) else { return };
        if let Some(e) = m.machine.as_ref().and_then(|x| x.error.as_ref()) {
            ui.colored_label(crate::theme::error(), i18n::trf!("止まりました: {} 行目: {}", e.line, i18n::tr_dyn(&e.message)));
        }
        if m.waiting {
            ui.label(egui::RichText::new(i18n::tr("コンパイルが終わると動き出します (それまでピンは入力のまま)")).small().color(NOTE));
        }
        if let Some(why) = m.real.as_ref().and_then(|n| n.avr.stopped()) {
            ui.colored_label(crate::theme::error(), i18n::trf!("止まりました: {}", i18n::tr_dyn(why)));
        }
        let serial = m.machine.as_ref().map(|x| &x.serial).or(m.real.as_ref().map(|n| &n.serial)).cloned();
        if let Some(serial) = serial
            && terminal(ui, &refdes, &serial, serial_baud(text), height)
            && let Some(m) = self.live.as_mut().and_then(|lv| lv.mcus.iter_mut().find(|m| m.refdes == refdes))
        {
            // クリア: what the program has sent so far is gone (it goes on sending)
            if let Some(x) = m.machine.as_mut() {
                x.serial.clear();
            }
            if let Some(n) = m.real.as_mut() {
                n.serial.clear();
            }
        }
        // the real chip reads what is typed here (Serial.read); the built-in reader has no Serial.read
        // (a chip with no UART, the ATtiny85, only sends: its Serial is software on one pin)
        let real_running = self.live.as_ref().and_then(|lv| lv.mcus.iter().find(|m| m.refdes == refdes)).is_some_and(|m| m.real.as_ref().is_some_and(|c| c.def.soft_tx.is_none()));
        if real_running {
            let mut sent = None;
            // the button from the right first, the box the width left (a box of a guessed width pushed the panel wider each frame)
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let send = ui.small_button(i18n::tr("送る")).on_hover_text(i18n::tr("打った文字に改行を付けて、マイコンのシリアル (Serial.read で読めます) に送ります。Enter でも送れます"));
                let text = self.serial_input.entry(refdes.clone()).or_default();
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()).hint_text(i18n::tr("送る文字")).font(egui::TextStyle::Monospace));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
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

    /// The sketch in a window of its own: line numbers, the mistake's line marked, 整形, whether it compiles, and while it runs
    /// the serial monitor. The same text as the box in the panel (a change in one is in the other).
    fn sketch_window_ui(&mut self, ctx: &egui::Context, sch: &mut SchEditor) {
        let Some(refdes) = self.sketch_window.clone() else { return };
        if !boards(&sch.sch).contains(&refdes) {
            self.sketch_window = None;
            return;
        }
        let board = board_of(&sch.sch, &refdes);
        let real = sch.sketch_real.contains(&refdes);
        let mut open = true;
        // as big as the area right of the panel (above the status lines): blocks and code side by side want the room
        let at = crate::widgets::beside_panel();
        let area = ctx.content_rect();
        let size = [(area.right() - at.x - 16.0).max(680.0), (area.bottom() - at.y - 70.0).max(560.0)];
        egui::Window::new(i18n::trf!("スケッチ ({refdes})"))
            .id(egui::Id::new("sketch_window"))
            .open(&mut open)
            .default_pos(crate::widgets::beside_panel())
            .default_size(size)
            .resizable(true)
            .show(ctx, |ui| {
                let mut text = sketch_of(sch, &refdes);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(i18n::tr("整形")).on_hover_text(i18n::tr("{ } の深さに合わせて行の頭の字下げをそろえます (2 文字ずつ)。言葉は変えません")).clicked() {
                        let f = super::code_edit::format(&text);
                        if f != text {
                            text = f.clone();
                            sch.sketches.insert(refdes.clone(), f);
                        }
                    }
                    self.compile_status(ui, &text, board, real);
                });
                let marked = self.mistake_line(&text, board, real);
                // the serial monitor keeps the bottom; the text the rest
                let running = self.live.as_ref().is_some_and(|lv| lv.mcus.iter().any(|m| m.refdes == refdes));
                let below = if running { 230.0 } else { 0.0 };
                let blocks = mode_bar(ui, sch, &refdes, &text);
                let blocks = if blocks.is_some() {
                    let area = ui.max_rect();
                    if let Some(back) = self.sketch_undo_buttons(ui, sch, &refdes).or_else(|| Self::sketch_undo_keys(ui, area)) {
                        self.sketch_undo(sch, &refdes, back);
                    }
                    text = sketch_of(sch, &refdes);
                    sim_core::sketch::blocks::from_code_with_lines(&text).ok()
                } else {
                    None
                };
                let now_at = self.running_line(&refdes);
                let size = egui::vec2(ui.available_width(), (ui.available_height() - below).max(120.0));
                if let Some((mut bs, lines)) = blocks {
                    // the blocks on the left, the code they make on the right (a change of either is in the other)
                    let w = (size.x - 8.0) / 2.0;
                    ui.allocate_ui(size, |ui| {
                        ui.horizontal_top(|ui| {
                            let changed = ui
                                .allocate_ui_with_layout(egui::vec2(w, size.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                                    egui::ScrollArea::vertical().id_salt(("blocks_window", &refdes)).max_height(size.y).show(ui, |ui| super::blocks_ui::edit_blocks(ui, &mut bs, board, &format!("{refdes}_window"), &lines, now_at)).inner
                                })
                                .inner;
                            if changed {
                                text = blocks_code(&bs);
                                sch.sketches.insert(refdes.clone(), text.clone());
                            }
                            let edit = ui.allocate_ui_with_layout(egui::vec2(w, size.y), egui::Layout::top_down(egui::Align::Min), |ui| super::code_edit::numbered_box(ui, &mut text, marked)).inner;
                            if edit.changed() {
                                sch.sketches.insert(refdes.clone(), text.clone());
                            }
                        });
                    });
                } else {
                    let edit = ui.allocate_ui(size, |ui| super::code_edit::numbered_box(ui, &mut text, marked)).inner;
                    if edit.changed() {
                        sch.sketches.insert(refdes.clone(), text.clone());
                    }
                }
                if running {
                    self.serial_ui(ui, &refdes, &text, (ui.available_height() - 40.0).max(80.0));
                }
            });
        if !open {
            self.sketch_window = None;
        }
    }
}

/// The choice of writing a board's sketch as code or as blocks (saved in `sketch_blocks`): the blocks to show when they are
/// chosen and the sketch reads as blocks. A sketch they cannot show (a function of its own, an array…) says why, and stays code.
fn mode_bar(ui: &mut egui::Ui, sch: &mut SchEditor, refdes: &str, text: &str) -> Option<(sim_core::sketch::blocks::BlockSketch, Vec<u32>)> {
    let on = sch.sketch_blocks.contains(refdes);
    let read = sim_core::sketch::blocks::from_code_with_lines(text);
    let why = read.as_ref().err().map(|e| i18n::trf!("いまのスケッチはブロックで表せません ({e})。コードで直すか、見本に戻すとブロックにできます"));
    let items = [("コード", None), ("ブロック", if on { None } else { why.as_deref() })];
    let tips = [
        i18n::tr("スケッチを文字 (Arduino の C) で書きます"),
        i18n::tr("命令のブロックを積み重ねて書きます (Scratch のように)。打ち間違いが起きません。ブロックからコードが作られ、「コード」に切り替えると、そのまま続きを文字で書けます"),
    ];
    for (k, r) in crate::widgets::segment_bar(ui, &items, Some(usize::from(on))).into_iter().enumerate() {
        let r = match (k, &why) {
            (1, Some(w)) if !on => r.on_hover_text(w.as_str()),
            _ => r.on_hover_text(tips[k]),
        };
        if r.clicked() && (k == 1) != on {
            if k == 1 {
                sch.sketch_blocks.insert(refdes.to_string());
            } else {
                sch.sketch_blocks.remove(refdes);
            }
        }
    }
    if !sch.sketch_blocks.contains(refdes) {
        return None;
    }
    match read {
        Ok((bs, lines)) => {
            // comments are not blocks: they go when the blocks are first changed
            let comments = text.lines().any(|l| (l.contains("//") || l.contains("/*")) && !is_header(l.trim()));
            if comments {
                ui.label(egui::RichText::new(i18n::tr("ブロックを変えると、コードのコメント (// の行) は消えます")).small().color(NOTE));
            }
            Some((bs, lines))
        }
        Err(e) => {
            ui.colored_label(crate::theme::warn(), i18n::trf!("ブロックで表せないので、コードで出しています ({e})"));
            None
        }
    }
}

/// How tall the blocks are in the panel before they scroll inside.
const BLOCKS_PANEL_HEIGHT: f32 = 420.0;

/// The most lines the sketch box shows in the panel before it scrolls inside.
const SKETCH_ROWS: usize = 14;

/// How tall the serial monitor is in the panel (the sketch's window gives it the rest of its height).
const SERIAL_PANEL_HEIGHT: f32 = 150.0;

/// The most lines the serial monitor shows (the board keeps up to 20 000 characters).
const TERMINAL_LINES: usize = 500;

/// The serial monitor drawn as a terminal: light text on black whatever the theme, its title with the speed, the lines kept
/// (scrolled back with the wheel; it follows the newest), a blinking block where the next character goes. True when クリア is
/// pressed.
fn terminal(ui: &mut egui::Ui, refdes: &str, serial: &str, baud: u32, height: f32) -> bool {
    const BG: Color32 = Color32::from_rgb(12, 12, 12);
    const FG: Color32 = Color32::from_rgb(204, 230, 204);
    const DIM: Color32 = Color32::from_rgb(120, 130, 120);
    let mut clear = false;
    egui::Frame::NONE.fill(BG).stroke(egui::Stroke::new(1.0, Color32::from_gray(70))).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("●").color(Color32::from_rgb(90, 200, 110)).small());
            ui.label(egui::RichText::new(i18n::trf!("シリアルモニタ  {baud} bps")).monospace().small().color(DIM))
                .on_hover_text(i18n::tr("スケッチの Serial.print / println が送った文字です (速さはスケッチの Serial.begin のもの)。ホイールで前の行に戻れます。新しい行が来ると下に付いていきます"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clear = ui
                    .add(egui::Button::new(egui::RichText::new(i18n::tr("クリア")).small().color(DIM)).fill(Color32::from_gray(30)))
                    .on_hover_text(i18n::tr("ここまでの出力を消します (マイコンは送り続けます)"))
                    .clicked();
            });
        });
        ui.add_space(2.0);
        // the last lines (an egui label of many thousand lines would be slow)
        let lines: Vec<&str> = serial.lines().collect();
        let mut shown = lines[lines.len().saturating_sub(TERMINAL_LINES)..].join("\n");
        if serial.ends_with('\n') && !shown.is_empty() {
            shown.push('\n');
        }
        // the cursor, on for half a second, off for half
        let on = (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
        shown.push(if on { '█' } else { ' ' });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        egui::ScrollArea::vertical().id_salt(("serial", refdes)).max_height(height).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(shown).monospace().color(FG)).wrap());
        });
    });
    clear
}

/// The blocks as code, its first line (the "made from blocks" comment) in the UI language.
fn blocks_code(bs: &sim_core::sketch::blocks::BlockSketch) -> String {
    let code = sim_core::sketch::blocks::to_code(bs);
    let header = sim_core::sketch::blocks::HEADER;
    match code.strip_prefix(header) {
        Some(rest) if !i18n::is_ja() => format!("{}{rest}", i18n::tr(header)),
        _ => code,
    }
}

/// The "made from blocks" comment, in any language (a sketch written in English blocks read in Japanese keeps its comment).
fn is_header(line: &str) -> bool {
    line == sim_core::sketch::blocks::HEADER || line == "// Sketch made from blocks"
}
