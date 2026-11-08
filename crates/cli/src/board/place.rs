//! Board commands that place parts: autoplace, move, orient, matchpairs, nudge, breakout, layers.

use super::*;

impl Session {
    pub(super) fn board_breakout(&mut self, rest: &[String]) -> Res {
        // board breakout [width=0.2]: before routing, a short wire out of every pin of a fine-pitch IC that needs one: the pins in the middle of a
        // side go straight out, the ones at the two ends turn outward (the nearer the end, the sooner), so they do not cross the way out of the
        // middle ones. A part whose stubs make the DRC find more than before gets none. The answer is `stubs`, `parts` and `dropped`.
        let (_, kv) = options(rest);
        let width = match opt(&kv, "width") {
            Some(v) => num(v)?,
            None => 0.2,
        };
        let board = self.board_mut()?;
        let r = pcb_core::breakout::breakout(board, width);
        Ok(obj(json!({"stubs": r.stubs, "parts": r.parts, "dropped": r.dropped})))
    }

    pub(super) fn board_orient(&mut self, rest: &[String]) -> Res {
        // board orient [dry=1] [parts=U1,U2] [minpads=3]: turn each part (0/90/180/270 degrees) to the way that makes its airwires shortest and crosses
        // the fewest. Parts with copper on a pad, locked parts, and turns that would land on something are left. A pin keeps its net.
        let (_, kv) = options(rest);
        let dry = opt(&kv, "dry") == Some("1");
        let only: Option<Vec<String>> = opt(&kv, "parts").map(|l| l.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        let board = self.board_mut()?;
        let min_pads = opt(&kv, "minpads").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        let r = pcb_core::orient::run(board, only.as_deref(), min_pads, dry);
        let turned: Vec<_> = r
            .turned
            .iter()
            .map(|t| json!({"part": t.refdes, "rot": [t.from, t.to], "length_mm": [t.length.0, t.length.1], "crossings": [t.crossings.0, t.crossings.1]}))
            .collect();
        Ok(obj(json!({"dry": dry, "turned": turned, "tried": r.tried})))
    }

    pub(super) fn board_matchpairs(&mut self, rest: &[String]) -> Res {
        // board matchpairs [pairs=D1,D2] [target=MM] [style=square|chamfer|round] [height=MM]: bring differential pairs to the same length (the mean of
        // their two lines) with a serpentine on both lines of each shorter pair. A pair is done on both lines or not at all.
        let (_, kv) = options(rest);
        let only: Option<Vec<String>> = opt(&kv, "pairs").map(|l| l.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        let target = opt(&kv, "target").and_then(|v| v.parse::<f64>().ok());
        let mut opts = pcb_core::meander::MeanderOptions::default();
        match opt(&kv, "style") {
            None | Some("square") => {}
            Some("chamfer") => opts.style = pcb_core::meander::MeanderStyle::Chamfer,
            Some("round") => opts.style = pcb_core::meander::MeanderStyle::Round,
            Some(_) => return Err("board matchpairs: style is square, chamfer or round".into()),
        }
        if let Some(h) = opt(&kv, "height").and_then(|v| v.parse::<f64>().ok()) {
            opts.height_max = h;
        }
        let board = self.board_mut()?;
        let pairs: Vec<_> = board.diff_pairs.iter().filter(|d| only.as_ref().is_none_or(|o| o.contains(&d.name))).cloned().collect();
        if pairs.is_empty() {
            return Err("there are no differential pairs on the board (name pairs in the schematic, or add them in the board editor)".into());
        }
        let r = pcb_core::diffpair::match_pairs(board, &pairs, target, &opts);
        let list: Vec<_> = r
            .pairs
            .iter()
            .map(|(name, len, res)| match res {
                Ok(added) => json!({"pair": name, "length_mm": len, "added_mm": added}),
                Err(e) => json!({"pair": name, "length_mm": len, "error": e}),
            })
            .collect();
        Ok(obj(json!({"target_mm": r.target, "pairs": list})))
    }

    pub(super) fn board_autoplace(&mut self, rest: &[String]) -> Res {
        // board autoplace [sheets=1] [arrays=0] [channels=0]: sheets=1 lays each schematic sheet out as a block (off by default), arrays=0 does not repeat the layout of copies (on by default)
        let (_, kv) = options(rest);
        let board = self.board_mut()?;
        let base = pcb_core::autoplace::Tuning::for_board(board);
        let flag = |name: &str, default: bool| match opt(&kv, name) {
            Some("1") => true,
            Some("0") => false,
            _ => default,
        };
        let tuning = pcb_core::autoplace::Tuning { sheets: flag("sheets", base.sheets), arrays: flag("arrays", base.arrays), channels: flag("channels", base.channels), groups: flag("groups", base.groups), apart: flag("apart", base.apart), ..base };
        let r = pcb_core::autoplace::autoplace_with(board, &tuning)?;
        Ok(obj(json!({"placed": r.placed, "width": r.width, "height": r.height, "poured_nets_ignored": r.poured, "repoured_vias": r.repoured, "airwire_crossings": r.crossings, "group_notes": r.group_notes})))
    }

    pub(super) fn board_nudge(&mut self, rest: &[String]) -> Res {
        // board nudge [budget=SECONDS] [layers=outer] [vias=0]: for each connection still open after a full route, try small parts near
        // it 1-3 mm away (their wiring taken away and routed again); a move stays only when fewer connections are open and the DRC
        // finds nothing more. The answer is `moved` (part, dx, dy), `open_before`, `open_after`, `tried`.
        let (_, kv) = options(rest);
        let budget = match opt(&kv, "budget") {
            Some(v) => num(v)?.max(1.0),
            None => 300.0,
        };
        let grid = match opt(&kv, "grid") {
            Some(g) => num(g)?,
            None => RouteOptions::default().grid,
        };
        let ro = RouteOptions { grid, pad_lead: Some(0.5), outer_only: opt(&kv, "layers") == Some("outer"), no_vias: opt(&kv, "vias") == Some("0"), ..RouteOptions::default() };
        let board = self.board_mut()?;
        let r = pcb_core::nudge::route_with_nudging(board, &ro, std::time::Duration::from_secs_f64(budget));
        let moved: Vec<Value> = r.moved.iter().map(|(p, dx, dy)| json!({"part": p, "dx": dx, "dy": dy})).collect();
        Ok(obj(json!({"moved": moved, "open_before": r.open_before, "open_after": r.open_after, "tried": r.tried, "out_of_time": r.out_of_time})))
    }

    pub(super) fn board_layers(&mut self, rest: &[String]) -> Res {
        // board layers N: the number of copper layers (2, 4, 6 ...), like the editor's 「銅箔の層数」. Refuses when an inner layer that would go has copper.
        let (words, _) = options(rest);
        let total = words.first().ok_or("board layers N  (2, 4, 6 ...)")?.parse::<usize>().map_err(|_| "board layers N  (2, 4, 6 ...)")?;
        if total < 2 || total % 2 != 0 || total > 8 {
            return Err("board layers N: N is 2, 4, 6 or 8".into());
        }
        let inner = (total - 2) as u8;
        let board = self.board_mut()?;
        let lost = board.tracks.iter().filter(|t| matches!(t.side, pcb_core::model::Side::Inner(n) if n > inner)).count() + board.zones.iter().filter(|z| matches!(z.side, pcb_core::model::Side::Inner(n) if n > inner)).count();
        if lost > 0 {
            return Err(format!("{total} layers would drop {lost} track(s)/zone(s) on the inner layers that go away: remove them first"));
        }
        board.inner_layers = inner;
        Ok(obj(json!({"layers": total})))
    }

    pub(super) fn board_move(&mut self, rest: &[String]) -> Res {
        // board move REF X Y [rot=0..3]: put a part (its origin) at X, Y mm; tracks are not moved
        let (words, kv) = options(rest);
        let [refdes, x, y] = words.as_slice() else { return Err("board move REF X Y [rot=0..3]  (mm; the part's origin; wiring is not moved)".into()) };
        let (x, y) = (num(x)?, num(y)?);
        let board = self.board_mut()?;
        let Some(part) = board.parts.iter().find(|p| p.refdes.eq_ignore_ascii_case(refdes)).cloned() else {
            let known: Vec<&str> = board.parts.iter().map(|p| p.refdes.as_str()).collect();
            return Err(format!("no part {refdes} on the board (parts: {})", known.join(" ")));
        };
        let rot = match opt(&kv, "rot") {
            Some(r) => r.parse::<u8>().map_err(|_| format!("rot={r}: use 0, 1, 2 or 3 (quarter turns)"))? % 4,
            None => part.rot,
        };
        board.place_part(&part.refdes, pcb_core::model::Point::new(x, y), rot);
        Ok(obj(json!({"part": part.refdes, "x": x, "y": y, "rot": rot})))
    }
}
