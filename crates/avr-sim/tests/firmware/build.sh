#!/bin/sh
# Builds check.elf from check.c and adc_time.elf from adc_time.c (avr-gcc from the Arduino IDE, or any avr-gcc on the PATH).
set -e
cd "$(dirname "$0")"
GCC=${AVR_GCC:-avr-gcc}
"$GCC" -mmcu=atmega328p -Os -Wall -o check.elf check.c
"$GCC" -mmcu=atmega328p -Os -Wall -o adc_time.elf adc_time.c
