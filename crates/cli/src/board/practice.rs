//! Board commands for the repair practice: break, health, reveal.

use super::*;

impl Session {
    pub(super) fn board_break(&mut self, rest: &[String]) -> Res {
        // board break [fault=bypass|cut|thin|short|outside] [seed=N]: put one fault into the board, to practise finding and mending it. The answer gives the
        // symptom (not the cause) and what the checks find; `board health` asks again (and says when the board is mended); `board reveal` gives the cause.
        use pcb_core::breakage::{self, Fault};
        let (_, kv) = options(rest);
        let seed = opt(&kv, "seed").and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
        let board = self.board_mut()?;
        let possible = breakage::possible(board);
        let fault = match opt(&kv, "fault") {
            None => *possible.get((seed % possible.len().max(1) as u64) as usize).ok_or("this board cannot have a fault (it needs parts and tracks)")?,
            Some("bypass") => Fault::BypassFar,
            Some("cut") => Fault::CutTrack,
            Some("thin") => Fault::ThinTrack,
            Some("short") => Fault::ShortCircuit,
            Some("outside") => Fault::PartOutside,
            Some(_) => return Err("board break: fault is bypass, cut, thin, short or outside".into()),
        };
        let before = breakage::health(board);
        let injected = breakage::inject(board, fault, seed)?;
        let now = breakage::health(board);
        let answer = json!({"symptom": injected.symptom, "health": health_json(&now), "health_before": health_json(&before), "possible_faults": possible.len()});
        self.broken = Some((before, injected));
        Ok(obj(answer))
    }

    pub(super) fn board_health(&mut self, _rest: &[String]) -> Res {
        // board health: what the checks find (DRC violations, unrouted connections, parts off the board, ICs with a far bypass capacitor); `repaired` after `board break`
        let board = self.board_mut()?;
        let now = pcb_core::breakage::health(board);
        let repaired = self.broken.as_ref().map(|(before, _)| pcb_core::breakage::repaired(before, &now));
        Ok(obj(json!({"health": health_json(&now), "repaired": repaired})))
    }

    pub(super) fn board_reveal(&mut self, _rest: &[String]) -> Res {
        // board reveal: what `board break` did, and how to mend it
        let (_, inj) = self.broken.as_ref().ok_or("nothing was broken (use `board break` first)")?;
        Ok(obj(json!({"fault": inj.fault.name(), "target": inj.target, "at": [inj.at.x, inj.at.y], "cause": inj.cause})))
    }
}
