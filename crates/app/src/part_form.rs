//! "Create part" window: pin table + package dimensions + simulation model -> `PartDef`.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use part_lib::{Issue, PartDef, PinDef};
use pcb_core::package::PackageSpec;
use sch_core::{PinSide, SimKind, SimModel};

#[derive(Clone, Copy, PartialEq)]
pub enum PkgKind {
    Chip2,
    Dual,
    Quad,
    Row,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SimChoice {
    None,
    Model,
    Subckt,
}

pub struct PartForm {
    pub name: String,
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
    pub sim: SimChoice,
    pub sim_element: String,
    pub sim_name: String,
    pub sim_text: String,
    pub sim_pins: String,
}

impl Default for PartForm {
    fn default() -> Self {
        Self {
            name: String::new(),
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
            sim: SimChoice::None,
            sim_element: "D".into(),
            sim_name: String::new(),
            sim_text: String::new(),
            sim_pins: String::new(),
        }
    }
}

fn parse_side(t: &str) -> Option<PinSide> {
    match t.to_ascii_lowercase().as_str() {
        "left" | "l" | "左" => Some(PinSide::Left),
        "right" | "r" | "右" => Some(PinSide::Right),
        "top" | "t" | "上" => Some(PinSide::Top),
        "bottom" | "b" | "下" => Some(PinSide::Bottom),
        _ => None,
    }
}

impl PartForm {
    fn parse_pins(&self) -> Result<Vec<PinDef>, String> {
        let mut out = vec![];
        for (n, line) in self.pins_text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let t: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
            if t.len() < 2 {
                return Err(format!("{}行目: 「番号 名前」の形式で入力してください", n + 1));
            }
            let side = match t.get(2) {
                None => None,
                Some(s) => Some(parse_side(s).ok_or_else(|| format!("{}行目: 向き「{s}」は left/right/top/bottom のいずれかです", n + 1))?),
            };
            out.push(PinDef { number: t[0].into(), name: t[1].into(), side });
        }
        Ok(out)
    }

    pub fn to_def(&self) -> Result<PartDef, String> {
        let pins = self.parse_pins()?;
        let n = pins.len();
        let package = match self.pkg {
            PkgKind::Chip2 => PackageSpec::Chip2 { pitch: self.pitch, pad_len: self.len, pad_width: self.width },
            PkgKind::Dual => PackageSpec::Dual {
                pins: n,
                pitch: self.pitch,
                row_spacing: self.row,
                pad_len: self.len,
                pad_width: self.width,
                drill: self.tht.then_some(self.drill),
            },
            PkgKind::Quad => PackageSpec::Quad { pins: n, pitch: self.pitch, span: self.span, pad_len: self.len, pad_width: self.width },
            PkgKind::Row => PackageSpec::SingleRow { pins: n, pitch: self.pitch, pad_d: self.pad_d, drill: self.drill },
        };
        let sim = match self.sim {
            SimChoice::None => None,
            k => Some(SimModel {
                kind: if k == SimChoice::Model { SimKind::Model } else { SimKind::Subckt },
                element: self.sim_element.trim().to_string(),
                name: self.sim_name.trim().to_string(),
                text: self.sim_text.clone(),
                pins: self.sim_pins.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect(),
                params: Default::default(),
            }),
        };
        Ok(PartDef {
            name: self.name.trim().to_string(),
            description: self.description.clone(),
            datasheet: self.datasheet.clone(),
            prefix: self.prefix.trim().to_string(),
            value: Some(self.value.trim().to_string()).filter(|s| !s.is_empty()),
            pins,
            package,
            footprint_name: None,
            sim,
        })
    }

    /// Draws the form. Returns a finished definition when the user presses save.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PartDef> {
        egui::Grid::new("part_basic").num_columns(2).show(ui, |ui| {
            ui.label("部品名");
            ui.text_edit_singleline(&mut self.name);
            ui.end_row();
            ui.label("部品番号の頭文字");
            ui.text_edit_singleline(&mut self.prefix);
            ui.end_row();
            ui.label("値 (省略で部品名)");
            ui.text_edit_singleline(&mut self.value);
            ui.end_row();
            ui.label("説明");
            ui.text_edit_singleline(&mut self.description);
            ui.end_row();
            ui.label("データシートURL");
            ui.text_edit_singleline(&mut self.datasheet);
            ui.end_row();
        });

        ui.separator();
        ui.label("ピン (1行に「番号 名前 [left|right|top|bottom]」。向き省略で前半が左・後半が右。NC は未接続ピン扱い)");
        ui.add(egui::TextEdit::multiline(&mut self.pins_text).desired_rows(6).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));

        ui.separator();
        ui.label("パッケージ (データシートの推奨ランドパターン寸法 mm。パッド数はピン数に合わせます)");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.pkg, PkgKind::Chip2, "2端子チップ");
            ui.selectable_value(&mut self.pkg, PkgKind::Dual, "デュアル(SOIC/DIP)");
            ui.selectable_value(&mut self.pkg, PkgKind::Quad, "クアッド(QFP)");
            ui.selectable_value(&mut self.pkg, PkgKind::Row, "1列(ヘッダ)");
        });
        let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
            ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=100.0).prefix(format!("{label} ")).suffix(" mm"));
        };
        ui.horizontal_wrapped(|ui| {
            dv(ui, "ピッチ", &mut self.pitch);
            match self.pkg {
                PkgKind::Chip2 => {
                    dv(ui, "パッド長", &mut self.len);
                    dv(ui, "パッド幅", &mut self.width);
                }
                PkgKind::Dual => {
                    dv(ui, "列間隔", &mut self.row);
                    dv(ui, "パッド長", &mut self.len);
                    dv(ui, "パッド幅", &mut self.width);
                    ui.checkbox(&mut self.tht, "スルーホール(DIP)");
                    if self.tht {
                        dv(ui, "穴径", &mut self.drill);
                    }
                }
                PkgKind::Quad => {
                    dv(ui, "対向パッド間隔", &mut self.span);
                    dv(ui, "パッド長", &mut self.len);
                    dv(ui, "パッド幅", &mut self.width);
                }
                PkgKind::Row => {
                    dv(ui, "パッド径", &mut self.pad_d);
                    dv(ui, "穴径", &mut self.drill);
                }
            }
        });
        ui.small("特殊な形状(サーマルパッド等)は、保存した JSON の package を explicit にして直接編集できます");

        ui.separator();
        ui.label("シミュレーションモデル (部品の特性)");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.sim, SimChoice::None, "なし");
            ui.selectable_value(&mut self.sim, SimChoice::Model, ".model (ダイオード/トランジスタ等)");
            ui.selectable_value(&mut self.sim, SimChoice::Subckt, ".subckt (IC マクロモデル)");
        });
        if self.sim != SimChoice::None {
            ui.horizontal(|ui| {
                if self.sim == SimChoice::Model {
                    ui.label("素子記号");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_element).desired_width(30.0));
                }
                ui.label("モデル名");
                ui.add(egui::TextEdit::singleline(&mut self.sim_name).desired_width(120.0));
                ui.label("ノード順のピン番号");
                ui.add(egui::TextEdit::singleline(&mut self.sim_pins).desired_width(160.0).hint_text("例: A K"));
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.sim_text)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(".model D1 D(IS=1n)  /  .subckt NAME 1 2 3 ... .ends"),
            );
        }

        ui.separator();
        let def = match self.to_def() {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                return None;
            }
        };
        let issues: Vec<Issue> = def.validate();
        for i in &issues {
            let (c, mark) = if i.error { (Color32::from_rgb(230, 60, 60), "エラー") } else { (Color32::from_rgb(220, 160, 40), "注意") };
            ui.colored_label(c, format!("{mark}: {}", i.message));
        }
        if let Ok(fp) = def.footprint() {
            footprint_preview(ui, &fp);
        }
        let ok = !def.has_errors();
        if ui.add_enabled(ok, egui::Button::new("保存してライブラリに追加")).clicked() {
            return Some(def);
        }
        None
    }
}

fn footprint_preview(ui: &mut egui::Ui, fp: &pcb_core::footprints::Footprint) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(420.0), 170.0), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_rgb(18, 22, 18));
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        x0 = x0.min(p.pos.x - w / 2.0);
        x1 = x1.max(p.pos.x + w / 2.0);
        y0 = y0.min(p.pos.y - h / 2.0);
        y1 = y1.max(p.pos.y + h / 2.0);
    }
    let scale = ((r.width() as f64 - 20.0) / (x1 - x0).max(0.1)).min((r.height() as f64 - 20.0) / (y1 - y0).max(0.1)) as f32;
    let c = r.center();
    let to = |x: f64, y: f64| Pos2::new(c.x + ((x - (x0 + x1) / 2.0) as f32) * scale, c.y - ((y - (y0 + y1) / 2.0) as f32) * scale);
    for p in &fp.pads {
        let (w, h) = p.shape.size();
        let ctr = to(p.pos.x, p.pos.y);
        let size = Vec2::new(w as f32 * scale, h as f32 * scale);
        let col = if p.pin == "1" { Color32::from_rgb(240, 120, 60) } else { Color32::from_rgb(220, 60, 60) };
        match p.shape {
            pcb_core::PadShape::Circle { .. } => {
                painter.circle_filled(ctr, size.x / 2.0, col);
            }
            _ => {
                painter.rect_filled(Rect::from_center_size(ctr, size), 0.0, col);
            }
        }
        if let Some(d) = p.drill {
            painter.circle_filled(ctr, d as f32 * scale / 2.0, Color32::from_rgb(18, 22, 18));
        }
        painter.text(ctr, Align2::CENTER_CENTER, &p.pin, FontId::proportional(11.0), Color32::WHITE);
    }
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
}
