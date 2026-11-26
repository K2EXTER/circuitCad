#pragma once
// MSVC: gettimeofday and clock_gettime (simavr stamps its log with them)
#include <time.h>
#include <windows.h>
struct timeval { long tv_sec; long tv_usec; };
#ifndef CLOCK_MONOTONIC_RAW
#define CLOCK_MONOTONIC_RAW 4
#endif
static __inline int gettimeofday(struct timeval *tv, void *tz) {
	FILETIME ft; ULARGE_INTEGER u; (void)tz;
	GetSystemTimeAsFileTime(&ft); u.LowPart = ft.dwLowDateTime; u.HighPart = ft.dwHighDateTime;
	u.QuadPart = u.QuadPart / 10 - 11644473600000000ULL;
	tv->tv_sec = (long)(u.QuadPart / 1000000); tv->tv_usec = (long)(u.QuadPart % 1000000); return 0;
}
int clock_gettime(int clk, struct timespec * tp);
