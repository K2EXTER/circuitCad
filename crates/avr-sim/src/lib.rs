//! The real instructions of an AVR microcontroller (candidate W5 (b)): the program built by avr-gcc (arduino-cli for a sketch) runs
//! in simavr (bundled in simavr/, GPL-3.0, copied by tools/vendor_simavr.py), cycle by cycle, with its timers, UART, ADC and
//! EEPROM. The circuit is solved beside it: [`Nano::advance`] runs the chip to a time with the pins' voltages of the circuit as its
//! inputs, and gives what each pin does for the circuit (an output's average level over the step: PWM and `tone` too).
//!
//! The ELF is read here ([`read_elf`]): the flash from the loadable segments, the EEPROM from the `.eeprom` one.

use std::ffi::{c_char, CString};

#[repr(C)]
struct CcAvr {
    _p: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CcEvent {
    cycle: u64,
    port: u8,
    bit: u8,
    value: u8,
    timer: u8,
}

unsafe extern "C" {
    fn ccavr_new(mcu: *const c_char, freq: u32, vcc_mv: u32) -> *mut CcAvr;
    fn ccavr_free(c: *mut CcAvr);
    fn ccavr_load(c: *mut CcAvr, flash: *const u8, size: u32, eeprom: *const u8, esize: u32);
    fn ccavr_run_until(c: *mut CcAvr, until: u64) -> i32;
    fn ccavr_cycle(c: *mut CcAvr) -> u64;
    fn ccavr_ports(c: *mut CcAvr, out: *mut u8);
    fn ccavr_set_pin(c: *mut CcAvr, port: c_char, bit: u32, value: u32);
    fn ccavr_set_adc(c: *mut CcAvr, channel: u32, mv: u32);
    fn ccavr_send(c: *mut CcAvr, byte: u8);
    fn ccavr_take_events(c: *mut CcAvr, out: *mut CcEvent, max: u32) -> u32;
    fn ccavr_take_serial(c: *mut CcAvr, out: *mut u8, max: u32) -> u32;
}

/// simavr's states of the core (`cpu_Done`, `cpu_Crashed`: it stopped).
const CPU_DONE: i32 = 6;
const CPU_CRASHED: i32 = 7;

/// The ports read from the chip, in the order B, C, D.
pub const PORTS: [char; 3] = ['B', 'C', 'D'];

/// One change of a pin the program drives: when (cycles from the start), which (port letter, bit), to what, and whether a timer's
/// compare output (PWM, `tone` on a timer pin) set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinEvent {
    pub cycle: u64,
    pub port: char,
    pub bit: u8,
    pub high: bool,
    pub timer: bool,
}

/// PORTx, DDRx, PINx of B, C, D.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ports {
    pub port: [u8; 3],
    pub ddr: [u8; 3],
    pub pin: [u8; 3],
}

fn port_index(p: char) -> usize {
    PORTS.iter().position(|x| *x == p).unwrap_or(0)
}

impl Ports {
    pub fn output(&self, p: char, bit: u8) -> bool {
        self.ddr[port_index(p)] >> bit & 1 == 1
    }
    pub fn high(&self, p: char, bit: u8) -> bool {
        self.port[port_index(p)] >> bit & 1 == 1
    }
}

/// An AVR running a program (one chip; it is let go when dropped).
pub struct Avr {
    c: *mut CcAvr,
    /// The clock (Hz).
    pub freq: u64,
    /// The supply (volts): the level of an output High, the reference of the ADC.
    pub vcc: f64,
    stopped: Option<String>,
}

// the chip's state is only touched through &mut self
unsafe impl Send for Avr {}

impl Drop for Avr {
    fn drop(&mut self) {
        unsafe { ccavr_free(self.c) }
    }
}

impl Avr {
    /// A chip by simavr's name ("atmega328p"), its clock and supply, with `image` in its flash and EEPROM.
    pub fn new(mcu: &str, freq: u64, vcc: f64, image: &Image) -> Result<Avr, String> {
        let name = CString::new(mcu).map_err(|_| "マイコンの名前が読めません".to_string())?;
        let c = unsafe { ccavr_new(name.as_ptr(), freq as u32, (vcc * 1000.0).round() as u32) };
        if c.is_null() {
            return Err(format!("マイコン {mcu} は、まだ動かせません (動かせるのは ATmega328P だけです)"));
        }
        let me = Avr { c, freq, vcc, stopped: None };
        unsafe { ccavr_load(c, image.flash.as_ptr(), image.flash.len() as u32, image.eeprom.as_ptr(), image.eeprom.len() as u32) };
        Ok(me)
    }

    /// The time the chip has run (seconds).
    pub fn time(&self) -> f64 {
        self.cycle() as f64 / self.freq as f64
    }

    pub fn cycle(&self) -> u64 {
        unsafe { ccavr_cycle(self.c) }
    }

    /// Why the chip stopped (it ran off its program, or crashed): None while it runs.
    pub fn stopped(&self) -> Option<&str> {
        self.stopped.as_deref()
    }

    /// Runs the chip until the time `t` (seconds). A chip that has stopped does not run on.
    pub fn run_to(&mut self, t: f64) {
        if self.stopped.is_some() {
            return;
        }
        let until = (t * self.freq as f64).round().max(0.0) as u64;
        match unsafe { ccavr_run_until(self.c, until) } {
            CPU_DONE => self.stopped = Some("プログラムが終わりました (main から戻りました)".into()),
            CPU_CRASHED => self.stopped = Some("プログラムが止まりました (無い番地を読んだ・スタックが溢れたなど)".into()),
            _ => {}
        }
    }

    pub fn ports(&self) -> Ports {
        let mut b = [0u8; 9];
        unsafe { ccavr_ports(self.c, b.as_mut_ptr()) };
        Ports { port: [b[0], b[3], b[6]], ddr: [b[1], b[4], b[7]], pin: [b[2], b[5], b[8]] }
    }

    /// The level the circuit puts on a pin (read by the program while the pin is an input).
    pub fn set_pin(&mut self, port: char, bit: u8, high: bool) {
        unsafe { ccavr_set_pin(self.c, port as c_char, bit as u32, high as u32) }
    }

    /// The voltage on an ADC input.
    pub fn set_adc(&mut self, channel: u8, volts: f64) {
        unsafe { ccavr_set_adc(self.c, channel as u32, (volts.max(0.0) * 1000.0).round() as u32) }
    }

    /// A byte to the program's serial port (UART0).
    pub fn send(&mut self, byte: u8) {
        unsafe { ccavr_send(self.c, byte) }
    }

    /// The pin changes since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<PinEvent> {
        let mut out = vec![];
        let mut buf = [CcEvent::default(); 1024];
        loop {
            let n = unsafe { ccavr_take_events(self.c, buf.as_mut_ptr(), buf.len() as u32) } as usize;
            out.extend(buf[..n].iter().map(|e| PinEvent { cycle: e.cycle, port: e.port as char, bit: e.bit, high: e.value != 0, timer: e.timer != 0 }));
            if n < buf.len() {
                return out;
            }
        }
    }

    /// The bytes the program sent on UART0 since the last call.
    pub fn take_serial(&mut self) -> Vec<u8> {
        let mut out = vec![];
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { ccavr_take_serial(self.c, buf.as_mut_ptr(), buf.len() as u32) } as usize;
            out.extend_from_slice(&buf[..n]);
            if n < buf.len() {
                return out;
            }
        }
    }
}

/// What goes into the chip: the flash from address 0, and the EEPROM's first bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub flash: Vec<u8>,
    pub eeprom: Vec<u8>,
}

/// avr-gcc puts the EEPROM at this address of its own (the flash is below 0x800000, the RAM from 0x800000).
const EEPROM_AT: u32 = 0x81_0000;
const RAM_AT: u32 = 0x80_0000;

/// The program of an ELF file from avr-gcc: each loadable segment at its load address (the `.data` that is copied to the RAM at
/// the start lies in the flash after the code), and the `.eeprom` segment.
pub fn read_elf(b: &[u8]) -> Result<Image, String> {
    let bad = |why: &str| format!("ELF のファイルが読めません: {why}");
    if b.len() < 52 || &b[..4] != b"\x7fELF" {
        return Err(bad("ELF ではありません"));
    }
    if b[4] != 1 || b[5] != 1 {
        return Err(bad("32 ビット・リトルエンディアンの ELF ではありません"));
    }
    let u16_at = |o: usize| b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
    let u32_at = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    if u16_at(18) != Some(83) {
        return Err(bad("AVR のプログラムではありません"));
    }
    let (phoff, phentsize, phnum) = (u32_at(28).unwrap() as usize, u16_at(42).unwrap() as usize, u16_at(44).unwrap() as usize);
    let mut img = Image::default();
    for i in 0..phnum {
        let h = phoff + i * phentsize;
        let (Some(kind), Some(off), Some(paddr), Some(filesz)) = (u32_at(h), u32_at(h + 4), u32_at(h + 12), u32_at(h + 16)) else {
            return Err(bad("プログラムヘッダが途中で切れています"));
        };
        if kind != 1 || filesz == 0 {
            continue;
        }
        let data = b.get(off as usize..(off + filesz) as usize).ok_or_else(|| bad("セグメントがファイルの外にあります"))?;
        let (to, at) = if paddr >= EEPROM_AT {
            (&mut img.eeprom, paddr - EEPROM_AT)
        } else if paddr >= RAM_AT {
            continue;
        } else {
            (&mut img.flash, paddr)
        };
        let end = at as usize + data.len();
        if to.len() < end {
            to.resize(end, 0xff);
        }
        to[at as usize..end].copy_from_slice(data);
    }
    if img.flash.is_empty() {
        return Err(bad("フラッシュに入れる中身がありません"));
    }
    Ok(img)
}

/// Arduino Nano (ATmega328P, 16 MHz, 5 V): its pins D0..D13, A0..A7.
pub mod nano {
    pub const MCU: &str = "atmega328p";
    pub const FREQ: u64 = 16_000_000;
    pub const VCC: f64 = 5.0;
    pub const PINS: usize = 22;

    /// The port and bit of a pin (A6 and A7 are only ADC inputs: None).
    pub fn port_bit(pin: u8) -> Option<(char, u8)> {
        match pin {
            0..=7 => Some(('D', pin)),
            8..=13 => Some(('B', pin - 8)),
            14..=19 => Some(('C', pin - 14)),
            _ => None,
        }
    }

    /// The ADC channel of a pin (A0..A7).
    pub fn adc(pin: u8) -> Option<u8> {
        (14..22).contains(&pin).then(|| pin - 14)
    }
}

/// What a pin does for the circuit over a step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinOut {
    /// An input (left to the circuit).
    Input,
    /// An input with the chip's pull-up.
    Pullup,
    /// An output at this share of the supply: 0 or 1, or the share of the step it was High (PWM, `tone`).
    Out(f64),
}

/// An Arduino Nano running its program beside the circuit.
pub struct Nano {
    pub avr: Avr,
    /// What the program sent on Serial (the oldest cut off past `SERIAL_MAX`).
    pub serial: String,
}

/// The most text the serial window keeps.
const SERIAL_MAX: usize = 20_000;

impl Nano {
    pub fn new(image: &Image) -> Result<Nano, String> {
        Ok(Nano { avr: Avr::new(nano::MCU, nano::FREQ, nano::VCC, image)?, serial: String::new() })
    }

    /// Runs the chip to the time `t` with the circuit's voltages on its pins now (`volts(pin)`: an input is High above half the
    /// supply; A0..A7 also go to the ADC), and gives what each pin D0..D13, A0..A7 does for the circuit: an output's level is
    /// the share of the step it was High.
    pub fn advance(&mut self, t: f64, volts: impl Fn(u8) -> f64) -> [PinOut; nano::PINS] {
        let before = self.avr.ports();
        for pin in 0..nano::PINS as u8 {
            let v = volts(pin);
            if let Some((p, b)) = nano::port_bit(pin)
                && !before.output(p, b)
            {
                self.avr.set_pin(p, b, v > self.avr.vcc / 2.0);
            }
            if let Some(ch) = nano::adc(pin) {
                self.avr.set_adc(ch, v);
            }
        }
        let start = self.avr.cycle();
        self.avr.run_to(t);
        let end = self.avr.cycle().max(start + 1);
        let events = self.avr.take_events();
        let text = self.avr.take_serial();
        if !text.is_empty() {
            self.serial.push_str(&String::from_utf8_lossy(&text));
            if self.serial.len() > SERIAL_MAX {
                let mut cut = self.serial.len() - SERIAL_MAX;
                while !self.serial.is_char_boundary(cut) {
                    cut += 1;
                }
                self.serial.drain(..cut);
            }
        }
        let after = self.avr.ports();
        let mut out = [PinOut::Input; nano::PINS];
        for (pin, o) in out.iter_mut().enumerate() {
            let Some((p, b)) = nano::port_bit(pin as u8) else { continue };
            *o = if after.output(p, b) {
                // the share of the step it was High, from its level at the start and its changes on the way
                let mut level = before.output(p, b) && before.high(p, b);
                let (mut at, mut high, mut changed) = (start, 0u64, false);
                // while a timer has the pin, only its changes count (simavr also passes on the writes of the port)
                let mine: Vec<&PinEvent> = events.iter().filter(|e| e.port == p && e.bit == b).collect();
                let timer = mine.iter().any(|e| e.timer);
                for e in mine.into_iter().filter(|e| e.timer || !timer) {
                    let c = e.cycle.clamp(start, end);
                    if level {
                        high += c - at;
                    }
                    at = c;
                    changed |= e.high != level;
                    level = e.high;
                }
                if level {
                    high += end - at;
                }
                if changed || timer { PinOut::Out(high as f64 / (end - start) as f64) } else { PinOut::Out(after.high(p, b) as u8 as f64) }
            } else if after.high(p, b) {
                PinOut::Pullup
            } else {
                PinOut::Input
            };
        }
        out
    }
}
