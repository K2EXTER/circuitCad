//! "Create part" window: pin table + package dimensions + simulation model -> `PartDef`.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use part_lib::passive::{self, PassiveKind, PassiveTemplate};
use part_lib::templates::{ChargerTemplate, CurrentSetting, Topology};
use part_lib::{Issue, PartDef, PinDef};
use pcb_core::footprints::Footprint;
use pcb_core::body::{BodyMark, BodySpec};
use pcb_core::package::{HoleSpec, PackageSpec, PadSpec};
use pcb_core::PadShape;
use sch_core::{LifeRating, PartRatings, PinSide, RippleRating, SimKind, SimModel, SpecRow};

#[derive(Clone, Copy, PartialEq)]
pub enum PkgKind {
    Chip2,
    Dual,
    Quad,
    Row,
}

/// The tabs of the form.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FormTab {
    Basic,
    Pins,
    Package,
    Sim,
    Ratings,
}

const FORM_TABS: &[(FormTab, &str, &str)] = &[
    (FormTab::Basic, "① 基本", "部品の名前・番号の頭文字・値・説明。チップ部品や充電ICは、ここの「かんたん作成」で数字を入れるだけで作れます"),
    (FormTab::Pins, "② ピン", "ピンの番号と名前、記号のどの辺に出すか。データシートの「ピン配置」の表を見ながら入れます"),
    (FormTab::Package, "③ 基板の形", "データシートの推奨ランドパターン (パッド) と外形寸法 (本体の大きさ・高さ) を入れます。右の絵で確かめられます"),
    (FormTab::Sim, "④ シミュレーション", "SPICE モデルを貼ると、種類・端子を自動で入力します。シミュレーションしない部品 (基板づくりだけ) は「なし」のままです"),
    (FormTab::Ratings, "⑤ 定格", "データシートの絶対最大定格 (電圧・電流・電力) と電気的特性。入れておくと、回路で超えそうなときに警告します"),
];

/// What kind of part: the first question of the form.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PartType {
    Ic,
    Transistor,
    Diode,
    Capacitor,
    Resistor,
    Inductor,
    Connector,
}

const PART_TYPES: &[(PartType, &str, &str)] = &[
    (PartType::Ic, "IC", "オペアンプ・レギュレータ・マイコンなど。足が2列に並ぶ (SOIC・DIP) 形を例に、ピン・パッケージ・本体の外形を入れます。QFN や放熱パッドは「③ 基板の形」で変えます"),
    (PartType::Transistor, "トランジスタ", "NPN・PNP・MOSFET。足が1列の形 (TO-92 など) を例に入れます。記号は規格の形になります"),
    (PartType::Diode, "ダイオード", "整流・ショットキー・ツェナー・LED。足が2本の形 (DO-35 など) を例に入れます。カソードの棒も付きます"),
    (PartType::Capacitor, "コンデンサ", "積層セラミック・電解・タンタル・フィルム。種類を選ぶと、基板の形 (リード付きかチップか)・極性・高さの例が入ります。「① 基本」の「かんたん作成」に、容量と ESR などの数字を入れてモデルを作ります"),
    (PartType::Resistor, "抵抗", "チップ抵抗 (0603 など) かリード付き。「① 基本」の「かんたん作成」に、抵抗値を入れて作ります"),
    (PartType::Inductor, "コイル", "チップインダクタかリード付き。「① 基本」の「かんたん作成」に、インダクタンスと DCR を入れて作ります"),
    (PartType::Connector, "コネクタ", "ピンヘッダ・端子台など、足が1列に並ぶ部品"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SimChoice {
    None,
    Model,
    Subckt,
}

pub struct PartForm {
    /// The tab shown and the kind of part chosen first (None = not chosen: the form is as before).
    pub tab: FormTab,
    pub ptype: Option<PartType>,
    /// The sub-kind chosen under the kind ("ceramic", "electrolytic", "tantalum", "film", "chip", "leaded").
    pub ptype_sub: &'static str,
    /// A polarised capacitor (electrolytic, tantalum): the symbol shows the + and the pins are named + and -.
    pub pas_polar: bool,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub value: String,
    pub description: String,
    pub datasheet: String,
    pub pins_text: String,
    pub pkg: PkgKind,
    pub pitch: f64,
    pub row: f64,
    pub span: f64,
    pub len: f64,
    pub width: f64,
    pub pad_d: f64,
    pub drill: f64,
    pub tht: bool,
    /// The exposed pad under an IC (QFN, ESOP, SOT-223 tab): its pin, width and length. The pin must be in the pin table.
    pub thermal_on: bool,
    pub thermal_pin: String,
    pub thermal_w: f64,
    pub thermal_h: f64,
    /// The body drawn on the silkscreen: a rectangle (`body_w` x `body_h`) or a circle (`body_w` across), centred `body_x`, `body_y` from the origin.
    pub body_on: bool,
    pub body_circle: bool,
    pub body_w: f64,
    pub body_h: f64,
    pub body_x: f64,
    pub body_y: f64,
    pub body_mark: BodyMark,
    /// The pin the orientation mark belongs to (empty = the first pad).
    pub body_pin: String,
    /// Body height above the board in mm (0 = not given: the 3D view guesses from the footprint).
    pub height: f64,
    /// Extra pads and holes beyond the package: shield legs, locating pegs, slots. One per line, see `EXTRAS_HELP`.
    pub extras: String,
    /// A built-in footprint used as the package (empty = the dimensions typed below).
    pub fp_preset: String,
    /// Symbol drawn for a chip part (see `PartDef.glyph`); set by the chip part section.
    pub glyph: String,
    /// The chip part section: kind, value and what the data sheet says, as typed.
    pub pas_kind: PassiveKind,
    pub pas_value: String,
    pub pas_esr: String,
    /// The impedance at 100 kHz from the data sheet's table; for an electrolytic it is about the ESR.
    pub pas_z100k: String,
    pub pas_esl: String,
    pub pas_dcr: String,
    pub pas_cp: String,
    pub pas_srf: String,
    pub pas_bias: String,
    pub pas_table: String,
    /// Temperature coefficients typed in ppm (/°C and /°C²) and the table they can be fitted from ("°C percent" per line).
    pub pas_tc1: String,
    pub pas_tc2: String,
    pub pas_tctable: String,
    pub pas_message: Option<Result<String, String>>,
    /// The diode section: points read off the forward characteristics ("current voltage" per line), the reverse
    /// ratings, and the fitted (or slider-set) IS / N / RS.
    pub dio_points: String,
    pub dio_vrrm: String,
    pub dio_ir: String,
    pub dio_is: f64,
    pub dio_n: f64,
    pub dio_rs: f64,
    pub dio_message: Option<Result<String, String>>,
    pub sim: SimChoice,
    pub sim_element: String,
    pub sim_name: String,
    pub sim_text: String,
    pub sim_pins: String,
    /// What the model leaves out, one note per line.
    pub sim_limits: String,
    /// Data sheet ratings: absolute maximum voltage [V] / current [A] / power [W] (blank = not given) and
    /// further rows, one per line as `name | value | condition`.
    pub rt_v: String,
    pub rt_i: String,
    pub rt_p: String,
    pub rt_rows: String,
    /// An electrolytic capacitor's rated ripple current [A rms] and the frequency it is given at, the data sheet's
    /// multiplier table (one `frequency multiplier` per line), and its endurance (hours at a temperature).
    pub rt_ripple: String,
    pub rt_ripple_f: String,
    pub rt_ripple_table: String,
    pub rt_life_h: String,
    pub rt_life_t: String,
    /// Which checklist of what to look up the form offers (index into `CHECKLISTS`).
    pub rt_kind: usize,
    /// The charger template section and what it said last.
    pub tpl: ChargerTemplate,
    pub tpl_message: String,
    /// What the last "fill from SPICE" did or why it could not.
    pub spice_message: Option<Result<String, String>>,
    /// Result of the last trial read of the model (shown under the model box).
    pub trial_message: Option<trial::Trial>,
}

impl Default for PartForm {
    fn default() -> Self {
        Self {
            tab: FormTab::Basic,
            ptype: None,
            ptype_sub: "",
            pas_polar: false,
            name: String::new(),
            category: "ユーザー".into(),
            prefix: "U".into(),
            value: String::new(),
            description: String::new(),
            datasheet: String::new(),
            pins_text: "1 IN\n2 GND\n3 OUT\n4 VCC".into(),
            pkg: PkgKind::Dual,
            pitch: 1.27,
            row: 5.4,
            span: 8.4,
            len: 1.55,
            width: 0.6,
            pad_d: 1.7,
            drill: 1.0,
            tht: false,
            thermal_on: false,
            thermal_pin: String::new(),
            thermal_w: 3.0,
            thermal_h: 3.0,
            body_on: false,
            body_circle: false,
            body_w: 3.9,
            body_h: 4.9,
            body_x: 0.0,
            body_y: 0.0,
            body_mark: BodyMark::Dot,
            body_pin: String::new(),
            height: 0.0,
            extras: String::new(),
            fp_preset: String::new(),
            glyph: String::new(),
            pas_kind: PassiveKind::Capacitor,
            pas_value: String::new(),
            pas_esr: String::new(),
            pas_z100k: String::new(),
            pas_esl: String::new(),
            pas_dcr: String::new(),
            pas_cp: String::new(),
            pas_srf: String::new(),
            pas_bias: String::new(),
            pas_table: String::new(),
            pas_tc1: String::new(),
            pas_tc2: String::new(),
            pas_tctable: String::new(),
            pas_message: None,
            dio_points: String::new(),
            dio_vrrm: String::new(),
            dio_ir: String::new(),
            dio_is: 1e-10,
            dio_n: 1.5,
            dio_rs: 0.05,
            dio_message: None,
            sim: SimChoice::None,
            sim_element: "D".into(),
            sim_name: String::new(),
            sim_text: String::new(),
            sim_pins: String::new(),
            sim_limits: String::new(),
            rt_v: String::new(),
            rt_i: String::new(),
            rt_p: String::new(),
            rt_rows: String::new(),
            rt_ripple: String::new(),
            rt_ripple_f: "100k".into(),
            rt_ripple_table: String::new(),
            rt_life_h: String::new(),
            rt_life_t: "105".into(),
            rt_kind: 0,
            tpl: ChargerTemplate::default(),
            tpl_message: String::new(),
            spice_message: None,
            trial_message: None,
        }
    }
}

mod checklist;
mod kind;
mod parse;
mod chip;
mod diode;
mod zchart;
mod preview;
mod tabs;
mod template;
mod trial;
#[cfg(test)]
mod tests;

use checklist::*;
use parse::*;

impl PartForm {
    /// Draws the form. Returns a finished definition when the user presses save.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PartDef> {
        let built = self.to_def();
        self.kind_bar(ui);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (tab, label, tip) in FORM_TABS {
                let warn = match tab {
                    FormTab::Basic => self.name.trim().is_empty(),
                    FormTab::Pins => self.parse_pins().is_err(),
                    FormTab::Package => built.is_err() || built.as_ref().is_ok_and(|d| d.footprint().is_err()),
                    _ => false,
                };
                let text = if warn { format!("⚠ {label}") } else { (*label).to_string() };
                ui.selectable_value(&mut self.tab, *tab, egui::RichText::new(text).size(15.0)).on_hover_text(*tip);
            }
        });
        ui.separator();
        let h = 470.0;
        let mut saved = None;
        ui.horizontal_top(|ui| {
            let right_w = 370.0;
            // the window may be dragged wider; the form keeps to a width where nothing stretches across the screen
            let total = ui.available_width().clamp(760.0, 1040.0);
            let left_w = total - right_w - 20.0;
            ui.allocate_ui_with_layout(Vec2::new(left_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(left_w);
                egui::ScrollArea::vertical().id_salt("part_form_tab").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    match self.tab {
                        FormTab::Basic => self.tab_basic(ui),
                        FormTab::Pins => self.tab_pins(ui),
                        FormTab::Package => self.tab_package(ui),
                        FormTab::Sim => self.tab_sim(ui),
                        FormTab::Ratings => self.tab_ratings(ui),
                    }
                });
            });
            ui.separator();
            ui.allocate_ui_with_layout(Vec2::new(right_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(right_w);
                saved = self.preview_pane(ui, &built);
            });
        });
        saved
    }
}
