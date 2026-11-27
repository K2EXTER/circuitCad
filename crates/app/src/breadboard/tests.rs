use super::*;
use crate::{App, Mode};

#[test]
fn the_copper_side_is_the_board_mirrored_and_a_lead_clicked_there_picks_its_part() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Stripboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let l = app.breadboard.strip_shown(&app.sch);
    let (front, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
    // F over the board turns it over
    let middle = front + Vec2::new(l.cols as f32 / 2.0, l.rows as f32 / 2.0) * pitch;
    let key = egui::Event::Key { key: egui::Key::F, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
    run(&mut app, vec![egui::Event::PointerMoved(middle)]);
    run(&mut app, vec![key]);
    assert!(app.breadboard.strip.back, "F turned the board over");
    run(&mut app, vec![]);
    let (origin, _) = app.breadboard.strip.drawn.unwrap();
    // U1's pin 1, seen from below: its column counted from the right
    let pin1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap().leads.iter().find(|(p, _)| p == "1").unwrap().1;
    let at = origin + Vec2::new((l.cols - 1 - pin1.1) as f32, pin1.0 as f32) * pitch;
    let button = |pressed: bool| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    let before = app.sch.stripboard.clone();
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![button(true)]);
    run(&mut app, vec![button(false)]);
    assert_eq!(app.breadboard.picked.as_deref(), Some("U1"), "the lead clicked is U1's pin 1");
    assert_eq!(app.sch.stripboard, before, "nothing is edited on the copper side");
}

/// From the copper side of a perfboard: the bare wire tool draws a bare wire between two holes of a row (without the layout being made
/// again: the frame after the edit takes far less than the automatic layout's 1.5 s), a click with the move tool picks it, Delete takes
/// it away; on the parts' side the hole under the pointer names its net. (On a board put by a fixed rule, `plain_perfboard`: its top
/// row is free, whatever the automatic layout does.)
#[test]
fn bare_wires_are_drawn_and_taken_away_from_the_copper_side() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.sch.stripboard = Some(sch_core::strip_edit::plain_perfboard(&app.sch.sch));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let l = app.breadboard.strip_shown(&app.sch);
    app.breadboard.strip.back = true;
    app.breadboard.strip.tool = strip_view::StripTool::Bare;
    run(&mut app, vec![]);
    let (origin, pitch) = app.breadboard.strip.drawn.unwrap();
    let below = |a: (u16, u16)| origin + Vec2::new((l.cols - 1 - a.1) as f32, a.0 as f32) * pitch;
    let click = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), at: Pos2| {
        run(app, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            run(app, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
        }
    };
    // the top row is free: a wire from its second hole to its fifth
    let (row, c0) = (0, 0);
    click(&mut app, &mut run, below((row, c0 + 1)));
    let started = std::time::Instant::now();
    click(&mut app, &mut run, below((row, c0 + 4)));
    run(&mut app, vec![]);
    let took = started.elapsed();
    let m = app.sch.stripboard.clone().expect("the wire is kept with the sheet");
    assert!(m.bare.contains(&((row, c0 + 1), (row, c0 + 4))), "{:?}", m.bare);
    assert!(took < std::time::Duration::from_millis(900), "the layout was made again after the wire: {took:?}");
    // picked with the move tool (a click on its middle), and Delete
    app.breadboard.strip.tool = strip_view::StripTool::Move;
    let mid = below((row, c0 + 2)) + Vec2::new(-pitch * 0.5, 0.0);
    click(&mut app, &mut run, mid);
    assert!(app.breadboard.strip.picked_bare.is_some(), "the wire is picked");
    run(&mut app, vec![egui::Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    assert!(!app.sch.stripboard.clone().unwrap().bare.contains(&((row, c0 + 1), (row, c0 + 4))), "Delete took it away");
}

#[test]
fn the_checks_kept_between_frames_follow_an_edit_to_the_schematic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    let (l, strip) = (auto_layout(&app.sch.sch), sch_core::stripboard::strip_layout(&app.sch.sch));
    let bb = |app: &mut App| app.breadboard.board_check(&app.sch, &l).len();
    let rules = |app: &mut App| app.breadboard.strip_checks(&app.sch, &strip).rules.len();
    let (bb0, rules0) = (bb(&mut app), rules(&mut app));
    assert_eq!(bb0, 0, "the breadboard as laid out matches");
    // edits not yet an undo step (the schematic tab commits them later): the edit count is the same, the kept checks are not used
    app.sch.sch.components.iter_mut().find(|c| c.refdes == "R1").unwrap().refdes = "R9".into();
    app.sch.sch.requirements.apart.push(design_spec::KeepApart { name: "test".into(), a: vec!["R2".into()], b: vec!["D1".into()], min_dist: 1000.0, note: String::new() });
    assert!(bb(&mut app) > 0, "R9 is not on the breadboard");
    assert_eq!(rules(&mut app), rules0 + 1, "the rule added is checked");
    app.sch.commit_history_for_test();
    assert!(bb(&mut app) > 0 && rules(&mut app) == rules0 + 1, "the same once it is an undo step");
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    assert!(bb(&mut app) == 0 && rules(&mut app) == rules0, "checked again for the sheet opened");
}

/// The LED sample powered on the breadboard: the wires and strips round the loop carry the LED's current (the dots, X1).
#[test]
fn the_breadboard_wires_carry_the_led_current_while_it_runs() {
    let (mut app, mut run) = power_on_app("led.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    run(&mut app);
    let l = app.breadboard.layout(&app.sch);
    let (lv, b) = (app.breadboard.live.as_ref().expect("running"), app.breadboard.board.as_ref().unwrap());
    let i = lv.current("D1").unwrap_or(0.0);
    assert!(i > 5e-3, "{i}");
    let pieces = flow_dots::board_flows(&l, &b.nets, lv);
    // the supply's two wires off the board carry the whole current, one out and one back
    let off: Vec<f64> = pieces.iter().filter(|p| matches!(p.a, flow_dots::End::Off(..))).map(|p| p.amps).collect();
    assert_eq!(off.len(), 2, "{off:?}");
    assert!(off.iter().all(|a| (a.abs() - i).abs() < 1e-3 * i), "{off:?} vs {i}");
    assert!(off[0] * off[1] < 0.0, "one goes onto the board, the other comes back: {off:?}");
    // nothing on the board carries more than the loop's current
    assert!(pieces.iter().all(|p| p.amps.abs() < i * 1.001), "{:?}", pieces.iter().map(|p| p.amps).collect::<Vec<_>>());
}

fn power_on_app(sample: &str) -> (App, impl FnMut(&mut App)) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join(sample));
    app.mode = Mode::Proto;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let run = move |app: &mut App| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    (app, run)
}

/// A stripboard or perfboard runs powered as its copper joins it: the LED lights on the board as laid out; with a wire across
/// its leads (a solder bridge) it is dark.
#[test]
fn a_stripboard_runs_as_its_copper_joins_it() {
    for kind in [BoardKind::Perfboard, BoardKind::Stripboard] {
        let (mut app, mut run) = power_on_app("led.schjson");
        app.breadboard.kind = kind;
        app.breadboard.powered = true;
        run(&mut app);
        run(&mut app);
        let led = |app: &App| app.breadboard.live.as_ref().expect("running").current("D1").unwrap_or(0.0);
        let lit = led(&app);
        assert!(lit > 5e-3, "{kind:?}: the LED lights: {lit} A");
        assert!(burnt(&app).is_empty());
        // a wire across the LED's two leads (a solder bridge): it goes out
        let mut l = app.breadboard.strip_shown(&app.sch);
        let d1 = l.parts.iter().find(|p| p.refdes == "D1").unwrap().leads.clone();
        l.links.push(sch_core::stripboard::Link::new(d1[0].1, d1[1].1, String::new()));
        app.sch.stripboard = Some(l);
        run(&mut app);
        run(&mut app);
        assert!(led(&app) < 1e-4, "{kind:?}: shorted, the LED is dark: {} A", led(&app));
    }
}

/// On a perfboard the meter's probes go on the holes clicked (red, then black) and read the board's circuit: across a resistor
/// with the power off, its resistance; the probes are also put on from the copper side.
#[test]
fn the_meter_probes_a_perfboard_from_either_side() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("led.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    app.breadboard.strip.tool = strip_view::StripTool::Meter;
    app.breadboard.meter_range = MeterRange::Ohms;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let l = app.breadboard.strip_shown(&app.sch);
    let r1 = l.parts.iter().find(|p| p.refdes == "R1").unwrap().leads.clone();
    let mut click = |app: &mut App, at: sch_core::stripboard::At, back: bool| {
        let (origin, pitch) = app.breadboard.strip.drawn.unwrap();
        let col = if back { l.cols - 1 - at.1 } else { at.1 };
        let pos = origin + Vec2::new(col as f32, at.0 as f32) * pitch;
        let button = |pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(app, vec![egui::Event::PointerMoved(pos)]);
        run(app, vec![button(true)]);
        run(app, vec![button(false)]);
        run(app, vec![]);
    };
    click(&mut app, r1[0].1, false);
    click(&mut app, r1[1].1, false);
    let reading = |app: &App| app.breadboard.meter_cache.as_ref().and_then(|(_, r)| r.clone().ok()).unwrap_or_default();
    let ohms = |r: &str| sim_core::parse_value(r.trim_end_matches('Ω')).unwrap_or(0.0);
    let front = reading(&app);
    assert!(front.ends_with('Ω') && ohms(&front) > 100.0, "across R1: {front}");
    // the same two holes from below
    app.breadboard.strip.back = true;
    app.breadboard.meter_cache = None;
    click(&mut app, r1[1].1, true);
    click(&mut app, r1[0].1, true);
    assert_eq!(ohms(&reading(&app)), ohms(&front), "from the copper side: {}", reading(&app));
}

/// "つながる順に並べる" on the perfboard: the blinker's two resistors lined up and locked, the rest laid out again round them on
/// its thread, the board joined as drawn.
#[test]
fn picked_parts_line_up_and_the_rest_is_laid_out_round_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app);
    let mut l = app.breadboard.strip_shown(&app.sch);
    for _ in 0..200 {
        if !l.parts.is_empty() && !app.breadboard.strip_busy().0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        run(&mut app);
        l = app.breadboard.strip_shown(&app.sch);
    }
    let rs: Vec<String> = vec!["R1".into(), "R2".into()];
    app.breadboard.strip_align(&mut app.sch, &l, &rs, sch_core::strip_edit::StripAlign::NetOrder);
    assert!(app.breadboard.strip_busy().0, "laying the rest out: {}", app.sch.status);
    for _ in 0..300 {
        if !app.breadboard.strip_busy().0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        run(&mut app);
    }
    let m = app.sch.stripboard.clone().expect("kept with the sheet");
    assert!(rs.iter().all(|r| m.locked.contains(r)), "{:?}", m.locked);
    assert!(sch_core::stripboard::strip_check(&app.sch.sch, &m).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &m));
}

fn burnt(app: &App) -> Vec<String> {

    app.breadboard.issues().iter().filter(|r| r.level == sim_core::RatingLevel::Over).map(|r| r.refdes.clone()).collect()
}

/// The power switch works the board's DC point out by itself and the circuit runs; the meter reads the supply; an LED whose
/// resistor is far too small burns.
#[test]
fn switching_on_burns_an_led_without_enough_resistance_and_the_meter_reads() {
    let (mut app, mut run) = power_on_app("ne555_blinker.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    assert!(matches!(&app.breadboard.board, Some(Board { dc: Ok(_), .. })), "switching on worked the DC point out");
    assert!(app.breadboard.live.as_ref().is_some_and(|v| v.error.is_none()), "and the circuit runs");
    // the meter on the supply rail against ground reads the supply
    let l = auto_layout(&app.sch.sch);
    app.breadboard.tool = Tool::Meter;
    app.breadboard.red = Some(Hole::Rail { rail: Rail::TopPlus, col: 10 });
    app.breadboard.black = Some(Hole::Rail { rail: Rail::TopMinus, col: 10 });
    run(&mut app);
    assert_eq!(net_at(&app.sch.sch, &l, app.breadboard.red.unwrap()).as_deref(), Some("VCC"));
    assert!(app.breadboard.live.as_ref().and_then(|v| v.voltage("VCC")).is_some_and(|v| v > 4.0));
    // the power-on sample's LED resistor (R1, 330 ohm from 5 V) made 1 ohm: the LED is past its rating and shows as burnt
    let (mut app, mut run) = power_on_app("power_on.schjson");
    let led_r = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
    app.sch.sch.components[led_r].value = "1".into();
    app.breadboard.powered = true;
    run(&mut app);
    assert!(burnt(&app).iter().any(|r| r.starts_with('D')), "{:?}", burnt(&app));
}

/// The meter's other ranges: resistance with the power off (R1's two leads read R1 with what is beside it in the circuit, an
/// empty strip reads OL), and refuses with the power on; current with the power on, which blows the fuse across the supply rails
/// and reads nothing with both probes on one strip.
#[test]
fn the_meter_reads_ohms_with_the_power_off_and_amps_in_series() {
    let (mut app, mut run) = power_on_app("power_on.schjson");
    let l = auto_layout(&app.sch.sch);
    let r1 = l.parts.iter().find(|p| p.refdes == "R1").unwrap();
    let reading = |app: &crate::App| app.breadboard.meter_cache.as_ref().map(|(_, r)| r.clone());
    app.breadboard.tool = Tool::Meter;
    app.breadboard.meter_range = MeterRange::Ohms;
    (app.breadboard.red, app.breadboard.black) = (Some(r1.leads[0].1), Some(r1.leads[1].1));
    run(&mut app);
    let r = reading(&app).unwrap().unwrap();
    let ohms = sim_core::parse_value(r.trim_end_matches('Ω')).unwrap_or_else(|| panic!("{r}"));
    assert!(ohms > 10.0 && ohms <= 331.0, "R1 is 330 ohm, with the circuit beside it no more: {r}");
    // the power on: the resistance range refuses (the reading is not worked out)
    app.breadboard.powered = true;
    app.breadboard.meter_cache = None;
    run(&mut app);
    assert!(reading(&app).is_none());
    // current across the supply rails: the fuse
    app.breadboard.meter_range = MeterRange::Amps;
    let plus = l.parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1)).find(|h| net_at(&app.sch.sch, &l, *h).as_deref() == Some("VIN"));
    let gnd = Hole::Rail { rail: Rail::TopMinus, col: 3 };
    let plus = plus.expect("a lead on VIN");
    (app.breadboard.red, app.breadboard.black) = (Some(plus), Some(gnd));
    run(&mut app);
    assert!(reading(&app).unwrap().unwrap().starts_with("FUSE"), "{:?}", reading(&app));
    // both probes on R1's first strip: nothing flows through the meter
    let same = r1.leads[0].1;
    (app.breadboard.red, app.breadboard.black) = (Some(same), Some(same));
    run(&mut app);
    assert_eq!(reading(&app).unwrap().unwrap(), "SAME");
}

/// The board is what is simulated: a jumper across the LED's resistor (not in the schematic) burns the LED, though the
/// schematic is fine; taking the jumper away again, it lives.
#[test]
fn a_jumper_across_the_resistor_burns_the_led_the_schematic_does_not() {
    let (mut app, mut run) = power_on_app("power_on.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    assert!(burnt(&app).is_empty(), "as drawn nothing burns: {:?}", burnt(&app));
    let l = auto_layout(&app.sch.sch);
    let r1 = l.parts.iter().find(|p| p.refdes == "R1").unwrap();
    let (a, b) = (r1.leads[0].1, r1.leads[1].1);
    // a free hole in the strip (or rail) of each lead
    let free = |h: Hole| -> Hole {
        let taken: BTreeSet<Hole> = l.parts.iter().flat_map(|p| p.leads.iter().map(|x| x.1)).chain(l.wires.iter().flat_map(|w| [w.a, w.b])).collect();
        let cands: Vec<Hole> = match h {
            Hole::Strip { col, row } => (if row < 5 { 0..5 } else { 5..10 }).map(|r| Hole::Strip { col, row: r }).collect(),
            Hole::Rail { rail, .. } => (0..l.cols).map(|c| Hole::Rail { rail, col: c }).collect(),
        };
        cands.into_iter().find(|x| !taken.contains(x)).expect("a free hole")
    };
    app.sch.breadboard = Some(with_wire(&app.sch.sch, &l, free(a), free(b)));
    run(&mut app);
    assert!(!check(&app.sch.sch, app.sch.breadboard.as_ref().unwrap()).is_empty(), "the board differs from the schematic");
    assert!(burnt(&app).iter().any(|r| r.starts_with('D')), "the LED burns: {:?}", burnt(&app));
    assert!(app.breadboard.live.as_ref().is_some_and(|v| v.error.is_none()), "and the mis-wired board still runs");
    app.sch.breadboard = Some(l.clone());
    run(&mut app);
    assert!(burnt(&app).is_empty(), "{:?}", burnt(&app));
    // a jumper from the + rail to the − rail: the supply is shorted, and says so
    let (plus, minus) = (Hole::Rail { rail: Rail::TopPlus, col: l.cols - 1 }, Hole::Rail { rail: Rail::TopMinus, col: l.cols - 1 });
    app.sch.breadboard = Some(with_wire(&app.sch.sch, &l, plus, minus));
    run(&mut app);
    let short: Vec<String> = app.breadboard.issues().iter().filter(|r| r.message.contains("ショート")).map(|r| r.refdes.clone()).collect();
    assert!(short.iter().any(|r| r.starts_with('V')), "{:?}", app.breadboard.issues());
}

/// The room's temperature slider: warmer, the thermistors' dividers move (the run starts again at the new temperature).
#[test]
fn a_warmer_room_moves_the_thermistor_dividers() {
    let (mut app, mut run) = power_on_app("thermistor_basics.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    let nets: BTreeSet<String> = board_nets(&app.sch.sch, &auto_layout(&app.sch.sch)).pins.into_values().collect();
    let read = |app: &App| -> Vec<f64> { nets.iter().map(|n| app.breadboard.live.as_ref().and_then(|v| v.voltage(n)).unwrap_or(0.0)).collect() };
    let cool = read(&app);
    app.breadboard.temp_c = Some(80.0);
    app.breadboard.run_key = None;
    run(&mut app);
    let warm = read(&app);
    assert!(cool.iter().zip(&warm).any(|(a, b)| (a - b).abs() > 0.2), "{cool:?} -> {warm:?}");
}

/// The rating check follows the running circuit: the supply's knob turned up past what the LED's resistor allows burns the LED
/// once it has stayed there a moment; turned back down, the LED stays burnt until the power is switched off.
#[test]
fn turning_the_supply_up_burns_the_led_while_it_runs() {
    let (mut app, mut run) = power_on_app("led.schjson");
    let r1 = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
    app.sch.sch.components[r1].value = "180".into();
    // a 1 W resistor: the LED is the part pushed past its rating (an 0805 one would burn open first and save the LED)
    app.sch.sch.components[r1].footprint = "R_2512".into();
    app.breadboard.powered = true;
    run(&mut app);
    assert!(burnt(&app).is_empty(), "about 16 mA at 5 V: {:?}", app.breadboard.issues());
    let turn = |app: &mut App, v: f64| {
        let lv = app.breadboard.live.as_mut().unwrap();
        let k = lv.knob_index("V1").expect("the supply is a knob");
        lv.set_knob(k, v);
    };
    let step = |app: &mut App, run: &mut dyn FnMut(&mut App)| {
        app.breadboard.live.as_mut().unwrap().advance(0.015);
        run(app);
    };
    turn(&mut app, 10.0);
    step(&mut app, &mut run);
    assert!(burnt(&app).is_empty(), "a moment past the rating does not burn it yet");
    for _ in 0..20 {
        step(&mut app, &mut run);
    }
    assert!(burnt(&app).contains(&"D1".to_string()), "{:?}", app.breadboard.issues());
    // the burnt LED opens: the run starts again with it broken, and no current flows through it any more
    run(&mut app);
    assert!(app.breadboard.broken.iter().any(|(r, f)| r == "D1" && *f == sim_core::Fault::Open), "{:?}", app.breadboard.broken);
    step(&mut app, &mut run);
    let i = app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0);
    assert!(i.abs() < 1e-6, "a burnt LED is dark: {i}");
    turn(&mut app, 5.0);
    step(&mut app, &mut run);
    assert!(burnt(&app).contains(&"D1".to_string()), "a burnt LED stays burnt");
    app.breadboard.powered = false;
    run(&mut app);
    app.breadboard.powered = true;
    run(&mut app);
    assert!(burnt(&app).is_empty(), "switched off and on with the knob back: a new start");
    assert!(app.breadboard.broken.is_empty());
}

/// With the small (0805) resistor, turning the supply up burns the resistor first: it opens, and the LED behind it survives.
#[test]
fn a_resistor_that_burns_open_saves_its_led() {
    let (mut app, mut run) = power_on_app("led.schjson");
    let r1 = app.sch.sch.components.iter().position(|c| c.refdes == "R1").unwrap();
    app.sch.sch.components[r1].value = "180".into();
    app.breadboard.powered = true;
    run(&mut app);
    {
        let lv = app.breadboard.live.as_mut().unwrap();
        let k = lv.knob_index("V1").unwrap();
        lv.set_knob(k, 10.0);
    }
    for _ in 0..30 {
        app.breadboard.live.as_mut().unwrap().advance(0.015);
        run(&mut app);
    }
    assert!(app.breadboard.broken.iter().any(|(r, f)| r == "R1" && *f == sim_core::Fault::Open), "{:?}", app.breadboard.broken);
    assert!(!burnt(&app).contains(&"D1".to_string()), "{:?}", app.breadboard.issues());
    assert!(app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0).abs() < 1e-6, "no current through the open resistor");
}

/// The potentiometer is a knob while it runs: turned towards pin 1 (5 V) the transistor lights the LED, towards pin 3 (GND)
/// it goes dark.
/// The buzzer sounds while its push button is held, louder with more voltage, not at all on too little or with the sound off.
#[test]
fn the_buzzer_sounds_while_its_button_is_held() {
    let (mut app, mut run) = power_on_app("buzzer_button.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    let step = |app: &App| {
        let l = auto_layout(&app.sch.sch);
        app.breadboard.buzz_step(&app.sch, &RunBoard::Bread(l))
    };
    assert_eq!(step(&app), None, "silent until the button is pressed");
    let lv = app.breadboard.live.as_mut().unwrap();
    let k = lv.switch_index("SW1").unwrap();
    lv.press(k);
    lv.advance(0.01);
    run(&mut app);
    let l = auto_layout(&app.sch.sch);
    let v = app.breadboard.buzzers(&app.sch, &RunBoard::Bread(l.clone()));
    assert!(v.iter().any(|(r, v)| r == "BZ1" && *v > 4.5), "{v:?}");
    assert_eq!(step(&app), Some(4), "loud at 5 V");
    assert!(app.breadboard.beeper.is_playing());
    // the supply turned down to 2.7 V: quieter; to 2 V: silent
    let lv = app.breadboard.live.as_mut().unwrap();
    let s = lv.knob_index("V1").unwrap();
    lv.set_knob(s, 2.7);
    lv.advance(0.01);
    assert_eq!(step(&app), Some(1));
    let lv = app.breadboard.live.as_mut().unwrap();
    lv.set_knob(s, 2.0);
    lv.advance(0.01);
    assert_eq!(step(&app), None);
    let lv = app.breadboard.live.as_mut().unwrap();
    lv.set_knob(s, 5.0);
    lv.advance(0.01);
    app.breadboard.sound_off = true;
    assert_eq!(step(&app), None, "the learner turned the sound off");
    app.breadboard.sound_off = false;
    // released: silent again; leaving the tab stops the tone
    app.breadboard.live.as_mut().unwrap().release();
    app.breadboard.live.as_mut().unwrap().advance(0.01);
    assert_eq!(step(&app), None);
    app.breadboard.live.as_mut().unwrap().press(k);
    app.breadboard.live.as_mut().unwrap().advance(0.01);
    run(&mut app);
    assert!(app.breadboard.beeper.is_playing());
    app.mode = Mode::Schematic;
    run(&mut app);
    assert!(!app.breadboard.beeper.is_playing());
}

/// The pot is turned on the board itself: a drag to the right or up turns it on, a wheel notch by 5 %.
#[test]
fn the_pot_turns_by_a_drag_on_the_board() {
    assert!((knob_dragged(0.3, Pos2::new(100.0, 100.0), Pos2::new(140.0, 100.0)) - 0.5).abs() < 1e-9, "40 points right: a fifth of a turn");
    assert!((knob_dragged(0.3, Pos2::new(100.0, 100.0), Pos2::new(100.0, 160.0)) - 0.0).abs() < 1e-9, "down past the end stops at 0");
    assert_eq!(knob_dragged(0.9, Pos2::new(0.0, 0.0), Pos2::new(0.0, -100.0)), 1.0);
    // on the running sample, with the pointer: press on VR1 (at 30 %), drag 60 points right, let go, then a wheel notch up
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("pot_dimmer.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.powered = true;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    // on the drawn knob (its lower edge, in the middle gap): away from the leads, where the hover help says to turn it
    let drawn = app.breadboard.drawn.iter().find(|(r, _)| r == "VR1").unwrap().1;
    let at = Pos2::new(drawn.center().x, drawn.bottom() - 3.0);
    let knob = |app: &App| app.breadboard.live.as_ref().and_then(|v| v.knobs.get(v.knob_index("VR1")?)).unwrap().value;
    assert!((knob(&app) - 0.3).abs() < 1e-9);
    let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![button(at, true)]);
    let to = at + Vec2::new(60.0, 0.0);
    run(&mut app, vec![egui::Event::PointerMoved(to)]);
    run(&mut app, vec![button(to, false)]);
    assert!((knob(&app) - 0.6).abs() < 1e-6, "dragged to {}", knob(&app));
    assert_eq!(app.sch.sch.components.iter().find(|c| c.refdes == "VR1").unwrap().value, "10k POS=30", "the schematic keeps its value");
    assert!(app.sch.breadboard.is_none(), "turning the knob does not move the part");
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: Vec2::new(0.0, 1.0), phase: egui::TouchPhase::Move, modifiers: Default::default() }]);
    assert!((knob(&app) - 0.65).abs() < 1e-6, "a notch up: {}", knob(&app));
}

/// On the stripboard, a part dragged by the pointer moves by whole holes and the layout is kept with the sheet; the check then
/// finds what the move broke; a click of the cut tool cuts a strip; Ctrl+Z and Ctrl+Y undo and redo them; "自動で並べ直す" (None)
/// gives the automatic layout back.
#[test]
fn a_part_dragged_on_the_stripboard_stays_and_is_checked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Stripboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let auto = sch_core::stripboard::strip_layout(&app.sch.sch);
    let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
    let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
    // a standing two-lead part, pressed on its middle (between its leads, away from the links' ends)
    let part = auto.parts.iter().find(|p| p.leads.len() == 2 && p.leads[0].1 .1 == p.leads[1].1 .1 && p.leads[1].1 .0.abs_diff(p.leads[0].1 .0) >= 2).expect("a standing part");
    let (a, b) = (hole(part.leads[0].1), hole(part.leads[1].1));
    let at = a + (b - a) * 0.5;
    let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![button(at, true)]);
    let to = at + Vec2::new(pitch * 2.0, 0.0);
    run(&mut app, vec![egui::Event::PointerMoved(to)]);
    run(&mut app, vec![button(to, false)]);
    let saved = app.sch.stripboard.clone().expect("the moved layout is kept");
    let moved = saved.parts.iter().find(|p| p.refdes == part.refdes).unwrap();
    assert_eq!(moved.leads[0].1, (part.leads[0].1 .0, part.leads[0].1 .1 + 2), "two holes right");
    let saved_text = crate::schematic::SheetSettings { stripboard: Some(saved.clone()), ..Default::default() };
    let mut v = serde_json::json!({});
    crate::schematic::with_settings(&mut v, &saved_text);
    assert_eq!(crate::schematic::take_settings(&mut v).stripboard, Some(saved.clone()), "saved with the sheet");
    // the cut tool: a click on a free hole of the bottom row cuts it
    app.breadboard.strip.tool = strip_view::StripTool::Cut;
    let free = (saved.rows - 1, saved.cols - 1);
    run(&mut app, vec![egui::Event::PointerMoved(hole(free))]);
    run(&mut app, vec![button(hole(free), true)]);
    run(&mut app, vec![button(hole(free), false)]);
    assert!(app.sch.stripboard.as_ref().unwrap().cuts.contains(&free));
    // Ctrl+Z takes the cut back, then the move (the automatic layout again); Ctrl+Y does the move again
    let key = |key: egui::Key| vec![egui::Event::ModifiersChanged(egui::Modifiers::COMMAND), egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }];
    run(&mut app, key(egui::Key::Z));
    assert_eq!(app.sch.stripboard.as_ref(), Some(&saved), "the cut is undone");
    run(&mut app, key(egui::Key::Z));
    assert_eq!(app.sch.stripboard, None, "the move is undone");
    run(&mut app, key(egui::Key::Z));
    assert_eq!(app.sch.stripboard, None, "nothing more to undo");
    run(&mut app, key(egui::Key::Y));
    assert_eq!(app.sch.stripboard.as_ref(), Some(&saved), "the move is done again");
    app.sch.stripboard = None;
    run(&mut app, vec![]);
    assert_eq!(app.breadboard.strip_shown(&app.sch), auto);
}

/// On the perfboard, L locks the picked part (kept with the sheet, the padlock's mark in the list); laid out again round it, the
/// locked part stays in the same holes, the lock kept, the board as the schematic joins it. "自動配置" places the parts round it
/// and leaves the wires off (the nets shown as lines to join), and "自動配線" wires them, a wire drawn by hand kept.
#[test]
fn a_part_locked_on_the_perfboard_stays_when_the_rest_is_laid_out_again() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let auto = app.breadboard.strip_shown(&app.sch);
    let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
    let u1 = auto.parts.iter().find(|p| p.refdes == "U1").unwrap().clone();
    let pts: Vec<Pos2> = u1.leads.iter().map(|(_, a)| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch).collect();
    let at = egui::Rect::from_points(&pts).center();
    let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![button(at, true)]);
    run(&mut app, vec![button(at, false)]);
    assert_eq!(app.breadboard.picked.as_deref(), Some("U1"));
    run(&mut app, vec![egui::Event::Key { key: egui::Key::L, physical_key: Some(egui::Key::L), pressed: true, repeat: false, modifiers: Default::default() }]);
    let locked = app.sch.stripboard.clone().expect("the lock is kept with the sheet");
    assert!(locked.locked.contains("U1"));
    app.breadboard.strip_relayout(&app.sch, &locked);
    run(&mut app, vec![]);
    let again = app.sch.stripboard.clone().unwrap();
    assert_eq!(again.parts.iter().find(|p| p.refdes == "U1").unwrap().leads, u1.leads, "U1 stays");
    assert!(again.locked.contains("U1"), "still locked");
    assert!(sch_core::stripboard::strip_check(&app.sch.sch, &again).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &again));
    // "自動配置": the parts placed, U1 where it is, no wires
    app.breadboard.strip_place(&app.sch, &again);
    run(&mut app, vec![]);
    let placed = app.sch.stripboard.clone().unwrap();
    assert_eq!(placed.parts.iter().find(|p| p.refdes == "U1").unwrap().leads, u1.leads, "U1 stays");
    assert!(placed.links.is_empty() && placed.bare.is_empty(), "no wires");
    assert!(!sch_core::stripboard::strip_unjoined(&app.sch.sch, &placed).is_empty(), "lines to join");
    // a wire drawn by hand first (from U1's first lead to the free hole beside it), then "自動配線"
    let a = u1.leads[0].1;
    let b = [(a.0, a.1.wrapping_sub(1)), (a.0.wrapping_sub(1), a.1), (a.0 + 1, a.1)].into_iter().find(|h| h.1 < placed.cols && h.0 < placed.rows && !placed.used().iter().any(|x| x.0 == *h)).unwrap();
    let mut hand = placed.clone();
    hand.bare.push((a, b));
    app.breadboard.strip_route(&app.sch, &hand);
    run(&mut app, vec![]);
    let wired = app.sch.stripboard.clone().unwrap();
    assert!(sch_core::stripboard::strip_unjoined(&app.sch.sch, &wired).is_empty() && sch_core::stripboard::strip_check(&app.sch.sch, &wired).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &wired));
    assert!(wired.bare.contains(&(a, b)), "the wire drawn by hand kept");
}

/// On the perfboard, a section's name dragged two rows down: the box is put there (kept with the sheet, marked as put by hand),
/// the others are packed round it and the board still joins what the schematic joins.
#[test]
fn a_section_box_dragged_by_its_name_stays_where_it_is_put() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
    app.sch.sch.requirements.groups = vec![group("タイマ", &["U1", "R1", "R2", "C1", "C2"]), group("表示", &["R3", "D1"])];
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let auto = app.breadboard.strip_shown(&app.sch);
    let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
    let (name, a, b) = auto.sections.iter().find(|x| x.0 == "表示").expect("the section's box").clone();
    let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
    let at = egui::Rect::from_two_pos(hole(a), hole(b)).expand(pitch * 0.45).left_top() + Vec2::new(pitch * 0.8, -pitch * 0.4);
    let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![button(at, true)]);
    let to = at + Vec2::new(0.0, pitch * 2.0);
    run(&mut app, vec![egui::Event::PointerMoved(to)]);
    run(&mut app, vec![button(to, false)]);
    run(&mut app, vec![]);
    let put = app.sch.stripboard.clone().expect("the layout round the box is kept with the sheet");
    assert!(put.pinned.contains(&name), "marked as put by hand");
    assert_eq!(put.sections.iter().find(|x| x.0 == name).unwrap().1, (a.0 + 2, a.1), "two rows down");
    assert!(sch_core::stripboard::strip_check(&app.sch.sch, &put).is_empty(), "{:?}", sch_core::stripboard::strip_check(&app.sch.sch, &put));
}

/// On the perfboard, the bare wire tool seen from above: a click on a hole and another on its row draws a tinned wire on the parts'
/// side (B11: the side looked at), one off its row and column draws nothing, a right click lets a wire begun go, and the eraser
/// takes the wire away. (On a board put by a fixed rule, `plain_perfboard`: its top row is free, whatever the automatic layout does.)
#[test]
fn a_bare_wire_is_drawn_and_taken_away_on_the_perfboard() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("seg7_counter.schjson"));
    app.sch.stripboard = Some(sch_core::strip_edit::plain_perfboard(&app.sch.sch));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    app.breadboard.strip.tool = strip_view::StripTool::Bare;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let (origin, pitch) = app.breadboard.strip.drawn.expect("the board was drawn");
    let hole = |a: sch_core::stripboard::At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
    let click = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), at: sch_core::stripboard::At, button: egui::PointerButton| {
        let p = hole(at);
        run(app, vec![egui::Event::PointerMoved(p)]);
        run(app, vec![egui::Event::PointerButton { pos: p, button, pressed: true, modifiers: Default::default() }]);
        run(app, vec![egui::Event::PointerButton { pos: p, button, pressed: false, modifiers: Default::default() }]);
    };
    // the board as put (no strips, no wires); its top row free
    let auto = app.breadboard.strip_shown(&app.sch);
    assert!(auto.perf && auto.links.is_empty() && auto.bare.is_empty());
    let (r, c0) = (0, 1);
    // (a hole a row down and two along: not on the start's row or column, nor at 45 degrees)
    let (a, b, off) = ((r, c0), (r, c0 + 2), (r + 1, c0 + 2));
    click(&mut app, &mut run, a, egui::PointerButton::Primary);
    click(&mut app, &mut run, off, egui::PointerButton::Primary);
    assert_eq!(app.sch.stripboard.as_ref(), Some(&auto), "not on the start's row or column nor at 45 degrees: nothing drawn");
    click(&mut app, &mut run, b, egui::PointerButton::Primary);
    let drawn = app.sch.stripboard.clone().expect("kept with the sheet");
    let last = drawn.links.last().map(|k| (k.a, k.b, k.back, k.tinned));
    assert!(drawn.perf && drawn.bare.len() == auto.bare.len() && drawn.links.len() == auto.links.len() + 1 && last == Some((a, b, false, true)), "{last:?}");
    // the right click as on the schematic and the board: a wire begun is let go, then the tool goes back to Select (the wire stays)
    click(&mut app, &mut run, a, egui::PointerButton::Primary);
    assert!(app.breadboard.strip.link_start.is_some());
    click(&mut app, &mut run, (r, c0 + 1), egui::PointerButton::Secondary);
    assert!(app.breadboard.strip.link_start.is_none() && app.breadboard.strip.tool == strip_view::StripTool::Bare, "the wire begun is let go");
    click(&mut app, &mut run, (r, c0 + 1), egui::PointerButton::Secondary);
    assert_eq!(app.breadboard.strip.tool, strip_view::StripTool::Move, "then back to Select");
    assert_eq!(app.sch.stripboard.as_ref().map(|l| l.links.len()), Some(auto.links.len() + 1), "a right click takes no wire away");
    // the eraser takes it away: a click on it
    app.breadboard.strip.tool = strip_view::StripTool::Erase;
    click(&mut app, &mut run, (r, c0 + 1), egui::PointerButton::Primary);
    assert_eq!(app.sch.stripboard.as_ref().map(|l| l.links.len()), Some(auto.links.len()), "the eraser's click takes it away");
    // seen from below, "線を引く" draws an insulated wire on the copper side (the holes mirrored left and right), and the eraser
    // from below takes it away
    app.breadboard.strip.back = true;
    app.breadboard.strip.tool = strip_view::StripTool::Link;
    run(&mut app, vec![]);
    let cols = app.breadboard.strip_shown(&app.sch).cols;
    let (o2, p2) = app.breadboard.strip.drawn.expect("drawn from below");
    let tap = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), at: sch_core::stripboard::At| {
        let p = o2 + Vec2::new((cols - 1 - at.1) as f32, at.0 as f32) * p2;
        run(app, vec![egui::Event::PointerMoved(p)]);
        run(app, vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        run(app, vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    };
    tap(&mut app, &mut run, a);
    tap(&mut app, &mut run, b);
    let l = app.sch.stripboard.clone().unwrap();
    assert_eq!(l.links.last().map(|k| (k.a, k.b, k.back, k.tinned)), Some((a, b, true, false)));
    app.breadboard.strip.tool = strip_view::StripTool::Erase;
    tap(&mut app, &mut run, (r, c0 + 1));
    assert_eq!(app.sch.stripboard.as_ref().map(|l| l.links.len()), Some(auto.links.len()), "the eraser from below takes it away");
    app.breadboard.strip.back = false;
    // the stripboard does not show a layout kept for the perfboard
    app.breadboard.kind = BoardKind::Stripboard;
    assert!(!app.breadboard.strip_shown(&app.sch).perf);
}

/// With the move tool on the perfboard, a right click lets go of the parts picked (as Esc).
#[test]
fn a_right_click_lets_the_pick_go() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("led.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let (origin, _) = app.breadboard.strip.drawn.expect("the board was drawn");
    app.breadboard.strip_pick_box("A1", "Z60");
    run(&mut app, vec![]);
    assert!(app.breadboard.strip.chosen.len() >= 2, "the parts in the box are picked: {:?}", app.breadboard.strip.chosen);
    let p = origin;
    run(&mut app, vec![egui::Event::PointerMoved(p)]);
    for pressed in [true, false] {
        run(&mut app, vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() }]);
    }
    assert!(app.breadboard.strip.chosen.is_empty() && app.breadboard.picked.is_none(), "a right click lets the pick go");
}

/// The schematic's small window on the proto tab: a click on a part in it picks that part on the board (and the board's pick is
/// what the window rings); a part moved off its column on the breadboard leaves lines still to join.
#[test]
fn the_schematic_window_picks_parts_on_the_board() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.show_schematic = true;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    run(&mut app, vec![]);
    // the window's place on the screen
    let area = ctx.memory(|m| m.area_rect(egui::Id::new("proto_schematic"))).expect("the window is shown");
    assert!(app.breadboard.show_schematic);
    // click on each part's place until one is picked: the window maps the click back to a part of the schematic
    let mut picked = None;
    'scan: for fy in [0.3f32, 0.4, 0.5, 0.6, 0.7] {
        for fx in [0.2f32, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
            let p = area.min + egui::vec2(area.width() * fx, area.height() * fy);
            let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            run(&mut app, vec![egui::Event::PointerMoved(p)]);
            run(&mut app, vec![button(true)]);
            run(&mut app, vec![button(false)]);
            if let Some(r) = app.breadboard.picked.clone() {
                picked = Some(r);
                break 'scan;
            }
        }
    }
    let r = picked.expect("a click on a part of the schematic picks it");
    assert!(app.sch.sch.components.iter().any(|c| c.refdes == r));
    assert_eq!(app.sch.linked.part.as_deref(), Some(r.as_str()), "the schematic rings the part picked");
}

#[test]
fn turning_the_pot_dims_the_led() {
    let (mut app, mut run) = power_on_app("pot_dimmer.schjson");
    app.breadboard.powered = true;
    run(&mut app);
    let mut led_at = |app: &mut App, pos: f64| {
        let lv = app.breadboard.live.as_mut().unwrap();
        let k = lv.knob_index("VR1").expect("the pot is a knob");
        assert_eq!(lv.knobs[k].range(), 0.0..=1.0);
        lv.set_knob(k, pos);
        lv.advance(0.02);
        run(app);
        app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0)
    };
    let full = led_at(&mut app, 0.0);
    assert!((0.012..0.02).contains(&full), "about 16 mA with the wiper on 5 V: {full}");
    let some = led_at(&mut app, 0.3);
    assert!((0.003..0.01).contains(&some), "dimmer at 30 %: {some}");
    assert!(led_at(&mut app, 1.0).abs() < 1e-4, "dark with the wiper on GND");
    assert!(burnt(&app).is_empty(), "{:?}", app.breadboard.issues());
}

/// The LED on the wiper itself (its resistor moved from the emitter to the wiper): turned near pin 1, its current runs through
/// a short piece of the track, which burns open, and the LED goes dark.
#[test]
fn an_led_on_the_wiper_burns_the_pot() {
    let (mut app, mut run) = power_on_app("pot_dimmer.schjson");
    // the label on R1's pin 1 sits at the end of the short wire from the pin
    let r1 = app.sch.sch.components.iter().find(|c| c.refdes == "R1").unwrap().pins().into_iter().find(|(n, _)| n == "1").unwrap().1;
    let end = app.sch.sch.wires.iter().find_map(|w| (w.a == r1).then_some(w.b)).unwrap();
    app.sch.sch.labels.iter_mut().find(|l| l.pos == end).unwrap().text = "W".into();
    app.breadboard.powered = true;
    run(&mut app);
    {
        let lv = app.breadboard.live.as_mut().unwrap();
        let k = lv.knob_index("VR1").unwrap();
        lv.set_knob(k, 0.05);
    }
    for _ in 0..30 {
        app.breadboard.live.as_mut().unwrap().advance(0.015);
        run(&mut app);
    }
    assert!(burnt(&app).contains(&"VR1".to_string()), "{:?}", app.breadboard.issues());
    assert!(app.breadboard.broken.iter().any(|(r, f)| r == "VR1.A" && *f == sim_core::Fault::Open), "the piece from pin 1 to the wiper: {:?}", app.breadboard.broken);
    assert!(app.breadboard.live.as_ref().and_then(|v| v.current("D1")).unwrap_or(0.0).abs() < 1e-6, "no current past the burnt track");
}

#[test]
fn resistor_bands_read_the_value() {
    let b = bands("4.7k").unwrap();
    // yellow, violet, red (x100), gold
    assert_eq!(b[0], Color32::from_rgb(240, 220, 40));
    assert_eq!(b[1], Color32::from_rgb(140, 60, 180));
    assert_eq!(b[2], Color32::from_rgb(210, 40, 40));
    assert_eq!(bands("10").unwrap()[2], Color32::from_rgb(20, 20, 20), "10 = brown black black");
    assert!(bands("x").is_none());
}

#[test]
fn the_tab_draws_a_sample_and_a_drag_is_kept_with_the_sheet() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    assert!(app.sch.breadboard.is_none(), "looking does not change the sheet");
    assert!(!app.sch.dirty());
    // the layout kept with the sheet once it is asked for, and read back from the file
    app.sch.breadboard = Some(auto_layout(&app.sch.sch));
    assert!(app.sch.dirty());
    let mut v = serde_json::json!({});
    crate::schematic::with_settings(&mut v, &app.sch.sheet_settings());
    let back = crate::schematic::take_settings(&mut v);
    assert_eq!(back.breadboard, app.sch.breadboard);
    run(&mut app, vec![]);
    // every tool draws
    for t in [Tool::Wire, Tool::Meter, Tool::Scope, Tool::Move] {
        app.breadboard.tool = t;
        run(&mut app, vec![]);
    }
    // an empty schematic says what to do
    let mut empty = App::default();
    empty.mode = Mode::Proto;
    run(&mut empty, vec![]);
}

/// The perfboard picture zooms with the wheel about the pointer (the hole under it stays there), and Home gives the whole board back.
#[test]
fn the_board_picture_zooms_about_the_pointer_and_home_fits_it_again() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let (o0, p0) = app.breadboard.strip.drawn.expect("the board was drawn");
    let hole = Vec2::new(4.0, 3.0);
    let at = o0 + hole * p0;
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    for _ in 0..4 {
        run(&mut app, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: Vec2::new(0.0, 1.0), phase: egui::TouchPhase::Move, modifiers: Default::default() }]);
    }
    for _ in 0..30 {
        run(&mut app, vec![]);
    }
    let (o1, p1) = app.breadboard.strip.drawn.unwrap();
    assert!(p1 > p0 * 1.2, "zoomed in: {p0} -> {p1}");
    assert!((o1 + hole * p1).distance(at) < 1.0, "the hole stays under the pointer");
    run(&mut app, vec![egui::Event::Key { key: egui::Key::Home, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    run(&mut app, vec![]);
    assert_eq!(app.breadboard.strip.drawn.unwrap().1, p0, "Home: the whole board again");
}

/// With two sheets the schematic's small window has a tab for each, turns to the sheet of a part picked on the board, and leaves the
/// sheet being edited (and the schematic) as they were.
#[test]
fn the_schematic_window_shows_the_sheet_of_the_part_picked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("two_sheets.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.show_schematic = true;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    run(&mut app, vec![]);
    let active = app.sch.sch.active_sheet;
    let before = app.sch.sch.clone();
    assert!(app.sch.sch.components.iter().all(|c| c.refdes != "R1"), "R1 is on the other sheet");
    app.breadboard.picked = Some("R1".into());
    run(&mut app, vec![]);
    run(&mut app, vec![]);
    assert_eq!(app.sch.preview_shown_sheet(), Some(1), "the window turned to R1's sheet");
    assert_eq!(app.sch.sch.active_sheet, active, "the sheet being edited is the same");
    assert_eq!(app.sch.sch, before, "nothing changed in the schematic");
}

/// A drag with the middle button moves the picture: it does not draw a selection box or move a part (the left button's work).
#[test]
fn a_middle_drag_moves_the_picture_and_selects_nothing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let (o0, p0) = app.breadboard.strip.drawn.expect("the board was drawn");
    let before = app.sch.stripboard.clone();
    let button = |pos: Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: Default::default() };
    // from a hole of a part across the board
    let from = o0 + Vec2::new(3.0, 3.0) * p0;
    run(&mut app, vec![egui::Event::PointerMoved(from)]);
    run(&mut app, vec![button(from, true)]);
    for k in 1..=6 {
        run(&mut app, vec![egui::Event::PointerMoved(from + Vec2::new(20.0 * k as f32, 10.0 * k as f32))]);
    }
    let to = from + Vec2::new(120.0, 60.0);
    run(&mut app, vec![button(to, false)]);
    run(&mut app, vec![]);
    assert!(app.breadboard.strip.chosen.is_empty() && app.breadboard.picked.is_none(), "nothing picked");
    assert_eq!(app.sch.stripboard, before, "no part moved");
    let (o1, _) = app.breadboard.strip.drawn.unwrap();
    assert!((o1 - o0 - Vec2::new(120.0, 60.0)).length() < 30.0, "the picture moved with the pointer: {o0:?} -> {o1:?}");
}

/// As on the schematic: a left drag from an empty hole round the board picks the parts it encloses (the box goes when the button is
/// let go and the parts are marked); a part of the pick dragged moves them all; Shift + click takes one out of the pick; a right drag
/// picks what its box touches.
#[test]
fn parts_are_picked_with_a_box_and_moved_together() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    app.breadboard.kind = BoardKind::Perfboard;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    // Shift is read from the frame's state (held for the frames `held` says)
    let held = std::cell::Cell::new(egui::Modifiers::default());
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let events = std::iter::once(egui::Event::ModifiersChanged(held.get())).chain(events).collect();
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let l = app.breadboard.strip_shown(&app.sch);
    let (o, p) = app.breadboard.strip.drawn.expect("the board was drawn");
    let hole = |a: (u16, u16)| o + Vec2::new(a.1 as f32, a.0 as f32) * p;
    let drag = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), from: Pos2, to: Pos2, button: egui::PointerButton, modifiers: egui::Modifiers| {
        run(app, vec![egui::Event::PointerMoved(from)]);
        run(app, vec![egui::Event::PointerButton { pos: from, button, pressed: true, modifiers }]);
        for k in 1..=8 {
            run(app, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0))]);
        }
        run(app, vec![egui::Event::PointerButton { pos: to, button, pressed: false, modifiers }]);
        run(app, vec![]);
    };
    // round the whole board, from outside its top-left corner (three holes left of and one above the first: the box starts at the edge)
    let (from, to) = (hole((0, 0)) - Vec2::new(p * 3.0, p * 1.2), hole((l.rows - 1, l.cols - 1)));
    drag(&mut app, &mut run, from, to, egui::PointerButton::Primary, Default::default());
    let mut all: Vec<String> = l.parts.iter().map(|x| x.refdes.clone()).collect();
    let mut chosen = app.breadboard.strip.chosen.clone();
    all.sort();
    chosen.sort();
    assert_eq!(chosen, all, "every part is picked");
    assert!(app.breadboard.strip.drag.is_none(), "the box is gone once let go");
    // a part of the pick dragged one hole right: all of them go
    let r1 = l.parts.iter().find(|x| x.refdes == "R1").unwrap();
    let at = hole(r1.leads[0].1);
    drag(&mut app, &mut run, at, at + Vec2::new(p, 0.0), egui::PointerButton::Primary, Default::default());
    let m = app.sch.stripboard.clone().expect("the move is kept with the sheet");
    for x in &l.parts {
        let y = m.parts.iter().find(|y| y.refdes == x.refdes).unwrap();
        assert!(x.leads.iter().zip(&y.leads).all(|(a, b)| b.1 == (a.1 .0, a.1 .1 + 1)), "{} moved with the rest: {:?} -> {:?}", x.refdes, x.leads, y.leads);
    }
    // (the board keeps one free column past its parts: one wider now, drawn a little elsewhere)
    run(&mut app, vec![]);
    let (o, p) = app.breadboard.strip.drawn.expect("the board was drawn");
    let hole = |a: (u16, u16)| o + Vec2::new(a.1 as f32, a.0 as f32) * p;
    // Shift + click takes R1 out
    let r1 = m.parts.iter().find(|x| x.refdes == "R1").unwrap();
    let at = hole(r1.leads[0].1);
    let shift = egui::Modifiers { shift: true, ..Default::default() };
    held.set(shift);
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    for pressed in [true, false] {
        run(&mut app, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: shift }]);
    }
    held.set(Default::default());
    assert!(!app.breadboard.strip.chosen.contains(&"R1".to_string()) && app.breadboard.strip.chosen.len() == all.len() - 1, "{:?}", app.breadboard.strip.chosen);
    // a right drag over one hole of the chip's pins picks the chip, which it only touches (and what crosses that hole)
    let u1 = m.parts.iter().find(|x| x.refdes == "U1").unwrap();
    let pin = hole(u1.leads[0].1);
    drag(&mut app, &mut run, pin - Vec2::splat(p * 0.2), pin + Vec2::splat(p * 0.2), egui::PointerButton::Secondary, Default::default());
    let now = app.breadboard.strip_picked_parts();
    assert!(now.contains(&"U1".to_string()), "the chip touched is picked: {now:?}");
    assert!(now.len() < all.len(), "only what the small box touches: {now:?}");
    // the whole board boxed again and Delete: every wire on top and bare wire on the copper side goes (the parts stay)
    let m = app.sch.stripboard.clone().unwrap();
    assert!(m.links.len() + m.bare.len() > 0, "the blinker has wires to take away");
    let from = hole((0, 0)) - Vec2::new(p * 3.0, p * 1.2);
    drag(&mut app, &mut run, from, hole((m.rows - 1, m.cols - 1)) + Vec2::splat(p * 2.0), egui::PointerButton::Primary, Default::default());
    let at = hole((m.rows / 2, m.cols / 2));
    run(&mut app, vec![egui::Event::PointerMoved(at)]);
    run(&mut app, vec![egui::Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    let gone = app.sch.stripboard.clone().unwrap();
    assert!(gone.links.is_empty() && gone.bare.is_empty() && gone.parts.len() == m.parts.len(), "{} links, {} bare wires left", gone.links.len(), gone.bare.len());
}

/// On the breadboard too: a left drag from an empty spot round the parts picks them all, and one of them dragged a column takes the
/// rest along.
#[test]
fn breadboard_parts_are_picked_with_a_box_and_moved_together() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut app = App::default();
    app.sch.load_path(root.join("ne555_blinker.schjson"));
    app.mode = Mode::Proto;
    let ctx = egui::Context::default();
    let mut f = eframe::Frame::_new_kittest();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
    let mut run = |app: &mut App, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| eframe::App::ui(app, ui, &mut f));
        out.textures_delta.clear();
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let l = app.breadboard.layout(&app.sch);
    let (o, p) = app.breadboard.bread_drawn.expect("the breadboard was drawn");
    let hole = |h: Hole| o + at(h) * p;
    let drag = |app: &mut App, run: &mut dyn FnMut(&mut App, Vec<egui::Event>), from: Pos2, to: Pos2| {
        run(app, vec![egui::Event::PointerMoved(from)]);
        run(app, vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        for k in 1..=8 {
            run(app, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0))]);
        }
        run(app, vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        run(app, vec![]);
    };
    // from beyond the last column at the bottom right, to above the first column at the top left
    let last = l.cols - 1;
    let (from, to) = (hole(Hole::Rail { rail: Rail::BottomPlus, col: last }) + Vec2::splat(p), hole(Hole::Rail { rail: Rail::TopPlus, col: 0 }) - Vec2::splat(p));
    drag(&mut app, &mut run, from, to);
    let mut all: Vec<String> = l.parts.iter().map(|x| x.refdes.clone()).collect();
    let mut chosen = app.breadboard.chosen.clone();
    all.sort();
    chosen.sort();
    assert_eq!(chosen, all, "every part is picked");
    // R1 dragged one column right: all go
    let r1 = l.parts.iter().find(|x| x.refdes == "R1").unwrap().leads[0].1;
    let start = hole(r1);
    drag(&mut app, &mut run, start, start + Vec2::new(p, 0.0));
    let m = app.sch.breadboard.clone().expect("the move is kept with the sheet");
    for x in &l.parts {
        let y = m.parts.iter().find(|y| y.refdes == x.refdes).unwrap();
        assert!(x.leads.iter().zip(&y.leads).all(|(a, b)| b.1.col() == a.1.col() + 1), "{} moved with the rest", x.refdes);
    }
}
