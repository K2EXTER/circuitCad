//! The rule check while one works (online DRC): every time the copper or the rules change, `drc::check` runs again on another
//! thread, and its findings are marked on the board at once (a small red ring with a cross; the pointer over one shows what is
//! wrong). The full "DRC を実行" still makes the list (with the finish, the pours in pieces and the open connections), but a
//! clearance broken by a track just drawn or a part just moved shows up without it. While parts are dragged the last answer
//! stays (the check runs again when they are dropped).

use super::*;
use pcb_core::drc::Violation;
use std::sync::mpsc::{Receiver, TryRecvError};

/// The state of the check: the answer for one state of the board, and the job working on a newer one.
pub struct LiveDrc {
    /// On by default; the checkbox in "③ 確認して発注" turns it off.
    pub on: bool,
    shown: Option<(u64, std::rc::Rc<Vec<Violation>>)>,
    job: Option<(u64, Receiver<Vec<Violation>>)>,
}

impl Default for LiveDrc {
    fn default() -> Self {
        Self { on: true, shown: None, job: None }
    }
}

/// What the check depends on: the copper and holes, the outline, the rules and the net classes.
fn board_key(b: &pcb_core::Board) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    pcb_core::zonefill::geometry_key(b).hash(&mut h);
    format!("{:?}{:?}{:?}{:?}", b.rules, b.net_classes, b.outline, b.silk.len()).hash(&mut h);
    h.finish()
}

/// `drc::check`, less what is said about parts still waiting outside the board (just imported, not yet placed: putting them on
/// the board is the next step, not a mistake).
pub fn live_check(board: &pcb_core::Board) -> Vec<Violation> {
    let parked: Vec<Point> = board
        .parts
        .iter()
        .filter(|p| !pcb_core::geom::inside_polygon(&board.outline, p.pos))
        .flat_map(|p| board.pads.iter().filter(move |q| q.refdes == p.refdes).map(|q| q.pos))
        .collect();
    let mut found = drc::check(board);
    found.retain(|v| !parked.iter().any(|p| p.dist(v.at) < 1e-6));
    found
}

impl BoardEditor {
    /// Start the check for the board as it is, take in an answer that came back, and ask for frames while one is being made.
    pub(super) fn update_live_drc(&mut self, ctx: &egui::Context) {
        if !self.live_drc.on {
            return;
        }
        // a drag keeps the last answer (the board changes every frame)
        if self.slow_results_kept() {
            return;
        }
        let key = board_key(&self.board);
        if let Some((k, rx)) = &self.live_drc.job {
            match rx.try_recv() {
                Ok(v) => {
                    let k = *k;
                    self.live_drc.shown = Some((k, std::rc::Rc::new(v)));
                    self.live_drc.job = None;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    return;
                }
                Err(TryRecvError::Disconnected) => self.live_drc.job = None,
            }
        }
        if self.live_drc.shown.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let board = self.board.clone();
        std::thread::spawn(move || {
            let _ = tx.send(live_check(&board));
        });
        self.live_drc.job = Some((key, rx));
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }

    /// The findings of the last check (empty when it is off or none has come back yet).
    pub fn live_findings(&self) -> std::rc::Rc<Vec<Violation>> {
        match &self.live_drc.shown {
            Some((_, v)) if self.live_drc.on => v.clone(),
            _ => Default::default(),
        }
    }

    /// Wait for the check of the board as it is (for tests: the editor never waits).
    #[cfg(test)]
    pub fn settle_live_drc(&mut self) {
        if !self.live_drc.on {
            return;
        }
        let ctx = egui::Context::default();
        for _ in 0..2000 {
            self.update_live_drc(&ctx);
            if self.live_drc.job.is_none() && self.live_drc.shown.as_ref().is_some_and(|(k, _)| *k == board_key(&self.board)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// The marks on the board, and the message of the one under the pointer.
    pub(super) fn draw_live_drc(&self, painter: &egui::Painter, rect: Rect, hover: Option<Pos2>) {
        let found = self.live_findings();
        if found.is_empty() {
            return;
        }
        let red = Color32::from_rgb(255, 70, 70);
        let mut tip: Option<(Pos2, &str)> = None;
        for v in found.iter().filter(|_| self.vis.drc_marks) {
            let c = self.to_screen(rect, v.at);
            if !rect.expand(20.0).contains(c) {
                continue;
            }
            painter.circle_filled(c, 7.0, red.gamma_multiply(0.25));
            painter.circle_stroke(c, 7.0, Stroke::new(1.5, red));
            let d = 3.5;
            painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], Stroke::new(1.5, red));
            painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], Stroke::new(1.5, red));
            if tip.is_none() && hover.is_some_and(|h| h.distance(c) <= 9.0) {
                tip = Some((c, v.message.as_str()));
            }
        }
        if let Some((c, msg)) = tip {
            let galley = painter.layout(i18n::tr_dyn(msg), egui::FontId::proportional(14.0), Color32::WHITE, 380.0);
            let at = c + Vec2::new(12.0, 12.0);
            let bg = Rect::from_min_size(at, galley.size()).expand(5.0);
            painter.rect_filled(bg, 4.0, Color32::from_rgb(90, 20, 20).gamma_multiply(0.95));
            painter.galley(at, galley, Color32::WHITE);
        }
        // the count in the corner, so a finding off the screen is not missed
        let n = found.len();
        let text = if self.vis.drc_marks {
            format!("ルール違反 {n} 件 (赤い × の所。ポインタを合わせると内容)")
        } else {
            format!("ルール違反 {n} 件 (印は隠しています。右上の「物の表示とロック」の「点検の印」で出せます)")
        };
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), Color32::WHITE);
        let at = Pos2::new(rect.left() + 44.0, rect.bottom() - 30.0 - galley.size().y);
        painter.rect_filled(Rect::from_min_size(at, galley.size()).expand(4.0), 4.0, red.gamma_multiply(0.8));
        painter.galley(at, galley, Color32::WHITE);
    }

    /// The checkbox that turns the check on and off, beside the "DRC を実行" button.
    pub(super) fn live_drc_toggle(&mut self, ui: &mut egui::Ui) {
        let n = self.live_findings().len();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.live_drc.on, i18n::tr("作業中も点検する")).on_hover_text(
                i18n::tr("配線を引いたり部品を動かしたりするたびに、線どうしの間隔・穴・基板の端などの規則を裏で調べ直して、違反の所に赤い × を出します (ポインタを合わせると内容が出ます)。\n「DRC を実行」は、仕上げの形・ベタの分断・未配線も含めて一覧にします。発注の前には、そちらも実行してください"),
            );
            if self.live_drc.on && n > 0 {
                ui.colored_label(crate::theme::ERROR, i18n::trf!("いま {n} 件"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_waiting_outside_the_board_are_not_findings() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut sch = crate::schematic::SchEditor::default();
        sch.load_path(root.join("tp5100_charger.schjson"));
        let mut e = BoardEditor::default();
        e.import_from_schematic(&sch);
        assert!(!drc::check(&e.board).is_empty(), "the full check speaks of the holes outside");
        assert!(live_check(&e.board).is_empty(), "{:?}", live_check(&e.board));
    }

    #[test]
    fn a_track_too_close_is_found_without_running_the_drc_and_goes_away_with_it() {
        let mut e = BoardEditor::default();
        e.board = pcb_core::Board::new("t", 30.0, 20.0);
        for (x, net) in [(5.0, "A"), (8.0, "B")] {
            let mut p = pcb_core::Pad::new(Point::new(x, 10.0), pcb_core::PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = net.into();
            e.board.pads.push(p);
        }
        e.settle_live_drc();
        assert!(e.live_findings().is_empty());
        // a track of B that ends just beside the pad of A
        e.checkpoint();
        e.board.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(8.0, 10.0), b: Point::new(5.7, 10.0), width: 0.25 });
        e.settle_live_drc();
        assert!(!e.live_findings().is_empty(), "the short is marked at once");
        assert!(e.violations.is_empty(), "the list of the full DRC is not touched");
        e.undo();
        e.settle_live_drc();
        assert!(e.live_findings().is_empty());
        // off: nothing is shown
        e.live_drc.on = false;
        e.board.tracks.push(pcb_core::Track { side: Side::Top, a: Point::new(8.0, 10.0), b: Point::new(5.7, 10.0), width: 0.25 });
        e.settle_live_drc();
        assert!(e.live_findings().is_empty());
    }
}
