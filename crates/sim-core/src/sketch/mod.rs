//! An Arduino sketch run beside the circuit (candidate B6 ⑤): a small part of Arduino's C (variables and arrays, `if`, loops,
//! functions), with `pinMode`, `digitalWrite`, `digitalRead`, `analogRead`, `analogWrite`, `delay`, `millis` and `Serial.print`.
//! The sketch is compiled to a stack machine (`vm.rs`) that stops at `delay` and goes on when the simulated time has come; the
//! circuit is solved in between. The pins are given to the circuit through `Io`: an output is a voltage (a fraction of the
//! supply), an input is left to the circuit and read as a voltage. Not the instructions of the real chip (that is W5 (b)): the time
//! the sketch takes between two `delay`s is none. `analogWrite` gives the pulses of the real board (`Drive::Pwm`: 490 Hz, 980 Hz on
//! D5 and D6), so an LED on it lights for that share of the time.

mod lex;
mod parse;
mod vm;

use vm::{Bi, Code, Op, Val};

/// The supply of the board's pins (volts).
pub const VCC: f64 = 5.0;

/// The microcontroller board a sketch runs on: its pins by Arduino's numbers (D0.., then A0..), the ones `analogWrite` pulses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Board {
    /// Arduino Nano (ATmega328P): D0..D13, A0..A7.
    #[default]
    Nano,
    /// Arduino Mega 2560 (ATmega2560): D0..D53, A0..A15.
    Mega,
}

impl Board {
    pub const ALL: [Board; 2] = [Board::Nano, Board::Mega];

    /// The part's model in the library (`ARDUINO_NANO`).
    pub fn model(self) -> &'static str {
        match self {
            Board::Nano => "ARDUINO_NANO",
            Board::Mega => "ARDUINO_MEGA",
        }
    }

    pub fn from_model(name: &str) -> Option<Board> {
        Board::ALL.into_iter().find(|b| b.model() == name)
    }

    /// arduino-cli's name of the board (W5 (b)).
    pub fn fqbn(self) -> &'static str {
        match self {
            Board::Nano => "arduino:avr:nano",
            Board::Mega => "arduino:avr:mega",
        }
    }

    /// The number of the first analog pin.
    pub fn a0(self) -> u8 {
        match self {
            Board::Nano => 14,
            Board::Mega => 54,
        }
    }

    /// How many analog pins (A0..).
    pub fn analog_pins(self) -> u8 {
        match self {
            Board::Nano => 8,
            Board::Mega => 16,
        }
    }

    /// The number of pins: D0.., then A0...
    pub fn pins(self) -> usize {
        (self.a0() + self.analog_pins()) as usize
    }

    /// The name of a pin: D13, A0.
    pub fn pin_name(self, pin: u8) -> String {
        if pin < self.a0() { format!("D{pin}") } else { format!("A{}", pin - self.a0()) }
    }

    /// The pins of the I2C bus (Wire): SDA and SCL (A4 and A5 on the Nano, D20 and D21 on the Mega).
    pub fn i2c_pins(self) -> (u8, u8) {
        match self {
            Board::Nano => (18, 19),
            Board::Mega => (20, 21),
        }
    }

    /// The pins `analogWrite` gives pulses on (the others go High from 128).
    pub fn pwm_pins(self) -> &'static [u8] {
        match self {
            Board::Nano => &[3, 5, 6, 9, 10, 11],
            Board::Mega => &[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 44, 45, 46],
        }
    }

    /// The frequency of `analogWrite`'s pulses on a pin: timer 0 (Nano D5, D6; Mega D4, D13) runs twice as fast as the others.
    pub fn pwm_hz(self, pin: u8) -> f64 {
        let fast = match self {
            Board::Nano => pin == 5 || pin == 6,
            Board::Mega => pin == 4 || pin == 13,
        };
        if fast { 980.0 } else { 490.0 }
    }
}
/// Instructions one call of `run` may take before it gives the circuit its turn (a loop without `delay` goes on next frame).
const BUDGET: usize = 200_000;
/// The most text the serial window keeps.
const SERIAL_MAX: usize = 20_000;


/// What the sketch does with a pin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drive {
    /// Left to the circuit (`INPUT`, and every pin at the start).
    Input,
    /// An input with the chip's pull-up resistor to the supply (`INPUT_PULLUP`).
    Pullup,
    /// An output at this share of the supply (0 = Low, 1 = High).
    Out(f64),
    /// An output switching between Low and High: High for `duty` of each period, `hz` times a second (`analogWrite`, `tone`).
    Pwm { duty: f64, hz: f64 },
}

impl Drive {
    /// The share of the supply on average (a PWM's duty).
    pub fn average(self) -> f64 {
        match self {
            Drive::Out(x) => x,
            Drive::Pwm { duty, .. } => duty,
            Drive::Pullup => 1.0,
            Drive::Input => 0.0,
        }
    }
}

/// The circuit as the sketch sees it.
pub trait Io {
    /// The voltage on a pin now.
    fn volts(&self, pin: u8) -> f64;
    /// A pin changes what it does.
    fn drive(&mut self, pin: u8, d: Drive);
}

#[derive(Clone, Debug, PartialEq)]
pub struct SketchError {
    pub line: u32,
    pub message: String,
}

impl std::fmt::Display for SketchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} 行目: {}", self.line, self.message)
    }
}

/// Compiles a sketch for a board (its pin numbers: `A0` is 14 on the Nano, 54 on the Mega).
pub fn compile(src: &str, board: Board) -> Result<Sketch, SketchError> {
    let err = |(line, message): (u32, String)| SketchError { line, message };
    let toks = lex::lex(src).map_err(err)?;
    let prog = parse::parse(toks).map_err(err)?;
    let code = vm::compile(&prog, board).map_err(err)?;
    Ok(Sketch { code, board })
}

pub struct Sketch {
    code: Code,
    board: Board,
}

struct Frame {
    ret: usize,
    base: usize,
}

/// A sketch running: where it is, its variables, the pins, the serial output.
pub struct Machine {
    sketch: Sketch,
    pc: usize,
    stack: Vec<Val>,
    globals: Vec<Val>,
    locals: Vec<Val>,
    frames: Vec<Frame>,
    /// It waits in `delay` until this simulated time.
    wake: f64,
    pub drives: Vec<Drive>,
    /// What the sketch wrote with `Serial.print` (the oldest cut off past `SERIAL_MAX`).
    pub serial: String,
    pub error: Option<SketchError>,
    seed: u64,
}

impl Machine {
    pub fn new(sketch: Sketch) -> Machine {
        let (g, pins) = (sketch.code.globals, sketch.board.pins());
        Machine { sketch, pc: 0, stack: vec![], globals: vec![Val::I(0); g], locals: vec![], frames: vec![], wake: 0.0, drives: vec![Drive::Input; pins], serial: String::new(), error: None, seed: 0x2545_F491_4F6C_DD1D }
    }

    /// When the sketch is waiting in `delay` until (simulated seconds).
    pub fn waiting_until(&self) -> f64 {
        self.wake
    }

    /// Runs the sketch at the simulated time `now` until it waits in `delay` (or its instructions for this turn are spent, or it
    /// fails: `error`).
    pub fn run(&mut self, now: f64, io: &mut dyn Io) {
        if self.error.is_some() || now < self.wake {
            return;
        }
        for _ in 0..BUDGET {
            match self.step(now, io) {
                Ok(true) => {}
                Ok(false) => return,
                Err(message) => {
                    let line = self.sketch.code.lines.get(self.pc.saturating_sub(1)).copied().unwrap_or(0);
                    self.error = Some(SketchError { line, message });
                    return;
                }
            }
        }
    }

    fn pop(&mut self) -> Val {
        self.stack.pop().unwrap_or(Val::I(0))
    }

    fn pin(&self, v: &Val) -> Result<u8, String> {
        let p = vm::val_i(v);
        let b = self.sketch.board;
        if (0..b.pins() as i64).contains(&p) {
            Ok(p as u8)
        } else {
            Err(format!("ピン {p} はありません (0〜{} と A0〜A{})", b.a0() - 1, b.analog_pins() - 1))
        }
    }

    fn set(&mut self, pin: u8, d: Drive, io: &mut dyn Io) {
        if self.drives[pin as usize] != d {
            self.drives[pin as usize] = d;
            io.drive(pin, d);
        }
    }

    fn print(&mut self, s: &str) {
        self.serial.push_str(s);
        if self.serial.len() > SERIAL_MAX {
            let cut = self.serial.len() - SERIAL_MAX;
            let at = (cut..self.serial.len()).find(|i| self.serial.is_char_boundary(*i)).unwrap_or(cut);
            self.serial.drain(..at);
        }
    }

    /// One instruction; Ok(false) when it is waiting in `delay` now.
    fn step(&mut self, now: f64, io: &mut dyn Io) -> Result<bool, String> {
        let op = self.sketch.code.ops.get(self.pc).cloned().ok_or("スケッチの終わりに来ました")?;
        self.pc += 1;
        match op {
            Op::Push(v) => self.stack.push(v),
            Op::Load(v) => {
                let (global, i, _, _) = vm::place(&v);
                let base = self.frames.last().map_or(0, |f| f.base);
                let x = if global { self.globals[i].clone() } else { self.locals[base + i].clone() };
                self.stack.push(x);
            }
            Op::Store(v) => {
                let (global, i, ty, _) = vm::place(&v);
                let x = vm::store(ty, self.pop());
                let base = self.frames.last().map_or(0, |f| f.base);
                if global { self.globals[i] = x.clone() } else { self.locals[base + i] = x.clone() }
                self.stack.push(x);
            }
            Op::LoadIdx(v) | Op::StoreIdx(v) => {
                let store = matches!(op, Op::StoreIdx(_));
                let value = if store { Some(self.pop()) } else { None };
                let (global, i, ty, len) = vm::place(&v);
                let k = vm::val_i(&self.pop());
                let n = len.unwrap_or(1) as i64;
                if !(0..n).contains(&k) {
                    return Err(format!("配列の番号 {k} が範囲 (0〜{}) の外です", n - 1));
                }
                let base = self.frames.last().map_or(0, |f| f.base);
                let at = i + k as usize;
                match value {
                    Some(x) => {
                        let x = vm::store(ty, x);
                        if global { self.globals[at] = x.clone() } else { self.locals[base + at] = x.clone() }
                        self.stack.push(x);
                    }
                    None => {
                        let x = if global { self.globals[at].clone() } else { self.locals[base + at].clone() };
                        self.stack.push(x);
                    }
                }
            }
            Op::Bin(o) => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(vm::binary(o, &a, &b)?);
            }
            Op::Un(o) => {
                let a = self.pop();
                self.stack.push(vm::unary(o, &a));
            }
            Op::Pop => {
                self.pop();
            }
            Op::Dup => {
                let a = self.stack.last().cloned().unwrap_or(Val::I(0));
                self.stack.push(a);
            }
            Op::Jmp(t) => self.pc = t,
            Op::Jz(t) => {
                if !vm::truthy(&self.pop()) {
                    self.pc = t;
                }
            }
            Op::Call(f, argc) => {
                if self.frames.len() > 200 {
                    return Err("関数の呼び出しが深すぎます (自分を呼び続けていませんか)".into());
                }
                let code = &self.sketch.code;
                let base = self.locals.len();
                let args: Vec<Val> = self.stack.split_off(self.stack.len().saturating_sub(argc));
                self.locals.resize(base + code.slots_of(f).max(code.params_of(f)), Val::I(0));
                for (k, a) in args.into_iter().enumerate() {
                    self.locals[base + k] = vm::store(code.param_ty(f, k), a);
                }
                self.frames.push(Frame { ret: self.pc, base });
                self.pc = code.entry(f);
            }
            Op::Ret => {
                let v = self.pop();
                let fr = self.frames.pop().ok_or("return の戻り先がありません")?;
                self.locals.truncate(fr.base);
                self.pc = fr.ret;
                self.stack.push(v);
            }
            Op::Builtin(b, argc) => {
                let args: Vec<Val> = self.stack.split_off(self.stack.len().saturating_sub(argc));
                let a = |k: usize| args.get(k).cloned().unwrap_or(Val::I(0));
                let r = match b {
                    Bi::PinMode => {
                        let pin = self.pin(&a(0))?;
                        let d = match vm::val_i(&a(1)) {
                            1 => Drive::Out(0.0),
                            2 => Drive::Pullup,
                            _ => Drive::Input,
                        };
                        self.set(pin, d, io);
                        Val::I(0)
                    }
                    Bi::DigitalWrite => {
                        let pin = self.pin(&a(0))?;
                        let high = vm::truthy(&a(1));
                        let d = match self.drives[pin as usize] {
                            Drive::Out(_) | Drive::Pwm { .. } => Drive::Out(if high { 1.0 } else { 0.0 }),
                            // a write to an input turns its pull-up on or off, as on the real chip
                            _ => if high { Drive::Pullup } else { Drive::Input },
                        };
                        self.set(pin, d, io);
                        Val::I(0)
                    }
                    Bi::DigitalRead => {
                        let pin = self.pin(&a(0))?;
                        Val::I((io.volts(pin) > VCC / 2.0) as i64)
                    }
                    Bi::AnalogRead => {
                        let mut pin = self.pin(&a(0))?;
                        // analogRead(0) means A0
                        let b = self.sketch.board;
                        if pin < b.analog_pins() && (vm::val_i(&a(0)) as u8) < b.analog_pins() {
                            pin += b.a0();
                        }
                        Val::I(((io.volts(pin) / VCC * 1023.0).round() as i64).clamp(0, 1023))
                    }
                    Bi::AnalogWrite => {
                        let pin = self.pin(&a(0))?;
                        let v = vm::val_i(&a(1)).clamp(0, 255);
                        // as the real board: 0 and 255 are a steady Low and High, the rest pulses on the PWM pins
                        let d = match v {
                            0 => Drive::Out(0.0),
                            255 => Drive::Out(1.0),
                            _ if self.sketch.board.pwm_pins().contains(&pin) => Drive::Pwm { duty: v as f64 / 255.0, hz: self.sketch.board.pwm_hz(pin) },
                            _ => Drive::Out(if v >= 128 { 1.0 } else { 0.0 }),
                        };
                        self.set(pin, d, io);
                        Val::I(0)
                    }
                    Bi::Delay | Bi::DelayUs => {
                        let t = vm::val_f(&a(0)).max(0.0) * if b == Bi::Delay { 1e-3 } else { 1e-6 };
                        self.wake = now + t;
                        self.stack.push(Val::I(0));
                        return Ok(t <= 0.0);
                    }
                    Bi::Millis => Val::I((now * 1e3) as i64),
                    Bi::Micros => Val::I((now * 1e6) as i64),
                    Bi::SerialBegin => Val::I(0),
                    Bi::SerialPrint | Bi::SerialPrintln => {
                        let text = match (args.first(), args.get(1).map(vm::val_i)) {
                            (None, _) => String::new(),
                            (Some(v @ Val::F(f)), Some(d)) => {
                                let _ = v;
                                format!("{f:.*}", d.clamp(0, 9) as usize)
                            }
                            (Some(Val::I(i)), Some(16)) => format!("{i:X}"),
                            (Some(Val::I(i)), Some(2)) => format!("{i:b}"),
                            (Some(v), _) => v.text(),
                        };
                        self.print(&text);
                        if b == Bi::SerialPrintln {
                            self.print("\n");
                        }
                        Val::I(0)
                    }
                    Bi::Map => {
                        let (x, il, ih, ol, oh) = (vm::val_i(&a(0)), vm::val_i(&a(1)), vm::val_i(&a(2)), vm::val_i(&a(3)), vm::val_i(&a(4)));
                        if ih == il {
                            return Err("map の入力の範囲の両端が同じです".into());
                        }
                        Val::I((x - il) * (oh - ol) / (ih - il) + ol)
                    }
                    Bi::Constrain => {
                        let (x, lo, hi) = (a(0), a(1), a(2));
                        if vm::val_f(&x) < vm::val_f(&lo) { lo } else if vm::val_f(&x) > vm::val_f(&hi) { hi } else { x }
                    }
                    Bi::Abs => match a(0) {
                        Val::F(f) => Val::F(f.abs()),
                        v => Val::I(vm::val_i(&v).abs()),
                    },
                    Bi::Min | Bi::Max => {
                        let (x, y) = (a(0), a(1));
                        let first = (vm::val_f(&x) < vm::val_f(&y)) == (b == Bi::Min);
                        if first { x } else { y }
                    }
                    Bi::Random => {
                        self.seed ^= self.seed << 13;
                        self.seed ^= self.seed >> 7;
                        self.seed ^= self.seed << 17;
                        let (lo, hi) = if argc == 2 { (vm::val_i(&a(0)), vm::val_i(&a(1))) } else { (0, vm::val_i(&a(0))) };
                        if hi <= lo { Val::I(lo) } else { Val::I(lo + (self.seed % (hi - lo) as u64) as i64) }
                    }
                    Bi::ToInt => Val::I(vm::val_i(&a(0))),
                    Bi::ToFloat => Val::F(vm::val_f(&a(0))),
                };
                self.stack.push(r);
            }
        }
        // keep the stack from growing without end in a buggy sketch
        if self.stack.len() > 10_000 {
            return Err("計算の途中の値がたまりすぎました".into());
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circuit of nothing but the pins: what is driven is read back; A0 is a fixed voltage.
    #[derive(Default)]
    struct Pins {
        out: std::collections::BTreeMap<u8, Drive>,
        a0: f64,
        log: Vec<(u8, Drive)>,
    }

    impl Io for Pins {
        fn volts(&self, pin: u8) -> f64 {
            if pin == 14 {
                return self.a0;
            }
            self.out.get(&pin).map_or(0.0, |d| d.average() * VCC)
        }
        fn drive(&mut self, pin: u8, d: Drive) {
            self.out.insert(pin, d);
            self.log.push((pin, d));
        }
    }

    fn machine(src: &str) -> Machine {
        Machine::new(compile(src, Board::Nano).unwrap_or_else(|e| panic!("{e}")))
    }

    #[test]
    fn blink_toggles_the_led_every_half_second() {
        let mut m = machine("void setup() { pinMode(LED_BUILTIN, OUTPUT); }\nvoid loop() {\n  digitalWrite(13, HIGH);\n  delay(500);\n  digitalWrite(13, LOW);\n  delay(500);\n}");
        let mut io = Pins::default();
        let mut seen = vec![];
        for k in 0..8 {
            let now = k as f64 * 0.25;
            m.run(now, &mut io);
            seen.push(io.volts(13) > 2.5);
        }
        assert_eq!(seen, [true, true, false, false, true, true, false, false]);
        assert!(m.error.is_none());
    }

    #[test]
    fn arithmetic_loops_arrays_functions_and_serial() {
        let src = r#"
            #define N 5
            int pins[] = {2, 3, 4};
            float total = 0;
            int square(int x) { return x * x; }
            void setup() {
              Serial.begin(9600);
              for (int i = 0; i < N; i++) { total += square(i); }
              Serial.println(total);
              int k = 7 / 2;
              Serial.print(k); Serial.print(" "); Serial.println(7 % 3);
              int n = 0;
              while (true) { n++; if (n >= 10) break; }
              Serial.println(n > 5 ? n : -1);
              for (int i = 0; i < 3; i++) { pinMode(pins[i], OUTPUT); digitalWrite(pins[i], i % 2 == 0); }
              Serial.println(map(512, 0, 1023, 0, 255));
            }
            void loop() { delay(1000); }
        "#;
        let mut m = machine(src);
        let mut io = Pins::default();
        m.run(0.0, &mut io);
        assert!(m.error.is_none(), "{:?}", m.error);
        assert_eq!(m.serial, "30.00\n3 1\n10\n127\n");
        assert_eq!((io.volts(2), io.volts(3), io.volts(4)), (5.0, 0.0, 5.0));
    }

    #[test]
    fn analog_read_and_write() {
        let mut m = machine("void setup() { pinMode(9, OUTPUT); }\nvoid loop() { int v = analogRead(A0); analogWrite(9, v / 4); delay(10); }");
        let mut io = Pins { a0: 2.5, ..Default::default() };
        m.run(0.0, &mut io);
        // 2.5 V of 5: 512 of 1023, 128 of 255: pulses of 490 Hz, High half the time
        match io.out.get(&9).copied() {
            Some(Drive::Pwm { duty, hz }) => assert!((duty - 0.5).abs() < 0.01 && hz == 490.0, "{duty} {hz}"),
            d => panic!("{d:?}"),
        }
    }

    /// The Mega's own numbers: A8 is pin 62, D44 pulses with `analogWrite`, D53 is a pin and D70 is not.
    #[test]
    fn the_mega_has_its_own_pins() {
        let b = Board::Mega;
        assert_eq!((b.pins(), b.a0(), b.pin_name(62), b.pin_name(53)), (70, 54, "A8".to_string(), "D53".to_string()));
        let mut m = Machine::new(compile("void setup() { pinMode(53, OUTPUT); digitalWrite(53, HIGH); analogWrite(44, 64); Serial.println(A8); }\nvoid loop() { delay(10); }", b).unwrap());
        let mut io = Pins::default();
        m.run(0.0, &mut io);
        assert!(m.error.is_none(), "{:?}", m.error);
        assert_eq!(m.serial, "62\n");
        assert_eq!(m.drives[53], Drive::Out(1.0));
        assert!(matches!(m.drives[44], Drive::Pwm { hz, .. } if hz == 490.0), "{:?}", m.drives[44]);
        let e = Machine::new(compile("void setup() { pinMode(70, OUTPUT); }\nvoid loop() {}", b).unwrap());
        let mut e = e;
        e.run(0.0, &mut io);
        assert!(e.error.as_ref().is_some_and(|x| x.message.contains("0〜53")), "{:?}", e.error);
    }

    #[test]
    fn mistakes_say_where_and_why() {
        let e = compile("void setup() { pinMode(13, OUTPUT) }\nvoid loop() {}", Board::Nano).err().unwrap();
        assert_eq!(e.line, 1);
        assert!(e.message.contains(";"), "{e}");
        let e = compile("void setup() { x = 1; }\nvoid loop() {}", Board::Nano).err().unwrap();
        assert!(e.message.contains("変数 x"), "{e}");
        let e = compile("void setup() {}", Board::Nano).err().unwrap();
        assert!(e.message.contains("loop"), "{e}");
        let e = compile("void setup() {}\nvoid loop() {\n  digitalWrit(13, HIGH);\n}", Board::Nano).err().unwrap();
        assert_eq!(e.line, 3);
        // at run time: a pin that is not there, with its line
        let mut m = machine("void setup() {}\nvoid loop() {\n  pinMode(40, OUTPUT);\n}");
        m.run(0.0, &mut Pins::default());
        let e = m.error.unwrap();
        assert_eq!(e.line, 3);
        assert!(e.message.contains("40"));
    }

    #[test]
    fn a_loop_without_delay_gives_the_circuit_its_turn() {
        let mut m = machine("int n = 0;\nvoid setup() {}\nvoid loop() { n++; }");
        m.run(0.0, &mut Pins::default());
        assert!(m.error.is_none());
    }
}
