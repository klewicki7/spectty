//! The PURE `String -> String` JSON managed-namespace editor (D17, R7).
//!
//! Spectty registers its MCP tools by editing the agent's config JSON in place.
//! The HEADLINE invariant (R7): the editor owns ONLY the managed `spectty` key
//! under `mcpServers` — every FOREIGN key (other users' MCP servers, gentle-ai
//! entries, unrelated top-level keys) round-trips with its VALUE and (with
//! `serde_json`'s `preserve_order` feature, enabled for this crate) its relative
//! ORDER intact. Inject only ADDS the managed key; retract only REMOVES it.
//!
//! What this is NOT: byte-identity. `~/.claude.json` is machine-managed JSON, so
//! the contract is VALUE + ORDER preservation, not text preservation. We parse to
//! [`serde_json::Value`] and re-serialize with the standard pretty formatter, which
//! NORMALIZES whitespace/indentation and re-renders inline objects across lines.
//! A hand-formatted input therefore comes back reflowed (canonical 2-space pretty)
//! even though every foreign key/value/order survives. True byte-identity would
//! require a structural text editor that mutates the document in place; the design
//! (D17) deliberately rejected that — text markers corrupt `~/.claude.json` (one
//! big nested JSON doc) and `claude mcp add` is neither atomic nor testable.
//!
//! These functions are PURE: they take the current file text + the desired entry
//! and return the new text. No I/O — the impure shell is the [`ConfigFile`](super::file_io::ConfigFile)
//! seam.

use serde_json::{Map, Value};
use spectty_core::ProvisioningError;

/// The `mcpServers` entry Spectty writes for its managed server.
///
/// Serializes to `{ "command": "...", "args": [...], "env": { ... } }` — the exact
/// Claude Code MCP stdio entry shape. `env` is a sorted `Vec<(String, String)>`
/// so the managed entry serializes deterministically (the same input always yields
/// the same `spectty` sub-object).
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

/// Serialize with `serde_json`'s pretty formatter. With the crate's `preserve_order`
/// feature this keeps foreign keys in their original ORDER; whitespace/indentation
/// is still NORMALIZED to canonical 2-space pretty (not byte-identical to arbitrary
/// hand-formatted input — see the module docs).
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
    /// already in the editor's CANONICAL pretty form (built via `to_string_pretty`).
    /// Used only by the round-trip-STABILITY test below — NOT a fixture for proving
    /// foreign-key preservation on arbitrary input.
    fn config_in_canonical_form() -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "numStartups": 7,
            "mcpServers": {
                "user-tool": { "command": "user", "args": [], "env": {} },
                "gentle-ai": { "command": "gentle", "args": ["x"], "env": { "K": "V" } }
            }
        }))
        .expect("fixture serializes")
    }

    /// A HAND-FORMATTED config (NOT built via `to_string_pretty`): deliberately
    /// non-alphabetical top-level key order, 4-space indent, and inline objects.
    /// This is the realistic shape of a user's `~/.claude.json` and is what exposes
    /// reordering bugs.
    fn hand_formatted_config() -> &'static str {
        r#"{
    "theme": "dark",
    "numStartups": 7,
    "projects": {
        "/home/u/repo": { "lastUsed": "yesterday" }
    },
    "mcpServers": {
        "user-tool": { "command": "user", "args": [], "env": {} },
        "gentle-ai": { "command": "gentle", "args": ["x"], "env": { "K": "V" } }
    }
}"#
    }

    /// The order in which `keys` first APPEAR in the raw `json` TEXT. This reads the
    /// serialized string directly — NOT re-parsed `Value`s — so it actually detects
    /// reordering. (Re-parsing both sides would hide an alphabetical re-sort: a
    /// `BTreeMap`-backed `Value` sorts BOTH sides identically, making a naive
    /// parse-and-compare tautological — the exact flaw of the old headline test.)
    fn text_key_order(json: &str, keys: &[&str]) -> Vec<String> {
        let mut found: Vec<(usize, String)> = keys
            .iter()
            .filter_map(|k| {
                let needle = format!("\"{k}\"");
                json.find(&needle).map(|pos| (pos, (*k).to_string()))
            })
            .collect();
        found.sort_by_key(|(pos, _)| *pos);
        found.into_iter().map(|(_, k)| k).collect()
    }

    /// THE HONEST R7 TEST. Starts from genuinely HAND-FORMATTED input (see
    /// [`hand_formatted_config`]) and asserts that inject→retract preserves every
    /// foreign key's VALUE and (with `preserve_order`) its relative ORDER, and that
    /// no `spectty` key is left behind. It does NOT assert byte-identity — the
    /// document is reflowed by the pretty serializer, which is the documented
    /// contract.
    #[test]
    fn inject_then_retract_preserves_hand_formatted_foreign_values() {
        let original = hand_formatted_config();
        let original_value: Value = serde_json::from_str(original).expect("valid input");

        let injected = inject_spectty_mcp(original, "spectty", &entry()).expect("inject ok");
        let retracted = retract_spectty_mcp(&injected, "spectty").expect("retract ok");
        let retracted_value: Value = serde_json::from_str(&retracted).expect("valid output");

        // No managed key survives retract.
        assert!(
            retracted_value.get("spectty").is_none()
                && retracted_value["mcpServers"].get("spectty").is_none(),
            "no spectty key remains after retract"
        );

        // Every foreign top-level key keeps its VALUE.
        for key in ["theme", "numStartups", "projects"] {
            assert_eq!(
                retracted_value[key], original_value[key],
                "foreign top-level value preserved: {key}"
            );
        }

        // Every foreign mcpServers entry keeps its VALUE.
        for key in ["user-tool", "gentle-ai"] {
            assert_eq!(
                retracted_value["mcpServers"][key], original_value["mcpServers"][key],
                "foreign mcpServers value preserved: {key}"
            );
        }

        // With preserve_order, foreign keys keep their original relative ORDER.
        // Asserted against the RAW serialized TEXT (not re-parsed Values) so an
        // alphabetical re-sort is actually caught.
        let top = ["theme", "numStartups", "projects", "mcpServers"];
        assert_eq!(
            text_key_order(&retracted, &top),
            text_key_order(original, &top),
            "top-level key TEXT order unchanged after inject→retract"
        );
        let mcp = ["user-tool", "gentle-ai"];
        assert_eq!(
            text_key_order(&retracted, &mcp),
            text_key_order(original, &mcp),
            "mcpServers key TEXT order unchanged after inject→retract"
        );
    }

    /// Round-trip STABILITY of already-canonical input. This proves that feeding the
    /// editor its OWN canonical output back through inject→retract is byte-identical
    /// — it does NOT prove preservation of arbitrary hand-formatted input (that is
    /// [`inject_then_retract_preserves_hand_formatted_foreign_values`]).
    #[test]
    fn inject_then_retract_is_stable_on_already_canonical_input() {
        let original = config_in_canonical_form();

        let injected = inject_spectty_mcp(&original, "spectty", &entry()).expect("inject ok");
        assert!(injected.contains("spectty"), "managed key added");

        let retracted = retract_spectty_mcp(&injected, "spectty").expect("retract ok");
        assert_eq!(
            retracted, original,
            "canonical input round-trips byte-identical (stability, not arbitrary-input preservation)"
        );
    }

    #[test]
    fn retract_removes_only_spectty_keys() {
        let original = config_in_canonical_form();
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
        let original = config_in_canonical_form();
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
        let original = config_in_canonical_form();
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

    /// A pre-existing `mcpServers` of the WRONG shape (here a JSON array) must be a
    /// [`ProvisioningError::Parse`], NOT silently overwritten — we never destroy a
    /// foreign value, even a malformed one. Surfacing the error lets the caller
    /// decide; clobbering it would be data loss.
    #[test]
    fn non_object_mcp_servers_is_a_parse_error_not_data_loss() {
        let config = r#"{ "mcpServers": [] }"#;
        let err = inject_spectty_mcp(config, "spectty", &entry()).expect_err("must error");
        assert!(
            matches!(err, ProvisioningError::Parse(_)),
            "non-object mcpServers surfaces a Parse error instead of clobbering it"
        );
    }
}
