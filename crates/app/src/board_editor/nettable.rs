//! "ネットの一覧" window: one row per net (class, pads, track length in all and per layer, detour, vias, track widths, pours,
//! open connections), sorted by any column, with a filter and a copy as CSV. The numbers come from `pcb_core::nettable`.

use super::*;
use pcb_core::nettable::{net_table, sort_rows, table_csv, NetRow, SortBy};

/// The table of the window, kept while the board does not change.
#[derive(Default)]
pub struct NetTableState {
    pub open: bool,
    pub sort: Option<(SortBy, bool)>,
    pub filter: String,
    cache: Option<(u64, Vec<NetRow>)>,
}

impl BoardEditor {
    /// The rows for the board as it is now (worked out again only after a change of copper or classes).
    fn net_rows(&mut self) -> Vec<NetRow> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        for c in &self.board.net_classes {
            c.name.hash(&mut h);
            c.nets.hash(&mut h);
        }
        let key = h.finish();
        if let Some((k, rows)) = &self.net_table.cache
            && *k == key {
                return rows.clone();
            }
        let wires = self.cached_airwires();
        let rows = net_table(&self.board, Some(&wires));
        self.net_table.cache = Some((key, rows.clone()));
        rows
    }

    pub fn net_table_window(&mut self, ctx: &egui::Context) {
        if !self.net_table.open {
            return;
        }
        let mut open = true;
        let mut clicked: Option<String> = None;
        egui::Window::new(i18n::tr("ネットの一覧")).id(egui::Id::new("net_table_window")).open(&mut open).default_width(760.0).default_pos([380.0, 140.0]).show(ctx, |ui| {
            let mut rows = self.net_rows();
            if rows.is_empty() {
                ui.label(i18n::tr("回路図から部品を取り込むと、ネットの一覧が出ます"));
                return;
            }
            ui.horizontal(|ui| {
                ui.label(i18n::tr("絞り込み"));
                ui.add(egui::TextEdit::singleline(&mut self.net_table.filter).desired_width(140.0).hint_text(i18n::tr("ネット名・クラス")))
                    .on_hover_text(i18n::tr("ネット名かネットクラスの名前に、この文字を含む行だけを出します (大文字・小文字は区別しません)"));
                if ui.button(i18n::tr("CSV をコピー")).on_hover_text(i18n::tr("いま出ている行を CSV (表計算ソフトに貼れる形) でクリップボードに写します")).clicked() {
                    ui.ctx().copy_text(table_csv(&self.filtered(&rows)));
                    self.status = "ネットの一覧を CSV でコピーしました (表計算ソフトに貼り付けられます)".into();
                }
            });
            if let Some((by, desc)) = self.net_table.sort {
                sort_rows(&mut rows, by, desc);
            } else {
                sort_rows(&mut rows, SortBy::Name, false);
            }
            let rows = self.filtered(&rows);
            let mut layers: Vec<String> = rows.iter().flat_map(|r| r.by_layer.keys().cloned()).collect();
            layers.sort_by_key(|l| match l.as_str() {
                "表面" => 0,
                "裏面" => 99,
                _ => 1,
            });
            layers.dedup();
            let total_open: usize = rows.iter().map(|r| r.open).sum();
            ui.small(i18n::trf!("{} ネット・配線の長さの合計 {:.1} mm・未配線 {total_open} 本。見出しをクリックすると、その列で並べ替えます (もう一度で逆順)。ネット名のクリックで、そのネットの設定の窓が開きます", rows.len(), rows.iter().map(|r| r.length).sum::<f64>()));
            egui::ScrollArea::both().max_height(460.0).show(ui, |ui| {
                egui::Grid::new("net_table").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                    let cols: [(&str, SortBy, &str); 4] = [
                        ("ネット", SortBy::Name, "ネットの名前 (回路図のネット名)"),
                        ("クラス", SortBy::Class, "ネットクラス (線幅・間隔・ビアをまとめた設定)。空欄は基板の設定のまま"),
                        ("パッド", SortBy::Pads, "このネットにつながるパッドの数"),
                        ("長さ", SortBy::Length, "配線 (銅の線) の長さの合計。ベタは入りません"),
                    ];
                    for (name, by, tip) in cols {
                        self.sort_header(ui, name, by, tip);
                    }
                    for l in &layers {
                        ui.strong(l.as_str()).on_hover_text(i18n::trf!("{l}の配線の長さ"));
                    }
                    self.sort_header(ui, "遠回り", SortBy::Detour, "配線の長さ ÷ パッドを直線で結んだ最短の長さ。1.0 前後がいちばん短く (途中で枝分かれすると 1 より少し小さくなることもあります)、大きいほど遠回り。ベタのあるネットは出しません");
                    self.sort_header(ui, "ビア", SortBy::Vias, "このネットのビア (層をつなぐ穴) の数");
                    self.sort_header(ui, "線幅", SortBy::Thinnest, "このネットの配線の、いちばん細い〜いちばん太い線幅");
                    ui.strong(i18n::tr("ベタ")).on_hover_text(i18n::tr("このネットのベタ (銅箔ゾーン) がある層"));
                    self.sort_header(ui, "未配線", SortBy::Open, "まだ銅でつながっていない接続 (黄色の線) の数");
                    ui.end_row();
                    for r in &rows {
                        if ui.selectable_label(false, &r.net).on_hover_text(i18n::tr("クリックで、このネットの設定の窓を開きます")).clicked() {
                            clicked = Some(r.net.clone());
                        }
                        ui.label(if r.class.is_empty() { "—" } else { r.class.as_str() });
                        ui.label(r.pads.to_string());
                        ui.label(format!("{:.1} mm", r.length));
                        for l in &layers {
                            ui.label(r.by_layer.get(l).map_or("—".to_string(), |v| format!("{v:.1}")));
                        }
                        match r.detour() {
                            Some(d) if d >= 2.0 => ui.colored_label(crate::theme::WARN, format!("{d:.2}")).on_hover_text(i18n::tr("最短の 2 倍以上です。遠回りしていないか、「配線を短くする」で引き直せないか見てみましょう")),
                            Some(d) => ui.label(format!("{d:.2}")),
                            None => ui.label("—"),
                        };
                        ui.label(r.vias.to_string());
                        if r.thinnest > 0.0 {
                            let (lo, hi) = (format!("{:.2}", r.thinnest), format!("{:.2}", r.widest));
                            ui.label(if lo == hi { lo } else { format!("{lo}〜{hi}") });
                        } else {
                            ui.label("—");
                        }
                        ui.label(if r.pours.is_empty() { "—".to_string() } else { r.pours.join("・") });
                        if r.open > 0 {
                            ui.colored_label(crate::theme::ERROR, r.open.to_string());
                        } else {
                            ui.label("0");
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if let Some(n) = clicked {
            self.open_net_window(&n);
        }
        if !open {
            self.net_table.open = false;
        }
    }

    fn filtered(&self, rows: &[NetRow]) -> Vec<NetRow> {
        let f = self.net_table.filter.trim().to_lowercase();
        rows.iter().filter(|r| f.is_empty() || r.net.to_lowercase().contains(&f) || r.class.to_lowercase().contains(&f)).cloned().collect()
    }

    /// A column heading that sorts by its column: names go A→Z first, numbers biggest first; a second click turns the order round.
    fn sort_header(&mut self, ui: &mut egui::Ui, name: &str, by: SortBy, tip: &str) {
        let now = self.net_table.sort.filter(|(b, _)| *b == by);
        let mark = match now {
            Some((_, true)) => " ▼",
            Some((_, false)) => " ▲",
            None => "",
        };
        if ui.selectable_label(now.is_some(), egui::RichText::new(format!("{name}{mark}")).strong()).on_hover_text(tip).clicked() {
            let first_desc = !matches!(by, SortBy::Name | SortBy::Class);
            self.net_table.sort = Some((by, now.map_or(first_desc, |(_, d)| !d)));
        }
    }
}
