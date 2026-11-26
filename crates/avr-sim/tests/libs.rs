//! A sketch arduino-cli built with the libraries that come with Arduino's AVR core (tests/firmware/libs.ino → libs.elf) runs on
//! the Nano as on the real board: EEPROM, SPI, Wire with nothing on the bus, SoftwareSerial, attachInterrupt, tone, millis.

use avr_sim::{read_elf, Avr, Board, PinOut, NANO};

const LIBS: &[u8] = include_bytes!("firmware/libs.elf");

/// Runs to `until` in steps of 1 ms, the pins at `volts(t, pin)`; gives the pins of the last step.
fn run(n: &mut Board, until: f64, volts: impl Fn(f64, u8) -> f64) -> Vec<PinOut> {
    let mut last = vec![];
    let mut t = n.avr.time();
    while t < until - 1e-9 {
        t += 1e-3;
        let now = n.avr.time();
        last = n.advance(t, |p| volts(now, p));
    }
    last
}

#[test]
fn the_libraries_interrupts_and_tone_run_as_on_the_board() {
    let mut n = Board::new(&NANO, &read_elf(LIBS).unwrap()).unwrap();
    // D2 pressed (to GND) twice: 0.30–0.35 s and 0.40–0.45 s
    let pressed = |t: f64| (0.30..0.35).contains(&t) || (0.40..0.45).contains(&t);
    let pins = run(&mut n, 0.7, |t, p| if p == 2 && pressed(t) { 0.0 } else { 5.0 });
    eprintln!("{}", n.serial);
    assert!(n.avr.stopped().is_none(), "{:?}", n.avr.stopped());
    // EEPROM written and read back; SPI reads MISO (D12, held at 5 V) as 0xFF; Wire with no device answering: 2 (the address
    // not acknowledged)
    assert!(n.serial.starts_with("ee=A7 spi=FF wire=2\r\n"), "{:?}", n.serial);
    // millis keeps time (a line each 100 ms) and the interrupt counted each press once
    assert!(n.serial.contains("presses=0\r\npresses=1\r\npresses=2\r\n"), "{:?}", n.serial);
    assert!(n.serial.trim_end().ends_with("presses=2"), "{:?}", n.serial);
    // tone(8, 1000): a 1 kHz square wave on D8
    match pins[8] {
        PinOut::Pwm { duty, hz } => assert!((duty - 0.5).abs() < 0.02 && (hz - 1000.0).abs() < 5.0, "{duty} {hz}"),
        o => panic!("D8 {o:?}"),
    }
}

#[test]
fn software_serial_sends_its_bits_on_its_pin() {
    let mut a = Avr::new(NANO.mcu, NANO.freq, NANO.vcc, &read_elf(LIBS).unwrap()).unwrap();
    // D2 held high (its pull-up): no presses
    a.set_pin('D', 2, true);
    a.run_to(0.1);
    // D7 = PD7: the changes of the soft serial's TX, read at 9600 baud (8N1, idle high)
    let edges: Vec<(f64, bool)> = a
        .take_events()
        .into_iter()
        .filter(|e| e.port == 'D' && e.bit == 7)
        .map(|e| (e.cycle as f64 / NANO.freq as f64, e.high))
        .collect();
    let level = |t: f64| edges.iter().take_while(|e| e.0 <= t).last().is_none_or(|e| e.1);
    let bit = 1.0 / 9600.0;
    let mut text = String::new();
    let mut i = 0;
    while let Some(&(start, _)) = edges[i..].iter().find(|e| !e.1) {
        let byte = (0..8).fold(0u8, |b, k| b | (u8::from(level(start + (1.5 + k as f64) * bit)) << k));
        text.push(byte as char);
        let end = start + 9.5 * bit;
        i = edges.iter().position(|e| e.0 > end).unwrap_or(edges.len());
    }
    assert_eq!(text, "soft");
}
