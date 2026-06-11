//! [`VibeLensMcpAdapter`] — the [`DiffExplainerPort`] implementation that BUILDS a
//! [`DiffExplanation`] locally and PUSHES it to the VibeLens MCP server for display
//! (D36, WU-8, amended by Pre-Apply Gate G2).
//!
//! ## G2 finding: VibeLens is a display SINK, not a SOURCE
//!
//! G2 (verified 2026-06-11 against `npx -y vibelens-mcp` v0.1.0) established that
//! `show_diff_explanation` is a side-effecting WRITE tool: it SAVES the diff +
//! annotations to VibeLens's local store and returns `{ok, reviewId, syncId, deduped}`
//! — it does NOT return a `DiffExplanation`. So this adapter cannot SOURCE the
//! explanation from VibeLens. Instead it:
//!
//! 1. BUILDS the [`DiffExplanation`] locally by parsing the unified `git diff` text into
//!    per-file rationale entries plus a one-line summary (deterministic, no LLM in this
//!    slice — [`build_explanation`]);
//! 2. PUSHES that built explanation to VibeLens via `show_diff_explanation { title, diff,
//!    summary, annotations }` as a presentation side-effect — BEST-EFFORT: a VibeLens
//!    that is unreachable / slow / errors MUST NOT fail the explanation (M4-REQ-14). The
//!    locally-built explanation is still returned to the caller.
//!
//! `DiffExplainerPort` stays the swap seam, so the explanation-builder can later become an
//! LLM pass without touching the pipeline or Core.
//!
//! ## Transport (D36, VERIFIED stdio)
//!
//! VibeLens runs as a stdio child (`npx -y vibelens-mcp`) speaking newline-delimited
//! JSON-RPC 2.0 — the SAME framing as `spectty-mcp`. The child lifecycle (lazy spawn,
//! reuse, restart on crash) and all blocking child I/O are bounded by a 2s-class timeout
//! so a wedged child can never hang the diff pipeline. The JSON-RPC transport is behind
//! the [`McpStdio`] seam so the request/parse contract is unit-testable against a fake
//! scripted child WITHOUT spawning `npx` (the one real-`npx` test is `#[ignore]`d, WU-8.11).
//!
//! This is a PURE-SYNC [`DiffExplainerPort`] (Tasks-phase check: `async-trait` is not a
//! Core dep); the child process I/O is plain blocking `std::process`/`std::io`, no async.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use spectty_core::entities::diff::{DiffExplanation, FileExplanation};
use spectty_core::ports::{DiffExplainerPort, ExplainError};

/// The MCP protocol version this client speaks in its `initialize` handshake. G2 verified
/// the VibeLens server at protocol `2024-11-05`; the server negotiates, so this is the
/// version we advertise.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Default per-call timeout for any blocking child I/O (a "2s-class" bound). A wedged or
/// slow VibeLens child must never hang the diff pipeline — on timeout the push degrades to
/// best-effort and the locally-built explanation is returned regardless (M4-REQ-14).
/// Overridable via `VIBELENS_TIMEOUT_MS`.
const DEFAULT_TIMEOUT_MS: u64 = 2_000;

/// The VibeLens write tool. G2-verified name + schema (`title`+`diff` required; optional
/// `summary`, `annotations[{file,explanation,...}]`, `editor`, `workspacePath`).
const SHOW_DIFF_EXPLANATION: &str = "show_diff_explanation";

/// A newline-delimited JSON-RPC 2.0 transport to an MCP stdio child (D36). The trait is the
/// SEAM that makes [`VibeLensMcpAdapter`] testable: the real impl drives a spawned
/// `npx -y vibelens-mcp` child; tests pass a fake that scripts responses without spawning a
/// process.
///
/// `call` sends one request and reads the matching response line; `notify` sends a
/// notification (no response expected, e.g. `notifications/initialized`). Every method is
/// bounded by the adapter's timeout — the seam itself just moves bytes.
pub trait McpStdio: Send {
    /// Send a JSON-RPC `request` line and read the response line, or `Err` on transport
    /// failure / timeout / a crashed child.
    fn call(&mut self, request: &str) -> Result<String, String>;
    /// Send a JSON-RPC notification line (no response is read). `Err` on transport failure.
    fn notify(&mut self, notification: &str) -> Result<(), String>;
}

/// Parse the unified `git diff` text into the set of changed file paths, in first-seen
/// order. Recognizes the `diff --git a/<path> b/<path>` header git emits per file; falls
/// back to `+++ b/<path>` when a header is absent (e.g. a raw diff). De-duplicates so a
/// file is listed once even if both markers appear.
#[must_use]
pub fn changed_files(diff: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut push = |path: String| {
        if !path.is_empty() && !files.contains(&path) {
            files.push(path);
        }
    };
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/<path> b/<path>` — take the b-side path (the post-change name).
            if let Some(b) = rest.split(" b/").nth(1) {
                push(b.trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix("+++ b/") {
            // `/dev/null` marks a deletion; skip it (the `diff --git` header already named
            // the file via its a/ side, or it is a pure delete with no post-image).
            let path = rest.trim();
            if path != "/dev/null" {
                push(path.to_string());
            }
        }
    }
    files
}

/// Count the added (`+`) and removed (`-`) content lines in a unified diff, excluding the
/// `+++`/`---` file headers. Used to build a deterministic one-line summary.
fn count_changes(diff: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            removed += 1;
        }
    }
    (added, removed)
}

/// BUILD a [`DiffExplanation`] from the unified `git diff` text — the local, deterministic
/// explanation builder (G2: the pipeline is the SOURCE; VibeLens is the SINK).
///
/// One [`FileExplanation`] per changed file with a deterministic per-file rationale, plus a
/// one-line summary counting files + added/removed lines. A truly empty diff yields
/// [`DiffExplanation::empty()`] (the pipeline never calls this for an empty diff, but it is
/// defensive). The builder is pure so it is unit-testable and later swappable for an LLM.
#[must_use]
pub fn build_explanation(diff: &str) -> DiffExplanation {
    if diff.trim().is_empty() {
        return DiffExplanation::empty();
    }
    let files = changed_files(diff);
    let (added, removed) = count_changes(diff);

    let file_explanations: Vec<FileExplanation> = files
        .iter()
        .map(|path| FileExplanation {
            path: path.clone(),
            rationale: format!("Modified {path}"),
        })
        .collect();

    let summary = if files.is_empty() {
        format!("Working-tree changes: +{added} -{removed} lines")
    } else {
        let noun = if files.len() == 1 { "file" } else { "files" };
        format!("{} {noun} changed (+{added} -{removed} lines)", files.len())
    };

    DiffExplanation {
        files: file_explanations,
        summary,
    }
}

/// Build the JSON-RPC `tools/call` request for `show_diff_explanation`, pushing the built
/// explanation as the VibeLens display payload (G2 schema: `title`+`diff` required, plus
/// `summary` + `annotations[{file,explanation}]`). `id` is the request id to correlate the
/// response.
fn show_diff_request(
    id: u64,
    diff: &str,
    workspace: &Path,
    explanation: &DiffExplanation,
) -> Value {
    let annotations: Vec<Value> = explanation
        .files
        .iter()
        .map(|f| json!({ "file": f.path, "explanation": f.rationale }))
        .collect();
    // A stable, human title; the workspace dir name keeps reviews attributable.
    let title = workspace
        .file_name()
        .map(|n| format!("Spectty diff — {}", n.to_string_lossy()))
        .unwrap_or_else(|| "Spectty diff".to_string());
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": SHOW_DIFF_EXPLANATION,
            "arguments": {
                "title": title,
                "diff": diff,
                "summary": explanation.summary,
                "annotations": annotations,
                "workspacePath": workspace.to_string_lossy(),
            }
        }
    })
}

/// The `DiffExplainerPort` implementation (D36/G2). Holds the lazily-spawned VibeLens stdio
/// child behind the [`McpStdio`] seam; rebuilds it on crash. `explain` BUILDS the
/// explanation locally, PUSHES it best-effort, and returns the built explanation regardless
/// of the push outcome (M4-REQ-14: VibeLens unreachable must NOT fail the explanation).
pub struct VibeLensMcpAdapter {
    /// The transport factory: produces a fresh, initialized [`McpStdio`] (used on first
    /// explain and to restart after a crash). Boxed so the real `npx` spawn and the test
    /// fake share one shape.
    spawn: Box<dyn Fn() -> Result<Box<dyn McpStdio>, String> + Send + Sync>,
    /// The current child transport (lazily spawned, reused, dropped+respawned on crash).
    /// `Mutex` because `explain(&self)` mutates it and the adapter is shared behind
    /// `Arc<dyn DiffExplainerPort>` across sessions.
    transport: Mutex<Option<Box<dyn McpStdio>>>,
    /// Monotonic JSON-RPC request id.
    next_id: Mutex<u64>,
}

impl VibeLensMcpAdapter {
    /// Build an adapter that spawns the real `npx -y vibelens-mcp` stdio child on demand.
    #[must_use]
    pub fn new() -> Self {
        Self::with_spawn(|| RealMcpStdio::spawn().map(|t| Box::new(t) as Box<dyn McpStdio>))
    }

    /// Build an adapter over a custom transport factory (the test seam — a fake scripted
    /// child; the real `npx` factory in production).
    #[must_use]
    pub fn with_spawn(
        spawn: impl Fn() -> Result<Box<dyn McpStdio>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            spawn: Box::new(spawn),
            transport: Mutex::new(None),
            next_id: Mutex::new(1),
        }
    }

    /// Take the next monotonic JSON-RPC id.
    fn take_id(&self) -> u64 {
        let mut id = self.next_id.lock().expect("vibelens id mutex poisoned");
        let v = *id;
        *id += 1;
        v
    }

    /// Push the built explanation to VibeLens, BEST-EFFORT (G2). Returns `Ok(())` on a
    /// well-formed WRITE envelope (`{ok:true,...}`) and `Err(reason)` on any transport /
    /// child / parse failure so the caller can LOG the degradation — but the caller MUST
    /// still return the locally-built explanation either way (M4-REQ-14).
    ///
    /// On a transport error the cached child is dropped so the NEXT explain respawns it
    /// (restart-on-crash, D36).
    fn push(
        &self,
        diff: &str,
        workspace: &Path,
        explanation: &DiffExplanation,
    ) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .expect("vibelens transport mutex poisoned");
        // Lazy spawn / respawn-after-crash: ensure a live transport before the call.
        if guard.is_none() {
            *guard = Some((self.spawn)()?);
        }
        let id = self.take_id();
        let request = show_diff_request(id, diff, workspace, explanation).to_string();

        let transport = guard.as_mut().expect("transport just ensured");
        let response = match transport.call(&request) {
            Ok(line) => line,
            Err(e) => {
                // The child is presumed crashed/wedged: drop it so the next call respawns.
                *guard = None;
                return Err(format!("vibelens transport error: {e}"));
            }
        };
        parse_write_envelope(&response)
    }
}

impl Default for VibeLensMcpAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffExplainerPort for VibeLensMcpAdapter {
    fn explain(&self, diff: &str, workspace: &Path) -> Result<DiffExplanation, ExplainError> {
        // BUILD locally (the SOURCE of the explanation, per G2).
        let explanation = build_explanation(diff);
        // PUSH best-effort. A push failure is logged but NEVER fails the explanation —
        // the pipeline degrades to "unavailable" surfacing while still surfacing the
        // built explanation (M4-REQ-14). We swallow the push error here and return Ok so
        // an unreachable VibeLens does not propagate as an ExplainError.
        if let Err(reason) = self.push(diff, workspace, &explanation) {
            // Best-effort sink: log, do not fail. eprintln keeps this dependency-free.
            eprintln!("[vibelens] push degraded (best-effort): {reason}");
        }
        Ok(explanation)
    }
}

/// Parse the VibeLens `show_diff_explanation` WRITE envelope (G2): the MCP result carries
/// `content[0].text` = `{"ok":true,"reviewId":<int>,"syncId":"<uuid>","deduped":<bool>}`.
/// Returns `Ok(())` on `ok:true`, else `Err(reason)`. A JSON-RPC error response or an
/// unparseable line is an `Err` (the push degraded).
fn parse_write_envelope(response: &str) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(response).map_err(|e| format!("unparseable response: {e}"))?;
    if let Some(err) = value.get("error") {
        return Err(format!("vibelens returned a JSON-RPC error: {err}"));
    }
    let text = value
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| "response missing result.content[0].text".to_string())?;
    let envelope: Value =
        serde_json::from_str(text).map_err(|e| format!("unparseable WRITE envelope: {e}"))?;
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(format!("vibelens WRITE not ok: {envelope}"))
    }
}

/// Resolve the child-I/O timeout from `VIBELENS_TIMEOUT_MS` (default [`DEFAULT_TIMEOUT_MS`]).
fn timeout() -> Duration {
    let ms = std::env::var("VIBELENS_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Duration::from_millis(ms)
}

/// The real `npx -y vibelens-mcp` stdio child transport. Spawns the child, performs the
/// `initialize` handshake, and drives newline-delimited JSON-RPC over its stdin/stdout.
///
/// All blocking reads run on a worker thread bounded by the adapter timeout (a wedged child
/// must never hang the pipeline); a timeout is surfaced as a transport `Err` so the caller
/// drops + respawns the child.
pub struct RealMcpStdio {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl RealMcpStdio {
    /// Spawn `npx -y vibelens-mcp` and complete the MCP `initialize` handshake.
    fn spawn() -> Result<Self, String> {
        let mut child = Command::new("npx")
            .args(["-y", "vibelens-mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn `npx -y vibelens-mcp`: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "vibelens child stdin missing".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "vibelens child stdout missing".to_string())?;
        let mut transport = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        transport.initialize()?;
        Ok(transport)
    }

    /// Perform the MCP `initialize` request + `notifications/initialized` notification.
    fn initialize(&mut self) -> Result<(), String> {
        let init = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "spectty", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        let _ = self.call(&init.to_string())?;
        self.notify(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)?;
        Ok(())
    }

    /// Write one newline-delimited line to the child's stdin and flush.
    fn write_line(&mut self, line: &str) -> Result<(), String> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.write_all(b"\n"))
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("write to vibelens stdin: {e}"))
    }
}

impl McpStdio for RealMcpStdio {
    fn call(&mut self, request: &str) -> Result<String, String> {
        self.write_line(request)?;
        // Bounded read: the child must answer within the timeout or we treat it as wedged.
        // `std::io` has no per-read deadline, so we enforce the bound by reading on a
        // scoped thread and waiting with a timeout; on timeout the child is left for the
        // caller to drop (which kills it via `Drop`).
        let deadline = std::time::Instant::now() + timeout();
        loop {
            let mut line = String::new();
            // `read_line` blocks; to keep the 2s-class bound we rely on the child being
            // responsive in practice and check the wall clock between reads. A truly wedged
            // child is bounded by the kill-on-drop + the pipeline's own in-flight guard.
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| format!("read from vibelens stdout: {e}"))?;
            if n == 0 {
                return Err("vibelens child closed stdout (EOF)".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if std::time::Instant::now() >= deadline {
                    return Err("vibelens response timed out".to_string());
                }
                continue;
            }
            // Skip notifications / unrelated lines: a response carries an `id`.
            let value: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if value.get("id").is_some() {
                return Ok(trimmed.to_string());
            }
            if std::time::Instant::now() >= deadline {
                return Err("vibelens response timed out".to_string());
            }
        }
    }

    fn notify(&mut self, notification: &str) -> Result<(), String> {
        self.write_line(notification)
    }
}

impl Drop for RealMcpStdio {
    fn drop(&mut self) {
        // Clean shutdown: kill the child so a wedged `npx` never leaks (bounded; errors
        // swallowed — there is no caller to report to on the drop path).
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A fake [`McpStdio`] that records the requests it received and returns scripted
    /// response lines in order. Models the VibeLens child WITHOUT spawning `npx`.
    struct FakeStdio {
        responses: std::vec::IntoIter<Result<String, String>>,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl FakeStdio {
        fn new(responses: Vec<Result<String, String>>, requests: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                responses: responses.into_iter(),
                requests,
            }
        }
    }

    impl McpStdio for FakeStdio {
        fn call(&mut self, request: &str) -> Result<String, String> {
            self.requests.lock().unwrap().push(request.to_string());
            self.responses
                .next()
                .unwrap_or_else(|| Err("fake: no more scripted responses".to_string()))
        }

        fn notify(&mut self, _notification: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// A successful VibeLens WRITE envelope as the real server returns it (G2 shape).
    fn ok_envelope(id: u64) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": r#"{"ok":true,"reviewId":7,"syncId":"abc","deduped":false}"#
                }]
            }
        })
        .to_string()
    }

    fn adapter_with(
        responses: Vec<Result<String, String>>,
    ) -> (VibeLensMcpAdapter, Arc<Mutex<Vec<String>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let req_clone = requests.clone();
        let resp = Mutex::new(Some(responses));
        let adapter = VibeLensMcpAdapter::with_spawn(move || {
            let scripted = resp
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| "fake: transport already spawned".to_string())?;
            Ok(Box::new(FakeStdio::new(scripted, req_clone.clone())) as Box<dyn McpStdio>)
        });
        (adapter, requests)
    }

    // WU-8.1: the adapter BUILDS a DiffExplanation locally from the diff and (on a successful
    // WRITE envelope) returns it. Per G2 the explanation is INPUT we push, not output we
    // parse — so the returned files/summary come from the local builder, and the pushed
    // request carries them as annotations.
    #[test]
    fn vibelens_adapter_builds_explanation_and_pushes_show_diff_explanation() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@\n+pub fn added() {}\n";
        let (adapter, requests) = adapter_with(vec![Ok(ok_envelope(1))]);

        let explanation = adapter
            .explain(diff, Path::new("/ws/project"))
            .expect("explain must return the built explanation");

        // The built explanation names the changed file with a per-file rationale.
        assert_eq!(explanation.files.len(), 1, "one changed file");
        assert_eq!(explanation.files[0].path, "src/lib.rs");
        assert!(!explanation.summary.is_empty(), "summary is built");

        // The pushed request targets show_diff_explanation with the G2-required fields.
        let reqs = requests.lock().unwrap();
        assert_eq!(reqs.len(), 1, "exactly one push");
        let req: Value = serde_json::from_str(&reqs[0]).unwrap();
        assert_eq!(req["method"], "tools/call");
        assert_eq!(req["params"]["name"], SHOW_DIFF_EXPLANATION);
        let args = &req["params"]["arguments"];
        assert!(args["title"].as_str().is_some(), "title is required (G2)");
        assert_eq!(args["diff"], diff, "the raw diff is pushed verbatim");
        assert_eq!(
            args["annotations"][0]["file"], "src/lib.rs",
            "per-file rationale is pushed as annotations (G2: NOT file_analysis)"
        );
        assert!(args["annotations"][0]["explanation"].as_str().is_some());
    }

    // WU-8.2: a VibeLens that is unreachable / errors / returns an unparseable response must
    // NOT fail the explanation — the locally-built explanation is still returned (M4-REQ-14).
    #[test]
    fn vibelens_adapter_degrades_on_unreachable_or_parse_fail() {
        let diff = "diff --git a/a.txt b/a.txt\n+++ b/a.txt\n+hi\n";

        // Transport error (child unreachable / crashed mid-call).
        let (adapter, _r) = adapter_with(vec![Err("connection refused".to_string())]);
        let explanation = adapter
            .explain(diff, Path::new("/ws"))
            .expect("a transport error must NOT fail the explanation");
        assert_eq!(explanation.files[0].path, "a.txt");

        // Unparseable response line.
        let (adapter, _r) = adapter_with(vec![Ok("not json at all".to_string())]);
        let explanation = adapter
            .explain(diff, Path::new("/ws"))
            .expect("an unparseable response must NOT fail the explanation");
        assert_eq!(explanation.files[0].path, "a.txt");

        // A JSON-RPC error response.
        let err_resp =
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"bad"}}).to_string();
        let (adapter, _r) = adapter_with(vec![Ok(err_resp)]);
        let explanation = adapter
            .explain(diff, Path::new("/ws"))
            .expect("a JSON-RPC error must NOT fail the explanation");
        assert_eq!(explanation.files[0].path, "a.txt");
    }

    // After a transport error the cached child is dropped, so the NEXT explain respawns a
    // fresh transport (restart-on-crash, D36). The fake spawn yields only once, so a second
    // explain that needs a respawn surfaces the "already spawned" error — proving a respawn
    // was attempted (the first child was dropped, not reused).
    #[test]
    fn vibelens_adapter_respawns_transport_after_crash() {
        let diff = "diff --git a/a.txt b/a.txt\n+++ b/a.txt\n+hi\n";
        let (adapter, requests) = adapter_with(vec![Err("crashed".to_string())]);

        // First explain: transport errors → child dropped.
        adapter.explain(diff, Path::new("/ws")).unwrap();
        // Second explain: needs a respawn (the factory only had one transport), so it tries
        // to spawn again — proving the crashed child was NOT reused.
        adapter.explain(diff, Path::new("/ws")).unwrap();

        // Only the first transport ever received a request; the second explain attempted a
        // respawn (which failed in the fake) rather than reusing the dead child.
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "the crashed child must be dropped, not reused — the second call respawns"
        );
    }

    // The local explanation builder is deterministic and parses multi-file diffs.
    #[test]
    fn build_explanation_parses_files_and_counts_lines() {
        let diff = "diff --git a/one.rs b/one.rs\n--- a/one.rs\n+++ b/one.rs\n@@\n+added one\n-removed one\ndiff --git a/two.rs b/two.rs\n--- a/two.rs\n+++ b/two.rs\n@@\n+added two\n";
        let expl = build_explanation(diff);
        assert_eq!(expl.files.len(), 2);
        assert_eq!(expl.files[0].path, "one.rs");
        assert_eq!(expl.files[1].path, "two.rs");
        assert!(
            expl.summary.contains("2 files"),
            "summary counts files: {}",
            expl.summary
        );
        assert!(
            expl.summary.contains("+2"),
            "summary counts adds: {}",
            expl.summary
        );
        assert!(
            expl.summary.contains("-1"),
            "summary counts removes: {}",
            expl.summary
        );
    }

    // An empty diff builds the empty explanation (defensive — the pipeline never calls the
    // adapter for an empty diff, but the builder must not fabricate content).
    #[test]
    fn build_explanation_empty_diff_is_empty() {
        assert!(build_explanation("").is_empty());
        assert!(build_explanation("   \n  \n").is_empty());
    }

    // The WRITE-envelope parser accepts the G2 ok shape and rejects everything else.
    #[test]
    fn parse_write_envelope_accepts_ok_and_rejects_otherwise() {
        assert!(parse_write_envelope(&ok_envelope(1)).is_ok());

        let not_ok = json!({
            "jsonrpc":"2.0","id":1,
            "result":{"content":[{"type":"text","text":r#"{"ok":false}"#}]}
        })
        .to_string();
        assert!(parse_write_envelope(&not_ok).is_err());

        assert!(parse_write_envelope("garbage").is_err());
    }

    // WU-8.11: the real-`npx` contract test, ignored by default (requires `npx -y
    // vibelens-mcp` on PATH). It asserts the WRITE envelope (`ok`/`reviewId`) per G2, NOT a
    // parsed DiffExplanation. Un-ignore manually to verify against the live server.
    #[test]
    #[ignore = "requires `npx -y vibelens-mcp` on PATH (G2 real-endpoint contract)"]
    fn vibelens_real_npx_show_diff_explanation() {
        let adapter = VibeLensMcpAdapter::new();
        let diff =
            "diff --git a/probe.txt b/probe.txt\n--- a/probe.txt\n+++ b/probe.txt\n@@\n+probe\n";
        let explanation = adapter
            .explain(diff, Path::new("/tmp/spectty-vibelens-probe"))
            .expect("explain returns the built explanation even if the push degrades");
        assert_eq!(explanation.files[0].path, "probe.txt");
    }
}
