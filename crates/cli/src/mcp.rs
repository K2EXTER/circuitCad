//! `ccad mcp`: a Model Context Protocol server on standard input/output.
//!
//! Messages are JSON-RPC 2.0, one per line. The session (schematic, board) lives as long as the server, so an
//! agent can build a circuit one call at a time and look at the answers in between.

use crate::runner::run_script;
use crate::session::{Session, HELP};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

const CONVENTIONS: &str = "\
Conventions
 - Circuits are built by NAME, not by coordinates: 'add' places a part, 'net NAME REF.PIN ...' connects pins.
 - The net named GND is the 0 V reference; every circuit needs one.
 - Pins can be written as number or name, any case. Look them up with 'part NAME' (library) or 'part R' / 'part LED' (basic types).
 - An answer with \"ok\":true only says the command ran. Check 'erc' (errors == 0) before trusting 'dc' / 'tran'.
 - 'tran' starts from the DC operating point; add 'uic' to start from zero (step responses, oscillators).
 - The run of one tool call stops at the first failing line (unless keep_going is true). The session keeps everything done before.
 - Results come from a simplified simulator: see docs/accuracy.md. Part data is approximate; check data sheets before ordering a board.";

fn tools() -> Value {
    json!([
        {
            "name": "ccad_run",
            "description": "Run ccad script lines (one command per line) in the persistent circuit session and return one JSON answer per line. Build a circuit by name: new, add TYPE [VALUE] [as=REF], net NAME REF.PIN ..., then erc, dc, tran, list, and for a board: board import / autoplace / route / drc / fab DIR. Call ccad_help first for the full command list and conventions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": {"type": "string", "description": "Commands, one per line ('#' starts a comment)."},
                    "keep_going": {"type": "boolean", "description": "Continue after a failing line (default: stop at the first failure)."}
                },
                "required": ["script"]
            }
        },
        {
            "name": "ccad_help",
            "description": "The ccad command list and conventions (GND, pin names, what 'ok' means). Read this before the first ccad_run.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn call(session: &mut Session, name: &str, args: &Value) -> Result<Value, (i64, String)> {
    match name {
        "ccad_help" => Ok(text_result(format!("{HELP}\n{CONVENTIONS}"), false)),
        "ccad_run" => {
            let script = args.get("script").and_then(Value::as_str).ok_or((-32602, "ccad_run needs a string 'script'".to_string()))?;
            let keep_going = args.get("keep_going").and_then(Value::as_bool).unwrap_or(false);
            let (answers, failed) = run_script(session, script, keep_going);
            let text = answers.iter().map(|a| Value::Object(a.clone()).to_string()).collect::<Vec<_>>().join("\n");
            Ok(text_result(if text.is_empty() { "(no commands)".into() } else { text }, failed))
        }
        other => Err((-32602, format!("unknown tool '{other}'"))),
    }
}

/// One incoming message -> the reply to write, if any (notifications get none).
fn handle(session: &mut Session, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let reply = |result: Result<Value, (i64, String)>| -> Option<Value> {
        let id = id.clone()?;
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        })
    };
    match method {
        "initialize" => {
            let version = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            reply(Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "ccad", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Build circuits with ccad_run (by part and pin names), check them with erc, simulate with dc / tran, and make a board. Call ccad_help first."
            })))
        }
        "ping" => reply(Ok(json!({}))),
        "tools/list" => reply(Ok(json!({"tools": tools()}))),
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            reply(call(session, name, &args))
        }
        // notifications (no id) and anything else we do not know
        _ if id.is_none() => None,
        other => reply(Err((-32601, format!("method '{other}' is not supported")))),
    }
}

pub fn serve(mut session: Session) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&mut session, &msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})),
        };
        if let Some(r) = reply
            && (writeln!(out, "{r}").is_err() || out.flush().is_err())
        {
            break;
        }
    }
}
