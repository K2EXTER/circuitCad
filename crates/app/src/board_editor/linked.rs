//! What is picked on the board, as a `Focus` for the schematic and the 3D view (`crate::link`).

use super::*;
use crate::link::Focus;

impl BoardEditor {
    /// The part, or the net (and the part of a pad), that is selected; several parts when several are picked (E6).
    pub fn focus(&self) -> Focus {
        let b = &self.board;
        let net = |n: Option<&str>| n.filter(|n| !n.is_empty()).map(str::to_string);
        let parts: Vec<String> = self.multi.iter().filter_map(|it| if let Item::Part(i) = it { b.parts.get(*i).map(|p| p.refdes.clone()) } else { None }).collect();
        if !parts.is_empty() {
            return Focus::of_parts(parts);
        }
        match self.selected {
            Some(Item::Part(i)) => b.parts.get(i).map(|p| Focus::of_parts(vec![p.refdes.clone()])).unwrap_or_default(),
            Some(Item::Track(i)) if i < b.tracks.len() => Focus { net: net(self.cached_connectivity().track_net(i)), ..Default::default() },
            Some(Item::Via(i)) if i < b.vias.len() => Focus { net: net(self.cached_connectivity().via_net(i)), ..Default::default() },
            Some(Item::Pad(i)) if i < b.pads.len() => Focus { part: Some(b.pads[i].refdes.clone()).filter(|r| !r.is_empty()), net: net(self.cached_connectivity().pad_net(i)), others: vec![] },
            _ => Focus::default(),
        }
    }

    /// The parts the 3D view rings besides the selected one: the board's own pick of several, or the parts picked in the schematic.
    pub(crate) fn ringed_parts(&self) -> Vec<String> {
        let own: Vec<String> = self.multi.iter().filter_map(|it| if let Item::Part(i) = it { self.board.parts.get(*i).map(|p| p.refdes.clone()) } else { None }).collect();
        if own.len() > 1 { own } else { self.linked.parts().map(str::to_string).collect() }
    }
}
