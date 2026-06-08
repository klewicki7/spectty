//! WU-11.2 — end-to-end stdio handshake against the built `spectty-mcp` binary.
//!
//! This pins the wire framing (D15/R4 / design open-question c): the official
//! MCP stdio transport is newline-delimited JSON-RPC 2.0 — one JSON object per
//! line over stdin/stdout, no embedded newlines, no `Content-Length` headers.
//!
//! We spawn the real binary (`CARGO_BIN_EXE_spectty-mcp`), write an
//! `initialize` line and a `tools/list` line, close stdin, read to EOF, and
//! assert the five frozen tool names come back plus the `-32601` contract for
//! an unknown `tools/call`. Deterministic: write → flush → close stdin → drain.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

/// Drive the binary with the given newline-delimited request lines and collect
/// every non-empty response line, parsed as JSON.
fn run_handshake(requests: &[&str]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spectty-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spectty-mcp binary must spawn");

    {
        let mut stdin = child.stdin.take().expect("child stdin");
        for line in requests {
            writeln!(stdin, "{line}").expect("write request line");
        }
        stdin.flush().expect("flush stdin");
        // stdin dropped here → EOF → the server's reader loop exits cleanly.
    }

    let stdout = child.stdout.take().expect("child stdout");
    let responses: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map(|l| l.expect("read response line"))
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(&l).expect("response line is valid JSON"))
        .collect();

    let status = child.wait().expect("child must exit");
    assert!(
        status.success(),
        "spectty-mcp must exit cleanly on stdin EOF, got {status:?}"
    );

    responses
}

#[test]
fn spectty_mcp_stdio_handshake_advertises_five_tools() {
    let responses = run_handshake(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    ]);

    assert_eq!(responses.len(), 2, "one response line per request");

    // initialize response.
    let init = &responses[0];
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "spectty-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // tools/list response — the five frozen tools, in canonical order.
    let list = &responses[1];
    assert_eq!(list["id"], 2);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(
        names,
        vec![
            "spectty_spec",
            "spectty_diff",
            "spectty_approval",
            "spectty_status",
            "spectty_cost"
        ],
        "the stdio handshake must advertise the five frozen tools"
    );
}

#[test]
fn spectty_mcp_stdio_unknown_tool_call_returns_method_not_found() {
    let responses = run_handshake(&[
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
    ]);

    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert_eq!(resp["id"], 7);
    assert_eq!(
        resp["error"]["code"], -32601,
        "the R4 unknown-tool contract must hold end-to-end over stdio"
    );
}
