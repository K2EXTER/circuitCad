// The libraries that come with Arduino's AVR core, interrupts and tone, on a Nano (W5 (5)): tests/libs.rs runs the ELF
// arduino-cli makes of this (libs.elf, `arduino-cli compile -b arduino:avr:nano --output-dir .`).
#include <EEPROM.h>
#include <SPI.h>
#include <SoftwareSerial.h>
#include <Wire.h>

SoftwareSerial soft(6, 7); // RX D6, TX D7
volatile unsigned presses = 0;

void pressed() { presses++; }

void setup() {
  Serial.begin(9600);
  soft.begin(9600);
  pinMode(2, INPUT_PULLUP);
  attachInterrupt(digitalPinToInterrupt(2), pressed, FALLING);
  tone(8, 1000);
  EEPROM.write(5, 0xA7);
  SPI.begin();
  byte spi = SPI.transfer(0x5A);
  Wire.begin();
  Wire.setWireTimeout(3000, true);
  Wire.beginTransmission(0x40);
  byte wire = Wire.endTransmission();
  Serial.print("ee=");
  Serial.print(EEPROM.read(5), HEX);
  Serial.print(" spi=");
  Serial.print(spi, HEX);
  Serial.print(" wire=");
  Serial.println(wire);
  soft.print("soft");
}

void loop() {
  static unsigned long last = 0;
  if (millis() - last >= 100) {
    last = millis();
    Serial.print("presses=");
    Serial.println(presses);
  }
}
