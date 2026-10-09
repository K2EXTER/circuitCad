//! Circuit simulation core (GUI independent): DC operating point so far.

mod ac;
mod circuit;
mod dc;
mod dcsweep;
mod from_schematic;
mod montecarlo;
mod noise;
mod spectrum;
mod spice_in;
mod temperature;
mod tran;
mod value;
mod waveform;

pub use ac::{ac_analysis, AcResult, AcSettings, AcSignal, AcSweep, C64};
pub use circuit::{BjtModel, Circuit, DiodeModel, Element, MosModel, SwitchModel};
pub use dcsweep::{dc_sweep, inverter_metrics, DcSweepResult, DcSweepSettings, DcSweepSignal, InverterMetrics};
pub use dc::{dc_op, DcResult, ElementResult, SimError};
pub use montecarlo::{monte_carlo, McResult, McSettings, McStats};
pub use noise::{describe_source, integrate_psd, noise_analysis, source_element, NoiseContribution, NoiseKind, NoiseResult, NoiseSettings, SourceInfo};
pub use from_schematic::{from_schematic, ConvertError};
pub use spice_in::{add_model_instance, add_subckt_instance, parse_models};
pub use temperature::{temp_sweep, T_NOM_C};
pub use tran::{transient, Signal, TranResult, TranSettings};
pub use spectrum::{spectrum, stats, Spectrum, Stats};
pub use value::{format_value, parse_part_value, parse_value};
pub use waveform::{parse_source, parse_source_full, Source, Waveform};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_555;
#[cfg(test)]
mod tests_ac;
#[cfg(test)]
mod tests_blinker;
#[cfg(test)]
mod tests_dcsweep;
#[cfg(test)]
mod tests_logic;
#[cfg(test)]
mod tests_mos;
#[cfg(test)]
mod tests_parts;
#[cfg(test)]
mod tests_switch;
#[cfg(test)]
mod tests_thermal;
#[cfg(test)]
mod tests_noise;
#[cfg(test)]
mod tests_tran;
#[cfg(test)]
mod tests_variation;
#[cfg(test)]
mod tests_perf;
