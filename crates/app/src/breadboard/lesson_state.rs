//! What the prototype lessons (`proto_lessons.rs`) look at: the tab's state as plain values (the power, the tool and the meter's range,
//! the last reading the meter and the scope showed, whether the board matches the schematic, the LEDs' currents, the switches, the
//! sketch's serial output). The meter and the scope write what they show (`Seen`) while their panel is drawn.

use super::*;
use std::collections::BTreeMap;

/// The meter's reading as it was shown.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// Volts between the red and the black probe.
    Volts(f64),
    /// Amps through the meter (red in, black out).
    Amps(f64),
    /// The current range with both probes on one net (the meter is not in series).
    SameNet,
    /// The current range across a supply: the meter's fuse went.
    Fuse,
    /// Ohms between the probes (None: open, "OL").
    Ohms(Option<f64>),
}

/// What the meter and the scope showed the last time their panel was drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub reading: Option<Reading>,
    /// The nets the red and the black probe are on.
    pub red: Option<String>,
    pub black: Option<String>,
    /// The scope's net and its running trace's lowest and highest volts (only while the circuit runs).
    pub scope: Option<(String, f64, f64)>,
}

/// The tab's state for a lesson's step.
#[derive(Clone, Debug, Default)]
pub struct LessonState {
    pub powered: bool,
    pub tool: Tool,
    pub range: MeterRange,
    pub seen: Seen,
    /// The parts are plugged in as the schematic joins them (on the breadboard).
    pub matches: bool,
    /// The current through each part the running circuit reads (the LEDs), in amps.
    pub currents: BTreeMap<String, f64>,
    /// Each switch and whether it is on now.
    pub switches: Vec<(String, bool)>,
    /// The knobs (potentiometers, supplies) and where they are now (0..1 for a potentiometer, volts for a supply).
    pub knobs: Vec<(String, f64)>,
    /// What the first microcontroller has written to its serial port so far.
    pub serial: String,
    /// The sketch of the first microcontroller is shown as blocks.
    pub blocks: bool,
    /// The text of the first microcontroller's sketch.
    pub sketch: Option<String>,
}

impl BreadboardView {
    /// The state the prototype lessons judge their steps on.
    pub fn lesson_state(&mut self, sch: &SchEditor) -> LessonState {
        let matches = match self.kind {
            BoardKind::Breadboard => {
                let l = self.layout(sch);
                self.board_check(sch, &l).is_empty()
            }
            _ => true,
        };
        let live = self.live.as_ref().filter(|v| self.powered && v.error.is_none());
        let mcu = sch.sch.components.iter().find(|c| sketch_ui::model_board(c).is_some());
        let sketch = mcu.map(|c| sketch_ui::sketch_of(sch, &c.refdes));
        let blocks = mcu.is_some_and(|c| sch.sketch_blocks.contains(&c.refdes));
        LessonState {
            powered: self.powered,
            tool: self.tool,
            range: self.meter_range,
            seen: self.seen.clone(),
            matches,
            currents: live.map(|lv| sch.sch.components.iter().filter_map(|c| lv.current(&c.refdes).map(|i| (c.refdes.clone(), i))).collect()).unwrap_or_default(),
            switches: live.map(|lv| lv.switches.iter().map(|s| (s.refdes.clone(), s.on)).collect()).unwrap_or_default(),
            knobs: live.map(|lv| lv.knobs.iter().map(|k| (k.refdes.clone(), k.value)).collect()).unwrap_or_default(),
            serial: live
                .and_then(|lv| lv.mcus.first())
                .and_then(|m| m.machine.as_ref().map(|x| x.serial.clone()).or(m.real.as_ref().map(|r| r.serial.clone())))
                .unwrap_or_default(),
            blocks,
            sketch,
        }
    }
}
