//! The meter (volts, amps, ohms between two probes) and the scope (a probe on a hole: its net's waveform).

use super::*;

impl BreadboardView {
    /// The meter's reading: the DC voltage of the red probe's net against the black's.
    pub(super) fn meter_ui(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &Layout, matches: bool) {
        let probe = |h: Option<Hole>| h.map(|h| (super::hole_text(h), net_at(&sch.sch, l, h)));
        let (red, black) = (probe(self.red), probe(self.black));
        self.meter_panel(ui, sch, &RunBoard::Bread(l.clone()), red, black, matches);
    }

    /// The meter's ranges and its reading for the probes on the board (`board`: the breadboard, or a stripboard or perfboard).
    pub(super) fn meter_panel(&mut self, ui: &mut egui::Ui, sch: &SchEditor, board: &RunBoard, red: Option<Probe>, black: Option<Probe>, matches: bool) {
        {
            const RANGES: [(MeterRange, &str, &str); 3] = [
                (MeterRange::Volts, "電圧 V", "2 本の棒の間の直流の電圧を読みます。部品の両端に当てます (並列)。黒い棒はふつう GND に"),
                (MeterRange::Amps, "電流 A", "テスターに電流を通して読みます。回路の線を 1 か所外し、その両端に棒を当てます (直列)。電源の + と − に当てると、テスターがショートになってヒューズが切れます"),
                (MeterRange::Ohms, "抵抗 Ω", "電源を切って、2 本の棒の間の抵抗を読みます。テスターが小さな電流を流して測るので、電源が入っていると正しく読めません。部品を回路につないだままだと、まわりの部品も一緒に測ります"),
            ];
            let items: Vec<(&str, Option<&str>)> = RANGES.iter().map(|(_, name, _)| (*name, None)).collect();
            let now = RANGES.iter().position(|(r, ..)| *r == self.meter_range);
            for (resp, (r, _, tip)) in crate::widgets::segment_bar(ui, &items, now).into_iter().zip(RANGES) {
                if resp.on_hover_text(i18n::tr(tip)).clicked() {
                    self.meter_range = r;
                }
            }
        }
        if self.meter_range != MeterRange::Volts {
            self.meter_other_range(ui, sch, board, red, black);
            return;
        }
        let have_red = red.is_some();
        let (Some((red, na)), Some((black, nb))) = (red, black) else {
            ui.label(egui::RichText::new(if !have_red { i18n::tr("赤い棒を当てる穴をクリックします") } else { i18n::tr("黒い棒を当てる穴をクリックします (ふつうは GND の青い列)") }).color(NOTE));
            return;
        };
        let live = self.live.as_ref().filter(|v| v.error.is_none());
        if !matches && live.is_none() {
            ui.colored_label(crate::theme::warn(), i18n::tr("回路図と違う差し方なので、電源を入れてから読みます (差したとおりの回路を計算します)"));
            return;
        }
        let volts = sch.bench_voltages();
        let v_of = |n: &Option<String>| -> Option<f64> {
            let n = n.as_ref()?;
            if is_ground(n) {
                return Some(0.0);
            }
            if let Some(v) = live.and_then(|lv| lv.voltage(n)) {
                return Some(v);
            }
            volts.as_ref()?.iter().find(|(m, _)| m.eq_ignore_ascii_case(n)).map(|(_, v)| *v)
        };
        let name = |h: &str, n: &Option<String>| format!("{} ({})", h, n.clone().unwrap_or_else(|| i18n::tr("どこにもつながっていない").to_string()));
        ui.label(i18n::trf!("赤: {}　黒: {}", name(&red, &na), name(&black, &nb)));
        match (v_of(&na), v_of(&nb)) {
            (Some(a), Some(b)) => {
                ui.label(egui::RichText::new(format!("{} V", sim_core::format_value(a - b))).size(22.0).strong().color(crate::theme::ok()));
            }
            _ if na.is_none() || nb.is_none() => {
                ui.colored_label(crate::theme::warn(), i18n::tr("棒の片方が、どこにもつながっていない穴です (表示は定まりません)"));
            }
            _ => {
                ui.label(egui::RichText::new(i18n::tr("DC 動作点が求まっていません (回路図の解析で確かめます)")).color(NOTE));
            }
        }
    }

    /// The current and resistance ranges: worked out on the circuit as the board joins it (the parts burnt so far broken in it).
    pub(super) fn meter_other_range(&mut self, ui: &mut egui::Ui, sch: &SchEditor, board: &RunBoard, red: Option<Probe>, black: Option<Probe>) {
        let have_red = red.is_some();
        let (Some((red, na)), Some((black, nb))) = (red, black) else {
            ui.label(egui::RichText::new(if !have_red { i18n::tr("赤い棒を当てる穴をクリックします") } else { i18n::tr("黒い棒を当てる穴をクリックします") }).color(NOTE));
            return;
        };
        let name = |h: &str, n: &Option<String>| format!("{} ({})", h, n.clone().unwrap_or_else(|| i18n::tr("どこにもつながっていない").to_string()));
        ui.label(i18n::trf!("赤: {}　黒: {}", name(&red, &na), name(&black, &nb)));
        let range = self.meter_range;
        if range == MeterRange::Ohms && self.powered {
            ui.colored_label(crate::theme::warn(), i18n::tr("電源が入っています。抵抗は電源を切ってから測ります (テスターが自分で流す小さな電流に、回路の電流が混ざって正しく読めません。実物では、テスターを傷めることもあります)"));
            return;
        }
        if range == MeterRange::Amps && !self.powered {
            ui.label(egui::RichText::new(i18n::tr("電流は、電源を入れてから測ります (切っているあいだは 0 A)")).color(NOTE));
            return;
        }
        let (Some(a), Some(b)) = (na, nb) else {
            ui.colored_label(crate::theme::warn(), i18n::tr("棒の片方が、どこにもつながっていない穴です"));
            return;
        };
        let key = format!("{range:?}|{a}|{b}|{}|{:?}|{}|{:?}|{:?}", self.powered, self.broken, sch.rev, self.temp_c, self.kind);
        if self.meter_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let nets = board.nets(&sch.sch);
            let built = sch.board_circuit(&nets, self.temp_c).map(|c| self.broken.iter().fold(c, |c, (r, f)| sim_core::with_fault(&c, r, *f).unwrap_or(c)));
            let reading = built.and_then(|c| match range {
                MeterRange::Ohms => sim_core::meter::resistance(&c, &a, &b).map(|r| match r {
                    Some(r) => format!("{}Ω", sim_core::format_value(r)),
                    None => "OL".to_string(),
                }),
                _ => sim_core::meter::current(&c, &a, &b).map(|x| match x {
                    sim_core::meter::Amps::Reading(i) => format!("{}A", sim_core::format_value(i)),
                    sim_core::meter::Amps::SameNet => "SAME".to_string(),
                    sim_core::meter::Amps::FuseBlown(i) => format!("FUSE {}", sim_core::format_value(i.abs())),
                }),
            });
            self.meter_cache = Some((key, reading));
        }
        let Some((_, reading)) = &self.meter_cache else { return };
        match reading {
            Err(e) => {
                ui.colored_label(crate::theme::error(), i18n::tr_dyn(e));
            }
            Ok(r) if r == "SAME" => {
                ui.label(egui::RichText::new("0 A").size(22.0).strong().color(crate::theme::ok()));
                ui.label(egui::RichText::new(i18n::tr("2 本の棒が同じ線 (ネット) に当たっています。電流計は直列に入れます: 測りたい線を 1 か所外し (ジャンパー線を抜く・部品の足を別の列へ)、空いた 2 つの穴に赤と黒を当てます")).color(NOTE));
            }
            Ok(r) if r.starts_with("FUSE") => {
                ui.label(egui::RichText::new(i18n::tr("ヒューズが切れました")).size(18.0).strong().color(crate::theme::error()));
                ui.label(egui::RichText::new(i18n::trf!("テスターの中を {}A 流れようとしました。電流のレンジのテスターは、中がほとんど 0 Ω の線です。電源の + と − (並列) に当てるとショートになります。電流は、線を外した所に直列に入れて測ります", &r[5..])).color(crate::theme::warn()));
            }
            Ok(r) if r == "OL" => {
                ui.label(egui::RichText::new("OL").size(22.0).strong().color(crate::theme::ok()));
                ui.label(egui::RichText::new(i18n::tr("つながっていない (開いている) か、とても大きな抵抗です。コンデンサは直流を通さないので OL です")).color(NOTE));
            }
            Ok(r) => {
                ui.label(egui::RichText::new(r.as_str()).size(22.0).strong().color(crate::theme::ok()));
                let note = if range == MeterRange::Ohms {
                    i18n::tr("回路につないだまま測ると、並んだ部品も一緒に測ります (並列なら小さく出ます)。部品だけの値は、片方の足を外して測ります。ダイオード・LED は向きと流す電流で読みが変わります")
                } else {
                    i18n::tr("赤い棒から入って黒い棒へ抜ける向きが + です。テスターの 0.1Ω が入るので、ごく小さい抵抗の回路では少し減ります")
                };
                ui.label(egui::RichText::new(note).color(NOTE));
            }
        }
    }

    /// The scope: the transient waveform of the probe's net.
    pub(super) fn scope_ui(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, l: &Layout) {
        let probe = self.scope.map(|h| (super::hole_text(h), net_at(&sch.sch, l, h)));
        self.scope_panel(ui, sch, probe);
    }

    /// The scope's screen for the probe on the board: the running circuit's trace, else the schematic's transient run.
    pub(super) fn scope_panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor, probe: Option<Probe>) {
        let Some((hole, net)) = probe else {
            ui.label(egui::RichText::new(i18n::tr("探り針を当てる穴をクリックします")).color(NOTE));
            return;
        };
        let Some(net) = net else {
            ui.colored_label(crate::theme::warn(), i18n::trf!("{} は、どこにもつながっていない穴です", hole));
            return;
        };
        ui.label(i18n::trf!("探り針: {} ({net})", hole));
        if let Some(lv) = self.live.as_mut().filter(|v| v.error.is_none()) {
            lv.set_probe(Some(net.clone()));
            let (t, v) = lv.trace();
            ui.label(egui::RichText::new(i18n::tr("動いている波形 (画面の直近 3 秒)")).color(NOTE));
            if t.len() > 1 {
                scope_plot(ui, &t, &v);
            }
            return;
        }
        let signal = if is_ground(&net) { None } else { sch.tran_signal(&format!("V({net})")) };
        if ui.button(i18n::tr("過渡解析を実行")).on_hover_text(i18n::tr("回路図の過渡解析の欄の時間で計算し直します")).clicked() {
            sch.analyze_tran();
        }
        let Some((t, v)) = signal else {
            ui.label(egui::RichText::new(if is_ground(&net) { i18n::tr("GND は 0 V です") } else { i18n::tr("過渡解析の結果がありません。「過渡解析を実行」を押します") }).color(NOTE));
            return;
        };
        scope_plot(ui, &t, &v);
    }
}

/// A small scope screen: the waveform, its highest and lowest value and the time span.
pub(super) fn scope_plot(ui: &mut egui::Ui, t: &[f64], v: &[f64]) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width().min(380.0), 170.0), egui::Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 4.0, Color32::from_rgb(16, 26, 20));
    for k in 1..5 {
        let y = r.top() + r.height() * k as f32 / 5.0;
        painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new(0.5, Color32::from_rgb(40, 70, 50)));
        let x = r.left() + r.width() * k as f32 / 5.0;
        painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(0.5, Color32::from_rgb(40, 70, 50)));
    }
    let (Some(&t0), Some(&t1)) = (t.first(), t.last()) else { return };
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    let pad = ((hi - lo) * 0.1).max(1e-3);
    let (lo, hi) = (lo - pad, hi + pad);
    let pts: Vec<Pos2> = t.iter().zip(v).map(|(t, v)| Pos2::new(r.left() + ((t - t0) / (t1 - t0).max(1e-30)) as f32 * r.width(), r.bottom() - ((v - lo) / (hi - lo)) as f32 * r.height())).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.8, Color32::from_rgb(120, 240, 140))));
    let f = egui::FontId::proportional(12.0);
    painter.text(r.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, format!("{}V", sim_core::format_value(hi - pad)), f.clone(), Color32::from_gray(220));
    painter.text(r.left_bottom() + Vec2::new(4.0, -2.0), egui::Align2::LEFT_BOTTOM, format!("{}V", sim_core::format_value(lo + pad)), f.clone(), Color32::from_gray(220));
    painter.text(r.right_bottom() + Vec2::new(-4.0, -2.0), egui::Align2::RIGHT_BOTTOM, format!("{}s", sim_core::format_value(t1 - t0)), f, Color32::from_gray(220));
}
