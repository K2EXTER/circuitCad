#pragma once
// MSVC: ffs from strings.h
#include <string.h>
#include <intrin.h>
static __inline int ffs(int x) { unsigned long i; return _BitScanForward(&i, (unsigned long)x) ? (int)i + 1 : 0; }
