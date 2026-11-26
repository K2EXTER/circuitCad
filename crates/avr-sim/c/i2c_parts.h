// The parts on the chip's I2C bus (TWI), answering the program as the real ones do (W5 (5)): the Sensirion SHT3x temperature and
// humidity sensor. simavr's TWI tells the bus's events as messages (start with the address, a byte written, a byte read, stop);
// a part answers with an acknowledge or the byte it sends.
#pragma once
#include <stdint.h>
#include "sim_avr.h"

typedef struct ccavr_i2c_t ccavr_i2c_t;

// listens on the chip's TWI (NULL when the chip has none)
ccavr_i2c_t * i2c_parts_new(avr_t * avr);
void i2c_parts_free(ccavr_i2c_t * b);

// an SHT3x at the 7-bit address `addr` (0x44 or 0x45); 0 when there is no room for more parts
int i2c_sht3x_add(ccavr_i2c_t * b, uint8_t addr);
// what it measures, and whether it answers at all (powered, its bus wired and pulled up)
void i2c_sht3x_set(ccavr_i2c_t * b, uint8_t addr, int present, double temp_c, double rh);
