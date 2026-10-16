//! "電力と効率" box of the transient viewer: pick the voltage and current of the input and of the output.

use super::*;
use sim_core::{efficiency, mean_power, TranResult};

fn pick(ui: &mut egui::Ui, id: &str, label: &str, tip: &str, names: &[&str], cur: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(if cur.is_empty() { "(選ぶ)" } else { cur.as_str() })
            .width(86.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(cur, String::new(), "(なし)");
                for n in names {
                    ui.selectable_value(cur, n.to_string(), *n);
                }
            })
            .response
            .on_hover_text(tip);
    });
}

/// `pw` = [input V, input I, output V, output I] signal names; `w` = the time window being looked at.
pub(super) fn power_ui(ui: &mut egui::Ui, res: &TranResult, order: &[&str], w: (f64, f64), pw: &mut [String; 4]) {
    let volts: Vec<&str> = order.iter().copied().filter(|n| res.signal(n).is_some_and(|s| s.unit == "V")).collect();
    let amps: Vec<&str> = order.iter().copied().filter(|n| res.signal(n).is_some_and(|s| s.unit != "V")).collect();
    crate::icons::help_small(ui, "電力と効率の出し方", "入力 (電源) と出力 (負荷) の電圧と電流を選ぶと、表示中の時間範囲での平均電力と効率が出ます");
    pick(ui, "pw_iv", "入力の電圧", "電源のプラス側の点の電圧 (V(...))", &volts, &mut pw[0]);
    pick(ui, "pw_ii", "入力の電流", "電源を流れる電流 (I(V1) など)。向きは気にしなくて大丈夫です (大きさで計算します)", &amps, &mut pw[1]);
    pick(ui, "pw_ov", "出力の電圧", "負荷にかかる電圧 (V(...))", &volts, &mut pw[2]);
    pick(ui, "pw_oi", "出力の電流", "負荷を流れる電流 (I(R2) など)", &amps, &mut pw[3]);
    let p = |v: &str, i: &str| -> Option<f64> {
        let (sv, si) = (res.signal(v)?, res.signal(i)?);
        mean_power(&res.time, &sv.values, &si.values, w.0, w.1)
    };
    let (pin, pout) = (p(&pw[0], &pw[1]).map(f64::abs), p(&pw[2], &pw[3]).map(f64::abs));
    if let Some(x) = pin {
        ui.small(format!("  入力の平均電力 {}", eng(x, "W")));
    }
    if let Some(x) = pout {
        ui.small(format!("  出力の平均電力 {}", eng(x, "W")));
    }
    if let (Some(a), Some(b)) = (pin, pout) {
        match efficiency(a, b) {
            Some(e) => {
                let col = if e > 1.02 { Color32::from_rgb(230, 70, 60) } else if e >= 0.8 { Color32::from_rgb(90, 190, 110) } else { Color32::from_rgb(220, 160, 40) };
                let loss = (a - b).max(0.0);
                ui.label(egui::RichText::new(format!("  効率 {:.1} %   損失 {}", e * 100.0, eng(loss, "W"))).color(col))
                    .on_hover_text("効率 = 出力の電力 ÷ 入力の電力。損失は熱になります (スイッチング電源は 80〜95%、リニアレギュレータは 出力電圧÷入力電圧 くらい)");
                if e > 1.02 {
                    ui.small("  100% を超えています。電圧と電流の組み合わせか、時間範囲 (起動直後を含む、など) を確かめてください");
                }
            }
            None => {
                ui.small("  入力の電力が 0 なので効率は出せません");
            }
        }
    }
}
