//! The "差動ペア" panel: pick two nets, route them side by side, even out their lengths.

use super::*;
use pcb_core::diffpair::{pair_report, suggest_pairs, tune_length, DiffPair};

impl BoardEditor {
    pub(super) fn panel_pairs(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(format!("差動ペア ({}) (?)", self.board.diff_pairs.len())).id_salt("diff_pairs").default_open(false).show(ui, |ui| {
            let nets = self.board_nets();
            ui.horizontal(|ui| {
                if ui.button("名前から探す").on_hover_text("ネット名が USB_DP / USB_DM、TX_P / TX_N、D+ / D- のように対になっているものを、ペアにします").clicked() {
                    let found: Vec<DiffPair> = suggest_pairs(&nets).into_iter().filter(|d| !self.board.diff_pairs.iter().any(|x| x.p == d.p && x.n == d.n)).collect();
                    if found.is_empty() {
                        self.status = "対になっているネット名が見つかりませんでした。下で2つのネットを選んで追加できます".into();
                    } else {
                        self.checkpoint();
                        self.status = format!("差動ペアを {} 組、追加しました", found.len());
                        self.board.diff_pairs.extend(found);
                    }
                }
            });
            ui.horizontal(|ui| {
                for (k, label) in [(0, "＋側"), (1, "－側")] {
                    let cur = if k == 0 { &mut self.pair_new.0 } else { &mut self.pair_new.1 };
                    egui::ComboBox::from_id_salt(("pair_pick", k)).selected_text(if cur.is_empty() { label.to_string() } else { cur.clone() }).width(90.0).show_ui(ui, |ui| {
                        for n in &nets {
                            ui.selectable_value(cur, n.clone(), n);
                        }
                    });
                }
                let ok = !self.pair_new.0.is_empty() && !self.pair_new.1.is_empty() && self.pair_new.0 != self.pair_new.1;
                if ui.add_enabled(ok, egui::Button::new("ペアに追加")).on_hover_text("選んだ2つのネットを、差動ペアにします").clicked() {
                    self.checkpoint();
                    let name = format!("{}/{}", self.pair_new.0, self.pair_new.1);
                    self.board.diff_pairs.push(DiffPair::new(&name, &self.pair_new.0, &self.pair_new.1));
                    self.pair_new = Default::default();
                }
            });
            let mut remove = None;
            let mut route = None;
            let mut tune = None;
            for (i, d) in self.board.diff_pairs.iter_mut().enumerate() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{}  ({} / {})", d.name, d.p, d.n)).strong());
                    if ui.small_button("削除").on_hover_text("この組を差動ペアから外します (配線はそのまま残ります)").clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut d.gap).speed(0.01).range(0.05..=2.0).prefix("間隔 ").suffix(" mm")).on_hover_text("2本の線の縁どうしの目標の間隔。発注先の最小間隔より狭くはできません。差動インピーダンスは、この間隔と線幅・基板の厚みで決まります (USB は 90Ω が目安。基板の積層で変わるので、発注先の計算ツールで確かめます)");
                    ui.add(egui::DragValue::new(&mut d.max_skew).speed(0.05).range(0.0..=20.0).prefix("長さの差 ").suffix(" mm以下")).on_hover_text("2本の長さの差の許容値。信号は銅の上を 1mm あたり約 6〜7ps で進みます。低速 (数 MHz 以下) なら数 mm ずれても問題になりません");
                });
                ui.horizontal(|ui| {
                    if ui.button("ペアで配線").on_hover_text("＋側を配線してから、－側をそのすぐ隣に配線します。通せなかった接続は黄色の線のまま残ります").clicked() {
                        route = Some(i);
                    }
                    if ui.button("長さを合わせる").on_hover_text("短いほうの線にジグザグ (蛇行) を足して、長さを揃えます。まっすぐで長い所が必要です。ルール違反になるときは、何もしません").clicked() {
                        tune = Some(i);
                    }
                });
            }
            for d in &self.board.diff_pairs {
                let r = pair_report(&self.board, d);
                if r.len_p == 0.0 && r.len_n == 0.0 {
                    continue;
                }
                let ok = r.skew <= d.max_skew + 1e-9;
                let shorter = r.len_p.min(r.len_n);
                ui.separator();
                ui.label(egui::RichText::new(format!("{} {}", d.name, if ok { "✔ 長さが合っています" } else { "⚠ 長さの差が大きい" })).strong());
                ui.label(format!("{} {:.1}mm (ビア{})  /  {} {:.1}mm (ビア{})", d.p, r.len_p, r.vias_p, d.n, r.len_n, r.vias_n));
                ui.label(format!(
                    "長さの差 {:.2}mm  並走 {:.0}%{}",
                    r.skew,
                    if shorter > 0.0 { 100.0 * r.coupled / shorter } else { 0.0 },
                    r.gap_min.zip(r.gap_max).map_or(String::new(), |(a, b)| format!("  間隔 {:.2}〜{:.2}mm", a.max(0.0), b)),
                ));
            }
            if let Some(i) = remove {
                self.checkpoint();
                self.board.diff_pairs.remove(i);
            }
            if let Some(i) = route {
                self.route_pair_at(i);
            }
            if let Some(i) = tune {
                let d = self.board.diff_pairs[i].clone();
                self.checkpoint();
                match tune_length(&mut self.board, &d) {
                    Ok(add) => self.status = format!("{} の短いほうに蛇行を足して、{:.2}mm 長くしました。DRC で確認しましょう", d.name, add),
                    Err(e) => {
                        self.undo.pop();
                        self.status = e;
                    }
                }
            }
        }).header_response.on_hover_text("USB の D+ / D- のように、2本で1つの信号を運ぶ配線です。並べて同じ長さで引くと、ノイズに強くなります。");
    }

    /// Route one pair side by side. One undo step; nothing changes (and no undo step) when nothing could be routed.
    pub(super) fn route_pair_at(&mut self, i: usize) {
        let Some(d) = self.board.diff_pairs.get(i).cloned() else { return };
        if self.board.outline.len() < 3 {
            self.status = "先に基板の外形を決めてください".into();
            return;
        }
        self.checkpoint();
        let opt = RouteOptions { track_width: self.track_width, via_diameter: self.via_dia, via_drill: self.via_drill, grid: actions::route_pitch(self.grid), follow: None, pad_lead: self.pad_lead.then_some(self.pad_lead_len) };
        let r = autoroute::route_pair(&mut self.board, &d, &opt);
        if r.routed == 0 {
            self.undo.pop();
        }
        self.violations.clear();
        self.status = if r.failed.is_empty() {
            format!("{}: {} 本を並べて配線しました。DRC で確認しましょう", d.name, r.routed)
        } else {
            format!("{}: {} 本を配線、{} 本は通せませんでした。部品の位置や層数を見直してください", d.name, r.routed, r.failed.len())
        };
    }
}
