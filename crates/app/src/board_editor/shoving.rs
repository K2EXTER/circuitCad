//! Push-and-shove helpers for the editor: the routing preview, dragging one track, ripping up a net.

use super::*;
use pcb_core::shove;

impl BoardEditor {
    /// What laying the route from `a` to `p` would do: (tracks pushed aside, a clearance rule still broken).
    /// Remembered for the last cursor position, so a still mouse costs nothing.
    pub(super) fn route_preview(&self, a: Point, p: Point) -> (usize, bool) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        for v in [a.x, a.y, p.x, p.y, self.track_width] {
            v.to_bits().hash(&mut h);
        }
        (self.side, self.shove, self.route_mode as u8, self.bend_flip).hash(&mut h);
        let key = h.finish();
        if let Some((k, n, bad)) = *self.preview_cache.borrow()
            && k == key {
                return (n, bad);
            }
        let new = self.route_segments(a, p);
        let mut trial = self.board.clone();
        let pushed = if self.shove {
            shove::add_tracks(&mut trial, new)
        } else {
            trial.tracks.extend(new);
            0
        };
        // only the copper-to-copper rule: the route may start in the middle of a pad, or end on the board edge
        let count = |b: &Board| drc::clearance(b).iter().filter(|v| v.message.contains("銅箔どうし")).count();
        let bad = count(&trial) > count(&self.board);
        *self.preview_cache.borrow_mut() = Some((key, pushed, bad));
        (pushed, bad)
    }

    /// Start dragging the track under the pointer (Select tool).
    pub(super) fn start_track_drag(&mut self, i: usize, at: Point) {
        self.checkpoint();
        self.seg_drag = Some((self.board.clone(), i, at));
        self.selected = Some(Item::Track(i));
        self.multi.clear();
    }

    /// Follow the pointer while a track is being dragged: the board is rebuilt from the start each time.
    pub(super) fn update_track_drag(&mut self, p: Point) {
        let Some((orig, i, anchor)) = self.seg_drag.clone() else { return };
        let (dx, dy) = (p.x - anchor.x, p.y - anchor.y);
        self.board = orig;
        if self.shove {
            let pushed = shove::move_track_shoving(&mut self.board, i, dx, dy);
            if pushed > 0 {
                self.status = format!("押しのけ配線: 邪魔な配線 {pushed} 本を脇へ寄せています");
            }
        } else {
            shove::move_track(&mut self.board, i, dx, dy);
        }
    }

    pub(super) fn finish_track_drag(&mut self) {
        if let Some((orig, _, _)) = self.seg_drag.take()
            && pcb_core::zonefill::geometry_key(&orig) == pcb_core::zonefill::geometry_key(&self.board) {
                self.undo.pop(); // it did not move: no undo step
            }
    }

    /// Remove every track and via of the net of track `i`. One undo step.
    pub(super) fn rip_up_selected_net(&mut self, i: usize) {
        let conn = drc::connectivity(&self.board);
        let Some(net) = conn.track_net(i).map(str::to_string) else {
            self.status = "この配線はどのパッドにもつながっていないので、ネットが分かりません。選んで Delete で消します".into();
            return;
        };
        self.checkpoint();
        let n = shove::rip_up_net(&mut self.board, &net);
        self.selected = None;
        self.violations.clear();
        self.status = format!("ネット {net} の配線とビア {n} 個を消しました (未配線の線が戻ります)");
    }
}
