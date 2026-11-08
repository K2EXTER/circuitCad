// No console window on Windows (panics are written to %APPDATA%\circuitCad\crash.log instead).
#![windows_subsystem = "windows"]
// Structs with many fields (calculation results, test fixtures) are filled by assigning after `Default::default()`; that reads better than one long initializer.
#![allow(clippy::field_reassign_with_default)]

use board_editor::BoardEditor;
use eframe::egui;
use schematic::SchEditor;

mod args;
mod autosave;
mod board_editor;
mod files;
mod glossary;
mod help;
mod icons;
mod link;
mod bench_guide;
mod measure_guide;
mod samples;
mod learn;
mod part_form;
mod plot;
/// Starting width of the left tool panels. The panels only grow to fit their widest line (the analysis tab, a selected part's
/// form, the wire tool's settings), so a narrow start made the circuit shift sideways whenever the tab or the selection changed.
const PANEL_WIDTH: f32 = 400.0;

mod prefs;
mod project;
mod schematic;
mod screenshot;
mod source_form;
mod startup;
mod theme;
mod tips;
mod widgets;
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
    /// The three switches of the requirements' frames and lines as last written to the settings (the file is written when one changes).
    req_view: (bool, bool, bool, bool),
    /// Show what is picked in one of the schematic, the board and the 3D view in the others, and what each had picked last frame.
    link_selection: bool,
    last_focus: (link::Focus, link::Focus),
    /// Multisampling the window was made with, and the setting for the next start (the menu changes only the second).
    smooth_edges_running: bool,
    smooth_edges: bool,
    /// Where the screenshot being taken goes (the capture arrives a frame after the request).
    shot_path: Option<std::path::PathBuf>,
    help: help::Help,
    /// Finished lessons (saved in the app data folder) and the learning-path window.
    progress: learn::Progress,
    learn: learn::LearnWindow,
    glossary: glossary::GlossaryWindow,
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
    /// A copy of unsaved work left by an earlier run (asked about at start-up), and when the copy was last written.
    recovery: Option<autosave::Found>,
    last_autosave: std::time::Instant,
}

impl App {
    /// The part or net picked in the schematic or on the board (the one that changed) goes to the other as `linked`; the 3D view reads
    /// the board's. With the link off, nothing is shown in the other.
    fn sync_focus(&mut self) {
        if !self.link_selection {
            if !self.sch.linked.is_empty() || !self.board_ed.linked.is_empty() {
                self.sch.linked = Default::default();
                self.board_ed.linked = Default::default();
            }
            return;
        }
        let (s, b) = (self.sch.focus(), self.board_ed.focus());
        if s != self.last_focus.0 {
            // a pick on the sheet; putting it away puts away what the board shows of it
            self.board_ed.linked = s.clone();
            self.sch.linked = Default::default();
            self.last_focus.0 = s;
        } else if b != self.last_focus.1 {
            self.sch.linked = b.clone();
            self.board_ed.linked = Default::default();
            self.last_focus.1 = b;
        }
    }

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
        Self { welcome: false, show_hints: true, mode: Mode::Schematic, sch: SchEditor::default(), board_ed: BoardEditor::default(), req_view: (true, true, true, true), link_selection: true, last_focus: Default::default(), smooth_edges_running: true, smooth_edges: true, shot_path: None, help: help::Help::default(), progress: learn::Progress::default(), learn: learn::LearnWindow::default(), glossary: glossary::GlossaryWindow::default(), measure: measure_guide::MeasureGuide::default(), bench: bench_guide::BenchGuide::default(), auto_shot: None, project: None, closing_ok: false, recovery: None, last_autosave: std::time::Instant::now() }
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
        // the copy of unsaved work goes away once the window really closes with nothing to lose
        if ui.ctx().input(|i| i.viewport().close_requested()) && (self.closing_ok || !self.dirty()) {
            autosave::clear(&autosave::default_dir());
        }
        self.tick_autosave(ui.ctx());
        self.recovery_window(ui.ctx());
        // the switches of the requirements' frames and of the sheet colours survive a restart
        let now = (self.sch.req_frames, self.board_ed.show_req_marks, self.board_ed.show_req_lines, self.board_ed.show_sheets);
        if now != self.req_view {
            self.req_view = now;
            let file = prefs::Prefs::default_file();
            if let Err(e) = (prefs::Prefs { req_frames: now.0, req_groups: now.1, req_lines: now.2, sheet_colors: now.3, ..prefs::Prefs::load(&file) }).save(&file) {
                self.sch.status = e;
            }
        }
        self.sync_focus();
        self.finish_screenshot(ui);
        self.tick_auto_screenshot(ui);
        // F1, and the small "操作" buttons of the panels, open the guide
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.show(if self.mode == Mode::Schematic { help::HelpTab::Schematic } else { help::HelpTab::Board });
        }
        match self.board_ed.currents_request.take() {
            Some(board_editor::CurrentSource::Dc) => {
                let currents = self.sch.dc_net_currents();
                // taken for "全部つなぐ": the route follows at once and the panel stays shut (opening it switches the tool to the
                // wire tool and pushes the step list below the fold, with "DRC を実行" out of sight)
                let for_route = self.board_ed.route_after_currents.is_some();
                self.board_ed.use_simulated_currents(currents, vec![]);
                if for_route {
                    self.board_ed.close_width_panel();
                }
            }
            Some(board_editor::CurrentSource::Tran) => match self.sch.tran_net_currents() {
                Ok((currents, stats)) => self.board_ed.use_simulated_currents(Ok(currents), stats),
                Err(e) => self.board_ed.use_simulated_currents(Err(e), vec![]),
            },
            None => {}
        }
        // the window of a net asked for the DC current of that net alone
        if let Some(net) = self.board_ed.net_current_request.take() {
            let currents = self.sch.dc_net_currents();
            self.board_ed.fill_net_current(&net, currents);
        }
        // "全部" was pressed with no currents declared: the currents are in now (or could not be had), so route
        if let Some(net) = self.board_ed.route_after_currents.take() {
            self.board_ed.autoroute_now(net.as_deref());
        }
        if std::mem::take(&mut self.board_ed.renumber_request) {
            let plan = self.board_ed.renumber_plan();
            let answer = self.sch.sch.rename_parts(&plan);
            self.board_ed.renumber_applied(&plan, answer);
        }
        if std::mem::take(&mut self.board_ed.to_schematic_request) {
            let trades = self.board_ed.board.pin_map.clone();
            let answer = self.sch.sch.apply_pin_trades(&trades);
            self.board_ed.trades_applied(answer);
        }
        // a finding accepted (or taken back) in the board's list goes into the schematic's table too
        if let Some((add, w)) = self.board_ed.waiver_request.take() {
            let list = &mut self.sch.sch.requirements.waivers;
            let same = |x: &design_spec::Waiver| x.kind == w.kind && x.name == w.name && (x.item.is_empty() || x.item == w.item);
            if add {
                if !list.iter().any(&same) {
                    list.push(w);
                }
            } else {
                list.retain(|x| !same(x));
            }
        }
        if std::mem::take(&mut self.board_ed.open_sch_requirements) {
            self.mode = Mode::Schematic;
            self.sch.open_requirements();
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
        if let Some(word) = self.sch.glossary_request.take().or_else(|| self.board_ed.glossary_request.take()) {
            self.glossary.open_at(word);
        }
        match self.glossary.show(ui.ctx()) {
            Some(glossary::Target::Schematic(i)) => {
                self.mode = Mode::Schematic;
                self.sch.start_lesson(i);
            }
            Some(glossary::Target::Board(which)) => self.start_board_lesson(which),
            None => {}
        }
        let welcome_choice = if self.welcome { learn::welcome_card(ui.ctx(), &self.progress) } else { None };
        if let Some(choice) = welcome_choice {
            self.welcome = false;
            self.progress.mark_welcomed();
            match choice {
                learn::Welcome::Start(a) => action = Some(a),
                learn::Welcome::Path => {
                    self.learn.select(learn::FIRST_PATH_ID);
                    self.learn.open = true;
                }
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
                    if ui.button("KiCad の回路図を開く…").on_hover_text("KiCad 6 以降の回路図 (.kicad_sch) を取り込みます。部品はこのソフトの記号で並べ直し、つながりはネットの名前で表します (KiCad の図の形・位置は引き継ぎません)。子シート・バスなど読めないものは、取り込んだあとの案内に出ます").clicked() && self.confirm_unsaved_one(true, "開くと、いまの回路図は置き換わります。") {
                        self.sch.open_kicad_sch();
                        self.mode = Mode::Schematic;
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
                    if sch_mode && ui.button("回路図をSVGで保存…").on_hover_text("いま見えているシートを、拡大しても粗くならない図 (SVG) にします。ブラウザや Inkscape で開けて、ブラウザの印刷から PDF にもできます。選択やグリッドは入らず、電圧・電流の表示は、出ていればそのまま入ります").clicked() {
                        self.sch.export_svg();
                        ui.close();
                    }
                    if sch_mode && ui.button("回路図をPDFで保存…").on_hover_text("いま見えているシートを、1ページの PDF にします。印刷や提出、メールで送るのに使えます。文字は形(アウトライン)で入るので、どの環境でも同じ見た目ですが、PDF の中で文字の検索やコピーはできません。図枠は右下に入ります。選択やグリッドは入らず、電圧・電流の表示は、出ていればそのまま入ります").clicked() {
                        self.sch.export_pdf();
                        ui.close();
                    }
                    if sch_mode && ui.button("図枠 (タイトル・版・日付)…").on_hover_text("回路の名前・版・日付・作者を書いておきます。SVG に書き出すと、図の右下に枠で入ります").clicked() {
                        self.sch.open_title_block();
                        ui.close();
                    }
                    if !sch_mode && ui.button("ケースの図面 (DXF) から外形と穴を取り込む…").on_hover_text("ケースの CAD で描いた DXF (ASCII 形式) から、基板の外形 (線・円弧・ポリラインがつながって閉じた、いちばん大きい輪) と、その中の円 (取り付け穴) を取り込みます。外形の左下が (0, 0) になります。単位は図面の設定 (INSUNITS) から mm にします。「元に戻す」で戻せます").clicked() {
                        self.board_ed.import_dxf();
                        ui.close();
                    }
                    if !sch_mode && ui.button("基板の外形・穴・部品の位置をDXFで保存…").on_hover_text("基板の外形、取り付け穴、部品を囲む箱と部品番号を、DXF (mm) にします。ケースを設計する CAD (Fusion・FreeCAD・SolidWorks など) に渡して、基板が収まるか、穴の位置が合うかを確かめるのに使います。画面と同じ向き (上から見た形) で書き出します").clicked() {
                        self.board_ed.export_dxf();
                        ui.close();
                    }
                    if !sch_mode {
                        for (label, side) in [("基板の表面をSVGで保存…", pcb_core::Side::Top), ("基板の裏面をSVGで保存…", pcb_core::Side::Bottom)] {
                            if ui.button(label).on_hover_text("基板の絵を、拡大しても粗くならない図 (SVG) にします。銅・シルク・穴は、発注用の Gerber と同じデータから描くので、できあがりの姿です。記事・レポート・印刷に使えます。裏面は、基板を裏返して下から見た向きです").clicked() {
                                self.board_ed.export_svg(side);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("発注の準備・製造ファイルの書き出し (Gerber / ドリル / BOM / 部品配置)…").on_hover_text("発注先を選んで、発注前のチェック・出来上がりの確認・Gerber などの書き出しを順に行う窓を開きます").clicked() {
                            self.board_ed.open_order();
                            ui.close();
                        }
                    }
                });
                ui.menu_button("学ぶ", |ui| {
                    if ui.button("学習パス (地図) を開く…").on_hover_text("学習パスの窓を開きます。パス (部品の素顔・安定化電源・Arduino 互換ボードなど) を切り替えて、レッスンの関係を樹形図の「学習の地図」で見られます。どこまで進んだかも残ります").clicked() {
                        self.learn.open = true;
                        ui.close();
                    }
                    if ui.button("用語集・索引…").on_hover_text("電子回路の言葉と、このソフトの言葉を、読みと説明つきで引けます。語を選ぶと、その語が出てくるレッスンも分かります").clicked() {
                        self.glossary.open = true;
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
                        let pairs_mark = if self.progress.is_done(board_editor::lesson::PAIRS_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{pairs_mark}{}", board_editor::lesson::PAIRS_TITLE)).on_hover_text("2 組の差動ペアを同じ長さにそろえます。蛇行は 2 本をいっしょに曲げるので、間隔が保たれます").clicked() {
                            self.start_board_lesson(6);
                            ui.close();
                        }
                        let repair_mark = if self.progress.is_done(board_editor::lesson::REPAIR_ID) { "✔ " } else { "　 " };
                        if ui.button(format!("{repair_mark}{}", board_editor::lesson::REPAIR_TITLE)).on_hover_text("ちゃんと動く基板に、故障を 1 つ仕込みます。症状から原因を探して、点検の道具で確かめ、直します。何度でも、別の故障で練習できます").clicked() {
                            self.start_board_lesson(7);
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
                    ui.separator();
                    if ui.button("設計の要件 (表)…").on_hover_text("ネットの役割・線幅・電流、近くに置く部品の組、離しておくものを、設計全体の表で書きます。基板に取り込むと、ネットクラス・自動配置・距離のチェックに使われます。回路図・基板のどちらからでも開けます").clicked() {
                        self.mode = Mode::Schematic;
                        self.sch.open_requirements();
                        ui.close();
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
                    if ui.checkbox(&mut self.link_selection, "選んだ部品・ネットを回路図・基板・3Dで連動").on_hover_text("回路図で選んだ部品やネット (配線・ネットラベル) が、基板と3D表示でも光り、基板や3Dで選んだものが、回路図でも光ります (水色の枠と線)。画面を切り替えたときに、同じものをすぐ探せます。オフにすると、それぞれ別々に選びます").changed() {
                        changed = true;
                    }
                    let note = if self.smooth_edges != self.smooth_edges_running { " (次の起動から)" } else { "" };
                    if ui
                        .checkbox(&mut self.smooth_edges, format!("線の縁をなめらかにする{note}"))
                        .on_hover_text("画面の描画を4倍のマルチサンプリング(MSAA)にして、配線やシルクの縁をなめらかに見せます。オンのほうが、描く三角形が少なく軽くなります。起動のときに決まるので、変えると次の起動から効きます。画面が崩れる、極端に重いなど、グラフィックスカードとの相性が悪いときはオフにしてください (起動のときに --no-msaa を付けても、その回だけオフになります)")
                        .changed()
                    {
                        changed = true;
                    }
                    // these survive a restart
                    if changed {
                        let file = prefs::Prefs::default_file();
                        let saved = prefs::Prefs { icon_only: only, show_hints: self.show_hints, smooth_edges: self.smooth_edges, link_selection: self.link_selection, ..prefs::Prefs::load(&file) }.save(&file);
                        if let Err(e) = saved {
                            self.sch.status = e;
                        }
                    }
                });
            });
        });
        self.sch.settings_window(ui.ctx());
        self.sch.wizard_window(ui.ctx());
        self.sch.requirements_window(ui.ctx());
        self.sch.title_block_window(ui.ctx());
        egui::Panel::bottom("status").show(ui, |ui| {
            // line 1: what just happened (left) and a rotating general tip (right); line 2: what to do next
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                let status = if self.mode == Mode::Schematic { &self.sch.status } else { &self.board_ed.status };
                if self.show_hints {
                    // a long status keeps to two thirds of the line (the rest is the tip) and is read in full by pointing at it
                    let width = ui.available_width() * 0.66;
                    ui.scope(|ui| {
                        ui.set_max_width(width);
                        ui.add(egui::Label::new(status.as_str()).truncate()).on_hover_text(status.as_str());
                    });
                } else {
                    ui.label(status.as_str());
                }
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
                // the word "ヒント" carries the colour; the sentence is plain text so a long hint stays easy to read
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("ヒント").strong().color(crate::theme::OK));
                    ui.add(egui::Label::new(hint.as_str()).truncate()).on_hover_text(hint.as_str());
                });
            }
        });
        if self.mode == Mode::Schematic {
            egui::Panel::left("sch_tools").default_size(PANEL_WIDTH).min_size(220.0).frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(12, 8))).show(ui, |ui| self.sch.side_panel(ui));
            if self.sch.has_wave() {
                egui::Panel::bottom("wave").resizable(true).default_size(320.0).show(ui, |ui| self.sch.wave_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| self.sch.canvas(ui));
            self.sch.lesson_window(ui.ctx());
            self.sch.challenge_window(ui.ctx());
            self.sch.part_popup_window(ui.ctx());
            return;
        }
        self.board_ed.sync_requirements(&mut self.sch);
        self.board_ed.note_schematic_diff(&self.sch);
        self.board_ed.dc_currents_ready = self.sch.dc_currents_ready();
        self.board_ed.order_window(ui.ctx());
        self.board_ed.net_window(ui.ctx());
        self.board_ed.current_window(ui.ctx());
        self.board_ed.view3d_window(ui.ctx());
        self.board_ed.lesson_window(ui.ctx());
        egui::Panel::left("board_tools").default_size(PANEL_WIDTH).min_size(240.0).show(ui, |ui| self.board_ed.side_panel(ui, &self.sch));
        egui::CentralPanel::default().show(ui, |ui| self.board_ed.canvas(ui));
    }
}

fn main() -> eframe::Result {
    install_crash_log();
    // developer tool: compare the DC solution with ngspice, no window
    if xcheck::run_cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        return Ok(());
    }
    // multisampling is fixed when the window is made: read the setting before it (`--no-msaa` leaves it out for this start)
    let smooth_edges = prefs::Prefs::load(&prefs::Prefs::default_file()).smooth_edges && !std::env::args().any(|a| a == "--no-msaa");
    board_editor::set_multisampled(smooth_edges);
    eframe::run_native(
        "circuitCad",
        eframe::NativeOptions {
            viewport: {
                let mut vp = egui::ViewportBuilder::default().with_maximized(true).with_title("circuitCad");
                if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
                    vp = vp.with_icon(icon);
                }
                vp
            },
            multisampling: if smooth_edges { 4 } else { 0 },
            ..Default::default()
        },
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            let mut app = App::default();
            app.progress = learn::Progress::load(&learn::Progress::default_file());
            let prefs = prefs::Prefs::load(&prefs::Prefs::default_file());
            app.show_hints = prefs.show_hints;
            (app.smooth_edges_running, app.smooth_edges) = (smooth_edges, prefs.smooth_edges);
            icons::set_icon_only(prefs.icon_only);
            (app.sch.req_frames, app.board_ed.show_req_marks, app.board_ed.show_req_lines, app.board_ed.show_sheets) = (prefs.req_frames, prefs.req_groups, prefs.req_lines, prefs.sheet_colors);
            app.link_selection = prefs.link_selection;
            app.req_view = (prefs.req_frames, prefs.req_groups, prefs.req_lines, prefs.sheet_colors);
            let args: Vec<String> = std::env::args().skip(1).collect();
            apply_args(&mut app, &args);
            // `--screenshot <file>` alone still counts as a plain launch (so the card can be checked on a screenshot)
            let opens_something = args.iter().enumerate().any(|(i, a)| a != "--screenshot" && !(i > 0 && args[i - 1] == "--screenshot"));
            if !opens_something {
                app.look_for_recovery();
            }
            app.welcome = !opens_something && app.recovery.is_none() && app.progress.needs_welcome();
            Ok(Box::new(app))
        }),
    )
}
