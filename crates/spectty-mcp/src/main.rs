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
//! The message-handling dispatch (`handle_message_with`) is parameterized over an
//! [`EngramClient`] seam so it is unit-testable WITHOUT real stdio or a real daemon;
//! `main` is a thin reader loop around it backed by the real [`ReqwestEngramClient`].
//!
//! ## M4 (WU-4): `spectty_spec` gains a real EFFECT behind the FROZEN schema
//!
//! The advertised `tools/list` schema is byte-frozen (D16/R4). M4 changes ONLY the
//! `tools/call` EFFECT: a `spectty_spec` call now upserts the contract to the canonical
//! engram key `spectty/{session_id}/spec` via a thin HTTP client and returns immediately
//! — the app's poll loop (PR-1 `SpecBus`) surfaces the change as a `spec_updated` event.
//!
//! Depends on serde/serde_json + a thin `reqwest` HTTP client ONLY — NOT spectty-core,
//! NOT tauri (D16). The engram wire shapes are the G1-verified ones the adapter uses.

use std::io::{BufRead, Write};
use std::time::Duration;

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

    // The real engram transport. Effects (`spectty_spec` upsert) go through this; if the
    // daemon is unreachable the effect degrades to a benign error result (never a crash).
    let client = ReqwestEngramClient::from_env();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            // stdin closed or an I/O error: stop cleanly.
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message_with(&line, &client) {
            // One JSON object per line; flush so the agent sees it promptly.
            if writeln!(out, "{response}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

/// JSON-RPC 2.0 dispatch over a single newline-delimited message, with NO engram
/// effects (a [`NoopEngramClient`] is used). Kept for the schema/handshake tests that
/// assert pure protocol behavior; effect-bearing callers use [`handle_message_with`].
///
/// Returns `Some(response_line)` for a request (a message carrying an `id`),
/// or `None` for a notification (no `id`) — notifications get no response per
/// JSON-RPC 2.0. A malformed line yields a `-32700` parse error with a null id.
#[cfg(test)]
pub fn handle_message(request_json: &str) -> Option<String> {
    handle_message_with(request_json, &NoopEngramClient)
}

/// JSON-RPC 2.0 dispatch parameterized over an [`EngramClient`] so `tools/call` effects
/// (M4: `spectty_spec` upsert) are exercised against a fake in tests and the real
/// transport in `main`. Notification/parse-error semantics are unchanged from M2.
pub fn handle_message_with(request_json: &str, engram: &dyn EngramClient) -> Option<String> {
    let value: Value = match serde_json::from_str(request_json) {
        Ok(value) => value,
        Err(_) => return Some(error_response(Value::Null, PARSE_ERROR, "Parse error")),
    };

    // A request carries an `id`; a notification does not. Notifications never
    // get a response.
    let id = value.get("id").cloned();
    let is_notification = id.is_none();

    let method = value.get("method").and_then(Value::as_str).unwrap_or("");

    let result = dispatch(method, value.get("params"), engram);

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
fn dispatch(
    method: &str,
    params: Option<&Value>,
    engram: &dyn EngramClient,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(handle_initialize()),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(params, engram),
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

/// `tools/call` → dispatch a KNOWN tool to its EFFECT. `spectty_spec` upserts the
/// contract to engram (M4); the remaining tools keep the benign stub ack until their
/// effects land. Unknown tool → `-32601`; missing/bad params → `-32602`.
fn handle_tools_call(params: Option<&Value>, engram: &dyn EngramClient) -> Result<Value, RpcError> {
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

    let arguments = params.get("arguments");
    match name {
        "spectty_spec" => spectty_spec_effect(arguments, engram),
        // The remaining tools keep the benign stub ack (effects land in later slices).
        other => Ok(stub_ack(other)),
    }
}

/// The `spectty_spec` EFFECT (M4 WU-4, D29/D5): parse `{session_id, spec}`, upsert the
/// serialized contract to the canonical key `spectty/{session_id}/spec`, and return
/// IMMEDIATELY. A malformed payload (missing `session_id`/`spec`, or a `spec` that is not
/// an object) is rejected as `-32602` WITHOUT upserting a partial blob. An engram
/// transport failure degrades to a benign error result (`isError: true`) — never a panic,
/// so a down daemon does not break the agent's turn.
fn spectty_spec_effect(
    arguments: Option<&Value>,
    engram: &dyn EngramClient,
) -> Result<Value, RpcError> {
    let arguments = arguments.ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing arguments"))?;

    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "spectty_spec: missing session_id"))?;

    // `spec` MUST be present and an object — reject a partial/malformed payload BEFORE
    // any upsert so engram never stores a half-built blob.
    let spec = arguments
        .get("spec")
        .filter(|v| v.is_object())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "spectty_spec: missing or non-object spec"))?;

    let topic_key = format!("spectty/{session_id}/spec");
    // Serialize the spec object verbatim — the app side deserializes it into the Core
    // `SpecContract`. `to_string` cannot fail for an already-parsed `Value`.
    let content = spec.to_string();

    match engram.upsert(&topic_key, &content) {
        Ok(()) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_spec: upserted contract to {topic_key}")
            }],
            "isError": false
        })),
        // Degrade, do not crash: the agent's turn continues even when engram is down.
        Err(EngramClientError::Transport(msg)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_spec: engram unavailable ({msg}); spec not persisted")
            }],
            "isError": true
        })),
    }
}

/// The benign stub acknowledgement for tools whose effects have not yet landed.
fn stub_ack(name: &str) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": format!(
                "spectty-mcp: '{name}' acknowledged. Tool effect is not yet implemented; \
                 no side effect was performed."
            )
        }],
        "isError": false
    })
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

/// The thin engram write seam (D16). The MCP binary only ever UPSERTS observations
/// (it pushes spec/diff/status/cost out); it never reads back. Sync signature — the real
/// impl uses `reqwest::blocking`, so no `async`/`tokio` leaks into this stdio binary.
pub trait EngramClient {
    /// Create-or-update the observation under `topic_key` with `content`.
    fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError>;
}

/// Failure modes of an [`EngramClient`] upsert. The effect maps this to a benign error
/// result so a down daemon never crashes the agent's turn.
#[derive(Debug)]
pub enum EngramClientError {
    /// The HTTP request failed (connection refused, timeout, non-success status, ...).
    Transport(String),
}

/// A no-op [`EngramClient`] used by the pure schema/handshake tests and `handle_message`:
/// it accepts every upsert without performing I/O.
#[cfg(test)]
struct NoopEngramClient;

#[cfg(test)]
impl EngramClient for NoopEngramClient {
    fn upsert(&self, _topic_key: &str, _content: &str) -> Result<(), EngramClientError> {
        Ok(())
    }
}

/// The engram HTTP base URL. Matches the adapter's `ENGRAM_BASE_URL`; overridable via the
/// `ENGRAM_BASE_URL` env var so the binary can target a non-default daemon.
const ENGRAM_BASE_URL: &str = "http://localhost:7437";

/// The real engram client over `reqwest::blocking` (D16: serde + http only; never
/// core/tauri). Mirrors the G1-verified wire shapes the `EngramAdapter` uses: a
/// `POST /sessions` (idempotent) precedes each `POST /observations` because the
/// observation's `session_id` must already exist.
pub struct ReqwestEngramClient {
    base_url: String,
    project: String,
    client: reqwest::blocking::Client,
}

impl ReqwestEngramClient {
    /// Build a client from the environment: `ENGRAM_BASE_URL` (default
    /// [`ENGRAM_BASE_URL`]) and `SPECTTY_ENGRAM_PROJECT` (default `"spectty"`).
    fn from_env() -> Self {
        // A 2s request timeout is REQUIRED (spectty-spec-effect.md:31,36: the effect must
        // "return promptly + degrade"). Without it, an accepted-but-unresponsive engram
        // daemon would hang the agent's turn forever. We `expect` rather than fall back to
        // the default (timeout-less) client: a builder failure is a programmer error, and a
        // silent fallback would reintroduce the unbounded-hang it exists to prevent.
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest blocking client builder with a 2s timeout must succeed");
        Self {
            base_url: std::env::var("ENGRAM_BASE_URL")
                .unwrap_or_else(|_| ENGRAM_BASE_URL.to_string()),
            project: std::env::var("SPECTTY_ENGRAM_PROJECT")
                .unwrap_or_else(|_| "spectty".to_string()),
            client,
        }
    }

    /// Derive the engram session id from a canonical `spectty/{session_id}/...` key.
    fn session_id_of(topic_key: &str) -> String {
        let mut parts = topic_key.split('/');
        match (parts.next(), parts.next()) {
            (Some("spectty"), Some(sid)) if !sid.is_empty() => sid.to_string(),
            _ => "spectty".to_string(),
        }
    }
}

impl EngramClient for ReqwestEngramClient {
    fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError> {
        let session_id = Self::session_id_of(topic_key);

        // Ensure the session row exists (idempotent INSERT-OR-IGNORE).
        let resp = self
            .client
            .post(format!("{}/sessions", self.base_url))
            .json(&json!({ "id": session_id, "project": self.project }))
            .send()
            .map_err(|e| EngramClientError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(EngramClientError::Transport(format!(
                "POST /sessions returned {}",
                resp.status()
            )));
        }

        let resp = self
            .client
            .post(format!("{}/observations", self.base_url))
            .json(&json!({
                "session_id": session_id,
                "topic_key": topic_key,
                "project": self.project,
                "scope": "project",
                "content": content,
                "type": "architecture",
                "title": topic_key,
            }))
            .send()
            .map_err(|e| EngramClientError::Transport(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(EngramClientError::Transport(format!(
                "POST /observations returned {}",
                resp.status()
            )))
        }
    }
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
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// Parse a response line back into a `Value` for assertions.
    fn response_of(request: &str) -> Value {
        let line = handle_message(request).expect("request must produce a response");
        serde_json::from_str(&line).expect("response must be valid JSON")
    }

    /// A recording [`EngramClient`] double: it stores upserts (so a test can assert the
    /// canonical key + content) and can be scripted to fail (engram-down degrade path).
    #[derive(Default)]
    struct FakeEngramClient {
        store: Mutex<HashMap<String, String>>,
        fail: bool,
    }

    impl FakeEngramClient {
        fn new() -> Self {
            Self::default()
        }

        fn failing() -> Self {
            Self {
                fail: true,
                ..Self::default()
            }
        }

        fn get(&self, topic_key: &str) -> Option<String> {
            self.store.lock().unwrap().get(topic_key).cloned()
        }
    }

    impl EngramClient for FakeEngramClient {
        fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError> {
            if self.fail {
                return Err(EngramClientError::Transport(
                    "fake: engram down".to_string(),
                ));
            }
            self.store
                .lock()
                .unwrap()
                .insert(topic_key.to_string(), content.to_string());
            Ok(())
        }
    }

    /// Parse an effect-bearing response over a given client.
    fn response_with(request: &str, engram: &dyn EngramClient) -> Value {
        let line = handle_message_with(request, engram).expect("request must produce a response");
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
    fn tools_call_known_stub_tool_returns_ack_no_effect() {
        // `spectty_status` has no effect yet, so over the no-op client it acks benignly.
        let resp = response_of(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call",
               "params":{"name":"spectty_status","arguments":{"session_id":"s1","message":"hi"}}}"#,
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
            text.contains("acknowledged"),
            "ack text should signal the stub: {text}"
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

    // ── M4 WU-4 ──────────────────────────────────────────────────────────────────────

    /// WU-4.1: the advertised `tools/list` schema is FROZEN (D16/R4). M4 changes ONLY
    /// `tools/call` EFFECTS, never the schema. This pins the schema byte-for-byte against
    /// a checked-in fixture; mutating any name/order/parameter breaks this test (RED).
    #[test]
    fn spectty_mcp_tools_list_schema_is_byte_frozen() {
        let resp = response_of(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        // Compare the schema (the `tools` array) against the frozen fixture verbatim.
        let actual = &resp["result"]["tools"];
        let frozen: Value = serde_json::from_str(FROZEN_TOOLS_SCHEMA)
            .expect("frozen schema fixture must be valid JSON");
        assert_eq!(
            *actual, frozen,
            "tools/list schema drifted from the M3-frozen contract (D16/R4)"
        );
    }

    /// WU-4.2: `spectty_spec` upserts the serialized contract to the canonical key
    /// `spectty/{session_id}/spec` and returns promptly (non-error). The exact contract
    /// JSON is preserved.
    #[test]
    fn spectty_spec_upserts_canonical_key_and_returns_immediately() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                 "name":"spectty_spec",
                 "arguments":{"session_id":"42","spec":{"intent":"fix it","tasks":[]}}}}"#,
            &engram,
        );

        assert!(
            resp.get("error").is_none(),
            "a valid call must not be a JSON-RPC error"
        );
        assert_eq!(resp["result"]["isError"], false);

        let stored = engram
            .get("spectty/42/spec")
            .expect("the contract must be upserted under the canonical key");
        let stored: Value = serde_json::from_str(&stored).expect("stored content is JSON");
        assert_eq!(stored["intent"], "fix it");
        assert!(stored["tasks"].as_array().expect("tasks array").is_empty());
    }

    /// WU-4.2 (triangulation): a different session id targets a different canonical key —
    /// proving the key is built from the payload, not hardcoded.
    #[test]
    fn spectty_spec_keys_are_per_session() {
        let engram = FakeEngramClient::new();
        response_with(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                 "name":"spectty_spec",
                 "arguments":{"session_id":"abc-7","spec":{"intent":"x","tasks":[]}}}}"#,
            &engram,
        );
        assert!(engram.get("spectty/abc-7/spec").is_some());
        assert!(
            engram.get("spectty/42/spec").is_none(),
            "only the addressed session's key must be written"
        );
    }

    /// WU-4.2: a malformed `spectty_spec` payload (missing `spec`) is rejected as
    /// `-32602` WITHOUT upserting a partial blob — no crash.
    #[test]
    fn spectty_spec_malformed_payload_is_rejected_without_upsert() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                 "name":"spectty_spec","arguments":{"session_id":"42"}}}"#,
            &engram,
        );
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
        assert!(
            engram.get("spectty/42/spec").is_none(),
            "a malformed payload must NOT upsert a partial blob"
        );
    }

    /// WU-4.2: when engram is unreachable the effect DEGRADES to a benign error result
    /// (`isError: true`) instead of panicking — a down daemon must not break the turn.
    #[test]
    fn spectty_spec_degrades_when_engram_down() {
        let engram = FakeEngramClient::failing();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
                 "name":"spectty_spec",
                 "arguments":{"session_id":"42","spec":{"intent":"x","tasks":[]}}}}"#,
            &engram,
        );
        // Not a JSON-RPC protocol error — a tool result flagged isError so the agent sees
        // the degradation but the turn continues.
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
    }

    /// The other four tools keep their benign stub ack (no effect yet) over a real client
    /// seam — proving the dispatch only routed `spectty_spec` to an effect.
    #[test]
    fn non_spec_tools_keep_stub_ack() {
        let engram = FakeEngramClient::new();
        for name in [
            "spectty_diff",
            "spectty_approval",
            "spectty_status",
            "spectty_cost",
        ] {
            let req = format!(
                r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"{name}","arguments":{{"session_id":"s"}}}}}}"#
            );
            let resp = response_with(&req, &engram);
            assert_eq!(resp["result"]["isError"], false, "{name} must ack");
            assert!(engram.get("spectty/s/spec").is_none());
        }
    }

    /// The byte-frozen `tools/list` schema fixture (M3-swap contract). If a deliberate
    /// schema change is ever ratified, this fixture and the spec must be updated together.
    const FROZEN_TOOLS_SCHEMA: &str = r#"[
        {
            "name": "spectty_spec",
            "description": "Push plan progress to the Spectty Spec pane. M2 stub: acknowledged with no effect; effects land in M3.",
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
                                        "status": { "enum": ["pending", "in_progress", "done", "skipped"] },
                                        "notes": { "type": "string" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        {
            "name": "spectty_diff",
            "description": "Request a diff explanation for the current session's worktree. M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" },
                    "hint": { "type": "string" }
                }
            }
        },
        {
            "name": "spectty_approval",
            "description": "Request user approval before a risky action. M2 stub: acknowledged with no effect; effects land in M3.",
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
        },
        {
            "name": "spectty_status",
            "description": "Push a transient status message to the session badge and status bar. M2 stub: acknowledged with no effect; effects land in M3.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id", "message"],
                "properties": {
                    "session_id": { "type": "string" },
                    "message": { "type": "string" },
                    "phase": { "type": "string" }
                }
            }
        },
        {
            "name": "spectty_cost",
            "description": "Push accumulated token/cost metrics for this session. M2 stub: acknowledged with no effect; effects land in M3.",
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
        }
    ]"#;
}
