// The parts on the chip's I2C bus: see i2c_parts.h.
//
// SHT3x (Sensirion's data sheet "SHT3x-DIS", sections 4.3 to 4.12): 16-bit commands, written after the address; a measurement's
// result is six bytes (temperature, CRC, humidity, CRC) read after it is done. While a measurement runs, or with no result to
// give, the sensor does not acknowledge its address for reading, as the real one does.
#include <stdlib.h>
#include <string.h>
#include "i2c_parts.h"
#include "sim_irq.h"
#include "avr_twi.h"

#define MAX_PARTS 8

typedef struct {
	uint8_t addr;
	int present;
	double temp_c, rh;
	// the command being written (two bytes) and the bytes to send when read
	uint8_t cmd[2];
	int ncmd;
	uint8_t out[6];
	int nout, iout;
	// a single measurement's result ready at this cycle (has_result: one to give)
	int has_result;
	uint64_t ready;
	// periodic mode: a new result each `period` cycles from `next`
	int periodic;
	uint64_t period, next;
	uint16_t status;
	int heater;
} sht3x_t;

struct ccavr_i2c_t {
	avr_t * avr;
	avr_irq_t * in;
	sht3x_t sht[MAX_PARTS];
	int n;
	// the part the master addressed (-1: none), and whether it reads
	int selected;
	int reading;
};

// CRC-8, polynomial 0x31, from 0xFF (the data sheet's 4.12)
static uint8_t crc8(const uint8_t * d, int n) {
	uint8_t crc = 0xff;
	for (int i = 0; i < n; i++) {
		crc ^= d[i];
		for (int b = 0; b < 8; b++) crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x31) : (uint8_t)(crc << 1);
	}
	return crc;
}

static void put_word(uint8_t * out, uint16_t w) {
	out[0] = (uint8_t)(w >> 8);
	out[1] = (uint8_t)w;
	out[2] = crc8(out, 2);
}

static uint16_t clamp_word(double x) {
	if (x < 0) return 0;
	if (x > 65535) return 65535;
	return (uint16_t)(x + 0.5);
}

// the result of a measurement now: T = -45 + 175 * S / 65535, RH = 100 * S / 65535 (the data sheet's 4.13)
static void measure(sht3x_t * s) {
	double t = s->temp_c + (s->heater ? 3.0 : 0.0);
	put_word(s->out, clamp_word((t + 45.0) / 175.0 * 65535.0));
	put_word(s->out + 3, clamp_word(s->rh / 100.0 * 65535.0));
	s->nout = 6;
}

static uint64_t ms(avr_t * avr, double m) { return (uint64_t)(m * 1e-3 * avr->frequency); }

static void command(ccavr_i2c_t * b, sht3x_t * s) {
	avr_t * avr = b->avr;
	uint16_t c = (uint16_t)(s->cmd[0] << 8 | s->cmd[1]);
	uint8_t msb = s->cmd[0];
	s->nout = 0;
	switch (c) {
	// single shot, without clock stretching: high, medium, low repeatability (typical times, the data sheet's table 4)
	case 0x2400: s->has_result = 1; s->ready = avr->cycle + ms(avr, 12.5); return;
	case 0x240B: s->has_result = 1; s->ready = avr->cycle + ms(avr, 4.5); return;
	case 0x2416: s->has_result = 1; s->ready = avr->cycle + ms(avr, 2.5); return;
	// with clock stretching the sensor holds SCL until it is done: to the master the result is there when it reads
	case 0x2C06: case 0x2C0D: case 0x2C10: s->has_result = 1; s->ready = avr->cycle; return;
	// fetch data in periodic mode: the result goes out on the next read (none yet: the read is not acknowledged)
	case 0xE000:
		if (s->periodic && avr->cycle >= s->next) {
			measure(s);
			while (s->next <= avr->cycle) s->next += s->period;
		}
		return;
	case 0x3093: s->periodic = 0; return;                     // break (stop periodic mode)
	case 0x30A2: s->periodic = 0; s->heater = 0; s->has_result = 0; s->status = 0x0010; return; // soft reset
	case 0x306D: s->heater = 1; s->status |= 0x2000; return;  // heater on
	case 0x3066: s->heater = 0; s->status &= ~0x2000; return; // heater off
	case 0x3041: s->status = 0; return;                       // clear status
	// read status. Bits 6 to 9 say a periodic result is waiting to be fetched: Sensirion's own Arduino driver
	// (SensirionI2cSht3x::blockingReadMeasurement) waits for them, though the older data sheet calls them reserved
	case 0xF32D: put_word(s->out, (uint16_t)(s->status | (s->periodic && avr->cycle >= s->next ? 0x0040 : 0))); s->nout = 3; return;
	case 0x3780: case 0x3682: put_word(s->out, 0x1234); put_word(s->out + 3, 0x5678); return; // serial number
	case 0x2B32: msb = 0x23; break;                           // ART: periodic at 4 Hz
	}
	// periodic mode: 0.5, 1, 2, 4, 10 measurements a second (the second byte only sets the repeatability)
	double per = 0;
	switch (msb) {
	case 0x20: per = 2000; break;
	case 0x21: per = 1000; break;
	case 0x22: per = 500; break;
	case 0x23: per = 250; break;
	case 0x27: per = 100; break;
	}
	if (per > 0) {
		s->periodic = 1;
		s->has_result = 0;
		s->period = ms(avr, per);
		s->next = avr->cycle + ms(avr, 12.5);
	}
}

static void reply(ccavr_i2c_t * b, uint8_t msg, uint8_t addr, uint8_t data) {
	avr_raise_irq(b->in, avr_twi_irq_msg(msg, addr, data));
}

static void on_twi(struct avr_irq_t * irq, uint32_t value, void * param) {
	ccavr_i2c_t * b = param; (void)irq;
	avr_twi_msg_irq_t m;
	m.u.v = value;
	uint8_t msg = m.u.twi.msg, addr = m.u.twi.addr;
	if (msg & TWI_COND_STOP) b->selected = -1;
	if (msg & TWI_COND_START) {
		// the address byte: which part, and whether the master reads
		b->selected = -1;
		for (int i = 0; i < b->n; i++) {
			sht3x_t * s = &b->sht[i];
			if (!s->present || s->addr != (addr >> 1)) continue;
			if (addr & 1) {
				// a single measurement done: its result goes out now (and is gone once read)
				if (s->has_result && b->avr->cycle >= s->ready) {
					measure(s);
					s->has_result = 0;
				}
				if (s->nout == 0) return; // nothing to read: not acknowledged
				s->iout = 0;
			} else {
				s->ncmd = 0;
			}
			b->selected = i;
			b->reading = addr & 1;
			reply(b, TWI_COND_ACK, addr, 1);
			return;
		}
		return;
	}
	if (b->selected < 0) return;
	sht3x_t * s = &b->sht[b->selected];
	if ((msg & TWI_COND_WRITE) && !b->reading) {
		if (s->ncmd < 2) s->cmd[s->ncmd++] = m.u.twi.data;
		if (s->ncmd == 2) command(b, s);
		reply(b, TWI_COND_ACK, addr, 1);
	} else if ((msg & TWI_COND_READ) && b->reading) {
		uint8_t v = s->iout < s->nout ? s->out[s->iout++] : 0xff;
		// the bytes are given once: a read that stops early loses the rest, as on the real sensor
		if (s->iout >= s->nout || !(msg & TWI_COND_ACK)) s->nout = 0;
		reply(b, TWI_COND_READ, addr, v);
	}
}

ccavr_i2c_t * i2c_parts_new(avr_t * avr) {
	avr_irq_t * out = avr_io_getirq(avr, AVR_IOCTL_TWI_GETIRQ(0), TWI_IRQ_OUTPUT);
	avr_irq_t * in = avr_io_getirq(avr, AVR_IOCTL_TWI_GETIRQ(0), TWI_IRQ_INPUT);
	if (!out || !in) return NULL;
	ccavr_i2c_t * b = calloc(1, sizeof *b);
	b->avr = avr;
	b->in = in;
	b->selected = -1;
	avr_irq_register_notify(out, on_twi, b);
	return b;
}

void i2c_parts_free(ccavr_i2c_t * b) { free(b); }

int i2c_sht3x_add(ccavr_i2c_t * b, uint8_t addr) {
	if (!b || b->n >= MAX_PARTS) return 0;
	sht3x_t * s = &b->sht[b->n++];
	memset(s, 0, sizeof *s);
	s->addr = addr;
	s->temp_c = 25;
	s->rh = 50;
	s->status = 0x0010; // "reset detected": it has just been powered
	return 1;
}

void i2c_sht3x_set(ccavr_i2c_t * b, uint8_t addr, int present, double temp_c, double rh) {
	if (!b) return;
	for (int i = 0; i < b->n; i++)
		if (b->sht[i].addr == addr) {
			sht3x_t * s = &b->sht[i];
			// powered up again: it starts from its reset
			if (present && !s->present) {
				s->periodic = 0; s->has_result = 0; s->nout = 0; s->heater = 0; s->status = 0x0010;
			}
			s->present = present;
			s->temp_c = temp_c;
			s->rh = rh;
		}
}
