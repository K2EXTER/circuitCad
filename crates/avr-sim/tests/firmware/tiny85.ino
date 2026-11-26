// An ATtiny85 with ATTinyCore (W5 (3)): tests/tiny85.rs runs the ELF arduino-cli makes of this (tiny85.elf,
// `arduino-cli compile -b ATTinyCore:avr:attinyx5:chip=85,clock=8internal --output-dir …`).
// PB1 turns over every 10 ms, PB4 is a 25 % PWM, A1 (PB2) and PB3 (pulled up) are read and sent on Serial (software, PB0).
void setup() {
  Serial.begin(9600);
  pinMode(1, OUTPUT);
  pinMode(3, INPUT_PULLUP);
  analogWrite(4, 64);
}

void loop() {
  static byte n = 0;
  digitalWrite(1, !digitalRead(1));
  delay(10);
  if (++n == 10) {
    n = 0;
    Serial.print("a1=");
    Serial.print(analogRead(A1));
    Serial.print(" b3=");
    Serial.println(digitalRead(3));
  }
}
