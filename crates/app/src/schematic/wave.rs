//! Which waveform viewer is open, and the "recompute on change" mode.

use super::*;

impl SchEditor {
    pub fn has_wave(&self) -> bool {
        match self.wave_mode {
            WaveMode::Tran => self.tran.is_some(),
            WaveMode::Ac => self.ac.is_some(),
            WaveMode::Noise => self.noise.is_some(),
            WaveMode::Impedance => self.z.is_some(),
            WaveMode::Sweep => self.sweep.is_some(),
            WaveMode::Mc => self.mc.is_some(),
            WaveMode::Sens => self.sens.is_some(),
            WaveMode::Param => self.param.is_some(),
            WaveMode::Fault => self.fault_wave.is_some(),
        }
    }

    /// Bottom panel: the waveform viewer for the most recent analysis.
    pub fn wave_panel(&mut self, ui: &mut egui::Ui) {
        match self.wave_mode {
            WaveMode::Tran => self.tran_wave_panel(ui),
            WaveMode::Ac => self.ac_wave_panel(ui),
            WaveMode::Noise => self.noise_wave_panel(ui),
            WaveMode::Impedance => self.z_wave_panel(ui),
            WaveMode::Sweep => self.sweep_wave_panel(ui),
            WaveMode::Mc => self.mc_wave_panel(ui),
            WaveMode::Sens => self.sens_wave_panel(ui),
            WaveMode::Param => self.param_wave_panel(ui),
            WaveMode::Fault => self.fault_wave_panel(ui),
        }
    }

    /// With "自動で再計算" on, redo the analysis that is on screen after the circuit changed,
    /// keeping the signals and zoom the user had chosen.
    pub(super) fn live_update(&mut self) {
        if !self.live {
            return;
        }
        if self.sch == self.live_snap {
            return;
        }
        // a heavy circuit is not redone every frame while a slider moves: wait about as long as the last run took (the
        // changes pile up in `sch`, so the last value is always drawn in the end)
        if self.live_cost > LIVE_CHEAP && self.live_last.is_some_and(|t| t.elapsed().as_secs_f64() < self.live_cost * 1.5) {
            self.live_waiting = true;
            return;
        }
        self.live_waiting = false;
        let started = std::time::Instant::now();
        // the first change after a pause keeps the picture it replaces as the faint "before" curve
        // (the very first call only takes the snapshot of the sheet: nothing was moved yet)
        let keep_before = self.live_ghost && self.live_snap != Schematic::default() && self.live_last.is_none_or(|t| t.elapsed().as_secs_f64() > 0.6);
        let before = if !keep_before {
            None
        } else {
            match self.wave_mode {
                WaveMode::Tran => self.tran.as_ref().and_then(tran_ghost),
                WaveMode::Ac => self.ac.as_ref().and_then(ac_ghost),
                _ => None,
            }
        };
        self.live_snap = self.sch.clone();
        // a value that does not converge must not wipe the picture: the last good result stays, with the reason
        let mut failed: Option<String> = None;
        if self.dc.is_some() {
            let old = self.dc.take();
            self.run_dc();
            if let (Some(o), Some(n)) = (old, self.dc.as_ref())
                && o.outcome.is_ok()
                && let Err(msgs) = &n.outcome {
                    failed = Some(format!("DC解析: {}", msgs.first().cloned().unwrap_or_default()));
                    self.dc = Some(o);
                }
        }
        match self.wave_mode {
            WaveMode::Tran if self.tran.is_some() => {
                let old = self.tran.take();
                self.run_tran();
                if let (Some(o), Some(n)) = (old, self.tran.as_mut())
                    && o.outcome.is_ok() {
                        if let Err(msgs) = &n.outcome {
                            failed = Some(format!("過渡解析: {}", msgs.first().cloned().unwrap_or_default()));
                            self.tran = Some(o);
                        } else {
                            n.selected = o.selected;
                            n.plot = o.plot;
                            n.spectrum = o.spectrum;
                            n.spec_t0 = o.spec_t0;
                            n.spec_t1 = o.spec_t1;
                            n.spec_plot = o.spec_plot;
                            n.ghost = before.or(o.ghost);
                            n.power = o.power;
                        }
                    }
            }
            WaveMode::Ac if self.ac.is_some() => {
                let old = self.ac.take();
                if old.as_ref().is_some_and(|o| o.loop_run) {
                    self.run_loop();
                } else if old.as_ref().is_some_and(|o| o.pz_run) {
                    self.run_pz();
                } else if old.as_ref().is_some_and(|o| o.psrr_run) {
                    self.run_psrr();
                } else {
                    self.run_ac();
                }
                if let (Some(o), Some(n)) = (old, self.ac.as_mut())
                    && o.outcome.is_ok() {
                        if let Err(msgs) = &n.outcome {
                            failed = Some(format!("AC解析: {}", msgs.first().cloned().unwrap_or_default()));
                            self.ac = Some(o);
                        } else {
                            n.selected = o.selected;
                            n.plot = o.plot;
                            n.ghost = before.or(o.ghost);
                        }
                    }
            }
            _ => {}
        }
        self.live_cost = started.elapsed().as_secs_f64();
        self.live_last = Some(std::time::Instant::now());
        if let Some(why) = failed {
            self.status = format!("この値では計算が収束しませんでした。直前の結果を表示しています (原因: {why})。スライダーの「幅」を「細かい」にするか、値を戻してみてください");
        }
    }
}

/// A run shorter than this (seconds) is redone on every change; a longer one is paced.
const LIVE_CHEAP: f64 = 0.05;

/// The selected curves of a transient result as the faint "before" curves.
fn tran_ghost(v: &TranView) -> Option<Ghost> {
    let res = v.outcome.as_ref().ok()?;
    let sigs: Vec<GhostSig> = res
        .signals
        .iter()
        .filter(|sg| v.selected.contains(&sg.name))
        .map(|sg| GhostSig { base: sg.name.clone(), name: format!("{} (前)", sg.name), group: if sg.unit == "V" { 0 } else { 1 }, v: sg.values.clone() })
        .collect();
    (!sigs.is_empty()).then(|| Ghost { x: res.time.clone(), sigs })
}

/// The selected curves of an AC result (magnitude and phase) as the faint "before" curves.
fn ac_ghost(v: &AcView) -> Option<Ghost> {
    let res = v.outcome.as_ref().ok()?;
    let mut sigs = vec![];
    for sg in res.signals.iter().filter(|sg| v.selected.contains(&sg.name)) {
        let name = format!("{} (前)", sg.name);
        sigs.push(GhostSig { base: sg.name.clone(), name: name.clone(), group: 0, v: sg.db() });
        sigs.push(GhostSig { base: sg.name.clone(), name, group: 1, v: sg.phase_deg() });
    }
    (!sigs.is_empty()).then(|| Ghost { x: res.freq.clone(), sigs })
}

/// The checkboxes that pick which signals are drawn, side by side, in a box about three lines high that scrolls (a sheet with
/// forty nets used to push the tables under it off the panel). With many signals a filter box comes first; the chosen ones are
/// listed first so they stay in view. The colour of a chosen name is its curve's colour (its place among the chosen, in
/// `order`), so the plot and the list agree.
pub(super) fn signal_list(ui: &mut egui::Ui, id: &str, order: &[&str], selected: &mut BTreeSet<String>) {
    let filter_id = egui::Id::new(("signal_filter", id));
    let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
    if order.len() > 12 {
        ui.add(egui::TextEdit::singleline(&mut filter).hint_text(i18n::tr("信号の名前で絞り込み (例: OUT)")).desired_width(f32::INFINITY))
            .on_hover_text(i18n::tr("名前の一部を入れると、その文字を含む信号だけが並びます (大文字と小文字は区別しません)。選んでいる信号はいつも先頭に出ます"));
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    }
    let needle = filter.trim().to_lowercase();
    let colour_of = |name: &str, selected: &BTreeSet<String>| order.iter().filter(|n| selected.contains(**n)).position(|n| *n == name).map(|i| PALETTE[i % PALETTE.len()]);
    let mut shown: Vec<&str> = order.iter().copied().filter(|n| selected.contains(*n)).collect();
    shown.extend(order.iter().copied().filter(|n| !selected.contains(*n) && (needle.is_empty() || n.to_lowercase().contains(&needle))));
    // currents ("I(R1)") on a line of their own under the voltages, each with a small heading, when there are both
    let is_current = |n: &str| n.starts_with("I(") || n.starts_with("I ");
    let (currents, others): (Vec<&str>, Vec<&str>) = shown.into_iter().partition(|n| is_current(n));
    let both = !currents.is_empty() && !others.is_empty();
    egui::ScrollArea::vertical().id_salt(("signal_list", id)).max_height(84.0).auto_shrink([false, true]).show(ui, |ui| {
        for (heading, names) in [("電圧", others), ("電流", currents)] {
            if names.is_empty() {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                if both {
                    ui.label(egui::RichText::new(i18n::tr(heading)).small().weak());
                }
                for name in names {
                    let mut on = selected.contains(name);
                    // a name made here ("T ループゲイン", "合計") is shown in the screen's language; the key stays the Japanese
                    let shown = i18n::tr_dyn(name);
                    let text = match colour_of(name, selected) {
                        Some(c) if on => egui::RichText::new(shown).color(c),
                        _ => egui::RichText::new(shown),
                    };
                    if ui.checkbox(&mut on, text).changed() {
                        if on {
                            selected.insert(name.to_string());
                        } else {
                            selected.remove(name);
                        }
                    }
                }
            });
        }
    });
}
