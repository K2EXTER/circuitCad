use super::*;

impl PartForm {
    /// Fill the pin list, the model and the description from the charger template.
    pub(super) fn apply_template(&mut self) {
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

    pub(super) fn template_ui(&mut self, ui: &mut egui::Ui) {
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
}
