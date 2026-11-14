//! `ccad`: the command line of circuitCad without the window (the commands are in `lib.rs`). See `docs/cli.md`.

use cli::{mcp, runner, session};
use serde_json::Value;
use session::Session;
use std::io::Read;

const USAGE: &str = "ccad [--lib DIR] [--keep-going] [--strict] <script-file | - | -c \"line\\nline\">\n  ccad [--lib DIR] [--strict] mcp     serve the commands as an MCP server on standard input/output\n  ccad help                           list the commands\n  '-' reads the script from standard input";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut lib: Option<std::path::PathBuf> = None;
    let mut keep_going = false;
    let mut strict = false;
    let mut serve_mcp = false;
    let mut script: Option<String> = None;
    while !args.is_empty() {
        let a = args.remove(0);
        match a.as_str() {
            "--lib" if !args.is_empty() => lib = Some(args.remove(0).into()),
            "--keep-going" => keep_going = true,
            "--strict" => strict = true,
            "mcp" if script.is_none() => serve_mcp = true,
            "-c" if !args.is_empty() => script = Some(args.remove(0).replace(r"\n", "\n")),
            "-" => {
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() {
                    eprintln!("standard input could not be read");
                    std::process::exit(2);
                }
                script = Some(s);
            }
            "help" | "--help" | "-h" => {
                println!("{USAGE}\n\n{}", session::HELP);
                return;
            }
            f => match std::fs::read_to_string(f) {
                Ok(s) => script = Some(s),
                Err(e) => {
                    eprintln!("{f}: {e}\n{USAGE}");
                    std::process::exit(2);
                }
            },
        }
    }
    let mut session = Session::new(lib);
    session.strict = strict;
    if serve_mcp {
        mcp::serve(session);
        return;
    }
    let Some(script) = script else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let (answers, failed) = runner::run_script(&mut session, &script, keep_going);
    for a in answers {
        println!("{}", Value::Object(a));
    }
    if failed {
        std::process::exit(1);
    }
}
