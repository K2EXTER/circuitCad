//! What is being computed on another thread right now (the transient analysis, the fault scan, the fault diagnosis): the line in
//! the status bar with a spinner, the seconds so far, how far it is and "止める (Esc)" (ui-rules.md, 2026-11-17).

use super::*;
use std::sync::atomic::Ordering;

/// One computation on another thread, for the status line.
pub struct Running {
    /// What is computed ("過渡解析").
    pub what: &'static str,
    pub seconds: f64,
    /// 0..1 when it is known.
    pub fraction: Option<f64>,
}

impl SchEditor {
    /// Takes the results that have come from other threads (every frame, from the window, whatever tab is shown).
    pub fn poll_jobs(&mut self, ctx: &egui::Context) {
        self.poll_tran_job(ctx);
        self.poll_fault_scan(ctx);
        self.poll_diagnosis(ctx);
    }

    /// The computation running on another thread, if any (the first of them when there are several).
    pub fn running(&self) -> Option<Running> {
        if let Some(job) = &self.tran_job {
            let fraction = (job.tstop > 0.0).then(|| (job.watch.time() / job.tstop).clamp(0.0, 1.0));
            return Some(Running { what: "過渡解析", seconds: job.started.elapsed().as_secs_f64(), fraction });
        }
        if let Some(run) = &self.fault_ui.scan_run {
            let fraction = (run.total > 0).then(|| run.done.load(Ordering::Relaxed) as f64 / run.total as f64);
            return Some(Running { what: "よくある故障を順に試す", seconds: run.started.elapsed().as_secs_f64(), fraction });
        }
        if self.diagnosis_running() {
            return Some(Running { what: "故障の候補の計算", seconds: self.diagnosis_seconds(), fraction: None });
        }
        None
    }

    /// Stops what is running on other threads ("止める" and Esc in the status line).
    pub fn stop_running(&mut self) {
        self.stop_tran_job();
        if let Some(run) = &self.fault_ui.scan_run {
            run.cancel.store(true, Ordering::Relaxed);
        }
        self.stop_diagnosis();
    }
}

/// The left of the status line while something is computed: a spinner, "〈what〉を計算中… n 秒 (k %)" and "止める (Esc)". Returns
/// whether stop was asked (the button, or Esc anywhere in the window).
pub fn running_line(ui: &mut egui::Ui, r: &Running) -> bool {
    ui.spinner();
    let pct = r.fraction.map(|f| format!(" ({:.0} %)", f * 100.0)).unwrap_or_default();
    ui.label(i18n::trf!("{}を計算中… {:.0} 秒{pct}", i18n::tr(r.what), r.seconds));
    let clicked = ui.small_button(i18n::tr("止める (Esc)")).on_hover_text(i18n::tr("計算を途中でやめます。前の結果は、そのまま残ります")).clicked();
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    clicked || ui.input(|i| i.key_pressed(egui::Key::Escape))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transient run with a watch takes exactly the steps of one without, reports the time it reached, and stops when asked.
    #[test]
    fn a_watched_run_is_the_same_run_and_can_be_stopped() {
        let mut e = SchEditor::default();
        e.load_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/rc_lowpass.schjson"));
        let c = e.sim_circuit().unwrap();
        let s = TranSettings { tstop: 5e-3, tstep: 1e-5, uic: false };
        let plain = transient(&c, &s).unwrap();
        let w = std::sync::Arc::new(sim_core::TranWatch::default());
        let mut run = sim_core::TranRun::start(&c, &s, None).unwrap();
        run.watch(w.clone());
        run.advance_to(s.tstop).unwrap();
        let watched = run.finish();
        assert_eq!(plain.time, watched.time);
        assert!(w.time() > 0.0 && w.time() <= s.tstop);
        let mut run = sim_core::TranRun::start(&c, &s, None).unwrap();
        w.stop.store(true, Ordering::Relaxed);
        run.watch(w);
        assert_eq!(run.advance_to(s.tstop).unwrap_err().to_string(), sim_core::TRAN_STOPPED);
        // nothing runs: no line in the status bar
        assert!(e.running().is_none());
    }
}
