//! The browser version's window: pick a sample, see it with its DC voltages. On a narrow screen (a phone) the list folds into a bar
//! at the top; on a wide one it is a panel on the left.

use crate::samples::SAMPLES;
use crate::view::{sim_volts, Sheet};
use eframe::egui;
use i18n::tr;

/// Below this width (points) the screen is laid out for a phone.
const NARROW: f32 = 700.0;

/// The Japanese font of the app (BIZ UD Gothic, SIL OFL 1.1: `crates/app/assets/fonts/OFL.txt`, served next to the page) cut down
/// to the characters the browser version shows (`tools/web_font_subset.py`: 1.4 MB instead of 4.7): the browser has no system
/// fonts to read, so it travels inside the wasm file.
const FONT: &[u8] = include_bytes!("../assets/BIZUDGothic-web.ttf");

pub struct WebApp {
    sample: usize,
    sheet: Option<Sheet>,
    error: Option<String>,
    /// On a narrow screen: the list of samples is open (under the title bar, over the circuit).
    list_open: bool,
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
        let mut app = WebApp { sample: 0, sheet: None, error: None, list_open: false };
        app.open(0);
        app
    }

    fn open(&mut self, k: usize) {
        self.sample = k;
        match SAMPLES[k].schematic() {
            Ok(sch) => {
                self.sheet = Some(Sheet::new(sch, SAMPLES[k].live));
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
            // the digital circuits that run come after the ones solved for DC, under a heading of their own
            if s.live && !SAMPLES[k.saturating_sub(1)].live {
                ui.separator();
                ui.weak(tr("動かして見る (デジタル)"));
            }
            if ui.selectable_label(k == self.sample, tr(s.title)).on_hover_text(tr(s.about)).clicked() {
                pick = Some(k);
            }
        }
        if let Some(k) = pick {
            self.open(k);
            self.list_open = false;
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
                // a button that opens the list under the title bar (big rows for a finger), not a small combo box popup
                if narrow && ui.button(format!("{} ▼", tr(SAMPLES[self.sample].title))).on_hover_text(tr("サンプルを選びます")).clicked() {
                    self.list_open = !self.list_open;
                }
                if ui.button(tr("全体を表示")).on_hover_text(tr("回路図が画面に収まるように拡大・縮小します")).clicked()
                    && let Some(s) = &mut self.sheet
                {
                    s.refit();
                }
            });
            if let Some(ll) = self.sheet.as_mut().and_then(|s| s.live.as_mut()) {
                crate::live_ui::controls(ui, ll);
            }
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            } else if let Some(s) = &self.sheet {
                ui.label(tr(SAMPLES[self.sample].about));
                ui.small(&s.status);
                if let Some(ll) = &s.live {
                    crate::live_ui::findings(ui, ll);
                    if let Some(t) = &ll.truth {
                        // a phone keeps most of the screen for the circuit: the table scrolls
                        let most = ui.ctx().content_rect().height() * 0.3;
                        egui::ScrollArea::vertical().max_height(most).show(ui, |ui| crate::live_ui::truth_table(ui, t));
                    }
                }
            }
            self.part_info(ui);
        });
        if narrow && self.list_open {
            egui::Panel::top("sample_list").show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| self.sample_list(ui));
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character the browser version writes (its own text, the sample circuits it opens) is in the cut-down font or in egui's; when this
    /// fails, run `python tools/web_font_subset.py` again.
    #[test]
    fn the_cut_down_font_has_every_character_shown() {
        // the fonts egui carries come after ours: a symbol neither Japanese font has (⏸) is drawn from them
        let defaults = egui::FontDefinitions::default();
        let mut fonts: Vec<&[u8]> = vec![FONT];
        fonts.extend(defaults.font_data.values().map(|f| &f.font[..]));
        let faces: Vec<ttf_parser::Face> = fonts.iter().map(|f| ttf_parser::Face::parse(f, 0).unwrap()).collect();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut text = String::new();
        for f in ["app.rs", "live_ui.rs", "samples.rs", "view.rs", "main.rs"] {
            text += &std::fs::read_to_string(root.join("src").join(f)).unwrap();
        }
        for s in SAMPLES {
            text += &format!("{:?}", s.schematic().unwrap());
        }
        let missing: std::collections::BTreeSet<char> = text.chars().filter(|c| !c.is_control() && faces.iter().all(|f| f.glyph_index(*c).is_none())).collect();
        assert!(missing.is_empty(), "not in the font: {missing:?}");
    }
}
