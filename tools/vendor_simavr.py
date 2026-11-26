"""Copy the part of simavr that crates/avr-sim builds (the simulator core `sim/` and the ATmega328 core) out of a simavr checkout, and
rewrite the few GCC-only bits so that MSVC compiles it too (gcc and clang take the rewritten text the same).

    git clone --depth 1 https://github.com/buserror/simavr.git <dir>
    python tools/vendor_simavr.py <dir>

What is rewritten (docs/code-notes/avr-sim.md): `case A ... B:` ranges become lists of cases, `0b11` becomes 3, the
named variadic macro `STATE(_f, args...)` becomes `STATE(...)`, and variable-length char arrays go on the stack with `_alloca`.
`__attribute__`, `__sync_synchronize` and the POSIX calls are left to crates/avr-sim/shim (MSVC only). The files are written with
LF endings. The version copied is written to crates/avr-sim/simavr/VERSION.

Fixes of simavr's own (`FIXES`, each must still be found in the file copied; drop one that upstream has fixed):
- the ADC's conversion time: `current_prescale` was a uint8_t, so the prescaler of 128 (the one Arduino uses) times 2 wrapped to 0
  and the conversion ended as it was sampled (16 us instead of 104 us); and the end was 11 times the sampling delay after it. Now
  it holds the ADC clock and a conversion takes 13 ADC clocks (25 for the first), as the datasheet says.
"""
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "avr-sim" / "simavr"

# sim/ files left out: the gdb server (sockets), the ELF and DWARF readers (libelf: the ELF is read in Rust), the command-line
# program, USB (not on the ATmega328)
SKIP = {"sim_gdb.c", "sim_elf.c", "sim_dwarf.c", "run_avr.c", "avr_usb.c", "avr_usb.h"}
CORES = ["sim_mega328.c", "sim_megax8.c", "sim_megax8.h", "sim_core_declare.h"]
AVR_HEADERS = ["iom328p.h"]

# (file, text, what it becomes)
FIXES = [
    ("avr_adc.h", "        uint8_t                 current_prescale;",
     "        uint32_t                current_prescale;	// circuitCad: the ADC clock in CPU cycles (was uint8_t: 256 wrapped to 0)"),
    ("avr_adc.c", """		div = (1 << div);
		div *= (p->first ? 14 : 2);	// first conversion is longer
                p->current_prescale = div;
                avr_cycle_timer_register(avr, div, avr_adc_convert, p);""",
     """		div = (1 << div);
		// circuitCad: sampled after 2 ADC clocks (14 for the first conversion), done 11 later: 13 and 25 as the datasheet says
                p->current_prescale = div;
                avr_cycle_timer_register(avr, div * (p->first ? 14 : 2), avr_adc_convert, p);"""),
]


def value(tok):
    tok = tok.strip()
    if re.fullmatch(r"'.'", tok):
        return ord(tok[1]), lambda v: repr(chr(v))
    if re.fullmatch(r"0x[0-9a-fA-F]+|\d+", tok):
        return int(tok, 0), hex
    m = re.fullmatch(r"([A-Za-z_]+?)(\d+)", tok)
    if m:
        p = m.group(1)
        return int(m.group(2)), lambda v, p=p: f"{p}{v}"
    raise ValueError(f"a case range end I cannot read: {tok}")


def cases(m):
    (a, fmt), (b, _) = value(m.group(1)), value(m.group(2))
    return " ".join(f"case {fmt(v)}:" for v in range(a, b + 1))


def rewrite(text):
    t = re.sub(r"case\s+([^:\s]+)\s*\.\.\.\s*([^:\s]+)\s*:", cases, text)
    t = re.sub(r"#define (\w+)\(_f, args\.\.\.\)", r"#define \1(...)", t)
    t = re.sub(r"\bchar (\w+)\[([a-z_]\w*\s*\+\s*\d+|\d+\s*\+\s*[a-z_]\w*\s*\+\s*\d+)\];", r"char * \1 = _alloca(\2);", t)
    t = re.sub(r"\b0b([01]+)\b", lambda m: str(int(m.group(1), 2)), t)
    return t


def copy(src, dst, changed):
    text = src.read_text(encoding="utf-8").replace("\r\n", "\n")
    new = rewrite(text) if src.suffix == ".c" else text
    n = sum(1 for a, b in zip(text.splitlines(), new.splitlines()) if a != b)
    if n:
        changed.append((src.name, n))
    for name, old, fixed in FIXES:
        if name == src.name:
            if new.count(old) != 1:
                sys.exit(f"the fix of {name} no longer fits (upstream changed it?): look at FIXES in {__file__}")
            new = new.replace(old, fixed)
            changed.append((f"{src.name} (fix)", old.count("\n") + 1))
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(new, encoding="utf-8", newline="\n")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    src = pathlib.Path(sys.argv[1]) / "simavr"
    if not (src / "sim" / "sim_avr.c").is_file():
        sys.exit(f"not a simavr checkout: {sys.argv[1]}")
    if OUT.exists():
        shutil.rmtree(OUT)
    changed = []
    for f in sorted((src / "sim").glob("*.[ch]")):
        if f.name not in SKIP:
            copy(f, OUT / "sim" / f.name, changed)
    copy(src / "sim" / "avr" / "avr_mcu_section.h", OUT / "sim" / "avr" / "avr_mcu_section.h", changed)
    for f in CORES:
        copy(src / "cores" / f, OUT / "cores" / f, changed)
    for f in AVR_HEADERS:
        copy(src / "cores" / "avr" / f, OUT / "cores" / "avr" / f, changed)
    shutil.copy(src.parent / "COPYING", OUT / "COPYING")
    rev = subprocess.run(["git", "-C", str(src.parent), "log", "-1", "--format=%H %cs"], capture_output=True, text=True).stdout.strip()
    (OUT / "VERSION").write_text(f"https://github.com/buserror/simavr {rev}\n", encoding="utf-8", newline="\n")
    print(f"copied simavr {rev} to {OUT.relative_to(ROOT)}")
    for name, n in changed:
        print(f"  rewritten for MSVC: {name} ({n} lines)")


if __name__ == "__main__":
    main()
