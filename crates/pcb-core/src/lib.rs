//! PCB data model and manufacturing-file export (Gerber RS-274X / Excellon).
//! All lengths are millimetres.

pub mod assembly;
pub mod autoplace;
pub mod autoroute;
pub mod current;
pub mod drc;
pub mod excellon;
pub mod export;
pub mod fab;
pub mod footprints;
pub mod geom;
pub mod gerber;
pub mod gerber_view;
pub mod model;
pub mod netlist;
pub mod outline;
pub mod package;
pub mod parts;
pub mod ratsnest;
pub mod strokefont;

#[cfg(test)]
mod tests_autoroute;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_preview;
#[cfg(test)]
mod tests_bottom;
#[cfg(test)]
mod tests_zone;
#[cfg(test)]
mod tests_fab;

pub use model::*;
