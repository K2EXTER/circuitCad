/* Read before every simavr file with GCC and Clang (-include): the copies put variable-length char arrays on the stack
   with MSVC's _alloca (tools/vendor_simavr.py), which is alloca here. */
#pragma once
#include <alloca.h>
#define _alloca alloca
