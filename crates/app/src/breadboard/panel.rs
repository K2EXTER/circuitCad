//! The prototype tab's left panel: the board kind, the check against the schematic, the tools, the parts and their holes.

use super::*;

/// The width of the power's bar (切 | 入), beside the other buttons of its row.
const POWER_BAR_WIDTH: f32 = 140.0;

impl BreadboardView {
    /// The left panel: the board kind on top, then (scrolled as one) the check against the schematic, the tools, the actions and
    /// the lists. The three kinds share this order (`strip_panel` for the stripboard and the perfboard).
    pub fn panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        self.kind_ui(ui);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.kind == BoardKind::Breadboard {
                self.bread_panel(ui, sch);
            } else {
                self.strip_panel(ui, sch);
            }
        });
    }

    /// The board kind as one bar, and the schematic's small window; what the chosen board is sits in the "(?)".
    fn kind_ui(&mut self, ui: &mut egui::Ui) {
        const KINDS: [(BoardKind, &str, &str); 3] = [
            (BoardKind::Breadboard, "ブレッドボード", "はんだ不要の差し込み式の板。試しに組んで、電源を入れて動かせます"),
            (BoardKind::Stripboard, "ストリップ基板", "銅の帯が並んだ穴あき基板。帯を切る所と、表を通す線の並べ方を出します"),
            (BoardKind::Perfboard, "ユニバーサル基板", "穴だけの基板。裏で穴をつなぐすずめっき線と、表を通す線の並べ方を出します"),
        ];
        let items: Vec<(&str, Option<&str>)> = KINDS.iter().map(|(_, name, _)| (*name, None)).collect();
        let now = KINDS.iter().position(|(k, ..)| *k == self.kind);
        for (r, (k, _, tip)) in crate::widgets::segment_bar(ui, &items, now).into_iter().zip(KINDS) {
            if r.on_hover_text(i18n::tr(tip)).clicked() && self.kind != k {
                // another board is another circuit: the power goes off
                self.powered = false;
                self.kind = k;
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_schematic, i18n::tr("回路図を小窓で見る")).on_hover_text(i18n::tr(
                "回路図を小さな窓に出します (動かせる・大きさを変えられる)。窓の中の部品をクリックすると、基板の上でその部品が光り、基板で選んだ部品は窓の中で囲まれます。差し方を回路図と見比べながら組めます",
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::icons::help(
                    ui,
                    match self.kind {
                        BoardKind::Breadboard => "回路図の部品を、ブレッドボードのどの穴に差すかを並べました。縦に並んだ 5 つの穴 (a〜e、f〜j) は中でつながっていて、上下の赤と青の列は電源とGNDです。部品をドラッグすると差し替えられます。",
                        BoardKind::Stripboard => "銅の帯が横に並んだ基板 (ストリップ基板) に組む並べ方です。IC の段の上と下に帯の組があり、1 本ずつ 1 つのネットです (左に名前)。2 本足の部品は、つなぐ 2 つの帯をまたいで縦に差します。IC は × の穴で帯を切って足ごとに分け、上半分の足は上の帯へ、下半分の足は下の帯へ色の線でつなぎます。同じネットの帯どうしは左の線でつなぎます。帯を切るのは、裏から穴の銅を削ります (ドリルの刃を手で回す・専用のカッター)",
                        BoardKind::Perfboard => "穴だけの基板 (ユニバーサル基板) に組む並べ方です。帯が無いので、部品はつなぐ相手の近くに置き (長いヘッダは上の縁に寝かせて)、ネットごとに灰色の線 (すずめっき線) を裏で縦・横にまっすぐはんだ付けします。ほかのネットの線とは触れません。裏で通れない所は、色の線 (表を通すビニール線) で飛び越えます",
                    },
                );
            });
        });
        ui.add_space(crate::theme::SPACE_S);
    }

    /// The check against the schematic: one green line, or the differences (the same look on the three kinds of board).
    pub(super) fn check_ui(ui: &mut egui::Ui, problems: &[String]) {
        if problems.is_empty() {
            ui.colored_label(crate::theme::ok(), i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            ui.colored_label(BAD, i18n::trf!("回路図と違う所が {} 件あります", problems.len()));
            for p in problems {
                ui.label(egui::RichText::new(i18n::trf!("・{}", i18n::tr_dyn(p))).color(BAD));
            }
        }
    }

    /// The power switch, the same bar on the three kinds of board: 電源 切 | 入.
    pub(super) fn power_button(&mut self, ui: &mut egui::Ui, tip: &str) {
        ui.label(i18n::tr("電源")).on_hover_text(i18n::tr_dyn(tip));
        let size = egui::vec2(POWER_BAR_WIDTH, ui.spacing().interact_size.y);
        let bar = ui.allocate_ui(size, |ui| crate::widgets::segment_bar(ui, &[("切", None), ("入", None)], Some(usize::from(self.powered)))).inner;
        for (k, r) in bar.into_iter().enumerate() {
            if r.on_hover_text(i18n::tr_dyn(tip)).clicked() && (k == 1) != self.powered {
                self.powered = k == 1;
                self.dc_asked = false;
            }
        }
    }

    /// A folded group of the panel's lower half (the lists, the board's size): a band like the schematic's panel, the open state kept
    /// per `id`.
    pub(super) fn fold(ui: &mut egui::Ui, id: &str, title: impl Into<egui::WidgetText>, default_open: bool, body: impl FnOnce(&mut egui::Ui)) {
        let key = egui::Id::new(("proto_fold", id));
        let open = ui.data_mut(|d| *d.get_temp_mut_or(key, default_open));
        let (_, clicked) = crate::widgets::band_header(ui, id, title, open, false, body);
        if clicked {
            ui.data_mut(|d| d.insert_temp(key, !open));
        }
    }

    /// The breadboard's part of the panel.
    fn bread_panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let l = self.layout(sch);
        let problems: Vec<String> = self.board_check(sch, &l).iter().map(|p| p.message()).collect();
        Self::check_ui(ui, &problems);
        crate::widgets::group_gap(ui);
        const TOOLS: [(Tool, &str, &str); 4] = [
                (Tool::Move, "選択", "部品やジャンパー線をクリックで選び、ドラッグで差し替えます。選んだ部品は R で回り、選んだ線は Delete で消えます (回路図・基板の「選択」と同じ。ほかの道具からは Esc か右クリックで戻ります)"),
                (Tool::Wire, "ジャンパー線", "穴をクリックして、つなぐ先の穴をクリックすると、線を差します (W)。右クリックか Esc でやめます"),
                (Tool::Meter, "テスター", "1 回目のクリックで赤い棒、2 回目で黒い棒を穴に当てます。レンジは 電圧 (並列に当てる)・電流 (線を外して直列に入れる)・抵抗 (電源を切って測る) から選びます"),
                (Tool::Scope, "オシロ", "探り針を穴に当てると、そのネットの電圧の時間の変化 (過渡解析) が左に出ます"),
        ];
        let items: Vec<(&str, Option<&str>)> = TOOLS.iter().map(|(_, name, _)| (*name, None)).collect();
        let now = TOOLS.iter().position(|(t, ..)| *t == self.tool);
        for (r, (t, _, tip)) in crate::widgets::segment_bar(ui, &items, now).into_iter().zip(TOOLS) {
            if r.on_hover_text(i18n::tr(tip)).clicked() {
                self.tool = t;
                self.wire_start = None;
                self.dc_asked = false;
            }
        }
        // how the parts are placed, under the tools
        ui.horizontal(|ui| {
            if sch.breadboard.is_some() {
                if ui.small_button(i18n::tr("自動で並べ直す")).on_hover_text(i18n::tr("回路図から、部品の差し方を並べ直します (IC は中央の溝をまたぎ、電源とGNDは上下の列、同じネットの足は同じ縦の列)。手で動かした分は消えます")).clicked() {
                    sch.breadboard = Some(auto_layout(&sch.sch));
                }
            } else {
                // whole on the next line when it does not fit (not broken in two)
                ui.add(egui::Label::new(egui::RichText::new(i18n::tr("(自動で並べた形)")).color(NOTE)).extend()).on_hover_text(i18n::tr("まだ手で動かしていないので、回路図を変えると並べ直されます。部品を動かすと、その並べ方が回路図のファイルに入ります"));
            }
        });
        crate::widgets::group_gap(ui);
        ui.horizontal_wrapped(|ui| {
            self.power_button(ui, "電源を入れると、回路が時間とともに動き出します (過渡解析を実時間で進め続けます)。計算するのはブレッドボードに差したとおりの回路なので、回路図と違う差し方なら、そのとおりに動いたり壊れたりします。LED は流れる電流で光り、スイッチと押しボタンはクリックで切り替わります。定格を超えた部品は焦げた印と理由が出ます (実物なら壊れるところ)");
            // only for a circuit of logic chips (an Arduino's sketch is not shown there)
            if sch.sch.components.iter().any(sim_core::live_logic::is_logic_part)
                && ui
                .button(i18n::tr("デジタル回路を回路図の上で動かす"))
                .on_hover_text(i18n::tr("回路図のタブに移って、デジタル回路を動かしながら見ます。配線が High / Low の色になり、真理値表・ロジックアナライザ・フリップフロップの記憶・7 セグメントの表示・グリッチの印が出ます (ブレッドボードの絵より、信号の流れが追いやすい形)"))
                .clicked()
            {
                self.want_live_logic = true;
            }
        });
        // the meter with the power off reads the schematic's DC point (what the board will read once it is switched on)
        if !self.powered && self.tool == Tool::Meter && sch.bench_voltages().is_none() && !self.dc_asked && problems.is_empty() {
            self.dc_asked = true;
            sch.analyze_dc();
        }
        self.live_frame(ui, sch, &RunBoard::Bread(l.clone()), problems.is_empty());
        match self.tool {
            Tool::Meter => self.meter_ui(ui, sch, &l, problems.is_empty()),
            Tool::Scope => self.scope_ui(ui, sch, &l),
            Tool::Wire | Tool::Move => {}
        }
        self.power_issues_ui(ui);
        self.sketch_ui(ui, sch);
        crate::widgets::group_gap(ui);
        Self::fold(ui, "bread_parts", i18n::tr("部品と差す穴"), true, |ui| {
            // one part a row, its holes wrapped within the panel (a 30-pin board on one line widened the panel past the breadboard)
            for (k, p) in l.parts.iter().enumerate() {
                let picked = self.picked.as_deref() == Some(p.refdes.as_str());
                let row = |ui: &mut egui::Ui, picked: bool| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        let clicked = ui.add_sized([44.0, 18.0], egui::Button::selectable(picked, egui::RichText::new(&p.refdes).strong())).on_hover_text(i18n::tr("選ぶと、ブレッドボードの上でこの部品が光ります")).clicked();
                        let text = p.leads.iter().map(|(pin, h)| format!("{pin}:{}", super::hole_text(*h))).collect::<Vec<_>>().join("  ");
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
                    ui.label(format!("{}  {}", o.refdes, o.pins.iter().map(|(pin, net, h)| format!("{pin} ({net}) → {}", super::hole_text(*h))).collect::<Vec<_>>().join(i18n::tr("、"))));
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
                ui.colored_label(crate::theme::warn(), i18n::trf!("電源を入れたときの直流の様子が求まりませんでした: {}", i18n::tr_dyn(e)));
            } else if issues.is_empty() {
                ui.colored_label(crate::theme::ok(), i18n::tr("電源を入れました。定格を超えた部品はありません"));
            } else {
                for r in &issues {
                    let (col, head) = if r.level == sim_core::RatingLevel::Over { (BAD, i18n::tr("壊れた")) } else { (crate::theme::warn(), i18n::tr("熱い・余裕が少ない")) };
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
