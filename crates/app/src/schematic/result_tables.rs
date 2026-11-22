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
    /// The "ピン定義" window, and its last check of a firmware file (the file and the findings).
    pub pins: bool,
    pin_check: Option<(String, Vec<sch_core::pindefs::PinFinding>)>,
    /// The format to write (0: Arduino header, 1: Zephyr overlay, 2..: an FPGA's constraints) and the FPGA part.
    pin_format: usize,
    pin_part: String,
    tab: Tab,
    node_sort: NodeSort,
    elem_sort: ElemSort,
    filter: String,
    net_filter: String,
}

impl SchEditor {
    /// Opens the "解析結果の表" window (the start arguments use it for screenshots).
    pub fn open_result_table(&mut self) {
        self.tables.results = true;
    }

    /// Opens the "ピン定義" window (`--pins`, for screenshots).
    pub fn open_pins(&mut self) {
        self.tables.pins = true;
    }

    /// Opens the "ネットリスト" window (`--netlist`, for screenshots).
    pub fn open_netlist(&mut self) {
        self.tables.netlist = true;
    }

    /// Draws whichever of the two windows is open (called every frame from the side panel).
    pub(super) fn result_windows(&mut self, ctx: &egui::Context) {
        let mut st = std::mem::take(&mut self.tables);
        let mut goto: Option<String> = None;
        let mut goto_net: Option<String> = None;
        if st.results {
            let mut open = true;
            egui::Window::new(i18n::tr("解析結果の表")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_size([440.0, 460.0]).show(ctx, |ui| {
                self.results_table(ui, &mut st, &mut goto, &mut goto_net);
            });
            st.results = open;
        }
        if st.netlist {
            let mut open = true;
            egui::Window::new(i18n::tr("ネットリスト")).open(&mut open).default_pos(crate::widgets::beside_panel() + egui::vec2(24.0, 24.0)).default_size([440.0, 420.0]).show(ctx, |ui| {
                self.netlist_table(ui, &mut st, &mut goto_net);
            });
            st.netlist = open;
        }
        if st.pins {
            let mut open = true;
            let mut check = st.pin_check.take();
            egui::Window::new(i18n::tr("ピン定義とファーム")).open(&mut open).default_pos(crate::widgets::beside_panel() + egui::vec2(48.0, 48.0)).default_size([520.0, 420.0]).show(ctx, |ui| {
                self.pins_contents(ui, &mut check, &mut st.pin_format, &mut st.pin_part);
                self.firmware_contents(ui);
            });
            st.pin_check = check;
            st.pins = open;
        }
        self.tables = st;
        self.lib_sync_window(ctx);
        self.adc_measure_window(ctx);
        self.time_log_window(ctx);
        if let Some(name) = goto_net {
            self.goto_net(&name);
        }
        if let Some(refdes) = goto {
            self.goto_part(&refdes);
        }
    }

    fn results_table(&self, ui: &mut egui::Ui, st: &mut State, goto: &mut Option<String>, goto_net: &mut Option<String>) {
        let Some(res) = self.dc.as_ref().and_then(|d| d.outcome.as_ref().ok()) else {
            ui.label(i18n::tr("まだ結果がありません。「DC動作点を解析」を押してから開いてください"));
            return;
        };
        crate::widgets::tab_bar(
            ui,
            &mut st.tab,
            &[(Tab::Nodes, "ノード電圧", "回路の各ノード (ネット) の電圧です"), (Tab::Elements, "素子", "素子ごとの電流・電圧・電力です")],
        );
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(i18n::tr("検索 (名前)")).desired_width(160.0))
                .on_hover_text(i18n::tr("名前に、この文字が入っているものだけを出します (大文字・小文字は区別しません)"));
            match st.tab {
                Tab::Nodes => {
                    ui.selectable_value(&mut st.node_sort, NodeSort::Name, i18n::tr("名前順")).on_hover_text(i18n::tr("ノード名の順に並べます"));
                    ui.selectable_value(&mut st.node_sort, NodeSort::VoltageHigh, i18n::tr("電圧が高い順")).on_hover_text(i18n::tr("電圧の高いノードを上に並べます (電源のレールを探すとき)"));
                }
                Tab::Elements => {
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Name, i18n::tr("名前順")).on_hover_text(i18n::tr("素子名の順に並べます"));
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Power, i18n::tr("電力が大きい順")).on_hover_text(i18n::tr("電力の大きい素子を上に並べます (熱くなる部品を探すとき)"));
                    ui.selectable_value(&mut st.elem_sort, ElemSort::Current, i18n::tr("電流が大きい順")).on_hover_text(i18n::tr("電流の大きい素子を上に並べます"));
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
                // the readings taken on the board (L2, entered in the bench tab's "測る"): beside the prediction, with the difference
                let measured = !self.bench.dc.is_empty();
                egui::Grid::new("res_nodes").striped(true).num_columns(if measured { 4 } else { 2 }).spacing([24.0, 4.0]).show(ui, |ui| {
                    ui.strong(i18n::tr("ノード"));
                    num_head(ui, "電圧");
                    if measured {
                        num_head(ui, "実測");
                        num_head(ui, "差");
                    }
                    ui.end_row();
                    for (n, v) in rows {
                        if ui.link(n.as_str()).on_hover_text(i18n::tr("クリックで、このネットにつながる部品を回路図で選びます")).clicked() {
                            *goto_net = Some(n.clone());
                        }
                        num(ui, eng(*v, "V"));
                        if measured {
                            match self.dc_reading(n) {
                                Some(r) => {
                                    num(ui, eng(r.volts, "V"));
                                    let (diff, agree) = super::judge(*v, None, r.volts, r.uncert);
                                    let color = match agree {
                                        super::Agree::Good => crate::theme::ok(),
                                        super::Agree::Off => crate::theme::warn(),
                                        super::Agree::Far => crate::theme::error(),
                                    };
                                    ui.label(egui::RichText::new(eng(diff, "V")).monospace().color(color)).on_hover_text(i18n::tr("実測 − 予測。緑は合う、橙は少しずれ、赤は大きくずれ (実機のタブの「測る」と同じ判定)"));
                                }
                                None => {
                                    ui.label("");
                                    ui.label("");
                                }
                            }
                        }
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
                egui::Grid::new("res_elems").striped(true).num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
                    ui.strong(i18n::tr("素子"));
                    num_head(ui, "電流");
                    num_head(ui, "電圧");
                    num_head(ui, "電力");
                    ui.end_row();
                    for e in rows {
                        if ui.link(e.name.as_str()).on_hover_text(i18n::tr("クリックで、この部品を回路図で選びます")).clicked() {
                            *goto = Some(e.name.clone());
                        }
                        num(ui, eng(e.current, "A"));
                        num(ui, eng(e.voltage, "V"));
                        num(ui, eng(e.power, "W"));
                        ui.end_row();
                    }
                });
                ui.small(i18n::tr("電流は素子の第1端子→第2端子の向き。電源は電力を供給すると負"));
            }
        });
    }

    fn netlist_table(&self, ui: &mut egui::Ui, st: &mut State, goto_net: &mut Option<String>) {
        ui.add(egui::TextEdit::singleline(&mut st.net_filter).hint_text(i18n::tr("検索 (ネット名・部品番号)")).desired_width(f32::INFINITY))
            .on_hover_text(i18n::tr("ネットの名前か、そのネットにつながるピン (例: R1) に、この文字が入っているものだけを出します"));
        let needle = st.net_filter.to_lowercase();
        let nets = extract(&self.sch);
        // a table: the net, how many pins, the pins (wrapped in their column)
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("netlist_rows").striped(true).num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
                ui.strong(i18n::tr("ネット"));
                num_head(ui, "ピン数");
                ui.strong(i18n::tr("つながるピン (部品.ピン)"));
                ui.end_row();
                for n in &nets {
                    let pins: Vec<String> = n.pins.iter().map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                    if !needle.is_empty() && !n.name.to_lowercase().contains(&needle) && !pins.iter().any(|p| p.to_lowercase().contains(&needle)) {
                        continue;
                    }
                    if ui.link(egui::RichText::new(&n.name).strong()).on_hover_text(i18n::tr("クリックで、このネットにつながる部品を回路図で選びます")).clicked() {
                        *goto_net = Some(n.name.clone());
                    }
                    num(ui, pins.len().to_string());
                    ui.add(egui::Label::new(pins.join(", ")).wrap());
                    ui.end_row();
                }
            });
        });
    }

    /// Selects the part with this part number and brings it to the middle of the sheet (opening the sheet it is on when that is another one).
    pub(super) fn goto_part(&mut self, refdes: &str) {
        let mut sheet = (self.sch.components.iter().any(|c| c.refdes == refdes)).then_some(self.sch.active_sheet);
        if sheet.is_none() {
            sheet = (0..self.sch.sheets.len()).find(|&k| k != self.sch.active_sheet && self.sch.sheets[k].components.iter().any(|c| c.refdes == refdes));
        }
        let Some(k) = sheet else {
            self.status = format!("{refdes} が回路図に見つかりません");
            return;
        };
        if k != self.sch.active_sheet {
            self.show_sheet(k);
            self.fit_pending = false; // the view is set below, not fitted to the whole sheet
        }
        let Some(i) = self.sch.components.iter().position(|c| c.refdes == refdes) else { return };
        self.selected = Some(Sel::Comp(i));
        self.multi.clear();
        let at = self.sch.components[i].pos;
        self.pan = Vec2::new(at.0 as f32, at.1 as f32);
    }

    /// Selects the first part on a net (the net itself cannot be selected) and brings it to the middle of the sheet.
    pub(super) fn goto_net(&mut self, name: &str) {
        let first = extract(&self.sch).into_iter().find(|n| n.name == name).and_then(|n| n.pins.first().map(|p| p.refdes.clone()));
        match first {
            Some(r) => self.goto_part(&r),
            None => self.status = format!("ネット {name} につながる部品が見つかりません"),
        }
    }
}

/// A number in a table cell: monospace and right-aligned in a fixed width, so the digits of a column line up.
fn num(ui: &mut egui::Ui, text: String) {
    ui.label(egui::RichText::new(format!("{text:>10}")).monospace());
}

/// The heading of a column of `num`s, right-aligned like them.
fn num_head(ui: &mut egui::Ui, name: &str) {
    let t = i18n::tr(name);
    // a Japanese letter is two columns wide in the monospace font
    let width: usize = t.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    ui.label(egui::RichText::new(format!("{}{t}", " ".repeat(10usize.saturating_sub(width)))).monospace().strong());
}
