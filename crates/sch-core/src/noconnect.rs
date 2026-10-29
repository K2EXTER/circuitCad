//! Pins left unconnected on purpose (`Schematic::no_connects`): an unused output, a pin the data sheet says to leave open. ERC stays quiet
//! about them, so the warnings that remain are the ones that are real.

use crate::Schematic;

impl Schematic {
    /// Is this pin marked as left open on purpose?
    pub fn is_no_connect(&self, refdes: &str, pin: &str) -> bool {
        self.no_connects.iter().any(|(r, p)| r == refdes && p == pin)
    }

    /// Mark the pin (or take the mark off). Returns whether anything changed.
    pub fn set_no_connect(&mut self, refdes: &str, pin: &str, on: bool) -> bool {
        let has = self.is_no_connect(refdes, pin);
        match (on, has) {
            (true, false) => {
                self.no_connects.push((refdes.to_string(), pin.to_string()));
                true
            }
            (false, true) => {
                self.no_connects.retain(|(r, p)| !(r == refdes && p == pin));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{erc, Kind};

    fn ic() -> Schematic {
        let mut s = Schematic::default();
        s.add(Kind::Resistor, (10, 10), 0);
        s
    }

    #[test]
    fn a_pin_marked_as_left_open_is_not_asked_about_and_a_wire_on_it_is() {
        let mut s = ic();
        let asks = |s: &Schematic| erc(s).iter().filter(|p| p.message.contains("R1 ピン")).map(|p| p.message.clone()).collect::<Vec<_>>();
        assert_eq!(asks(&s).len(), 2, "both pins of a lone resistor: {:?}", asks(&s));
        assert!(s.set_no_connect("R1", "2", true));
        assert!(!s.set_no_connect("R1", "2", true), "a second mark changes nothing");
        let after = asks(&s);
        assert_eq!(after.len(), 1, "{after:?}");
        assert!(after[0].contains("ピン1"));
        assert!(erc(&s).iter().find(|p| p.message.contains("ピン1")).unwrap().pin == Some(("R1".into(), "1".into())));
        assert!(s.set_no_connect("R1", "2", false));
        assert_eq!(asks(&s).len(), 2);
        assert!(s.no_connects.is_empty());
    }

    #[test]
    fn the_marks_are_saved_and_read_back_and_an_empty_list_is_not_written() {
        let mut s = ic();
        assert!(!serde_json::to_string(&s).unwrap().contains("no_connects"));
        s.set_no_connect("R1", "1", true);
        let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(back.is_no_connect("R1", "1"));
    }
}
