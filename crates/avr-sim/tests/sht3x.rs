//! An SHT3x temperature and humidity sensor on the Nano's I2C bus (c/i2c_parts.c), read by a sketch with Wire alone
//! (tests/firmware/sht3x.ino → sht3x.elf, built by arduino-cli).

use avr_sim::{read_elf, Board, NANO};

const SHT3X: &[u8] = include_bytes!("firmware/sht3x.elf");

fn nano() -> Board {
    Board::new(&NANO, &read_elf(SHT3X).unwrap()).unwrap()
}

fn run(n: &mut Board, until: f64) {
    let mut t = n.avr.time();
    while t < until - 1e-9 {
        t += 1e-3;
        n.advance(t, |_| 5.0);
    }
}

#[test]
fn the_sketch_reads_the_temperature_and_humidity_the_sensor_is_given() {
    let mut n = nano();
    assert!(n.avr.add_sht3x(0x44));
    n.avr.set_sht3x(0x44, true, 23.4, 61.0);
    run(&mut n, 0.5);
    // too early, not acknowledged; the status after power on; then the readings, through the CRC
    assert!(n.serial.starts_with("early=0\r\nstatus=10\r\nt=23.4 rh=61.0\r\n"), "{:?}", n.serial);
    // the room warms and dries while it runs
    n.avr.set_sht3x(0x44, true, -5.2, 12.3);
    run(&mut n, 1.0);
    assert!(n.serial.trim_end().ends_with("t=-5.2 rh=12.3"), "{:?}", n.serial);
    assert!(n.avr.stopped().is_none());
}

#[test]
fn a_sensor_not_there_or_at_another_address_does_not_answer() {
    // nothing on the bus
    let mut n = nano();
    run(&mut n, 0.3);
    assert!(n.serial.contains("no sensor"), "{:?}", n.serial);
    // at 0x45 (ADDR high) the sketch asking 0x44 hears nothing
    let mut n = nano();
    n.avr.add_sht3x(0x45);
    n.avr.set_sht3x(0x45, true, 20.0, 50.0);
    run(&mut n, 0.3);
    assert!(n.serial.contains("no sensor") && !n.serial.contains("t="), "{:?}", n.serial);
    // unpowered (or its bus not pulled up): the same
    let mut n = nano();
    n.avr.add_sht3x(0x44);
    n.avr.set_sht3x(0x44, false, 20.0, 50.0);
    run(&mut n, 0.3);
    assert!(n.serial.contains("no sensor") && !n.serial.contains("t="), "{:?}", n.serial);
}
