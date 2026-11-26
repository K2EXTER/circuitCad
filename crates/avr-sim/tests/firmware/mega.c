// The firmware avr-sim's Mega test runs (tests/nano.rs: the_mega_runs_its_own_pins): pins on several ports by Arduino's numbers.
// D13 (PB7) toggles every 10 ms; D22 (PA0) follows D2 (PE4); D53 (PB0) is High while A8 (PK0, ADC8) reads above 512; D11 (PB5,
// OC1A) is a 25 % PWM of 1 kHz from timer 1; Serial (USART0) at 115200 sends "mega" once, then A8's reading every 50 ms.
#define F_CPU 16000000UL
#include <avr/io.h>
#include <util/delay.h>
#include <stdio.h>

static int put(char c, FILE * f) { (void)f; while (!(UCSR0A & (1 << UDRE0))) {} UDR0 = c; return 0; }
static FILE out = FDEV_SETUP_STREAM(put, NULL, _FDEV_SETUP_WRITE);

int main(void) {
	DDRB = (1 << PB7) | (1 << PB5) | (1 << PB0);
	DDRA = (1 << PA0);
	ADMUX = (1 << REFS0);                 // channel 8: MUX4:0 = 0 with MUX5 set
	ADCSRB = (1 << MUX5);
	ADCSRA = (1 << ADEN) | (7 << ADPS0);
	ICR1 = 1999; OCR1A = 500;
	TCCR1A = (1 << COM1A1) | (1 << WGM11);
	TCCR1B = (1 << WGM13) | (1 << WGM12) | (1 << CS11);
	UBRR0 = 8; UCSR0A = 0; UCSR0B = (1 << TXEN0); UCSR0C = (3 << UCSZ00);
	stdout = &out;
	printf("mega\n");
	for (uint16_t n = 0;; n++) {
		if (n % 10 == 0) PORTB ^= (1 << PB7);
		if (PINE & (1 << PE4)) PORTA |= (1 << PA0); else PORTA &= ~(1 << PA0);
		ADCSRA |= (1 << ADSC);
		while (ADCSRA & (1 << ADSC)) {}
		uint16_t a = ADC;
		if (a > 512) PORTB |= (1 << PB0); else PORTB &= ~(1 << PB0);
		if (n % 50 == 0) printf("a8=%u\n", a);
		_delay_us(890);
	}
}
