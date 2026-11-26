//! Arduino's Servo library (tests/firmware/servo.ino → servo.elf): its pulses on D9 every 20 ms, as wide as the angle asks
//! (544 us at 0 degrees, 2400 us at 180), made by timer 1's interrupt, and what the circuit is given for them (the pulses).

use avr_sim::{read_elf, Avr, Board, PinOut, NANO};

const SERVO: &[u8] = include_bytes!("firmware/servo.elf");

/// The High pulses on D9 (PB1) from `from` to `to` s: (start, width) in seconds.
fn pulses(from: f64, to: f64) -> Vec<(f64, f64)> {
    let mut a = Avr::new(NANO.mcu, NANO.freq, NANO.vcc, &read_elf(SERVO).unwrap()).unwrap();
    a.run_to(to);
    let mut out = vec![];
    let mut rise = None;
    for e in a.take_events().into_iter().filter(|e| e.port == 'B' && e.bit == 1) {
        let t = e.cycle as f64 / NANO.freq as f64;
        match (e.high, rise) {
            (true, None) => rise = Some(t),
            (false, Some(r)) => {
                if r >= from {
                    out.push((r, t - r));
                }
                rise = None;
            }
            _ => {}
        }
    }
    out
}

#[test]
fn the_servo_library_sends_its_pulses_every_20_ms_as_wide_as_the_angle() {
    let p = pulses(0.0, 0.9);
    // 20 ms apart
    for w in p.windows(2) {
        assert!((w[1].0 - w[0].0 - 0.020).abs() < 0.0005, "{w:?}");
    }
    // the width the library makes of each angle (it counts the timer in 0.5 us ticks, a few us for its own instructions)
    let width_at = |t: f64| p.iter().find(|(s, _)| *s > t).unwrap().1 * 1e6;
    for (t, us) in [(0.15, 544.0), (0.45, 1472.0), (0.75, 2400.0)] {
        let w = width_at(t);
        assert!((w - us).abs() < 15.0, "at {t} s: {w} us, not {us}");
    }
}

#[test]
fn the_circuit_is_given_the_servo_pulses() {
    // 50 Hz pulses reach the circuit as pulses (a 1 ms step would catch them only now and then)
    let mut n = Board::new(&NANO, &read_elf(SERVO).unwrap()).unwrap();
    let mut t = 0.0;
    let mut last = vec![];
    while t < 0.25 {
        t += 1e-3;
        last = n.advance(t, |_| 5.0);
    }
    match last[9] {
        PinOut::Pwm { duty, hz } => assert!((hz - 50.0).abs() < 1.0 && (duty - 0.0272).abs() < 0.002, "{duty} {hz}"),
        o => panic!("D9 {o:?}"),
    }
}
