//! What a pin of a library part does electrically (a smaller set of KiCad's pin types): the ERC looks at the pins that have one
//! (outputs tied together, inputs nothing drives, supply pins), and leaves the pins without one to the checks by name.
//! `PinKind::guess` proposes one from the data sheet's pin table (its I/O column) or from the pin's name.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinKind {
    /// Takes a signal (a logic input, an op-amp's input, an enable).
    Input,
    /// Drives its net high and low (a push-pull output): two on one net fight.
    Output,
    /// Input and output by turns (a data line, I²C's SDA).
    Bidir,
    /// Pulls low only (open drain / open collector): several may share a net with a pull-up.
    OpenDrain,
    /// Takes the supply (VCC, VDD, VIN, GND).
    PowerIn,
    /// Gives a supply (a regulator's output).
    PowerOut,
    /// A plain terminal (a resistor's end, a transistor's pin).
    Passive,
    /// Not connected inside: left open.
    Nc,
}

impl PinKind {
    pub const ALL: [PinKind; 8] = [PinKind::Input, PinKind::Output, PinKind::Bidir, PinKind::OpenDrain, PinKind::PowerIn, PinKind::PowerOut, PinKind::Passive, PinKind::Nc];

    /// The word for it in the part form's pin text (`kind=in`).
    pub fn word(self) -> &'static str {
        match self {
            PinKind::Input => "in",
            PinKind::Output => "out",
            PinKind::Bidir => "io",
            PinKind::OpenDrain => "od",
            PinKind::PowerIn => "pwr",
            PinKind::PowerOut => "pwrout",
            PinKind::Passive => "pas",
            PinKind::Nc => "nc",
        }
    }

    pub fn from_word(w: &str) -> Option<PinKind> {
        PinKind::ALL.into_iter().find(|k| k.word().eq_ignore_ascii_case(w))
    }

    /// The name shown on screen (Japanese; the screens wrap it in the translation).
    pub fn label(self) -> &'static str {
        match self {
            PinKind::Input => "入力",
            PinKind::Output => "出力",
            PinKind::Bidir => "入出力",
            PinKind::OpenDrain => "オープンドレイン",
            PinKind::PowerIn => "電源入力",
            PinKind::PowerOut => "電源出力",
            PinKind::Passive => "受動",
            PinKind::Nc => "未接続 (NC)",
        }
    }

    /// The kind a data sheet's pin table says in its I/O (type) column: "I", "O", "I/O", "OD", "P", "G", "NC" … None for a cell that
    /// says nothing known (TI's "—" of a supply pin).
    pub fn from_io_cell(cell: &str) -> Option<PinKind> {
        let c: String = cell.trim().to_ascii_uppercase().chars().filter(|c| !c.is_whitespace() && *c != '(' && *c != ')').collect();
        // "I/O(1)": a note's number after it
        let c = c.trim_end_matches(|ch: char| ch.is_ascii_digit());
        Some(match c {
            "I" | "IN" | "INPUT" | "AI" | "DI" => PinKind::Input,
            "O" | "OUT" | "OUTPUT" | "AO" | "DO" => PinKind::Output,
            "I/O" | "IO" | "B" | "BIDIR" => PinKind::Bidir,
            "OD" | "OC" | "O/OD" | "ODO" => PinKind::OpenDrain,
            "P" | "PWR" | "POWER" | "S" | "SUPPLY" | "G" | "GND" | "GROUND" => PinKind::PowerIn,
            // TI's tables put a dash for the supply pins: the column says nothing, the name tells
            "NC" => PinKind::Nc,
            _ => return None,
        })
    }

    /// A kind from a pin's name alone, when the name says it plainly: NC; a supply or ground (VCC, VDD, VIN, AVDD, VBAT, GND, VSS,
    /// AGND); a supply output (VOUT, VO); an output (OUT, 1OUT, Y, Q); an input (IN, IN+, −IN, EN, SHDN, CS, CLK, RESET). None
    /// otherwise (FB, ADJ, SW, a numbered name): the person picks.
    pub fn guess_from_name(name: &str) -> Option<PinKind> {
        let n = name.trim().to_ascii_uppercase().replace(['\u{2212}', '\u{2013}'], "-");
        let n = n.trim_start_matches('/').trim_start_matches('~');
        // a leading digit of the unit ("1OUT", "2IN-") and a trailing one ("VDD2", "IN1")
        let core = n.trim_start_matches(|c: char| c.is_ascii_digit());
        let bare = core.trim_end_matches(|c: char| c.is_ascii_digit());
        if matches!(bare, "NC" | "N.C." | "DNC") {
            return Some(PinKind::Nc);
        }
        if ["GND", "VSS", "AGND", "DGND", "PGND", "SGND", "VEE", "V-"].contains(&bare) {
            return Some(PinKind::PowerIn);
        }
        if ["VCC", "VDD", "VIN", "AVDD", "DVDD", "AVCC", "DVCC", "VBAT", "VS", "V+", "VDDA", "VDDIO", "IOVDD", "VCCIO", "VPP"].contains(&bare) {
            return Some(PinKind::PowerIn);
        }
        if matches!(bare, "VOUT" | "VO") {
            return Some(PinKind::PowerOut);
        }
        if matches!(bare, "OUT" | "OUTA" | "OUTB" | "Y" | "Q" | "/Q" | "QB" | "Q-") {
            return Some(PinKind::Output);
        }
        let input = ["IN", "IN+", "IN-", "+IN", "-IN", "INA", "INB", "EN", "CE", "SHDN", "CS", "CLK", "SCK", "SCLK", "RESET", "RST", "MR"];
        if input.contains(&bare) {
            return Some(PinKind::Input);
        }
        None
    }

    /// The kind to propose for a pin: the data sheet's I/O cell when it says one, else the name's.
    pub fn guess(name: &str, io_cell: Option<&str>) -> Option<PinKind> {
        io_cell.and_then(PinKind::from_io_cell).or_else(|| PinKind::guess_from_name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_is_guessed_from_the_io_column_then_the_name() {
        use PinKind::*;
        for (cell, want) in [("I", Some(Input)), ("O", Some(Output)), ("I/O", Some(Bidir)), ("I/O(1)", Some(Bidir)), ("OD", Some(OpenDrain)), ("P", Some(PowerIn)), ("G", Some(PowerIn)), ("NC", Some(Nc)), ("—", None), ("-", None), ("Description", None)] {
            assert_eq!(PinKind::from_io_cell(cell), want, "{cell}");
        }
        for (name, want) in [("VCC", Some(PowerIn)), ("VDD2", Some(PowerIn)), ("GND", Some(PowerIn)), ("NC", Some(Nc)), ("1OUT", Some(Output)), ("2IN−", Some(Input)), ("EN", Some(Input)), ("VOUT", Some(PowerOut)), ("FB", None), ("SW", None), ("B", None), ("A", None)] {
            assert_eq!(PinKind::guess_from_name(name), want, "{name}");
        }
        // the column goes first: an "OUT" the table calls open drain
        assert_eq!(PinKind::guess("OUT", Some("OD")), Some(OpenDrain));
        assert_eq!(PinKind::guess("OUT", Some("")), Some(Output));
        for k in PinKind::ALL {
            assert_eq!(PinKind::from_word(k.word()), Some(k));
        }
    }
}
