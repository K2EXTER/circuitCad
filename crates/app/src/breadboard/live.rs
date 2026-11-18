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

/// A supply or a source off the board whose voltage is turned while the circuit runs (a bench supply's knob, the light on a
/// light sensor's input): its part, the value it was drawn with, and the value now.
#[derive(Clone, Debug)]
pub struct Knob {
    pub refdes: String,
    pub nominal: f64,
    pub value: f64,
}

impl Knob {
    /// The range of the slider: from 0 to twice the drawn value (at least 1 V either way).
    pub fn range(&self) -> std::ops::RangeInclusive<f64> {
        let far = (2.0 * self.nominal.abs()).max(1.0);
        if self.nominal < 0.0 { -far..=0.0 } else { 0.0..=far }
    }
}

pub struct Live {
    run: Option<TranRun>,
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
}

impl Live {
    /// Starts the run on `circuit`. `currents`: the elements whose current is read (the LEDs); `switches`: from
    /// `SchEditor::live_switches`; `knobs`: the parts whose DC voltage may be turned (the ones that are DC sources in the circuit).
    pub fn start(circuit: sim_core::Circuit, currents: &[String], switches: Vec<(String, String, bool)>, knobs: &[String]) -> Live {
        let knobs = circuit
            .elements
            .iter()
            .filter_map(|e| match e {
                sim_core::Element::VSource { name, wave: Waveform::Dc(v), .. } if knobs.contains(name) => Some(Knob { refdes: name.clone(), nominal: *v, value: *v }),
                _ => None,
            })
            .collect();
        let mut me = Live {
            run: None,
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

    pub fn time(&self) -> f64 {
        self.run.as_ref().map_or(0.0, |r| r.time())
    }

    pub fn voltage(&self, net: &str) -> Option<f64> {
        self.volts.iter().find(|(n, _)| n.eq_ignore_ascii_case(net)).map(|(_, v)| *v)
    }

    /// The operating point now (for the rating check while it runs).
    pub fn snapshot(&self) -> Option<sim_core::DcResult> {
        self.run.as_ref().map(|r| r.snapshot())
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

    /// Turns knob `i` to `v` volts from now on.
    pub fn set_knob(&mut self, i: usize, v: f64) {
        let Some(k) = self.knobs.get_mut(i) else { return };
        if k.value == v {
            return;
        }
        k.value = v;
        let name = k.refdes.clone();
        if let Some(r) = self.run.as_mut() {
            r.set_source(&name, Waveform::Dc(v));
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
        self.currents.clear();
        for s in &got.signals {
            if let (Some(name), Some(v)) = (s.name.strip_prefix("I(").and_then(|n| n.strip_suffix(')')), s.values.last()) {
                self.currents.insert(name.to_string(), *v);
            }
        }
        self.volts = run.voltages().into_iter().collect();
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
}
