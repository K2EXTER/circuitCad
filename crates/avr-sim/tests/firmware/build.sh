#!/bin/sh
# Builds check.elf from check.c (avr-gcc from the Arduino IDE, or any avr-gcc on the PATH).
set -e
cd "$(dirname "$0")"
GCC=${AVR_GCC:-avr-gcc}
"$GCC" -mmcu=atmega328p -Os -Wall -o check.elf check.c
