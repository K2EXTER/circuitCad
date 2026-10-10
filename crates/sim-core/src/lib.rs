//! Circuit simulation core (GUI independent): DC operating point so far.

mod ac;
mod circuit;
mod dc;
mod dcsweep;
mod designcalc;
mod diagnose;
mod expr;
mod fault;
mod from_schematic;
mod measure;
mod montecarlo;
mod noise;
mod options;
mod paramsweep;
mod spectrum;
mod spice_in;
mod stepmetrics;
mod temperature;
mod tran;
mod value;
mod waveform;

pub use ac::{ac_analysis, AcResult, AcSettings, AcSignal, AcSweep, C64};
pub use expr::Expr;
pub use circuit::{BjtModel, Circuit, DiodeModel, Element, MosModel, SwitchModel};
pub use dcsweep::{dc_sweep, inverter_metrics, DcSweepResult, DcSweepSettings, DcSweepSignal, InverterMetrics};
pub use options::{options, set_options, SimOptions};
pub use paramsweep::{param_sweep, sweep_values, with_number, ParamAnalysis, ParamCurve, ParamResult, ParamSettings, MAX_RUNS};
pub use designcalc::{battery_life, zener_regulator, divider, e24, led_resistor, rc_filter, regulator, smoothing_cap, transistor_switch, CalcOut};
pub use fault::{describe_fault, break_pin, break_part, simulate_broken, simulate_broken_ac, simulate_broken_tran, Broken, applicable as fault_applicable, breakable, simulate_fault, simulate_fault_ac, simulate_fault_tran, with_fault, AcFaultReport, Fault, FaultReport, PartChange, TranFaultReport};
pub use diagnose::diagnose;
pub use dc::{dc_op, DcResult, ElementResult, SimError};
pub use measure::{stability_margins, wave_measure, Margins, WaveMeasure};
pub use montecarlo::{minus_3db, monte_carlo, McMeasure, McResult, McSettings, McStats, TranMetric};
pub use noise::{describe_source, integrate_psd, noise_analysis, source_element, NoiseContribution, NoiseKind, NoiseResult, NoiseSettings, SourceInfo};
pub use from_schematic::{from_schematic, from_schematic_with_pin_fault, ConvertError, PinFaultSpec, PinHow};
pub use spice_in::{add_model_instance, add_subckt_instance, parse_models};
pub use temperature::{temp_sweep, T_NOM_C};
pub use tran::{transient, Signal, TranResult, TranSettings};
pub use stepmetrics::{step_metrics, StepMetrics};
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
mod tests_charger;
#[cfg(test)]
mod tests_tp5100;
#[cfg(test)]
mod tests_ip2332;
#[cfg(test)]
mod tests_bsource;
#[cfg(test)]
mod tests_dcsweep;
#[cfg(test)]
mod tests_logic;
#[cfg(test)]
mod tests_mos;
#[cfg(test)]
mod tests_parts;
#[cfg(test)]
mod tests_parts_more;
#[cfg(test)]
mod tests_switch;
#[cfg(test)]
mod tests_thermal;
#[cfg(test)]
mod tests_noise;
#[cfg(test)]
mod tests_stepmetrics;
#[cfg(test)]
mod tests_tran;
#[cfg(test)]
mod tests_variation;
#[cfg(test)]
mod tests_perf;
