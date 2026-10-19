//! `ccad`: circuitCad without the window.
//!
//! A script is one command per line (`#` starts a comment). Every command answers with one JSON object on its own
//! line: `{"line":3,"cmd":"add","ok":true,...}` or `{"line":3,"cmd":"net","ok":false,"error":"..."}`.
//! The first failing command stops the run (exit code 1) unless `--keep-going` is given.
//! See `docs/cli.md`.

mod board;
mod build;
mod session;

use serde_json::{json, Map, Value};
use session::Session;
use std::io::Read;

pub type Out = Map<String, Value>;
pub type Res = Result<Out, String>;

const USAGE: &str = "ccad [--lib DIR] [--keep-going] [--strict] <script-file | - | -c \"line\\nline\">\n  ccad help        list the commands\n  '-' reads the script from standard input";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut lib: Option<std::path::PathBuf> = None;
    let mut keep_going = false;
    let mut strict = false;
    let mut script: Option<String> = None;
    while !args.is_empty() {
        let a = args.remove(0);
        match a.as_str() {
            "--lib" if !args.is_empty() => lib = Some(args.remove(0).into()),
            "--keep-going" => keep_going = true,
            "--strict" => strict = true,
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
    let Some(script) = script else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let mut session = Session::new(lib);
    session.strict = strict;
    let mut failed = false;
    for (i, raw) in script.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words = session::split_words(line);
        let cmd = words.first().cloned().unwrap_or_default();
        let mut obj = Out::new();
        obj.insert("line".into(), json!(i + 1));
        obj.insert("cmd".into(), json!(cmd));
        match session.run(&words) {
            Ok(r) => {
                obj.insert("ok".into(), json!(true));
                obj.extend(r);
            }
            Err(e) => {
                obj.insert("ok".into(), json!(false));
                obj.insert("error".into(), json!(e));
                failed = true;
            }
        }
        println!("{}", Value::Object(obj));
        if failed && !keep_going {
            break;
        }
    }
    if failed {
        std::process::exit(1);
    }
}
