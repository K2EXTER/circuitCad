// The ADC's conversion time (avr-sim's test the_adc_takes_as_long_as_the_datasheet_says): D13 (PB5) is High while a conversion
// runs (prescaler 128: 25 ADC clocks = 200 us for the first, 13 = 104 us after), D12 (PB4) High for 100 us between them.
#define F_CPU 16000000UL
#include <avr/io.h>
#include <util/delay.h>
int main(void) {
	DDRB = (1 << PB5) | (1 << PB4);
	ADMUX = (1 << REFS0);
	ADCSRA = (1 << ADEN) | (7 << ADPS0);
	for (;;) {
		PORTB |= (1 << PB5);
		ADCSRA |= (1 << ADSC);
		while (ADCSRA & (1 << ADSC)) {}
		PORTB &= ~(1 << PB5);
		PORTB |= (1 << PB4);
		_delay_us(100);
		PORTB &= ~(1 << PB4);
	}
}
