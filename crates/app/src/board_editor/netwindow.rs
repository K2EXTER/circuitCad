//! The window of a net: click copper of a net with the Select tool and this opens, with the current the net carries, the track width
//! that current needs, the net class of the net and its values. What the "配線の太さ" and "ネットクラス" parts of the panel do for every
//! net, for the one that was clicked.

use super::*;
use pcb_core::current::{net_widths, round_up_width, width_for_current, CopperSpec};
use pcb_core::{NetClass, WidthException};
use pcb_core::model::Role;

/// Did the slider move the value (not just rounding of what it shows)?
fn slider_changed(before: Option<f64>, after: f64) -> bool {
    before.is_none_or(|b| (b - after).abs() > 1e-9)
}

/// One of the three kinds of finishing for the nets of a class: follow the board, or on, or off.
fn finish_kind(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>, board: bool, hover: &str) {
    let text = |v: Option<bool>| match v {
        None => format!("基板に従う ({})", if board { "入" } else { "切" }),
        Some(true) => "入れる".to_string(),
        Some(false) => "切る".to_string(),
    };
    ui.horizontal(|ui| {
        ui.label(label).on_hover_text(hover);
        egui::ComboBox::from_id_salt(("net_finish", label)).selected_text(text(*value)).show_ui(ui, |ui| {
            for v in [None, Some(true), Some(false)] {
                ui.selectable_value(value, v, text(v));
            }
        }).response.on_hover_text(hover);
    });
}

/// "整え方": what the finish does on the nets of a class when it differs from the board's settings (left panel "配線を整える").
fn finish_override_ui(ui: &mut egui::Ui, o: &mut pcb_core::smooth::FinishOverride, board: &pcb_core::smooth::Finish) {
    egui::CollapsingHeader::new(if o.is_default() { i18n::tr("整え方 (基板全体の設定に従う)") } else { i18n::tr("整え方 (このクラスだけ変えている)") })
        .id_salt("net_window_finish")
        .default_open(!o.is_default())
        .show(ui, |ui| {
            ui.small(i18n::tr("配線を整える形 (角の丸み・太さの段差・分かれ目の丸み) を、このクラスのネットだけ変えます。形が特性を決める線 (USB・高周波・インピーダンスを決めた線) は、3つとも「切る」にします"));
            finish_kind(ui, i18n::tr("角を丸める"), &mut o.corners, board.corners, i18n::tr("同じ太さの配線がつながる角を、円弧にします"));
            finish_kind(ui, i18n::tr("太さの段差をなだらかに"), &mut o.tapers, board.tapers, i18n::tr("太さが変わる所と、パッド・ビアに入る根元に、中間の太さを挟みます"));
            finish_kind(ui, i18n::tr("分かれ目を丸める"), &mut o.forks, board.forks, i18n::tr("鋭い角度で分かれる2本の間のくぼみを、銅で埋めて丸くします"));
            finish_kind(ui, i18n::tr("すき間を埋める"), &mut o.slots, board.slots, i18n::tr("並んで走る2本の間の細いすき間を、銅で埋めます"));
            ui.horizontal(|ui| {
                let mut own = o.radius.is_some();
                if ui.checkbox(&mut own, i18n::tr("角の半径を変える")).on_hover_text(i18n::tr("入れると、このクラスの角の丸みの半径を、基板全体の値と別にします")).changed() {
                    o.radius = own.then_some(board.radius);
                }
                if let Some(r) = o.radius.as_mut() {
                    ui.add(egui::DragValue::new(r).speed(0.05).range(0.1..=5.0).suffix(" mm")).on_hover_text(i18n::tr("角の丸みの半径。配線が短いと、入る大きさまで自動で小さくなります"));
                }
            });
            if !o.is_default() && ui.button(i18n::tr("基板全体の設定に戻す")).on_hover_text(i18n::tr("このクラスの整え方の変更をすべてやめます")).clicked() {
                *o = Default::default();
            }
        });
}

impl BoardEditor {
    /// "線幅の例外": the stretches of this net that keep their own width (for a net with no class, `c` is the class made for it when the first
    /// stretch is added). A stretch is added from the selected tracks; its width can be changed
    /// (the tracks along it follow) and it can be taken away (the tracks keep the width they have).
    fn width_exceptions_ui(&mut self, ui: &mut egui::Ui, net: &str, c: &mut NetClass) {
        let min_w = self.board.rules.min_track_width;
        let count = c.width_exceptions.iter().filter(|e| e.net == net).count();
        // the selected tracks of this net that are not a stretch yet
        let picks: Vec<usize> = {
            let conn = self.cached_connectivity();
            let chosen = self.selected.into_iter().chain(self.multi.iter().copied());
            let mut v: Vec<usize> = chosen.filter_map(|it| if let Item::Track(i) = it { Some(i) } else { None }).filter(|&i| i < self.board.tracks.len() && conn.track_net(i) == Some(net)).collect();
            v.sort_unstable();
            v.dedup();
            v.retain(|&i| !c.width_exceptions.iter().any(|e| e.net == net && e.covers(&self.board.tracks[i])));
            v
        };
        let mut remove: Option<usize> = None;
        let mut apply: Vec<(Vec<usize>, f64)> = vec![];
        let mut add = false;
        egui::CollapsingHeader::new(i18n::trf!("線幅の例外 ({count} か所)")).id_salt("net_window_width_exceptions").default_open(count > 0).show(ui, |ui| {
            ui.small(i18n::tr("クラスの線幅とは別の太さにしたい区間 (ピッチの細い IC のピンに入る所だけ細くする、など) を登録します。登録した区間は、上のスライダー・「配線を推奨幅にする」・電流とインピーダンスのチェックの対象になりません。区間は基板上の位置で決まるので、その配線を動かすと例外から外れます"));
            for (k, e) in c.width_exceptions.iter_mut().enumerate().filter(|(_, e)| e.net == net) {
                let covered: Vec<usize> = self.board.tracks.iter().enumerate().filter(|(_, t)| e.covers(t)).map(|(i, _)| i).collect();
                ui.horizontal(|ui| {
                    ui.label(format!("({:.1}, {:.1}) → ({:.1}, {:.1})", e.a.x, e.a.y, e.b.x, e.b.y)).on_hover_text(i18n::tr("例外にした区間の両端の位置 (mm)"));
                    let before = e.width;
                    ui.add(egui::DragValue::new(&mut e.width).speed(0.01).range(min_w..=5.0).suffix(" mm")).on_hover_text(i18n::tr("この区間の線幅。変えると、区間に沿った配線の太さも変わります。発注先の最小線幅より細くはできません"));
                    if (e.width - before).abs() > 1e-9 {
                        apply.push((covered.clone(), e.width));
                    }
                    if covered.is_empty() {
                        ui.small(i18n::tr("該当する配線なし")).on_hover_text(i18n::tr("この区間に沿った配線が、いまはありません (配線を動かしたか消しました)。不要なら × で消します"));
                    } else {
                        ui.small(i18n::trf!("{} 本", covered.len()));
                    }
                    if ui.small_button("×").on_hover_text(i18n::tr("例外をやめます。配線の太さは、いまのままです")).clicked() {
                        remove = Some(k);
                    }
                });
            }
            if ui.add_enabled(!picks.is_empty(), egui::Button::new(i18n::trf!("選んだ配線を例外にする ({} 本)", picks.len()))).on_hover_text(i18n::tr("基板で選んだ、このネットの配線を、いまの太さのまま例外にします。選択ツールで配線をクリック (Shift で複数) してから押します")).clicked() {
                add = true;
            }
        });
        if let Some(k) = remove {
            c.width_exceptions.remove(k);
        }
        if add {
            for &i in &picks {
                let t = &self.board.tracks[i];
                c.width_exceptions.push(WidthException { net: net.to_string(), a: t.a, b: t.b, width: t.width });
            }
            self.status = format!("ネット {net}: 配線 {} 本を線幅の例外にしました", picks.len());
        }
        if !apply.is_empty() {
            self.checkpoint_once();
            for (tracks, w) in apply {
                for i in tracks {
                    self.board.tracks[i].width = w;
                }
            }
        }
    }

    /// Put the classes in the board when they differ (one undo step for a whole drag of a value).
    pub(super) fn commit_classes(&mut self, classes: Vec<NetClass>, pointer_down: bool) {
        if classes != self.board.net_classes {
            self.checkpoint_once();
            self.board.net_classes = classes;
        }
        if !pointer_down {
            self.drag_edit = false;
        }
    }

    /// Open the window of `net` (command line and tests; a click on the net's copper does the same).
    pub fn open_net_window(&mut self, net: &str) {
        self.net_win = Some(net.to_string());
    }

    /// The tab of the net window (command line): 0 width, 1 class, 2 finish.
    pub fn set_net_tab(&mut self, tab: u8) {
        self.net_tab = tab.min(2);
    }

    /// The right-click menu on copper of a net (Select tool).
    pub(super) fn net_menu_ui(&mut self, ui: &mut egui::Ui) {
        let Some(net) = self.net_menu.clone() else { return };
        ui.label(egui::RichText::new(self.net_caption(&net)).strong());
        ui.separator();
        if ui.button(i18n::tr("ネットの設定…")).on_hover_text(i18n::tr("このネットの電流・推奨の線幅・ネットクラス (線幅・間隔・ビア・整え方) の窓を開きます")).clicked() {
            self.open_net_window(&net);
            ui.close();
        }
    }

    pub fn net_window(&mut self, ctx: &egui::Context) {
        let Some(net) = self.net_win.clone() else {
            self.net_win_asked = None;
            return;
        };
        if !self.board.pads.iter().any(|p| p.net == net) {
            self.net_win = None;
            return;
        }
        let mut open = true;
        let down = ctx.input(|i| i.pointer.any_down());
        let spec = CopperSpec::default();
        egui::Window::new(i18n::trf!("ネット {net} の設定")).id(egui::Id::new("net_window")).open(&mut open).default_width(360.0).default_pos([420.0, 120.0]).show(ctx, |ui| {
            ui.small(self.net_caption(&net));
            // --- the class of the net: on view in every tab
            let rules = self.board.rules.clone();
            let mut classes = self.board.net_classes.clone();
            let current = classes.iter().position(|c| c.nets.contains(&net));
            let label = current.map_or("なし (基板全体の設定)".to_string(), |k| classes[k].name.clone());
            let mut choose: Option<Option<usize>> = None;
            ui.horizontal(|ui| {
                ui.label(i18n::tr("クラス"));
                egui::ComboBox::from_id_salt("net_window_class").selected_text(label).show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_none(), i18n::tr("なし (基板全体の設定)")).clicked() {
                        choose = Some(None);
                    }
                    for (k, c) in classes.iter().enumerate() {
                        if ui.selectable_label(current == Some(k), &c.name).clicked() {
                            choose = Some(Some(k));
                        }
                    }
                }).response.on_hover_text(i18n::tr("このネットを入れるクラスを選びます。クラスの線幅・間隔・ビアで、このネットの配線を引き、DRC がチェックします"));
            });
            if let Some(c) = choose {
                for cl in classes.iter_mut() {
                    cl.nets.retain(|n| *n != net);
                }
                if let Some(k) = c {
                    classes[k].nets.push(net.clone());
                }
            }
            if let Some(k) = classes.iter().position(|c| c.nets.contains(&net)) {
                let others: Vec<String> = classes[k].nets.iter().filter(|n| **n != net).cloned().collect();
                if !others.is_empty() {
                    ui.small(i18n::trf!("この値は、同じクラスのネット ({}) にも効きます", others.join("・")));
                }
            }
            ui.add_space(4.0);
            crate::widgets::tab_bar(ui, &mut self.net_tab, &[
                (0, "線幅", "電流と推奨の線幅、線幅のスライダー、インピーダンス、例外の区間"),
                (1, "クラス", "役割、間隔、ビア。このネットの配線ルール"),
                (2, "整え方", "角の丸み・太さの段差・分かれ目の丸み。このクラスだけ基板全体の設定と変えるとき"),
            ]);
            let mut set_width: Option<f64> = None;
            let mut idx: Vec<usize> = vec![];
            // the width the slider, a button or the impedance asks for this frame
            let mut new_width: Option<f64> = None;
            let mut new_target: Option<f64> = None;
            match self.net_tab {
                0 => {
                    // --- the current and the width it needs
                    let mut amps = self.board.net_currents.get(&net).copied().unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.label(i18n::tr("流れる電流"));
                        ui.add(egui::DragValue::new(&mut amps).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(2)).on_hover_text(i18n::tr("このネットに流れる最大の電流。分からなければ、つなぐ部品のデータシートの最大電流を見ます。0 のままなら、推奨の線幅は出しません"));
                    });
                    if amps > 0.0 {
                        self.board.net_currents.insert(net.clone(), amps);
                    } else {
                        self.board.net_currents.remove(&net);
                        // no current yet: the schematic's DC analysis is asked for this net's current, once (at once when the answer is at hand;
                        // else on a button, so that the window does not wait for a solve of seconds)
                        if self.net_win_asked.as_deref() != Some(net.as_str()) {
                            if self.dc_currents_ready {
                                self.net_win_asked = Some(net.clone());
                                self.net_current_request = Some(net.clone());
                            } else if ui.button(i18n::tr("回路図の DC 解析で電流を調べる")).on_hover_text(i18n::tr("回路図のこの回路を DC で解いて、このネットに流れる電流を入れます。回路が大きいと数秒かかります (一度解くと、回路図を変えるまで、ほかのネットはすぐ出ます)")).clicked() {
                                self.net_win_asked = Some(net.clone());
                                self.net_current_request = Some(net.clone());
                            }
                        }
                    }
                    if let Some((n, text)) = &self.net_win_note
                        && *n == net
                    {
                        ui.small(text.as_str());
                    }
                    if let Some(s) = self.current_stats.iter().find(|s| s.net == net).cloned() {
                        ui.horizontal_wrapped(|ui| {
                            ui.small(i18n::trf!("過渡解析: 実効 {:.3} A / ピーク {:.3} A{}", s.rms, s.peak, if s.spiky() { " ⚠ ピークが実効値の10倍以上" } else { "" }));
                            if (s.rms - amps).abs() > 1e-6 && s.rms > 1e-6 && ui.small_button(i18n::tr("実効値を入れる")).on_hover_text(i18n::tr("過渡解析の実効値を、このネットの電流にします")).clicked() {
                                amps = s.rms;
                                self.board.net_currents.insert(net.clone(), amps);
                            }
                        });
                    }
                    let mut widen = false;
                    if amps > 0.0 {
                        let rec = round_up_width(width_for_current(amps, &spec));
                        let conn = self.cached_connectivity();
                        let now = net_widths(&self.board, &conn, &spec).into_iter().find(|w| w.net == net).and_then(|w| w.thinnest);
                        ui.label(egui::RichText::new(i18n::trf!("推奨の線幅: {rec:.2} mm")).strong()).on_hover_text(i18n::tr("銅箔 35µm・温度上昇 10℃ の外層で、この電流を流せる太さ (IPC-2221 の式) を、0.05 mm 単位に切り上げたものです。内層や、厚い銅箔、温度上昇を許す場合は、もっと細くできます"));
                        match now {
                            None => {
                                ui.small(i18n::tr("このネットの配線は、まだありません。配線するときの線幅に使えます"));
                            }
                            Some(t) if t + 1e-9 < rec => {
                                ui.colored_label(crate::theme::ERROR, i18n::trf!("今の最も細い配線は {t:.2} mm で、足りません"));
                            }
                            Some(t) => {
                                ui.colored_label(Color32::from_rgb(90, 190, 110), i18n::trf!("今の最も細い配線は {t:.2} mm で、足ります ✔"));
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if now.is_some_and(|t| t + 1e-9 < rec) && ui.button(i18n::tr("配線を推奨幅にする")).on_hover_text(i18n::tr("このネットの、推奨より細い配線を、推奨の太さまで太くします (元に戻せます)。太くしたあと、DRC で間隔が足りているか確かめましょう")).clicked() {
                                widen = true;
                            }
                            if ui.button(i18n::tr("この幅で引く")).on_hover_text(i18n::tr("これから引く、このネットの配線の線幅を、推奨の太さにします (ネットクラスがあれば、そのクラスの線幅を変えます)")).clicked() {
                                set_width = Some(rec);
                            }
                        });
                    } else {
                        ui.small(i18n::tr("電流を入れると、推奨の線幅が出ます:"));
                        ui.horizontal(|ui| {
                            for a in [0.5, 1.0, 2.0, 3.0] {
                                if ui.small_button(format!("{a} A")).on_hover_text(i18n::tr("この電流を入れます")).clicked() {
                                    self.board.net_currents.insert(net.clone(), a);
                                }
                            }
                        });
                    }
                    if widen {
                        self.widen_net_to_need(&net);
                    }
                    // --- the one width of the net: its tracks follow the slider while it is dragged (one undo step), and so does the class
                    ui.separator();
                    ui.strong(i18n::tr("線幅"));
                    idx = {
                        let conn = self.cached_connectivity();
                        // (the necks of a trunk keep their width: they are thin so that the solder stays on the small pads; so do the stretches
                        // the class lets keep their own)
                        (0..self.board.tracks.len()).filter(|&i| conn.track_net(i) == Some(net.as_str()) && !pcb_core::current::is_neck(&self.board, &conn, i) && !pcb_core::current::is_excepted(&self.board, &net, i)).collect()
                    };
                    let class_k = classes.iter().position(|c| c.nets.contains(&net));
                    let start = class_k.map(|k| classes[k].track_width).or_else(|| idx.iter().map(|&i| self.board.tracks[i].width).reduce(f64::max));
                    let rec = self.board.net_currents.get(&net).map(|a| round_up_width(width_for_current(*a, &spec)));
                    match start {
                        None => {
                            ui.small(i18n::tr("このネットには、配線もクラスもまだありません。「クラス」タブの「このネット専用のクラスを作る」か、配線を引くと、スライダーが使えます"));
                        }
                        Some(mut v) => {
                            let hi = rec.map_or(3.0, |r| (r * 2.0).max(3.0));
                            let slider = ui.add(egui::Slider::new(&mut v, rules.min_track_width..=hi).logarithmic(true).fixed_decimals(2).suffix(" mm"));
                            slider.on_hover_text(i18n::tr("ドラッグすると、このネットの配線の太さが、動かした値にそろって変わります (配線がなければ、これから引く線の太さ)。ネットクラスに入っていれば、クラスの線幅も変わります。元に戻すで、ドラッグの前に戻せます"));
                            if slider_changed(start, v) {
                                new_width = Some(v);
                            }
                            if let Some(r) = rec {
                                if v + 1e-9 >= r {
                                    ui.colored_label(Color32::from_rgb(90, 190, 110), i18n::trf!("推奨 {r:.2} mm 以上で、足ります ✔"));
                                } else {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(crate::theme::ERROR, i18n::trf!("推奨 {r:.2} mm に足りません"));
                                        if ui.small_button(i18n::tr("推奨まで上げる")).on_hover_text(i18n::tr("スライダーを、推奨の線幅にします")).clicked() {
                                            new_width = Some(r);
                                        }
                                    });
                                }
                            } else {
                                ui.small(i18n::tr("電流を入れると、推奨の線幅と比べて、足りているか出ます"));
                            }
                            // --- the impedance of a track of this width (a rule of thumb), and the width for a wanted one
                            ui.separator();
                            ui.strong(i18n::tr("インピーダンス (目安)"));
                            let side = idx.first().map_or(self.side, |&i| self.board.tracks[i].side);
                            let kind = pcb_core::impedance::Kind::of(side);
                            let shown = new_width.unwrap_or(v);
                            let z = pcb_core::impedance::single_ended(&self.board.stackup, kind, shown);
                            ui.label(egui::RichText::new(i18n::trf!("線幅 {shown:.2} mm → 約 {z:.0} Ω")).strong()).on_hover_text(i18n::trf!("{}。教科書の式による目安で、±10% ほどの誤差があります。発注先の積層構成 (基板の厚さ・層の間隔・誘電率) に合わせた値が正確です。左パネルの「層構成 (層数・材料)」で、その値を入れると近づきます", kind.label()));
                            let mut target = class_k.and_then(|k| classes[k].impedance).unwrap_or(self.z_target);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(i18n::tr("目標"));
                                if ui.add(egui::DragValue::new(&mut target).speed(0.5).range(10.0..=200.0).suffix(" Ω")).on_hover_text(i18n::tr("このネットの配線を、この特性インピーダンスにしたい値。USB や高速信号は 50 Ω (差動なら 90 Ω や 100 Ω) などです")).changed() {
                                    self.z_target = target;
                                    if class_k.is_some_and(|k| classes[k].impedance.is_some()) {
                                        new_target = Some(target);
                                    }
                                }
                                if ui.button(i18n::tr("目標の線幅にする")).on_hover_text(i18n::tr("目標のインピーダンスになる線幅を、層構成から求めて、このネットの配線とクラスの線幅にします (クラスがなければ作ります)。そのあと、発注前チェックが、目標から外れた配線を教えます")).clicked() {
                                    match pcb_core::impedance::width_for(&self.board.stackup, kind, target) {
                                        Some(w) if w >= rules.min_track_width => {
                                            new_width = Some((w * 1000.0).round() / 1000.0);
                                            new_target = Some(target);
                                        }
                                        Some(w) => self.status = format!("目標 {target:.0} Ω の線幅は {w:.3} mm で、基板の最小線幅 {:.2} mm より細くなります。層の間隔を薄くするか、目標を上げてください", rules.min_track_width),
                                        None => self.status = format!("この層構成では、{target:.0} Ω にできる線幅が見つかりません。左パネルの「層構成 (層数・材料)」の値を確かめてください"),
                                    }
                                }
                            });
                            if let Some(k) = class_k
                                && let Some(zt) = classes[k].impedance
                                && (z - zt).abs() / zt > 0.1
                            {
                                ui.colored_label(crate::theme::WARN, i18n::trf!("クラスの目標 {zt:.0} Ω から {:.0}% ずれています", (z - zt).abs() / zt * 100.0));
                            }
                        }
                    }
                    // --- the stretches that keep their own width
                    ui.separator();
                    match class_k {
                        Some(k) => self.width_exceptions_ui(ui, &net, &mut classes[k]),
                        None => {
                            // no class yet: one for this net alone is made when the first stretch is added (what "このネット専用のクラスを作る" makes)
                            let width = self.board.net_currents.get(&net).map(|a| round_up_width(width_for_current(*a, &spec))).unwrap_or(self.track_width).max(rules.min_track_width);
                            let mut own = NetClass { name: net.clone(), nets: vec![net.clone()], track_width: width, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill, ..Default::default() };
                            self.width_exceptions_ui(ui, &net, &mut own);
                            if !own.width_exceptions.is_empty() {
                                classes.push(own);
                            }
                        }
                    }
                }
                1 => {
                    // --- the rules of the class
                    match classes.iter().position(|c| c.nets.contains(&net)) {
                        Some(k) => {
                            let c = &mut classes[k];
                            ui.horizontal(|ui| {
                                ui.label(i18n::tr("役割"));
                                egui::ComboBox::from_id_salt("net_window_role").selected_text(c.role.label()).show_ui(ui, |ui| {
                                    for r in [Role::None, Role::Power, Role::Switching, Role::Sense, Role::Analog, Role::Logic] {
                                        ui.selectable_value(&mut c.role, r, r.label());
                                    }
                                }).response.on_hover_text(i18n::tr("このクラスのネットの役割です。役割が変えるのは自動配線だけです: 役割の順 (大電流 → スイッチングノード → 検出 → アナログ → ロジック) に先に引き (先に引くほど短く・まっすぐ通りやすい)、「検出」のネットはビアを使わず、パッドと同じ層だけで引きます。線幅は変わりません (太さはクラスの線幅とネットの電流で決まります)。DRC と自動配置は役割を見ません"));
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::DragValue::new(&mut c.clearance).speed(0.01).range(rules.min_clearance..=5.0).prefix(i18n::tr("間隔 ")).suffix(" mm")).on_hover_text(i18n::tr("ほかのネットの銅が、このクラスの銅から離れる距離"));
                                ui.add(egui::DragValue::new(&mut c.via_diameter).speed(0.05).range(0.3..=3.0).prefix(i18n::tr("ビア径 ")).suffix(" mm")).on_hover_text(i18n::tr("このクラスのビアの外径"));
                                ui.add(egui::DragValue::new(&mut c.via_drill).speed(0.05).range(0.2..=2.0).prefix(i18n::tr("穴 ")).suffix(" mm")).on_hover_text(i18n::tr("このクラスのビアの穴径"));
                            });
                            c.via_drill = c.via_drill.min(c.via_diameter - 0.1).max(0.2);
                            ui.small(i18n::trf!("線幅は「線幅」タブのスライダーで変えます (いま {:.2} mm)", c.track_width));
                        }
                        None => {
                            ui.small(i18n::trf!("いまは、基板全体の設定 (線幅 {:.2} mm、ビア {:.2}/{:.2} mm) で引きます", self.track_width, self.via_dia, self.via_drill));
                            if ui.button(i18n::tr("このネット専用のクラスを作る")).on_hover_text(i18n::tr("このネットだけのクラスを作って、入れます。線幅は、推奨の線幅 (電流を入れていれば) か、いまの設定の線幅です。値は、あとで直せます")).clicked() {
                                let width = self.board.net_currents.get(&net).map(|a| round_up_width(width_for_current(*a, &spec))).unwrap_or(self.track_width).max(rules.min_track_width);
                                classes.push(NetClass { name: net.clone(), nets: vec![net.clone()], track_width: width, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill, ..Default::default() });
                            }
                        }
                    }
                }
                _ => match classes.iter().position(|c| c.nets.contains(&net)) {
                    Some(k) => finish_override_ui(ui, &mut classes[k].finish, &self.board.finish),
                    None => {
                        ui.small(i18n::tr("整え方は、クラスごとに決めます。「クラス」タブで、このネット専用のクラスを作ってください"));
                    }
                },
            }
            // --- what was asked this frame
            let class_k = classes.iter().position(|c| c.nets.contains(&net));
            if let Some(w) = set_width {
                match class_k {
                    Some(k) => classes[k].track_width = w,
                    None => {
                        self.track_width = w;
                        self.status = format!("線幅を {w:.2} mm にしました");
                    }
                }
            }
            if let Some(v) = new_width {
                self.checkpoint_once();
                for &i in &idx {
                    self.board.tracks[i].width = v;
                }
                let k = match class_k {
                    Some(k) => k,
                    None if new_target.is_some() => {
                        classes.push(NetClass { name: net.clone(), nets: vec![net.clone()], track_width: v, clearance: rules.min_clearance, via_diameter: self.via_dia, via_drill: self.via_drill, ..Default::default() });
                        classes.len() - 1
                    }
                    None => usize::MAX,
                };
                if let Some(c) = classes.get_mut(k) {
                    c.track_width = v;
                }
                self.status = format!("ネット {net} の線幅: {v:.2} mm ({} 本)", idx.len());
            }
            if let Some(zt) = new_target
                && let Some(c) = classes.iter_mut().find(|c| c.nets.contains(&net))
            {
                c.impedance = Some(zt);
            }
            self.commit_classes(classes, down);
            ui.separator();
            crate::icons::help_small(ui, "ほかのネットに切り替えるには", "基板で、別のネットの配線やパッドをクリックすると、そのネットの設定に切り替わります。何もない所をクリックすると閉じます");
        });
        if !open {
            self.net_win = None;
        }
    }
}
