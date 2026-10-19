//! Talks to a real `ccad mcp` process the way an MCP client does: JSON-RPC, one message per line.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start() -> Client {
        let lib = std::env::temp_dir().join(format!("ccad-test-mcp-lib-{}", std::process::id()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_ccad"))
            .args(["--lib", lib.to_str().unwrap(), "mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, out, next: 1 }
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"));
        assert_eq!(v["id"], id, "{v}");
        v
    }

    /// The tool's answers (one JSON object per line of its text) and the isError flag.
    fn run(&mut self, script: &str) -> (Vec<Value>, bool) {
        let r = self.request("tools/call", json!({"name": "ccad_run", "arguments": {"script": script}}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{r}")).to_string();
        let answers = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        (answers, r["result"]["isError"].as_bool().unwrap())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn handshake_lists_the_tools() {
    let mut c = Client::start();
    let init = c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "ccad");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // a notification gets no reply: the next request must still be answered in order
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let list = c.request("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["ccad_run", "ccad_help"]);
    assert_eq!(list["result"]["tools"][0]["inputSchema"]["required"][0], "script");
    let help = c.request("tools/call", json!({"name": "ccad_help", "arguments": {}}));
    let text = help["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("net NAME REF.PIN") && text.contains("named GND"), "{text}");
    let unknown = c.request("resources/list", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn the_circuit_lives_across_calls() {
    let mut c = Client::start();
    let (a, failed) = c.run("new\nadd V 12 as=V1\nadd R 10k as=R1\nadd R 4.7k as=R2\n");
    assert!(!failed, "{a:?}");
    let (a, failed) = c.run("net VIN V1.+ R1.1\nnet MID R1.2 R2.1\nnet GND V1.- R2.2\nerc\ndc\n");
    assert!(!failed, "{a:?}");
    let mid = a.iter().find(|v| v["cmd"] == "dc").unwrap()["voltages"]["MID"].as_f64().unwrap();
    assert!((mid - 12.0 * 4.7 / 14.7).abs() < 1e-6, "{mid}");
    // a failing line is an error result (isError), and the circuit built so far is still there
    let (a, failed) = c.run("net X R1.9\n");
    assert!(failed);
    assert!(a[0]["error"].as_str().unwrap().contains("has no pin"), "{a:?}");
    let (a, failed) = c.run("list\n");
    assert!(!failed);
    assert_eq!(a[0]["parts"].as_array().unwrap().len(), 3);
}

#[test]
fn bad_input_gets_protocol_errors_not_a_crash() {
    let mut c = Client::start();
    writeln!(c.stdin, "this is not json").unwrap();
    let mut line = String::new();
    c.out.read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["error"]["code"], -32700);
    let r = c.request("tools/call", json!({"name": "nope", "arguments": {}}));
    assert_eq!(r["error"]["code"], -32602);
    let r = c.request("tools/call", json!({"name": "ccad_run", "arguments": {}}));
    assert_eq!(r["error"]["code"], -32602);
    // still alive
    let r = c.request("ping", json!({}));
    assert!(r["result"].is_object());
}
