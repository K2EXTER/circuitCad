//! PCB data model and manufacturing-file export (Gerber RS-274X / Excellon).
//! All lengths are millimetres.

// `!(x > 0.0)` is deliberate: it is also true for NaN, which `x <= 0.0` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod assembly;
pub mod autoplace;
pub mod autoroute;
pub mod body;
pub mod current;
pub mod diffpair;
pub mod drc;
pub mod excellon;
pub mod export;
pub mod fab;
pub mod footprints;
pub mod geom;
pub mod irdrop;
pub mod gerber;
pub mod gerber_view;
pub mod model;
pub mod model3d;
pub mod netlist;
pub mod outline;
pub mod package;
pub mod parts;
pub mod ratsnest;
pub mod shove;
pub mod smooth;
pub mod strokefont;
pub mod zonefill;

#[cfg(test)]
mod tests_autoroute;
#[cfg(test)]
mod tests_layers;
#[cfg(test)]
mod tests_shove;
#[cfg(test)]
mod tests_smooth;
#[cfg(test)]
mod tests_diffpair;
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
#[cfg(test)]
mod tests_holes;

pub use model::*;
