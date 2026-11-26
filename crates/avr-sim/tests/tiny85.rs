//! An ATtiny85 program built by arduino-cli with ATTinyCore (tests/firmware/tiny85.ino → tiny85.elf) on avr-sim's ATtiny85:
//! its pins, its ADC on the pins ATTinyCore names A0..A3, its PWM, and its Serial sent in software on PB0.

use avr_sim::{read_elf, Board, PinOut, TINY85};

const TINY: &[u8] = include_bytes!("firmware/tiny85.elf");

#[test]
fn the_attiny85_runs_its_pins_adc_pwm_and_software_serial() {
    let mut n = Board::new(&TINY85, &read_elf(TINY).unwrap()).unwrap();
    assert_eq!(TINY85.adc(2), Some(1), "A1 is PB2");
    let mut t = 0.0;
    let mut flips = 0;
    let mut was = None;
    let mut last = vec![];
    while t < 0.45 {
        t += 1e-3;
        // PB2 (A1) at 2 V, then 4 V; PB3 held Low from 0.25 s
        last = n.advance(t, |p| match p {
            2 if t < 0.25 => 2.0,
            2 => 4.0,
            3 if t >= 0.25 => 0.0,
            _ => 5.0,
        });
        let now = matches!(last[1], PinOut::Out(x) if x > 0.5);
        if was.is_some_and(|w| w != now) {
            flips += 1;
        }
        was = Some(now);
    }
    assert!(n.avr.stopped().is_none(), "{:?}", n.avr.stopped());
    // PB1 every 10 ms (and the time the Serial lines take): 35 to 45 times in 0.45 s
    assert!((35..=45).contains(&flips), "PB1 turned over {flips} times");
    // PB4: analogWrite(4, 64) is a 25 % PWM
    match last[4] {
        PinOut::Pwm { duty, .. } => assert!((duty - 0.25).abs() < 0.02, "{duty}"),
        o => panic!("PB4 {o:?}"),
    }
    // the Serial lines, read back from PB0 at 9600 baud: 2 V is 409 of 1023, 4 V 818; PB3 pulled up, then Low
    let lines: Vec<&str> = n.serial.lines().collect();
    assert!(lines.len() >= 3, "{:?}", n.serial);
    assert!(["a1=409 b3=1", "a1=410 b3=1"].contains(&lines[0]), "{:?}", n.serial);
    assert!(lines.iter().any(|l| ["a1=818 b3=0", "a1=819 b3=0"].contains(&l.trim_end())), "{:?}", n.serial);
}
