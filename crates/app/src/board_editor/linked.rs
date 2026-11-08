//! What is picked on the board, as a `Focus` for the schematic and the 3D view (`crate::link`).

use super::*;
use crate::link::Focus;

impl BoardEditor {
    /// The part, or the net (and the part of a pad), that is selected.
    pub fn focus(&self) -> Focus {
        let b = &self.board;
        let net = |n: Option<&str>| n.filter(|n| !n.is_empty()).map(str::to_string);
        match self.selected {
            Some(Item::Part(i)) => b.parts.get(i).map(|p| Focus { part: Some(p.refdes.clone()), net: None }).unwrap_or_default(),
            Some(Item::Track(i)) if i < b.tracks.len() => Focus { part: None, net: net(self.cached_connectivity().track_net(i)) },
            Some(Item::Via(i)) if i < b.vias.len() => Focus { part: None, net: net(self.cached_connectivity().via_net(i)) },
            Some(Item::Pad(i)) if i < b.pads.len() => Focus { part: Some(b.pads[i].refdes.clone()).filter(|r| !r.is_empty()), net: net(self.cached_connectivity().pad_net(i)) },
            _ => Focus::default(),
        }
    }
}
