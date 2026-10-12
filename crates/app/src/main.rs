// No console window on Windows (panics are written to %APPDATA%\circuitCad\crash.log instead).
#![windows_subsystem = "windows"]

use board_editor::BoardEditor;
use eframe::egui;
use schematic::SchEditor;

mod board_editor;
mod help;
mod learn;
mod part_form;
mod plot;
mod schematic;
mod source_form;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Schematic,
    Board,
}

/// Short general tips, shown one at a time under the context hint.
const TIPS: &[&str] = &[
    "部品やボタンにカーソルを合わせると、使い方の説明が出ます",
    "回路図はホイールで拡大縮小、中ボタンのドラッグで移動できます (中ボタンが使えないときは、Alt を押しながら左ドラッグ)",
    "選択ツールで空いた所を左ドラッグすると、完全に囲んだ部品や配線だけを選べます。右ドラッグだと少しでも触れたものも選べます",
    "「パラメータスイープ」で部品の値を何通りか振ると、カットオフや波形のなまり方の違いを重ねて比べられます",
    "AC解析の「安定性の余裕」で位相余裕・ゲイン余裕が見られます。サンプル opamp_loop_gain で、出力に付けたコンデンサを大きくすると余裕が減る様子を試せます",
    "レッスン「交流を直流にする」では、トランス (ライブラリの「トランス」) とブリッジ整流でできるリップルを測れます。平滑コンデンサの容量は「値の決め方」で計算できます",
    "基板エディタの「3Dで見る」で、出来上がりの色と部品の高さを立体で確認できます。外形の角を丸める (角R) こともできます",
    "電池は電圧源ではなく、内部抵抗を持ち、使うほど電圧が下がります。ライブラリの「電池」と、レッスン「電池の内部抵抗と持ち時間」で試せます",
    "降圧コンバータは、スイッチを高速に入り切りして電圧を下げるので、リニアレギュレータより熱くなりません。レッスン「降圧コンバータ」で、コイルの電流と効率を見られます",
    "抵抗値を変えて解析をやり直すと、回路の動きがどう変わるか体感できます",
    "ノイズ解析の素子名にカーソルを合わせると、雑音の原因と対策が読めます",
    "DCスイープでインバーターの入出力特性を描くと、切替電圧やノイズマージンが分かります",
    "ERC で配線ミスを点検してから解析や基板に進むと、原因探しが楽です",
    "docs/examples にサンプル回路があります。「ファイル → 開く」から読み込めます",
    "基板に進む前に、回路図の部品にフットプリント (形) が合っているか確認しましょう",
    "基板は DRC が 0 件になってから、製造ファイルを書き出して発注します",
    "このヒント欄は、メニューの「ヒント」チェックで消せます",
];

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
    /// `--screenshot <file>`: (file, frames left before the capture); the app closes after saving.
    auto_shot: Option<(std::path::PathBuf, u32)>,
}

impl Default for App {
    fn default() -> Self {
        Self { show_hints: true, mode: Mode::Schematic, sch: SchEditor::default(), board_ed: BoardEditor::default(), shot_path: None, help: help::Help::default(), progress: learn::Progress::default(), learn: learn::LearnWindow::default(), auto_shot: None }
    }
}

impl App {
    /// Opens the board lesson with its sample circuit.
    fn start_board_lesson(&mut self) {
        self.mode = Mode::Board;
        if !self.sch.load_sample(board_editor::BoardEditor::lesson_circuit()) {
            self.board_ed.status = "レッスンの回路を読み込めませんでした".into();
        } else {
            self.board_ed.start_lesson();
        }
    }

    /// `--screenshot`: waits a few frames so the analyses and layout have settled, takes the capture, then quits.
    fn tick_auto_screenshot(&mut self, ui: &egui::Ui) {
        let Some((path, left)) = self.auto_shot.as_mut() else { return };
        ui.ctx().request_repaint();
        if *left > 0 {
            *left -= 1;
        } else if self.shot_path.is_none() {
            self.shot_path = Some(path.clone());
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    /// Writes the captured frame as a PNG once the viewport hands it back.
    fn finish_screenshot(&mut self, ui: &egui::Ui) {
        let Some(path) = self.shot_path.clone() else { return };
        let shot = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(img) = shot else { return };
        self.shot_path = None;
        let [w, h] = img.size;
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        let msg = match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
            Ok(()) => format!("画面を保存しました: {}", path.display()),
            Err(e) => format!("画像を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください"),
        };
        if self.auto_shot.take().is_some() {
            println!("{msg}");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.sch.status = msg.clone();
        self.board_ed.status = msg;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.finish_screenshot(ui);
        self.tick_auto_screenshot(ui);
        // F1, and the small "操作" buttons of the panels, open the guide
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.show(if self.mode == Mode::Schematic { help::HelpTab::Schematic } else { help::HelpTab::Board });
        }
        if let Some(t) = self.sch.help_request.take().or_else(|| self.board_ed.help_request.take()) {
            self.help.show(t);
        }
        self.help.window(ui.ctx());
        // finished lessons go into the saved record; the learning-path window can start one
        if let Some(id) = self.sch.finished_lesson() {
            self.progress.mark(id);
        }
        if self.board_ed.lesson_finished() {
            self.progress.mark("board");
        }
        if let Some(key) = self.sch.finished_challenge() {
            self.progress.mark(&key);
        }
        if let Some(action) = self.learn.show(ui.ctx(), &mut self.progress) {
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
                learn::Action::Board => self.start_board_lesson(),
            }
        }
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Schematic, "回路図").on_hover_text("回路図の画面。部品を置いて配線し、シミュレーションします");
                ui.selectable_value(&mut self.mode, Mode::Board, "基板パターン").on_hover_text("基板の画面。回路図から部品を取り込んで、配置・配線します");
                ui.separator();
                let sch_mode = self.mode == Mode::Schematic;
                ui.menu_button("ファイル", |ui| {
                    if ui.button("新規").on_hover_text("いまの図を空にして、新しく始めます").clicked() {
                        if sch_mode { self.sch.new_file() } else { self.board_ed.new_file() }
                    }
                    if ui.button("開く…").on_hover_text("保存したファイルを開きます (回路図は .schjson、基板は .pcbjson)").clicked() {
                        if sch_mode { self.sch.open() } else { self.board_ed.open() }
                    }
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
                    if ui.button("学習パス: 安定化電源を作ろう…").on_hover_text("入門から、電源の部品、安定化のしくみ、基板と実機まで、順に学ぶ道筋です。どこまで進んだかも残ります").clicked() {
                        self.learn.open = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.label("サンプル回路を動かしながら学べます (✔ = 完了)");
                    let ids = schematic::lesson_ids();
                    for (i, t) in schematic::lesson_titles().into_iter().enumerate() {
                        let mark = if self.progress.is_done(ids[i]) { "✔ " } else { "　 " };
                        if ui.button(format!("{mark}{t}")).clicked() {
                            self.mode = Mode::Schematic;
                            self.sch.start_lesson(i);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("値の決め方 (計算ツール)…").on_hover_text("LEDの抵抗・分圧・フィルタ・レギュレータ・トランジスタスイッチの値を、計算と注意点・故障パターンつきで決めます。部品の値は、勘ではなく計算で決められます。入力を変えると、結果・計算のしかた・注意点・壊れ方がすぐ下に出ます").clicked() {
                        self.mode = Mode::Schematic;
                        self.sch.open_wizard(0);
                        ui.close();
                    }
                    ui.separator();
                    let board_mark = if self.progress.is_done("board") { "✔ " } else { "　 " };
                    if ui.button(format!("{board_mark}{}", board_editor::lesson::TITLE)).on_hover_text("回路図から基板パターンを作り、発注用データを書き出すところまでの流れを体験します").clicked() {
                        self.start_board_lesson();
                        ui.close();
                    }
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
                    ui.checkbox(&mut self.show_hints, "画面下にヒントを出す").on_hover_text("画面下に、いま何をするとよいかのヒントと豆知識を表示します");
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
        self.board_ed.order_window(ui.ctx());
        self.board_ed.view3d_window(ui.ctx());
        self.board_ed.lesson_window(ui.ctx());
        egui::Panel::left("board_tools").min_size(240.0).show(ui, |ui| self.board_ed.side_panel(ui, &self.sch));
        egui::CentralPanel::default().show(ui, |ui| self.board_ed.canvas(ui));
    }
}

/// Without a console a panic would vanish silently: keep it in a log file.
fn install_crash_log() {
    let dir = part_lib::default_dir().parent().map(|p| p.to_path_buf());
    std::panic::set_hook(Box::new(move |info| {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::write(d.join("crash.log"), format!("{info}\n"));
        }
    }));
}

/// Command line: `circuitCad [file.schjson | file.pcbjson] [--dc] [--ac f0 f1 ppd] [--sweep src start stop step] [--param part from to count node [tran]] [--fault part kind [tran|ac node|chain] | scan] [--noise out in|- tempC] [--tran stop step [--spectrum]]
/// [--drc] [--round R] [--3d] [--order] [--guide [schematic|waveform|board|3d]] [--learn] [--lesson <id>] [--select <refdes>] [--new-part] [--challenge <id> [0|1] [--judge]] [--export dir] [--screenshot file.png]`. Mostly useful for scripts and for checking a build.
fn apply_args(app: &mut App, args: &[String]) {
    if let Some(i) = args.iter().position(|a| a == "--lesson")
        && let Some(id) = args.get(i + 1)
        && let Some(n) = schematic::lesson_ids().iter().position(|l| l == id) {
            app.sch.start_lesson(n);
        }
    if let Some(i) = args.iter().position(|a| a == "--challenge")
        && let Some(id) = args.get(i + 1) {
            app.sch.start_challenge(id, args.get(i + 2).and_then(|l| l.parse().ok()).unwrap_or(0));
            if args.iter().any(|a| a == "--judge") {
                app.sch.run_judge();
            }
        }
    if args.iter().any(|a| a == "--new-part") {
        app.sch.open_part_form();
    }
    if args.iter().any(|a| a == "--learn") {
        app.learn.open = true;
    }
    if let Some(i) = args.iter().position(|a| a == "--quick") {
        // open the quick-add box (key A) with a word already typed, for screenshots
        app.sch.open_quick_add_with(args.get(i + 1).filter(|a| !a.starts_with("--")).map_or("", |s| s.as_str()));
    }
    if let Some(i) = args.iter().position(|a| a == "--guide") {
        app.help.show(match args.get(i + 1).map(|s| s.as_str()) {
            Some("waveform") => help::HelpTab::Waveform,
            Some("board") => help::HelpTab::Board,
            Some("3d") => help::HelpTab::View3d,
            _ => help::HelpTab::Schematic,
        });
    }
    if let Some(i) = args.iter().position(|a| a == "--screenshot")
        && let Some(p) = args.get(i + 1) {
            app.auto_shot = Some((p.into(), 40));
        }
    if let Some(p) = args.iter().find(|a| a.ends_with(".schjson")) {
        app.sch.load_path(p.into());
        if args.iter().any(|a| a == "--dc") {
            app.sch.analyze_dc();
        }
        if let Some(i) = args.iter().position(|a| a == "--select")
            && let Some(r) = args.get(i + 1) {
                app.sch.select_part(r);
            }
        if let Some(i) = args.iter().position(|a| a == "--ac")
            && let (Some(f0), Some(f1), Some(p)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                app.sch.set_ac_settings(f0, f1, p);
                app.sch.analyze_ac();
            }
        if let Some(i) = args.iter().position(|a| a == "--sweep") {
            // --sweep <source> <start> <stop> <step>
            if let (Some(src), Some(a), Some(b), Some(st)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3), args.get(i + 4)) {
                app.sch.set_sweep_settings(src, a, b, st);
                app.sch.analyze_sweep();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--fault") {
            // --fault <part> <0 open | 1 short | 2 x10 | 3 /10>   or   --fault scan
            match args.get(i + 1).map(|s| s.as_str()) {
                Some("scan") => app.sch.analyze_fault_scan(),
                Some(part) => {
                    app.sch.set_fault(part, args.get(i + 2).and_then(|k| k.parse().ok()).unwrap_or(0));
                    let mut chain = false;
                    // ... <kind> tran <node>: compare the waveform at <node> instead of the DC voltages
                    // ... <kind> chain: follow the failures that come after (kind 7 / 8 / 9 = coil layer short 10 / 20 / 50%)
                    match args.get(i + 3).map(|m| m.as_str()) {
                        Some("chain") => chain = true,
                        Some("tran") => app.sch.set_fault_tran(args.get(i + 4).map_or("", |n| n.as_str())),
                        Some("ac") => app.sch.set_fault_ac(args.get(i + 4).map_or("", |n| n.as_str())),
                        _ => {}
                    }
                    if chain {
                        app.sch.analyze_fault_chain();
                    } else {
                        app.sch.analyze_fault();
                    }
                }
                None => {}
            }
        }
        for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--spec") {
            // --spec <kind index 0..8> <target> <lo|-> <hi|-> [frequency]  (repeatable; put it after --dc / --tran / --ac)
            if let (Some(k), Some(t), Some(lo), Some(hi)) = (args.get(i + 1).and_then(|k| k.parse().ok()), args.get(i + 2), args.get(i + 3), args.get(i + 4)) {
                app.sch.add_spec(k, t, lo, hi, args.get(i + 5).filter(|a| !a.starts_with("--")).map_or("", |a| a.as_str()));
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--param") {
            // --param <part> <from> <to> <count> <node> [tran]  (AC unless "tran"; uses the --ac / --tran settings)
            if let (Some(part), Some(a), Some(b), Some(n), Some(node)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3), args.get(i + 4), args.get(i + 5)) {
                app.sch.set_param_settings(part, a, b, n, node, args.get(i + 6).is_none_or(|m| m != "tran"));
                app.sch.analyze_param();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--noise") {
            // --noise <out node> <input source or -> <temp C> (uses the --ac frequency range)
            if let (Some(out), Some(inp), Some(t)) = (args.get(i + 1), args.get(i + 2), args.get(i + 3)) {
                let inp = if inp == "-" { "" } else { inp.as_str() };
                app.sch.set_noise_settings(out, inp, t);
                app.sch.analyze_noise();
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--tran")
            && let (Some(stop), Some(step)) = (args.get(i + 1), args.get(i + 2)) {
                app.sch.set_tran_settings(stop, step);
                app.sch.analyze_tran();
                if let Some(j) = args.iter().position(|a| a == "--power")
                    && let Some(n) = args.get(j + 1..j + 5) {
                        app.sch.set_power_signals([n[0].clone(), n[1].clone(), n[2].clone(), n[3].clone()]);
                    }
                if args.iter().any(|a| a == "--spectrum") {
                    app.sch.show_spectrum();
                }
            }
    }
    if let Some(p) = args.iter().find(|a| a.ends_with(".pcbjson")) {
        app.mode = Mode::Board;
        app.board_ed.load_path(p.into());
        if args.iter().any(|a| a == "--drc") {
            app.board_ed.run_drc();
        }
        if args.iter().any(|a| a == "--autoplace") {
            app.board_ed.autoplace();
        }
        if let Some(i) = args.iter().position(|a| a == "--round")
            && let Some(r) = args.get(i + 1).and_then(|r| r.parse::<f64>().ok()) {
                app.board_ed.round_outline(r);
            }
        for (i, _) in args.iter().enumerate().filter(|(_, a)| *a == "--ir") {
            // --ir <net> <amps>  (repeatable): declare the net's current and show the voltage drop of its copper
            if let (Some(net), Some(a)) = (args.get(i + 1), args.get(i + 2).and_then(|a| a.parse::<f64>().ok())) {
                app.board_ed.show_ir_drop(net, a);
            }
        }
        if args.iter().any(|a| a == "--3d") {
            app.board_ed.open_view3d();
        }
        if args.iter().any(|a| a == "--order") {
            app.board_ed.open_order();
        }
        if let Some(i) = args.iter().position(|a| a == "--export")
            && let Some(dir) = args.get(i + 1) {
                app.board_ed.status = match app.board_ed.write_fab_files(std::path::Path::new(dir)) {
                    Ok(n) => format!("書き出し完了: {dir} ({n} ファイル + zip)"),
                    Err(e) => format!("書き出し失敗: {e}"),
                };
            }
    }
}

fn main() -> eframe::Result {
    install_crash_log();
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
            let args: Vec<String> = std::env::args().skip(1).collect();
            apply_args(&mut app, &args);
            Ok(Box::new(app))
        }),
    )
}

/// egui's built-in fonts lack Japanese glyphs, and mixing them with a Windows font puts Latin
/// letters and kana on slightly different baselines. So use one system font that covers both
/// Japanese and half-width alphanumerics for everything; egui's own fonts stay as a fallback.
const BUNDLED_FONT: &[u8] = include_bytes!("../assets/fonts/BIZUDGothic-Regular.ttf");

fn install_japanese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
        // macOS
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux (Noto CJK / IPA / Takao packages)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
    ];
    // fall back to the bundled BIZ UD Gothic (SIL OFL 1.1, see assets/fonts/OFL.txt) so every OS shows Japanese
    // CIRCUITCAD_BUNDLED_FONT=1 skips the system fonts (to check the bundled one)
    let use_system = std::env::var_os("CIRCUITCAD_BUNDLED_FONT").is_none();
    let bytes = CANDIDATES.iter().filter(|_| use_system).find_map(|p| std::fs::read(p).ok()).unwrap_or_else(|| BUNDLED_FONT.to_vec());
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("jp".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().insert(0, "jp".into());
    }
    ctx.set_fonts(fonts);
    // the default body / button size (12.5 px) is not a whole pixel count (13 px fails the same way), and with this font the
    // thin hyphen / minus stroke then vanishes ("1e-6" reads "1e 6", "-3dB" reads "3dB")
    ctx.all_styles_mut(|style| {
        for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style.text_styles.insert(ts, egui::FontId::proportional(14.0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pcb_core::ratsnest::ratsnest;
    use pcb_core::{Point, Side, Track};
    use sch_core::{Kind, Wire};

    /// V1 -> R1 -> R2 -> GND drawn as a schematic, imported into the board.
    #[test]
    fn schematic_to_board_flow() {
        let mut app = App::default();
        let s = &mut app.sch.sch;
        s.add(Kind::VSource, (0, 0), 0);
        s.add(Kind::Resistor, (6, -6), 0);
        s.add(Kind::Resistor, (8, 0), 1);
        s.add(Kind::Ground, (0, 6), 0);
        let r2 = s.components[2].pins();
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), r2[0].1), (r2[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 6))] {
            s.wires.push(Wire { a, b });
        }

        app.board_ed.import_from_schematic(&app.sch);
        let board = &mut app.board_ed.board;
        assert_eq!(board.parts.len(), 3); // V1, R1, R2 (GND symbol is not a part)
        let net = |r: &str, p: &str| board.pads.iter().find(|x| x.refdes == r && x.pin == p).unwrap().net.clone();
        assert_eq!(net("V1", "-"), "GND");
        assert_eq!(net("R2", "2"), "GND");
        assert_eq!(net("R1", "2"), net("R2", "1"));
        // GND: V1.- and R2.2 ; nets +V/R1.1 and R1.2/R2.1 : three 2-pad nets => 3 airwires
        assert_eq!(ratsnest(board).len(), 3);

        // route one net by hand and the airwire disappears
        let a = board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "2").unwrap().pos;
        let b = board.pads.iter().find(|p| p.refdes == "R2" && p.pin == "1").unwrap().pos;
        board.tracks.push(Track { side: Side::Top, a, b, width: 0.25 });
        assert_eq!(ratsnest(board).len(), 2);
    }

    /// A library part (custom symbol + its own package) travels schematic -> netlist -> board.
    #[test]
    fn library_part_reaches_the_board() {
        let mut app = App::default();
        let def = part_lib::EXAMPLES
            .iter()
            .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
            .find(|d| d.name == "LM358")
            .unwrap();
        app.sch.library = vec![def.clone()];
        let u = app.sch.sch.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 10), 0);
        app.sch.sch.add(Kind::Resistor, (34, 10), 0);
        let out = app.sch.sch.components[u].pins().iter().find(|p| p.0 == "1").unwrap().1;
        app.sch.sch.wires.push(Wire { a: out, b: (32, 10) });

        app.board_ed.import_from_schematic(&app.sch);
        let board = &mut app.board_ed.board;
        assert!(board.footprints.iter().any(|f| f.name == "SOIC-8_P1.27"));
        assert_eq!(board.pads.iter().filter(|p| p.refdes == "U1").count(), 8);
        let u1_out = board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap().clone();
        let r1_in = board.pads.iter().find(|p| p.refdes == "R1" && p.pin == "1").unwrap();
        assert!(!u1_out.net.is_empty() && u1_out.net == r1_in.net);
        // moving the part keeps its nets
        board.place_part("U1", Point::new(10.0, 10.0), 0);
        let again = board.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap();
        assert_eq!(again.net, u1_out.net);
        assert_eq!(ratsnest(board).len(), 1);
    }

    #[test]
    fn dc_analysis_from_the_editor() {
        let mut app = App::default();
        let s = &mut app.sch.sch;
        s.add(Kind::VSource, (0, 0), 0);
        s.components[0].value = "10".into();
        s.add(Kind::Resistor, (6, -6), 0);
        s.add(Kind::Resistor, (12, 0), 1);
        s.add(Kind::Ground, (0, 6), 0);
        let r2 = s.components[2].pins();
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), r2[0].1), (r2[1].1, (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
            s.wires.push(Wire { a, b });
        }
        app.sch.analyze_dc();
        assert!(app.sch.status.contains("収束"), "{}", app.sch.status);
        // R1 = R2 = 10k default: the midpoint is at half the supply
        assert!((app.sch.pin_voltage("R2", "1").unwrap() - 5.0).abs() < 1e-6);
        assert!(app.sch.pin_voltage("V1", "-").unwrap().abs() < 1e-9);

        // a broken value is reported instead of a result
        app.sch.sch.components[1].value = "oops".into();
        app.sch.analyze_dc();
        assert!(app.sch.status.contains("失敗"));
    }

    #[test]
    fn command_line_opens_files_in_the_right_mode() {
        let dir = std::env::temp_dir().join(format!("circuitcad_cli_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut b = pcb_core::Board::new("cli", 20.0, 10.0);
        b.pads.push(pcb_core::Pad::new(Point::new(5.0, 5.0), pcb_core::PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top));
        let path = dir.join("b.pcbjson");
        std::fs::write(&path, serde_json::to_string(&b).unwrap()).unwrap();
        let out = dir.join("fab");
        std::fs::create_dir_all(&out).unwrap();

        let mut app = App::default();
        let args: Vec<String> = [path.to_str().unwrap(), "--drc", "--export", out.to_str().unwrap()].iter().map(|s| s.to_string()).collect();
        apply_args(&mut app, &args);
        assert!(app.mode == Mode::Board);
        assert_eq!(app.board_ed.board.name, "cli");
        assert!(app.board_ed.status.contains("書き出し完了"), "{}", app.board_ed.status);
        for f in ["cli.GTL", "cli.GKO", "cli-PTH.DRL", "cli-BOM.csv", "cli-CPL.csv", "cli_gerber.zip"] {
            assert!(out.join(f).exists(), "{f} missing");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Generates docs/examples/rlc_board.pcbjson from the RLC example schematic:
    /// `cargo test -p app write_example_board -- --ignored`
    #[test]
    #[ignore]
    fn write_example_board() {
        use pcb_core::{Pad, Zone};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("rlc_pulse.schjson"));
        app.board_ed.import_from_schematic(&app.sch);
        let b = &mut app.board_ed.board;
        b.name = "rlc".into();
        b.set_rect_outline(0.0, 0.0, 40.0, 25.0);
        for (r, x, y, rot) in [("V1", 6.0, 14.0, 0), ("R1", 16.0, 18.0, 0), ("L1", 25.0, 18.0, 0), ("C1", 33.0, 12.0, 3)] {
            b.place_part(r, Point::new(x, y), rot);
        }
        let pad = |b: &pcb_core::Board, r: &str, pin: &str| b.pads.iter().find(|p| p.refdes == r && p.pin == pin).map(|p: &Pad| p.pos).unwrap();
        let route = |b: &mut pcb_core::Board, side: Side, pts: &[Point]| {
            for w in pts.windows(2) {
                b.tracks.push(Track { side, a: w[0], b: w[1], width: 0.4 });
            }
        };
        let (v_p, v_m) = (pad(b, "V1", "+"), pad(b, "V1", "-"));
        let (r1, r2) = (pad(b, "R1", "1"), pad(b, "R1", "2"));
        let (l1, l2) = (pad(b, "L1", "1"), pad(b, "L1", "2"));
        let (c1, c2) = (pad(b, "C1", "1"), pad(b, "C1", "2"));
        route(b, Side::Top, &[v_p, Point::new(v_p.x, r1.y), r1]);
        route(b, Side::Top, &[r2, l1]);
        route(b, Side::Top, &[l2, Point::new(c1.x, l2.y), c1]);
        // GND: C1.2 reaches the bottom pour through a via, V1.- is a through-hole pad
        b.vias.push(pcb_core::Via { pos: Point::new(c2.x, c2.y - 1.8), diameter: 0.8, drill: 0.4 });
        route(b, Side::Top, &[c2, Point::new(c2.x, c2.y - 1.8)]);
        let _ = v_m;
        b.zones.push(Zone::new("GND", Side::Bottom, vec![Point::new(2.0, 2.0), Point::new(38.0, 2.0), Point::new(38.0, 23.0), Point::new(2.0, 23.0)]));
        b.holes.push(pcb_core::Hole::new(Point::new(3.0, 3.0), 3.0));
        b.holes.push(pcb_core::Hole::new(Point::new(37.0, 3.0), 3.0));
        let v = pcb_core::drc::check(b);
        println!("DRC: {v:?}");
        std::fs::write(root.join("rlc_board.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
    }

    /// Writes docs/examples/divider_tolerance.schjson: `cargo test -p app write_divider_example -- --ignored`
    #[test]
    #[ignore]
    fn write_divider_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut s = sch_core::Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        let r1 = s.add(Kind::Resistor, (6, -6), 0);
        let r2 = s.add(Kind::Resistor, (12, 0), 1);
        s.add(Kind::Ground, (0, 8), 0);
        s.components[v].value = "5".into();
        s.components[r1].value = "10k".into();
        s.components[r2].value = "10k".into();
        let rp = s.components[r2].pins();
        for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), rp[0].1), (rp[1].1, (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6)), ((0, 6), (0, 8))] {
            s.wires.push(Wire { a, b });
        }
        s.labels.push(sch_core::Label { pos: (12, -6), text: "MID".into() });
        std::fs::write(root.join("divider_tolerance.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Writes docs/examples/li_ion_charger.schjson: `cargo test -p app write_charger_example -- --ignored`
    #[test]
    #[ignore]
    fn write_charger_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let app = App::default();
        let lib = &app.sch.library;
        let tp = lib.iter().find(|d| d.name == "TP4056").expect("TP4056 is bundled");
        let mut s = sch_core::Schematic::default();
        let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
            let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
            s.labels.push(sch_core::Label { pos: at, text: text.into() });
        };
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "5".into();
        label(&mut s, v, "+", "VIN");
        label(&mut s, v, "-", "GND");
        let u = s.add_custom(tp.symbol(), &tp.default_value(), &tp.footprint_name(), (30, 0), 0);
        for (pin, net) in [("1", "GND"), ("2", "PROG"), ("3", "GND"), ("4", "VIN"), ("5", "BAT"), ("6", "STDBY"), ("7", "CHRG"), ("8", "VIN")] {
            label(&mut s, u, pin, net);
        }
        let rp = s.add(Kind::Resistor, (30, 22), 0);
        s.components[rp].value = "6k".into();
        label(&mut s, rp, "1", "PROG");
        label(&mut s, rp, "2", "GND");
        let cell = s.add(Kind::Capacitor, (56, 0), 1);
        s.components[cell].value = "470u IC=2.6 ESR=0.5".into();
        label(&mut s, cell, "1", "BAT");
        label(&mut s, cell, "2", "GND");
        for (k, (net, name)) in [("CHRG", "充電中"), ("STDBY", "充電完了")].iter().enumerate() {
            let y = 14 + 12 * k as i32;
            let r = s.add(Kind::Resistor, (70, y), 0);
            let d = s.add(Kind::Led, (84, y), 0);
            s.components[r].value = "1k".into();
            label(&mut s, r, "1", "VIN");
            let tag = format!("A{k}");
            label(&mut s, r, "2", &tag);
            label(&mut s, d, "A", &tag);
            label(&mut s, d, "K", net);
            let _ = name;
        }
        std::fs::write(root.join("li_ion_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Writes docs/examples/li_ion_cell_charge.schjson: the TP4056 charger sample with the 470 uF stand-in replaced
    /// by the 18650 cell model (starts at 20 %): `cargo test -p app write_cell_charge_example -- --ignored`
    #[test]
    #[ignore]
    fn write_cell_charge_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("li_ion_charger.schjson")).unwrap()).unwrap();
        let cell = s.components.iter().position(|c| c.kind == Kind::Capacitor && c.value.starts_with("470u")).unwrap();
        let pins: Vec<_> = s.components[cell].pins().into_iter().map(|p| p.1).collect();
        s.labels.retain(|l| !pins.contains(&l.pos));
        s.components.remove(cell);
        let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LIION_18650_CHG").unwrap();
        let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (62, 0), 0);
        for (pin, net) in [("1", "BAT"), ("2", "GND")] {
            let at = s.components[bt].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
            s.labels.push(sch_core::Label { pos: at, text: net.into() });
        }
        // the PROG resistor sets 1 A (1200 / 1.2k); 6k would be 0.2 A
        s.components.iter_mut().find(|c| c.value == "6k").unwrap().value = "1.2k".into();
        std::fs::write(root.join("li_ion_cell_charge.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Writes docs/examples/tp5100_cell_charge.schjson: the TP5100 charger sample with the 470 uF stand-in replaced
    /// by the 18650 cell model (starts at 20 %): `cargo test -p app write_tp5100_cell_charge_example -- --ignored`
    #[test]
    #[ignore]
    fn write_tp5100_cell_charge_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut s: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("tp5100_charger.schjson")).unwrap()).unwrap();
        let cell = s.components.iter().position(|c| c.refdes == "C2").unwrap();
        let (pos, pins) = (s.components[cell].pos, s.components[cell].pins().into_iter().map(|p| p.1).collect::<Vec<_>>());
        s.labels.retain(|l| !pins.contains(&l.pos));
        s.components.remove(cell);
        let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "LIION_18650_CHG").unwrap();
        let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), pos, 0);
        for (pin, net) in [("1", "BAT"), ("2", "GND")] {
            let at = s.components[bt].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
            s.labels.push(sch_core::Label { pos: at, text: net.into() });
        }
        std::fs::write(root.join("tp5100_cell_charge.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Writes docs/examples/tp5100_charger.schjson: `cargo test -p app write_tp5100_example -- --ignored`
    #[test]
    #[ignore]
    fn write_tp5100_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let app = App::default();
        let tp = app.sch.library.iter().find(|d| d.name == "TP5100").expect("TP5100 is bundled");
        let mut s = sch_core::Schematic::default();
        let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
            let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
            s.labels.push(sch_core::Label { pos: at, text: text.into() });
        };
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "9".into();
        label(&mut s, v, "+", "VIN");
        label(&mut s, v, "-", "GND");
        let cin = s.add(Kind::Capacitor, (10, 0), 1);
        s.components[cin].value = "10u".into();
        label(&mut s, cin, "1", "VIN");
        label(&mut s, cin, "2", "GND");
        let u = s.add_custom(tp.symbol(), &tp.default_value(), &tp.footprint_name(), (34, 0), 0);
        for (pin, net) in [("1", "VIN"), ("2", "LX"), ("7", "GND"), ("8", "VS"), ("9", "BAT"), ("10", "VREG"), ("11", "GND"), ("12", "RTRICK"), ("14", "STDBY"), ("15", "CHRG")] {
            label(&mut s, u, pin, net);
        }
        // the inductor and the catch diode of the buck stage, and the current sense resistor
        let l = s.add(Kind::Inductor, (52, -8), 0);
        s.components[l].value = "10u".into();
        label(&mut s, l, "1", "LX");
        label(&mut s, l, "2", "VS");
        let d = s.add(Kind::Diode, (52, 6), 1);
        s.components[d].value = "1N5817".into();
        label(&mut s, d, "A", "GND");
        label(&mut s, d, "K", "LX");
        let rs = s.add(Kind::Resistor, (66, -8), 0);
        s.components[rs].value = "0.1".into();
        label(&mut s, rs, "1", "VS");
        label(&mut s, rs, "2", "BAT");
        let cell = s.add(Kind::Capacitor, (80, 0), 1);
        s.components[cell].value = "470u IC=2.6 ESR=0.5".into();
        label(&mut s, cell, "1", "BAT");
        label(&mut s, cell, "2", "GND");
        let creg = s.add(Kind::Capacitor, (10, 16), 1);
        s.components[creg].value = "100n".into();
        label(&mut s, creg, "1", "VREG");
        label(&mut s, creg, "2", "GND");
        let rt = s.add(Kind::Resistor, (22, 16), 1);
        s.components[rt].value = "50k".into();
        label(&mut s, rt, "1", "RTRICK");
        label(&mut s, rt, "2", "GND");
        for (k, net) in ["CHRG", "STDBY"].iter().enumerate() {
            let y = 22 + 12 * k as i32;
            let r = s.add(Kind::Resistor, (44, y), 0);
            let d = s.add(Kind::Led, (58, y), 0);
            s.components[r].value = "1k".into();
            label(&mut s, r, "1", "VIN");
            let tag = format!("A{k}");
            label(&mut s, r, "2", &tag);
            label(&mut s, d, "A", &tag);
            label(&mut s, d, "K", net);
        }
        std::fs::write(root.join("tp5100_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Writes docs/examples/ip2332_charger.schjson: `cargo test -p app write_ip2332_example -- --ignored`
    #[test]
    #[ignore]
    fn write_ip2332_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let app = App::default();
        let ip = app.sch.library.iter().find(|d| d.name == "IP2332").expect("IP2332 is bundled");
        let mut s = sch_core::Schematic::default();
        let label = |s: &mut sch_core::Schematic, comp: usize, pin: &str, text: &str| {
            let at = s.components[comp].pins().into_iter().find(|p| p.0 == pin).unwrap().1;
            s.labels.push(sch_core::Label { pos: at, text: text.into() });
        };
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "5".into();
        label(&mut s, v, "+", "VIN");
        label(&mut s, v, "-", "GND");
        let cin = s.add(Kind::Capacitor, (10, 0), 1);
        s.components[cin].value = "10u".into();
        label(&mut s, cin, "1", "VIN");
        label(&mut s, cin, "2", "GND");
        let u = s.add_custom(ip.symbol(), &ip.default_value(), &ip.footprint_name(), (34, 0), 0);
        for (pin, net) in [("8", "VIN"), ("6", "LX"), ("5", "BAT"), ("7", "GND"), ("1", "NTC"), ("2", "ISET"), ("4", "LED1"), ("3", "LED2")] {
            label(&mut s, u, pin, net);
        }
        let l = s.add(Kind::Inductor, (52, -8), 0);
        s.components[l].value = "2.2u".into();
        label(&mut s, l, "1", "LX");
        label(&mut s, l, "2", "BAT");
        let cout = s.add(Kind::Capacitor, (64, 0), 1);
        s.components[cout].value = "10u".into();
        label(&mut s, cout, "1", "BAT");
        label(&mut s, cout, "2", "GND");
        let cell = s.add(Kind::Capacitor, (76, 0), 1);
        s.components[cell].value = "470u IC=2.6 ESR=0.1".into();
        label(&mut s, cell, "1", "BAT");
        label(&mut s, cell, "2", "GND");
        let riset = s.add(Kind::Resistor, (10, 16), 1);
        s.components[riset].value = "4.3k".into();
        label(&mut s, riset, "1", "ISET");
        label(&mut s, riset, "2", "GND");
        let rntc = s.add(Kind::Resistor, (22, 16), 1);
        s.components[rntc].value = "10k".into();
        label(&mut s, rntc, "1", "NTC");
        label(&mut s, rntc, "2", "GND");
        // LED1 is a constant-current output (no resistor), LED2 needs one
        let d1 = s.add(Kind::Led, (48, 18), 0);
        label(&mut s, d1, "A", "LED1");
        label(&mut s, d1, "K", "GND");
        let r2 = s.add(Kind::Resistor, (48, 30), 0);
        s.components[r2].value = "2k".into();
        label(&mut s, r2, "1", "LED2");
        label(&mut s, r2, "2", "A2");
        let d2 = s.add(Kind::Led, (62, 30), 0);
        label(&mut s, d2, "A", "A2");
        label(&mut s, d2, "K", "GND");
        std::fs::write(root.join("ip2332_charger.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// The router on a real circuit: the NE555 blinker laid out on a grid of parts.
    #[test]
    fn autoroute_handles_the_ne555_blinker() {
        use pcb_core::ratsnest::ratsnest;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut app = App::default();
        app.sch.load_path(root.join("ne555_blinker.schjson"));
        app.board_ed.import_from_schematic(&app.sch);
        let b = &mut app.board_ed.board;
        b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
        let refs: Vec<String> = b.parts.iter().map(|p| p.refdes.clone()).collect();
        for (k, r) in refs.iter().enumerate() {
            b.place_part(r, Point::new(8.0 + 10.0 * (k % 5) as f64, 10.0 + 14.0 * (k / 5) as f64), 0);
        }
        let before = ratsnest(b).len();
        assert!(before > 0);
        app.board_ed.autoroute(None);
        let left = ratsnest(&app.board_ed.board);
        println!("{} -> {} unrouted; {}", before, left.len(), app.board_ed.status);
        assert!(left.len() < before, "{}", app.board_ed.status);
        // whatever it routed keeps the rules
        assert!(pcb_core::drc::check(&app.board_ed.board).is_empty(), "{:?}", pcb_core::drc::check(&app.board_ed.board));
        app.board_ed.undo();
        assert_eq!(ratsnest(&app.board_ed.board).len(), before, "one undo takes the whole run back");
    }

    /// Schematic -> import -> autoplace -> autoroute on every shipped example: the placement is
    /// always rule-clean and the router finishes most of the work.
    #[test]
    fn autoplace_then_autoroute_on_the_shipped_examples() {
        use pcb_core::ratsnest::ratsnest;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for name in ["rc_lowpass", "led_blinker", "ne555_blinker", "opamp_amp", "regulator_led", "tp5100_charger", "ip2332_charger"] {
            let mut app = App::default();
            app.sch.load_path(root.join(format!("{name}.schjson")));
            app.board_ed.import_from_schematic(&app.sch);
            app.board_ed.autoplace();
            assert!(app.board_ed.status.starts_with("自動配置"), "{name}: {}", app.board_ed.status);
            assert!(pcb_core::drc::check(&app.board_ed.board).is_empty(), "{name}: {:?}", pcb_core::drc::check(&app.board_ed.board));
            let before = ratsnest(&app.board_ed.board).len();
            app.board_ed.autoroute(None);
            let left = ratsnest(&app.board_ed.board).len();
            println!("{name}: {before} -> {left} unrouted");
            assert!(left * 2 <= before, "{name}: {before} -> {left}");
            app.board_ed.undo();
            app.board_ed.undo();
            assert!(app.board_ed.board.parts.iter().all(|p| p.pos.x > 0.0 || p.pos.y > 0.0));
        }
    }

    /// The board lesson ticks all its steps when the learner does what it asks.
    #[test]
    fn the_board_lesson_walks_from_schematic_to_the_order_window() {
        let mut app = App::default();
        assert!(app.sch.load_sample(BoardEditor::lesson_circuit()));
        app.board_ed.start_lesson();
        let progress = |a: &mut App| a.board_ed.lesson_progress();
        assert_eq!(progress(&mut app), 0);
        app.board_ed.import_from_schematic(&app.sch);
        assert_eq!(progress(&mut app), 1);
        app.board_ed.autoplace();
        assert_eq!(progress(&mut app), 2);
        app.board_ed.autoroute(None);
        assert_eq!(progress(&mut app), 3, "{}", app.board_ed.status);
        app.board_ed.run_drc();
        assert_eq!(progress(&mut app), 4, "{}", app.board_ed.status);
        app.board_ed.open_order();
        assert_eq!(progress(&mut app), 5);
    }

    /// Writes scratch boards (imported + auto-placed + auto-routed) for looking at the 3D view:
    /// `cargo test -p app write_3d_demos -- --ignored`
    #[test]
    #[ignore]
    fn write_3d_demos() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let dir = std::env::temp_dir().join("claude");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["opamp_amp", "regulator_led", "bridge_rectifier", "ne555_blinker"] {
            let mut app = App::default();
            app.sch.load_path(root.join(format!("{name}.schjson")));
            app.board_ed.import_from_schematic(&app.sch);
            app.board_ed.autoplace();
            app.board_ed.autoroute(None);
            std::fs::write(dir.join(format!("3d_{name}.pcbjson")), serde_json::to_string_pretty(&app.board_ed.board).unwrap()).unwrap();
            println!("{name}: {} parts", app.board_ed.board.parts.len());
        }
    }

    /// Writes a scratch board with parts on both sides (not part of the repository):
    /// `cargo test -p app write_bottom_demo -- --ignored`
    #[test]
    #[ignore]
    fn write_bottom_demo() {
        let mut app = App::default();
        app.sch.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/rlc_pulse.schjson"));
        app.board_ed.import_from_schematic(&app.sch);
        let b = &mut app.board_ed.board;
        b.name = "bottom".into();
        b.set_rect_outline(0.0, 0.0, 40.0, 25.0);
        for (r, x, y, rot) in [("V1", 6.0, 14.0, 0), ("R1", 16.0, 18.0, 0), ("L1", 25.0, 18.0, 0), ("C1", 33.0, 12.0, 3)] {
            b.place_part(r, Point::new(x, y), rot);
        }
        b.flip_part("L1");
        b.flip_part("C1");
        let dir = std::env::temp_dir().join("claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bottom_demo.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
    }

    /// Generates docs/examples/buck_converter.schjson (12 V -> BUCK_DEMO -> 5 V):
    /// `cargo test -p app write_buck_example -- --ignored`
    #[test]
    #[ignore]
    fn write_buck_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
        let (buck, schottky) = (find("BUCK_DEMO"), find("1N5819"));
        let mut s = sch_core::Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "12".into();
        let u = s.add_custom(buck.symbol(), &buck.default_value(), &buck.footprint_name(), (24, 0), 0);
        let l = s.add(Kind::Inductor, (44, -6), 0);
        s.components[l].value = "100u DCR=0.05".into();
        let d = s.add_custom(schottky.symbol(), &schottky.default_value(), &schottky.footprint_name(), (44, 8), 0);
        let c = s.add(Kind::Capacitor, (62, 4), 1);
        s.components[c].value = "100u ESR=0.05".into();
        let r1 = s.add(Kind::Resistor, (74, -4), 1);
        s.components[r1].value = "30k".into();
        let r2 = s.add(Kind::Resistor, (74, 8), 1);
        s.components[r2].value = "10k".into();
        let rl = s.add(Kind::Resistor, (86, 4), 1);
        s.components[rl].value = "20".into();
        let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
            let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
            let c = s.components[idx].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            s.wires.push(Wire { a: pt, b: end });
            s.labels.push(sch_core::Label { pos: end, text: text.into() });
        };
        label(&mut s, v, "+", "VIN");
        label(&mut s, v, "-", "GND");
        label(&mut s, u, "1", "VIN");
        label(&mut s, u, "2", "GND");
        label(&mut s, u, "3", "SW");
        label(&mut s, u, "4", "FB");
        label(&mut s, l, "1", "SW");
        label(&mut s, l, "2", "VOUT");
        for (pin, net) in [("A", "GND"), ("K", "SW")] {
            label(&mut s, d, pin, net);
        }
        label(&mut s, c, "1", "VOUT");
        label(&mut s, c, "2", "GND");
        label(&mut s, r1, "1", "VOUT");
        label(&mut s, r1, "2", "FB");
        label(&mut s, r2, "1", "FB");
        label(&mut s, r2, "2", "GND");
        label(&mut s, rl, "1", "VOUT");
        label(&mut s, rl, "2", "GND");
        std::fs::write(root.join("buck_converter.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/buck_cout_electrolytic.schjson and buck_cout_ceramic.schjson: the buck sample with
    /// its output capacitor swapped for a library part (the rest is identical, so the two can be compared):
    /// `cargo test -p app write_buck_cout_examples -- --ignored`
    #[test]
    #[ignore]
    fn write_buck_cout_examples() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let base: sch_core::Schematic = serde_json::from_str(&std::fs::read_to_string(root.join("buck_converter.schjson")).unwrap()).unwrap();
        let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
        for (file, part) in [("buck_cout_electrolytic.schjson", "CE_100u_16V"), ("buck_cout_ceramic.schjson", "C_10u_0805")] {
            let def = find(part);
            let mut s = base.clone();
            let c = s.components.iter_mut().find(|c| c.refdes == "C1").unwrap();
            c.kind = Kind::Custom;
            c.custom = Some(def.symbol());
            c.value = def.default_value();
            c.footprint = def.footprint_name();
            std::fs::write(root.join(file), serde_json::to_string_pretty(&s).unwrap()).unwrap();
        }
    }

    /// Generates docs/examples/cap_impedance.schjson: a 1 V AC source feeds three 1 ohm resistors, each loaded by a
    /// capacitor (electrolytic / ceramic / both in parallel). With the 1 ohm series resistor, V(node) ~ |Z| while |Z| << 1 ohm:
    /// `cargo test -p app write_cap_impedance_example -- --ignored`
    #[test]
    #[ignore]
    fn write_cap_impedance_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let find = |n: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == n).unwrap();
        let (elec, cer) = (find("CE_100u_16V"), find("C_10u_0805"));
        let mut s = sch_core::Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "SIN(0 1 1k) AC 1".into();
        let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
            let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap_or_else(|| panic!("pin {pin}"));
            let c = s.components[idx].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            s.wires.push(Wire { a: pt, b: end });
            s.labels.push(sch_core::Label { pos: end, text: text.into() });
        };
        label(&mut s, v, "+", "IN");
        label(&mut s, v, "-", "GND");
        // (node, x, parts on that node)
        for (node, x, caps) in [("ZE", 20, vec![(&elec, 0)]), ("ZC", 36, vec![(&cer, 0)]), ("ZP", 52, vec![(&elec, 0), (&cer, 12)])] {
            let r = s.add(Kind::Resistor, (x, 0), 1);
            s.components[r].value = "1".into();
            label(&mut s, r, "1", "IN");
            label(&mut s, r, "2", node);
            for (def, dx) in caps {
                let c = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (x + dx, 14), 1);
                label(&mut s, c, "1", node);
                label(&mut s, c, "2", "GND");
            }
        }
        std::fs::write(root.join("cap_impedance.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/coin_cell_led.schjson (CR2032 -> resistor -> LED):
    /// `cargo test -p app write_coin_cell_example -- --ignored`
    #[test]
    #[ignore]
    fn write_coin_cell_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == "CR2032").unwrap();
        let mut s = sch_core::Schematic::default();
        let bt = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (0, 0), 0);
        let r = s.add(Kind::Resistor, (20, 0), 0);
        s.components[r].value = "100".into();
        let d = s.add(Kind::Led, (34, 0), 0);
        let label = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
            let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
            let c = s.components[idx].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            s.wires.push(Wire { a: pt, b: end });
            s.labels.push(sch_core::Label { pos: end, text: text.into() });
        };
        label(&mut s, bt, "1", "VBAT");
        label(&mut s, bt, "2", "GND");
        label(&mut s, r, "1", "VBAT");
        label(&mut s, r, "2", "N1");
        label(&mut s, d, "A", "N1");
        label(&mut s, d, "K", "GND");
        std::fs::write(root.join("coin_cell_led.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/bridge_rectifier.schjson (100 V AC -> transformer -> bridge -> smoothing):
    /// `cargo test -p app write_rectifier_example -- --ignored`
    #[test]
    #[ignore]
    fn write_rectifier_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = part_lib::EXAMPLES
            .iter()
            .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
            .find(|d| d.name == "TR100_12V_5VA")
            .unwrap();
        let mut s = sch_core::Schematic::default();
        let v = s.add(Kind::VSource, (0, 0), 0);
        s.components[v].value = "SIN(0 141.4 50) AC 1".into();
        let t = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (16, 0), 0);
        let mut diodes = vec![];
        for (k, x) in [(0, 40), (1, 52), (2, 64), (3, 76)] {
            diodes.push(s.add(Kind::Diode, (x, 0), 0));
            s.components[diodes[k]].value = "1N4007".into();
        }
        let c = s.add(Kind::Capacitor, (92, 0), 1);
        s.components[c].value = "1000u".into();
        let r = s.add(Kind::Resistor, (104, 0), 1);
        s.components[r].value = "100".into();
        // a short wire out of the pin, with the net name at its end (so the name does not sit on the symbol)
        let label = |s: &mut sch_core::Schematic, idx: usize, pin: usize, text: &str| {
            let pt = s.components[idx].pins()[pin].1;
            let c = s.components[idx].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            s.wires.push(Wire { a: pt, b: end });
            s.labels.push(sch_core::Label { pos: end, text: text.into() });
        };
        for (i, p) in s.components[t].pins().iter().enumerate() { println!("T pin {i} {:?} {:?}", p.0, p.1); }
        for (i, p) in s.components[diodes[0]].pins().iter().enumerate() { println!("D pin {i} {:?} {:?}", p.0, p.1); }
        label(&mut s, v, 0, "MAINS");
        label(&mut s, v, 1, "GND");
        label(&mut s, t, 0, "MAINS");
        label(&mut s, t, 1, "GND");
        label(&mut s, t, 2, "AC1");
        label(&mut s, t, 3, "AC2");
        // pins of a diode: 0 = anode, 1 = cathode
        for (k, (a, kk)) in [("AC1", "VDC"), ("AC2", "VDC"), ("GND", "AC1"), ("GND", "AC2")].iter().enumerate() {
            label(&mut s, diodes[k], 0, a);
            label(&mut s, diodes[k], 1, kk);
        }
        label(&mut s, c, 0, "VDC");
        label(&mut s, c, 1, "GND");
        label(&mut s, r, 0, "VDC");
        label(&mut s, r, 1, "GND");
        std::fs::write(root.join("bridge_rectifier.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/mosfet_switch.schjson (5 V pulse -> 2N7000 gate -> 47 ohm load to 5 V):
    /// `cargo test -p app write_mosfet_example -- --ignored`
    #[test]
    #[ignore]
    fn write_mosfet_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = part_lib::EXAMPLES
            .iter()
            .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
            .find(|d| d.name == "2N7000")
            .unwrap();
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "5".into();
        let v2 = s.add(Kind::VSource, (0, 14), 0);
        s.components[v2].value = "PULSE(0 5 100u 1u 1u 400u 1m)".into();
        let r1 = s.add(Kind::Resistor, (10, 8), 0);
        s.components[r1].value = "100".into();
        let r2 = s.add(Kind::Resistor, (20, -4), 1);
        s.components[r2].value = "47".into();
        s.components[r2].footprint = "R_2512".into();
        let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (20, 8), 0);
        // a short wire out of the pin with the net name at its end, so the name does not sit on the reference text
        let label_at = |s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str| {
            let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
            let c = s.components[idx].pos;
            let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
            let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
            s.wires.push(Wire { a: pt, b: end });
            s.labels.push(sch_core::Label { pos: end, text: text.into() });
        };
        label_at(&mut s, v1, "+", "VCC");
        label_at(&mut s, v1, "-", "GND");
        label_at(&mut s, v2, "+", "IN");
        label_at(&mut s, v2, "-", "GND");
        label_at(&mut s, r1, "1", "IN");
        label_at(&mut s, r1, "2", "G");
        label_at(&mut s, r2, "1", "VCC");
        label_at(&mut s, r2, "2", "OUT");
        label_at(&mut s, q, "2", "G");
        label_at(&mut s, q, "3", "OUT");
        label_at(&mut s, q, "1", "GND");
        std::fs::write(root.join("mosfet_switch.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Short wire out of a pin with the net name at its end (shared by the sample generators below).
    fn label_pin(s: &mut sch_core::Schematic, idx: usize, pin: &str, text: &str) {
        let (_, pt) = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap();
        let c = s.components[idx].pos;
        let (dx, dy) = (pt.0 - c.0, pt.1 - c.1);
        let end = if dx.abs() >= dy.abs() { (pt.0 + 3 * dx.signum(), pt.1) } else { (pt.0, pt.1 + 3 * dy.signum()) };
        s.wires.push(Wire { a: pt, b: end });
        s.labels.push(sch_core::Label { pos: end, text: text.into() });
    }

    fn library_part(name: &str) -> part_lib::PartDef {
        part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == name).unwrap()
    }

    /// Generates docs/examples/pnp_highside.schjson (PNP 2N3906 switching a 220 ohm load from the 5 V rail):
    /// `cargo test -p app write_pnp_example -- --ignored`
    #[test]
    #[ignore]
    fn write_pnp_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = library_part("2N3906");
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "5".into();
        let v2 = s.add(Kind::VSource, (0, 16), 0);
        s.components[v2].value = "PULSE(5 0 100u 1u 1u 400u 1m)".into();
        let r1 = s.add(Kind::Resistor, (12, 10), 0);
        s.components[r1].value = "4.7k".into();
        let r3 = s.add(Kind::Resistor, (20, -2), 1);
        s.components[r3].value = "10k".into();
        let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 6), 0);
        let r2 = s.add(Kind::Resistor, (28, 20), 1);
        s.components[r2].value = "220".into();
        s.components[r2].footprint = "R_1206".into();
        label_pin(&mut s, v1, "+", "VCC");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, v2, "+", "IN");
        label_pin(&mut s, v2, "-", "GND");
        label_pin(&mut s, r1, "1", "IN");
        label_pin(&mut s, r1, "2", "B");
        label_pin(&mut s, r3, "1", "VCC");
        label_pin(&mut s, r3, "2", "B");
        label_pin(&mut s, q, "2", "B");
        label_pin(&mut s, q, "1", "VCC");
        label_pin(&mut s, q, "3", "OUT");
        label_pin(&mut s, r2, "1", "OUT");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("pnp_highside.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/coil_layer_short.schjson (a relay coil driven by a 2N7000 with a flyback diode):
    /// `cargo test -p app write_coil_example -- --ignored`
    #[test]
    #[ignore]
    fn write_coil_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = library_part("2N7000");
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "12".into();
        let v2 = s.add(Kind::VSource, (0, 16), 0);
        s.components[v2].value = "PULSE(0 5 0 10u 10u 1m 2m)".into();
        let r1 = s.add(Kind::Resistor, (12, 10), 0);
        s.components[r1].value = "100".into();
        let l1 = s.add(Kind::Inductor, (28, -6), 1);
        s.components[l1].value = "20m DCR=80".into();
        let d1 = s.add(Kind::Diode, (38, -6), 1);
        let q = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 10), 0);
        label_pin(&mut s, v1, "+", "VCC");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, v2, "+", "IN");
        label_pin(&mut s, v2, "-", "GND");
        label_pin(&mut s, r1, "1", "IN");
        label_pin(&mut s, r1, "2", "G");
        label_pin(&mut s, l1, "1", "VCC");
        label_pin(&mut s, l1, "2", "OUT");
        label_pin(&mut s, d1, "A", "OUT");
        label_pin(&mut s, d1, "K", "VCC");
        label_pin(&mut s, q, "2", "G");
        label_pin(&mut s, q, "3", "OUT");
        label_pin(&mut s, q, "1", "GND");
        std::fs::write(root.join("coil_layer_short.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/opamp_inverting.schjson (LM358, gain -10, +-5 V supplies):
    /// `cargo test -p app write_inverting_example -- --ignored`
    #[test]
    #[ignore]
    fn write_inverting_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = library_part("LM358");
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "SIN(0 0.1 1k) AC 1".into();
        let r1 = s.add(Kind::Resistor, (14, 0), 0);
        s.components[r1].value = "1k".into();
        let r2 = s.add(Kind::Resistor, (26, -10), 0);
        s.components[r2].value = "10k".into();
        let u = s.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (28, 2), 0);
        let vp = s.add(Kind::VSource, (44, -6), 0);
        s.components[vp].value = "5".into();
        let vn = s.add(Kind::VSource, (44, 10), 0);
        s.components[vn].value = "5".into();
        label_pin(&mut s, v1, "+", "IN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r1, "1", "IN");
        label_pin(&mut s, r1, "2", "N");
        label_pin(&mut s, r2, "1", "N");
        label_pin(&mut s, r2, "2", "OUT");
        label_pin(&mut s, u, "2", "N");
        label_pin(&mut s, u, "3", "GND");
        label_pin(&mut s, u, "1", "OUT");
        label_pin(&mut s, u, "8", "VCC");
        label_pin(&mut s, u, "4", "VEE");
        label_pin(&mut s, vp, "+", "VCC");
        label_pin(&mut s, vp, "-", "GND");
        label_pin(&mut s, vn, "+", "GND");
        label_pin(&mut s, vn, "-", "VEE");
        std::fs::write(root.join("opamp_inverting.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/diode_iv.schjson (two diodes, each behind 100 ohm, fed by one source to sweep):
    /// `cargo test -p app write_diode_example -- --ignored`
    #[test]
    #[ignore]
    fn write_diode_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let (si, schottky) = (library_part("1N4007"), library_part("1N5819"));
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "3".into();
        let r1 = s.add(Kind::Resistor, (14, -6), 0);
        s.components[r1].value = "150".into();
        let d1 = s.add_custom(si.symbol(), &si.default_value(), &si.footprint_name(), (28, -6), 0);
        let r2 = s.add(Kind::Resistor, (14, 8), 0);
        s.components[r2].value = "150".into();
        let d2 = s.add_custom(schottky.symbol(), &schottky.default_value(), &schottky.footprint_name(), (28, 8), 0);
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r1, "1", "VIN");
        label_pin(&mut s, r1, "2", "A1");
        label_pin(&mut s, d1, "A", "A1");
        label_pin(&mut s, d1, "K", "GND");
        label_pin(&mut s, r2, "1", "VIN");
        label_pin(&mut s, r2, "2", "A2");
        label_pin(&mut s, d2, "A", "A2");
        label_pin(&mut s, d2, "K", "GND");
        std::fs::write(root.join("diode_iv.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/zener_regulator.schjson (9 V -> 150 ohm -> 5.1 V zener, 1 k load):
    /// `cargo test -p app write_zener_example -- --ignored`
    #[test]
    #[ignore]
    fn write_zener_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let z = library_part("1N4733A");
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "9".into();
        let r1 = s.add(Kind::Resistor, (14, -8), 0);
        s.components[r1].value = "150".into();
        s.components[r1].footprint = "R_1206".into();
        let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 2), 1);
        let r2 = s.add(Kind::Resistor, (42, 2), 1);
        s.components[r2].value = "1k".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r1, "1", "VIN");
        label_pin(&mut s, r1, "2", "VOUT");
        label_pin(&mut s, d1, "K", "VOUT");
        label_pin(&mut s, d1, "A", "GND");
        label_pin(&mut s, r2, "1", "VOUT");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("zener_regulator.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/emitter_follower.schjson (zener reference + NPN follower, 220 ohm load):
    /// `cargo test -p app write_follower_example -- --ignored`
    #[test]
    #[ignore]
    fn write_follower_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let (z, q) = (library_part("1N4733A"), library_part("2N3904"));
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "9".into();
        let r1 = s.add(Kind::Resistor, (14, -8), 0);
        s.components[r1].value = "330".into();
        let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 2), 1);
        let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (44, -2), 0);
        let r2 = s.add(Kind::Resistor, (58, 6), 1);
        s.components[r2].value = "220".into();
        s.components[r2].footprint = "R_1206".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r1, "1", "VIN");
        label_pin(&mut s, r1, "2", "Z");
        label_pin(&mut s, d1, "K", "Z");
        label_pin(&mut s, d1, "A", "GND");
        for (pin, net) in [("C", "VIN"), ("B", "Z"), ("E", "VOUT")] {
            // the library part names its pins by number: look the number up by name
            let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
            label_pin(&mut s, q1, &num, net);
        }
        label_pin(&mut s, r2, "1", "VOUT");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("emitter_follower.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/series_regulator.schjson (3.3 V zener reference + LM358 + NPN pass transistor
    /// + feedback divider -> 5 V, 100 ohm load):
    /// `cargo test -p app write_series_regulator_example -- --ignored`
    #[test]
    #[ignore]
    fn write_series_regulator_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let (z, q, op) = (library_part("1N4728A"), library_part("2N3904"), library_part("LM358"));
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "9".into();
        let r1 = s.add(Kind::Resistor, (14, -10), 0);
        s.components[r1].value = "1k".into();
        s.components[r1].footprint = "R_1206".into();
        let d1 = s.add_custom(z.symbol(), &z.default_value(), &z.footprint_name(), (28, 0), 1);
        let u = s.add_custom(op.symbol(), &op.default_value(), &op.footprint_name(), (44, -4), 0);
        let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (62, -8), 0);
        let r2 = s.add(Kind::Resistor, (78, 4), 1);
        s.components[r2].value = "100".into();
        s.components[r2].footprint = "R_2512".into();
        let r3 = s.add(Kind::Resistor, (62, 10), 1);
        s.components[r3].value = "5.1k".into();
        let r4 = s.add(Kind::Resistor, (50, 14), 1);
        s.components[r4].value = "10k".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r1, "1", "VIN");
        label_pin(&mut s, r1, "2", "VREF");
        label_pin(&mut s, d1, "K", "VREF");
        label_pin(&mut s, d1, "A", "GND");
        label_pin(&mut s, u, "3", "VREF");
        label_pin(&mut s, u, "2", "FB");
        label_pin(&mut s, u, "1", "DRIVE");
        label_pin(&mut s, u, "8", "VIN");
        label_pin(&mut s, u, "4", "GND");
        for (pin, net) in [("C", "VIN"), ("B", "DRIVE"), ("E", "VOUT")] {
            let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
            label_pin(&mut s, q1, &num, net);
        }
        label_pin(&mut s, r3, "1", "VOUT");
        label_pin(&mut s, r3, "2", "FB");
        label_pin(&mut s, r4, "1", "FB");
        label_pin(&mut s, r4, "2", "GND");
        label_pin(&mut s, r2, "1", "VOUT");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("series_regulator.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/regulator_78xx.schjson (12 V -> LM7805 with 0.33 uF / 0.1 uF capacitors, 100 ohm load):
    /// `cargo test -p app write_78xx_example -- --ignored`
    #[test]
    #[ignore]
    fn write_78xx_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let u = library_part("LM7805");
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "12".into();
        let c1 = s.add(Kind::Capacitor, (14, 4), 1);
        s.components[c1].value = "330n".into();
        let u1 = s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (30, -4), 0);
        let c2 = s.add(Kind::Capacitor, (46, 4), 1);
        s.components[c2].value = "100n".into();
        let r2 = s.add(Kind::Resistor, (60, 4), 1);
        s.components[r2].value = "100".into();
        s.components[r2].footprint = "R_2512".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, c1, "1", "VIN");
        label_pin(&mut s, c1, "2", "GND");
        label_pin(&mut s, u1, "1", "VIN");
        label_pin(&mut s, u1, "2", "GND");
        label_pin(&mut s, u1, "3", "VOUT");
        label_pin(&mut s, c2, "1", "VOUT");
        label_pin(&mut s, c2, "2", "GND");
        label_pin(&mut s, r2, "1", "VOUT");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("regulator_78xx.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/reverse_protection.schjson (9 V source feeding three 330 ohm loads: bare, behind a
    /// Schottky diode, behind a P-channel MOSFET; set V1 to -9 to connect the supply backwards):
    /// `cargo test -p app write_protection_example -- --ignored`
    #[test]
    #[ignore]
    fn write_protection_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let (d, q) = (library_part("1N5819"), library_part("AO3401A"));
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "9".into();
        let r3 = s.add(Kind::Resistor, (20, -16), 1);
        s.components[r3].value = "330".into();
        s.components[r3].footprint = "R_2512".into();
        let d1 = s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (16, 0), 0);
        let r1 = s.add(Kind::Resistor, (34, 4), 1);
        s.components[r1].value = "330".into();
        s.components[r1].footprint = "R_2512".into();
        let q1 = s.add_custom(q.symbol(), &q.default_value(), &q.footprint_name(), (16, 16), 0);
        let r2 = s.add(Kind::Resistor, (34, 20), 1);
        s.components[r2].value = "330".into();
        s.components[r2].footprint = "R_2512".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, r3, "1", "VIN");
        label_pin(&mut s, r3, "2", "GND");
        label_pin(&mut s, d1, "A", "VIN");
        label_pin(&mut s, d1, "K", "OUT_D");
        label_pin(&mut s, r1, "1", "OUT_D");
        label_pin(&mut s, r1, "2", "GND");
        for (pin, net) in [("D", "VIN"), ("G", "GND"), ("S", "OUT_M")] {
            let num = q.pins.iter().find(|p| p.name == pin).map(|p| p.number.clone()).unwrap();
            label_pin(&mut s, q1, &num, net);
        }
        label_pin(&mut s, r2, "1", "OUT_M");
        label_pin(&mut s, r2, "2", "GND");
        std::fs::write(root.join("reverse_protection.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/power_supply.schjson (the final challenge's answer: 12 V -> reverse-protection
    /// Schottky diode -> LM7805 with its two capacitors -> 5 V):
    /// `cargo test -p app write_power_supply_example -- --ignored`
    #[test]
    #[ignore]
    fn write_power_supply_example() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let (d, u) = (library_part("1N5819"), library_part("LM7805"));
        let mut s = sch_core::Schematic::default();
        let v1 = s.add(Kind::VSource, (0, 0), 0);
        s.components[v1].value = "12".into();
        let d1 = s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (14, -8), 0);
        let c1 = s.add(Kind::Capacitor, (28, 4), 1);
        s.components[c1].value = "330n".into();
        let u1 = s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (42, -4), 0);
        let c2 = s.add(Kind::Capacitor, (58, 4), 1);
        s.components[c2].value = "100n".into();
        label_pin(&mut s, v1, "+", "VIN");
        label_pin(&mut s, v1, "-", "GND");
        label_pin(&mut s, d1, "A", "VIN");
        label_pin(&mut s, d1, "K", "VPROT");
        label_pin(&mut s, c1, "1", "VPROT");
        label_pin(&mut s, c1, "2", "GND");
        label_pin(&mut s, u1, "1", "VPROT");
        label_pin(&mut s, u1, "2", "GND");
        label_pin(&mut s, u1, "3", "VOUT");
        label_pin(&mut s, c2, "1", "VOUT");
        label_pin(&mut s, c2, "2", "GND");
        std::fs::write(root.join("power_supply.schjson"), serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Generates docs/examples/regulator_led.schjson (12 V -> LM7805 -> resistor -> LED):
    /// `cargo test -p app write_library_demo -- --ignored`
    #[test]
    #[ignore]
    fn write_library_demo() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let def = part_lib::EXAMPLES
            .iter()
            .map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap())
            .find(|d| d.name == "LM7805")
            .unwrap();
        let mut sch = sch_core::Schematic::default();
        sch.add(Kind::VSource, (0, 0), 0);
        sch.components[0].value = "12".into();
        sch.add_custom(def.symbol(), &def.default_value(), &def.footprint_name(), (14, 0), 0);
        sch.add(Kind::Resistor, (22, 0), 0);
        sch.components[2].value = "330".into();
        sch.add(Kind::Led, (28, 0), 0);
        sch.add(Kind::Ground, (0, 8), 0);
        for (a, b) in [
            ((0, -2), (0, -6)), ((0, -6), (10, -6)), ((10, -6), (10, 0)), // V1+ -> IN
            ((18, 0), (20, 0)), ((24, 0), (26, 0)),                       // OUT -> R1 -> LED
            ((30, 0), (30, 8)), ((14, 4), (14, 8)), ((0, 2), (0, 8)), ((0, 8), (30, 8)),
        ] {
            sch.wires.push(Wire { a, b });
        }
        std::fs::write(root.join("regulator_led.schjson"), serde_json::to_string_pretty(&sch).unwrap()).unwrap();
    }

    /// Scratch board with one of every bundled footprint (not part of the repository):
    /// `cargo test -p app write_footprint_sheet -- --ignored`
    #[test]
    #[ignore]
    fn write_footprint_sheet() {
        let mut b = pcb_core::Board::new("sheet", 90.0, 50.0);
        let mut seen = std::collections::HashSet::new();
        let (mut x, mut y, mut n) = (8.0, 42.0, 0);
        for (_, text) in part_lib::EXAMPLES {
            let def: part_lib::PartDef = serde_json::from_str(text).unwrap();
            if !seen.insert(def.footprint_name()) {
                continue;
            }
            b.add_footprint(def.footprint().unwrap());
            n += 1;
            let refdes = format!("U{n}");
            b.set_part(pcb_core::Part { refdes, value: def.name.clone(), footprint: def.footprint_name(), pos: Point::new(x, y), rot: 0, side: Side::Top }).unwrap();
            x += if def.footprint_name().contains("TQFP") { 16.0 } else { 11.0 };
            if x > 80.0 {
                x = 8.0;
                y -= 14.0;
            }
        }
        let dir = std::env::temp_dir().join("claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sheet.pcbjson"), serde_json::to_string_pretty(&b).unwrap()).unwrap();
    }
}
