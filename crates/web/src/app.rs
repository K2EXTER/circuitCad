//! The browser version's window: pick a sample, see it with its DC voltages. On a narrow screen (a phone) the list folds into a bar
//! at the top; on a wide one it is a panel on the left.

use crate::samples::SAMPLES;
use crate::view::{sim_volts, Sheet};
use eframe::egui;
use i18n::tr;

/// Below this width (points) the screen is laid out for a phone.
const NARROW: f32 = 700.0;

/// The Japanese font bundled with the app (BIZ UD Gothic, SIL OFL 1.1: `crates/app/assets/fonts/OFL.txt`); the browser has no
/// system fonts to read.
const FONT: &[u8] = include_bytes!("../../app/assets/fonts/BIZUDGothic-Regular.ttf");

pub struct WebApp {
    sample: usize,
    sheet: Option<Sheet>,
    error: Option<String>,
}

impl WebApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("jp".into(), std::sync::Arc::new(egui::FontData::from_static(FONT)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().insert(0, "jp".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = WebApp { sample: 0, sheet: None, error: None };
        app.open(0);
        app
    }

    fn open(&mut self, k: usize) {
        self.sample = k;
        match SAMPLES[k].schematic() {
            Ok(sch) => {
                self.sheet = Some(Sheet::new(sch));
                self.error = None;
            }
            Err(e) => {
                self.sheet = None;
                self.error = Some(e);
            }
        }
    }

    fn sample_list(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        for (k, s) in SAMPLES.iter().enumerate() {
            if ui.selectable_label(k == self.sample, tr(s.title)).on_hover_text(tr(s.about)).clicked() {
                pick = Some(k);
            }
        }
        if let Some(k) = pick {
            self.open(k);
        }
    }

    /// What the picked part is, and the voltage on each of its pins.
    fn part_info(&self, ui: &mut egui::Ui) {
        let Some(sheet) = &self.sheet else { return };
        let Some(c) = sheet.selected.and_then(|k| sheet.sch.components.get(k)) else {
            ui.weak(tr("部品をタップすると、その部品の説明が出ます"));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("{} {}", c.refdes, c.value));
            ui.label(c.kind.label());
            for (pin, at) in c.pins() {
                if let Some(v) = sheet.pin_voltage(at) {
                    ui.label(format!("{} {pin}: {}", tr("ピン"), sim_volts(v)));
                }
            }
        });
    }
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let narrow = ui.ctx().content_rect().width() < NARROW;
        egui::Panel::top("title").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("circuitCad");
                ui.weak(tr("ブラウザ版 (試作)"));
                if narrow {
                    egui::ComboBox::from_id_salt("sample").selected_text(tr(SAMPLES[self.sample].title)).show_ui(ui, |ui| self.sample_list(ui));
                }
                if ui.button(tr("全体を表示")).on_hover_text(tr("回路図が画面に収まるように拡大・縮小します")).clicked()
                    && let Some(s) = &mut self.sheet
                {
                    s.refit();
                }
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            } else if let Some(s) = &self.sheet {
                ui.label(tr(SAMPLES[self.sample].about));
                ui.small(&s.status);
            }
            self.part_info(ui);
        });
        if !narrow {
            egui::Panel::left("samples").resizable(false).show(ui, |ui| {
                ui.strong(tr("サンプル"));
                ui.separator();
                self.sample_list(ui);
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            if let Some(s) = &mut self.sheet {
                s.show(ui);
            }
        });
    }
}
