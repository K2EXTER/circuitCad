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
        if self.dc.is_some() {
            self.run_dc();
        }
        match self.wave_mode {
            WaveMode::Tran if self.tran.is_some() => {
                let old = self.tran.take();
                self.run_tran();
                if let (Some(o), Some(n)) = (old, self.tran.as_mut())
                    && n.outcome.is_ok() && o.outcome.is_ok() {
                        n.selected = o.selected;
                        n.plot = o.plot;
                        n.spectrum = o.spectrum;
                        n.spec_t0 = o.spec_t0;
                        n.spec_t1 = o.spec_t1;
                        n.spec_plot = o.spec_plot;
                    }
            }
            WaveMode::Ac if self.ac.is_some() => {
                let old = self.ac.take();
                self.run_ac();
                if let (Some(o), Some(n)) = (old, self.ac.as_mut())
                    && n.outcome.is_ok() && o.outcome.is_ok() {
                        n.selected = o.selected;
                        n.plot = o.plot;
                    }
            }
            _ => {}
        }
    }
}
