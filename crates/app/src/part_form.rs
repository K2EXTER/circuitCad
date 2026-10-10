//! "Create part" window: pin table + package dimensions + simulation model -> `PartDef`.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use part_lib::templates::{ChargerTemplate, CurrentSetting, Topology};
use part_lib::{Issue, PartDef, PinDef};
use pcb_core::package::PackageSpec;
use sch_core::{PartRatings, PinSide, SimKind, SimModel, SpecRow};

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
    /// Which checklist of what to look up the form offers (index into `CHECKLISTS`).
    pub rt_kind: usize,
    /// The charger template section and what it said last.
    pub tpl: ChargerTemplate,
    pub tpl_message: String,
}

impl Default for PartForm {
    fn default() -> Self {
        Self {
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
            rt_kind: 0,
            tpl: ChargerTemplate::default(),
            tpl_message: String::new(),
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

    /// The data sheet information typed into the form (None when nothing was typed).
    fn parse_ratings(&self) -> Result<Option<PartRatings>, String> {
        let num = |label: &str, t: &str| -> Result<Option<f64>, String> {
            let t = t.trim();
            if t.is_empty() {
                return Ok(None);
            }
            sim_core::parse_value(t)
                .filter(|v| *v > 0.0)
                .map(Some)
                .ok_or_else(|| format!("{label}「{t}」を、0 より大きい数で入力してください (例: 40 / 1.5 / 500m)"))
        };
        let mut rows = vec![];
        for (n, line) in self.rt_rows.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 2 || parts[0].is_empty() {
                return Err(format!("データシートの情報 {}行目: 「項目 | 値 | 条件」の形式で入力してください (条件は省略できます。例: 順方向電圧 | 0.9V | IF = 1A)", n + 1));
            }
            if parts[1].is_empty() {
                continue; // an item of the checklist that has not been looked up yet
            }
            // from a ※ on, a condition is the checklist's note to the learner, not a data sheet figure
            let condition = parts.get(2).map(|c| c.split('※').next().unwrap_or("").trim().to_string()).unwrap_or_default();
            rows.push(SpecRow { name: parts[0].into(), value: parts[1].into(), condition });
        }
        let r = PartRatings { v_max: num("最大電圧", &self.rt_v)?, i_max: num("最大電流", &self.rt_i)?, p_max: num("最大電力", &self.rt_p)?, rows };
        Ok(Some(r).filter(|r| !r.is_empty()))
    }

    /// How much of the data sheet section is filled: (maximum ratings given 0..=3, rows with a value, rows still open).
    fn ratings_progress(&self) -> (usize, usize, usize) {
        let given = |t: &str| sim_core::parse_value(t.trim()).is_some_and(|v| v > 0.0) as usize;
        let max_given = given(&self.rt_v) + given(&self.rt_i) + given(&self.rt_p);
        let (mut filled, mut open) = (0, 0);
        for line in self.rt_rows.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() >= 2 && !parts[0].is_empty() {
                if parts[1].is_empty() { open += 1 } else { filled += 1 }
            }
        }
        (max_given, filled, open)
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
                limits: self.sim_limits.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
            }),
        };
        Ok(PartDef {
            name: self.name.trim().to_string(),
            category: self.category.trim().to_string(),
            description: self.description.clone(),
            datasheet: self.datasheet.clone(),
            prefix: self.prefix.trim().to_string(),
            value: Some(self.value.trim().to_string()).filter(|s| !s.is_empty()),
            pins,
            package,
            footprint_name: None,
            sim,
            ratings: self.parse_ratings()?,
        })
    }

    /// Fill the pin list, the model and the description from the charger template.
    fn apply_template(&mut self) {
        match self.tpl.generate(&self.name) {
            Ok(g) => {
                let side = |s: Option<PinSide>| match s {
                    Some(PinSide::Left) => "left",
                    Some(PinSide::Right) => "right",
                    Some(PinSide::Top) => "top",
                    _ => "bottom",
                };
                self.pins_text = g.pins.iter().map(|p| format!("{} {} {}", p.number, p.name, side(p.side))).collect::<Vec<_>>().join("\n");
                self.sim = SimChoice::Subckt;
                self.sim_name = g.sim_name;
                self.sim_text = g.sim_text;
                self.sim_pins = g.sim_pins.join(" ");
                self.description = g.description;
                self.sim_limits = g.limits.join("\n");
                if self.category.trim().is_empty() || self.category == "ユーザー" {
                    self.category = "電源管理".into();
                }
                self.tpl_message = format!("ピン {} 本とモデルを作りました。このあと、下の「パッケージ」をデータシートの寸法に合わせ、ピン番号がデータシートの番号になるよう直して保存します", g.pins.len());
            }
            Err(e) => self.tpl_message = e,
        }
    }

    fn template_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("テンプレートからピンとモデルを作る (リチウムイオン充電IC)").id_salt("tpl_charger").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("例から始める:");
                if ui.small_button("TP4056 風").on_hover_text("リニア式、電流 = 1200 ÷ R_PROG。まず形を見るのに向いています").clicked() {
                    self.tpl = ChargerTemplate::default();
                }
                if ui.small_button("IP2332 風").on_hover_text("降圧スイッチング式、電流 = 0.35 + 2.75 ÷ R_ISET[kΩ]、入力 4.3〜6V、温度入力あり").clicked() {
                    self.tpl = ChargerTemplate {
                        topology: Topology::Buck,
                        setting: CurrentSetting::Resistor,
                        current: 2.0,
                        k: 2750.0,
                        offset: 0.35,
                        precharge_fraction: 0.2,
                        vin_min: 4.3,
                        vin_max: 6.0,
                        dropout: 0.1,
                        ntc: Some((0.3, 2.73)),
                        ..Default::default()
                    };
                }
            });
            let t = &mut self.tpl;
            ui.horizontal(|ui| {
                ui.label("方式");
                ui.selectable_value(&mut t.topology, Topology::Linear, "リニア")
                    .on_hover_text("入力から電池へ、電流をそのまま通します。回路は簡単ですが、入力と電池の電圧差が熱になります (TP4056 など)");
                ui.selectable_value(&mut t.topology, Topology::Buck, "降圧スイッチング")
                    .on_hover_text("コイルを使って電圧を下げながら充電します。効率が良く、大きな電流向き。LX 端子に外付けのコイルをつなぎます (TP5100・IP2332 など)");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("セル数");
                ui.add(egui::DragValue::new(&mut t.cells).range(1..=8)).on_hover_text("電池を直列につなぐ数。満充電電圧 = セル数 × 1セルの電圧");
                ui.label("1セルの満充電電圧");
                ui.add(egui::DragValue::new(&mut t.cell_voltage).speed(0.01).range(2.0..=5.0).suffix(" V"))
                    .on_hover_text("リチウムイオンは 4.2V (高電圧タイプ 4.35V)、リン酸鉄リチウムは 3.6V");
                if ui.small_button("4.2V").on_hover_text("リチウムイオン電池の 1 セルの満充電電圧").clicked() {
                    t.cell_voltage = 4.2;
                }
                if ui.small_button("3.6V (リン酸鉄)").on_hover_text("リン酸鉄リチウム電池の 1 セルの満充電電圧").clicked() {
                    t.cell_voltage = 3.6;
                }
            });
            ui.horizontal(|ui| {
                ui.label("充電電流の決め方");
                ui.selectable_value(&mut t.setting, CurrentSetting::Fixed, "決まった値").on_hover_text("データシートに電流が1つだけ書かれている場合");
                ui.selectable_value(&mut t.setting, CurrentSetting::Resistor, "抵抗で設定 (PROG 端子)")
                    .on_hover_text("端子につなぐ抵抗で電流を決める場合。データシートの式 I = 定数 + k ÷ R に合わせます");
            });
            match t.setting {
                CurrentSetting::Fixed => {
                    ui.horizontal(|ui| {
                        ui.label("充電電流");
                        ui.add(egui::DragValue::new(&mut t.current).speed(0.01).range(0.0..=20.0).suffix(" A")).on_hover_text("充電電流 (A)。データシートに書かれた値を入れます");
                    });
                }
                CurrentSetting::Resistor => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("電流 I[A] =");
                        ui.add(egui::DragValue::new(&mut t.offset).speed(0.01).range(0.0..=20.0)).on_hover_text("式の定数の部分 (無ければ 0)。例: IP2332 は 0.35");
                        ui.label("+ k ÷ R[Ω]    k =");
                        ui.add(egui::DragValue::new(&mut t.k).speed(1.0).range(0.0..=1e6))
                            .on_hover_text("TP4056 は 1200 (1.2kΩ で 1A)。式が「2.75 ÷ R[kΩ]」なら、Ω に直して k = 2750");
                        ui.label("PROG 開放時の電流");
                        ui.add(egui::DragValue::new(&mut t.current).speed(0.01).range(0.0..=20.0).suffix(" A"))
                            .on_hover_text("PROG 端子に何もつながないときの電流。0 なら充電しません。IP2332 は 2A");
                    });
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("プリチャージ: 1セルがこの電圧未満");
                ui.add(egui::DragValue::new(&mut t.precharge_per_cell).speed(0.01).range(0.0..=5.0).suffix(" V"))
                    .on_hover_text("空に近い電池は、小さな電流でそっと充電します。0 ならプリチャージなし");
                ui.label("電流の割合");
                ui.add(egui::DragValue::new(&mut t.precharge_fraction).speed(0.01).range(0.0..=1.0)).on_hover_text("0.1 = 設定電流の 10%");
                ui.label("終了電流の割合");
                ui.add(egui::DragValue::new(&mut t.termination_fraction).speed(0.01).range(0.0..=0.9))
                    .on_hover_text("定電圧のあと、電流がこの割合まで減ったら満充電とみなします (0.1 = 10%)。状態表示の切替に使います");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("入力電圧");
                ui.add(egui::DragValue::new(&mut t.vin_min).speed(0.05).range(0.5..=60.0).prefix("下限 ").suffix(" V"))
                    .on_hover_text("これを下回ると動きません (低電圧ロックアウト)");
                ui.add(egui::DragValue::new(&mut t.vin_max).speed(0.05).range(0.0..=60.0).prefix("上限 ").suffix(" V"))
                    .on_hover_text("これを超えると止まります (過電圧保護)。0 なら上限なし");
                ui.add(egui::DragValue::new(&mut t.dropout).speed(0.01).range(0.0..=5.0).prefix("電池より ").suffix(" V 以上高い"))
                    .on_hover_text("入力が電池の電圧よりこれだけ高くないと充電できません");
            });
            ui.horizontal_wrapped(|ui| {
                let mut ntc = t.ntc.is_some();
                if ui
                    .checkbox(&mut ntc, "温度入力 (NTC 端子)")
                    .on_hover_text("電池の温度を見る端子。サーミスタ (NTC) をつなぎ、温度が低すぎる・高すぎるときは充電を止めます")
                    .changed()
                {
                    t.ntc = ntc.then_some((0.3, 2.73));
                }
                if let Some((lo, hi)) = &mut t.ntc {
                    ui.add(egui::DragValue::new(lo).speed(0.01).range(0.0..=5.0).prefix("正常な電圧 ").suffix(" V から")).on_hover_text("NTC 端子の電圧がこの値より低いと、温度が高すぎる (または低すぎる) として充電を止めます");
                    ui.add(egui::DragValue::new(hi).speed(0.01).range(0.0..=5.0).suffix(" V まで"))
                        .on_hover_text("NTC に 100µA を流したときの電圧がこの範囲なら充電します (電圧 = 100µA × NTC の抵抗)");
                }
                ui.checkbox(&mut t.status, "状態出力 (CHRG / STDBY)").on_hover_text("充電中は CHRG、満充電は STDBY が Low になる出力 (LED をつなぐ端子)");
            });
            ui.horizontal(|ui| {
                if ui
                    .button("ピンとモデルを作る")
                    .on_hover_text("上の数字から、ピンの一覧とシミュレーションモデルを作って、下の欄に書き込みます (すでにある内容は置き換わります)")
                    .clicked()
                {
                    self.apply_template();
                }
                if !self.tpl_message.is_empty() {
                    ui.small(&self.tpl_message);
                }
            });
        }).header_response.on_hover_text("データシートの数字を入れるだけで、充電ICの「外から見た動き」のモデルを作ります。市販のSPICEモデルが無いICに使えます。数字は、データシートの「電気的特性」の表から拾います");
    }

    /// Draws the form. Returns a finished definition when the user presses save.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<PartDef> {
        egui::Grid::new("part_basic").num_columns(2).show(ui, |ui| {
            ui.label("部品名");
            ui.text_edit_singleline(&mut self.name).on_hover_text("部品の名前 (例: LM358、2N3904)。部品の一覧や回路図の値に出ます");
            ui.end_row();
            ui.label("カテゴリ");
            ui.text_edit_singleline(&mut self.category).on_hover_text("一覧のグループ名 (例: オペアンプ、トランジスタ)");
            ui.end_row();
            ui.label("部品番号の頭文字");
            ui.text_edit_singleline(&mut self.prefix).on_hover_text("回路図で付く部品番号の頭文字。IC は U、トランジスタは Q、抵抗は R、ダイオードは D");
            ui.end_row();
            ui.label("値 (省略で部品名)");
            ui.text_edit_singleline(&mut self.value).on_hover_text("回路図に表示する値。空なら部品名が入ります");
            ui.end_row();
            ui.label("説明");
            ui.text_edit_singleline(&mut self.description).on_hover_text("一覧で部品にポインタを置いたときに出る説明");
            ui.end_row();
            ui.label("データシートURL");
            ui.text_edit_singleline(&mut self.datasheet).on_hover_text("データシート (メーカーの仕様書) の URL。あとで見返すためのメモです");
            ui.end_row();
        });

        self.template_ui(ui);

        ui.separator();
        ui.label("ピン (1行に「番号 名前 [left|right|top|bottom]」。向き省略で前半が左・後半が右。NC は未接続ピン扱い)");
        ui.add(egui::TextEdit::multiline(&mut self.pins_text).desired_rows(6).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace)).on_hover_text("ピンの定義を1行に1つ書きます。例: `1 IN+ left` (番号 名前 向き)。向きは left / right / top / bottom で、省略すると前半が左、後半が右になります。NC は未接続のピンです");

        ui.separator();
        ui.label("パッケージ (データシートの推奨ランドパターン寸法 mm。パッド数はピン数に合わせます)");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.pkg, PkgKind::Chip2, "2端子チップ").on_hover_text("抵抗・コンデンサ・ダイオードなど、足が2つのチップ部品");
            ui.selectable_value(&mut self.pkg, PkgKind::Dual, "デュアル(SOIC/DIP)").on_hover_text("足が左右の2列に並ぶ IC (SOIC、DIP など)");
            ui.selectable_value(&mut self.pkg, PkgKind::Quad, "クアッド(QFP)").on_hover_text("足が四方に並ぶ IC (QFP、QFN など)");
            ui.selectable_value(&mut self.pkg, PkgKind::Row, "1列(ヘッダ)").on_hover_text("足が1列に並ぶ部品 (ピンヘッダ、コネクタなど)");
        });
        let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
            ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=100.0).prefix(format!("{label} ")).suffix(" mm")).on_hover_text(pkg_tip(label));
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
                    ui.checkbox(&mut self.tht, "スルーホール(DIP)").on_hover_text("足を基板の穴に通す形 (DIP) にします。オフなら面実装 (表面にはんだ付け)");
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
        egui::CollapsingHeader::new("データシートの情報 (定格・最大定格)").id_salt("part_ratings").default_open(true).show(ui, |ui| {
            ui.label("部品を追加するときは、メーカーのデータシートを開いて調べます。見る表は 2 つです: 「絶対最大定格 (Absolute Maximum Ratings)」 = 超えると壊れる値、「電気的特性 (Electrical Characteristics)」 = 動作中の代表的な値。調べて入れておくと、回路のシミュレーション結果と比べて、超えそうなときに警告できます");
            ui.horizontal(|ui| {
                ui.label("部品の種類");
                egui::ComboBox::from_id_salt("rt_kind").selected_text(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].0).show_ui(ui, |ui| {
                    for (k, (name, _, _)) in CHECKLISTS.iter().enumerate() {
                        ui.selectable_value(&mut self.rt_kind, k, *name);
                    }
                }).response.on_hover_text("選ぶと、その種類で調べるとよい項目が出ます");
                if ui.button("調べる項目のチェックリストを入れる").on_hover_text("下の「そのほかの情報」に、調べる項目を並べます。データシートを見て、値を書き込んでください (すでに書いた行は、そのまま残ります)").clicked() {
                    self.rt_rows = merge_checklist(&self.rt_rows, self.rt_kind);
                }
            });
            ui.label(egui::RichText::new(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].1).weak());
            egui::Grid::new("part_ratings_grid").num_columns(2).show(ui, |ui| {
                ui.label("最大電圧 [V]").on_hover_text("部品にかけてよい最大の電圧。ダイオードなら逆耐圧 (VRRM)、トランジスタなら Vce の最大 (VCEO)、MOSFET なら Vds の最大");
                ui.add(egui::TextEdit::singleline(&mut self.rt_v).desired_width(80.0)).on_hover_text("例: 40 / 1000");
                ui.end_row();
                ui.label("最大電流 [A]").on_hover_text("流してよい最大の連続電流 (ダイオードなら平均順電流 IF、トランジスタなら Ic の最大)");
                ui.add(egui::TextEdit::singleline(&mut self.rt_i).desired_width(80.0)).on_hover_text("例: 1 / 200m");
                ui.end_row();
                ui.label("最大電力 [W]").on_hover_text("部品が熱にしてよい最大の電力 (許容損失 Ptot)。周囲温度が高いと下がります。データシートの条件を確認します");
                ui.add(egui::TextEdit::singleline(&mut self.rt_p).desired_width(80.0)).on_hover_text("例: 0.5 / 625m / 1");
                ui.end_row();
            });
            ui.label("そのほかの情報 (1行に1つ。「項目 | 値 | 条件」。値が空の行は、まだ調べていない項目として残ります)");
            ui.add(egui::TextEdit::multiline(&mut self.rt_rows).desired_rows(5).desired_width(f32::INFINITY).hint_text("順方向電圧 | 0.9V | IF = 1A"))
                .on_hover_text("定格・代表値を、データシートのまま書き写します。条件 (電流や温度) も一緒に書くと、あとで比べやすくなります。部品の一覧でポインタを置いたときに出ます");
            let (max_given, rows_given, rows_open) = self.ratings_progress();
            let summary = format!("入力済み: 最大定格 {max_given} / 3 項目、そのほかの情報 {rows_given} 行{}", if rows_open > 0 { format!(" (まだ調べていない項目 {rows_open} 行)") } else { String::new() });
            if max_given == 0 && rows_given == 0 {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの情報が空です。空のまま保存もできますが、定格のチェックと「部品選びのヒント」が使えません。データシートを開いて、調べてみましょう");
            } else {
                ui.label(summary);
            }
            if self.datasheet.trim().is_empty() && (max_given > 0 || rows_given > 0) {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの URL が空です。調べた元を残しておくと、あとで見返せます (上の「データシートURL」)");
            }
        });

        ui.separator();
        ui.label("シミュレーションモデル (部品の特性)");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.sim, SimChoice::None, "なし").on_hover_text("シミュレーションしない部品にします (基板づくりだけに使う部品)");
            ui.selectable_value(&mut self.sim, SimChoice::Model, ".model (ダイオード/トランジスタ等)").on_hover_text("ダイオードやトランジスタなどの 1 素子を `.model` 文で書いたモデル");
            ui.selectable_value(&mut self.sim, SimChoice::Subckt, ".subckt (IC マクロモデル)").on_hover_text("IC の中身を、複数の素子の組み合わせ (`.subckt`) で書いたモデル");
        });
        if self.sim != SimChoice::None {
            ui.horizontal(|ui| {
                if self.sim == SimChoice::Model {
                    ui.label("素子記号");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_element).desired_width(30.0)).on_hover_text("素子の種類の記号。ダイオード D、バイポーラトランジスタ Q、MOSFET M、スイッチ S");
                }
                ui.label("モデル名");
                ui.add(egui::TextEdit::singleline(&mut self.sim_name).desired_width(120.0)).on_hover_text("`.model` や `.subckt` に付けた名前。下の本文と同じ名前にします");
                ui.label("ノード順のピン番号");
                ui.add(egui::TextEdit::singleline(&mut self.sim_pins).desired_width(160.0).hint_text("例: A K")).on_hover_text("モデルの端子の順番に、部品のピン番号を並べます。例: ダイオードなら A K (アノード カソード)、トランジスタなら C B E の順");
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.sim_text)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(".model D1 D(IS=1n)  /  .subckt NAME 1 2 3 ... .ends"),
            );
            ui.label("このモデルの限界 (1行に1つ。実物との違いを書くと、部品を選んだときに表示されます)");
            ui.add(
                egui::TextEdit::multiline(&mut self.sim_limits)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("例: 温度による変化は扱いません。\n接合容量は扱いません。"),
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
        if ui.add_enabled(ok, egui::Button::new("保存してライブラリに追加")).on_hover_text("この部品をライブラリに保存して、すぐに回路図で使えるようにします。エラーが残っていると押せません").clicked() {
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

/// What to look up for each kind of part: (kind, which figures go in the three boxes, [(item, where to find it)]).
const CHECKLISTS: &[(&str, &str, &[(&str, &str)])] = &[
    (
        "ダイオード",
        "最大電圧 = 逆耐圧 (VRRM)、最大電流 = 平均順電流 (IF(AV))。どちらも「絶対最大定格」の表にあります",
        &[
            ("順方向電圧 Vf", "電気的特性。条件の電流 (IF) も一緒に書く"),
            ("逆回復時間 trr", "電気的特性。スイッチング電源や高い周波数で効く"),
            ("接合容量 Cj", "電気的特性。高い周波数で効く"),
            ("非繰り返しサージ電流 IFSM", "絶対最大定格。電源を入れた瞬間の電流に耐えるか"),
            ("逆方向漏れ電流 IR", "電気的特性。温度で増える"),
        ],
    ),
    (
        "ツェナーダイオード",
        "最大電力 = 許容損失 (Pd)。最大電流は、最大ツェナー電流 (IZM) を入れます",
        &[
            ("ツェナー電圧 Vz", "電気的特性。測定電流 (IZT) と許容差 (±5% など) も書く"),
            ("ツェナーインピーダンス Zzt", "電気的特性。小さいほど電圧が安定"),
            ("温度係数", "電気的特性。温度で電圧がどれだけ動くか"),
            ("最大ツェナー電流 IZM", "絶対最大定格 (または許容損失 ÷ Vz)"),
        ],
    ),
    (
        "トランジスタ (バイポーラ)",
        "最大電圧 = コレクタ・エミッタ間電圧 (VCEO)、最大電流 = コレクタ電流 (IC)、最大電力 = 許容損失 (Ptot)。すべて「絶対最大定格」にあります",
        &[
            ("電流増幅率 hFE", "電気的特性。範囲 (最小〜最大) と、条件の電流 (IC) を書く"),
            ("コレクタ・エミッタ飽和電圧 VCE(sat)", "電気的特性。スイッチに使うとき"),
            ("ベース・エミッタ間電圧 VBE", "電気的特性。約 0.7V"),
            ("トランジション周波数 fT", "電気的特性。増幅できる周波数の目安"),
        ],
    ),
    (
        "MOSFET",
        "最大電圧 = ドレイン・ソース間電圧 (VDS)、最大電流 = ドレイン電流 (ID)、最大電力 = 許容損失 (PD)。「絶対最大定格」にあります",
        &[
            ("しきい値電圧 VGS(th)", "電気的特性。ゲートをこれ以上にするとオンになり始める"),
            ("オン抵抗 RDS(on)", "電気的特性。ゲート電圧 (VGS) の条件に注意。スイッチの発熱を決める"),
            ("ゲート・ソース間の最大電圧 VGS", "絶対最大定格。超えるとゲートが壊れる"),
            ("ゲート電荷 Qg", "電気的特性。ゲートを駆動する電流とスイッチング速度に効く"),
        ],
    ),
    (
        "レギュレータ (三端子・LDO)",
        "最大電圧 = 入力電圧の最大、最大電流 = 出力電流の最大。「絶対最大定格」にあります",
        &[
            ("出力電圧 Vout", "電気的特性。許容差 (±4% など) も書く"),
            ("ドロップアウト電圧", "電気的特性。入力が出力よりこれ以上高くないと安定化しない"),
            ("ライン・レギュレーション", "電気的特性。入力が変わったときの出力の変化"),
            ("ロード・レギュレーション", "電気的特性。負荷が変わったときの出力の変化"),
            ("熱抵抗 θJA", "熱の表。電力 × 熱抵抗 = 温度上昇"),
            ("静止電流 (グラウンド電流)", "電気的特性。電池で使うときの消費"),
        ],
    ),
    (
        "オペアンプ",
        "最大電圧 = 電源電圧の最大 (単電源なら合計、両電源なら ± の 2 倍)。「絶対最大定格」にあります",
        &[
            ("利得帯域幅積 GBW", "電気的特性。ゲインを上げると帯域が狭くなる"),
            ("入力オフセット電圧 Vos", "電気的特性。直流のずれ"),
            ("入力バイアス電流 Ib", "電気的特性。大きな抵抗を使うと効く"),
            ("出力電圧の振れ幅", "電気的特性。電源までは届かない (レール・ツー・レールかどうか)"),
            ("スルーレート", "電気的特性。大きな信号で追いつける速さ"),
            ("動作する電源電圧の範囲", "推奨動作条件。単電源で使えるか"),
        ],
    ),
    (
        "そのほか",
        "部品に合わせて、「絶対最大定格」の電圧・電流・電力を入れます",
        &[
            ("動作温度範囲", "絶対最大定格 / 推奨動作条件"),
            ("電源電圧の範囲", "推奨動作条件"),
            ("消費電流", "電気的特性"),
        ],
    ),
];

/// Adds the checklist of `kind` to the rows already typed: an item whose name is already there is not added again.
fn merge_checklist(rows: &str, kind: usize) -> String {
    let (_, _, items) = CHECKLISTS[kind.min(CHECKLISTS.len() - 1)];
    let mut out: Vec<String> = rows.lines().map(String::from).collect();
    for (name, hint) in items {
        if !out.iter().any(|l| l.split('|').next().is_some_and(|n| n.trim() == *name)) {
            out.push(format!("{name} |  | ※{hint}"));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{add_subckt_instance, dc_op, Circuit, Element};

    struct Rig {
        c: Circuit,
    }

    /// The generated model with `vin` on VIN, a stiff cell source on BAT and `rprog` ohms on PROG
    /// (None = open); other pins get light loads. Returns the circuit.
    fn rig(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> Rig {
        let g = t.generate("TPL").unwrap();
        let mut c = Circuit::new();
        let ports: Vec<String> = g.pins.iter().map(|p| p.name.to_ascii_lowercase()).collect();
        let nodes: Vec<usize> = ports.iter().map(|p| if p == "gnd" { 0 } else { c.node(p) }).collect();
        add_subckt_instance(&mut c, "U1", &g.sim_name, &g.sim_text, &nodes).unwrap_or_else(|e| panic!("{e}"));
        let node = |c: &mut Circuit, n: &str| c.node(n);
        let (vin_n, bat_n) = (node(&mut c, "vin"), node(&mut c, "bat"));
        c.add(Element::vdc("Vin", vin_n, 0, vin));
        c.add(Element::vdc("Vbat", bat_n, 0, vbat));
        if ports.iter().any(|p| p == "lx") {
            let lx = node(&mut c, "lx");
            c.add(Element::Inductor { name: "L1".into(), a: lx, b: bat_n, l: 2.2e-6 });
        }
        if let (Some(r), true) = (rprog, ports.iter().any(|p| p == "prog")) {
            let prog = node(&mut c, "prog");
            c.add(Element::Resistor { name: "Rp".into(), a: prog, b: 0, r });
        }
        for (n, pull) in [("chrg", true), ("stdby", true), ("ntc", false)] {
            if ports.iter().any(|p| p == n) {
                let id = node(&mut c, n);
                if pull {
                    c.add(Element::Resistor { name: format!("Rpull_{n}"), a: vin_n, b: id, r: 1e3 });
                } else {
                    c.add(Element::Resistor { name: "Rntc_ext".into(), a: id, b: 0, r: 10e3 });
                }
            }
        }
        Rig { c }
    }

    fn charge(t: &ChargerTemplate, vin: f64, vbat: f64, rprog: Option<f64>) -> f64 {
        dc_op(&rig(t, vin, vbat, rprog).c).unwrap_or_else(|e| panic!("{e:?}")).element("Vbat").unwrap().current
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-3)
    }

    #[test]
    fn the_default_template_is_a_working_linear_charger() {
        let t = ChargerTemplate::default();
        // I = 1200 / R
        for (r, want) in [(1.2e3, 1.0), (2.4e3, 0.5), (4.8e3, 0.25)] {
            let i = charge(&t, 5.0, 3.7, Some(r));
            assert!(near(i, want, 0.04), "R {r}: {i}");
        }
        // pre-charge: a tenth below 2.9 V; taper near 4.2 V; nothing when full or without supply
        assert!(near(charge(&t, 5.0, 2.5, Some(2.4e3)), 0.05, 0.1));
        let mid = charge(&t, 5.0, 4.19, Some(2.4e3));
        assert!(mid < 0.45 && mid > 0.05, "{mid}");
        assert!(charge(&t, 5.0, 4.21, Some(2.4e3)).abs() < 0.02);
        assert!(charge(&t, 3.5, 3.0, Some(2.4e3)).abs() < 5e-3);
        // an open PROG pin gives the open-pin current (0 here)
        assert!(charge(&t, 5.0, 3.7, None).abs() < 5e-3);
    }

    #[test]
    fn the_ip2332_style_template_reproduces_the_bundled_part() {
        let t = ChargerTemplate {
            topology: Topology::Buck,
            current: 2.0,
            k: 2750.0,
            offset: 0.35,
            precharge_fraction: 0.2,
            vin_min: 4.3,
            vin_max: 6.0,
            dropout: 0.1,
            ntc: Some((0.3, 2.73)),
            ..Default::default()
        };
        // same table as the datasheet: 4.3k -> 1 A, 2.4k -> 1.5 A, open -> 2 A
        assert!(near(charge(&t, 5.0, 3.7, Some(4.3e3)), 0.99, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, Some(2.4e3)), 1.5, 0.05));
        assert!(near(charge(&t, 5.0, 3.7, None), 2.0, 0.05));
        assert!(charge(&t, 6.5, 3.7, Some(4.3e3)).abs() < 5e-3, "over-voltage");
        assert!(charge(&t, 4.0, 3.7, Some(4.3e3)).abs() < 5e-3, "under-voltage");
    }

    #[test]
    fn two_cells_float_at_8v4_and_pre_charge_at_5v8() {
        let t = ChargerTemplate { cells: 2, vin_min: 9.0, ..Default::default() };
        assert!(near(charge(&t, 12.0, 7.4, Some(2.4e3)), 0.5, 0.05));
        assert!(charge(&t, 12.0, 8.45, Some(2.4e3)).abs() < 0.02);
        assert!(near(charge(&t, 12.0, 5.0, Some(2.4e3)), 0.05, 0.1), "below 5.8 V");
    }

    #[test]
    fn a_fixed_current_part_has_no_prog_pin_and_status_lines_follow() {
        let t = ChargerTemplate { setting: CurrentSetting::Fixed, current: 0.8, ..Default::default() };
        assert!(near(charge(&t, 5.0, 3.7, None), 0.8, 0.04));
        let r = rig(&t, 5.0, 3.7, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("chrg").unwrap() < 1.0 && op.voltage("stdby").unwrap() > 4.0, "charging");
        let r = rig(&t, 5.0, 4.2, None);
        let op = dc_op(&r.c).unwrap();
        assert!(op.voltage("stdby").unwrap() < 1.0 && op.voltage("chrg").unwrap() > 4.0, "full");
    }

    #[test]
    fn applying_the_template_fills_the_form_and_the_result_passes_the_checks() {
        let mut f = PartForm { name: "MYCHG".into(), tpl: ChargerTemplate { status: false, ..Default::default() }, ..Default::default() };
        f.apply_template();
        assert!(f.tpl_message.contains("ピン 4 本"), "{}", f.tpl_message);
        assert!(f.pins_text.contains("2 GND bottom") && f.pins_text.contains("4 PROG left"));
        assert!(f.sim == SimChoice::Subckt);
        assert_eq!(f.sim_pins, "1 2 3 4");
        assert_eq!(f.category, "電源管理");
        // with a 4-pin dual package the definition has no errors
        f.pkg = PkgKind::Dual;
        let def = f.to_def().unwrap();
        assert!(!def.has_errors(), "{:?}", def.validate().iter().map(|i| &i.message).collect::<Vec<_>>());
        // the notes about what the model leaves out go into the saved part
        assert!(f.sim_limits.lines().count() >= 4, "{}", f.sim_limits);
        assert_eq!(def.sim.as_ref().unwrap().limits.len(), f.sim_limits.lines().count());
        assert!(def.sim.unwrap().limits.iter().any(|l| l.contains("リニア") || l.contains("LX")));
        // a missing name is explained
        let mut g = PartForm::default();
        g.apply_template();
        assert!(g.tpl_message.contains("部品名"), "{}", g.tpl_message);
    }
}

/// Hover text of a package dimension box of the part form (data sheet "recommended land pattern").
fn pkg_tip(label: &str) -> &'static str {
    match label {
        "ピッチ" => "隣り合う足の中心どうしの間隔 (mm)。例: 1.27、0.65、2.54",
        "パッド長" => "はんだ付けする銅の四角 (パッド) の長さ。データシートの推奨ランドパターンの値",
        "パッド幅" => "パッドの幅。足の幅より少し広くします",
        "列間隔" => "向かい合う足の列の中心どうしの間隔 (mm)。DIP の 0.3 インチ幅なら 7.62",
        "対向パッド間隔" => "向かい合うパッドの外側から外側まで (または中心から中心まで) の距離。データシートの推奨寸法を見ます",
        "パッド径" => "パッド (丸い銅) の直径",
        "穴径" => "スルーホールの穴の直径。足の太さ + 0.2〜0.3mm",
        _ => "データシートの推奨ランドパターンの寸法 (mm)",
    }
}

#[cfg(test)]
mod ratings_tests {
    use super::*;

    fn form() -> PartForm {
        let mut f = PartForm::default();
        f.name = "TEST".into();
        f.pins_text = "1 A\n2 K".into();
        f.pkg = PkgKind::Chip2;
        f
    }

    /// The checklist adds the items to look up; typing a value turns a line into a data sheet row, and the
    /// note to the learner (※...) is not saved as a condition.
    #[test]
    fn the_checklist_turns_into_rows_as_values_are_filled_in() {
        let mut f = form();
        f.rt_kind = 0; // diode
        f.rt_rows = merge_checklist(&f.rt_rows, 0);
        assert!(f.rt_rows.lines().count() >= 4 && f.rt_rows.contains("順方向電圧 Vf") && f.rt_rows.contains("※"));
        // nothing is looked up yet: the part has no ratings, and saving is still possible
        let d = f.to_def().unwrap();
        assert!(d.ratings.is_none());
        assert_eq!(f.ratings_progress(), (0, 0, f.rt_rows.lines().count()));
        // the learner fills in the forward voltage and the reverse voltage
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ※", "順方向電圧 Vf | 0.6V (最大) | IF = 1A ※");
        f.rt_rows = f.rt_rows.replace("順方向電圧 Vf |  | ", "順方向電圧 Vf | 0.6V (最大) | ");
        f.rt_v = "40".into();
        let r = f.to_def().unwrap().ratings.unwrap();
        assert_eq!(r.v_max, Some(40.0));
        let vf = r.rows.iter().find(|x| x.name == "順方向電圧 Vf").expect("the filled row");
        assert_eq!(vf.value, "0.6V (最大)");
        assert_eq!(vf.condition, "IF = 1A", "the note after ※ is dropped");
        let (max_given, filled, open) = f.ratings_progress();
        assert_eq!((max_given, filled), (1, 1));
        assert!(open >= 3);
        // asking for the checklist again does not repeat the items that are already there
        let again = merge_checklist(&f.rt_rows, 0);
        assert_eq!(again.lines().count(), f.rt_rows.lines().count());
        // every kind has a hint and items
        for (name, hint, items) in CHECKLISTS {
            assert!(!name.is_empty() && !hint.is_empty() && !items.is_empty(), "{name}");
        }
    }

    #[test]
    fn a_line_without_a_name_or_a_separator_is_explained() {
        let mut f = form();
        f.rt_rows = "順方向電圧 0.6V".into();
        assert!(f.to_def().unwrap_err().contains("項目 | 値 | 条件"));
        f.rt_rows = " | 0.6V".into();
        assert!(f.to_def().is_err());
        f.rt_rows = String::new();
        f.rt_v = "abc".into();
        assert!(f.to_def().unwrap_err().contains("最大電圧"));
    }
}
