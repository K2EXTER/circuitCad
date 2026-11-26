//! Builds the bundled simavr (simavr/: the simulator core and the ATmega328 and ATmega2560 cores, copied by tools/vendor_simavr.py) and the C glue
//! (c/glue.c) into a static library. With MSVC the GCC-only bits and the POSIX calls come from shim/ (read first with /FI); with GCC and Clang only `_alloca` (shim/gcc_first.h, read first with -include).

fn main() {
    let sim = std::path::Path::new("simavr/sim");
    let cores = std::path::Path::new("simavr/cores");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut b = cc::Build::new();
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(sim)
        .expect("simavr/sim")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "c"))
        .collect();
    files.sort();
    b.files(&files);
    b.file(cores.join("sim_mega328.c")).file(cores.join("sim_megax8.c")).file(cores.join("sim_mega2560.c"));
    b.file("c/glue.c").file("c/i2c_parts.c").file("c/gdb_stub.c");
    if msvc {
        b.file("c/posix_shim.c");
        b.include("shim").flag("/FImsvc_first.h").flag("/std:c11");
        b.define("_CRT_SECURE_NO_WARNINGS", None).define("WIN32_LEAN_AND_MEAN", None).define("NOMINMAX", None);
    } else {
        b.flag_if_supported("-std=gnu99");
        b.flag("-include").flag("shim/gcc_first.h");
    }
    b.include("c").include(sim).include(cores);
    // simavr's own warnings are not ours to fix
    b.warnings(false);
    b.compile("simavr");
    println!("cargo:rerun-if-changed=c");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=simavr");
}
