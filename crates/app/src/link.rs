//! What the schematic, the board and the 3D view look at together: the part and the net the person picked in one of them.
//! `App::update` copies the pick of the editor that changed to the others (`linked`), which show it in their own way: a ring
//! round the part, the wires / copper of the net in a colour of its own. Off with "設定 → 選んだ部品・ネットを3か所で連動".

/// A part (by reference designator) and / or a net (by name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub part: Option<String>,
    pub net: Option<String>,
}

impl Focus {
    pub fn is_empty(&self) -> bool {
        self.part.is_none() && self.net.is_none()
    }
}
