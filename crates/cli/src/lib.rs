//! `ccad`: circuitCad without the window. The commands live in this library; `main.rs` reads the arguments and prints the
//! answers, and the tests run scripts through `runner::run_script` directly (no process to start for every script).
//!
//! A script is one command per line (`#` starts a comment). Every command answers with one JSON object on its own
//! line: `{"line":3,"cmd":"add","ok":true,...}` or `{"line":3,"cmd":"net","ok":false,"error":"..."}`.
//! The first failing command stops the run (exit code 1) unless `--keep-going` is given.
//! `ccad mcp` serves the same commands as a Model Context Protocol server. See `docs/cli.md`.

mod board;
mod build;
mod design;
mod fitcmd;
mod libcmd;
pub mod mcp;
mod models;
mod reqcmd;
pub mod runner;
mod sheetcmd;
pub mod session;

use serde_json::{Map, Value};

pub type Out = Map<String, Value>;
pub type Res = Result<Out, String>;
