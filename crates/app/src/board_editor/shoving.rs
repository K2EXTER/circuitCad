//! Push-and-shove helpers for the editor: the routing preview, dragging one track, ripping up a net.

use super::*;
use pcb_core::shove;

/// A shove that takes longer than this (to work out one preview) makes the preview follow the cursor in steps.
pub(super) const PREVIEW_BUDGET: std::time::Duration = std::time::Duration::from_millis(60);
/// How long such a slow preview is kept before the next one is worked out.
pub(super) const PREVIEW_THROTTLE: std::time::Duration = std::time::Duration::from_millis(250);

/// What a route being laid would do (see `BoardEditor::route_preview`).
#[derive(Clone, Debug, Default)]
pub(super) struct RoutePreview {
    /// How many existing tracks are pushed aside.
    pub pushed: usize,
    /// A copper-to-copper rule is still broken after the shove.
    pub bad: bool,
    /// The pushed tracks: (where it is now, where it would be).
    pub moved: Vec<(Track, Track)>,
    /// The answer is that of an earlier cursor position (the shove is slow on this board).
    pub throttled: bool,
    /// The tracks the route would lay (the plain line, or the searched route).
    pub tracks: Vec<Track>,
    /// The route was searched: it goes round what is in the way.
    pub found: bool,
}

/// The last preview with the key of what it was worked out for, how long it took and when.
#[derive(Clone)]
pub(super) struct PreviewCache {
    key: u64,
    result: RoutePreview,
    cost: std::time::Duration,
    at: std::time::Instant,
}

impl BoardEditor {
    /// What laying the route from `a` to `p` would do: how many tracks are pushed aside and where they would
    /// go, and whether a clearance rule is still broken. Remembered for the last cursor position, so a still mouse
    /// costs nothing. On a board where the shove takes long, the answer is kept for a moment while the cursor
    /// moves (`throttled`), so the cursor stays smooth.
    pub(super) fn route_preview(&self, a: Point, p: Point) -> RoutePreview {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        for v in [a.x, a.y, p.x, p.y, self.track_width] {
            v.to_bits().hash(&mut h);
        }
        (self.side, self.shove, self.auto_route, self.route_mode as u8, self.bend_flip, self.pad_lead, self.pad_lead_len.to_bits()).hash(&mut h);
        let key = h.finish();
        if let Some(c) = self.preview_cache.borrow().as_ref() {
            if c.key == key {
                return c.result.clone();
            }
            if c.cost > PREVIEW_BUDGET && c.at.elapsed() < PREVIEW_THROTTLE {
                return RoutePreview { throttled: true, ..c.result.clone() };
            }
        }
        let started = std::time::Instant::now();
        let plan = self.auto_plan(a, p);
        let (found, tracks) = (plan.found, plan.tracks.clone());
        let mut trial = self.board.clone();
        // the shove is part of the automatic route: where the route is not searched, the tracks in the way are pushed
        let pushed = if self.auto_route && self.shove && !found {
            shove::add_tracks(&mut trial, plan.tracks)
        } else {
            trial.tracks.extend(plan.tracks);
            0
        };
        // the tracks that moved: where they were and where they would be
        let moved: Vec<(Track, Track)> = self
            .board
            .tracks
            .iter()
            .zip(&trial.tracks)
            .filter(|(old, new)| old.a.dist(new.a) > 1e-9 || old.b.dist(new.b) > 1e-9)
            .map(|(old, new)| (old.clone(), new.clone()))
            .collect();
        // only the copper-to-copper rule: the route may start in the middle of a pad, or end on the board edge
        let count = |b: &Board| drc::clearance(b).iter().filter(|v| v.message.contains("銅箔どうし")).count();
        // a route that runs over another net's pad or track joins the two nets: that is a short, not just a small gap
        let bad = count(&trial) > count(&self.board) || drc::shorts(&trial).len() > drc::shorts(&self.board).len();
        let result = RoutePreview { pushed, bad, moved, throttled: false, tracks, found };
        *self.preview_cache.borrow_mut() = Some(PreviewCache { key, result: result.clone(), cost: started.elapsed(), at: std::time::Instant::now() });
        result
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

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> BoardEditor {
        let mut e = BoardEditor::default();
        e.board = Board::new("t", 40.0, 30.0);
        e.board.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 12.0), b: Point::new(35.0, 12.0), width: 0.25 });
        e.shove = true;
        e.auto_route = true;
        e.track_width = 0.25;
        e.grid = 0.0;
        e
    }

    /// A shove that took long makes the preview keep its answer for a moment while the cursor moves; a quick one
    /// follows the cursor every time, and once the moment has passed the preview is worked out again.
    #[test]
    fn a_slow_preview_is_kept_for_a_moment_and_a_quick_one_is_not() {
        let e = editor();
        let (a, p1, p2) = (Point::new(10.0, 11.65), Point::new(20.0, 11.65), Point::new(21.0, 11.65));
        let first = e.route_preview(a, p1);
        assert!(!first.throttled && first.pushed == 1, "{first:?}");
        // the last one was quick: another cursor position is worked out again
        assert!(!e.route_preview(a, p2).throttled);
        // pretend it was slow and just now
        let key_before = e.preview_cache.borrow().as_ref().unwrap().key;
        {
            let mut c = e.preview_cache.borrow_mut();
            let c = c.as_mut().unwrap();
            c.cost = PREVIEW_BUDGET * 3;
            c.at = std::time::Instant::now();
        }
        let kept = e.route_preview(a, p1);
        assert!(kept.throttled, "a slow answer is kept while the cursor moves on");
        assert_eq!(kept.pushed, first.pushed, "the earlier answer");
        // the same cursor position is the same question: never throttled
        let again = e.route_preview(a, p2);
        assert!(again.throttled || e.preview_cache.borrow().as_ref().unwrap().key == key_before);
        // after the moment has passed it is worked out again
        e.preview_cache.borrow_mut().as_mut().unwrap().at = std::time::Instant::now() - PREVIEW_THROTTLE * 2;
        assert!(!e.route_preview(a, p1).throttled);
    }
}
