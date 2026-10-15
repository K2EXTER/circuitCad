//! "配線を整える" section of the board panel: the finish of the copper (round corners, teardrops, the round end of
//! a notch). It is a setting of the board that is applied when the board is shown and made, so the tracks stay as
//! drawn; the buttons bake the track part of it into the tracks.

use super::*;
use pcb_core::smooth::{self, Finish, Finished, SmoothReport};
use std::rc::Rc;

impl BoardEditor {
    /// The copper with the board's finish applied (`None` while the finish is off). Worked out again only when
    /// the copper or the settings change.
    pub(super) fn finished(&self) -> Option<Rc<Finished>> {
        if !self.board.finish.on {
            return None;
        }
        use std::hash::{Hash, Hasher};
        let f = &self.board.finish;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        (f.corners, f.tapers, f.forks, f.radius.to_bits()).hash(&mut h);
        self.board.finish_exclude.hash(&mut h);
        let key = h.finish();
        let mut cache = self.finish_cache.borrow_mut();
        if let Some((k, v)) = cache.as_ref()
            && *k == key
        {
            return Some(v.clone());
        }
        let fresh = Rc::new(smooth::finished(&self.board)?);
        *cache = Some((key, fresh.clone()));
        Some(fresh)
    }

    pub(super) fn panel_smooth(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("配線を整える").on_hover_text("市販の基板のように、配線の角を丸め、太さが変わる所やパッドに入る根元をなだらかにし、鋭角で分かれる2本の間のくぼみの先を丸くします。「整えた形で表示・出力」を入れると、配線そのものは変えずに、表示とガーバーだけに反映します");
        let mut f: Finish = self.board.finish;
        let mut changed = false;
        changed |= ui.checkbox(&mut f.on, "整えた形で表示・出力").on_hover_text("入れると、下の設定で整えた形を表示し、ガーバーに書き出します。配線は引いたままなので、あとから配線を動かしても、整えた形が自動でついてきます。切ると元の配線の形に戻ります。DRC (発注前チェックも) は整えた形で調べ、ルール違反になる所は整えません").changed();
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut f.corners, "角を丸める").on_hover_text("同じ太さの配線がつながる角を、円弧にします。角が丸いと電流が流れやすく、高周波でも反射が減ります。直線に近い角や、短くて入らない角は小さくなるか、そのままです").changed();
            changed |= ui.add_enabled(f.corners, egui::DragValue::new(&mut f.radius).speed(0.05).range(0.1..=5.0).prefix("半径 ").suffix(" mm"))
                .on_hover_text("角の丸みの半径。配線が短いと、入る大きさまで自動で小さくなります")
                .changed();
        });
        changed |= ui.checkbox(&mut f.tapers, "太さの段差をなだらかに").on_hover_text("太い配線と細い配線がつながる所と、配線がパッド・ビアに入る根元に、中間の太さの短い配線を挟みます (パッドより細い配線は、根元がパッドの幅から配線の太さへ徐々に細くなります)。電流が急に絞られず、配線の根元が割れにくくなります").changed();
        changed |= ui.checkbox(&mut f.forks, "鋭角の分かれ目を丸める").on_hover_text("1つの点から2本の配線が鋭い角度 (8〜60°) で出ている所は、間にできる細いくぼみの先が鋭くとがっています (製造のときにエッチング液がたまりやすい所です)。そのくぼみを銅で埋めて、先を凹んだ丸い形にします。配線の経路は変わりません。これは「整えた形で表示・出力」のときだけ働きます (配線には確定できません)").changed();
        if changed {
            self.checkpoint();
            self.board.finish = f;
        }
        self.panel_finish_exclude(ui);
        let any = f.corners || f.tapers;
        if ui.add_enabled(any, egui::Button::new("整えた形を配線に確定する")).on_hover_text("角の丸みと段差を、配線そのものに書き込みます (確定すると、ほかの CAD にも渡せます。あとから配線を動かしても、丸めた角は自動では直りません)。ルール違反が増える所と、接続が切れる所は変えません。分かれ目の丸みは確定できません。元に戻すで取り消せます").clicked() {
            self.smooth_tracks();
        }
        let picked = self.picked_tracks();
        if ui
            .add_enabled(!picked.is_empty() && any, egui::Button::new(format!("選んだ配線だけ確定する ({} 本)", picked.len())))
            .on_hover_text("選択している配線 (選択ツールでクリック、またはドラッグで範囲選択) の角と、パッド・ビアに入る根元だけを、配線に書き込みます。角でつながる隣の配線も、角を丸めるぶん短くなります")
            .clicked()
        {
            self.smooth_picked(&picked);
        }
    }

    /// The nets the finishing leaves alone: a line whose shape matters (controlled impedance, a differential pair).
    fn panel_finish_exclude(&mut self, ui: &mut egui::Ui) {
        let nets = self.board_nets();
        if nets.is_empty() {
            return;
        }
        let count = self.board.finish_exclude.len();
        egui::CollapsingHeader::new(format!("整えないネット ({count})"))
            .id_salt("finish_exclude")
            .default_open(false)
            .show(ui, |ui| {
                ui.small("チェックしたネットの配線は、整えません (配線の形が大事な線を守ります)");
                let mut next = self.board.finish_exclude.clone();
                for net in &nets {
                    let mut on = next.contains(net);
                    if ui.checkbox(&mut on, net).on_hover_text("入れると、このネットの配線は、角の丸み・段差・涙型・分かれ目の丸みを付けません。形が特性を決める線 (USB・高周波・インピーダンスを決めた線など) に使います").changed() {
                        if on {
                            next.push(net.clone());
                        } else {
                            next.retain(|x| x != net);
                        }
                    }
                }
                let pairs: Vec<String> = self.board.diff_pairs.iter().flat_map(|d| [d.p.clone(), d.n.clone()]).collect();
                if !pairs.is_empty() && ui.button("差動ペアのネットを全部入れる").on_hover_text("差動ペアに登録したネット (＋側と－側) をすべて、整えないネットにします").clicked() {
                    for n in pairs {
                        if !next.contains(&n) {
                            next.push(n);
                        }
                    }
                }
                if next != self.board.finish_exclude {
                    self.checkpoint();
                    self.board.finish_exclude = next;
                }
            });
    }

    /// Select track `i` (for screenshots).
    pub fn select_track(&mut self, i: usize) {
        self.selected = (i < self.board.tracks.len()).then_some(Item::Track(i));
    }

    /// Turn the finish on or off (for screenshots).
    pub fn set_finish(&mut self, on: bool) {
        self.board.finish.on = on;
    }

    /// Indices of the selected tracks (the one selected and those in the multi-selection).
    fn picked_tracks(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.selected.iter().chain(&self.multi).filter_map(|it| if let Item::Track(i) = it { Some(*i) } else { None }).collect();
        v.sort_unstable();
        v.dedup();
        v.retain(|i| *i < self.board.tracks.len());
        v
    }

    fn smooth_picked(&mut self, picked: &[usize]) {
        self.checkpoint();
        let opts = self.board.finish;
        let r = smooth::smooth_selected(&mut self.board, &opts, picked);
        self.report_smooth(r);
    }

    pub fn smooth_tracks(&mut self) {
        self.checkpoint();
        let opts = self.board.finish;
        let r = smooth::smooth_tracks(&mut self.board, &opts);
        self.report_smooth(r);
    }

    fn report_smooth(&mut self, SmoothReport { corners, tapers, .. }: SmoothReport) {
        self.violations.clear();
        self.status = if corners + tapers == 0 {
            self.undo.pop(); // nothing changed: no undo step
            "確定できる所がありませんでした (角がない、短くて入らない、またはルール違反になる所だけです)".into()
        } else {
            format!("角を {corners} か所丸め、太さの段差を {tapers} か所なだらかにして、配線に確定しました")
        };
    }
}
