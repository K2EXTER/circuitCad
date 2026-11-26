// The Servo library (Arduino's, 1.3.0) on a Nano (W5 (5)): tests/servo.rs runs the ELF arduino-cli makes of this (servo.elf).
// The servo on D9 goes to 0, 90 and 180 degrees, 300 ms each; its pulses come every 20 ms, 544 to 2400 us wide.
#include <Servo.h>

Servo s;

void setup() {
  s.attach(9);
}

void loop() {
  s.write(0);
  delay(300);
  s.write(90);
  delay(300);
  s.write(180);
  delay(300);
}
