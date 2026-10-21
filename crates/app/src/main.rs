// No console window on Windows (panics are written to %APPDATA%\circuitCad\crash.log instead).
#![windows_subsystem = "windows"]
// Structs with many fields (calculation results, test fixtures) are filled by assigning after `Default::default()`; that reads better than one long initializer.
#![allow(clippy::field_reassign_with_default)]

use board_editor::BoardEditor;
use eframe::egui;
use schematic::SchEditor;

mod args;
mod board_editor;
mod files;
mod help;
mod icons;
mod bench_guide;
mod measure_guide;
mod samples;
mod learn;
mod part_form;
mod plot;
mod prefs;
mod project;
mod schematic;
mod screenshot;
mod source_form;
mod startup;
mod tips;
mod xcheck;
mod xcheck_ac;
mod xcheck_tran;
#[cfg(test)]
mod tests;

use args::apply_args;
use startup::{install_crash_log, install_japanese_font};
use tips::TIPS;


#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Schematic,
    Board,
}

struct App {
    show_hints: bool,
    mode: Mode,
    sch: SchEditor,
    board_ed: BoardEditor,
    /// Where the screenshot being taken goes (the capture arrives a frame after the request).
    shot_path: Option<std::path::PathBuf>,
    help: help::Help,
    /// Finished lessons (saved in the app data folder) and the learning-path window.
    progress: learn::Progress,
    learn: learn::LearnWindow,
    /// The guide window of the bench step ("組み立てて、性能を測る").
    measure: measure_guide::MeasureGuide,
    bench: bench_guide::BenchGuide,
    /// `--screenshot <file>`: (file, frames left before the capture); the app closes after saving.
    auto_shot: Option<(std::path::PathBuf, u32)>,
    /// The `.ccproj` file the schematic and board were opened from / last saved to.
    project: Option<std::path::PathBuf>,
    /// Set once the user agreed to close (or a script closes the window), so the unsaved-changes question is not asked again.
    closing_ok: bool,
    /// The first-launch welcome card is up (only when started with no arguments and no lesson finished yet).
    welcome: bool,
}

impl App {
    /// Carry the board's copper resistance to the schematic's simulations, or take it away.
    fn apply_post_layout(&mut self, on: bool) {
        if !on {
            self.sch.wiring.clear();
            self.sch.couplings.clear();
            self.board_ed.post_layout_fresh = false;
            self.board_ed.status = "配線の抵抗・インダクタンス・容量を外しました。回路図のシミュレーションは理想の配線に戻ります".into();
            return;
        }
        let spec = pcb_core::irdrop::IrSpec::default();
        let series = pcb_core::irdrop::wiring_resistances(&self.board_ed.board, &spec);
        self.sch.couplings = pcb_core::irdrop::wiring_couplings(&series, &spec).into_iter().map(|c| sim_core::WiringK { a: c.a, b: c.b, k: c.k }).collect();
        self.sch.wiring = series.into_iter().map(|w| sim_core::WiringR { refdes: w.refdes, pin: w.pin, ohm: w.ohm, henry: w.henry, farad: w.farad }).collect();
        self.board_ed.post_layout_fresh = !self.sch.wiring.is_empty();
        self.board_ed.status = if self.sch.wiring.is_empty() {
            "配線されたネットがないので、反映するものがありません。先に配線してください".into()
        } else {
            format!("{} ピンの配線の抵抗を回路図のシミュレーションに反映しました。回路図に戻って解析を実行してください", self.sch.wiring.len())
        };
    }
}

impl Default for App {
    fn default() -> Self {
        Self { welcome: false, show_hints: true, mode: Mode::Schematic, sch: SchEditor::default(), board_ed: BoardEditor::default(), shot_path: None, help: help::Help::default(), progress: learn::Progress::default(), learn: learn::LearnWindow::default(), measure: measure_guide::MeasureGuide::default(), bench: bench_guide::BenchGuide::default(), auto_shot: None, project: None, closing_ok: false }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // closing the window with unsaved changes: ask first
        if ui.ctx().input(|i| i.viewport().close_requested()) && !self.closing_ok && self.dirty() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.confirm_unsaved("このまま閉じると、変更は失われます。") {
                self.closing_ok = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.finish_screenshot(ui);
        self.tick_auto_screenshot(ui);
        // F1, and the small "操作" buttons of the panels, open the guide
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.show(if self.mode == Mode::Schematic { help::HelpTab::Schematic } else { help::HelpTab::Board });
        }
        match self.board_ed.currents_request.take() {
            Some(board_editor::CurrentSource::Dc) => {
                let currents = self.sch.dc_net_currents();
                self.board_ed.use_simulated_currents(currents, vec![]);
            }
            Some(board_editor::CurrentSource::Tran) => match self.sch.tran_net_currents() {
                Ok((currents, stats)) => self.board_ed.use_simulated_currents(Ok(currents), stats),
                Err(e) => self.board_ed.use_simulated_currents(Err(e), vec![]),
            },
            None => {}
        }
        // "全部" was pressed with no currents declared: the currents are in now (or could not be had), so route
        if let Some(net) = self.board_ed.route_after_currents.take() {
            self.board_ed.autoroute_now(net.as_deref());
        }
        if let Some(on) = self.board_ed.post_layout_request.take() {
            self.apply_post_layout(on);
        }
        if let Some(t) = self.sch.help_request.take().or_else(|| self.board_ed.help_request.take()) {
            self.help.show(t);
        }
        self.help.window(ui.ctx());
        // finished lessons go into the saved record; the learning-path window can start one
        if let Some(id) = self.sch.finished_lesson() {
            self.progress.mark(id);
        }
        if let Some(id) = self.board_ed.finished_board_lesson() {
            self.progress.mark(id);
        }
        if let Some(key) = self.sch.finished_challenge() {
            self.progress.mark(&key);
        }
        let mut action = self.learn.show(ui.ctx(), &mut self.progress);
        let welcome_choice = if self.welcome { learn::welcome_card(ui.ctx(), &self.progress) } else { None };
        if let Some(choice) = welcome_choice {
            self.welcome = false;
            self.progress.mark_welcomed();
            match choice {
                learn::Welcome::Start(a) => action = Some(a),
                learn::Welcome::Path => self.learn.open = true,
                learn::Welcome::Guide => self.help.show(help::HelpTab::Schematic),
                learn::Welcome::Close => {}
            }
        }
        if let Some(action) = action {
            match action {
                learn::Action::Lesson(id) => {
                    if let Some(i) = schematic::lesson_ids().iter().position(|l| *l == id) {
                        self.mode = Mode::Schematic;
                        self.sch.start_lesson(i);
                    }
                }
                learn::Action::Build(id) => {
                    self.mode = Mode::Schematic;
                    self.sch.start_challenge(id, 0);
                }
                learn::Action::Board(n) => self.start_board_lesson(n),
                learn::Action::Measure => self.measure.open = true,
                learn::Action::Hands(id) => self.bench.show_guide(id),
            }
        }
        // the hands-on checklists: every box ticked records the item
        if let Some(id) = self.bench.show(ui.ctx(), &self.progress) {
            self.progress.mark(id);
        }
        // the bench step: the guide window, and "three readings compared with the simulation" finishes it
        let compared = self.sch.compared_measurements();
        if compared >= 3 {
            self.progress.mark("build_psu");
        }
        if let Some(learn_action) = self.measure.show(ui.ctx(), self.progress.is_done("build_psu"), compared) {
            match learn_action {
                measure_guide::GuideAction::AddRows => {
                    self.mode = Mode::Schematic;
                    self.sch.add_measure_rows();
                }
            }
        }
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Schematic, "回路図").on_hover_text("回路図の画面。部品を置いて配線し、シミュレーションします");
                let stale = !self.board_ed.schematic_diff(&self.sch).is_empty();
                ui.selectable_value(&mut self.mode, Mode::Board, if stale { "基板パターン ●" } else { "基板パターン" })
                    .on_hover_text(if stale { "回路図と基板が合っていません。基板の画面で「回路図から取り込み」を押すと反映されます (詳しくは基板の左パネル)" } else { "基板の画面。回路図から部品を取り込んで、配置・配線します" });
                ui.separator();
                let sch_mode = self.mode == Mode::Schematic;
                ui.menu_button("ファイル", |ui| {
                    if ui.button("プロジェクトを開く…").on_hover_text("回路図と基板をまとめて保存したファイル (.ccproj) を開きます。いまの回路図と基板は置き換わります").clicked() {
                        self.open_project();
                        ui.close();
                    }
                    if ui.button("プロジェクトを保存").on_hover_text("回路図と基板を、1つのファイル (.ccproj) にまとめて保存します。まだ保存していなければ、保存先を選びます").clicked() {
                        self.save_project(false);
                        ui.close();
                    }
                    if ui.button("名前を付けてプロジェクトを保存…").on_hover_text("回路図と基板を、別の名前のプロジェクトとして保存します").clicked() {
                        self.save_project(true);
                        ui.close();
                    }
                    ui.separator();
                    ui.label(if sch_mode { "いまの回路図だけ" } else { "いまの基板だけ" });
                    if ui.button("新規").on_hover_text("いまの図を空にして、新しく始めます").clicked() && self.confirm_unsaved_one(sch_mode, "新規にすると、いまの図は消えます。") {
                        if sch_mode { self.sch.new_file() } else { self.board_ed.new_file() }
                        ui.close();
                    }
                    if ui.button("開く…").on_hover_text("保存したファイルを開きます (回路図は .schjson、基板は .pcbjson)。回路図と基板をまとめて開くときは「プロジェクトを開く」").clicked() && self.confirm_unsaved_one(sch_mode, "開くと、いまの図は置き換わります。") {
                        if sch_mode { self.sch.open() } else { self.board_ed.open() }
                        ui.close();
                    }
                    ui.menu_button("サンプル回路を開く", |ui| {
                        for s in samples::SAMPLES {
                            if ui.button(s.title).on_hover_text(s.about).clicked() {
                                if self.confirm_unsaved_one(true, "開くと、いまの回路図は置き換わります。") && self.sch.open_sample(s.title, s.json) {
                                    self.mode = Mode::Schematic;
                                }
                                ui.close();
                            }
                        }
                    }).response.on_hover_text("できあがった回路をすぐ開いて、動かしてみます。保存するときは別の名前になるので、サンプルは壊れません");
                    if ui.button("保存").on_hover_text("いまの図を上書き保存します。まだ保存していなければ、保存先を選びます").clicked() {
                        if sch_mode { self.sch.save(false) } else { self.board_ed.save(false) }
                    }
                    if ui.button("名前を付けて保存…").on_hover_text("別の名前で保存します").clicked() {
                        if sch_mode { self.sch.save(true) } else { self.board_ed.save(true) }
                    }
                    ui.separator();
                    if ui.button("画面をPNGで保存…").on_hover_text("いま見えている画面 (回路図・電圧の色・波形・基板) を画像にします。レポートや質問への添付に使えます").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PNG画像", &["png"]).set_file_name("circuitCad.png").save_file() {
                            self.shot_path = Some(p);
                            // the dialog is closed by the time the next frame is drawn, so the capture shows the app only
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                        }
                        ui.close();
                    }
                    if !sch_mode {
                        ui.separator();
                        if ui.button("発注の準備・製造ファイルの書き出し (Gerber / ドリル / BOM / 部品配置)…").on_hover_text("発注先を選んで、発注前のチェック・出来上がりの確認・Gerber などの書き出しを順に行う窓を開きます").clicked() {
                            self.board_ed.open_order();
                            ui.close();
                        }
                    }
                });
                ui.menu_button("学ぶ", |ui| {
                    if ui.button("学習パス (地図) を開く…").on_hover_text("学習パスの窓を開きます。3 つのパス (安定化電源・Arduino 互換ボード・Mega 互換ボード) を切り替えて、レッスンの関係を樹形図の「学習の地図」で見られます。どこまで進んだかも残ります").clicked() {
                        self.learn.open = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.label("サンプル回路を動かしながら学べます (✔ = 完了)");
                    // the lessons are many: one submenu per learning path, one per stage of it (the lessons no path lists come last)
                    let ids = schematic::lesson_ids();
                    let titles = schematic::lesson_titles();
                    let mut listed = vec![false; ids.len()];
                    let mut start = None;
                    let mut open_path: Option<&str> = None;
                    let mut lesson_button = |ui: &mut egui::Ui, i: usize| {
                        let mark = if self.progress.is_done(ids[i]) { "✔ " } else { "　 " };
                        if ui.button(format!("{mark}{}", titles[i])).clicked() {
                            start = Some(i);
                            ui.close();
                        }
                    };
                    for path in learn::PATHS {
                        ui.menu_button(path.title, |ui| {
                            if ui.button("🗺 学習の地図 (樹形図) を開く…").on_hover_text("このパスの、レッスンの関係を樹形図で見ます (学習パスの窓が開きます)").clicked() {
                                open_path = Some(path.id);
                                ui.close();
                            }
                            ui.separator();
                            for stage in path.stages {
                                let lessons: Vec<usize> = stage.items.iter().filter_map(|it| match it.kind { learn::Kind::Lesson(id) => ids.iter().position(|x| *x == id), _ => None }).collect();
                                if lessons.is_empty() {
                                    continue;
                                }
                                let done = lessons.iter().filter(|&&i| self.progress.is_done(ids[i])).count();
                                ui.menu_button(format!("{} ({done}/{})", stage.name, lessons.len()), |ui| {
                                    for &i in &lessons {
                                        lesson_button(ui, i);
                                    }
                                });
                            }
                        });
                        for stage in path.stages {
                            for it in stage.items {
                                if let learn::Kind::Lesson(id) = it.kind
                                    && let Some(i) = ids.iter().position(|x| *x == id)
                                {
                                    listed[i] = true;
                                }
                            }
                        }
                    }
                    let others: Vec<usize> = (0..ids.len()).filter(|&i| !listed[i]).collect();
                    if !others.is_empty() {
                        ui.menu_button("そのほかのレッスン", |ui| {
                            for &i in &others {
                                lesson_button(ui, i);
                            }
                        });
                    }
                    if let Some(id) = open_path {
                        self.learn.select(id);
                        self.learn.open = true;
                    }
                    if let Some(i) = start {
                        self.mode = Mode::Schematic;
                        self.sch.start_lesson(i);
                    }
                    ui.separator();
                    if ui.button("値の決め方 (計算ツール)…").on_hover_text("LEDの抵抗・分圧・フィルタ・レギュレータ・トランジスタスイッチの値を、計算と注意点・故障パターンつきで決めます。部品の値は、勘ではなく計算で決められます。入力を変えると、結果・計算のしかた・注意点・壊れ方がすぐ下に出ます").clicked() {
                        self.mode = Mode::Schematic;
                        self.sch.open_wizard(0);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("基板のレッスン", |ui| {
                        let board_mark = if self.progress.is_done("board") { "✔ " } else { "　 " };
                        if ui.button(format!("{board_mark}{}", board_editor::lesson::TITLE)).on_hover_text("回路図から基板パターンを作り、発注用データを書き出すところまでの流れを体験します").clicked() {
                            self.start_board_lesson(0);
                            ui.close();
                        }
                        let copper_mark = if self.progress.is_done(board_editor::lesson::COPPER_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{copper_mark}{}", board_editor::lesson::COPPER_TITLE)).on_hover_text("細い配線で電圧が落ちるのをシミュレーションで見て、電流から必要な太さを決めて直します").clicked() {
                            self.start_board_lesson(1);
                            ui.close();
                        }
                        let fix_mark = if self.progress.is_done(board_editor::lesson::FIX_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{fix_mark}{}", board_editor::lesson::FIX_TITLE)).on_hover_text("わざと間違いのある基板を、発注前チェックの表示を頼りに直します (チャレンジ)").clicked() {
                            self.start_board_lesson(2);
                            ui.close();
                        }
                        let arduino_mark = if self.progress.is_done(board_editor::lesson::ARDUINO_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{arduino_mark}{}", board_editor::lesson::ARDUINO_TITLE)).on_hover_text("マイコンの基板で、水晶とバイパスコンデンサを正しい場所へ寄せ、通らない配線の直し方も練習します").clicked() {
                            self.start_board_lesson(3);
                            ui.close();
                        }
                        let usb_mark = if self.progress.is_done(board_editor::lesson::USB_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{usb_mark}{}", board_editor::lesson::USB_TITLE)).on_hover_text("USB の D+ / D- を、差動ペアとして、並べて同じ長さで配線します").clicked() {
                            self.start_board_lesson(4);
                            ui.close();
                        }
                        let mega_mark = if self.progress.is_done(board_editor::lesson::MEGA_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{mega_mark}{}", board_editor::lesson::MEGA_TITLE)).on_hover_text("自動配線が通せなかった 18 本の線が残った Arduino Mega 互換の基板を、手で仕上げます").clicked() {
                            self.start_board_lesson(5);
                            ui.close();
                        }
                    });
                });
                ui.menu_button("編集", |ui| {
                    if ui.button("元に戻す (Ctrl+Z)").on_hover_text("直前の操作を取り消します").clicked() {
                        if sch_mode { self.sch.undo() } else { self.board_ed.undo() }
                    }
                    if ui.button("やり直し (Ctrl+Y)").on_hover_text("取り消した操作をやり直します").clicked() {
                        if sch_mode { self.sch.redo() } else { self.board_ed.redo() }
                    }
                });
                ui.menu_button("ヘルプ", |ui| {
                    for (t, name, tip) in [
                        (help::HelpTab::Schematic, "回路図の操作", "部品の置き方・配線・選択・キー操作"),
                        (help::HelpTab::Waveform, "波形の操作", "ズーム・測定カーソル・信号の選び方"),
                        (help::HelpTab::Board, "基板の操作", "配置・配線・ビア・層の切り替え"),
                        (help::HelpTab::View3d, "3D表示の操作", "回す・動かす・拡大"),
                    ] {
                        if ui.button(name).on_hover_text(tip).clicked() {
                            self.help.show(t);
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.label("F1 キーでも開けます");
                });
                ui.menu_button("設定", |ui| {
                    if ui.button("シミュレーション設定…").on_hover_text("計算の精度や収束のしやすさを調整します。ふだんは変えなくて大丈夫です。「収束しませんでした」と出たとき、または結果の精度を上げたいときに調整します。変えたあとは、解析をもう一度実行してください (「自動で再計算」がオンなら自動で反映されます)。").clicked() {
                        self.sch.show_sim_opts = true;
                        ui.close();
                    }
                    ui.separator();
                    let mut changed = ui.checkbox(&mut self.show_hints, "画面下にヒントを出す").on_hover_text("画面下に、いま何をするとよいかのヒントと豆知識を表示します").changed();
                    let mut only = icons::icon_only();
                    if ui.checkbox(&mut only, "ツールをアイコンだけで表示").on_hover_text("ツールボタンの文字を省き、アイコンだけにします。ボタンにカーソルを重ねると名前と説明が出ます。慣れてきたら画面が広く使えます").changed() {
                        icons::set_icon_only(only);
                        changed = true;
                    }
                    // these two survive a restart
                    if changed {
                        let saved = prefs::Prefs { icon_only: only, show_hints: self.show_hints }.save(&prefs::Prefs::default_file());
                        if let Err(e) = saved {
                            self.sch.status = e;
                        }
                    }
                });
            });
        });
        self.sch.settings_window(ui.ctx());
        self.sch.wizard_window(ui.ctx());
        egui::Panel::bottom("status").show(ui, |ui| {
            // line 1: what just happened (left) and a rotating general tip (right); line 2: what to do next
            ui.horizontal(|ui| {
                ui.label(if self.mode == Mode::Schematic { &self.sch.status } else { &self.board_ed.status });
                if self.show_hints {
                    // rotate the general tips every 12 seconds
                    let t = ui.input(|i| i.time);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(format!("豆知識: {}", TIPS[(t / 12.0) as usize % TIPS.len()])).weak()).truncate());
                    });
                    ui.request_repaint_after(std::time::Duration::from_secs(2));
                }
            });
            if self.show_hints {
                let hint = if self.mode == Mode::Schematic { self.sch.hint() } else { self.board_ed.hint() };
                ui.colored_label(egui::Color32::from_rgb(90, 170, 110), format!("ヒント: {hint}"));
            }
        });
        if self.mode == Mode::Schematic {
            egui::Panel::left("sch_tools").min_size(220.0).show(ui, |ui| self.sch.side_panel(ui));
            if self.sch.has_wave() {
                egui::Panel::bottom("wave").resizable(true).default_size(320.0).show(ui, |ui| self.sch.wave_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| self.sch.canvas(ui));
            self.sch.lesson_window(ui.ctx());
            self.sch.challenge_window(ui.ctx());
            self.sch.part_popup_window(ui.ctx());
            return;
        }
        self.board_ed.note_schematic_diff(&self.sch);
        self.board_ed.order_window(ui.ctx());
        self.board_ed.view3d_window(ui.ctx());
        self.board_ed.lesson_window(ui.ctx());
        egui::Panel::left("board_tools").min_size(240.0).show(ui, |ui| self.board_ed.side_panel(ui, &self.sch));
        egui::CentralPanel::default().show(ui, |ui| self.board_ed.canvas(ui));
    }
}

fn main() -> eframe::Result {
    install_crash_log();
    // developer tool: compare the DC solution with ngspice, no window
    if xcheck::run_cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        return Ok(());
    }
    eframe::run_native(
        "circuitCad",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_maximized(true).with_title("circuitCad"),
            ..Default::default()
        },
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            let mut app = App::default();
            app.progress = learn::Progress::load(&learn::Progress::default_file());
            let prefs = prefs::Prefs::load(&prefs::Prefs::default_file());
            app.show_hints = prefs.show_hints;
            icons::set_icon_only(prefs.icon_only);
            let args: Vec<String> = std::env::args().skip(1).collect();
            apply_args(&mut app, &args);
            // `--screenshot <file>` alone still counts as a plain launch (so the card can be checked on a screenshot)
            let opens_something = args.iter().enumerate().any(|(i, a)| a != "--screenshot" && !(i > 0 && args[i - 1] == "--screenshot"));
            app.welcome = !opens_something && app.progress.needs_welcome();
            Ok(Box::new(app))
        }),
    )
}
