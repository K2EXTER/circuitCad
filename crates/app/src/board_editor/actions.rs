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
        let opt = RouteOptions { track_width: self.track_width, via_diameter: self.via_dia, via_drill: self.via_drill, grid: route_pitch(self.grid) };
        let report = autoroute::autoroute(&mut self.board, &opt, net);
        self.violations.clear();
        if report.routed == 0 {
            self.undo.pop(); // nothing changed: no undo step
        }
        self.status = if report.failed.is_empty() {
            format!("自動配線: {} 本を配線しました (格子 {:.3} mm。手で足すときは、配線や端の点に吸い付きます)。DRC で確認しましょう", report.routed, opt.grid)
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
        self.violations = drc::check(&self.board);
        self.drc_ran = true;
        self.status = format!("DRC: 違反 {} 件", self.violations.len());
    }
}
