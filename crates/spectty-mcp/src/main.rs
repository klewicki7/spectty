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
        "spectty_approval" => spectty_approval_effect(arguments, engram),
        "spectty_diff" => spectty_diff_effect(arguments, engram),
        // The remaining tools keep the benign stub ack (effects land in later slices).
        other => Ok(stub_ack(other)),
    }
}

/// The `spectty_diff` EFFECT (M4 WU-8, D37/D29): the COOPERATIVE diff trigger. A cooperative
/// agent calls this after it edits files; the effect upserts a trigger doc to
/// `spectty/{session_id}/diff` and returns IMMEDIATELY (it does NOT run git or VibeLens — the
/// app-side diff poll observes the trigger doc change and runs the pipeline, bypassing the
/// FileWatch debounce for low latency). The `hint` (optional, advertised) is recorded so the
/// app can attribute the trigger; a fresh nonce makes every call a genuine change the poll
/// detects (the doc must differ each time, even for the same hint, so a rapid re-edit always
/// re-triggers).
///
/// A malformed payload (missing `session_id`) is `-32602`. An engram transport failure
/// degrades to a benign `isError` result — never a panic, so a down daemon does not break the
/// agent's turn (spectty-diff-effect.md: "engram-down degrades isError").
fn spectty_diff_effect(
    arguments: Option<&Value>,
    engram: &dyn EngramClient,
) -> Result<Value, RpcError> {
    let arguments = arguments.ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing arguments"))?;

    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "spectty_diff: missing session_id"))?;

    let topic_key = format!("spectty/{session_id}/diff");
    // A monotonic-ish nonce so a back-to-back trigger for the SAME hint still changes the
    // doc content (the app poll change-detects by content; an identical re-upsert would be
    // a no-op and a rapid re-edit would be missed). System time in nanos is sufficient: the
    // doc is a transient signal, not an ordering key.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let trigger = json!({
        "session_id": session_id,
        "hint": arguments.get("hint").and_then(Value::as_str),
        "nonce": nonce.to_string(),
    });

    match engram.upsert(&topic_key, &trigger.to_string()) {
        Ok(()) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_diff: triggered diff pipeline via {topic_key}")
            }],
            "isError": false
        })),
        Err(EngramClientError::Transport(msg)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_diff: engram unavailable ({msg}); diff not triggered")
            }],
            "isError": true
        })),
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

/// Default approval long-poll interval (D31: "~500 ms, bounded"). Overridable via
/// `SPECTTY_APPROVAL_POLL_MS`.
const APPROVAL_POLL_MS: u64 = 500;
/// Default bounded number of long-poll attempts before the handler returns a `pending`
/// (timeout) result rather than hanging the agent's turn forever (D31, spec: "return
/// pending/timeout, never hang"). Default ≈ 5 minutes at 500 ms. Overridable via
/// `SPECTTY_APPROVAL_MAX_POLLS`.
const APPROVAL_MAX_POLLS: u32 = 600;

/// The `spectty_approval` EFFECT (M4 WU-5, D31/D33). This is the ONE genuinely BLOCKING
/// tool: it registers a pending request at `spectty/{session_id}/approval` and then
/// LONG-POLLS the same key until the app's `approve_prompt` writes back a `resolution`, at
/// which point the resolution is returned to the agent. A malformed payload (missing
/// `session_id`/`action_id`) is `-32602`. An engram transport failure degrades to a benign
/// `isError` result — never a panic, so a down daemon does not break the turn.
///
/// `main` drives this with a real 500 ms sleep; tests inject an immediate resolver via a
/// fake client and a zero interval so no real wall-clock time is spent.
fn spectty_approval_effect(
    arguments: Option<&Value>,
    engram: &dyn EngramClient,
) -> Result<Value, RpcError> {
    let arguments = arguments.ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing arguments"))?;

    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "spectty_approval: missing session_id"))?;
    let action_id = arguments
        .get("action_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "spectty_approval: missing action_id"))?;

    let topic_key = format!("spectty/{session_id}/approval");

    // Register the PENDING request (resolution: null). Built from the advertised arguments
    // so the app's status path can derive `quick_actions` from `options`. Idempotent: a
    // duplicate `(session_id, action_id)` upsert overwrites with the same pending content,
    // so exactly one pending entry exists for the key (spec M4-REQ-10).
    let pending = json!({
        "action_id": action_id,
        "description": arguments.get("description").and_then(Value::as_str).unwrap_or(""),
        "risk_level": arguments.get("risk_level").and_then(Value::as_str),
        "options": arguments.get("options").cloned().unwrap_or_else(|| json!([])),
        "resolution": Value::Null,
    });
    if let Err(EngramClientError::Transport(msg)) = engram.upsert(&topic_key, &pending.to_string())
    {
        return Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_approval: engram unavailable ({msg}); approval not registered")
            }],
            "isError": true
        }));
    }

    // Long-poll the same key until the app writes a non-null `resolution`, or the bounded
    // budget elapses (return a `pending` result rather than hanging — D31).
    let interval = Duration::from_millis(approval_poll_ms());
    match poll_for_resolution(
        engram,
        &topic_key,
        action_id,
        approval_max_polls(),
        interval,
    ) {
        Ok(Some(decision)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_approval: resolved as {decision}")
            }],
            "isError": false
        })),
        // Bounded budget elapsed with no resolution: tell the agent it is still pending so it
        // can decide what to do — the turn ends rather than blocking forever.
        Ok(None) => Ok(json!({
            "content": [{
                "type": "text",
                "text": "spectty_approval: still pending (approval long-poll timed out)"
            }],
            "isError": false
        })),
        Err(EngramClientError::Transport(msg)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("spectty_approval: engram unavailable ({msg}); cannot await resolution")
            }],
            "isError": true
        })),
    }
}

/// How many CONSECUTIVE transient `get` failures the long-poll tolerates before surfacing the
/// error (PR-3 Finding 4). A single blip — e.g. the 2 s client timeout firing once under load —
/// must NOT abort an otherwise-healthy poll; only a sustained outage does. The counter resets
/// on any successful read, so isolated blips never accumulate across the whole budget.
const APPROVAL_MAX_CONSECUTIVE_GET_FAILURES: u32 = 3;

/// Poll `topic_key` up to `max_polls` times (sleeping `interval` between attempts) for the
/// stored approval document's `resolution` to become a non-null decision matching
/// `action_id`. Returns:
/// - `Ok(Some(decision))` once a resolution is observed (the canonical Core `ApprovalState`
///   string the app wrote, e.g. `"Approved"`);
/// - `Ok(None)` if the budget elapses with no resolution (bounded — never hangs);
/// - `Err` only after [`APPROVAL_MAX_CONSECUTIVE_GET_FAILURES`] CONSECUTIVE transport failures
///   (a single transient blip is tolerated and retried within the same `max_polls` budget).
///
/// PURE over the [`EngramClient`] seam: tests pass a fake that resolves on the first read and
/// a zero `interval`, so no real wall-clock time is consumed and the loop logic is exercised
/// directly. `max_polls` stays the authoritative total bound — tolerated retries are spent
/// from the SAME budget, they never extend it.
fn poll_for_resolution(
    engram: &dyn EngramClient,
    topic_key: &str,
    action_id: &str,
    max_polls: u32,
    interval: Duration,
) -> Result<Option<String>, EngramClientError> {
    // Count consecutive transport failures so one blip does not abort the whole poll; reset to
    // zero on any successful read so only a SUSTAINED outage surfaces (PR-3 Finding 4).
    let mut consecutive_failures: u32 = 0;
    for attempt in 0..max_polls {
        match engram.get(topic_key) {
            Ok(maybe_content) => {
                consecutive_failures = 0;
                if let Some(content) = maybe_content {
                    if let Some(decision) = resolution_of(&content, action_id) {
                        return Ok(Some(decision));
                    }
                }
            }
            Err(err) => {
                consecutive_failures += 1;
                // Only give up once a SUSTAINED outage exceeds the tolerance — a transient blip
                // is absorbed and retried (still within the `max_polls` total bound).
                if consecutive_failures >= APPROVAL_MAX_CONSECUTIVE_GET_FAILURES {
                    return Err(err);
                }
            }
        }
        // Sleep BETWEEN attempts, not after the last one (which would waste an interval).
        if attempt + 1 < max_polls && !interval.is_zero() {
            std::thread::sleep(interval);
        }
    }
    Ok(None)
}

/// Extract a non-null `resolution` decision for `action_id` from a stored approval document,
/// or `None` if the document is unparseable, addresses a different `action_id`, or is still
/// pending (`resolution: null`). The decision is whatever canonical string the app wrote
/// (a Core `ApprovalState` variant); the MCP does not interpret it beyond returning it.
fn resolution_of(content: &str, action_id: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(content).ok()?;
    // Guard against a stale/other request under the same key.
    if doc.get("action_id").and_then(Value::as_str) != Some(action_id) {
        return None;
    }
    let resolution = doc.get("resolution")?;
    if resolution.is_null() {
        return None;
    }
    // The app writes `resolution` as a bare Core ApprovalState string ("Approved", ...).
    resolution.as_str().map(str::to_string)
}

/// The approval long-poll interval in millis (`SPECTTY_APPROVAL_POLL_MS`, default
/// [`APPROVAL_POLL_MS`]).
fn approval_poll_ms() -> u64 {
    std::env::var("SPECTTY_APPROVAL_POLL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(APPROVAL_POLL_MS)
}

/// The bounded long-poll attempt budget (`SPECTTY_APPROVAL_MAX_POLLS`, default
/// [`APPROVAL_MAX_POLLS`]).
fn approval_max_polls() -> u32 {
    std::env::var("SPECTTY_APPROVAL_MAX_POLLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(APPROVAL_MAX_POLLS)
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

/// The thin engram HTTP seam (D16). The MCP binary UPSERTS observations (it pushes
/// spec/diff/status/cost out) and — for the one BLOCKING tool, `spectty_approval` (D31) —
/// READS one back while long-polling for the user's resolution. Sync signatures — the real
/// impl uses `reqwest::blocking`, so no `async`/`tokio` leaks into this stdio binary.
pub trait EngramClient {
    /// Create-or-update the observation under `topic_key` with `content`.
    fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError>;

    /// Read the latest observation `content` for `topic_key`, or `None` when absent. Used
    /// ONLY by the `spectty_approval` long-poll (D31): the handler upserts a pending request
    /// then polls `get` on the same key until the app writes back a resolution.
    fn get(&self, topic_key: &str) -> Result<Option<String>, EngramClientError>;
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

    fn get(&self, _topic_key: &str) -> Result<Option<String>, EngramClientError> {
        Ok(None)
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

    fn get(&self, topic_key: &str) -> Result<Option<String>, EngramClientError> {
        // Mirror the EngramAdapter's G1-verified read: `?topic_key=` is NOT honored
        // server-side, so fetch the list and filter CLIENT-SIDE (case-insensitive — the
        // server lowercases topic keys), picking the most recently updated matching row.
        let resp = self
            .client
            .get(format!("{}/observations", self.base_url))
            .send()
            .map_err(|e| EngramClientError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(EngramClientError::Transport(format!(
                "GET /observations returned {}",
                resp.status()
            )));
        }
        let rows: Vec<RawObs> = resp
            .json()
            .map_err(|e| EngramClientError::Transport(e.to_string()))?;
        let wanted = topic_key.to_ascii_lowercase();
        let latest = rows
            .into_iter()
            .filter(|r| {
                r.topic_key
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(&wanted))
                    .unwrap_or(false)
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|r| r.content);
        Ok(latest)
    }
}

/// A single observation row as returned by `GET /observations` (the subset the approval
/// long-poll reads). Mirrors the adapter's `RawObs`; `topic_key` is optional because the
/// list may carry rows written without one.
#[derive(serde::Deserialize)]
struct RawObs {
    topic_key: Option<String>,
    content: String,
    #[serde(default)]
    updated_at: String,
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
    use std::sync::{Mutex, OnceLock};

    use super::*;

    /// Serializes the env-var-driven approval tests: `approval_poll_ms`/`approval_max_polls`
    /// read process-global env vars, so concurrent tests that set them would race. Each such
    /// test holds this lock while it mutates and reads the vars.
    fn approval_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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

        fn get(&self, topic_key: &str) -> Result<Option<String>, EngramClientError> {
            if self.fail {
                return Err(EngramClientError::Transport(
                    "fake: engram down".to_string(),
                ));
            }
            Ok(self.store.lock().unwrap().get(topic_key).cloned())
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
        // `spectty_spec` (WU-4), `spectty_approval` (WU-5), and `spectty_diff` (WU-8) now
        // have real effects; the remaining two keep the benign stub ack until their effects
        // land.
        let engram = FakeEngramClient::new();
        for name in ["spectty_status", "spectty_cost"] {
            let req = format!(
                r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"{name}","arguments":{{"session_id":"s"}}}}}}"#
            );
            let resp = response_with(&req, &engram);
            assert_eq!(resp["result"]["isError"], false, "{name} must ack");
            assert!(engram.get("spectty/s/spec").is_none());
        }
    }

    // ── M4 WU-8: spectty_diff cooperative trigger ─────────────────────────────────────

    /// WU-8.10: `spectty_diff` upserts a trigger doc to the canonical key
    /// `spectty/{session_id}/diff` and returns immediately (non-error). The app-side poll
    /// observes the change and runs the diff pipeline (bypassing the FileWatch debounce).
    #[test]
    fn spectty_diff_upserts_trigger_doc_to_canonical_key() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                 "name":"spectty_diff",
                 "arguments":{"session_id":"42","hint":"edited auth"}}}"#,
            &engram,
        );
        assert!(
            resp.get("error").is_none(),
            "a valid call is not an RPC error"
        );
        assert_eq!(resp["result"]["isError"], false);

        let stored = engram
            .get("spectty/42/diff")
            .expect("a trigger doc must be upserted under the canonical key");
        let doc: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(doc["session_id"], "42");
        assert_eq!(doc["hint"], "edited auth");
        assert!(
            doc["nonce"].as_str().is_some(),
            "a nonce makes each trigger a detectable change for the app poll"
        );
    }

    /// WU-8.10: a `spectty_diff` with no `hint` still triggers (hint is optional, advertised).
    #[test]
    fn spectty_diff_without_hint_still_triggers() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                 "name":"spectty_diff","arguments":{"session_id":"7"}}}"#,
            &engram,
        );
        assert_eq!(resp["result"]["isError"], false);
        assert!(engram.get("spectty/7/diff").is_some());
        // Only the addressed session's key is written.
        assert!(engram.get("spectty/42/diff").is_none());
    }

    /// WU-8.10: a malformed `spectty_diff` (missing `session_id`) is `-32602` with no upsert.
    #[test]
    fn spectty_diff_malformed_payload_is_rejected() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                 "name":"spectty_diff","arguments":{}}}"#,
            &engram,
        );
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    /// WU-8.10: when engram is unreachable the cooperative trigger DEGRADES to a benign
    /// error result (`isError: true`) instead of panicking — a down daemon must not break
    /// the agent's turn (spectty-diff-effect.md).
    #[test]
    fn spectty_diff_degrades_when_engram_down() {
        let engram = FakeEngramClient::failing();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
                 "name":"spectty_diff","arguments":{"session_id":"42"}}}"#,
            &engram,
        );
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
    }

    /// PINNED CROSS-SEAM (MCP → app): the trigger doc the MCP `spectty_diff` effect writes
    /// MUST be the shape the app-side diff poll consumes — a `session_id` plus a nonce that
    /// changes per call so consecutive triggers are detectable changes. We replicate the app
    /// side's change-detection (content differs ⇒ trigger) over two real MCP upserts to prove
    /// the two-sided contract: a second trigger writes a DIFFERENT doc, so the app poll fires
    /// again rather than deduping a genuine re-edit away.
    #[test]
    fn spectty_diff_consecutive_triggers_write_distinct_docs_for_app_poll() {
        let engram = FakeEngramClient::new();
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                 "name":"spectty_diff","arguments":{"session_id":"42","hint":"same hint"}}}"#;

        response_with(req, &engram);
        let first = engram.get("spectty/42/diff").expect("first trigger doc");
        // A tiny gap so the nanos-nonce advances even on a fast machine.
        std::thread::sleep(std::time::Duration::from_millis(2));
        response_with(req, &engram);
        let second = engram.get("spectty/42/diff").expect("second trigger doc");

        // Both docs deserialize and carry the session id (the app-side shape).
        let d1: Value = serde_json::from_str(&first).unwrap();
        let d2: Value = serde_json::from_str(&second).unwrap();
        assert_eq!(d1["session_id"], "42");
        assert_eq!(d2["session_id"], "42");
        assert_ne!(
            first, second,
            "consecutive triggers for the SAME hint MUST differ so the app poll re-fires the \
             pipeline on a rapid re-edit (the nonce guarantees change-detection)"
        );
    }

    // ── M4 WU-5: spectty_approval blocking long-poll resolver ─────────────────────────

    /// A fake that records upserts (so the pending registration is assertable) and, on
    /// `get`, returns a RESOLVED document for the addressed action — simulating the app's
    /// `approve_prompt` having written the resolution back concurrently. This lets the
    /// blocking long-poll resolve on its FIRST read with no real sleeping.
    struct ResolvingEngramClient {
        store: Mutex<HashMap<String, String>>,
        decision: String,
    }

    impl ResolvingEngramClient {
        fn new(decision: &str) -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
                decision: decision.to_string(),
            }
        }

        fn stored(&self, topic_key: &str) -> Option<String> {
            self.store.lock().unwrap().get(topic_key).cloned()
        }
    }

    impl EngramClient for ResolvingEngramClient {
        fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError> {
            self.store
                .lock()
                .unwrap()
                .insert(topic_key.to_string(), content.to_string());
            Ok(())
        }

        fn get(&self, topic_key: &str) -> Result<Option<String>, EngramClientError> {
            // Reflect the pending document the handler upserted, but with `resolution` set —
            // exactly what the app writes via approve_prompt.
            let pending = self.store.lock().unwrap().get(topic_key).cloned();
            Ok(pending.map(|content| {
                let mut doc: Value = serde_json::from_str(&content).expect("stored doc is JSON");
                doc["resolution"] = json!(self.decision);
                doc.to_string()
            }))
        }
    }

    /// PR-3 Finding 4: an [`EngramClient`] whose `get` fails (transport) a fixed number of
    /// times, then returns a RESOLVED document. Models a transient blip (e.g. the 2s client
    /// timeout firing once) inside an otherwise-healthy long-poll. `upsert` always succeeds so
    /// the pending registration lands.
    struct FlakyGetEngramClient {
        store: Mutex<HashMap<String, String>>,
        decision: String,
        remaining_failures: Mutex<u32>,
    }

    impl FlakyGetEngramClient {
        fn new(fail_times: u32, decision: &str) -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
                decision: decision.to_string(),
                remaining_failures: Mutex::new(fail_times),
            }
        }
    }

    impl EngramClient for FlakyGetEngramClient {
        fn upsert(&self, topic_key: &str, content: &str) -> Result<(), EngramClientError> {
            self.store
                .lock()
                .unwrap()
                .insert(topic_key.to_string(), content.to_string());
            Ok(())
        }

        fn get(&self, topic_key: &str) -> Result<Option<String>, EngramClientError> {
            {
                let mut left = self.remaining_failures.lock().unwrap();
                if *left > 0 {
                    *left -= 1;
                    return Err(EngramClientError::Transport(
                        "fake: transient blip".to_string(),
                    ));
                }
            }
            let pending = self.store.lock().unwrap().get(topic_key).cloned();
            Ok(pending.map(|content| {
                let mut doc: Value = serde_json::from_str(&content).expect("stored doc is JSON");
                doc["resolution"] = json!(self.decision);
                doc.to_string()
            }))
        }
    }

    /// PR-3 Finding 4: a couple of transient `get` failures inside the budget must NOT abort
    /// the long-poll. With a tolerance of N consecutive failures, two blips then a resolution
    /// still resolves the approval (the counter resets on the eventual success).
    #[test]
    fn spectty_approval_tolerates_transient_get_failures_then_resolves() {
        let _env = approval_env_lock().lock().unwrap();
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        std::env::set_var("SPECTTY_APPROVAL_MAX_POLLS", "10");
        // Fail twice (< the tolerance of 3), then resolve.
        let engram = FlakyGetEngramClient::new(2, "Approved");
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["approve"]}}}"#,
            &engram,
        );
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");
        std::env::remove_var("SPECTTY_APPROVAL_MAX_POLLS");

        assert!(resp.get("error").is_none());
        assert_eq!(
            resp["result"]["isError"], false,
            "transient blips inside tolerance must not abort the poll"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("Approved"),
            "the approval still resolves after transient blips: {text}"
        );
    }

    /// PR-3 Finding 4: N+ CONSECUTIVE transient failures surface the engram-unavailable error
    /// (the poll gives up only after the tolerance is exhausted, not on the first blip).
    #[test]
    fn spectty_approval_surfaces_error_after_consecutive_get_failures() {
        let _env = approval_env_lock().lock().unwrap();
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        std::env::set_var("SPECTTY_APPROVAL_MAX_POLLS", "10");
        // Fail more than the tolerance (3) consecutively → never resolves, surfaces error.
        let engram = FlakyGetEngramClient::new(50, "Approved");
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["approve"]}}}"#,
            &engram,
        );
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");
        std::env::remove_var("SPECTTY_APPROVAL_MAX_POLLS");

        assert!(resp.get("error").is_none());
        assert_eq!(
            resp["result"]["isError"], true,
            "consecutive failures beyond tolerance must surface the transport error"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("engram unavailable"),
            "exhausted tolerance reports engram unavailable: {text}"
        );
    }

    /// PR-3 Finding 4 (unit): `poll_for_resolution` tolerates transient failures up to the
    /// limit and resets the counter on a successful read. Driven directly over the seam.
    #[test]
    fn poll_for_resolution_resets_failure_counter_on_success() {
        // Fails twice (within tolerance 3), then resolves → Ok(Some).
        let engram = FlakyGetEngramClient::new(2, "Approved");
        engram
            .upsert(
                "spectty/42/approval",
                r#"{"action_id":"edit-1","options":[],"resolution":null}"#,
            )
            .unwrap();
        let got = poll_for_resolution(&engram, "spectty/42/approval", "edit-1", 10, Duration::ZERO);
        assert_eq!(got.unwrap().as_deref(), Some("Approved"));

        // Fails beyond tolerance consecutively → Err surfaces (poll aborts).
        let down = FlakyGetEngramClient::new(50, "Approved");
        down.upsert(
            "spectty/42/approval",
            r#"{"action_id":"edit-1","options":[],"resolution":null}"#,
        )
        .unwrap();
        let err = poll_for_resolution(&down, "spectty/42/approval", "edit-1", 10, Duration::ZERO);
        assert!(matches!(err, Err(EngramClientError::Transport(_))));
    }

    /// WU-5.1: a `spectty_approval` call registers EXACTLY ONE pending request keyed
    /// `(session_id, action_id)` carrying the `options` (the status path derives
    /// `quick_actions` from these). The pending doc has a null resolution.
    #[test]
    fn spectty_approval_registers_pending_with_options() {
        let _env = approval_env_lock().lock().unwrap();
        // A non-resolving fake so we can inspect the PENDING document before any resolution.
        // Use a bounded budget of 1 poll + zero interval so the handler returns promptly
        // (still pending) without sleeping.
        std::env::set_var("SPECTTY_APPROVAL_MAX_POLLS", "1");
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1",
                   "description":"rm -rf","risk_level":"high",
                   "options":["approve","reject"]}}}"#,
            &engram,
        );
        std::env::remove_var("SPECTTY_APPROVAL_MAX_POLLS");
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");

        assert!(
            resp.get("error").is_none(),
            "a valid call is not an RPC error"
        );

        let stored = engram
            .get("spectty/42/approval")
            .expect("a pending request must be registered under the canonical key");
        let doc: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(doc["action_id"], "edit-1");
        assert_eq!(doc["options"], json!(["approve", "reject"]));
        assert!(
            doc["resolution"].is_null(),
            "a freshly registered request is pending (null resolution)"
        );
    }

    /// WU-5.2: a duplicate `(session_id, action_id)` registration is idempotent — exactly one
    /// pending entry for the key (the second upsert overwrites with identical pending
    /// content).
    #[test]
    fn spectty_approval_duplicate_request_is_idempotent() {
        let _env = approval_env_lock().lock().unwrap();
        std::env::set_var("SPECTTY_APPROVAL_MAX_POLLS", "1");
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        let engram = FakeEngramClient::new();
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["a"]}}}"#;
        response_with(req, &engram);
        let first = engram.get("spectty/42/approval").unwrap();
        response_with(req, &engram);
        let second = engram.get("spectty/42/approval").unwrap();
        std::env::remove_var("SPECTTY_APPROVAL_MAX_POLLS");
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");

        assert_eq!(
            first, second,
            "a duplicate registration must leave a single, unchanged pending entry"
        );
    }

    /// WU-5.3: the blocking long-poll observes a resolution written back to the same key and
    /// returns it to the agent (non-error). The resolution is the canonical Core
    /// `ApprovalState` string the app wrote.
    #[test]
    fn spectty_approval_long_poll_returns_resolution() {
        let _env = approval_env_lock().lock().unwrap();
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        let engram = ResolvingEngramClient::new("Approved");
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["approve"]}}}"#,
            &engram,
        );
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");

        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("Approved"),
            "the resolution decision must be returned to the agent: {text}"
        );
        // The pending request was registered before the long-poll resolved.
        assert!(engram.stored("spectty/42/approval").is_some());
    }

    /// WU-5.6: the bounded long-poll returns a `pending` (timeout) result rather than hanging
    /// when no resolution ever arrives — the agent's turn ends, it does not block forever.
    #[test]
    fn spectty_approval_times_out_to_pending_without_hanging() {
        let _env = approval_env_lock().lock().unwrap();
        std::env::set_var("SPECTTY_APPROVAL_MAX_POLLS", "2");
        std::env::set_var("SPECTTY_APPROVAL_POLL_MS", "0");
        let engram = FakeEngramClient::new(); // never resolves
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["approve"]}}}"#,
            &engram,
        );
        std::env::remove_var("SPECTTY_APPROVAL_MAX_POLLS");
        std::env::remove_var("SPECTTY_APPROVAL_POLL_MS");

        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("pending"),
            "a timed-out approval reports pending: {text}"
        );
    }

    /// WU-5.6: a malformed `spectty_approval` payload (missing `action_id`) is rejected as
    /// `-32602` without registering anything — no crash.
    #[test]
    fn spectty_approval_malformed_payload_is_rejected() {
        let engram = FakeEngramClient::new();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
                 "name":"spectty_approval","arguments":{"session_id":"42"}}}"#,
            &engram,
        );
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
        assert!(engram.get("spectty/42/approval").is_none());
    }

    /// WU-5.6: when engram is unreachable the blocking tool DEGRADES to a benign error result
    /// instead of panicking — a down daemon must not break the agent's turn.
    #[test]
    fn spectty_approval_degrades_when_engram_down() {
        let engram = FakeEngramClient::failing();
        let resp = response_with(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
                 "name":"spectty_approval",
                 "arguments":{"session_id":"42","action_id":"edit-1","options":["approve"]}}}"#,
            &engram,
        );
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
    }

    /// `resolution_of` only returns a decision for the addressed action and only when
    /// resolved — a pending or mismatched document yields `None`.
    #[test]
    fn resolution_of_matches_action_and_requires_resolution() {
        let pending = r#"{"action_id":"edit-1","options":[],"resolution":null}"#;
        assert_eq!(resolution_of(pending, "edit-1"), None);

        let resolved = r#"{"action_id":"edit-1","options":[],"resolution":"Approved"}"#;
        assert_eq!(
            resolution_of(resolved, "edit-1").as_deref(),
            Some("Approved")
        );

        // A resolution for a DIFFERENT action under the same key is ignored.
        assert_eq!(resolution_of(resolved, "other"), None);

        // Garbage degrades to None (no panic).
        assert_eq!(resolution_of("{not json", "edit-1"), None);
    }

    /// PR-3 Finding 1 (MAJOR), MIRROR direction of the cross-seam contract. The Tauri
    /// `approve_prompt` serializes an `ApprovalRequest` with a `resolution` set to a canonical
    /// Core `ApprovalState` string. We replicate that EXACT Tauri-serialized resolved document
    /// as a JSON literal (src-tauri/src/commands/spec.rs `ApprovalRequest` serde shape —
    /// `risk_level` is `skip_serializing_if = "Option::is_none"`, so a resolved doc may OMIT
    /// it) and feed it into the REAL `resolution_of` parser. This proves the MCP long-poll
    /// unblocks on what the app actually writes — the seam the PR-2 blocker class missed.
    #[test]
    fn tauri_resolved_doc_unblocks_the_mcp_long_poll() {
        // EXACT shape the Tauri ApprovalRequest serializes once resolved (Approved). Note the
        // omitted `risk_level` (skip_serializing_if) and the bare Core ApprovalState string.
        let tauri_resolved_approved = r#"{"action_id":"edit-1","description":"delete the prod database","options":["approve","reject"],"resolution":"Approved"}"#;
        assert_eq!(
            resolution_of(tauri_resolved_approved, "edit-1").as_deref(),
            Some("Approved"),
            "the MCP long-poll MUST unblock on the Tauri-serialized resolved doc"
        );
        // The same doc is invisible under a different action_id (no false unblock).
        assert_eq!(resolution_of(tauri_resolved_approved, "other"), None);

        // The Adjusted variant must also unblock the long-poll with the canonical string.
        let tauri_resolved_adjusted = r#"{"action_id":"plan-1","description":"steer the plan","options":["adjust"],"resolution":"Adjusted"}"#;
        assert_eq!(
            resolution_of(tauri_resolved_adjusted, "plan-1").as_deref(),
            Some("Adjusted"),
            "the MCP long-poll MUST unblock on an Adjusted resolution"
        );

        // A still-pending Tauri doc (resolution:null, risk_level present) keeps the poll blocked.
        let tauri_pending = r#"{"action_id":"edit-1","description":"d","risk_level":"high","options":[],"resolution":null}"#;
        assert_eq!(resolution_of(tauri_pending, "edit-1"), None);
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
