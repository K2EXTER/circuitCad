//! "動かしながら見る" on the breadboard (candidate B6 ②): with the power on, the transient run of `sim-core` is kept open
//! (`sim_core::TranRun`) and solved on to the time of the display each frame, as the live logic view of the schematic does. The
//! LEDs glow by their current now, the switches and push buttons are clicked (their model's position source changed between
//! frames, `TranRun::set_source`), the meter reads the voltage now and the scope draws the last seconds of its net. A circuit
//! faster than the screen is slowed down by the speed setting; a frame that takes too long to solve leaves the run behind the
//! clock rather than freezing the window.

use sim_core::{TranRun, TranSettings, Waveform};
use std::collections::{BTreeMap, VecDeque};

/// The first end of the run (simulated seconds); it is pushed on when reached.
const SIM_STOP: f64 = 10.0;
/// The speeds offered (simulated seconds per second of the screen) and their names.
pub const SPEEDS: [(f64, &str); 4] = [(1.0, "実時間"), (0.1, "1/10"), (0.01, "1/100"), (0.001, "1/1000")];
/// Seconds of the screen the scope keeps.
const SCOPE_SPAN: f64 = 3.0;
/// The most time one frame may spend solving (seconds of the wall clock); past it the run falls behind.
const FRAME_BUDGET: f64 = 0.04;

/// A switch on the breadboard: its part, the source in its model that holds the position, push button or slide switch, and where it is.
#[derive(Clone, Debug)]
pub struct Switch {
    pub refdes: String,
    source: String,
    pub push: bool,
    pub on: bool,
}

/// What is turned while the circuit runs: a supply or a source off the board (a bench supply's knob, the light on a light sensor's
/// input; volts), or a potentiometer on the board (how far its knob is turned, 0..1). Its part, the value it was drawn with, and
/// the value now.
#[derive(Clone, Debug)]
pub struct Knob {
    pub refdes: String,
    pub nominal: f64,
    pub value: f64,
    /// A potentiometer: its whole resistance (ohms); its two halves `<refdes>.A` and `.B` are set from the position.
    pub pot: Option<f64>,
}

impl Knob {
    /// The range of the slider: a potentiometer's whole turn, or from 0 to twice the drawn voltage (at least 1 V either way).
    pub fn range(&self) -> std::ops::RangeInclusive<f64> {
        if self.pot.is_some() {
            return 0.0..=1.0;
        }
        let far = (2.0 * self.nominal.abs()).max(1.0);
        if self.nominal < 0.0 { -far..=0.0 } else { 0.0..=far }
    }
}

pub struct Live {
    run: Option<TranRun>,
    /// The circuit it runs.
    circuit: sim_core::Circuit,
    pub error: Option<String>,
    /// Simulated seconds per second of the screen.
    pub speed: f64,
    last_frame: Option<f64>,
    /// The run is behind the clock (the last frame ran out of its budget).
    pub behind: bool,
    volts: BTreeMap<String, f64>,
    /// The current of each kept element now (by element name, `D1`).
    currents: BTreeMap<String, f64>,
    pub switches: Vec<Switch>,
    pub knobs: Vec<Knob>,
    /// The push button held down by the mouse.
    held: Option<usize>,
    /// The scope: its net and the points of the last `SCOPE_SPAN` seconds of the screen (simulated time, volts).
    probe: Option<String>,
    trace: VecDeque<(f64, f64)>,
    /// The microcontroller boards running their sketches (B6 ⑤).
    pub mcus: Vec<Mcu>,
}

/// A microcontroller board on the breadboard and its sketch: the machine running it (None when the sketch does not compile: the
/// sketch box says why), and the net of each of its pins D0..D13, A0..A7 on the board. A board whose sketch is built by
/// arduino-cli runs the chip's real instructions instead (`real`, W5 (b)): `waiting` until the build is done.
pub struct Mcu {
    pub refdes: String,
    /// Which board (Nano, Mega): its pins' numbers and names, the chip the real instructions run on.
    pub board: sim_core::sketch::Board,
    pub machine: Option<sim_core::sketch::Machine>,
    nets: Vec<Option<String>>,
    pub real: Option<avr_sim::Board>,
    /// The real chip is waiting for its program (being built); the pins are inputs meanwhile.
    pub waiting: bool,
    /// The run's time when the real chip started (its own time counts from 0).
    real_from: f64,
    /// What the real chip's pins do now (set on the circuit when they change).
    drives: Vec<sim_core::sketch::Drive>,
}

/// The chip and pins avr-sim runs for a board.
fn avr_board(b: sim_core::sketch::Board) -> &'static avr_sim::BoardDef {
    match b {
        sim_core::sketch::Board::Nano => &avr_sim::NANO,
        sim_core::sketch::Board::Mega => &avr_sim::MEGA,
    }
}

/// The circuit as a running sketch sees it: a pin's voltage is its net's now, a pin's drive sets the source and the resistance
/// behind it inside the board's model (`ARDUINO_NANO`, `ARDUINO_MEGA`: `VpD13` and `RpD13`).
struct RunIo<'a> {
    run: &'a mut TranRun,
    refdes: &'a str,
    board: sim_core::sketch::Board,
    nets: &'a [Option<String>],
}

/// The resistance behind a pin: an output, the pull-up, an input.
const PIN_OUT_OHM: f64 = 25.0;
const PIN_PULLUP_OHM: f64 = 35e3;
const PIN_INPUT_OHM: f64 = 1e9;

/// Pulses between 0 and the supply from `now`: High for `duty` of each period of `hz` (edges a thousandth of the period).
fn pulses(now: f64, duty: f64, hz: f64) -> Waveform {
    use sim_core::sketch::VCC;
    let per = 1.0 / hz.max(1.0);
    let edge = per * 1e-3;
    let pw = (duty.clamp(0.0, 1.0) * per - edge).max(0.0);
    Waveform::Pulse { v1: 0.0, v2: VCC, td: now, tr: edge, tf: edge, pw, per }
}

impl sim_core::sketch::Io for RunIo<'_> {
    fn volts(&self, pin: u8) -> f64 {
        match self.nets.get(pin as usize).and_then(|n| n.as_deref()) {
            Some(n) if is_ground(n) => 0.0,
            Some(n) => self.run.voltage(n).unwrap_or(0.0),
            None => 0.0,
        }
    }
    fn drive(&mut self, pin: u8, d: sim_core::sketch::Drive) {
        use sim_core::sketch::{Drive, VCC};
        let name = self.board.pin_name(pin);
        let (wave, r) = match d {
            Drive::Input => (Waveform::Dc(0.0), PIN_INPUT_OHM),
            Drive::Pullup => (Waveform::Dc(VCC), PIN_PULLUP_OHM),
            Drive::Out(x) => (Waveform::Dc(x.clamp(0.0, 1.0) * VCC), PIN_OUT_OHM),
            // the pulses themselves from now on (an LED lights for that share of the time; an RC smooths them to the average)
            Drive::Pwm { duty, hz } => (pulses(self.run.time(), duty, hz), PIN_OUT_OHM),
        };
        self.run.set_source(&format!("{}.Vp{name}", self.refdes), wave);
        self.run.set_resistance(&format!("{}.Rp{name}", self.refdes), r);
    }
}

impl Live {
    /// Starts the run on `circuit`. `currents`: the elements whose current is read (the LEDs); `switches`: from
    /// `SchEditor::live_switches`; `knobs`: the parts whose DC voltage may be turned (the ones that are DC sources in the circuit)
    /// and the potentiometers (the ones whose two halves `.A` and `.B` are in the circuit).
    pub fn start(circuit: sim_core::Circuit, currents: &[String], switches: Vec<(String, String, bool)>, knobs: &[String]) -> Live {
        let resistance = |name: String| circuit.elements.iter().find_map(|e| match e {
            sim_core::Element::Resistor { name: n, r, .. } if *n == name => Some(*r),
            _ => None,
        });
        let mut knobs_found: Vec<Knob> = circuit
            .elements
            .iter()
            .filter_map(|e| match e {
                sim_core::Element::VSource { name, wave: Waveform::Dc(v), .. } if knobs.contains(name) => Some(Knob { refdes: name.clone(), nominal: *v, value: *v, pot: None }),
                _ => None,
            })
            .collect();
        for k in knobs {
            if let (Some(a), Some(b)) = (resistance(format!("{k}.A")), resistance(format!("{k}.B"))) {
                let pos = a / (a + b);
                knobs_found.push(Knob { refdes: k.clone(), nominal: pos, value: pos, pot: Some(a + b) });
            }
        }
        let knobs = knobs_found;
        let mut me = Live {
            run: None,
            circuit: circuit.clone(),
            error: None,
            speed: 1.0,
            last_frame: None,
            behind: false,
            volts: BTreeMap::new(),
            currents: BTreeMap::new(),
            switches: switches.into_iter().map(|(refdes, source, push)| Switch { refdes, source, push, on: false }).collect(),
            knobs,
            held: None,
            probe: None,
            trace: VecDeque::new(),
            mcus: vec![],
        };
        let wanted: Vec<String> = currents.iter().map(|n| format!("I({n})")).collect();
        let keep = |n: &str| n.starts_with("V(") || wanted.iter().any(|w| w.eq_ignore_ascii_case(n));
        // the largest step: a millisecond keeps a blinker's RC smooth; the error control takes smaller steps where it must
        match TranRun::start(&circuit, &TranSettings { tstop: SIM_STOP, tstep: 1e-3, uic: false }, Some(&keep)) {
            Ok(run) => me.run = Some(run),
            Err(e) => me.error = Some(format!("動かせませんでした: {e}")),
        }
        me.read_now();
        me
    }

    /// A microcontroller board with its sketch (or why it does not compile) and its pins' nets (D0..D13, A0..A7); its `setup` runs
    /// at once.
    pub fn add_mcu(&mut self, refdes: String, board: sim_core::sketch::Board, sketch: Result<sim_core::sketch::Sketch, sim_core::sketch::SketchError>, nets: Vec<Option<String>>) {
        let machine = sketch.ok().map(sim_core::sketch::Machine::new);
        let drives = vec![sim_core::sketch::Drive::Input; board.pins()];
        self.mcus.push(Mcu { refdes, board, machine, nets, real: None, waiting: false, real_from: 0.0, drives });
        self.run_mcus();
        self.read_now();
    }

    /// A board that runs its program as the chip's real instructions (W5 (b)); it waits (pins as inputs) until `give_program`.
    pub fn add_real_mcu(&mut self, refdes: String, board: sim_core::sketch::Board, nets: Vec<Option<String>>) {
        let drives = vec![sim_core::sketch::Drive::Input; board.pins()];
        self.mcus.push(Mcu { refdes, board, machine: None, nets, real: None, waiting: true, real_from: 0.0, drives });
    }

    /// The program of a waiting board has been built: the chip starts now (from its reset). An image the chip cannot take leaves it
    /// waiting no more, with the reason.
    pub fn give_program(&mut self, refdes: &str, image: &avr_sim::Image) -> Result<(), String> {
        let now = self.time();
        let Some(m) = self.mcus.iter_mut().find(|m| m.refdes == refdes && m.waiting) else { return Ok(()) };
        m.waiting = false;
        m.real = Some(avr_sim::Board::new(avr_board(m.board), image)?);
        m.real_from = now;
        self.run_mcus();
        self.read_now();
        Ok(())
    }

    /// The sketches whose time has come run on (at the run's time now).
    fn run_mcus(&mut self) {
        let Some(run) = self.run.as_mut() else { return };
        let now = run.time();
        for m in &mut self.mcus {
            if let Some(machine) = m.machine.as_mut() {
                machine.run(now, &mut RunIo { run, refdes: &m.refdes, board: m.board, nets: &m.nets });
            }
            if let Some(nano) = m.real.as_mut() {
                use sim_core::sketch::{Drive, Io};
                let mut io = RunIo { run, refdes: &m.refdes, board: m.board, nets: &m.nets };
                // the chip runs to now with the circuit's voltages as they are; what its pins did on the way drives the next step
                let volts: Vec<f64> = (0..m.board.pins() as u8).map(|p| io.volts(p)).collect();
                let outs = nano.advance(now - m.real_from, |p| volts[p as usize]);
                for (pin, o) in outs.iter().enumerate() {
                    let d = match *o {
                        avr_sim::PinOut::Input => Drive::Input,
                        avr_sim::PinOut::Pullup => Drive::Pullup,
                        avr_sim::PinOut::Out(x) => Drive::Out(x),
                        avr_sim::PinOut::Pwm { duty, hz } => Drive::Pwm { duty, hz },
                    };
                    if m.drives[pin] != d {
                        m.drives[pin] = d;
                        io.drive(pin as u8, d);
                    }
                }
            }
        }
    }

    /// When the next sketch wakes from its `delay` (None with no sketch running).
    fn next_wake(&self) -> Option<f64> {
        // a real chip runs all the time: it wants the circuit's turn every step (now: the step of a millisecond)
        let real = self.mcus.iter().any(|m| m.real.as_ref().is_some_and(|n| n.avr.stopped().is_none())).then(|| self.time());
        self.mcus.iter().filter_map(|m| m.machine.as_ref().filter(|x| x.error.is_none()).map(|x| x.waiting_until())).chain(real).reduce(f64::min)
    }

    pub fn time(&self) -> f64 {
        self.run.as_ref().map_or(0.0, |r| r.time())
    }

    pub fn voltage(&self, net: &str) -> Option<f64> {
        self.volts.iter().find(|(n, _)| n.eq_ignore_ascii_case(net)).map(|(_, v)| *v)
    }

    /// The element `name` of the running circuit (how a burnt part fails depends on what it is).
    pub fn element(&self, name: &str) -> Option<&sim_core::Element> {
        self.circuit.elements.iter().find(|e| e.name() == name)
    }

    /// The operating point now (for the rating check while it runs).
    pub fn snapshot(&self) -> Option<sim_core::DcResult> {
        self.run.as_ref().map(|r| r.snapshot())
    }

    /// What each part puts into each node now (`sim_core::part_currents`), for the moving dots.
    pub fn part_currents(&self) -> Option<std::collections::BTreeMap<String, Vec<f64>>> {
        Some(sim_core::part_currents(&self.circuit, &self.snapshot()?))
    }

    /// The node of a net (or of a pin's wire, `R1.1.wire`) in the running circuit.
    pub fn node_of(&self, name: &str) -> Option<usize> {
        self.circuit.find_node(name)
    }

    pub fn current(&self, element: &str) -> Option<f64> {
        self.currents.get(element).copied()
    }

    /// The scope's net (its trace starts again when it changes).
    pub fn set_probe(&mut self, net: Option<String>) {
        if self.probe != net {
            self.probe = net;
            self.trace.clear();
        }
    }

    pub fn trace(&self) -> (Vec<f64>, Vec<f64>) {
        self.trace.iter().copied().unzip()
    }

    fn set_switch(&mut self, i: usize, on: bool) {
        let Some(s) = self.switches.get_mut(i) else { return };
        if s.on == on {
            return;
        }
        s.on = on;
        let src = s.source.clone();
        if let Some(r) = self.run.as_mut() {
            r.set_source(&src, Waveform::Dc(if on { 1.0 } else { 0.0 }));
        }
    }

    /// The mouse pressed on switch `i`: a push button closes while held, a slide switch moves its lever.
    pub fn press(&mut self, i: usize) {
        match self.switches.get(i).map(|s| s.push) {
            Some(true) => {
                self.held = Some(i);
                self.set_switch(i, true);
            }
            Some(false) => {
                let on = !self.switches[i].on;
                self.set_switch(i, on);
            }
            None => {}
        }
    }

    /// The mouse let go: a held push button opens.
    pub fn release(&mut self) {
        if let Some(i) = self.held.take() {
            self.set_switch(i, false);
        }
    }

    /// Turns knob `i` to `v` from now on: volts for a source, the position (0..1) for a potentiometer.
    pub fn set_knob(&mut self, i: usize, v: f64) {
        let Some(k) = self.knobs.get_mut(i) else { return };
        if k.value == v {
            return;
        }
        k.value = v;
        let (name, pot) = (k.refdes.clone(), k.pot);
        if let Some(r) = self.run.as_mut() {
            match pot {
                Some(total) => {
                    let (a, b) = sch_core::pot_split(total, v);
                    r.set_resistance(&format!("{name}.A"), a);
                    r.set_resistance(&format!("{name}.B"), b);
                }
                None => {
                    r.set_source(&name, Waveform::Dc(v));
                }
            }
        }
    }

    pub fn knob_index(&self, refdes: &str) -> Option<usize> {
        self.knobs.iter().position(|k| k.refdes == refdes)
    }

    pub fn switch_index(&self, refdes: &str) -> Option<usize> {
        self.switches.iter().position(|s| s.refdes == refdes)
    }

    /// One frame: the run follows the clock of the screen (`now`, seconds) at the chosen speed.
    pub fn frame(&mut self, now: f64) {
        let dt = self.last_frame.map_or(0.0, |t| (now - t).clamp(0.0, 0.25));
        self.last_frame = Some(now);
        self.advance(dt * self.speed);
    }

    /// Solves `span` simulated seconds on (in pieces, stopping when the frame's budget is spent).
    pub fn advance(&mut self, span: f64) {
        let Some(run) = self.run.as_mut() else { return };
        if span <= 0.0 {
            return;
        }
        let started = std::time::Instant::now();
        let target = run.time() + span;
        if target > run.stop_time() {
            run.extend(target * 2.0);
        }
        self.behind = false;
        // with sketches: solved from one wake to the next, the sketches run at each (at most a millisecond apart, so a sketch
        // without delay still sees the circuit move)
        if self.next_wake().is_some() {
            while self.run.as_ref().is_some_and(|r| r.time() < target - 1e-12) {
                let now = self.run.as_ref().unwrap().time();
                let wake = self.next_wake().unwrap_or(target);
                let next = if wake > now + 1e-9 { wake.min(target) } else { (now + 1e-3).min(target) };
                if let Err(e) = self.run.as_mut().unwrap().advance_to(next) {
                    self.error = Some(format!("計算が止まりました: {e}"));
                    self.run = None;
                    return;
                }
                self.run_mcus();
                if started.elapsed().as_secs_f64() > FRAME_BUDGET {
                    self.behind = true;
                    break;
                }
            }
            self.read_now();
            return;
        }
        let Some(run) = self.run.as_mut() else { return };
        let pieces = 4;
        for k in 1..=pieces {
            let t = run.time() + (target - run.time()) / (pieces - k + 1) as f64;
            if let Err(e) = run.advance_to(t) {
                self.error = Some(format!("計算が止まりました: {e}"));
                self.run = None;
                return;
            }
            if k < pieces && started.elapsed().as_secs_f64() > FRAME_BUDGET {
                self.behind = true;
                break;
            }
        }
        self.read_now();
    }

    /// Reads the voltages and currents now, and adds the points solved on the way to the scope's trace.
    fn read_now(&mut self) {
        let Some(run) = self.run.as_mut() else { return };
        let got = run.drain();
        // what the frame shows is the average over the points solved since the last one (as an eye sees an LED driven by PWM,
        // and as a meter reads); the voltages that were not kept are the ones now
        let mean = |values: &[f64]| average(&got.time, values);
        self.currents.clear();
        for s in &got.signals {
            if let (Some(name), Some(v)) = (s.name.strip_prefix("I(").and_then(|n| n.strip_suffix(')')), mean(&s.values)) {
                self.currents.insert(name.to_string(), v);
            }
        }
        self.volts = run.voltages().into_iter().collect();
        for s in &got.signals {
            if let (Some(name), Some(v)) = (s.name.strip_prefix("V(").and_then(|n| n.strip_suffix(')')), mean(&s.values))
                && let Some(slot) = self.volts.get_mut(name)
            {
                *slot = v;
            }
        }
        if let Some(p) = &self.probe {
            let want = format!("V({p})");
            if let Some(sig) = got.signals.iter().find(|s| s.name.eq_ignore_ascii_case(&want)) {
                // at most a few hundred points a frame: the screen has no more pixels than that
                let step = (got.time.len() / 200).max(1);
                for (k, (t, v)) in got.time.iter().zip(&sig.values).enumerate() {
                    if k % step == 0 || k + 1 == got.time.len() {
                        self.trace.push_back((*t, *v));
                    }
                }
            } else if is_ground(p) {
                self.trace.push_back((run.time(), 0.0));
            }
            let from = run.time() - SCOPE_SPAN * self.speed;
            while self.trace.front().is_some_and(|(t, _)| *t < from) {
                self.trace.pop_front();
            }
        }
    }
}

/// The time-weighted average of a signal over its points (trapezoids); the one point when there is only one.
fn average(time: &[f64], values: &[f64]) -> Option<f64> {
    let n = time.len().min(values.len());
    let span = if n >= 2 { time[n - 1] - time[0] } else { 0.0 };
    if span <= 0.0 {
        return values.get(n.wrapping_sub(1)).copied();
    }
    let area: f64 = (1..n).map(|k| (time[k] - time[k - 1]) * (values[k] + values[k - 1]) / 2.0).sum();
    Some(area / span)
}

fn is_ground(n: &str) -> bool {
    matches!(n.to_ascii_uppercase().as_str(), "GND" | "0" | "VSS" | "AGND" | "DGND")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The circuit as the sample's own breadboard layout joins it (the schematic's, laid out automatically).
    fn board(e: &crate::schematic::SchEditor) -> sim_core::Circuit {
        let l = sch_core::breadboard::auto_layout(&e.sch);
        e.board_circuit(&sch_core::breadboard::board_nets(&e.sch, &l), None).unwrap()
    }

    fn sample(name: &str) -> crate::schematic::SchEditor {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let mut e = crate::schematic::SchEditor::default();
        e.load_path(root.join(name));
        e
    }

    /// The 555 blinker blinks: run on, its LED is lit at some frames and dark at others.
    #[test]
    fn the_blinker_blinks_while_it_runs() {
        let e = sample("ne555_blinker.schjson");
        let leds: Vec<String> = e.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
        let mut live = Live::start(board(&e), &leds, e.live_switches(), &[]);
        assert!(live.error.is_none(), "{:?}", live.error);
        let (mut lit, mut dark) = (false, false);
        for _ in 0..60 {
            live.advance(0.05);
            let i = live.current(&leds[0]).unwrap_or(0.0);
            lit |= i > 2e-3;
            dark |= i < 0.2e-3;
        }
        assert!(live.time() > 2.9, "{}", live.time());
        assert!(lit && dark, "the LED should blink (lit {lit}, dark {dark})");
    }

    /// A push button is clicked on the breadboard: held down, the half adder's sum LED lights; let go, it goes dark.
    #[test]
    fn a_push_button_held_down_changes_the_circuit() {
        let e = sample("half_adder.schjson");
        let leds: Vec<String> = e.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
        let mut live = Live::start(board(&e), &leds, e.live_switches(), &[]);
        assert!(live.error.is_none(), "{:?}", live.error);
        assert!(!live.switches.is_empty());
        let lit = |l: &Live| leds.iter().filter(|d| l.current(d).unwrap_or(0.0) > 1e-3).count();
        live.advance(0.01);
        let before = lit(&live);
        live.press(0);
        live.advance(0.01);
        let pressed = lit(&live);
        assert_ne!(before, pressed, "pressing the first switch changes the LEDs");
        if live.switches[0].push {
            live.release();
            live.advance(0.01);
            assert_eq!(lit(&live), before, "let go, the button opens again");
        }
    }

    /// The light sensor's light is a source off the board: turned up while it runs, the LDR's divider moves at once.
    #[test]
    fn a_knob_turns_a_source_while_it_runs() {
        let e = sample("light_sensors.schjson");
        let l = sch_core::breadboard::auto_layout(&e.sch);
        let knobs: Vec<String> = l.off_board.iter().map(|o| o.refdes.clone()).collect();
        let mut live = Live::start(board(&e), &[], vec![], &knobs);
        assert!(live.error.is_none(), "{:?}", live.error);
        let k = live.knob_index("V2").expect("the light source V2 is a knob");
        let nets = sch_core::breadboard::board_nets(&e.sch, &l);
        let out = nets.pins.get(&("R1".to_string(), "2".to_string())).or_else(|| nets.pins.get(&("R1".to_string(), "1".to_string()))).unwrap().clone();
        live.advance(0.01);
        let dark = live.voltage(&out).unwrap();
        let (_, hi) = live.knobs[k].range().into_inner();
        live.set_knob(k, hi);
        live.advance(0.01);
        let bright = live.voltage(&out).unwrap();
        assert!((bright - dark).abs() > 0.05, "more light moves the divider: {dark} -> {bright}");
    }

    /// The scope keeps the last seconds of its net.
    #[test]
    fn the_scope_traces_its_net() {
        let e = sample("ne555_blinker.schjson");
        let mut live = Live::start(board(&e), &[], vec![], &[]);
        let net = live.volts.keys().find(|n| !n.contains('.') && !n.contains('#')).cloned().unwrap();
        live.set_probe(Some(net));
        for _ in 0..10 {
            live.advance(0.05);
        }
        let (t, v) = live.trace();
        assert!(t.len() > 10 && t.len() == v.len());
        assert!(t.windows(2).all(|w| w[1] >= w[0]));
    }

    /// The board running with its microcontroller and sketch (as the proto tab starts it).
    fn with_sketch(e: &crate::schematic::SchEditor) -> Live {
        let l = sch_core::breadboard::auto_layout(&e.sch);
        let nets = sch_core::breadboard::board_nets(&e.sch, &l);
        let leds: Vec<String> = e.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
        let pots: Vec<String> = e.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Pot).map(|c| c.refdes.clone()).collect();
        let mut live = Live::start(e.board_circuit(&nets, None).unwrap(), &leds, vec![], &pots);
        for refdes in crate::breadboard::sketch_ui::boards(&e.sch) {
            let pins = crate::breadboard::sketch_ui::pin_nets(&e.sch, &refdes, &nets);
            let board = crate::breadboard::sketch_ui::board_of(&e.sch, &refdes);
            live.add_mcu(refdes.clone(), board, sim_core::sketch::compile(&crate::breadboard::sketch_ui::sketch_of(e, &refdes), board), pins);
        }
        live
    }

    /// The Arduino blink sample: the sketch turns D13 on and off every half second, and the LED behind it follows.
    #[test]
    fn the_arduino_sketch_blinks_the_led() {
        let e = sample("arduino_blink.schjson");
        let mut live = with_sketch(&e);
        assert!(live.error.is_none(), "{:?}", live.error);
        assert_eq!(live.mcus.len(), 1);
        let mut seen = vec![];
        for _ in 0..8 {
            live.advance(0.25);
            seen.push(live.current("D1").unwrap_or(0.0) > 2e-3);
        }
        assert!(live.mcus[0].machine.as_ref().unwrap().error.is_none());
        // read every 0.25 s: on for 0.5 s, off for 0.5 s (a reading at the very moment of a change shows the state up to it)
        assert_eq!(seen, [true, true, false, false, true, true, false, false], "lit every other half second");
    }

    /// The Arduino pot sample: turning the knob up brightens the LED on D9, and the readings go to the serial monitor.
    #[test]
    fn the_arduino_reads_the_knob_and_sets_the_led() {
        let e = sample("arduino_pot.schjson");
        let mut live = with_sketch(&e);
        let k = live.knob_index("VR1").unwrap();
        let at = |live: &mut Live, pos: f64| {
            live.set_knob(k, pos);
            // the sketch reads the knob every 200 ms: the LED is read over a frame after it has
            live.advance(0.5);
            live.advance(0.05);
            live.current("D1").unwrap_or(0.0)
        };
        let (low, high) = (at(&mut live, 0.9), at(&mut live, 0.1));
        // the wiper nearer pin 1 (5 V) reads more
        assert!(high > low + 2e-3, "low {low} high {high}");
        let serial = &live.mcus[0].machine.as_ref().unwrap().serial;
        assert!(serial.lines().count() >= 4, "{serial}");
    }

    /// The board running the real chip with a program (W5 (b)): avr-sim's test firmware (tests/firmware/check.c: D13 turns over
    /// every 10 ms, D9 is a 25 % PWM of 1 kHz).
    fn with_real(e: &crate::schematic::SchEditor) -> Live {
        let l = sch_core::breadboard::auto_layout(&e.sch);
        let nets = sch_core::breadboard::board_nets(&e.sch, &l);
        let leds: Vec<String> = e.sch.components.iter().filter(|c| c.kind == sch_core::Kind::Led).map(|c| c.refdes.clone()).collect();
        let mut live = Live::start(e.board_circuit(&nets, None).unwrap(), &leds, vec![], &[]);
        let image = avr_sim::read_elf(include_bytes!("../../../avr-sim/tests/firmware/check.elf")).unwrap();
        for refdes in crate::breadboard::sketch_ui::boards(&e.sch) {
            let pins = crate::breadboard::sketch_ui::pin_nets(&e.sch, &refdes, &nets);
            live.add_real_mcu(refdes.clone(), crate::breadboard::sketch_ui::board_of(&e.sch, &refdes), pins);
            assert!(live.mcus.last().unwrap().waiting);
            live.give_program(&refdes, &image).unwrap();
        }
        live
    }

    /// The real chip's D13 lights the blink sample's LED on and off as its program says (every 10 ms here).
    #[test]
    fn the_real_chip_blinks_the_led() {
        let e = sample("arduino_blink.schjson");
        let mut live = with_real(&e);
        assert!(live.error.is_none(), "{:?}", live.error);
        let mut lit = vec![];
        for _ in 0..40 {
            live.advance(0.005);
            lit.push(live.current("D1").unwrap_or(0.0) > 2e-3);
        }
        let changes = lit.windows(2).filter(|w| w[0] != w[1]).count();
        // 0.2 s: turned over 20 times by the program; read every 5 ms, so each change is seen
        assert!((17..=21).contains(&changes), "{changes} changes: {lit:?}");
        assert!(live.mcus[0].real.as_ref().unwrap().serial.starts_with("ee=42"));
    }

    /// The pot sample's LED on D9 with a sketch of the built-in reader writing `analogWrite(9, v)`: its current over a frame.
    fn led_with_analog_write(v: u8) -> f64 {
        let mut e = sample("arduino_pot.schjson");
        e.sketches.insert("U1".into(), format!("void setup() {{ pinMode(9, OUTPUT); analogWrite(9, {v}); }}
void loop() {{ delay(100); }}"));
        let mut live = with_sketch(&e);
        live.advance(0.02);
        live.advance(0.05);
        live.current("D1").unwrap_or(0.0)
    }

    /// `analogWrite` gives pulses, not their average voltage: a quarter of the time High lights the LED with a quarter of its
    /// current (the average voltage, 1.25 V, would be under the LED's forward voltage and leave it dark).
    #[test]
    fn analog_write_lights_an_led_for_its_share_of_the_time() {
        let (dim, bright) = (led_with_analog_write(64), led_with_analog_write(255));
        assert!(bright > 5e-3, "{bright}");
        assert!((dim / bright - 0.25).abs() < 0.03, "dim {dim} bright {bright}");
    }

    /// The real chip's timer PWM on D9 (25 % of 1 kHz) reaches the circuit as pulses: the LED has a quarter of its full current,
    /// and the pin's net reads a quarter of the supply on average.
    #[test]
    fn the_real_chips_pwm_dims_the_led() {
        let e = sample("arduino_pot.schjson");
        let mut live = with_real(&e);
        live.advance(0.02);
        live.advance(0.05);
        let dim = live.current("D1").unwrap_or(0.0);
        let bright = led_with_analog_write(255);
        assert!((dim / bright - 0.25).abs() < 0.03, "dim {dim} bright {bright}");
        let net = live.mcus[0].nets[9].clone().expect("D9 is wired");
        let v = live.voltage(&net).unwrap();
        assert!((v - 1.25).abs() < 0.1, "D9 {v} V");
    }

    /// The Mega sample: the Mega stays beside the breadboard with wires from only the pins it uses, and the sketch (A8, D22..D28,
    /// numbers of the Mega's own) runs a light along the four LEDs.
    #[test]
    fn the_mega_runs_a_light_along_its_leds() {
        let e = sample("arduino_mega.schjson");
        let l = sch_core::breadboard::auto_layout(&e.sch);
        let beside = l.off_board.iter().find(|o| o.refdes == "U1").expect("the Mega beside the board");
        // D22, D24, D26, D28, A8, 5V, GND: 7 of its 86 pins
        assert_eq!(beside.pins.len(), 7, "{:?}", beside.pins);
        assert!(!l.parts.iter().any(|p| p.refdes == "U1"));
        let mut live = with_sketch(&e);
        assert!(live.error.is_none(), "{:?}", live.error);
        assert!(live.mcus[0].machine.as_ref().is_some_and(|m| m.error.is_none()));
        let mut lit = std::collections::BTreeSet::new();
        for _ in 0..40 {
            live.advance(0.05);
            for d in ["D1", "D2", "D3", "D4"] {
                if live.current(d).unwrap_or(0.0) > 2e-3 {
                    lit.insert(d);
                }
            }
        }
        assert_eq!(lit.len(), 4, "every LED lit in turn: {lit:?}");
        let serial = &live.mcus[0].machine.as_ref().unwrap().serial;
        assert!(serial.lines().count() >= 1, "{serial}");
    }
}
