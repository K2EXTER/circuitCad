//! Common-mode rejection (CMRR) of a difference amplifier: the + and − inputs' sources are driven apart (the difference) and
//! together (the common part); the AC viewer draws the CMRR, Ad and Acm, and the figures (with the spread the resistors'
//! tolerance gives) are listed above the plot.

use super::*;
use sim_core::{cmrr, cmrr_input_sources, cmrr_spread, CmrrSettings, CMRR_CAP_DB};

/// A CMRR in dB, or "> 200 dB" at the cap (the parts match exactly: nothing of the common part is left).
fn shown_db(db: f64) -> String {
    if db >= CMRR_CAP_DB - 1e-9 { format!("> {CMRR_CAP_DB:.0} dB") } else { format!("{db:.1} dB") }
}

impl SchEditor {
    pub fn set_cmrr_settings(&mut self, plus: &str, minus: &str, out: &str) {
        (self.cmrr_plus, self.cmrr_minus, self.cmrr_out) = (plus.into(), minus.into(), out.into());
    }

    pub fn analyze_cmrr(&mut self) {
        self.run_cmrr();
    }

    /// The + and − sources: the ones chosen, else the first two that can drive an input.
    fn cmrr_inputs(&self, c: &sim_core::Circuit) -> (String, String) {
        let found = cmrr_input_sources(c);
        let pick = |chosen: &str, k: usize| if chosen.trim().is_empty() { found.get(k).cloned().unwrap_or_default() } else { chosen.trim().to_string() };
        (pick(&self.cmrr_plus, 0), pick(&self.cmrr_minus, 1))
    }

    pub(super) fn run_cmrr(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "AC解析の欄の開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let out = self.cmrr_out.trim().to_string();
        let tol = self.cmrr_tol.trim().trim_end_matches('%').parse::<f64>().ok().filter(|t| *t > 0.0 && *t < 100.0).map(|t| t / 100.0);
        let outcome = (|| {
            let c = self.sim_circuit()?;
            let (plus, minus) = self.cmrr_inputs(&c);
            if out.is_empty() {
                return Err(vec!["出力のネットを選んでください".to_string()]);
            }
            let s = CmrrSettings { plus: plus.clone(), minus: minus.clone(), output: out.clone(), ac: AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) } };
            let r = cmrr(&c, &s).map_err(|e| dc::small_signal_error(&c, e))?;
            let sum = r.summary().ok_or_else(|| vec!["周波数の点がありません".to_string()])?;
            let mut lines = vec![format!("{plus} (+ 入力) と {minus} (− 入力) を逆向きに揺らした差動ゲイン Ad と、同じ向きに揺らした同相ゲイン Acm の比です (CMRR が大きいほど、共通の揺れを打ち消しています)")];
            lines.push(format!(
                "CMRR: {} で {}、いちばん悪いのは {} で {}。差動ゲイン {:.2} 倍",
                eng(sum.low.0, "Hz"),
                shown_db(sum.low.1),
                eng(sum.worst.0, "Hz"),
                shown_db(sum.worst.1),
                r.diff.mag()[0]
            ));
            if !sum.at.is_empty() {
                lines.push(sum.at.iter().map(|(f, d)| format!("{}: {}", eng(*f, "Hz"), shown_db(*d))).collect::<Vec<_>>().join("  /  "));
            }
            // the resistors drawn inside their tolerance: how far a real board falls from the drawing
            if let Some(t) = tol {
                match cmrr_spread(&c, &s, f0, t, 300, 1) {
                    Ok(sp) => {
                        if let (Some(lo), Some(med)) = (sp.quantile(0.0), sp.quantile(0.5)) {
                            let worst = sp.alone.first().map(|(n, v)| format!("。1 本だけずれたときにいちばん効くのは {n} ({})", shown_db(*v))).unwrap_or_default();
                            lines.push(format!("抵抗が ±{}% ずれると ({}、300 回): 中央値 {}、悪いもので {}{worst}", t * 100.0, eng(f0, "Hz"), shown_db(med), shown_db(lo)));
                        }
                    }
                    Err(e) => lines.push(format!("抵抗のばらつきは計算できませんでした: {e}")),
                }
            }
            let mut ac = r.as_ac_result();
            // the bundled op amp models have no CMRR of their own: the answer is the resistors' alone
            let mut ics: Vec<&str> = c.elements.iter().filter_map(|e| e.name().split_once('.').map(|(p, _)| p)).filter(|p| p.starts_with(['U', 'u'])).collect();
            ics.sort();
            ics.dedup();
            if !ics.is_empty() {
                ac.warnings.insert(0, format!("注意: {} のモデルは、IC 自身の CMRR を持っていません。抵抗が完全にそろうと、計算では際限なく高くなります。実物では、データシートの CMRR (オペアンプで 70〜100 dB ほど) が上限です", ics.join(", ")));
            }
            Ok((ac, plus, minus, sum, lines))
        })();
        let mut selected = BTreeSet::new();
        selected.insert("CMRR".to_string());
        selected.insert(format!("Ad({out})"));
        selected.insert(format!("Acm({out})"));
        self.status = match &outcome {
            Ok((_, p, m, s, _)) => format!("同相除去比: {p}・{m} → {out}、いちばん悪いのは {} で {}", eng(s.worst.0, "Hz"), shown_db(s.worst.1)),
            Err(e) => format!("同相除去比の計算に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        let (outcome, notes) = match outcome {
            Ok((r, _, _, _, lines)) => (Ok(r), lines),
            Err(e) => (Err(e), vec![]),
        };
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature(), loop_run: false, pz_run: false, psrr_run: false, cmrr_run: true, notes });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn panel_cmrr(&mut self, ui: &mut egui::Ui) {
        let c = self.sim_circuit().ok();
        let sources = c.as_ref().map(cmrr_input_sources).unwrap_or_default();
        let (auto_p, auto_m) = c.as_ref().map(|c| self.cmrr_inputs(c)).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for (label, id, value, auto, tip) in [
                ("+ 入力", "cmrr_plus", &mut self.cmrr_plus, auto_p, "+ 入力 (非反転入力の側) につながった信号源。電圧源から選びます。値を「AC 1」にしておく必要はありません"),
                ("− 入力", "cmrr_minus", &mut self.cmrr_minus, auto_m, "− 入力 (反転入力の側) につながった信号源"),
            ] {
                ui.label(i18n::tr(label));
                let shown = if value.is_empty() { if auto.is_empty() { "選ぶ…".to_string() } else { auto } } else { value.clone() };
                egui::ComboBox::from_id_salt(id).width(50.0).selected_text(shown).show_ui(ui, |ui| {
                    for s in &sources {
                        ui.selectable_value(value, s.clone(), s);
                    }
                }).response.on_hover_text(i18n::tr(tip));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("出力"));
            let shown = if self.cmrr_out.is_empty() { "選ぶ…".to_string() } else { self.cmrr_out.clone() };
            egui::ComboBox::from_id_salt("cmrr_out").width(70.0).selected_text(shown).show_ui(ui, |ui| {
                for n in self.node_names() {
                    ui.selectable_value(&mut self.cmrr_out, n.clone(), n);
                }
            }).response.on_hover_text(i18n::tr("アンプの出力のネット"));
            ui.label(i18n::tr("抵抗の誤差 %"));
            ui.add(egui::TextEdit::singleline(&mut self.cmrr_tol).desired_width(30.0)).on_hover_text(i18n::tr("抵抗を許容差の中でランダムに 300 通りずらして、CMRR がどこまで下がるかも出します (いちばん低い周波数で)。1% の抵抗なら 1、空にすると計算しません"));
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "同相除去比を計算"), "同相除去比を計算").on_hover_text(i18n::tr("2 つの入力を逆向きに揺らしたとき (差動) と、同じ向きに揺らしたとき (同相) の出力の比を、周波数ごとに計算します。周波数の範囲と点数は AC解析の欄を使います。結果は AC解析のグラフに出ます")).clicked() {
            self.run_cmrr();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_difference_amplifier_sample_rejects_the_common_part() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/cmrr_diffamp.schjson");
        let mut e = SchEditor::default();
        e.load_path(path);
        e.set_cmrr_settings("", "", "OUT");
        e.run_cmrr();
        let v = e.ac.as_ref().unwrap();
        let r = v.outcome.as_ref().expect("runs");
        assert!(v.cmrr_run && matches!(e.wave_mode, WaveMode::Ac));
        // gain 10, matched resistors: the CMRR is limited by nothing but the solver
        assert!((r.signal("Ad(OUT)").unwrap().mag()[0] - 10.0).abs() < 0.05);
        assert!(r.signal("CMRR").unwrap().db()[0] > 100.0, "{}", e.status);
        // 1 % resistors: the spread line is there, and a real board is far below the drawing
        let spread = v.notes.iter().find(|n| n.starts_with("抵抗が")).expect("spread line");
        assert!(spread.contains("中央値"), "{spread}");
        assert!(r.warnings.first().is_some_and(|w| w.contains("U1")), "{:?}", r.warnings);
        assert!(e.status.contains("同相除去比"), "{}", e.status);
        e.cmrr_minus = e.cmrr_inputs(&e.sim_circuit().unwrap()).0;
        e.run_cmrr();
        assert!(e.status.contains("失敗"), "{}", e.status);
    }

    #[test]
    fn the_cap_reads_as_more_than() {
        assert_eq!(shown_db(CMRR_CAP_DB), "> 200 dB");
        assert_eq!(shown_db(60.04), "60.0 dB");
    }
}
