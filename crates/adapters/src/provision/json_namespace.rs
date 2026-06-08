//! The PURE `String -> String` JSON managed-namespace editor (D17, R7).
//!
//! Spectty registers its MCP tools by editing the agent's config JSON in place.
//! The HEADLINE invariant (R7): the editor owns ONLY the managed `spectty_*` key
//! under `mcpServers` — every FOREIGN key (other users' MCP servers, gentle-ai
//! entries, unrelated top-level keys, ordering) round-trips UNTOUCHED. An
//! inject-then-retract leaves the document byte-identical to the original (modulo
//! `serde_json`'s stable pretty formatting, which fixtures match).
//!
//! These functions are PURE: they take the current file text + the desired entry
//! and return the new text. No I/O — the impure shell is the [`ConfigFile`](super::file_io::ConfigFile)
//! seam. We parse with [`serde_json::Value`] (structural editing) rather than text
//! markers, which would corrupt `~/.claude.json` (one big nested JSON doc), and we
//! never shell out to `claude mcp add` (not atomic, not testable).

use serde_json::{Map, Value};
use spectty_core::ProvisioningError;

/// The `mcpServers` entry Spectty writes for its managed server.
///
/// Serializes to `{ "command": "...", "args": [...], "env": { ... } }` — the exact
/// Claude Code MCP stdio entry shape. `env` is a sorted `Vec<(String, String)>`
/// (deterministic serialization for byte-stable round-trip tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerEntry {
    /// The command to launch the MCP server (the `spectty-mcp` binary path).
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Sorted `(key, value)` environment pairs for deterministic serialization.
    pub env: Vec<(String, String)>,
}

impl McpServerEntry {
    /// Render this entry as the `serde_json::Value` Claude Code expects.
    fn to_value(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("command".to_string(), Value::String(self.command.clone()));
        obj.insert(
            "args".to_string(),
            Value::Array(self.args.iter().cloned().map(Value::String).collect()),
        );
        let mut env = Map::new();
        for (k, v) in &self.env {
            env.insert(k.clone(), Value::String(v.clone()));
        }
        obj.insert("env".to_string(), Value::Object(env));
        Value::Object(obj)
    }
}

/// Inject (or replace) ONLY the `<server_name>` key under `mcpServers`, leaving
/// every foreign key intact. Idempotent: injecting twice yields the same document.
///
/// If `mcpServers` is missing it is created. The root MUST be a JSON object; a
/// non-object document (or invalid JSON) is a [`ProvisioningError::Parse`].
pub fn inject_spectty_mcp(
    current_json: &str,
    server_name: &str,
    entry: &McpServerEntry,
) -> Result<String, ProvisioningError> {
    let mut root = parse_root_object(current_json)?;

    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| ProvisioningError::Parse("`mcpServers` is not an object".to_string()))?;

    servers.insert(server_name.to_string(), entry.to_value());

    serialize_pretty(&Value::Object(root))
}

/// Retract ONLY the `<server_name>` key under `mcpServers`, leaving every foreign
/// key intact. Idempotent: retracting an absent key is a no-op that still returns
/// the (re-serialized) document. An empty `mcpServers` object is left in place so
/// foreign formatting/keys are never disturbed beyond removing the managed key.
pub fn retract_spectty_mcp(
    current_json: &str,
    server_name: &str,
) -> Result<String, ProvisioningError> {
    let mut root = parse_root_object(current_json)?;

    if let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove(server_name);
    }

    serialize_pretty(&Value::Object(root))
}

/// Parse the document and require its root be a JSON object (the only valid shape
/// for a Claude Code config). Returns an owned map we can mutate.
fn parse_root_object(current_json: &str) -> Result<Map<String, Value>, ProvisioningError> {
    let value: Value =
        serde_json::from_str(current_json).map_err(|e| ProvisioningError::Parse(e.to_string()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(ProvisioningError::Parse(
            "config root is not a JSON object".to_string(),
        )),
    }
}

/// Serialize with `serde_json`'s stable pretty formatter so round-trips are
/// byte-stable across inject/retract.
fn serialize_pretty(value: &Value) -> Result<String, ProvisioningError> {
    serde_json::to_string_pretty(value).map_err(|e| ProvisioningError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The managed entry under test. `spectty` is the single managed key.
    fn entry() -> McpServerEntry {
        McpServerEntry {
            command: "/usr/local/bin/spectty-mcp".to_string(),
            args: vec!["--stdio".to_string()],
            env: vec![("SPECTTY_SESSION".to_string(), "s-1".to_string())],
        }
    }

    /// A config carrying a foreign user MCP server AND a foreign gentle-ai entry,
    /// pre-formatted to `serde_json` pretty so round-trip is byte-comparable.
    fn config_with_foreign_keys() -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "numStartups": 7,
            "mcpServers": {
                "user-tool": { "command": "user", "args": [], "env": {} },
                "gentle-ai": { "command": "gentle", "args": ["x"], "env": { "K": "V" } }
            }
        }))
        .expect("fixture serializes")
    }

    #[test]
    fn inject_then_retract_round_trips_foreign_keys_byte_identical() {
        let original = config_with_foreign_keys();

        let injected = inject_spectty_mcp(&original, "spectty", &entry()).expect("inject ok");
        // foreign keys + unrelated top-level key survive the inject
        assert!(
            injected.contains("user-tool"),
            "foreign user entry preserved"
        );
        assert!(
            injected.contains("gentle-ai"),
            "foreign gentle-ai entry preserved"
        );
        assert!(
            injected.contains("numStartups"),
            "unrelated top-level key preserved"
        );
        assert!(injected.contains("spectty"), "managed key added");

        let retracted = retract_spectty_mcp(&injected, "spectty").expect("retract ok");
        assert_eq!(
            retracted, original,
            "inject→retract is byte-identical to the original (R7 headline)"
        );
    }

    #[test]
    fn retract_removes_only_spectty_keys() {
        let original = config_with_foreign_keys();
        let injected = inject_spectty_mcp(&original, "spectty", &entry()).expect("inject");

        let retracted = retract_spectty_mcp(&injected, "spectty").expect("retract");

        assert!(!retracted.contains("spectty-mcp"), "managed entry gone");
        assert!(retracted.contains("user-tool"), "foreign user entry stays");
        assert!(
            retracted.contains("gentle-ai"),
            "foreign gentle-ai entry stays"
        );
    }

    #[test]
    fn inject_is_idempotent() {
        let original = config_with_foreign_keys();
        let once = inject_spectty_mcp(&original, "spectty", &entry()).expect("inject 1");
        let twice = inject_spectty_mcp(&once, "spectty", &entry()).expect("inject 2");
        assert_eq!(once, twice, "double inject == single inject");
    }

    #[test]
    fn inject_on_missing_mcp_servers_creates_valid_json() {
        let no_servers = serde_json::to_string_pretty(&serde_json::json!({ "numStartups": 1 }))
            .expect("fixture");

        let injected = inject_spectty_mcp(&no_servers, "spectty", &entry()).expect("inject");

        let parsed: Value = serde_json::from_str(&injected).expect("output is valid JSON");
        assert!(
            parsed["mcpServers"]["spectty"]["command"].is_string(),
            "created mcpServers object containing the managed key"
        );
        assert_eq!(
            parsed["numStartups"],
            serde_json::json!(1),
            "foreign key preserved"
        );
    }

    #[test]
    fn inject_into_empty_document_creates_mcp_servers_root() {
        let injected = inject_spectty_mcp("{}", "spectty", &entry()).expect("inject into empty");
        let parsed: Value = serde_json::from_str(&injected).expect("valid JSON");
        assert!(parsed["mcpServers"]["spectty"]["command"].is_string());
    }

    #[test]
    fn retract_when_absent_is_a_noop() {
        let original = config_with_foreign_keys();
        let retracted = retract_spectty_mcp(&original, "spectty").expect("retract absent");
        assert_eq!(
            retracted, original,
            "retracting an absent key changes nothing"
        );
    }

    #[test]
    fn invalid_json_is_a_parse_error_not_a_panic() {
        let err = inject_spectty_mcp("{not json", "spectty", &entry()).expect_err("must error");
        assert!(matches!(err, ProvisioningError::Parse(_)));
    }
}
