//! What the schematic, the board and the 3D view look at together: the part and the net the person picked in one of them.
//! `App::update` copies the pick of the editor that changed to the others (`linked`), which show it in their own way: a ring
//! round the part, the wires / copper of the net in a colour of its own. Off with "設定 → 選んだ部品・ネットを3か所で連動".

/// A part (by reference designator) and / or a net (by name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub part: Option<String>,
    pub net: Option<String>,
    /// The other parts picked together with `part` (a box or Shift pick of several, E6).
    pub others: Vec<String>,
}

impl Focus {
    pub fn is_empty(&self) -> bool {
        self.part.is_none() && self.net.is_none()
    }

    /// Several parts (the first is `part`); empty: none.
    pub fn of_parts(mut list: Vec<String>) -> Focus {
        list.dedup();
        if list.is_empty() {
            return Focus::default();
        }
        let part = list.remove(0);
        Focus { part: Some(part), net: None, others: list }
    }

    /// Every part picked: `part` and the `others`.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.part.iter().chain(self.others.iter()).map(String::as_str)
    }
}
