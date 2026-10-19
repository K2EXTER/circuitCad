//! "配線の太さ" section of the board panel: the current each net carries, the track width that
//! needs (IPC-2221), and a button that widens the tracks to it.

use super::*;
use pcb_core::current::{net_widths, round_up_width, widen_net, CopperSpec};

impl BoardEditor {
    /// Net names that appear on pads, in order of first appearance.
    pub(super) fn board_nets(&self) -> Vec<String> {
        let mut nets: Vec<String> = vec![];
        for p in &self.board.pads {
            if !p.net.is_empty() && !nets.contains(&p.net) {
                nets.push(p.net.clone());
            }
        }
        nets
    }

    /// Widens every track of `net` to the width its declared current needs. Returns how many were changed.
    pub(crate) fn widen_net_to_need(&mut self, net: &str) -> usize {
        let Some(&amps) = self.board.net_currents.get(net) else { return 0 };
        let width = round_up_width(pcb_core::current::width_for_current(amps, &CopperSpec::default()));
        let conn = drc::connectivity(&self.board);
        let would = self.board.tracks.iter().enumerate().filter(|(i, t)| conn.track_net(*i) == Some(net) && t.width + 1e-9 < width).count();
        if would == 0 {
            return 0;
        }
        self.checkpoint();
        let n = widen_net(&mut self.board, &conn, net, width);
        self.post_layout_fresh = false;
        self.status = format!("{net}: 配線 {n} 本を {width:.2} mm に太くしました。DRC で間隔が足りているか確かめましょう");
        n
    }

    /// Declares the simulated net currents (from the schematic's DC analysis) and opens the panel.
    pub fn use_simulated_currents(&mut self, currents: Result<Vec<(String, f64)>, String>, stats: Vec<sim_core::NetStat>) {
        self.current_stats = stats;
        match currents {
            Err(e) => self.status = format!("シミュレーションの電流を取れませんでした: {e}"),
            Ok(list) => {
                self.checkpoint();
                let n = pcb_core::current::set_net_currents(&mut self.board, &list);
                self.widths_open = Some(true);
                self.status = if n == 0 {
                    "基板のネットに合う電流がありません。回路図から部品を取り込んだあとで試してください".into()
                } else {
                    format!("{n} 個のネットに、シミュレーションの電流を入れました。「必要 ○ mm」を見て、足りないネットは「推奨幅にする」で太くできます")
                };
            }
        }
    }

    pub(super) fn panel_widths(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("配線の太さ (電流から)").default_open(false).open(self.widths_open.take()).show(ui, |ui| {
            let nets = self.board_nets();
            if nets.is_empty() {
                ui.small("回路図から部品を取り込むと、ネットの一覧が出ます");
                return;
            }
            let spec = CopperSpec::default();
            let conn = drc::connectivity(&self.board);
            let report = net_widths(&self.board, &conn, &spec);
            let mut widen: Vec<String> = vec![];
            let mut use_width = None;
            egui::Grid::new("net_widths").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for net in &nets {
                    ui.label(net);
                    let mut a = self.board.net_currents.get(net).copied().unwrap_or(0.0);
                    let r = ui.add(egui::DragValue::new(&mut a).speed(0.05).range(0.0..=30.0).suffix(" A").max_decimals(2));
                    r.on_hover_text("このネットに流れる最大の電流。0 のままなら調べません。分からなければ、つなぐ部品のデータシートの最大電流を見ます");
                    if a > 0.0 {
                        self.board.net_currents.insert(net.clone(), a);
                    } else {
                        self.board.net_currents.remove(net);
                    }
                    match report.iter().find(|w| &w.net == net) {
                        Some(w) => {
                            let rec = round_up_width(w.needed);
                            ui.horizontal(|ui| {
                                let (txt, col) = match w.thinnest {
                                    None => (format!("必要 {rec:.2} mm (配線なし)"), egui::Color32::from_gray(160)),
                                    Some(t) if w.too_thin() => (format!("必要 {rec:.2} mm / 今 {t:.2} mm"), egui::Color32::from_rgb(230, 90, 70)),
                                    Some(t) => (format!("必要 {rec:.2} mm / 今 {t:.2} mm ✔"), egui::Color32::from_rgb(90, 190, 110)),
                                };
                                ui.colored_label(col, txt).on_hover_text(format!("IPC-2221 の式で、{:.2} A を温度上昇 10℃ に収めて流すのに必要な幅は {:.3} mm です。丸めて {rec:.2} mm", w.amps, w.needed));
                                if w.too_thin() && ui.small_button("推奨幅にする").on_hover_text("このネットの細い配線を、必要な幅まで太くします (Ctrl+Z で戻せます)。太くした結果、隣と近づきすぎることがあるので、あとで DRC を実行してください").clicked() {
                                    widen.push(net.clone());
                                }
                                if ui.small_button("線幅に").on_hover_text("これから引く配線の線幅 (上の「線幅」) を、この値にします").clicked() {
                                    use_width = Some(rec);
                                }
                            });
                        }
                        None => {
                            ui.small("");
                        }
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
            if ui.button("DC解析の電流を入れる").on_hover_text("回路図のDC解析 (動作点) で各ネットに流れる電流を求めて、上の欄に入れます。ネットにつながる部品の電流を足して半分にした値で、電源とGNDには全体の電流が出ます。電流を自分で調べなくても、必要な線幅がすぐ分かります。ピーク電流 (起動時やスイッチング) は入らないので、余裕を見てください").clicked() {
                self.currents_request = Some(CurrentSource::Dc);
            }
            if ui.button("過渡解析の電流を入れる").on_hover_text("回路図で最後に実行した過渡解析の、後半 (起動のあとの落ち着いた部分) の電流の実効値 (RMS) を、各ネットに入れます。スイッチング電源やパルスの負荷のように、DCでは電流が決まらない回路用です。銅は数秒かけて温まるので、短いスパイクではなく実効値で太さを決めます。起動時のサージは含めません。先に回路図で過渡解析を実行してください").clicked() {
                self.currents_request = Some(CurrentSource::Tran);
            }
            });
            for s in &self.current_stats {
                let txt = format!("{}: 実効 {:.2} A / 10 ms の最大 {:.2} A / ピーク {:.2} A{}", s.net, s.rms, s.burst_rms, s.peak, if s.spiky() { "  ⚠ ピークが実効値の10倍以上" } else { "" });
                let r = if s.spiky() { ui.colored_label(egui::Color32::from_rgb(220, 160, 40), txt) } else { ui.small(txt) };
                r.on_hover_text("太さに使うのは「実効」です。「10 ms の最大」は短くても繰り返す山の熱、「ピーク」は一瞬の最大値で、参考です。⚠ は、起動時のサージやスパイクかもしれません。実物では電源の内部抵抗・ESR・ソフトスタートが抑えるので、そのまま太さにすると必要以上に太くなります");
            }
            self.ir_drop_ui(ui);
            if let Some(w) = use_width {
                self.track_width = w;
            }
            for n in widen {
                self.widen_net_to_need(&n);
            }
            if report.iter().any(|w| w.too_thin()) && ui.button("足りないネットをすべて推奨幅にする").on_hover_text("電流に対して細いネットの配線を、すべて必要な太さまで太くします (Ctrl+Z で戻せます)。太くした結果、配線の間隔が足りるか DRC で確かめましょう").clicked() {
                let thin: Vec<String> = report.iter().filter(|w| w.too_thin()).map(|w| w.net.clone()).collect();
                for n in thin {
                    self.widen_net_to_need(&n);
                }
            }
        }).header_response.on_hover_text("ネットごとに流す電流を入れると、必要な線幅を計算し、足りない配線を教えます (銅箔 35µm・温度上昇 10℃ の外層)。電源やモーターの線は、信号線より太くします");
    }
}

/// A number that changes whenever the copper or the declared currents change (to tell a stale result).
fn copper_fingerprint(b: &Board) -> f64 {
    let mut s = 0.0;
    for (i, t) in b.tracks.iter().enumerate() {
        s += (i as f64 + 1.0) * (t.a.x * 1.3 + t.a.y * 1.7 + t.b.x * 2.1 + t.b.y * 2.9 + t.width * 7.0 + if t.side == Side::Top { 0.0 } else { 3.3 });
    }
    for (i, v) in b.vias.iter().enumerate() {
        s += (i as f64 + 1.0) * (v.pos.x * 0.7 + v.pos.y * 1.1 + v.drill);
    }
    for (i, p) in b.pads.iter().enumerate() {
        s += (i as f64 + 1.0) * (p.pos.x * 0.9 + p.pos.y * 1.9) + p.net.len() as f64 * 0.01;
    }
    for z in &b.zones {
        s += z.polygon.iter().map(|p| p.x * 0.3 + p.y * 0.4).sum::<f64>() + z.net.len() as f64;
    }
    for (n, a) in &b.net_currents {
        s += a * 13.0 + n.len() as f64;
    }
    s
}

impl BoardEditor {
    /// Declares a current for `net`, computes the drop and opens the panel (command line `--ir <net> <amps>`).
    pub fn show_ir_drop(&mut self, net: &str, amps: f64) {
        self.board.net_currents.insert(net.to_string(), amps);
        let fp = copper_fingerprint(&self.board);
        self.ir_drop = Some((fp, pcb_core::irdrop::net_drops(&self.board, &pcb_core::irdrop::IrSpec::default())));
        self.widths_open = Some(true);
    }

    /// "電圧降下" part of the width panel: the voltage the copper eats at each net's declared current.
    fn ir_drop_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("配線の電圧降下").strong());
            if ui.button("計算する").on_hover_text("上で入れた電流が流れたとき、電源の入口から各ピンまでの配線 (銅箔・ビア) で何 V 下がるかを求めます。配線を変えたら、もう一度押してください").clicked() {
                let fp = copper_fingerprint(&self.board);
                self.ir_drop = Some((fp, pcb_core::irdrop::net_drops(&self.board, &pcb_core::irdrop::IrSpec::default())));
            }
        });
        ui.horizontal(|ui| {
            if ui.button("シミュレーションに反映").on_hover_text("この基板の配線の抵抗を、回路図のシミュレーション (DC・過渡・AC・ノイズ) に入れます。電源の入口から各ピンまでの銅箔の抵抗が、ピンの手前に直列に入ります。「配線を細くしたら電圧が落ちた」を波形で確かめられます。配線していないネットは理想の線のままです").clicked() {
                self.post_layout_request = Some(true);
            }
            if ui.small_button("やめる").on_hover_text("配線の抵抗を入れない理想の回路に戻します").clicked() {
                self.post_layout_request = Some(false);
            }
        });
        let fp = copper_fingerprint(&self.board);
        let Some((made_for, nets)) = self.ir_drop.as_ref() else {
            ui.small("電流を入れたネットについて、配線の抵抗による電圧降下を調べます (電源とGNDの両方を調べると、部品に届く電圧が分かります)");
            return;
        };
        if (made_for - fp).abs() > 1e-9 {
            ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "配線か電流が変わりました。「計算する」で更新してください");
        }
        if nets.is_empty() {
            ui.small("電流が入っているネットがありません (上の欄に A を入れてください)");
        }
        for nd in nets {
            if let Some(note) = &nd.note {
                ui.small(format!("{}: {note}", nd.net));
                continue;
            }
            let src = format!("{}.{}", nd.source.0, nd.source.1);
            match nd.worst() {
                Some(w) => {
                    let v = w.drop_v.unwrap_or(0.0);
                    let col = if v >= 0.1 { egui::Color32::from_rgb(230, 70, 60) } else if v >= 0.03 { egui::Color32::from_rgb(220, 160, 40) } else { egui::Color32::from_rgb(90, 190, 110) };
                    let line = format!("{}: 最大 {:.0} mV ({} まで, {:.0} mΩ)", nd.net, v * 1000.0, format_args!("{}.{}", w.refdes, w.pin), w.ohm.unwrap_or(0.0) * 1000.0);
                    let r = ui.colored_label(col, line);
                    r.on_hover_text(self.ir_detail(nd, &src));
                }
                None => {
                    ui.small(format!("{}: 入口 {src} からつながっているピンがありません (配線が切れています)", nd.net));
                }
            }
            if nd.pads.iter().any(|p| p.ohm.is_none()) {
                let names: Vec<String> = nd.pads.iter().filter(|p| p.ohm.is_none()).map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                ui.colored_label(egui::Color32::from_rgb(230, 70, 60), format!("  つながっていない: {}", names.join(", ")));
            }
        }
        ui.small("目安: 30 mV 未満は問題なし、100 mV 以上は電源の電圧が目に見えて下がります。電源とGNDの降下は足し算で効きます");
    }

    fn ir_detail(&self, nd: &pcb_core::irdrop::NetDrop, src: &str) -> String {
        let mut s = format!("電流 {:.2} A が {src}{} から入るとして、各ピンまでの抵抗と電圧降下:
", nd.amps, if nd.source_auto { " (自動で選んだ入口: コネクタがあればそこ、なければ最初のピン)" } else { "" });
        for p in &nd.pads {
            match p.ohm {
                Some(r) => s += &format!("  {}.{}  {:.1} mΩ  {:.1} mV
", p.refdes, p.pin, r * 1000.0, r * nd.amps * 1000.0),
                None => s += &format!("  {}.{}  つながっていません
", p.refdes, p.pin),
            }
        }
        s += "各ピンが電流の全部を1つで引くとした、いちばん厳しい見積もりです。";
        if nd.has_zone {
            s += "
ベタ (銅の面) は、抵抗の小さい面としておおまかに入れています。";
        }
        s += "
銅は 35µm、ビアは壁の厚み 25µm の円筒として計算しています";
        s
    }
}
