use super::*;

// ------------------------------------------------------------------ step conditions

pub(super) fn dc_fresh(e: &SchEditor) -> bool {
    e.dc.as_ref().is_some_and(|d| d.outcome.is_ok() && d.signature == e.signature())
}

pub(super) fn dc_ok(e: &SchEditor) -> bool {
    e.dc.as_ref().is_some_and(|d| d.outcome.is_ok())
}

pub(in crate::schematic) fn value_of<'a>(s: &'a Schematic, refdes: &str) -> Option<&'a str> {
    s.components.iter().find(|c| c.refdes == refdes).map(|c| c.value.as_str())
}

pub(super) fn changed(e: &SchEditor, refdes: &str) -> bool {
    value_of(&e.sch, refdes) != value_of(&e.lesson_base, refdes)
}

pub(super) fn r2_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R2") && dc_fresh(e)
}

pub(super) fn r3_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R3") && dc_fresh(e)
}

/// The follower lesson's last step: the load is down to 22 ohm (or less) and the result is fresh.
pub(super) fn follower_heavy_load(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R2").and_then(parse_value).is_some_and(|v| v <= 30.0)
}

/// The 78xx lesson's last load step: the load is down to 12 ohm (or less) and the result is fresh.
pub(super) fn regulator_heavy_load(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R1").and_then(parse_value).is_some_and(|v| v <= 12.0)
}

/// The heat lesson: the "熱 (温度と放熱板)" calculator is open.
pub(super) fn heat_calc_open(e: &SchEditor) -> bool {
    e.wiz_open && super::wizard::CALCS.get(e.wiz_kind).is_some_and(|c| c.title.starts_with("熱"))
}

/// The heat lesson's last step: the heavy load stays, the input is lowered to 8.5 V or less, and the result is fresh.
pub(super) fn input_lowered_under_load(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("R1").is_some_and(|r| r <= 12.0) && v("V1").is_some_and(|x| x <= 8.5)
}

/// The protection lesson: the supply is connected backwards (V1 negative) and the result is fresh.
pub(super) fn supply_reversed(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "V1").and_then(parse_value).is_some_and(|v| v < 0.0)
}

/// The protection lesson: the supply is right again and R2 (behind the diode) is nearly a short.
pub(super) fn diode_branch_shorted(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("V1").is_some_and(|x| x > 0.0) && v("R2").is_some_and(|r| r <= 2.0)
}

/// The protection lesson: the fuse calculator is open.
pub(super) fn fuse_calc_open(e: &SchEditor) -> bool {
    e.wiz_open && super::wizard::CALCS.get(e.wiz_kind).is_some_and(|c| c.title.starts_with("ヒューズ"))
}

/// The layer-short lesson: the chain trace is of a small layer short (a third of the turns or less) that left the driver alone.
pub(super) fn chain_small_short(e: &SchEditor) -> bool {
    match &e.fault_chain {
        Some(Ok(r)) => matches!(r.steps[0].fault, sim_core::Fault::TurnShort(x) if x <= 0.3) && !r.steps.iter().any(|s| s.part == "Q1"),
        _ => false,
    }
}

/// The layer-short lesson: the chain trace is of a half-turn short and the driver Q1 died on the way.
pub(super) fn chain_big_short(e: &SchEditor) -> bool {
    match &e.fault_chain {
        Some(Ok(r)) => matches!(r.steps[0].fault, sim_core::Fault::TurnShort(x) if x >= 0.5) && r.steps.iter().any(|s| s.part == "Q1"),
        _ => false,
    }
}

pub(super) fn r1_changed_and_measured(e: &SchEditor) -> bool {
    changed(e, "R1") && dc_fresh(e)
}

pub(super) fn live_on(e: &SchEditor) -> bool {
    e.live && dc_ok(e)
}

pub(super) fn mc_done(e: &SchEditor) -> bool {
    e.mc.as_ref().is_some_and(|m| m.outcome.is_ok())
}

pub(super) fn temp_sweep_done(e: &SchEditor) -> bool {
    e.sweep.as_ref().is_some_and(|s| s.temp && s.outcome.is_ok())
}

pub(super) fn ac_done(e: &SchEditor) -> bool {
    e.ac.as_ref().is_some_and(|a| a.outcome.is_ok())
}

pub(super) fn ac_fresh(e: &SchEditor) -> bool {
    e.ac.as_ref().is_some_and(|a| a.outcome.is_ok() && a.signature == e.signature())
}

/// (lowest frequency, highest frequency, number of points) of the sweep the last AC run really used.
fn ac_run_range(e: &SchEditor) -> Option<(f64, f64, usize)> {
    let r = e.ac.as_ref()?.outcome.as_ref().ok()?;
    Some((*r.freq.first()?, *r.freq.last()?, r.freq.len()))
}

/// The crystal lesson: a fresh run over 1 MHz - 100 MHz with few points (the narrow peak falls between them).
pub(super) fn wide_ac_fresh(e: &SchEditor) -> bool {
    ac_fresh(e) && ac_run_range(e).is_some_and(|(f0, f1, n)| f0 <= 2e6 && f1 >= 50e6 && n <= 1000)
}

/// The crystal lesson: R1 is changed and a fresh run covers the narrow range around 16 MHz finely.
pub(super) fn r1_changed_and_narrow_ac(e: &SchEditor) -> bool {
    changed(e, "R1") && ac_fresh(e) && ac_run_range(e).is_some_and(|(f0, f1, n)| f0 >= 15e6 && f1 <= 17e6 && n >= 300)
}

/// Largest value of a signal in the last transient run.
fn tran_peak(e: &SchEditor, name: &str) -> Option<f64> {
    let r = e.tran.as_ref()?.outcome.as_ref().ok()?;
    r.signal(name).map(|s| s.values.iter().copied().fold(f64::MIN, f64::max))
}

/// The reset lesson: C1 is a small capacitor (500 pF or less) and the run is fresh.
pub(super) fn c1_small_and_tran(e: &SchEditor) -> bool {
    tran_fresh(e) && value_of(&e.sch, "C1").and_then(parse_value).is_some_and(|c| c <= 500e-12)
}

/// The reset lesson: C1 is back at 100 nF or more, R1 is a weak pull-up (50k or more) and the run is fresh.
pub(super) fn r1_weak_and_tran(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    tran_fresh(e) && v("C1").is_some_and(|c| c >= 50e-9) && v("R1").is_some_and(|r| r >= 50e3)
}

/// The reset lesson: a diode with its anode on RESET and its cathode on VCC, and a fresh run whose RESET stays below 6.5 V.
pub(super) fn reset_clamped(e: &SchEditor) -> bool {
    let nets = sch_core::extract(&e.sch);
    let on = |net: &str, refdes: &str, pin: &str| nets.iter().any(|n| n.name == net && n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin));
    let diode = e.sch.components.iter().any(|c| c.refdes.starts_with('D') && on("RESET", &c.refdes, "A") && on("VCC", &c.refdes, "K"));
    diode && tran_fresh(e) && tran_peak(e, "V(RESET)").is_some_and(|p| p < 6.5)
}

/// The decoupling lesson: the parallel pair V(ZP) is on the AC graph and the run is fresh.
pub(super) fn zp_shown(e: &SchEditor) -> bool {
    ac_fresh(e) && e.ac.as_ref().is_some_and(|a| a.selected.contains("V(ZP)"))
}

/// The pull-up lesson, step 2: the button (R2) is pressed (a few ohms) and the result is fresh.
pub(super) fn button_pressed(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R2").and_then(parse_value).is_some_and(|r| r <= 100.0)
}

/// The pull-up lesson, step 3: the button is open again, the pull-up is a normal one (100k or less), a fresh transient.
pub(super) fn pullup_with_noise(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    tran_fresh(e) && v("R2").is_some_and(|r| r >= 1e6) && v("R1").is_some_and(|r| r <= 100e3)
}

/// The pull-up lesson, step 4: the pull-up is taken away (10 Meg or more) and the transient is fresh.
pub(super) fn pullup_removed(e: &SchEditor) -> bool {
    tran_fresh(e) && value_of(&e.sch, "R1").and_then(parse_value).is_some_and(|r| r >= 10e6)
}

/// The pin-LED lesson, step 2: the series resistor is 100 ohm or less and the result is fresh.
pub(super) fn pin_overdriven(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R1").and_then(parse_value).is_some_and(|r| r <= 100.0)
}

/// The pin-LED lesson, step 3: the series resistor is 800 ohm or more and the result is fresh.
pub(super) fn pin_gentle(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R1").and_then(parse_value).is_some_and(|r| r >= 800.0)
}

/// The "LEDの電流制限抵抗" calculator is open.
pub(super) fn led_calc_open(e: &SchEditor) -> bool {
    e.wiz_open && super::wizard::CALCS.get(e.wiz_kind).is_some_and(|c| c.title.starts_with("LED"))
}

/// The level-shift lesson, step 2: the divider's lower resistor R3 is 8k or more (a wrong ratio) and the run is fresh.
pub(super) fn divider_ratio_wrong(e: &SchEditor) -> bool {
    tran_fresh(e) && value_of(&e.sch, "R3").and_then(parse_value).is_some_and(|r| r >= 8e3)
}

/// The level-shift lesson, step 3: the divider is right again (3k or less) and the 3.3 V side pull-up R5 is weak (50k or more).
pub(super) fn level_pullup_weak(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    tran_fresh(e) && v("R3").is_some_and(|r| r <= 3e3) && v("R5").is_some_and(|r| r >= 50e3)
}

/// The serial lesson, step 2: a big cable capacitance (50n or more) with the 1k cable resistance, a fresh run.
pub(super) fn uart_cable_slow(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    tran_fresh(e) && v("C1").is_some_and(|c| c >= 50e-9) && v("R2").is_some_and(|r| r >= 500.0)
}

/// The serial lesson, step 3: the same capacitance, but the cable resistance is 200 ohm or less, a fresh run.
pub(super) fn uart_cable_fast(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    tran_fresh(e) && v("C1").is_some_and(|c| c >= 50e-9) && v("R2").is_some_and(|r| r <= 200.0)
}

/// The data sheet lesson, step 1: the diode D1 is selected and the DC result is fresh (the "部品選びのヒント" has numbers).
pub(super) fn d1_selected_and_measured(e: &SchEditor) -> bool {
    dc_fresh(e) && matches!(e.selected, Some(Sel::Comp(i)) if e.sch.components.get(i).is_some_and(|c| c.refdes == "D1"))
}

/// The data sheet lesson: R1 is between `lo` and `hi` ohm and the DC result is fresh.
fn r1_between(e: &SchEditor, lo: f64, hi: f64) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R1").and_then(parse_value).is_some_and(|r| (lo..=hi).contains(&r))
}

/// Step 2: about 33 ohm (125 mA: inside the absolute maximum, short of the margin of 2).
pub(super) fn diode_inside_but_no_margin(e: &SchEditor) -> bool {
    r1_between(e, 28.0, 40.0)
}

/// Step 3: about 22 ohm (185 mA: 93 % of the maximum).
pub(super) fn diode_near_the_limit(e: &SchEditor) -> bool {
    r1_between(e, 18.0, 26.0)
}

/// Step 4: 16 ohm or less (above the maximum).
pub(super) fn diode_over_the_limit(e: &SchEditor) -> bool {
    r1_between(e, 0.1, 16.0)
}

/// The USB power lesson, step 2: the device draws nearly an ampere (R3 = 6 ohm or less) and the result is fresh.
pub(super) fn vbus_heavy_load(e: &SchEditor) -> bool {
    dc_fresh(e) && value_of(&e.sch, "R3").and_then(parse_value).is_some_and(|r| r <= 6.0)
}

/// The USB power lesson, step 3: a thin cable (R2 = 1 ohm or more) with a half-ampere load (R3 between 8 and 12 ohm), fresh.
pub(super) fn vbus_thin_cable(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("R2").is_some_and(|r| r >= 1.0) && v("R3").is_some_and(|r| (8.0..=12.0).contains(&r))
}

/// The USB power lesson, step 4: a big bulk capacitor (C1 = 50 uF or more) plugged in from zero (the transient starts from zero).
pub(super) fn vbus_big_capacitor_plugged(e: &SchEditor) -> bool {
    tran_fresh(e) && e.tran_uic && value_of(&e.sch, "C1").and_then(parse_value).is_some_and(|c| c >= 50e-6)
}

/// The USB speed lesson, step 2: the 1.5k pull-up is on D- (R2) and none on D+ (R1 100Meg or more), fresh.
pub(super) fn usb_low_speed(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("R1").is_some_and(|r| r >= 1e6) && v("R2").is_some_and(|r| r <= 3e3)
}

/// The USB speed lesson, step 3: no pull-up on either line (nothing is plugged in), fresh.
pub(super) fn usb_nothing_plugged(e: &SchEditor) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("R1").is_some_and(|r| r >= 1e6) && v("R2").is_some_and(|r| r >= 1e6)
}

/// The USB signal lesson: C2 (the ESD protection's capacitance) is between `lo` and `hi` and the run is fresh.
fn c2_between(e: &SchEditor, lo: f64, hi: f64) -> bool {
    tran_fresh(e) && value_of(&e.sch, "C2").and_then(parse_value).is_some_and(|c| (lo..=hi).contains(&c))
}

/// Step 2: an ESD part of about 200 pF (the edge is out of the full-speed range).
pub(super) fn esd_too_big(e: &SchEditor) -> bool {
    c2_between(e, 150e-12, 300e-12)
}

/// Step 3: an ESD part of 400 pF or more (the bit no longer settles).
pub(super) fn esd_far_too_big(e: &SchEditor) -> bool {
    c2_between(e, 400e-12, 1e-6)
}

/// The PWM lesson: C1 is between `lo` and `hi` farad and the run is fresh.
fn pwm_c1_between(e: &SchEditor, lo: f64, hi: f64) -> bool {
    tran_fresh(e) && value_of(&e.sch, "C1").and_then(parse_value).is_some_and(|c| (lo..=hi).contains(&c))
}

/// Step 2: a small filter capacitor (200 nF or less): the ripple stays big.
pub(super) fn pwm_small_capacitor(e: &SchEditor) -> bool {
    pwm_c1_between(e, 1e-12, 200e-9)
}

/// Step 3: a big filter capacitor (5 uF or more): quiet but slow.
pub(super) fn pwm_big_capacitor(e: &SchEditor) -> bool {
    pwm_c1_between(e, 5e-6, 1.0)
}

/// Step 4: the 1 uF filter again, with a load of 20k or less on the output: the level falls.
pub(super) fn pwm_loaded(e: &SchEditor) -> bool {
    pwm_c1_between(e, 0.5e-6, 2e-6) && value_of(&e.sch, "R2").and_then(parse_value).is_some_and(|r| r <= 20e3)
}

/// The LDO lesson: the input V1 is between `lo` and `hi` volt and the DC result is fresh.
fn ldo_input_between(e: &SchEditor, lo: f64, hi: f64) -> bool {
    dc_fresh(e) && value_of(&e.sch, "V1").and_then(parse_value).is_some_and(|v| (lo..=hi).contains(&v))
}

/// Step 2: the input is about 4 V (the 1.1 V dropout part cannot hold 3.3 V any more).
pub(super) fn ldo_input_4v(e: &SchEditor) -> bool {
    ldo_input_between(e, 3.8, 4.2)
}

/// Step 3: the input is down at the output voltage (3.4 V or less).
pub(super) fn ldo_input_low(e: &SchEditor) -> bool {
    ldo_input_between(e, 0.0, 3.4)
}

/// Step 4: a 12 V input (10 V or more).
pub(super) fn ldo_input_high(e: &SchEditor) -> bool {
    ldo_input_between(e, 10.0, 60.0)
}

/// The TL431 lesson: the DC result is fresh and R2 is between `lo2` and `hi2`, R4 between `lo4` and `hi4` (ohm).
fn tl431_values(e: &SchEditor, (lo2, hi2): (f64, f64), (lo4, hi4): (f64, f64)) -> bool {
    let v = |r: &str| value_of(&e.sch, r).and_then(parse_value);
    dc_fresh(e) && v("R2").is_some_and(|r| (lo2..=hi2).contains(&r)) && v("R4").is_some_and(|r| (lo4..=hi4).contains(&r))
}

/// Step 2: the upper divider resistor is about 15k (a 6.2 V output), the load stays light.
pub(super) fn tl431_six_volts(e: &SchEditor) -> bool {
    tl431_values(e, (12e3, 20e3), (10e3, 1e9))
}

/// Step 3: the divider is back at 10k and the load is about 330 ohm (15 mA).
pub(super) fn tl431_loaded(e: &SchEditor) -> bool {
    tl431_values(e, (9e3, 11e3), (200.0, 500.0))
}

/// Step 4: a load of 150 ohm or less (more than the bias resistor can supply).
pub(super) fn tl431_overloaded(e: &SchEditor) -> bool {
    tl431_values(e, (9e3, 11e3), (1.0, 150.0))
}

/// The flyback lesson, step 2: a diode with its anode on DRAIN and its cathode on VCC, a fresh run whose DRAIN stays below 20 V.
pub(super) fn flyback_diode_added(e: &SchEditor) -> bool {
    let nets = sch_core::extract(&e.sch);
    let on = |net: &str, refdes: &str, pin: &str| nets.iter().any(|n| n.name == net && n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin));
    let diode = e.sch.components.iter().any(|c| c.refdes.starts_with('D') && on("DRAIN", &c.refdes, "A") && on("VCC", &c.refdes, "K"));
    diode && tran_fresh(e) && tran_peak(e, "V(DRAIN)").is_some_and(|p| p < 20.0)
}

/// Step 3: the diode is there and the coil is a big one (L1 500 mH or more); a fresh run.
pub(super) fn flyback_big_coil(e: &SchEditor) -> bool {
    flyback_diode_added(e) && value_of(&e.sch, "L1").and_then(parse_value).is_some_and(|l| l >= 0.5)
}

/// The debounce lesson: C1 is between `lo` and `hi` farad and the run is fresh.
fn bounce_c1_between(e: &SchEditor, lo: f64, hi: f64) -> bool {
    tran_fresh(e) && value_of(&e.sch, "C1").and_then(parse_value).is_some_and(|c| (lo..=hi).contains(&c))
}

/// Step 2: a debounce capacitor of 50 nF to 500 nF (with the 10k).
pub(super) fn debounce_right_size(e: &SchEditor) -> bool {
    bounce_c1_between(e, 50e-9, 500e-9)
}

/// Step 3: a far too big capacitor (800 nF or more).
pub(super) fn debounce_too_big(e: &SchEditor) -> bool {
    bounce_c1_between(e, 800e-9, 1.0)
}

pub(super) fn tran_fresh(e: &SchEditor) -> bool {
    e.tran.as_ref().is_some_and(|t| t.outcome.is_ok() && t.signature == e.signature())
}

pub(super) fn c1_changed_and_ac(e: &SchEditor) -> bool {
    changed(e, "C1") && ac_fresh(e) && e.wave_mode == WaveMode::Ac
}

pub(super) fn mc_cutoff_done(e: &SchEditor) -> bool {
    e.mc_kind == McKind::Ac && mc_done(e)
}

pub(super) fn tran_done(e: &SchEditor) -> bool {
    e.tran.as_ref().is_some_and(|t| t.outcome.is_ok())
}

pub(super) fn c1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "C1") && tran_fresh(e)
}

pub(super) fn r2_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "R2") && tran_fresh(e)
}

/// The variable-supply lesson: the dial (R2) is turned to 2.8k or more and the transient is fresh.
pub(super) fn r2_dial_too_high(e: &SchEditor) -> bool {
    tran_fresh(e) && value_of(&e.sch, "R2").and_then(parse_value).is_some_and(|r| r >= 2800.0)
}

pub(super) fn l1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "L1") && tran_fresh(e)
}

pub(super) fn r3_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "R3") && tran_fresh(e)
}

pub(super) fn r1_restored_and_tran(e: &SchEditor) -> bool {
    value_of(&e.sch, "R1") == value_of(&e.lesson_base, "R1") && tran_fresh(e)
}

pub(super) fn r1_changed_and_tran(e: &SchEditor) -> bool {
    changed(e, "R1") && tran_fresh(e)
}

pub(super) fn mc_overshoot_done(e: &SchEditor) -> bool {
    e.mc_kind == McKind::Tran && e.mc_metric == TranMetric::Overshoot && mc_done(e)
}

pub(super) fn sweep_done(e: &SchEditor) -> bool {
    e.sweep.as_ref().is_some_and(|s| !s.temp && s.outcome.is_ok())
}

pub(super) fn gain_restored_and_ac(e: &SchEditor) -> bool {
    value_of(&e.sch, "R1") == value_of(&e.lesson_base, "R1") && ac_fresh(e) && e.wave_mode == WaveMode::Ac
}

pub(super) fn currents_shown(e: &SchEditor) -> bool {
    e.show_i && dc_fresh(e)
}
