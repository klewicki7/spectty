//! `spectty-mcp` — stub MCP server for the Spectty Agent Protocol (Layer 1).
//!
//! M2 scaffold made real (WU-8): the crate is a standalone binary the agent
//! launches as a child process over stdio, so the provisioner's
//! `McpServerEntry.command` can point at a real binary path.
//!
//! It speaks newline-delimited JSON-RPC 2.0 over stdin/stdout (one JSON object
//! per line, no embedded newlines — the official MCP stdio transport framing):
//!
//! - `initialize`  → `protocolVersion` + `capabilities.tools` + `serverInfo`.
//! - `tools/list`  → the five Spectty tool schemas (the FROZEN M3-swap contract,
//!   D15/R4): `spectty_spec`, `spectty_diff`, `spectty_approval`,
//!   `spectty_status`, `spectty_cost`.
//! - `tools/call`  → for a KNOWN tool, a non-error acknowledgement with NO side
//!   effect (effects land in M3); unknown tool → `-32601`; bad params →
//!   `-32602`; unknown method → `-32601`; malformed JSON → `-32700`.
//!
//! The message handling is a PURE dispatch (`handle_message`) so it is
//! unit-testable WITHOUT real stdio; `main` is a thin reader loop around it.
//!
//! Depends on serde/serde_json ONLY — NOT spectty-core, NOT tauri (D16).

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// The MCP protocol version this stub speaks. M2 placeholder; the wire shape is
/// the part frozen as the M3-swap contract, not this date string.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Server identity advertised in the `initialize` handshake.
const SERVER_NAME: &str = "spectty-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// JSON-RPC 2.0 error codes (a subset; see the spec).
const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// The five Spectty protocol tools, in canonical order. The names and the
/// structural shape of the input schemas are FROZEN (D15/R4): M3 swaps the
/// EFFECTS behind `tools/call` without changing this advertised contract.
const TOOL_NAMES: [&str; 5] = [
    "spectty_spec",
    "spectty_diff",
    "spectty_approval",
    "spectty_status",
    "spectty_cost",
];

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            // stdin closed or an I/O error: stop cleanly.
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&line) {
            // One JSON object per line; flush so the agent sees it promptly.
            if writeln!(out, "{response}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

/// Pure JSON-RPC 2.0 dispatch over a single newline-delimited message.
///
/// Returns `Some(response_line)` for a request (a message carrying an `id`),
/// or `None` for a notification (no `id`) — notifications get no response per
/// JSON-RPC 2.0. A malformed line yields a `-32700` parse error with a null id.
pub fn handle_message(request_json: &str) -> Option<String> {
    let value: Value = match serde_json::from_str(request_json) {
        Ok(value) => value,
        Err(_) => return Some(error_response(Value::Null, PARSE_ERROR, "Parse error")),
    };

    // A request carries an `id`; a notification does not. Notifications never
    // get a response.
    let id = value.get("id").cloned();
    let is_notification = id.is_none();

    let method = value.get("method").and_then(Value::as_str).unwrap_or("");

    let result = dispatch(method, value.get("params"));

    if is_notification {
        return None;
    }
    let id = id.unwrap_or(Value::Null);

    Some(match result {
        Ok(result) => success_response(id, result),
        Err(rpc) => error_response(id, rpc.code, &rpc.message),
    })
}

/// A JSON-RPC error outcome from a dispatch handler.
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Route a method to its handler. Returns the `result` payload on success.
fn dispatch(method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(handle_initialize()),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(params),
        other => Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!("Method not found: {other}"),
        )),
    }
}

/// `initialize` → advertise protocol version, tool capability, and identity.
fn handle_initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
    })
}

/// `tools/list` → the five frozen Spectty tool schemas.
fn handle_tools_list() -> Value {
    json!({ "tools": tool_schemas() })
}

/// `tools/call` → for a KNOWN tool, a benign non-error ack with no side effect
/// (M2 stub; effects land in M3). Unknown tool → `-32601`; missing/bad params →
/// `-32602`.
fn handle_tools_call(params: Option<&Value>) -> Result<Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing tool name"))?;

    if !TOOL_NAMES.contains(&name) {
        return Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!("Unknown tool: {name}"),
        ));
    }

    // M2 stub: acknowledge, do NOTHING. No spec persisted, no diff triggered,
    // no approval resolved, no session state mutated. M3 swaps these effects in
    // behind the same advertised schema.
    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "spectty-mcp M2 stub: '{name}' acknowledged. Tool effects are not yet \
                 implemented (M3); no side effect was performed."
            )
        }],
        "isError": false
    }))
}

/// Build the five tool schema objects. Descriptions and input schemas are
/// honest M2 placeholders but structurally complete and forward-compatible with
/// the M3 effects (D15/R4) — mirrors `docs/architecture/agent-protocol.md`.
fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": "spectty_spec",
            "description": "Push plan progress to the Spectty Spec pane. \
                            M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "spec"],
                "properties": {
                    "session_id": { "type": "string" },
                    "spec": {
                        "type": "object",
                        "properties": {
                            "proposal": { "type": "string" },
                            "tasks": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["id", "title", "status"],
                                    "properties": {
                                        "id": { "type": "string" },
                                        "title": { "type": "string" },
                                        "status": {
                                            "enum": ["pending", "in_progress", "done", "skipped"]
                                        },
                                        "notes": { "type": "string" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
        json!({
            "name": "spectty_diff",
            "description": "Request a diff explanation for the current session's worktree. \
                            M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" },
                    "hint": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "spectty_approval",
            "description": "Request user approval before a risky action. \
                            M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "action_id", "description"],
                "properties": {
                    "session_id": { "type": "string" },
                    "action_id": { "type": "string" },
                    "description": { "type": "string" },
                    "risk_level": { "enum": ["low", "medium", "high"] },
                    "options": { "type": "array", "items": { "type": "string" } }
                }
            }
        }),
        json!({
            "name": "spectty_status",
            "description": "Push a transient status message to the session badge and status bar. \
                            M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "message"],
                "properties": {
                    "session_id": { "type": "string" },
                    "message": { "type": "string" },
                    "phase": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "spectty_cost",
            "description": "Push accumulated token/cost metrics for this session. \
                            M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "delta"],
                "properties": {
                    "session_id": { "type": "string" },
                    "delta": {
                        "type": "object",
                        "properties": {
                            "input_tokens": { "type": "integer" },
                            "output_tokens": { "type": "integer" },
                            "cache_read_tokens": { "type": "integer" },
                            "estimated_usd": { "type": "number" }
                        }
                    },
                    "model": { "type": "string" }
                }
            }
        }),
    ]
}

fn success_response(id: Value, result: Value) -> String {
    // serde_json::to_string never fails for these owned, finite Values.
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a response line back into a `Value` for assertions.
    fn response_of(request: &str) -> Value {
        let line = handle_message(request).expect("request must produce a response");
        serde_json::from_str(&line).expect("response must be valid JSON")
    }

    #[test]
    fn initialize_returns_protocol_and_serverinfo() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        let result = &resp["result"];
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(
            result["capabilities"]["tools"].is_object(),
            "must advertise the tools capability"
        );
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(result["serverInfo"]["version"], SERVER_VERSION);
    }

    #[test]
    fn tools_list_advertises_exactly_five_schemas() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);

        let tools = resp["result"]["tools"]
            .as_array()
            .expect("tools must be an array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("each tool has a name"))
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
            "must advertise exactly the five frozen tools in canonical order"
        );
    }

    #[test]
    fn tools_list_schemas_are_structurally_complete() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let tools = resp["result"]["tools"].as_array().unwrap();

        for tool in tools {
            assert!(
                tool["description"].as_str().is_some(),
                "every tool needs a description"
            );
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "every input schema is an object schema"
            );
            assert!(
                tool["inputSchema"]["properties"]
                    .get("session_id")
                    .is_some(),
                "every Spectty tool is session-scoped"
            );
        }
    }

    #[test]
    fn tools_call_known_returns_ack_no_effect() {
        let resp = response_of(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call",
               "params":{"name":"spectty_spec","arguments":{"session_id":"s1"}}}"#,
        );

        assert!(
            resp.get("error").is_none(),
            "a known tool call must NOT be an error"
        );
        let result = &resp["result"];
        assert_eq!(result["isError"], false);
        let text = result["content"][0]["text"]
            .as_str()
            .expect("ack carries text content");
        assert!(
            text.contains("stub") || text.contains("acknowledged"),
            "ack text should signal the M2 stub: {text}"
        );
    }

    #[test]
    fn tools_call_unknown_returns_method_not_found() {
        let resp = response_of(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call",
               "params":{"name":"spectty_unknown","arguments":{}}}"#,
        );

        assert!(
            resp.get("result").is_none(),
            "unknown tool yields no result"
        );
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
        assert!(resp["error"]["message"].as_str().is_some());
    }

    #[test]
    fn tools_call_missing_name_returns_invalid_params() {
        let resp = response_of(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"arguments":{}}}"#,
        );

        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn tools_call_missing_params_returns_invalid_params() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call"}"#);

        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":8,"method":"does/not/exist"}"#);

        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn malformed_json_returns_parse_error_without_panic() {
        let resp = response_of("{ this is not valid json ");

        assert_eq!(resp["error"]["code"], PARSE_ERROR);
        assert_eq!(resp["id"], Value::Null);
    }

    #[test]
    fn notification_without_id_yields_no_response() {
        // No `id` => a JSON-RPC notification => no response per spec.
        let out = handle_message(r#"{"jsonrpc":"2.0","method":"initialize"}"#);
        assert!(out.is_none(), "notifications get no response");
    }

    #[test]
    fn response_always_echoes_the_request_id() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list"}"#);
        assert_eq!(resp["id"], "abc");
    }
}
