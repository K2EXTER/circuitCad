//! The real firmware tests/firmware/check.elf (its source is check.c beside it) run on the Nano, the circuit's side played by the
//! test: the pins it drives, the input it follows, the ADC, the timer's PWM, the pull-up, the serial port and the EEPROM.

use avr_sim::{read_elf, Nano, PinOut};

const CHECK: &[u8] = include_bytes!("firmware/check.elf");
const D2: u8 = 2;
const D4: u8 = 4;
const D9: u8 = 9;
const D11: u8 = 11;
const D12: u8 = 12;
const D13: u8 = 13;
const A0: u8 = 14;

fn nano() -> Nano {
    Nano::new(&read_elf(CHECK).unwrap()).unwrap()
}

/// Runs to `until` in steps of 1 ms with the voltages `volts(t, pin)`, and gives the pins of each step.
fn run(n: &mut Nano, until: f64, volts: impl Fn(f64, u8) -> f64) -> Vec<(f64, [PinOut; 22])> {
    let mut out = vec![];
    let mut t = n.avr.time();
    while t < until - 1e-9 {
        t += 1e-3;
        let now = n.avr.time();
        out.push((t, n.advance(t, |p| volts(now, p))));
    }
    out
}

fn level(o: PinOut) -> f64 {
    match o {
        PinOut::Out(x) => x,
        _ => f64::NAN,
    }
}

#[test]
fn the_elf_gives_the_flash_and_the_eeprom() {
    let img = read_elf(CHECK).unwrap();
    assert!(img.flash.len() > 500, "{} bytes", img.flash.len());
    assert_eq!(img.eeprom.first(), Some(&42));
    assert!(read_elf(b"not an elf").unwrap_err().contains("ELF"));
}

#[test]
fn the_pins_follow_the_program() {
    let mut n = nano();
    let steps = run(&mut n, 0.2, |t, p| match p {
        D2 if t >= 0.1 => 5.0,
        A0 if t >= 0.1 => 4.0,
        A0 => 2.0,
        _ => 0.0,
    });
    // D13 is turned over every 10 iterations of about 1 ms: 18 to 20 times in 0.2 s
    let flips = steps.windows(2).filter(|w| (level(w[0].1[D13 as usize]) > 0.5) != (level(w[1].1[D13 as usize]) > 0.5)).count();
    assert!((17..=21).contains(&flips), "D13 turned over {flips} times");
    // D11 follows D2 and D12 the ADC (Low before 0.1 s, High after, within two iterations)
    let at = |t: f64, pin: u8| level(steps.iter().find(|s| s.0 >= t - 1e-9).unwrap().1[pin as usize]);
    assert_eq!((at(0.09, D11), at(0.09, D12)), (0.0, 0.0));
    assert_eq!((at(0.105, D11), at(0.105, D12)), (1.0, 1.0));
    // timer 1's PWM on D9: a quarter of each step High
    let pwm = at(0.15, D9);
    assert!((pwm - 0.25).abs() < 0.03, "D9 {pwm}");
    // D4: an input with its pull-up; D2: an input
    assert_eq!(steps[50].1[D4 as usize], PinOut::Pullup);
    assert_eq!(steps[50].1[D2 as usize], PinOut::Input);
}

#[test]
fn the_serial_port_and_the_eeprom_are_heard() {
    let mut n = nano();
    run(&mut n, 0.12, |_, p| if p == A0 { 2.0 } else { 0.0 });
    assert!(n.serial.starts_with("ee=42\n"), "{:?}", n.serial);
    // 2 V of 5 V: 409 of 1023
    assert!(n.serial.contains("a0=409\n") || n.serial.contains("a0=410\n"), "{:?}", n.serial);
    n.avr.send(b'x');
    run(&mut n, 0.13, |_, _| 0.0);
    assert!(n.serial.contains('x'), "{:?}", n.serial);
}
