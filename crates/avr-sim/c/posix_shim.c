// MSVC: strsep and clock_gettime, which simavr takes from the C library on Linux and macOS.
#include <string.h>
#include <time.h>
#include <windows.h>
char * strsep(char ** s, const char * delim) {
	char * start = *s;
	if (!start) return NULL;
	char * end = start + strcspn(start, delim);
	if (*end) { *end = 0; *s = end + 1; } else *s = NULL;
	return start;
}
int clock_gettime(int clk, struct timespec * tp) {
	LARGE_INTEGER f, c; (void)clk;
	QueryPerformanceFrequency(&f); QueryPerformanceCounter(&c);
	tp->tv_sec = (time_t)(c.QuadPart / f.QuadPart);
	tp->tv_nsec = (long)((c.QuadPart % f.QuadPart) * 1000000000LL / f.QuadPart);
	return 0;
}
