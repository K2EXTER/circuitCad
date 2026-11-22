//! The prototype tab's left panel: the board kind, the check against the schematic, the tools, the parts and their holes.

use super::*;

impl BreadboardView {
    /// The left panel: what this is, the differences from the schematic, the parts and where their leads go.
    pub fn panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        ui.horizontal(|ui| {
            for (k, name, tip) in [
                (BoardKind::Breadboard, "ブレッドボード", "はんだ不要の差し込み式の板。試しに組んで、電源を入れて動かせます"),
                (BoardKind::Stripboard, "ストリップ基板", "銅の帯が並んだ穴あき基板。帯を切る所と、表を通す線の並べ方を出します"),
                (BoardKind::Perfboard, "ユニバーサル基板", "穴だけの基板。裏で穴をつなぐすずめっき線と、表を通す線の並べ方を出します"),
            ] {
                if ui.selectable_label(self.kind == k, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    // another board is another circuit: the power goes off
                    if self.kind != k {
                        self.powered = false;
                    }
                    self.kind = k;
                }
            }
        });
        ui.checkbox(&mut self.show_schematic, i18n::tr("回路図を小窓で見る")).on_hover_text(i18n::tr(
            "回路図を小さな窓に出します (動かせる・大きさを変えられる)。窓の中の部品をクリックすると、基板の上でその部品が光り、基板で選んだ部品は窓の中で囲まれます。差し方を回路図と見比べながら組めます",
        ));
        if self.kind != BoardKind::Breadboard {
            self.strip_panel(ui, sch);
            return;
        }
        let l = self.layout(sch);
        crate::widgets::section_heading(ui, i18n::tr("ブレッドボード"));
        ui.label(spaced("回路図の部品を、ブレッドボードのどの穴に差すかを並べました。縦に並んだ 5 つの穴 (a〜e、f〜j) は中でつながっていて、上下の赤と青の列は電源とGNDです。部品をドラッグすると差し替えられます。").color(NOTE));
        let problems = self.board_check(sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            ui.colored_label(BAD, i18n::trf!("回路図と違う所が {} 件あります", problems.len()));
            for p in &problems {
                ui.label(egui::RichText::new(i18n::trf!("・{}", i18n::tr_dyn(&p.message()))).color(BAD));
            }
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (t, name, tip) in [
                (Tool::Move, "動かす", "部品やジャンパー線の端をドラッグして差し替えます。選んだ部品は R で回り、選んだ線は Delete で消えます"),
                (Tool::Wire, "ジャンパー線", "穴をクリックして、つなぐ先の穴をクリックすると、線を差します。Esc でやめます"),
                (Tool::Meter, "テスター", "1 回目のクリックで赤い棒、2 回目で黒い棒を穴に当てます。レンジは 電圧 (並列に当てる)・電流 (線を外して直列に入れる)・抵抗 (電源を切って測る) から選びます"),
                (Tool::Scope, "オシロ", "探り針を穴に当てると、そのネットの電圧の時間の変化 (過渡解析) が左に出ます"),
            ] {
                if ui.selectable_label(self.tool == t, i18n::tr(name)).on_hover_text(i18n::tr(tip)).clicked() {
                    self.tool = t;
                    self.wire_start = None;
                    self.dc_asked = false;
                }
            }
        });
        let power_label = if self.powered { i18n::tr("電源を切る") } else { i18n::tr("電源を入れる") };
        if ui.button(power_label).on_hover_text(i18n::tr("電源を入れると、回路が時間とともに動き出します (過渡解析を実時間で進め続けます)。計算するのはブレッドボードに差したとおりの回路なので、回路図と違う差し方なら、そのとおりに動いたり壊れたりします。LED は流れる電流で光り、スイッチと押しボタンはクリックで切り替わります。定格を超えた部品は焦げた印と理由が出ます (実物なら壊れるところ)")).clicked() {
            self.powered = !self.powered;
            self.dc_asked = false;
        }
        if ui
            .button(i18n::tr("デジタル回路を回路図の上で動かす"))
            .on_hover_text(i18n::tr("回路図のタブに移って、デジタル回路を動かしながら見ます。配線が High / Low の色になり、真理値表・ロジックアナライザ・フリップフロップの記憶・7 セグメントの表示・グリッチの印が出ます (ブレッドボードの絵より、信号の流れが追いやすい形)"))
            .clicked()
        {
            self.want_live_logic = true;
        }
        self.sketch_ui(ui, sch);
        // the meter with the power off reads the schematic's DC point (what the board will read once it is switched on)
        if !self.powered && self.tool == Tool::Meter && sch.bench_voltages().is_none() && !self.dc_asked && problems.is_empty() {
            self.dc_asked = true;
            sch.analyze_dc();
        }
        self.live_frame(ui, sch, &RunBoard::Bread(l.clone()), problems.is_empty());
        match self.tool {
            Tool::Meter => self.meter_ui(ui, sch, &l, problems.is_empty()),
            Tool::Scope => self.scope_ui(ui, sch, &l),
            Tool::Wire => {
                ui.label(egui::RichText::new(i18n::tr("穴をクリックして、つなぐ先の穴をクリックします")).color(NOTE));
            }
            Tool::Move => {}
        }
        self.power_issues_ui(ui);
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("自動で並べ直す")).on_hover_text(i18n::tr("回路図から、部品の差し方を並べ直します (IC は中央の溝をまたぎ、電源とGNDは上下の列、同じネットの足は同じ縦の列)。手で動かした分は消えます")).clicked() {
                sch.breadboard = Some(auto_layout(&sch.sch));
            }
            if sch.breadboard.is_none() {
                ui.label(egui::RichText::new(i18n::tr("(自動で並べた形)")).color(NOTE)).on_hover_text(i18n::tr("まだ手で動かしていないので、回路図を変えると並べ直されます。部品を動かすと、その並べ方が回路図のファイルに入ります"));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(i18n::tr("部品と差す穴")).strong());
        // one part a row, its holes wrapped within the panel (a 30-pin board on one line widened the panel past the breadboard)
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (k, p) in l.parts.iter().enumerate() {
                let picked = self.picked.as_deref() == Some(p.refdes.as_str());
                let row = |ui: &mut egui::Ui, picked: bool| {
                    ui.horizontal_top(|ui| {
                        let clicked = ui.add_sized([44.0, 18.0], egui::Button::selectable(picked, egui::RichText::new(&p.refdes).strong())).on_hover_text(i18n::tr("選ぶと、ブレッドボードの上でこの部品が光ります")).clicked();
                        let text = p.leads.iter().map(|(pin, h)| format!("{pin}:{}", h.name())).collect::<Vec<_>>().join("  ");
                        ui.add(egui::Label::new(text).wrap());
                        clicked
                    })
                    .inner
                };
                let clicked = if k % 2 == 1 {
                    egui::Frame::NONE.fill(ui.visuals().faint_bg_color).show(ui, |ui| row(ui, picked)).inner
                } else {
                    row(ui, picked)
                };
                if clicked {
                    self.picked = if picked { None } else { Some(p.refdes.clone()) };
                }
            }
            if !l.off_board.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ブレッドボードの外 (電源・電池・信号源)")).strong()).on_hover_text(i18n::tr("実験用の電源・電池・発振器などは、ブレッドボードの外に置いて、線でつなぎます"));
                for o in &l.off_board {
                    ui.label(format!("{}  {}", o.refdes, o.pins.iter().map(|(pin, net, h)| format!("{pin} ({net}) → {}", h.name())).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// With the power on: the parts burnt or near their ratings (or why the circuit has no DC point).
    pub(super) fn power_issues_ui(&self, ui: &mut egui::Ui) {
        if !self.powered {
            return;
        }
        {
            let issues = self.issues();
            if let Some(Board { dc: Err(e), .. }) = &self.board {
                ui.colored_label(crate::theme::WARN, i18n::trf!("電源を入れたときの直流の様子が求まりませんでした: {}", i18n::tr_dyn(e)));
            } else if issues.is_empty() {
                ui.colored_label(crate::theme::OK, i18n::tr("電源を入れました。定格を超えた部品はありません"));
            } else {
                for r in &issues {
                    let (col, head) = if r.level == sim_core::RatingLevel::Over { (BAD, i18n::tr("壊れた")) } else { (crate::theme::WARN, i18n::tr("熱い・余裕が少ない")) };
                    ui.label(egui::RichText::new(format!("{} {}: {}", r.refdes, head, i18n::tr_dyn(&r.message))).color(col));
                    if let Some((_, f)) = self.broken.iter().find(|(b, _)| element_of(b, &r.refdes)) {
                        ui.label(egui::RichText::new(i18n::trf!("　{} は {} になり、回路からはそのように計算しています", r.refdes, i18n::tr_dyn(&f.label()))).color(BAD));
                    }
                    ui.label(egui::RichText::new(format!("　→ {}", i18n::tr_dyn(&r.hint))).color(NOTE));
                }
            }
        }
    }

    /// The parts past (or near) their ratings on the board's circuit with the power on: while it runs, the parts burnt so far and
    /// the ones near their ratings now; else at its DC point.
    pub(super) fn issues(&self) -> Vec<sim_core::RatingIssue> {
        if let Some(w) = self.watch.as_ref().filter(|_| self.powered) {
            return w.burnt.iter().chain(&w.hot).cloned().collect();
        }
        match &self.board {
            Some(Board { dc: Ok((_, r)), .. }) if self.powered => r.clone(),
            _ => vec![],
        }
    }
}
