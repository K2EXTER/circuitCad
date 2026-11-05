//! Windows for the long lists of a result: the DC results table (every node voltage, every element's current, voltage and power) and the
//! net list. The panel only shows a summary line and a button, so a big circuit does not stretch the panel; the windows have a search
//! box, a sort order, and a click on a row jumps to that part on the sheet.

use super::*;

#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
    Nodes,
    Elements,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum NodeSort {
    #[default]
    Name,
    VoltageHigh,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ElemSort {
    #[default]
    Name,
    Power,
    Current,
}

/// What the two windows remember (which are open, the search text and the sort order).
#[derive(Default)]
pub(super) struct State {
    pub results: bool,
    pub netlist: bool,
    tab: Tab,
    node_sort: NodeSort,
    elem_sort: ElemSort,
    filter: String,
    net_filter: String,
}

impl SchEditor {
    /// Draws whichever of the two windows is open (called every frame from the side panel).
    pub(super) fn result_windows(&mut self, ctx: &egui::Context) {
        let mut st = std::mem::take(&mut self.tables);
        let mut goto: Option<String> = None;
        let mut goto_net: Option<String> = None;
        if st.results {
            let mut open = true;
            egui::Window::new("解析結果の表").open(&mut open).default_pos(crate::widgets::beside_panel()).default_size([440.0, 460.0]).show(ctx, |ui| {
                self.results_table(ui, &mut st, &mut goto, &mut goto_net);
            });
            st.results = open;
        }
        if st.netlist {
            let mut open = true;
            egui::Window::new("ネットリスト").open(&mut open).default_pos(crate::widgets::beside_panel() + egui::vec2(24.0, 24.0)).default_size([440.0, 420.0]).show(ctx, |ui| {
                self.netlist_table(ui, &mut st, &mut goto_net);
            });
            st.netlist = open;
        }
        self.tables = st;
        if let Some(name) = goto_net {
            self.goto_net(&name);
        }
        if let Some(refdes) = goto {
            self.goto_part(&refdes);
        }
    }

    fn results_table(&self, ui: &mut egui::Ui, st: &mut State, goto: &mut Option<String>, goto_net: &mut Option<String>) {
        let Some(res) = self.dc.as_ref().and_then(|d| d.outcome.as_ref().ok()) else {
            ui.label("まだ結果がありません。「DC動作点を解析」を押してから開いてください");
            return;
        };
        crate::widgets::tab_bar(
            ui,
            &mut st.tab,
            &[(Tab::Nodes, "ノード電圧", "回路の各ノード (ネット) の電圧です"), (Tab::Elements, "素子", "素子ごとの電流・電圧・電力です")],
        );
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text("検索 (名前)").desired_width(160.0))
                .on_hover_text("名前に、この文字が入っているものだけを出します (大文字・小文字は区別しません)");
            match st.tab {
                Tab::Nodes => {
                    ui.selectable_value(&mut st.node_sort, NodeSort::Name, "名前順").on_hover_text("ノード名の順に並べます");
                    ui.selectable_value(&mut st.node_sort, NodeSort::VoltageHigh, "電圧が高い順").on_hover_text("電圧の高いノードを上に並べます (電源のレールを探すとき)");
                }
                Tab::Elements => {
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Name, "名前順").on_hover_text("素子名の順に並べます");
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Power, "電力が大きい順").on_hover_text("電力の大きい素子を上に並べます (熱くなる部品を探すとき)");
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Current, "電流が大きい順").on_hover_text("電流の大きい素子を上に並べます");
                }
            }
        });
        let needle = st.filter.to_lowercase();
        let keep = |name: &str| !name.contains('.') && (needle.is_empty() || name.to_lowercase().contains(&needle));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match st.tab {
            Tab::Nodes => {
                let mut rows: Vec<&(String, f64)> = res.voltages.iter().filter(|(n, _)| keep(n)).collect();
                match st.node_sort {
                    NodeSort::Name => rows.sort_by(|a, b| a.0.cmp(&b.0)),
                    NodeSort::VoltageHigh => rows.sort_by(|a, b| b.1.total_cmp(&a.1)),
                }
                egui::Grid::new("res_nodes").striped(true).num_columns(2).show(ui, |ui| {
                    for (n, v) in rows {
                        if ui.add(egui::Label::new(n).sense(egui::Sense::click())).on_hover_text("クリックで、このネットにつながる部品を回路図で選びます").clicked() {
                            *goto_net = Some(n.clone());
                        }
                        ui.label(eng(*v, "V"));
                        ui.end_row();
                    }
                });
            }
            Tab::Elements => {
                let mut rows: Vec<&sim_core::ElementResult> = res.elements.iter().filter(|e| keep(&e.name)).collect();
                match st.elem_sort {
                    ElemSort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name)),
                    ElemSort::Power => rows.sort_by(|a, b| b.power.abs().total_cmp(&a.power.abs())),
                    ElemSort::Current => rows.sort_by(|a, b| b.current.abs().total_cmp(&a.current.abs())),
                }
                egui::Grid::new("res_elems").striped(true).num_columns(4).show(ui, |ui| {
                    ui.strong("素子");
                    ui.strong("電流");
                    ui.strong("電圧");
                    ui.strong("電力");
                    ui.end_row();
                    for e in rows {
                        if ui.add(egui::Label::new(&e.name).sense(egui::Sense::click())).on_hover_text("クリックで、この部品を回路図で選びます").clicked() {
                            *goto = Some(e.name.clone());
                        }
                        ui.label(eng(e.current, "A"));
                        ui.label(eng(e.voltage, "V"));
                        ui.label(eng(e.power, "W"));
                        ui.end_row();
                    }
                });
                ui.small("電流は素子の第1端子→第2端子の向き。電源は電力を供給すると負");
            }
        });
    }

    fn netlist_table(&self, ui: &mut egui::Ui, st: &mut State, goto_net: &mut Option<String>) {
        ui.add(egui::TextEdit::singleline(&mut st.net_filter).hint_text("検索 (ネット名・部品番号)").desired_width(f32::INFINITY))
            .on_hover_text("ネットの名前か、そのネットにつながるピン (例: R1) に、この文字が入っているものだけを出します");
        let needle = st.net_filter.to_lowercase();
        let nets = extract(&self.sch);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for n in &nets {
                let pins: Vec<String> = n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                if !needle.is_empty() && !n.name.to_lowercase().contains(&needle) && !pins.iter().any(|p| p.to_lowercase().contains(&needle)) {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add(egui::Label::new(egui::RichText::new(&n.name).strong()).sense(egui::Sense::click())).on_hover_text("クリックで、このネットにつながる部品を回路図で選びます").clicked() {
                        *goto_net = Some(n.name.clone());
                    }
                    ui.label(format!("({} ピン)", pins.len()));
                    ui.label(pins.join(", "));
                });
            }
        });
    }

    /// Selects the part with this part number and brings it to the middle of the sheet.
    pub(super) fn goto_part(&mut self, refdes: &str) {
        let Some(i) = self.sch.components.iter().position(|c| c.refdes == refdes) else {
            self.status = format!("{refdes} は、いま開いているシートにありません。ほかのシートを探してください");
            return;
        };
        self.selected = Some(Sel::Comp(i));
        self.multi.clear();
        let at = self.sch.components[i].pos;
        self.pan = Vec2::new(at.0 as f32, at.1 as f32);
    }

    /// Selects the first part on a net (the net itself cannot be selected) and brings it to the middle of the sheet.
    fn goto_net(&mut self, name: &str) {
        let first = extract(&self.sch).into_iter().find(|n| n.name == name).and_then(|n| n.pins.first().map(|p| p.refdes.clone()));
        match first {
            Some(r) => self.goto_part(&r),
            None => self.status = format!("ネット {name} につながる部品が見つかりません"),
        }
    }
}
