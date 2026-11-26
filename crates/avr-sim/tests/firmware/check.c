// The firmware the tests of avr-sim run (built with build.sh; the .elf is kept beside it, so the tests need no avr-gcc).
// D13 (PB5) toggles every 10 ms; D11 (PB3) follows D2 (PD2); D12 (PB4) is High while A0 reads above 512; D9 (PB1) is a 25 %
// PWM from timer 1; D4 (PD4) is an input with its pull-up; UART0 at 115200 sends "ee=<EEPROM byte 0>" once, then the A0 reading
// every 50 ms; a byte received is sent back.
#define F_CPU 16000000UL
#include <avr/io.h>
#include <avr/eeprom.h>
#include <util/delay.h>
#include <stdio.h>

uint8_t EEMEM first = 42;

static int put(char c, FILE * f) { (void)f; while (!(UCSR0A & (1 << UDRE0))) {} UDR0 = c; return 0; }
static FILE out = FDEV_SETUP_STREAM(put, NULL, _FDEV_SETUP_WRITE);

int main(void) {
	DDRB = (1 << PB5) | (1 << PB4) | (1 << PB3) | (1 << PB1);
	PORTD = (1 << PD4);
	ADMUX = (1 << REFS0);
	ADCSRA = (1 << ADEN) | (7 << ADPS0);
	// timer 1: fast PWM, TOP = ICR1 = 1999 (1 kHz), OC1A at 500 (25 %)
	ICR1 = 1999; OCR1A = 500;
	TCCR1A = (1 << COM1A1) | (1 << WGM11);
	TCCR1B = (1 << WGM13) | (1 << WGM12) | (1 << CS11);
	UBRR0 = 8; UCSR0A = 0; UCSR0B = (1 << TXEN0) | (1 << RXEN0); UCSR0C = (3 << UCSZ00);
	stdout = &out;
	printf("ee=%u\n", eeprom_read_byte(&first));
	for (uint16_t n = 0;; n++) {
		if (n % 10 == 0) PORTB ^= (1 << PB5);
		if (PIND & (1 << PD2)) PORTB |= (1 << PB3); else PORTB &= ~(1 << PB3);
		ADCSRA |= (1 << ADSC);
		while (ADCSRA & (1 << ADSC)) {}
		uint16_t a = ADC;
		if (a > 512) PORTB |= (1 << PB4); else PORTB &= ~(1 << PB4);
		if (n % 50 == 0) printf("a0=%u\n", a);
		if (UCSR0A & (1 << RXC0)) put(UDR0, 0);
		_delay_us(900);
	}
}
