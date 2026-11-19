//! "音で聴く" box of the transient viewer (B4): a signal of the run played as sound (its DC taken off, scaled to a safe loudness,
//! repeated until it lasts a second), or saved as a WAV file (`sim_core::audio`). The simulated time is the sound's time.

use super::*;
use sim_core::TranResult;

/// The signal picked, kept between frames.
fn picked_id() -> egui::Id {
    egui::Id::new("tran_audio_signal")
}

pub(super) fn audio_ui(ui: &mut egui::Ui, res: &TranResult, order: &[&str], w: (f64, f64)) {
    crate::icons::help_small(
        ui,
        "音で聴く",
        "選んだ信号を、表示している時間の範囲だけ音にします。シミュレーションの 1 秒が音の 1 秒なので、1kHz の信号は 1kHz の音です。直流の分は取り除き、いちばん大きい振れを決まった音量にそろえます (大きすぎる音で耳を傷めないように)。範囲が 1 秒より短いときは、くり返して 1 秒にします。フィルタの前と後、ひずんだ波形ときれいな波形を聴き比べてみましょう。電源の値に WAV(ファイル名 1) と書くと、逆に音のファイルを入力にできます",
    );
    let mut picked: String = ui.data(|d| d.get_temp(picked_id())).unwrap_or_default();
    if !order.contains(&picked.as_str()) {
        picked = order.iter().find(|n| res.signal(n).is_some_and(|s| s.unit == "V")).map(|s| s.to_string()).unwrap_or_default();
    }
    ui.horizontal(|ui| {
        ui.label(i18n::tr("信号"));
        egui::ComboBox::from_id_salt("tran_audio_pick").selected_text(if picked.is_empty() { i18n::tr("(選ぶ)").to_string() } else { picked.clone() }).width(120.0).show_ui(ui, |ui| {
            for n in order {
                ui.selectable_value(&mut picked, n.to_string(), *n);
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(picked_id(), picked.clone()));
    let Some(sig) = res.signal(&picked) else { return };
    let sound = sim_core::audio::to_wav(&res.time, &sig.values, w.0, w.1, 1.0);
    match &sound {
        Ok(s) => {
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("▶ 聴く")).on_hover_text(i18n::tr("スピーカーかイヤホンから鳴らします。音量は小さめから")).clicked() {
                    crate::breadboard::sound::play_clip(s.wav.clone());
                }
                if ui.button(i18n::tr("■ 止める")).clicked() {
                    crate::breadboard::sound::stop_clip();
                }
                if ui.button(i18n::tr("WAV で保存…")).on_hover_text(i18n::tr("この音を WAV のファイルにします (44.1kHz・16 ビット・モノラル)")).clicked() {
                    let name = format!("{}.wav", picked.replace(['(', ')', '/', '\\', ':'], "_"));
                    if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(name).save_file() {
                        let _ = std::fs::write(p, &s.wav);
                    }
                }
            });
            let unit = sig.unit;
            let repeat = if s.repeats > 1 { i18n::trf!("、表示範囲を {} 回くり返し", s.repeats) } else { String::new() };
            ui.small(i18n::trf!("{:.2} 秒{repeat}。中心 {}{unit} を 0 に、振れ ±{}{unit} を音量 {:.0} % に", s.seconds, eng(s.offset, ""), eng(s.volts_full_scale * sim_core::audio::PEAK, ""), sim_core::audio::PEAK * 100.0));
        }
        Err(e) => {
            ui.colored_label(crate::theme::WARN, i18n::tr_dyn(e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listening sample: heard through the filter, the 4 kHz whistle is far weaker than at the input while the 440 Hz tone stays;
    /// and the box draws.
    #[test]
    fn the_filter_takes_the_whistle_out_of_the_sound() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/audio_filter.schjson"));
        e.run_tran();
        let res = e.tran.as_ref().unwrap().outcome.as_ref().unwrap();
        // the strength of a tone in the sound, by correlation with a sine and a cosine of it
        let tone = |name: &str, hz: f64| {
            let s = res.signal(name).unwrap();
            let sound = sim_core::audio::to_wav(&res.time, &s.values, 0.01, 0.05, 0.0).unwrap();
            let (rate, x) = sim_core::audio::read_wav(&sound.wav).unwrap();
            let (mut a, mut b) = (0.0, 0.0);
            for (i, v) in x.iter().enumerate() {
                let ph = std::f64::consts::TAU * hz * i as f64 / rate as f64;
                a += v * ph.sin();
                b += v * ph.cos();
            }
            (a * a + b * b).sqrt() * sound.volts_full_scale
        };
        let (in_hi, out_hi) = (tone("V(IN)", 4000.0), tone("V(OUT)", 4000.0));
        let (in_lo, out_lo) = (tone("V(IN)", 440.0), tone("V(OUT)", 440.0));
        assert!(out_hi < in_hi * 0.35, "4 kHz: {in_hi} -> {out_hi}");
        assert!(out_lo > in_lo * 0.8, "440 Hz: {in_lo} -> {out_lo}");
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0))), ..Default::default() };
        let order = ["V(IN)", "V(OUT)"];
        let mut out = ctx.run_ui(input, |ui| audio_ui(ui, res, &order, (0.0, 0.05)));
        out.textures_delta.clear();
    }
}

