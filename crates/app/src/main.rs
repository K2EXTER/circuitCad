// No console window on Windows (panics are written to %APPDATA%\circuitCad\crash.log instead).
#![windows_subsystem = "windows"]

use board_editor::BoardEditor;
use eframe::egui;
use schematic::SchEditor;

mod board_editor;
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
    "回路図はホイールで拡大縮小、中ボタンのドラッグで移動できます",
    "選択ツールで空いた所を左ドラッグすると、完全に囲んだ部品や配線だけを選べます。右ドラッグだと少しでも触れたものも選べます",
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
}

impl Default for App {
    fn default() -> Self {
        Self { show_hints: true, mode: Mode::Schematic, sch: SchEditor::default(), board_ed: BoardEditor::default() }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Schematic, "回路図");
                ui.selectable_value(&mut self.mode, Mode::Board, "基板パターン");
                ui.separator();
                ui.checkbox(&mut self.show_hints, "ヒント").on_hover_text("画面下に、いま何をするとよいかのヒントを表示します");
                ui.separator();
                let sch_mode = self.mode == Mode::Schematic;
                ui.menu_button("ファイル", |ui| {
                    if ui.button("新規").clicked() {
                        if sch_mode { self.sch.new_file() } else { self.board_ed.new_file() }
                    }
                    if ui.button("開く…").clicked() {
                        if sch_mode { self.sch.open() } else { self.board_ed.open() }
                    }
                    if ui.button("保存").clicked() {
                        if sch_mode { self.sch.save(false) } else { self.board_ed.save(false) }
                    }
                    if ui.button("名前を付けて保存…").clicked() {
                        if sch_mode { self.sch.save(true) } else { self.board_ed.save(true) }
                    }
                    if !sch_mode {
                        ui.separator();
                        if ui.button("製造ファイルを書き出し (Gerber / ドリル / BOM / 部品配置)…").clicked() {
                            self.board_ed.export();
                        }
                    }
                });
                ui.menu_button("編集", |ui| {
                    if ui.button("元に戻す (Ctrl+Z)").clicked() {
                        if sch_mode { self.sch.undo() } else { self.board_ed.undo() }
                    }
                    if ui.button("やり直し (Ctrl+Y)").clicked() {
                        if sch_mode { self.sch.redo() } else { self.board_ed.redo() }
                    }
                });
                if !sch_mode {
                    if ui.button("DRC").clicked() {
                        self.board_ed.run_drc();
                    }
                }
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(if self.mode == Mode::Schematic { &self.sch.status } else { &self.board_ed.status });
            if self.show_hints {
                let hint = if self.mode == Mode::Schematic { self.sch.hint() } else { self.board_ed.hint() };
                ui.colored_label(egui::Color32::from_rgb(90, 170, 110), format!("ヒント: {hint}"));
                // rotate the general tips every 12 seconds
                let t = ui.input(|i| i.time);
                ui.small(format!("豆知識: {}", TIPS[(t / 12.0) as usize % TIPS.len()]));
                ui.request_repaint_after(std::time::Duration::from_secs(2));
            }
        });
        if self.mode == Mode::Schematic {
            egui::Panel::left("sch_tools").min_size(220.0).show(ui, |ui| self.sch.side_panel(ui));
            if self.sch.has_wave() {
                egui::Panel::bottom("wave").resizable(true).default_size(320.0).show(ui, |ui| self.sch.wave_panel(ui));
            }
            egui::CentralPanel::default().show(ui, |ui| self.sch.canvas(ui));
            return;
        }
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

/// Command line: `circuitCad [file.schjson | file.pcbjson] [--dc] [--ac f0 f1 ppd] [--sweep src start stop step] [--noise out in|- tempC] [--tran stop step [--spectrum]]
/// [--drc] [--export dir]`. Mostly useful for scripts and for checking a build.
fn apply_args(app: &mut App, args: &[String]) {
    if let Some(p) = args.iter().find(|a| a.ends_with(".schjson")) {
        app.sch.load_path(p.into());
        if args.iter().any(|a| a == "--dc") {
            app.sch.analyze_dc();
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
            let args: Vec<String> = std::env::args().skip(1).collect();
            apply_args(&mut app, &args);
            Ok(Box::new(app))
        }),
    )
}

/// egui's built-in fonts lack Japanese glyphs, and mixing them with a Windows font puts Latin
/// letters and kana on slightly different baselines. So use one Windows font that covers both
/// Japanese and half-width alphanumerics for everything; egui's own fonts stay as a fallback.
fn install_japanese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("jp".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().insert(0, "jp".into());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
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
        b.holes.push(pcb_core::Hole { pos: Point::new(3.0, 3.0), drill: 3.0 });
        b.holes.push(pcb_core::Hole { pos: Point::new(37.0, 3.0), drill: 3.0 });
        let v = pcb_core::drc::check(b);
        println!("DRC: {v:?}");
        std::fs::write(root.join("rlc_board.pcbjson"), serde_json::to_string_pretty(&*b).unwrap()).unwrap();
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
