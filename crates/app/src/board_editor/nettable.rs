//! "ネットの一覧" window: one row per net (class, pads, track length in all and per layer, detour, vias, track widths, the current with
//! the width it needs, pours, open connections), sorted by any column, with a filter and a copy as CSV. The numbers come from
//! `pcb_core::nettable` and `pcb_core::current`. The currents were a window of their own until 2026-11-14 (the same nets in two
//! windows, each with half of what one wants to compare: the width drawn and the width the current needs).

use super::*;
use pcb_core::current::{net_widths, round_up_width, CopperSpec};
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
    /// The schematic's DC current of `net` came back (`App` answers `net_current_request`): it is declared for that net alone, and
    /// only if the net has none yet.
    pub fn fill_net_current(&mut self, net: &str, currents: Result<Vec<(String, f64)>, String>) {
        let note = match currents {
            Err(e) => format!("回路図のシミュレーションから電流を取れませんでした: {e}。電流を自分で入れると、推奨の線幅が出ます"),
            Ok(list) => match list.iter().find(|(n, _)| n == net).map(|(_, a)| *a) {
                Some(a) if a > 1e-6 && !self.board.net_currents.contains_key(net) => {
                    let a = (a * 1000.0).round() / 1000.0;
                    self.board.net_currents.insert(net.to_string(), a.max(0.001));
                    format!("回路のDC解析の電流を入れました ({a} A)。ピーク電流 (起動時・スイッチング) は入らないので、余裕を見てください")
                }
                Some(_) => "回路のDC解析では、このネットに流れる電流が、ほとんどありません。電流を自分で入れると、推奨の線幅が出ます".into(),
                None => "回路のDC解析に、このネットが見つかりません (回路図とつながりが合っているか、基板の「回路図から取り込み」を確かめてください)".into(),
            },
        };
        self.net_win_note = Some((net.to_string(), note));
    }

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
        egui::Window::new(i18n::tr("ネットの一覧")).id(egui::Id::new("net_table_window")).open(&mut open).default_width(940.0).default_pos([380.0, 140.0]).show(ctx, |ui| {
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
                ui.separator();
                if ui.button(i18n::tr("DC解析の電流を入れる")).on_hover_text(i18n::tr("回路図のDC解析 (動作点) で各ネットに流れる電流を求めて、全部のネットに入れます (いま入れてある電流は置き換わります)")).clicked() {
                    self.currents_request = Some(CurrentSource::Dc);
                }
                if ui.button(i18n::tr("過渡解析の電流を入れる")).on_hover_text(i18n::tr("回路図で最後に実行した過渡解析の、後半の電流の実効値 (RMS) を、各ネットに入れます。先に回路図で過渡解析を実行してください")).clicked() {
                    self.currents_request = Some(CurrentSource::Tran);
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
            // the figures stay on the line; how to use the table goes behind the (?)
            ui.horizontal(|ui| {
                ui.small(i18n::trf!("{} ネット・配線の長さの合計 {:.1} mm・未配線 {total_open} 本", rows.len(), rows.iter().map(|r| r.length).sum::<f64>()));
                crate::icons::help(ui, "見出しをクリックすると、その列で並べ替えます (もう一度で逆順)。ネット名のクリックで、そのネットの設定の窓が開きます");
            });
            // the widths the currents need (only worked out when some net has a current)
            let has_currents = !self.board.net_currents.is_empty();
            let report = if has_currents { net_widths(&self.board, &drc::connectivity(&self.board), &CopperSpec::default()) } else { vec![] };
            let stats = !self.current_stats.is_empty();
            if !has_currents {
                ui.small(i18n::tr("電流はまだ入っていません。上の「DC解析の電流を入れる」か、電流の欄に打つと、その電流に要る線幅 (推奨) が出ます"));
            }
            egui::ScrollArea::both().max_height(460.0).show(ui, |ui| {
                egui::Grid::new("net_table").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                    let cols: [(&str, SortBy, &str); 4] = [
                        ("ネット", SortBy::Name, "ネットの名前 (回路図のネット名)"),
                        ("クラス", SortBy::Class, "ネットクラス (線幅・間隔・ビアをまとめた設定)。空欄は基板の設定のまま"),
                        ("パッド", SortBy::Pads, "このネットにつながるパッドの数"),
                        ("長さ (mm)", SortBy::Length, "配線 (銅の線) の長さの合計。ベタは入りません"),
                    ];
                    for (name, by, tip) in cols {
                        self.sort_header(ui, name, by, tip);
                    }
                    for l in &layers {
                        ui.strong(l.as_str()).on_hover_text(i18n::trf!("{l}の配線の長さ (mm)"));
                    }
                    self.sort_header(ui, "遠回り", SortBy::Detour, "配線の長さ ÷ パッドを直線で結んだ最短の長さ。1.0 前後がいちばん短く (途中で枝分かれすると 1 より少し小さくなることもあります)、大きいほど遠回り。ベタのあるネットは出しません");
                    self.sort_header(ui, "ビア", SortBy::Vias, "このネットのビア (層をつなぐ穴) の数");
                    self.sort_header(ui, "線幅 (mm)", SortBy::Thinnest, "このネットの配線の、いちばん細い〜いちばん太い線幅");
                    ui.strong(i18n::tr("電流")).on_hover_text(i18n::tr("このネットに流れる最大の電流。打って変えられます。0 なら調べません"));
                    ui.strong(i18n::tr("推奨の線幅 (mm)")).on_hover_text(i18n::tr("電流に要る線幅 (銅箔 35µm・温度上昇 10℃ の外層、IPC-2221)。✔ は今のいちばん細い配線で足りている、✖ は足りない"));
                    if stats {
                        ui.strong(i18n::tr("実効 (過渡)")).on_hover_text(i18n::tr("過渡解析の後半の電流の実効値。太さに使うのは、これです"));
                        ui.strong(i18n::tr("10 ms の最大")).on_hover_text(i18n::tr("短くても繰り返す山の熱"));
                        ui.strong(i18n::tr("ピーク")).on_hover_text(i18n::tr("一瞬の最大値。参考です"));
                    }
                    ui.strong(i18n::tr("ベタ")).on_hover_text(i18n::tr("このネットのベタ (銅箔ゾーン) がある層"));
                    self.sort_header(ui, "未配線", SortBy::Open, "まだ銅でつながっていない接続 (黄色の線) の数");
                    ui.end_row();
                    for r in &rows {
                        if ui.selectable_label(false, &r.net).on_hover_text(i18n::tr("クリックで、このネットの設定の窓を開きます")).clicked() {
                            clicked = Some(r.net.clone());
                        }
                        ui.label(if r.class.is_empty() { "—" } else { r.class.as_str() });
                        ui.label(r.pads.to_string());
                        ui.label(format!("{:.1}", r.length));
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
                        let mut a = self.board.net_currents.get(&r.net).copied().unwrap_or(0.0);
                        ui.add(egui::DragValue::new(&mut a).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(3)).on_hover_text(i18n::tr("このネットに流れる最大の電流。0 なら調べません"));
                        if a > 0.0 {
                            self.board.net_currents.insert(r.net.clone(), a);
                        } else {
                            self.board.net_currents.remove(&r.net);
                        }
                        match report.iter().find(|w| w.net == r.net) {
                            Some(w) => {
                                let need = format!("{:.2}", round_up_width(w.needed));
                                match w.thinnest {
                                    None => ui.label(need).on_hover_text(i18n::tr("まだ配線がありません")),
                                    Some(t) if w.too_thin() => ui.colored_label(crate::theme::ERROR, format!("{need} ✖")).on_hover_text(i18n::trf!("今のいちばん細い配線は {t:.2} mm で、足りません")),
                                    Some(t) => ui.colored_label(Color32::from_rgb(90, 190, 110), format!("{need} ✔")).on_hover_text(i18n::trf!("今のいちばん細い配線は {t:.2} mm で、足りています")),
                                };
                            }
                            None => {
                                ui.label("—");
                            }
                        }
                        if stats {
                            match self.current_stats.iter().find(|s| s.net == r.net) {
                                Some(s) => {
                                    ui.label(format!("{:.3} A", s.rms));
                                    ui.label(format!("{:.3} A", s.burst_rms));
                                    let txt = format!("{:.3} A{}", s.peak, if s.spiky() { " ⚠" } else { "" });
                                    if s.spiky() {
                                        ui.colored_label(crate::theme::WARN, txt).on_hover_text(i18n::tr("ピークが実効値の10倍以上です。起動時のサージやスパイクかもしれません。実物では電源の内部抵抗・ESR・ソフトスタートが抑えるので、そのまま太さにすると必要以上に太くなります"));
                                    } else {
                                        ui.label(txt);
                                    }
                                }
                                None => {
                                    ui.label("—");
                                    ui.label("—");
                                    ui.label("—");
                                }
                            }
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
            let thin: Vec<String> = report.iter().filter(|w| w.too_thin()).map(|w| w.net.clone()).collect();
            if !thin.is_empty() && ui.button(i18n::trf!("足りない {} ネットを推奨幅にする", thin.len())).on_hover_text(i18n::tr("電流に対して細いネットの配線を、すべて必要な太さまで太くします (Ctrl+Z で戻せます)。太くしたあと、DRC で間隔が足りるか確かめましょう")).clicked() {
                for n in thin {
                    self.widen_net_to_need(&n);
                }
            }
            if has_currents {
                ui.small(i18n::tr("太さの目安は、銅箔 35µm・温度上昇 10℃ の外層 (IPC-2221) です。ピーク電流 (起動時・スイッチング) は入らないので、余裕を見てください"));
            }
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
        if ui.selectable_label(now.is_some(), egui::RichText::new(format!("{}{mark}", i18n::tr_dyn(name))).strong()).on_hover_text(i18n::tr_dyn(tip)).clicked() {
            let first_desc = !matches!(by, SortBy::Name | SortBy::Class);
            self.net_table.sort = Some((by, now.map_or(first_desc, |(_, d)| !d)));
        }
    }
}
