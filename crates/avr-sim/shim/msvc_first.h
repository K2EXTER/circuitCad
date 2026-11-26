#pragma once
// Read before every C file when built with MSVC (/FI): the GCC-only bits simavr uses, and the POSIX calls of posix_shim.c.
#include <malloc.h>
#include <string.h>
#include <intrin.h>
#define __attribute__(x)
#define __sync_synchronize() _ReadWriteBarrier()
char * strsep(char ** s, const char * delim);
