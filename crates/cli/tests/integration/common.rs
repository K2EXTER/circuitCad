//! Running a script the way `ccad --lib DIR [--keep-going] [--strict] -` does, but inside the test (no process to start: starting
//! `ccad` for every script took most of the time of these tests). The binary itself is still run by `mcp` and by
//! `script::the_binary_reads_standard_input_and_exits_1_on_a_failure`.

use serde_json::Value;

#[derive(Clone, Copy, Default)]
pub struct Flags {
    pub keep_going: bool,
    pub strict: bool,
}

/// The library read once for every script that does not write parts: the bundled parts in a folder shared by the test runs
/// (installing them and reading them again took about 0.13 s per script, more than many of the scripts themselves).
fn shared_library() -> &'static (std::path::PathBuf, Vec<part_lib::PartDef>, Vec<String>) {
    static LIB: std::sync::OnceLock<(std::path::PathBuf, Vec<part_lib::PartDef>, Vec<String>)> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        // emptied first: the folder outlives the run, and a bundled part already installed there is never written again, so a
        // part changed since the last run would be read in its old version
        let dir = std::env::temp_dir().join("ccad-test-shared-lib");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = part_lib::install_examples(&dir);
        let (lib, errors) = part_lib::load_library(&dir);
        (dir, lib, errors)
    })
}

/// The answers of the script (one JSON object per line, as `ccad` prints them) and whether every line went through (the exit
/// code 0). A script that writes parts (`part import`, then maybe `lib reload`) gets a library folder of its own (the tests run
/// side by side, and a part written by one must not be read half-written by another), removed afterwards; the others share
/// `shared_library`. Each run has a thread of its own (the simulation options are per thread, and a script may change them).
pub fn ccad(script: &str, flags: Flags) -> (Vec<Value>, bool) {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let lower = script.to_ascii_lowercase();
    let own = (lower.contains("part import") || lower.contains("lib reload"))
        .then(|| std::env::temp_dir().join(format!("ccad-test-lib-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst))));
    let (answers, failed) = std::thread::scope(|s| {
        s.spawn(|| {
            let mut session = match &own {
                Some(dir) => cli::session::Session::new(Some(dir.clone())),
                None => {
                    let (dir, lib, errors) = shared_library();
                    cli::session::Session::with_library(dir.clone(), lib.clone(), errors.clone())
                }
            };
            session.strict = flags.strict;
            cli::runner::run_script(&mut session, script, flags.keep_going)
        })
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
    });
    if let Some(dir) = own {
        let _ = std::fs::remove_dir_all(&dir);
    }
    (answers.into_iter().map(Value::Object).collect(), !failed)
}
