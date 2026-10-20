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
