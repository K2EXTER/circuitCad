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
        let would = self.board.tracks.iter().enumerate().filter(|(i, t)| conn.track_net(*i) == Some(net) && t.width + 1e-9 < width && !pcb_core::current::is_neck(&self.board, &conn, *i) && !pcb_core::current::is_excepted(&self.board, net, *i)).count();
        if would == 0 {
            return 0;
        }
        self.checkpoint();
        let n = widen_net(&mut self.board, &conn, net, width);
        self.post_layout_fresh = false;
        self.status = format!("{net}: 配線 {n} 本を {width:.2} mm に太くしました。DRC で間隔が足りているか確かめましょう");
        n
    }

    /// Closes the net-current panel that `use_simulated_currents` asked for.
    pub fn close_width_panel(&mut self) {
        self.widths_open = None;
        self.net_table.open = false;
    }

    /// Declares the simulated net currents (from the schematic's DC analysis) and opens the panel.
    pub fn use_simulated_currents(&mut self, currents: Result<Vec<(String, f64)>, String>, stats: Vec<sim_core::NetStat>) {
        self.current_stats = stats;
        match currents {
            Err(e) => self.status = format!("シミュレーションの電流を取れませんでした: {e}"),
            Ok(list) => {
                self.checkpoint();
                let n = pcb_core::current::set_net_currents(&mut self.board, &list);
                // the currents are columns of the table of the nets
                self.net_table.open = true;
                self.status = if n == 0 {
                    "基板のネットに合う電流がありません。回路図から部品を取り込んだあとで試してください".into()
                } else {
                    format!("{n} 個のネットに、シミュレーションの電流を入れました。「必要 ○ mm」を見て、足りないネットは「推奨幅にする」で太くできます")
                };
            }
        }
    }

    pub(super) fn panel_widths(&mut self, ui: &mut egui::Ui) {
        // heading + (?) instead of a two-line paragraph; the buttons as one bar (ui-rules.md "基板パネルの帯")
        ui.horizontal(|ui| {
            crate::widgets::section_heading(ui, "電流と電圧降下");
            crate::icons::help(ui, "流れる電流から、必要な線幅を決めます。電流は、基板でネットの配線やパッドをクリックして出る窓か、「ネットの一覧」の窓で入れます");
        });
        let nets = self.board_nets();
        if nets.is_empty() {
            ui.small(i18n::tr("回路図から部品を取り込むと、ネットの一覧が出ます"));
            return;
        }
        let conn = drc::connectivity(&self.board);
        let thin = net_widths(&self.board, &conn, &CopperSpec::default()).iter().filter(|w| w.too_thin()).count();
        let bar = crate::widgets::segment_row(ui, &[("ネットの一覧", None)]);
        if bar[0].clicked() {
            self.net_table.open = true;
        }
        bar[0].clone().on_hover_text(i18n::tr("全部のネットの、配線の長さ (層ごとも)・遠回りの度合い・ビアの数・線幅・電流と推奨の線幅・ベタ・未配線の数・ネットクラスを、1つの表で見ます。回路図のDC解析・過渡解析から電流を入れることも、足りない配線を推奨幅にすることもできます。見出しのクリックで並べ替え、CSV でコピーもできます"));
        if thin > 0 {
            ui.colored_label(crate::theme::error(), i18n::trf!("電流に対して細いネット {thin} 本"));
        }
        self.ir_drop_ui(ui);
    }
}

/// A number that changes whenever the copper or the declared currents change (to tell a stale result).
pub(crate) fn copper_fingerprint(b: &Board) -> f64 {
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
        s += z.shapes.iter().flat_map(|zs| zs.shape.verts.iter()).map(|v| v.p.x * 0.3 + v.p.y * 0.4).sum::<f64>() + z.net.len() as f64 + z.shapes.len() as f64;
    }
    for (n, a) in &b.net_currents {
        s += a * 13.0 + n.len() as f64;
    }
    s
}

impl BoardEditor {
    /// "熱": the board's heat map and the thermal vias.
    pub(super) fn panel_heat(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            crate::widgets::section_heading(ui, "熱");
            crate::icons::help(ui, "部品の熱が基板のどこをどれだけ温めるかを調べます。熱の地図は回路図の「自己発熱」の電力から、銅 (ベタ・配線・ビア) が熱を広げ、表と裏の面から空気へ逃げる様子を計算します。部品どうしが近すぎるときの「離す」候補、電源を入れてからの時間の変化も出ます");
        });
        let bar = crate::widgets::segment_row(ui, &[("熱の地図…", None)]);
        if bar[0].clicked() {
            self.open_thermal_view();
        }
        bar[0].clone().on_hover_text(i18n::tr("回路図の自己発熱の電力から、基板のどこが何 ℃ になるかを計算して、サーマルカメラのような色で重ねます。ベタやビアで熱がどう広がるかが分かります"));
        let lacking = self.thermal_lacking();
        if lacking > 0
            && ui
                .button(i18n::trf!("サーマルビアを置く ({lacking} 個の放熱パッド)"))
                .on_hover_text(i18n::tr("放熱パッド (ICの裏のパッドなど) の中に、熱を裏面へ逃がすビアを格子状に置きます。そのあと、裏面に同じネット (たいてい GND) のゾーンを作ると、熱が基板全体に広がります"))
                .clicked()
        {
            self.checkpoint();
            let n = pcb_core::drc::add_thermal_vias(&mut self.board);
            self.status = if n > 0 { format!("サーマルビアを {n} 個置きました。裏面に同じネットのゾーンを作ってください") } else { "サーマルビアを置けませんでした (パッドが小さい、または他の穴に近すぎます)".into() };
        }
    }

    /// "回路図に反映": what the board carries over to the schematic's analyses, whether it is on, and the button to carry it (again) or
    /// to take it away.
    pub(super) fn panel_reflect(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            crate::widgets::section_heading(ui, "回路図に反映");
            crate::icons::help(ui, "この基板を回路図の解析に入れます。電気: 電源の入口から各ピンまでの配線の抵抗・インダクタンス・容量が、ピンの手前に直列に入ります (DC・過渡・AC・ノイズ)。熱: 自己発熱と温度の時間変化が、基板の熱の地図から部品の熱抵抗と部品どうしの温め合いを取り、配線の抵抗は配線の温度で計算します。配線していないネットは理想の線のままです");
        });
        let state = match self.post_layout_on {
            None => egui::RichText::new(i18n::tr("いまは反映していません (回路図は理想の配線)")).weak(),
            Some(fp) if (fp - copper_fingerprint(&self.board)).abs() > 1e-9 => egui::RichText::new(i18n::tr("反映中。そのあと基板が変わりました (もう一度押すと新しい配線で)")).color(crate::theme::warn()),
            Some(_) => egui::RichText::new(i18n::tr("反映中: 配線の抵抗・インダクタンス・容量、基板の熱")).color(crate::theme::ok()),
        };
        ui.label(state.small());
        let on = self.post_layout_on.is_some();
        let bar = crate::widgets::segment_row(ui, &[("シミュレーションに反映", None), ("外す", (!on).then_some("いまは反映していません"))]);
        if bar[0].clicked() {
            self.post_layout_request = Some(true);
        }
        bar[0].clone().on_hover_text(i18n::tr("この基板の配線と熱を、回路図のシミュレーションに入れます。「配線を細くしたら電圧が落ちた」「ベタを広げたら部品が冷えた」を回路図の解析で確かめられます"));
        if bar[1].clicked() {
            self.post_layout_request = Some(false);
        }
        bar[1].clone().on_hover_text(i18n::tr("配線の抵抗と基板の熱を外し、回路図を理想の配線に戻します"));
    }

    /// Declares a current for `net`, computes the drop and opens the panel (command line `--ir <net> <amps>`).
    pub fn show_ir_drop(&mut self, net: &str, amps: f64) {
        self.board.net_currents.insert(net.to_string(), amps);
        let fp = copper_fingerprint(&self.board);
        self.ir_drop = Some((fp, pcb_core::irdrop::net_drops(&self.board, &pcb_core::irdrop::IrSpec::default())));
        self.widths_open = Some(true);
    }

    /// "電圧降下" part of the width panel: the voltage the copper eats at each net's declared current.
    fn ir_drop_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(crate::theme::SPACE_S);
        ui.horizontal(|ui| {
            crate::widgets::section_heading(ui, "配線の電圧降下");
            crate::icons::help(ui, "電流を入れたネットについて、配線の抵抗による電圧降下を調べます (電源とGNDの両方を調べると、部品に届く電圧が分かります)");
        });
        let bar = crate::widgets::segment_row(ui, &[("計算する", None)]);
        if bar[0].clicked() {
            let fp = copper_fingerprint(&self.board);
            self.ir_drop = Some((fp, pcb_core::irdrop::net_drops(&self.board, &pcb_core::irdrop::IrSpec::default())));
        }
        bar[0].clone().on_hover_text(i18n::tr("上で入れた電流が流れたとき、電源の入口から各ピンまでの配線 (銅箔・ビア) で何 V 下がるかを求めます。配線を変えたら、もう一度押してください"));
        let fp = copper_fingerprint(&self.board);
        let Some((made_for, nets)) = self.ir_drop.as_ref() else {
            return;
        };
        if (made_for - fp).abs() > 1e-9 {
            ui.colored_label(crate::theme::warn(), i18n::tr("配線か電流が変わりました。「計算する」で更新してください"));
        }
        if nets.is_empty() {
            ui.small(i18n::tr("電流が入っているネットがありません (上の欄に A を入れてください)"));
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
                    let col = if v >= 0.1 { crate::theme::error() } else if v >= 0.03 { crate::theme::warn() } else { egui::Color32::from_rgb(90, 190, 110) };
                    let line = i18n::trf!("{}: 最大 {:.0} mV ({} まで, {:.0} mΩ, {:.0} nH)", nd.net, v * 1000.0, format_args!("{}.{}", w.refdes, w.pin), w.ohm.unwrap_or(0.0) * 1000.0, w.henry.unwrap_or(0.0) * 1e9);
                    let r = ui.colored_label(col, line);
                    r.on_hover_text(self.ir_detail(nd, &src));
                }
                None => {
                    ui.small(i18n::trf!("{}: 入口 {src} からつながっているピンがありません (配線が切れています)", nd.net));
                }
            }
            if nd.pads.iter().any(|p| p.ohm.is_none()) {
                let names: Vec<String> = nd.pads.iter().filter(|p| p.ohm.is_none()).map(|p| format!("{}.{}", p.refdes, p.pin)).collect();
                ui.colored_label(crate::theme::error(), i18n::trf!("  つながっていない: {}", names.join(", ")));
            }
        }
        if !nets.is_empty() {
            crate::icons::help_small(ui, "色の目安 (緑・橙・赤)", "目安: 30 mV 未満は問題なし、100 mV 以上は電源の電圧が目に見えて下がります。電源とGNDの降下は足し算で効きます");
        }
    }

    fn ir_detail(&self, nd: &pcb_core::irdrop::NetDrop, src: &str) -> String {
        let mut s = format!("電流 {:.2} A が {src}{} から入るとして、各ピンまでの抵抗と電圧降下:
", nd.amps, if nd.source_auto { " (自動で選んだ入口: コネクタがあればそこ、なければ最初のピン)" } else { "" });
        for p in &nd.pads {
            match p.ohm {
                Some(r) => s += &format!("  {}.{}  {:.1} mΩ  {:.1} mV  {:.1} nH
", p.refdes, p.pin, r * 1000.0, r * nd.amps * 1000.0, p.henry.unwrap_or(0.0) * 1e9),
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
