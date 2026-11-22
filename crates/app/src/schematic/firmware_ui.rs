//! "ビルドと書き込み" (candidate W4) at the bottom of the "ピン定義" window: the firmware's tool chain, folder and board kept with
//! the schematic (`sch_core::firmware`), the commands they make, the tools missing, and the run with its log. The run goes on another
//! thread and shows in the status line ("止める (Esc)" stops the tool).

use super::*;
use sch_core::firmware::{build_steps, flash_steps, missing, FirmwareBuild, Toolchain, TOOLCHAINS};

/// A build or load running (or the last one): the run (its log), when it began, and what it is.
pub(super) struct FirmwareRun {
    pub(super) run: std::sync::Arc<sch_core::firmware::Run>,
    pub(super) started: std::time::Instant,
    pub(super) what: &'static str,
}

const BUSY: &str = "いま動いています。終わるか、止めてからにしてください";

impl SchEditor {
    /// The folder the firmware folder is relative to: the schematic's (the current folder when it is not saved yet).
    fn firmware_base(&self) -> std::path::PathBuf {
        self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    /// Whether a build or load is going on now.
    pub(super) fn firmware_running(&self) -> bool {
        self.fw_run.as_ref().is_some_and(|r| r.run.done.lock().unwrap().is_none())
    }

    pub(super) fn firmware_contents(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            crate::widgets::heading_inline(ui, "ビルドと書き込み");
            crate::icons::help(
                ui,
                "ファームをビルドして、ボードに書き込みます。外部のツール (arduino-cli・Pico SDK・yosys など) を呼ぶだけで、ツールは同梱していません。無いときは、何をどこから入れるかを出します。設定は回路図のファイルに保存され、ccad の「firmware」でも同じことができます",
            );
        });
        let base = self.firmware_base();
        let running = self.firmware_running();
        let fw: &mut FirmwareBuild = &mut self.sch.firmware;
        let kind = Toolchain::parse(&fw.toolchain);
        egui::Grid::new("fw_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(i18n::tr("種類"));
            let shown = kind.map_or_else(|| i18n::tr("選んでください").to_string(), |k| k.name().to_string());
            egui::ComboBox::from_id_salt("fw_kind").selected_text(shown).show_ui(ui, |ui| {
                for (k, key, name, tip) in TOOLCHAINS {
                    if ui.selectable_label(kind == Some(*k), i18n::tr_dyn(name)).on_hover_text(i18n::tr_dyn(tip)).clicked() {
                        fw.toolchain = key.to_string();
                    }
                }
            });
            ui.end_row();
            ui.label(i18n::tr("フォルダ"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut fw.folder).desired_width(260.0).hint_text("firmware/blink"))
                    .on_hover_text(i18n::tr("スケッチ・CMake のプロジェクト・Verilog のあるフォルダ。回路図のファイルの場所からの相対か、絶対のパス"));
                if ui.small_button(i18n::tr("選ぶ…")).on_hover_text(i18n::tr("フォルダを選びます (回路図と同じ場所の中なら、相対のパスにします)")).clicked()
                    && let Some(d) = rfd::FileDialog::new().set_directory(&base).pick_folder()
                {
                    fw.folder = d.strip_prefix(&base).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| d.to_string_lossy().to_string());
                }
            });
            ui.end_row();
            match kind {
                Some(Toolchain::Arduino) => {
                    ui.label(i18n::tr("ボード"));
                    ui.add(egui::TextEdit::singleline(&mut fw.board).hint_text("arduino:avr:mega"))
                        .on_hover_text(i18n::tr("ボードの FQBN。UNO は arduino:avr:uno、Mega は arduino:avr:mega、Nano は arduino:avr:nano。arduino-cli board list で、つないだボードのものが分かります"));
                    ui.end_row();
                    ui.label(i18n::tr("ポート"));
                    ui.add(egui::TextEdit::singleline(&mut fw.port).hint_text("COM3")).on_hover_text(i18n::tr("書き込むシリアルポート (Windows は COM3 など、デバイスマネージャーの「ポート」で分かります)"));
                    ui.end_row();
                }
                Some(Toolchain::Pico) => {
                    ui.label(i18n::tr("ボード"));
                    ui.add(egui::TextEdit::singleline(&mut fw.board).hint_text("pico")).on_hover_text(i18n::tr("PICO_BOARD の名前 (pico・pico_w・pico2)。空なら pico"));
                    ui.end_row();
                }
                Some(Toolchain::Ice40) => {
                    ui.label(i18n::tr("デバイス"));
                    ui.add(egui::TextEdit::singleline(&mut fw.board).hint_text("up5k:sg48"))
                        .on_hover_text(i18n::tr("デバイスとパッケージを「:」でつなぎます (iCEBreaker・UPduino は up5k:sg48、iCEstick は hx1k:tq144)"));
                    ui.end_row();
                    ui.label(i18n::tr("トップ"));
                    ui.add(egui::TextEdit::singleline(&mut fw.top).hint_text("top")).on_hover_text(i18n::tr("いちばん上のモジュールの名前"));
                    ui.end_row();
                    ui.label(i18n::tr("制約"));
                    ui.add(egui::TextEdit::singleline(&mut fw.pcf).hint_text("top.pcf"))
                        .on_hover_text(i18n::tr("ピンの制約ファイル (.pcf)。上の「書き出す形式」で FPGA の .pcf を選ぶと、回路図から書き出せます"));
                    ui.end_row();
                }
                Some(Toolchain::Custom) => {
                    ui.label(i18n::tr("ビルド"));
                    ui.add(egui::TextEdit::singleline(&mut fw.build_cmd).desired_width(300.0).hint_text("make")).on_hover_text(i18n::tr("ビルドのコマンド。フォルダの中で、シェル (Windows は cmd) で動かします"));
                    ui.end_row();
                    ui.label(i18n::tr("書き込み"));
                    ui.add(egui::TextEdit::singleline(&mut fw.flash_cmd).desired_width(300.0).hint_text("make flash")).on_hover_text(i18n::tr("書き込みのコマンド"));
                    ui.end_row();
                }
                None => {}
            }
        });
        // the commands as they will run, and what is missing on this computer
        let build = build_steps(fw, &base);
        let flash = flash_steps(fw, &base);
        let all: Vec<_> = build.iter().chain(flash.iter()).flatten().cloned().collect();
        for (tool, how) in missing(&all) {
            ui.colored_label(crate::theme::warn(), i18n::trf!("▲ 「{tool}」が見つかりません")).on_hover_text(i18n::tr_dyn(how));
        }
        let pico = kind == Some(Toolchain::Pico);
        let busy = running.then(|| BUSY.to_string());
        let why_build = busy.clone().or_else(|| build.as_ref().err().cloned());
        // a Pico's load needs the .uf2 of a build: "ビルドして書き込む" works it out after the build
        let flash_err = flash.as_ref().err().cloned();
        let why_flash_after = busy.clone().or_else(|| flash_err.clone().filter(|e| !(pico && e.contains(".uf2"))));
        let why_flash_only = busy.clone().or_else(|| flash_err.clone());
        let lines = |s: &Result<Vec<sch_core::firmware::Step>, String>| s.as_ref().map(|s| s.iter().map(|x| x.line()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        let mut start: Option<(&'static str, bool, bool)> = None;
        ui.horizontal(|ui| {
            if crate::widgets::unless(ui, why_build.as_deref(), egui::Button::new(i18n::tr("ビルド"))).on_hover_text(lines(&build)).clicked() {
                start = Some(("ファームのビルド", true, false));
            }
            let why_both = why_build.clone().or(why_flash_after.clone());
            if crate::widgets::unless(ui, why_both.as_deref(), egui::Button::new(i18n::tr("ビルドして書き込む"))).on_hover_text(i18n::tr("ビルドが通ったら、続けてボードに書き込みます")).clicked() {
                start = Some(("ファームのビルドと書き込み", true, true));
            }
            if crate::widgets::unless(ui, why_flash_only.as_deref(), egui::Button::new(i18n::tr("書き込みだけ"))).on_hover_text(lines(&flash)).clicked() {
                start = Some(("ファームの書き込み", false, true));
            }
        });
        if let Some((what, b, f)) = start {
            self.start_firmware(what, b, f);
        }
        // the log of the last run
        if let Some(r) = &self.fw_run {
            let log = r.run.lines.lock().unwrap().clone();
            let done = r.run.done.lock().unwrap().clone();
            match &done {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(i18n::trf!("{}… {:.0} 秒", i18n::tr(r.what), r.started.elapsed().as_secs_f64()));
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                }
                Some(Ok(())) => {
                    ui.colored_label(crate::theme::ok(), i18n::trf!("✔ {}: 終わりました", i18n::tr(r.what)));
                }
                Some(Err(e)) => {
                    ui.colored_label(crate::theme::error(), e);
                }
            }
            egui::ScrollArea::vertical().id_salt("fw_log").max_height(200.0).stick_to_bottom(true).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(log.join("\n")).monospace().small()).wrap());
            });
        }
    }

    /// Starts the build and / or the load on another thread. A Pico's load is worked out after its build (the .uf2 it makes).
    fn start_firmware(&mut self, what: &'static str, build: bool, flash: bool) {
        let base = self.firmware_base();
        let fw = self.sch.firmware.clone();
        let mut steps = vec![];
        if build {
            match build_steps(&fw, &base) {
                Ok(s) => steps.extend(s),
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
        }
        let pico_after = flash && build && Toolchain::parse(&fw.toolchain) == Some(Toolchain::Pico);
        if flash && !pico_after {
            match flash_steps(&fw, &base) {
                Ok(s) => steps.extend(s),
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
        }
        let run = if pico_after { sch_core::firmware::start_then(steps, move || flash_steps(&fw, &base)) } else { sch_core::firmware::start(steps) };
        self.fw_run = Some(FirmwareRun { run, started: std::time::Instant::now(), what });
    }
}
