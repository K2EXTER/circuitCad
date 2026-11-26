#pragma once
// MSVC: the one call simavr takes from unistd.h
#include <windows.h>
static __inline int usleep(unsigned us) { Sleep(us / 1000); return 0; }
