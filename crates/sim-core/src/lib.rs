//! Circuit simulation core (GUI independent): DC operating point so far.

// `!(x > 0.0)` is deliberate: it is also true for NaN, which `x <= 0.0` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// Structs with many fields (calculation results, test fixtures) are filled by assigning after `Default::default()`; that reads better than one long initializer.
#![allow(clippy::field_reassign_with_default)]

mod ac;
mod chain;
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
mod power;
mod ratings;
mod ripple;
mod spec;
mod spectrum;
mod spice_in;
mod spice_flat;
mod stepmetrics;
mod temperature;
mod tran;
mod value;
mod waveform;

pub use ac::{ac_analysis, AcResult, AcSettings, AcSignal, AcSweep, C64};
pub use expr::Expr;
pub use circuit::{BjtModel, Circuit, DiodeModel, Element, MosModel, SeqKind, SeqLevels, SwitchModel};
pub use dcsweep::{dc_sweep, inverter_metrics, DcSweepResult, DcSweepSettings, DcSweepSignal, InverterMetrics};
pub use options::{options, set_options, SimOptions};
pub use paramsweep::{param_sweep, sweep_values, with_number, ParamAnalysis, ParamCurve, ParamResult, ParamSettings, MAX_RUNS};
pub use designcalc::{boost_design, mosfet_switch, opamp_gain, buck_design, battery_life, zener_regulator, divider, e24, fuse_design, heat_design, led_resistor, rc_filter, regulator, smoothing_cap, transistor_switch, CalcOut};
pub use chain::{chain_failure, ChainEnd, ChainReport, ChainStep};
pub use fault::{TURN_SHORTS, assess_safety, Danger, FaultSafety, describe_fault, break_pin, break_part, simulate_broken, simulate_broken_ac, simulate_broken_tran, Broken, applicable as fault_applicable, breakable, simulate_fault, simulate_fault_ac, simulate_fault_tran, with_fault, AcFaultReport, Fault, FaultReport, PartChange, TranFaultReport};
pub use diagnose::diagnose;
pub use ratings::{check_ratings, meets, package_power_limit, requirement, stress, RatingIssue, RatingLevel, Requirement, Stress};
pub use ripple::{ripple_reports, RippleReport, MAX_LIFE_HOURS, RIPPLE_HEATING_AT_RATED_C};
pub use dc::{dc_op, DcResult, ElementResult, SimError};
pub use measure::{stability_margins, wave_measure, Margins, WaveMeasure};
pub use montecarlo::{minus_3db, monte_carlo, McMeasure, McResult, McSettings, McStats, TranMetric};
pub use noise::{describe_source, integrate_psd, noise_analysis, source_element, NoiseContribution, NoiseKind, NoiseResult, NoiseSettings, SourceInfo};
pub use from_schematic::{from_schematic, from_schematic_with_pin_fault, ConvertError, PinFaultSpec, PinHow};
pub use spice_in::{add_model_instance, add_subckt_instance, decode_spice_text, ignored_model_params, parse_models};
pub use temperature::{temp_sweep, T_NOM_C};
pub use tran::{transient, Signal, TranResult, TranSettings};
pub use stepmetrics::{step_metrics, StepMetrics};
pub use power::{efficiency, mean_power};
pub use spec::{compare_measured, evaluate as evaluate_spec, in_range, measure as measure_spec, Agreement, Inputs as SpecInputs, MeasuredCompare, Spec, SpecEval, SpecKind};
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
mod tests_flat;
#[cfg(test)]
mod tests_vendor_bulk;
#[cfg(test)]
mod tests_tempco;
#[cfg(test)]
mod tests_saturation;
#[cfg(test)]
mod tests_bjt;
#[cfg(test)]
mod tests_diode_cap;
#[cfg(test)]
mod tests_params;
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
mod tests_parts_power;
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
