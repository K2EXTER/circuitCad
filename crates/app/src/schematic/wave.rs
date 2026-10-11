//! Which waveform viewer is open, and the "recompute on change" mode.

use super::*;

impl SchEditor {
    pub fn has_wave(&self) -> bool {
        match self.wave_mode {
            WaveMode::Tran => self.tran.is_some(),
            WaveMode::Ac => self.ac.is_some(),
            WaveMode::Noise => self.noise.is_some(),
            WaveMode::Sweep => self.sweep.is_some(),
            WaveMode::Mc => self.mc.is_some(),
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
            WaveMode::Sweep => self.sweep_wave_panel(ui),
            WaveMode::Mc => self.mc_wave_panel(ui),
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
                            n.ghost = o.ghost;
                            n.power = o.power;
                        }
                    }
            }
            WaveMode::Ac if self.ac.is_some() => {
                let old = self.ac.take();
                self.run_ac();
                if let (Some(o), Some(n)) = (old, self.ac.as_mut())
                    && o.outcome.is_ok() {
                        if let Err(msgs) = &n.outcome {
                            failed = Some(format!("AC解析: {}", msgs.first().cloned().unwrap_or_default()));
                            self.ac = Some(o);
                        } else {
                            n.selected = o.selected;
                            n.plot = o.plot;
                            n.ghost = o.ghost;
                        }
                    }
            }
            _ => {}
        }
        if let Some(why) = failed {
            self.status = format!("この値では計算が収束しませんでした。直前の結果を表示しています (原因: {why})。スライダーの「幅」を「細かい」にするか、値を戻してみてください");
        }
    }
}
