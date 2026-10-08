//! Circuit simulation core (GUI independent): DC operating point so far.

mod circuit;
mod dc;
mod from_schematic;
mod spice_in;
mod tran;
mod value;
mod waveform;

pub use circuit::{BjtModel, Circuit, DiodeModel, Element};
pub use dc::{dc_op, DcResult, ElementResult, SimError};
pub use from_schematic::{from_schematic, ConvertError};
pub use spice_in::{add_model_instance, add_subckt_instance, parse_models};
pub use tran::{transient, Signal, TranResult, TranSettings};
pub use value::parse_value;
pub use waveform::{parse_source, Waveform};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_tran;
