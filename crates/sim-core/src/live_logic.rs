//! "デジタル回路を動かしながら見る" (候補 B8): a learning mode laid over the schematic. The switches and push buttons are
//! clicked to 0 / 1, the pulse sources become a clock the learner drives (speed, pause, one clock, one event), the nets are
//! painted High / Low / X, the pins of the logic parts carry 0 / 1, the LEDs light, a logic analyser runs to the right, and a
//! truth table fills as the inputs are tried. Slow motion stretches the gate delays (nanoseconds) to seconds.
//!
//! There is no separate engine: it is the transient run kept open (`TranRun`) and solved on to the time of the display each
//! frame, with the switch and clock sources changed in between (`TranRun::set_source`).
//!
//! This is the part without a screen, shared by the app (`app/src/schematic/live_logic.rs`, which draws it over the schematic
//! with the logic analyser) and the browser version (`crates/web`). Time comes in as `tick(dt)`: the caller measures it.

use crate::logic_level::{self, Level};
use crate::{TimingViolation, TranRun, TranSettings, Waveform};
use sch_core::{extract, net_at, Component, Kind, Pt, Schematic};
use std::collections::{BTreeMap, BTreeSet};

/// The first end of the run (seconds of simulated time) and how far it is pushed on when reached.
const SIM_STOP: f64 = 60.0;
/// The largest step: the circuits of this mode have no capacitors that need it smaller, and the digital events keep their own time.
const SIM_STEP: f64 = 0.02;
/// Simulated seconds per second of the screen in slow motion: a gate delay of 10 ns takes one second.
pub const SLOW_RATE: f64 = 10e-9;
/// How far "1 イベント進める" goes when no output is on its way yet (an input that was just switched has to be solved first).
const EVENT_NUDGE: f64 = 1e-9;
/// How far past an event "1 イベント進める" stops: an output that drives an LED (an analog node) shows in the solve after its event.
const EVENT_PAST: f64 = 0.2e-9;
/// Seconds of the screen the logic analyser keeps.
pub const HISTORY: f64 = 30.0;
/// The truth table is kept for circuits with at most this many inputs (16 rows).
const TRUTH_INPUTS: usize = 4;
/// An LED lights when its anode is this much above its cathode.
const LED_ON_V: f64 = 1.2;

/// A pulse on a logic net shorter than this (simulated seconds) is a glitch: a gate saw its inputs change one after the other.
const GLITCH_MAX: f64 = 100e-9;

/// How long (screen seconds) a glitch stays marked on its wires.
pub const GLITCH_SHOW: f64 = 4.0;

/// How many digital events a frame solves one by one (to see short pulses) before it jumps to the frame's time.
const EVENTS_PER_FRAME: usize = 400;

/// A short pulse seen on a net: its width (s) and when it was seen (screen seconds).
#[derive(Clone, Debug)]
pub struct Glitch {
    pub net: String,
    pub width: f64,
    pub high: bool,
    pub seen: f64,
}
/// The speed of the clock: the range of the knob (Hz).
pub const HZ_RANGE: (f64, f64) = (0.5, 10.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// A push button: closed while the mouse holds it down.
    Push,
    /// A slide switch: a click moves the lever.
    Slide,
}

/// A switch the learner clicks: its part, the source inside its model that holds its position, and the net it gives the logic.
#[derive(Clone, Debug)]
pub struct Input {
    pub refdes: String,
    pub source: String,
    pub kind: InputKind,
    pub on: bool,
    pub net: Option<String>,
}

/// A pulse source of the circuit, driven by the knob and the buttons instead of its own waveform.
#[derive(Clone, Debug)]
pub struct Clock {
    pub source: String,
    pub lo: f64,
    pub hi: f64,
    pub high: bool,
}

#[derive(Clone, Debug)]
pub struct Led {
    pub refdes: String,
    pub anode: Option<String>,
    pub cathode: Option<String>,
    /// The name in the truth table: the net that drives it through its resistor, else its part number.
    pub name: String,
    pub lit: bool,
}

/// A 7-segment display (a library part with the glyph `seg7`): the nets of its segments a..g and dp, of its common pin, and which
/// segments carry current now (either way round, so a common-anode display lights as well).
#[derive(Clone, Debug)]
pub struct Seg7 {
    pub refdes: String,
    pub segs: [Option<String>; 8],
    pub com: Option<String>,
    pub lit: [bool; 8],
}

impl Seg7 {
    /// The digit (or letter) the lit segments make, if they make one of 0-9 / A-F.
    pub fn shown(&self) -> Option<char> {
        let bits: u8 = self.lit[..7].iter().enumerate().map(|(k, on)| (*on as u8) << k).sum();
        // the digits also as some decoders draw them (the CD4511B: 6 without its top, 9 without its tail; 7 with f), before the letters
        if let Some(&(_, d)) = SEG7_PLAIN.iter().find(|(b, _)| *b == bits) {
            return Some(d);
        }
        SEG7_DIGITS.iter().position(|d| *d == bits).map(|k| std::char::from_digit(k as u32, 16).unwrap().to_ascii_uppercase())
    }
}

/// The segments (bit 0 = a ... bit 6 = g) of 0-9 and A-F.
/// Other ways of drawing 6, 9 and 7.
const SEG7_PLAIN: [(u8, char); 3] = [(0x7C, '6'), (0x67, '9'), (0x27, '7')];

const SEG7_DIGITS: [u8; 16] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F, 0x77, 0x7C, 0x39, 0x5E, 0x79, 0x71];

/// The truth table: one row per combination of the inputs (the first input is the highest bit), each output 0 / 1 as seen,
/// or `None` when the same inputs have shown both (a circuit with a memory).
#[derive(Clone, Debug, Default)]
pub struct Truth {
    pub ins: Vec<String>,
    pub outs: Vec<String>,
    pub rows: BTreeMap<u32, Vec<Option<bool>>>,
}

impl Truth {
    pub fn complete(&self) -> bool {
        !self.ins.is_empty() && self.rows.len() == 1 << self.ins.len()
    }
}

pub struct LiveLogic {
    run: Option<TranRun>,
    pub error: Option<String>,
    /// The circuit the mode was started on: a change of it ends the mode.
    pub signature: String,
    pub inputs: Vec<Input>,
    pub clocks: Vec<Clock>,
    pub leds: Vec<Led>,
    pub seg7s: Vec<Seg7>,
    /// The stored states of the flip-flops of each part (refdes, the states in the order of their cells' names).
    pub memory: Vec<(String, Vec<bool>)>,
    /// Per logic net: its level, when it last changed (simulated s) and the level before that, to find short pulses.
    pub edges: BTreeMap<String, (Level, f64, Level)>,
    /// The nets the clocks drive: their pulses are made short on purpose ("1 クロック"), not glitches.
    pub clock_nets: BTreeSet<String>,
    /// The glitches seen lately, newest last.
    pub glitches: Vec<Glitch>,
    /// The flip-flops' timing violations so far (setup, hold, pulse width): one line each.
    pub races: Vec<TimingViolation>,
    /// The net of every wire, and of every pin of a logic part off the supply (component, point, net).
    pub wire_net: Vec<Option<String>>,
    pub pin_nets: Vec<(usize, Pt, String)>,
    /// The level of every net now, and the High level of the logic.
    pub levels: BTreeMap<String, Level>,
    /// The voltage of those nets now, for the ones between Low and High (an RC on its way, a pull-up against a load).
    pub volts: BTreeMap<String, f64>,
    pub vdd: f64,
    pub running: bool,
    pub slow: bool,
    pub hz: f64,
    /// Seconds of the screen since the clock last changed.
    pub clock_wait: f64,
    /// Clock edges still to make for "1 クロック進める" while paused.
    pub burst: u8,
    /// An input changed since the run was last solved on.
    pub dirty: bool,
    /// Seconds of the screen since the start (the time axis of the logic analyser).
    pub wall: f64,
    /// The logic analyser: the nets watched and their voltages at each frame.
    pub watched: Vec<String>,
    pub hist_t: Vec<f64>,
    pub hist_v: Vec<Vec<f64>>,
    pub truth: Option<Truth>,
    /// The push button held down by the mouse.
    pub held: Option<usize>,
    /// Simulated seconds solved with "1 イベント進める" and the clock edges made, for the status line and the lessons.
    pub events_stepped: usize,
    pub clock_edges: usize,
}

/// What kind of switch a part is, from the source its model holds the position in (`SW_PUSH`: `Vpress`, `SW_SLIDE`: `Vpos`).
pub fn switch_of(c: &Component) -> Option<(InputKind, String)> {
    let text = &c.custom.as_ref()?.sim.as_ref()?.text;
    let has = |name: &str| text.lines().any(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(name)));
    if has("Vpress") {
        Some((InputKind::Push, format!("{}.Vpress", c.refdes)))
    } else if has("Vpos") {
        Some((InputKind::Slide, format!("{}.Vpos", c.refdes)))
    } else {
        None
    }
}

pub fn is_logic_part(c: &Component) -> bool {
    c.custom.as_ref().and_then(|s| s.sim.as_ref()).is_some_and(|m| m.text.lines().any(|l| l.trim_start().starts_with(['Y', 'y'])))
}

impl LiveLogic {
    /// Prepares the mode on the schematic `sch` and its circuit `circuit` (the app: `SchEditor::sim_circuit`, the browser: `from_schematic`), and starts the run. `signature` is what the caller compares to end the mode when the circuit changes.
    pub fn new(sch: &Schematic, mut circuit: crate::Circuit, signature: String) -> LiveLogic {
        let nets = extract(sch);
        let net_of = |refdes: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).map(|n| n.name.clone());
        // the pulse sources become clocks held at their first level
        let mut clocks = vec![];
        for e in &mut circuit.elements {
            if let crate::Element::VSource { name, wave, .. } = e
                && let Waveform::Pulse { v1, v2, .. } = *wave
            {
                clocks.push(Clock { source: name.clone(), lo: v1, hi: v2, high: false });
                *wave = Waveform::Dc(v1);
            }
        }
        // the nets of the supply: what the DC sources, power symbols and ground touch
        let mut supply = BTreeSet::new();
        for c in &sch.components {
            let dc_source = c.kind == Kind::VSource && !clocks.iter().any(|k| k.source == c.refdes);
            if dc_source || matches!(c.kind, Kind::Power | Kind::Ground) {
                for (pin, _) in c.pins() {
                    if let Some(n) = net_of(&c.refdes, &pin) {
                        supply.insert(n);
                    }
                }
            }
        }
        supply.insert("GND".into());
        let mut inputs = vec![];
        for c in &sch.components {
            if let Some((kind, source)) = switch_of(c) {
                let pins: Vec<String> = c.pins().into_iter().map(|p| p.0).collect();
                // the net it gives the logic: a slide switch's common pin, else the pin away from the supply
                let net = match kind {
                    InputKind::Slide => net_of(&c.refdes, "2"),
                    InputKind::Push => pins.iter().filter_map(|p| net_of(&c.refdes, p)).find(|n| !supply.contains(n)),
                };
                inputs.push(Input { refdes: c.refdes.clone(), source, kind, on: false, net });
            }
        }
        inputs.sort_by(|a, b| natural(&a.refdes, &b.refdes));
        let mut leds = vec![];
        for c in sch.components.iter().filter(|c| c.kind == Kind::Led) {
            let (anode, cathode) = (net_of(&c.refdes, "A"), net_of(&c.refdes, "K"));
            // the net that drives it through its resistor (a resistor from the anode or cathode net to a net with a name)
            let through = |n: &Option<String>| {
                let n = n.as_ref()?;
                sch.components.iter().filter(|r| r.kind == Kind::Resistor).find_map(|r| {
                    let (a, b) = (net_of(&r.refdes, "1")?, net_of(&r.refdes, "2")?);
                    let other = if &a == n { b } else if &b == n { a } else { return None };
                    (!supply.contains(&other)).then_some(other)
                })
            };
            let name = through(&anode).or_else(|| through(&cathode)).unwrap_or_else(|| c.refdes.clone());
            leds.push(Led { refdes: c.refdes.clone(), anode, cathode, name, lit: false });
        }
        leds.sort_by(|a, b| natural(&a.refdes, &b.refdes));
        let mut seg7s = vec![];
        for c in sch.components.iter() {
            let Some(sym) = c.custom.as_ref().filter(|s| s.glyph == "seg7") else { continue };
            let net = |name: &str| sym.pins.iter().find(|p| p.name.eq_ignore_ascii_case(name)).and_then(|p| net_of(&c.refdes, &p.number));
            let segs = ["a", "b", "c", "d", "e", "f", "g", "dp"].map(net);
            seg7s.push(Seg7 { refdes: c.refdes.clone(), segs, com: net("COM"), lit: [false; 8] });
        }
        let mut pin_nets = vec![];
        for (i, c) in sch.components.iter().enumerate().filter(|(_, c)| is_logic_part(c)) {
            for (pin, pt) in c.pins() {
                if let Some(n) = net_of(&c.refdes, &pin).filter(|n| !supply.contains(n)) {
                    pin_nets.push((i, pt, n));
                }
            }
        }
        let clock_nets: Vec<String> = sch.components.iter().filter(|c| clocks.iter().any(|k| k.source == c.refdes)).filter_map(|c| net_of(&c.refdes, "+")).collect();
        // the nets that carry logic (a pin of a logic part, a switch, a clock): only these get the colours of the levels (the
        // net between an LED and its resistor is at the LED's forward voltage, neither High nor Low)
        let logic_nets: BTreeSet<String> = pin_nets.iter().map(|p| p.2.clone()).chain(inputs.iter().filter_map(|i| i.net.clone())).chain(clock_nets.iter().cloned()).collect();
        let wire_net = sch.wires.iter().map(|w| net_at(sch, w.a).filter(|n| logic_nets.contains(n))).collect();
        // the logic analyser: the inputs, the clocks, then the other nets with a label
        let mut watched: Vec<String> = vec![];
        let mut watch = |n: &str| {
            if logic_nets.contains(n) && !watched.iter().any(|w| w == n) && watched.len() < 12 {
                watched.push(n.to_string());
            }
        };
        for i in &inputs {
            if let Some(n) = &i.net {
                watch(n);
            }
        }
        for n in &clock_nets {
            watch(n);
        }
        let mut labelled: Vec<&str> = sch.labels.iter().map(|l| l.text.as_str()).collect();
        labelled.sort_by(|a, b| natural(a, b));
        for l in labelled {
            if nets.iter().any(|n| n.name == l) {
                watch(l);
            }
        }
        let ins: Vec<String> = inputs.iter().map(|i| i.net.clone().unwrap_or_else(|| i.refdes.clone())).collect();
        let truth = (clocks.is_empty() && !inputs.is_empty() && inputs.len() <= TRUTH_INPUTS && !leds.is_empty())
            .then(|| Truth { ins, outs: leds.iter().map(|l| l.name.clone()).collect(), rows: BTreeMap::new() });
        let n_watch = watched.len();
        let mut me = LiveLogic {
            run: None,
            error: None,
            signature,
            inputs,
            clocks,
            leds,
            seg7s,
            memory: vec![],
            edges: BTreeMap::new(),
            clock_nets: clock_nets.into_iter().collect(),
            glitches: vec![],
            races: vec![],
            wire_net,
            pin_nets,
            levels: BTreeMap::new(),
            volts: BTreeMap::new(),
            vdd: 5.0,
            running: true,
            slow: false,
            hz: 1.0,
            clock_wait: 0.0,
            burst: 0,
            dirty: false,
            wall: 0.0,
            watched,
            hist_t: vec![],
            hist_v: vec![vec![]; n_watch],
            truth,
            held: None,
            events_stepped: 0,
            clock_edges: 0,
        };
        match TranRun::start(&circuit, &TranSettings { tstop: SIM_STOP, tstep: SIM_STEP, uic: false }, Some(&|n: &str| n.starts_with("V("))) {
            Ok(run) => me.run = Some(run),
            Err(e) => me.error = Some(format!("動かせませんでした: {e}")),
        }
        me.refresh();
        me
    }

    pub fn time(&self) -> f64 {
        self.run.as_ref().map_or(0.0, |r| r.time())
    }

    /// The next change of a gate's output that is on its way (slow motion and "1 イベント進める" show them one by one).
    pub fn next_event(&self) -> Option<f64> {
        self.run.as_ref()?.next_digital_event()
    }

    /// Nothing is on its way: the inputs have been solved and no output is waiting for its delay.
    pub fn settled(&self) -> bool {
        !self.dirty && self.next_event().is_none()
    }

    pub fn level(&self, net: &str) -> Option<Level> {
        self.levels.get(net).copied()
    }

    pub fn wire_level(&self, i: usize) -> Option<Level> {
        self.level(self.wire_net.get(i)?.as_deref()?)
    }

    fn set_source(&mut self, name: &str, v: f64) {
        if let Some(r) = self.run.as_mut() {
            r.set_source(name, Waveform::Dc(v));
            self.dirty = true;
        }
    }

    /// Flips the slide switch `i`, or presses / releases the push button `i`.
    pub fn set_input(&mut self, i: usize, on: bool) {
        let Some(inp) = self.inputs.get_mut(i) else { return };
        if inp.on == on {
            return;
        }
        inp.on = on;
        let src = inp.source.clone();
        self.set_source(&src, if on { 1.0 } else { 0.0 });
    }

    pub fn input_index(&self, refdes: &str) -> Option<usize> {
        self.inputs.iter().position(|i| i.refdes.eq_ignore_ascii_case(refdes))
    }

    /// Every clock to its other level (one edge).
    pub fn flip_clocks(&mut self) {
        for k in 0..self.clocks.len() {
            let c = &mut self.clocks[k];
            c.high = !c.high;
            let (src, v) = (c.source.clone(), if c.high { c.hi } else { c.lo });
            self.set_source(&src, v);
        }
        self.clock_edges += 1;
        self.clock_wait = 0.0;
    }

    /// Solves on to simulated time `t` (pushing the end of the run on when needed).
    fn solve_to(&mut self, t: f64) {
        let Some(run) = self.run.as_mut() else { return };
        if t > run.stop_time() {
            run.extend(t * 2.0);
        }
        // event by event (a few hundred at most per frame), looking at the logic nets after each: a pulse of a few nanoseconds
        // falls between two points of an ordinary step and would go unseen
        let th = logic_level::thresholds(self.vdd);
        let watch: Vec<String> = self.wire_net.iter().flatten().chain(self.pin_nets.iter().map(|p| &p.2)).filter(|n| !self.clock_nets.contains(*n)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        let mut seen: Vec<(f64, Vec<Level>)> = vec![];
        let mut result = Ok(());
        for k in 0..EVENTS_PER_FRAME {
            // a switch or a clock just changed: nothing is waiting yet until its line has been solved once (1 ns on)
            let next = match run.next_digital_event() {
                None if k == 0 && run.time() + 1e-9 < t => Some(run.time() + 1e-9 - EVENT_PAST),
                e => e,
            };
            match next {
                Some(te) if te + EVENT_PAST < t => {
                    result = run.advance_to(te + EVENT_PAST);
                    if result.is_err() {
                        break;
                    }
                    seen.push((run.time(), watch.iter().map(|n| logic_level::level(run.voltage(n).unwrap_or(0.0), th)).collect()));
                }
                _ => break,
            }
        }
        if result.is_ok() {
            result = run.advance_to(t);
            seen.push((run.time(), watch.iter().map(|n| logic_level::level(run.voltage(n).unwrap_or(0.0), th)).collect()));
        }
        if let Err(e) = result {
            self.error = Some(format!("計算が止まりました: {e}"));
            self.run = None;
            return;
        }
        // the points solved on the way are not kept
        let _ = run.drain();
        for (time, levels) in seen {
            for (net, lv) in watch.iter().zip(levels) {
                if lv == Level::Mid {
                    continue;
                }
                match self.edges.get_mut(net) {
                    None => {
                        self.edges.insert(net.clone(), (lv, time, lv));
                    }
                    Some(e) if e.0 != lv => {
                        // back to the level it had before a change shortly ago: a pulse of that width
                        if lv == e.2 && time - e.1 < GLITCH_MAX && time > e.1 {
                            self.glitches.push(Glitch { net: net.clone(), width: time - e.1, high: e.0 == Level::High, seen: self.wall });
                        }
                        *e = (lv, time, e.0);
                    }
                    _ => {}
                }
            }
        }
        let wall = self.wall;
        self.glitches.retain(|g| wall - g.seen < GLITCH_SHOW);
        if self.glitches.len() > 20 {
            self.glitches.drain(..self.glitches.len() - 20);
        }
        self.races = run.timing_violations();
        self.dirty = false;
    }

    /// One frame of `dt` seconds of the screen: the clock runs, the simulated time follows (one to one, or slowed down), the
    /// levels are read.
    pub fn tick(&mut self, dt: f64) {
        if self.run.is_none() {
            return;
        }
        let dt = dt.clamp(0.0, 0.25);
        let flowing = self.running || self.burst > 0 || (self.slow && self.held.is_some());
        if flowing {
            self.wall += dt;
            self.clock_wait += dt;
            let rate = if self.slow { SLOW_RATE } else { 1.0 };
            let target = self.time() + dt * rate;
            self.solve_to(target);
            // the clock: a half period of the knob has passed (in slow motion, also only once the last edge has gone through)
            if !self.clocks.is_empty() {
                let half = 0.5 / self.hz.clamp(HZ_RANGE.0, HZ_RANGE.1);
                let due = if self.burst > 0 { self.clock_wait >= half.min(0.5) } else { self.running && self.clock_wait >= half };
                if due && (!self.slow || self.settled()) {
                    self.flip_clocks();
                    self.burst = self.burst.saturating_sub(1);
                }
            } else if self.burst > 0 && self.settled() {
                self.burst = 0;
            }
        } else if self.dirty && !self.slow {
            // paused at full speed: a switch still takes effect at once (only slow motion holds it for "1 イベント進める")
            let t = self.time() + 1e-6;
            self.solve_to(t);
        }
        self.refresh();
    }

    /// "1 イベント進める": on to the next change of a gate's output (or, after a switch, through the switch's own net).
    pub fn step_event(&mut self) {
        let t = self.time();
        let target = match self.next_event() {
            Some(te) if te > t => te + EVENT_PAST,
            _ => t + EVENT_NUDGE,
        };
        self.solve_to(target);
        self.events_stepped += 1;
        self.refresh();
    }

    /// "1 クロック進める": one rising and one falling edge, then the mode stays paused.
    pub fn step_clock(&mut self) {
        if self.clocks.is_empty() {
            return;
        }
        self.running = false;
        if self.slow {
            // edge by edge at the speed of slow motion
            self.burst = 2;
            self.clock_wait = f64::INFINITY;
        } else {
            for _ in 0..2 {
                self.flip_clocks();
                let t = self.time() + 1e-6;
                self.solve_to(t);
                self.wall += 0.25;
                self.refresh();
            }
        }
    }

    /// Reads the levels of the nets and the LEDs, fills the truth table, and adds a point to the logic analyser.
    fn refresh(&mut self) {
        let Some(run) = self.run.as_ref() else { return };
        self.vdd = run.logic_supply().filter(|v| *v > 0.5).unwrap_or(5.0);
        let th = logic_level::thresholds(self.vdd);
        let volts: BTreeMap<String, f64> = run.voltages().into_iter().collect();
        let v = |n: &str| volts.get(n).copied().unwrap_or(0.0);
        self.levels.clear();
        let names: BTreeSet<&String> = self.wire_net.iter().flatten().chain(self.pin_nets.iter().map(|p| &p.2)).chain(self.watched.iter()).collect();
        self.volts.clear();
        for n in names {
            self.levels.insert(n.clone(), logic_level::level(v(n), th));
            self.volts.insert(n.clone(), v(n));
        }
        for l in &mut self.leds {
            let a = l.anode.as_deref().map_or(0.0, v);
            let k = l.cathode.as_deref().map_or(0.0, v);
            l.lit = a - k > LED_ON_V;
        }
        for s in &mut self.seg7s {
            let com = s.com.as_deref().map_or(0.0, v);
            for (k, seg) in s.segs.iter().enumerate() {
                s.lit[k] = seg.as_deref().is_some_and(|n| (v(n) - com).abs() > LED_ON_V);
            }
        }
        // the flip-flops' stored states, per part (cells are named `U1.YFa`: the part, then the cell)
        let mut memory: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
        for (name, st) in run.cell_states() {
            if let Some((part, cell)) = name.split_once('.') {
                memory.entry(part.to_string()).or_default().push((cell.to_string(), st));
            }
        }
        self.memory = memory
            .into_iter()
            .map(|(part, mut cells)| {
                cells.sort_by(|a, b| natural(&a.0, &b.0));
                (part, cells.into_iter().map(|(_, s)| s).collect())
            })
            .collect();
        if let Some(t) = self.truth.as_mut()
            && !self.dirty
            && run.next_digital_event().is_none()
        {
            let mut key = 0u32;
            for i in &self.inputs {
                let high = i.net.as_deref().is_some_and(|n| v(n) >= th.1);
                key = key << 1 | high as u32;
            }
            let outs: Vec<bool> = self.leds.iter().map(|l| l.lit).collect();
            let row = t.rows.entry(key).or_insert_with(|| outs.iter().map(|&o| Some(o)).collect());
            for (cell, o) in row.iter_mut().zip(&outs) {
                if *cell != Some(*o) {
                    *cell = None;
                }
            }
        }
        // the logic analyser: a point per frame, the last `HISTORY` seconds
        self.hist_t.push(self.wall);
        for (k, n) in self.watched.iter().enumerate() {
            self.hist_v[k].push(v(n));
        }
        let keep_from = self.hist_t.partition_point(|&t| t < self.wall - HISTORY);
        if keep_from > 64 {
            self.hist_t.drain(..keep_from);
            for h in &mut self.hist_v {
                h.drain(..keep_from);
            }
        }
    }

    /// The input under the mouse is pressed (`down`) or let go. With `stay` (Shift + click) a push button stays pressed until
    /// it is clicked again, so that two buttons can be held at once.
    pub fn mouse_on_input(&mut self, i: usize, down: bool, stay: bool) {
        match self.inputs.get(i).map(|x| x.kind) {
            Some(InputKind::Push) if stay || self.inputs[i].on => {
                if down {
                    let on = !self.inputs[i].on;
                    self.set_input(i, on);
                }
            }
            Some(InputKind::Push) => {
                self.held = down.then_some(i);
                self.set_input(i, down);
            }
            Some(InputKind::Slide) if down => {
                let on = !self.inputs[i].on;
                self.set_input(i, on);
            }
            _ => {}
        }
    }

    /// The push button held down is let go (the mouse or finger is lifted).
    pub fn release_held(&mut self) {
        if let Some(i) = self.held.take() {
            self.set_input(i, false);
        }
    }
}

/// `SW2` before `SW10`.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let d = s.trim_end_matches(|c: char| c.is_ascii_digit());
        (d.to_string(), s[d.len()..].parse::<u64>().unwrap_or(0))
    };
    split(a).cmp(&split(b)).then(a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mode started on a sample of `docs/examples`.
    fn start(sample: &str) -> LiveLogic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{sample}.schjson"));
        let sch: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let circuit = crate::from_schematic(&sch).unwrap();
        let ll = LiveLogic::new(&sch, circuit, String::new());
        assert!(ll.error.is_none(), "{:?}", ll.error);
        ll
    }

    fn lit(ll: &LiveLogic) -> Vec<bool> {
        ll.leds.iter().map(|l| l.lit).collect()
    }

    /// The half adder fills its truth table as the two switches are tried: S = A xor B, C = A and B.
    #[test]
    fn the_half_adder_fills_its_truth_table() {
        let mut ll = start("half_adder");
        assert_eq!(ll.inputs.len(), 2);
        assert_eq!(ll.truth.as_ref().unwrap().outs, ["S", "C"]);
        for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
            ll.set_input(0, a);
            ll.set_input(1, b);
            ll.tick(0.05);
            assert_eq!(lit(&ll), [a ^ b, a & b], "A={a} B={b}");
        }
        let t = ll.truth.as_ref().unwrap();
        assert!(t.complete());
        assert_eq!(t.rows[&0b11], [Some(false), Some(true)]);
    }

    /// The SR latch keeps what it was set to: the same inputs (both buttons released) show Q on and off, so its row says "記憶".
    #[test]
    fn the_sr_latch_remembers_and_its_row_says_so() {
        let mut ll = start("sr_latch");
        // it starts in one of its two states and stays there (it does not ring between them)
        ll.tick(0.05);
        assert!(ll.settled(), "the latch is still changing at the start");
        assert!(ll.truth.as_ref().unwrap().rows[&0b11].iter().all(|o| o.is_some()));
        assert_ne!(lit(&ll)[0], lit(&ll)[1], "Q and QB are opposite");
        let (set, reset) = (ll.input_index("SW1").unwrap(), ll.input_index("SW2").unwrap());
        for (i, q) in [(set, true), (reset, false), (set, true)] {
            ll.mouse_on_input(i, true, false);
            ll.tick(0.05);
            ll.release_held();
            ll.tick(0.05);
            assert_eq!(lit(&ll)[0], q, "after pressing input {i}");
        }
        let t = ll.truth.clone().unwrap();
        // both released (both inputs high, through the pull-ups) has shown Q both ways
        assert_eq!(t.rows[&0b11][0], None, "{:?}", t.rows);
    }

    /// "1 クロック進める" counts the counter up by one; slow motion takes several frames per edge and goes through the gate
    /// delays one event at a time.
    #[test]
    fn one_clock_counts_up_by_one_and_slow_motion_shows_the_events() {
        let mut ll = start("live_counter");
        let count = |ll: &LiveLogic| ll.leds.iter().enumerate().map(|(k, l)| (l.lit as u32) << k).sum::<u32>();
        let c0 = count(&ll);
        ll.step_clock();
        assert_eq!(count(&ll), (c0 + 1) % 16);
        ll.step_clock();
        assert_eq!(count(&ll), (c0 + 2) % 16);
        // slow motion: an edge is solved through its delays over several "1 イベント" steps
        ll.slow = true;
        ll.running = false;
        ll.flip_clocks();
        let mut steps = 0;
        while !ll.settled() && steps < 50 {
            ll.step_event();
            steps += 1;
        }
        assert!(steps >= 2, "the clock edge went through at once");
        assert!(ll.settled());
    }

    /// The 7-segment counter: each clock shows the next digit on the display, 9 is followed by 0 (the NAND clears the 74HC161 at
    /// 10), and the counter's four flip-flops are shown as its memory.
    #[test]
    fn the_seven_segment_counter_shows_0_to_9_and_its_memory() {
        let mut ll = start("seg7_counter");
        assert_eq!(ll.seg7s.len(), 1);
        let digit = |ll: &LiveLogic| ll.seg7s[0].shown();
        let first = digit(&ll).and_then(|c| c.to_digit(10)).expect("a digit at the start");
        let mut seen = vec![first];
        for _ in 0..11 {
            ll.step_clock();
            seen.push(digit(&ll).and_then(|c| c.to_digit(10)).unwrap_or_else(|| panic!("no digit after {seen:?}: {:?}", ll.seg7s[0].lit)));
        }
        for w in seen.windows(2) {
            assert_eq!(w[1], (w[0] + 1) % 10, "{seen:?}");
        }
        // the 74HC161's four flip-flops hold the count shown
        let (_, bits) = ll.memory.iter().find(|(p, _)| p == "U1").expect("the counter's memory");
        assert_eq!(bits.len(), 4);
        let value: u32 = bits.iter().enumerate().map(|(k, b)| (*b as u32) << k).sum();
        assert_eq!(value, *seen.last().unwrap(), "{bits:?}");
    }

    /// 9 -> 0 of the decade counter goes through 10 for a moment: the NAND pulls the clear low for a few nanoseconds. That pulse is
    /// a glitch on CLR_N, and the counter's own flip-flops break no timing rule.
    #[test]
    fn the_asynchronous_clear_is_seen_as_a_glitch() {
        let mut ll = start("seg7_counter");
        for _ in 0..12 {
            ll.step_clock();
        }
        let nets: Vec<(&str, f64)> = ll.glitches.iter().map(|g| (g.net.as_str(), g.width)).collect();
        assert!(ll.glitches.iter().any(|g| g.net == "CLR_N" && !g.high && g.width < 50e-9), "{nets:?}");
        assert!(ll.races.is_empty(), "{:?}", ll.races);
    }

    /// The shift register: with SW1 on, each clock moves a 1 one place further along QA..QH.
    #[test]
    fn the_shift_register_moves_its_pattern_one_place_per_clock() {
        let mut ll = start("shift_register");
        let sw = ll.input_index("SW1").unwrap();
        ll.set_input(sw, true);
        ll.tick(0.05);
        for n in 1..=3 {
            ll.step_clock();
            let lit: Vec<bool> = ll.leds.iter().map(|l| l.lit).collect();
            assert_eq!(lit.iter().take_while(|x| **x).count(), n, "after {n} clocks: {lit:?}");
        }
        // a 0 in: the ones move on, a gap behind them
        ll.set_input(sw, false);
        ll.tick(0.05);
        ll.step_clock();
        let lit: Vec<bool> = ll.leds.iter().map(|l| l.lit).collect();
        assert_eq!(&lit[..5], &[false, true, true, true, false], "{lit:?}");
    }

    /// The traffic light: green for four clocks, yellow for one, red for five.
    #[test]
    fn the_traffic_light_goes_green_yellow_red() {
        let mut ll = start("traffic_light");
        let names: Vec<String> = ll.leds.iter().map(|l| l.name.clone()).collect();
        let colour = |ll: &LiveLogic| ll.leds.iter().filter(|l| l.lit).map(|l| l.name.clone()).collect::<Vec<_>>();
        let mut seen = vec![colour(&ll)];
        for _ in 0..10 {
            ll.step_clock();
            seen.push(colour(&ll));
        }
        let one = |k: usize| seen[k].join("+");
        let order: Vec<String> = (0..11).map(one).collect();
        assert_eq!(&order[..10], &["GREEN", "GREEN", "GREEN", "GREEN", "Q4", "RED", "RED", "RED", "RED", "RED"], "{names:?} {order:?}");
        assert_eq!(order[10], "GREEN");
    }
}
