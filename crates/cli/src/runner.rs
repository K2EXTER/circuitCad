//! Running script lines against a session: shared by the command line and the MCP server.

use crate::session::{split_words, Session};
use crate::Out;
use serde_json::json;

/// Run the lines of `script`. Each command gives one answer (`line`, `cmd`, `ok`, then the result or `error`).
/// Returns the answers and whether any command failed; stops at the first failure unless `keep_going`.
pub fn run_script(session: &mut Session, script: &str, keep_going: bool) -> (Vec<Out>, bool) {
    let mut answers = vec![];
    let mut failed = false;
    for (i, raw) in script.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words = split_words(line);
        let mut obj = Out::new();
        obj.insert("line".into(), json!(i + 1));
        obj.insert("cmd".into(), json!(words.first().cloned().unwrap_or_default()));
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
        answers.push(obj);
        if failed && !keep_going {
            break;
        }
    }
    (answers, failed)
}
