//! Supply rejection (PSRR): a 1 V ripple on one supply alone, swept over frequency; the share that reaches the chosen node is
//! drawn in the AC viewer, and the rejection figures are listed above the plot.

use super::*;
use sim_core::{psrr_summary, supply_sources, with_supply_ripple};

/// A rejection as a plain ratio: 60 dB is "1/1k" (the ripple becomes a thousandth), next to nothing is "1".
fn as_ratio(db: f64) -> String {
    if db < 0.5 {
        return "1".into();
    }
    let r = 10f64.powf(db / 20.0);
    let shown = if r >= 100.0 {
        let p = 10f64.powf(r.log10().floor() - 1.0);
        (r / p).round() * p
    } else {
        (r * 10.0).round() / 10.0
    };
    format!("1/{}", format_value(shown))
}

impl SchEditor {
    pub fn set_psrr_settings(&mut self, source: &str, out: &str) {
        self.psrr_source = source.into();
        self.psrr_out = out.into();
    }

    pub fn analyze_psrr(&mut self) {
        self.run_psrr();
    }

    pub(super) fn run_psrr(&mut self) {
        let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
            self.status = "AC解析の欄の開始・終了周波数と点数/デケードを数値で入力してください (例: 10, 1Meg, 20)".into();
            return;
        };
        let out = self.psrr_out.trim().to_string();
        let outcome = (|| {
            let c = self.sim_circuit()?;
            let source = match self.psrr_source.trim() {
                "" => supply_sources(&c).into_iter().next().ok_or_else(|| vec!["揺らす電源がありません。直流の電圧源 (5V の V1 など) を置いてください".to_string()])?,
                s => s.to_string(),
            };
            if out.is_empty() {
                return Err(vec!["揺れを見る点 (出力のネット) を選んでください".to_string()]);
            }
            let c = with_supply_ripple(&c, &source).map_err(|e| dc::small_signal_error(&c, e))?;
            let mut r = ac_analysis(&c, &AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) }).map_err(|e| vec![e.to_string()])?;
            let sig = r.signal(&format!("V({out})")).ok_or_else(|| vec![format!("ネット {out} が見つかりません")])?;
            let s = psrr_summary(&r.freq, &sig.db()).ok_or_else(|| vec!["周波数の点がありません".to_string()])?;
            // the figures go first in the list above the plot
            let mut lines = vec![format!("{source} に 1 V の揺れを入れて、{out} に届く揺れを見ています (グラフの dB がマイナスほど、よく除去しています)")];
            lines.push(format!("除去比: {} で {:.1} dB (揺れは {} 倍)、いちばん悪いのは {} で {:.1} dB (揺れは {} 倍)", eng(s.low.0, "Hz"), s.low.1, as_ratio(s.low.1), eng(s.worst.0, "Hz"), s.worst.1, as_ratio(s.worst.1)));
            if !s.at.is_empty() {
                lines.push(s.at.iter().map(|(f, d)| format!("{}: {d:.1} dB", eng(*f, "Hz"))).collect::<Vec<_>>().join("  /  "));
            }
            // the bundled IC models mostly leave the supply out of their insides: their rejection is better than a real part's
            let mut ics: Vec<&str> = c.elements.iter().filter_map(|e| e.name().split_once('.').map(|(p, _)| p)).filter(|p| p.starts_with(['U', 'u'])).collect();
            ics.sort();
            ics.dedup();
            if !ics.is_empty() {
                r.warnings.insert(0, format!("注意: {} は IC のモデルです。同梱のモデルの多くは、実物の電源除去比を持たないか簡単にしてあり、特に高い周波数では実物よりよく出ます。IC の分は、データシートの PSRR のグラフと比べてください", ics.join(", ")));
            }
            Ok((r, source, s, lines))
        })();
        let mut selected = BTreeSet::new();
        selected.insert(format!("V({out})"));
        self.status = match &outcome {
            Ok((_, src, s, _)) => format!("電源除去比: {src} → {out}、いちばん悪いのは {} で {:.1} dB", eng(s.worst.0, "Hz"), s.worst.1),
            Err(e) => format!("電源除去比の計算に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let (lo, hi) = if f1 > f0 * 1.0001 { (f0, f1) } else { (f0 / 2.0, f0 * 2.0) };
        let ghost = self.ac.as_ref().and_then(|v| v.ghost.clone());
        let (outcome, notes) = match outcome {
            Ok((r, _, _, lines)) => (Ok(r), lines),
            Err(e) => (Err(e), vec![]),
        };
        self.ac = Some(AcView { outcome, selected, plot: PlotState::log(lo.max(1e-30), hi), ghost, signature: self.signature(), loop_run: false, pz_run: false, psrr_run: true, notes });
        self.wave_mode = WaveMode::Ac;
    }

    pub(super) fn panel_psrr(&mut self, ui: &mut egui::Ui) {
        let supplies = self.sim_circuit().map(|c| supply_sources(&c)).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr("揺らす電源"));
            let shown = if self.psrr_source.is_empty() { supplies.first().cloned().unwrap_or_else(|| "選ぶ…".into()) } else { self.psrr_source.clone() };
            egui::ComboBox::from_id_salt("psrr_source").width(60.0).selected_text(shown).show_ui(ui, |ui| {
                for s in &supplies {
                    ui.selectable_value(&mut self.psrr_source, s.clone(), s);
                }
            }).response.on_hover_text(i18n::tr("揺れ (リップル・雑音) が乗る電源。直流の電圧源から選びます。この電源だけに 1 V の揺れを入れ、ほかの電源の「AC」は外して計算します (回路図は書き換えません)"));
            ui.label(i18n::tr("見る点"));
            let shown = if self.psrr_out.is_empty() { "選ぶ…".to_string() } else { self.psrr_out.clone() };
            egui::ComboBox::from_id_salt("psrr_out").width(70.0).selected_text(shown).show_ui(ui, |ui| {
                for n in self.node_names() {
                    ui.selectable_value(&mut self.psrr_out, n.clone(), n);
                }
            }).response.on_hover_text(i18n::tr("揺れがどれだけ届くかを見るネット。レギュレータやアンプの出力を選びます"));
        });
        if self.lit(crate::widgets::primary_button_wide(ui, true, "電源除去比を計算"), "電源除去比を計算").on_hover_text(i18n::tr("電源の揺れが、選んだ点にどれだけ届くかを周波数ごとに計算します。周波数の範囲と点数は AC解析の欄を使います。結果は AC解析のグラフに出ます")).clicked() {
            self.run_psrr();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_read_as_plain_fractions() {
        assert_eq!(as_ratio(60.0), "1/1k");
        assert_eq!(as_ratio(20.0), "1/10");
        assert_eq!(as_ratio(6.0), "1/2");
        assert_eq!(as_ratio(0.1), "1");
    }

    #[test]
    fn a_zener_regulator_rejects_its_supply_ripple() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/zener_regulator.schjson");
        let mut e = SchEditor::default();
        e.load_path(path);
        // the zener's node: the one that is not the supply
        let out = e.node_names().into_iter().find(|n| {
            e.psrr_out = n.clone();
            e.run_psrr();
            e.ac.as_ref().unwrap().outcome.as_ref().is_ok_and(|r| r.signal(&format!("V({n})")).unwrap().db()[0] < -3.0)
        });
        assert!(out.is_some(), "some node rejects the ripple: {}", e.status);
        let v = e.ac.as_ref().unwrap();
        assert!(v.psrr_run && matches!(e.wave_mode, WaveMode::Ac));
        assert!(v.notes.iter().any(|n| n.starts_with("除去比")), "{:?}", v.notes);
        assert!(e.status.contains("電源除去比"), "{}", e.status);
        e.psrr_out = String::new();
        e.run_psrr();
        assert!(e.status.contains("失敗"), "{}", e.status);
    }
}
