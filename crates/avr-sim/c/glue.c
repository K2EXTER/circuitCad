// A small C face over simavr for the Rust side (src/lib.rs): make a core, load its flash and EEPROM, run it to a cycle, read the
// ports, set an input pin and an ADC input, and take what happened on the way (the pins that changed, the bytes sent on UART0).
#include <stdlib.h>
#include <string.h>
#include "sim_avr.h"
#include "sim_irq.h"
#include "avr_ioport.h"
#include "avr_adc.h"
#include "avr_uart.h"
#include "avr_eeprom.h"
#include "avr_spi.h"

// one change of a pin: the cycle, the port's letter, the bit, the level, and whether a timer's compare output set it (simavr marks
// those with AVR_IOPORT_OUTPUT; a write of the port reaches the pin too, which the chip would not let through while the timer has it)
typedef struct { uint64_t cycle; uint8_t port; uint8_t bit; uint8_t value; uint8_t timer; } ccavr_event_t;

typedef struct ccavr_t ccavr_t;

// the ports a chip may have (the ATmega328 has B, C, D; the ATmega2560 all but I): a port the chip lacks has no IRQs and reads 0
#define NPORTS 11
static const char PORTS[NPORTS] = { 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L' };
typedef struct { ccavr_t * c; uint8_t port; uint8_t bit; } ccavr_pin_t;

struct ccavr_t {
	avr_t * avr;
	ccavr_event_t * events;
	uint32_t n_events, cap_events;
	uint8_t * serial;
	uint32_t n_serial, cap_serial;
	// set while we raise an input pin ourselves, so that our own change is not taken for the program's
	int driving;
	ccavr_pin_t pins[NPORTS * 8];
	// SPI: simavr moves the bytes, not the pins; the byte a master reads back is the MISO pin's level held through the transfer
	avr_irq_t * spi_in;
	uint8_t miso_bit;
};

static void grow(void ** p, uint32_t * cap, uint32_t need, size_t size) {
	if (need <= *cap) return;
	uint32_t n = *cap ? *cap * 2 : 256;
	while (n < need) n *= 2;
	*p = realloc(*p, n * size);
	*cap = n;
}

static void on_pin(struct avr_irq_t * irq, uint32_t value, void * param) {
	ccavr_pin_t * p = param; (void)irq;
	ccavr_t * c = p->c;
	if (c->driving) return;
	grow((void **)&c->events, &c->cap_events, c->n_events + 1, sizeof(ccavr_event_t));
	ccavr_event_t * e = &c->events[c->n_events++];
	e->cycle = c->avr->cycle; e->port = p->port; e->bit = p->bit; e->value = (value & 0xff) ? 1 : 0; e->timer = (value & AVR_IOPORT_OUTPUT) ? 1 : 0;
}

// the master sent a byte: what it reads in the same 8 clocks is the MISO pin (port B) as the circuit holds it, all ones or all
// zeros (left alone, simavr would leave the byte sent in SPDR, as if MISO were wired to MOSI)
static void on_spi(struct avr_irq_t * irq, uint32_t value, void * param) {
	ccavr_t * c = param; (void)irq; (void)value;
	avr_ioport_state_t st;
	if (avr_ioctl(c->avr, AVR_IOCTL_IOPORT_GETSTATE('B'), &st) != 0) return;
	avr_raise_irq(c->spi_in, ((st.pin >> c->miso_bit) & 1) ? 0xff : 0x00);
}

static void on_byte(struct avr_irq_t * irq, uint32_t value, void * param) {
	ccavr_t * c = param; (void)irq;
	grow((void **)&c->serial, &c->cap_serial, c->n_serial + 1, 1);
	c->serial[c->n_serial++] = (uint8_t)value;
}

// a sleeping core (SLEEP) would wait in real time: here it only skips the cycles
static void no_wait(avr_t * avr, avr_cycle_count_t how_long) { (void)avr; (void)how_long; }

ccavr_t * ccavr_new(const char * mcu, uint32_t freq, uint32_t vcc_mv) {
	avr_t * avr = avr_make_mcu_by_name(mcu);
	if (!avr) return NULL;
	avr_init(avr);
	avr->frequency = freq;
	avr->vcc = avr->avcc = avr->aref = vcc_mv;
	avr->log = 0;
	avr->sleep = no_wait;
	ccavr_t * c = calloc(1, sizeof *c);
	c->avr = avr;
	for (int k = 0; k < NPORTS; k++)
		for (int b = 0; b < 8; b++) {
			ccavr_pin_t * p = &c->pins[k * 8 + b];
			p->c = c; p->port = (uint8_t)PORTS[k]; p->bit = (uint8_t)b;
			avr_irq_t * irq = avr_io_getirq(avr, AVR_IOCTL_IOPORT_GETIRQ(PORTS[k]), b);
			if (irq) avr_irq_register_notify(irq, on_pin, p);
		}
	// the bytes of Serial come to us, not to the console; and a program polling the UART is not slowed to real time
	uint32_t f = 0;
	if (avr_ioctl(avr, AVR_IOCTL_UART_GET_FLAGS('0'), &f) == 0) {
		f &= ~(AVR_UART_FLAG_STDIO | AVR_UART_FLAG_POLL_SLEEP);
		avr_ioctl(avr, AVR_IOCTL_UART_SET_FLAGS('0'), &f);
	}
	avr_irq_t * out = avr_io_getirq(avr, AVR_IOCTL_UART_GETIRQ('0'), UART_IRQ_OUTPUT);
	if (out) avr_irq_register_notify(out, on_byte, c);
	// MISO is PB3 on the ATmega2560, PB4 on the ATmega328
	c->miso_bit = strcmp(mcu, "atmega2560") == 0 ? 3 : 4;
	c->spi_in = avr_io_getirq(avr, AVR_IOCTL_SPI_GETIRQ(0), SPI_IRQ_INPUT);
	avr_irq_t * spi_out = avr_io_getirq(avr, AVR_IOCTL_SPI_GETIRQ(0), SPI_IRQ_OUTPUT);
	if (spi_out && c->spi_in) avr_irq_register_notify(spi_out, on_spi, c);
	return c;
}

void ccavr_free(ccavr_t * c) {
	if (!c) return;
	avr_terminate(c->avr);
	free(c->avr);
	free(c->events);
	free(c->serial);
	free(c);
}

// the program's flash from address 0, and the EEPROM's first bytes (none when size is 0)
void ccavr_load(ccavr_t * c, const uint8_t * flash, uint32_t size, const uint8_t * eeprom, uint32_t esize) {
	avr_t * avr = c->avr;
	avr_loadcode(avr, (uint8_t *)flash, size, 0);
	avr->codeend = size;
	avr->pc = 0;
	if (esize) {
		avr_eeprom_desc_t d = { .ee = (uint8_t *)eeprom, .offset = 0, .size = esize };
		avr_ioctl(avr, AVR_IOCTL_EEPROM_SET, &d);
	}
}

// runs until the cycle count reaches `until` or the core stops (it returns its state then: cpu_Done, cpu_Crashed)
int ccavr_run_until(ccavr_t * c, uint64_t until) {
	avr_t * avr = c->avr;
	int st = avr->state;
	while (avr->cycle < until) {
		st = avr_run(avr);
		if (st == cpu_Done || st == cpu_Crashed) break;
	}
	return st;
}

uint64_t ccavr_cycle(ccavr_t * c) { return c->avr->cycle; }

// PORTx, DDRx and PINx of each of the ports A..L (without I), in that order (33 bytes; 0 for a port the chip lacks)
void ccavr_ports(ccavr_t * c, uint8_t * out) {
	for (int k = 0; k < NPORTS; k++) {
		avr_ioport_state_t s;
		memset(&s, 0, sizeof s);
		avr_ioctl(c->avr, AVR_IOCTL_IOPORT_GETSTATE(PORTS[k]), &s);
		out[k * 3] = (uint8_t)s.port; out[k * 3 + 1] = (uint8_t)s.ddr; out[k * 3 + 2] = (uint8_t)s.pin;
	}
}

// the level the circuit puts on an input pin
void ccavr_set_pin(ccavr_t * c, char port, uint32_t bit, uint32_t value) {
	avr_irq_t * irq = avr_io_getirq(c->avr, AVR_IOCTL_IOPORT_GETIRQ(port), bit);
	if (!irq) return;
	c->driving = 1;
	avr_raise_irq(irq, value ? 1 : 0);
	c->driving = 0;
}

// the voltage on an ADC input (millivolts)
void ccavr_set_adc(ccavr_t * c, uint32_t channel, uint32_t mv) {
	avr_irq_t * irq = avr_io_getirq(c->avr, AVR_IOCTL_ADC_GETIRQ, ADC_IRQ_ADC0 + channel);
	if (irq) avr_raise_irq(irq, mv);
}

// a byte to the program's UART0 (what the serial monitor sends)
void ccavr_send(ccavr_t * c, uint8_t byte) {
	avr_irq_t * irq = avr_io_getirq(c->avr, AVR_IOCTL_UART_GETIRQ('0'), UART_IRQ_INPUT);
	if (irq) avr_raise_irq(irq, byte);
}

// the pin changes since the last call (up to `max`; the rest stay for the next call), oldest first
uint32_t ccavr_take_events(ccavr_t * c, ccavr_event_t * out, uint32_t max) {
	uint32_t n = c->n_events < max ? c->n_events : max;
	memcpy(out, c->events, n * sizeof(ccavr_event_t));
	memmove(c->events, c->events + n, (c->n_events - n) * sizeof(ccavr_event_t));
	c->n_events -= n;
	return n;
}

uint32_t ccavr_take_serial(ccavr_t * c, uint8_t * out, uint32_t max) {
	uint32_t n = c->n_serial < max ? c->n_serial : max;
	memcpy(out, c->serial, n);
	memmove(c->serial, c->serial + n, c->n_serial - n);
	c->n_serial -= n;
	return n;
}
