// An SHT3x read with Wire alone, on a Nano (W5 (5)): tests/sht3x.rs runs the ELF arduino-cli makes of this (sht3x.elf,
// `arduino-cli compile -b arduino:avr:nano --output-dir …`). The sensor sits at 0x44 (ADDR low).
#include <Wire.h>

const byte SHT = 0x44;

// CRC-8 of the sensor's words: polynomial 0x31 from 0xFF
byte crc8(const byte *d, byte n) {
  byte crc = 0xFF;
  for (byte i = 0; i < n; i++) {
    crc ^= d[i];
    for (byte b = 0; b < 8; b++) crc = (crc & 0x80) ? (crc << 1) ^ 0x31 : crc << 1;
  }
  return crc;
}

bool command(word c) {
  Wire.beginTransmission(SHT);
  Wire.write(c >> 8);
  Wire.write(c & 0xFF);
  return Wire.endTransmission() == 0;
}

// one measurement (high repeatability, 15 ms); false when the sensor does not answer or the data is damaged
bool readSht(float &t, float &rh) {
  if (!command(0x2400)) return false;
  delay(15);
  if (Wire.requestFrom(SHT, (byte)6) != 6) return false;
  byte d[6];
  for (byte i = 0; i < 6; i++) d[i] = Wire.read();
  if (crc8(d, 2) != d[2] || crc8(d + 3, 2) != d[5]) return false;
  t = -45 + 175.0 * ((word)d[0] << 8 | d[1]) / 65535.0;
  rh = 100.0 * ((word)d[3] << 8 | d[4]) / 65535.0;
  return true;
}

void setup() {
  Serial.begin(9600);
  Wire.begin();
  Wire.setWireTimeout(3000, true);
  // asked too early, the sensor does not answer: requestFrom gives 0 bytes
  command(0x2400);
  Serial.print("early=");
  Serial.println(Wire.requestFrom(SHT, (byte)6));
  delay(20);
  Wire.requestFrom(SHT, (byte)6);
  while (Wire.available()) Wire.read();
  // the status register: 0x0010 (a reset was seen) after power on
  command(0xF32D);
  Wire.requestFrom(SHT, (byte)3);
  word st = (word)Wire.read() << 8;
  st |= Wire.read();
  Serial.print("status=");
  Serial.println(st, HEX);
}

void loop() {
  float t, rh;
  if (readSht(t, rh)) {
    Serial.print("t=");
    Serial.print(t, 1);
    Serial.print(" rh=");
    Serial.println(rh, 1);
  } else {
    Serial.println("no sensor");
  }
  delay(200);
}
