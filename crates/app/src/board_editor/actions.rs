//! Whole-board actions: autorouting and the DRC.

use super::*;

/// The auto-router's grid pitch for an editor grid of `grid` mm: a divisor of it, so that every point the editor snaps
/// to is also a point the router can use (0.5 mm -> 0.25 mm, 2.54 mm -> 0.231 mm). Without a grid: 0.2 mm.
pub(super) fn route_pitch(grid: f64) -> f64 {
    if grid < 0.1 {
        return 0.2;
    }
    grid / (grid / 0.25).ceil().max(1.0)
}

impl BoardEditor {
    /// Route the airwires of every net, or only of `net`. One undo step.
    pub fn autoroute(&mut self, net: Option<&str>) {
        if self.board.outline.len() < 3 {
            self.status = "先に基板の外形を決めてください".into();
            return;
        }
        if !ratsnest(&self.board).iter().any(|w| net.is_none_or(|n| w.net == n)) {
            self.status = "自動配線: 未配線はありません".into();
            return;
        }
        self.checkpoint();
        let tracks_before = self.board.tracks.len();
        let opt = RouteOptions { track_width: self.track_width, via_diameter: self.via_dia, via_drill: self.via_drill, grid: route_pitch(self.grid), follow: None };
        // differential pairs first, side by side; the rest afterwards
        let mut pair_routed = 0;
        let mut pair_failed = vec![];
        for d in self.board.diff_pairs.clone() {
            if net.is_none_or(|n| n == d.p || n == d.n) {
                let r = autoroute::route_pair(&mut self.board, &d, &opt);
                pair_routed += r.routed;
                pair_failed.extend(r.failed);
            }
        }
        // nets with a declared current ("配線の太さ (電流から)") are routed with the width that current needs
        let mut report = autoroute::RouteReport::default();
        let mut wide: Vec<(String, f64)> = vec![];
        for (n, amps) in self.board.net_currents.clone() {
            let need = pcb_core::current::round_up_width(pcb_core::current::width_for_current(amps, &Default::default()));
            if need > opt.track_width + 1e-9 && net.is_none_or(|x| x == n) {
                wide.push((n, need));
            }
        }
        for (n, w) in &wide {
            let r = autoroute::autoroute(&mut self.board, &RouteOptions { track_width: *w, ..opt.clone() }, Some(n));
            report.routed += r.routed;
            report.failed.extend(r.failed);
        }
        let rest = autoroute::autoroute(&mut self.board, &opt, net);
        report.routed += rest.routed;
        // a wide net that failed may still go through with the normal width (the width check then warns)
        report.failed = rest.failed;
        report.routed += pair_routed;
        // a pair connection that failed is tried again by the plain router above; count only what is still open
        let _ = pair_failed;
        self.violations.clear();
        if report.routed == 0 {
            self.undo.pop(); // nothing changed: no undo step
        }
        // the new tracks leave and enter the pads at a right angle, like those drawn by hand
        let aligned = if self.pad_lead && report.routed > 0 { self.align_pad_entries(Some((tracks_before..self.board.tracks.len()).collect())) } else { 0 };
        self.status = if report.failed.is_empty() {
            let tail = if aligned > 0 { format!("パッドの根元 {aligned} か所を垂直にそろえました。") } else { String::new() };
            format!("自動配線: {} 本を配線しました。{tail}(格子 {:.3} mm。手で足すときは、配線や端の点に吸い付きます)。DRC で確認しましょう", report.routed, opt.grid)
        } else {
            format!(
                "自動配線: {} 本を配線、{} 本は通せませんでした (黄色の線のまま)。部品の位置を動かすか、手動で配線してください",
                report.routed,
                report.failed.len()
            )
        };
    }

    /// Pack the parts into rows (connected ones together) and fit the outline. One undo step.
    pub fn autoplace(&mut self) {
        self.checkpoint();
        match pcb_core::autoplace::autoplace(&mut self.board) {
            Ok(r) => {
                self.violations.clear();
                self.selected = None;
                self.multi.clear();
                self.fit_pending = true;
                self.status = format!("自動配置: {} 個を並べ、外形を {} × {} mm にしました。位置は手で動かして調整できます", r.placed, r.width, r.height);
            }
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    pub fn run_drc(&mut self) {
        self.violations = drc::check_final(&self.board);
        self.violations.extend(pcb_core::diffpair::check_pairs(&self.board));
        self.drc_ran = true;
        self.status = format!("DRC: 違反 {} 件", self.violations.len());
    }
}
