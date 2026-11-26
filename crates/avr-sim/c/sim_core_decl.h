// What simavr's Makefile would generate: the table of cores looked up by name.
#ifndef __SIM_CORE_DECL_H__
#define __SIM_CORE_DECL_H__
#include "sim_core_config.h"
extern avr_kind_t mega328;
extern avr_kind_t mega2560;
extern avr_kind_t * avr_kind[];
#ifdef AVR_KIND_DECL
avr_kind_t * avr_kind[] = { &mega328, &mega2560, NULL };
#endif
#endif
