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

/// One hook command Spectty registers under a `hooks.<EventName>` array.
///
/// Serializes to the Claude Code hook shape:
/// `{ "type": "command", "command": "<spectty-hook path>", "args": ["--event","<Name>"] }`
/// The `type` field is always `"command"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommandEntry {
    /// The command to launch (the `spectty-hook` binary path).
    pub command: String,
    /// Arguments passed to the command (e.g. `["--event", "Stop"]`).
    pub args: Vec<String>,
}

impl HookCommandEntry {
    /// Render this entry as the hook-list element Claude Code expects:
    /// `{ "hooks": [ { "type": "command", "command": "...", "args": [...] } ] }`
    ///
    /// An optional `matcher` string is inserted when `matcher` is `Some`.
    fn to_hook_list_element(&self, matcher: Option<&str>) -> Value {
        let mut inner = Map::new();
        inner.insert("type".to_string(), Value::String("command".to_string()));
        inner.insert("command".to_string(), Value::String(self.command.clone()));
        inner.insert(
            "args".to_string(),
            Value::Array(self.args.iter().cloned().map(Value::String).collect()),
        );
        let mut outer = Map::new();
        if let Some(m) = matcher {
            outer.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        outer.insert(
            "hooks".to_string(),
            Value::Array(vec![Value::Object(inner)]),
        );
        Value::Object(outer)
    }
}

/// Inject (or replace) ONLY the Spectty-owned rows in `hooks.<EventName>[]`, leaving
/// every foreign hook entry and every foreign top-level key intact. Idempotent:
/// injecting twice with the same entry yields the same document.
///
/// `events` maps `EventName` → `(HookCommandEntry, Option<matcher>)`. The owned rows
/// are identified by the inner `hooks[].command` field equalling `entry.command` (the
/// Spectty hook binary path), so a user's own hook on the same event survives.
///
/// If `hooks` is missing it is created. The root MUST be a JSON object; a non-object
/// document is a [`ProvisioningError::Parse`].
///
/// **R7 GENERALIZED (D21)**: settings.json `hooks` is `EventName → [{ matcher?, hooks:
/// [{type, command, args}] }]` — more nested than `mcpServers`. The owned-key predicate
/// changes: we own ROWS whose inner `hooks[].command` == our sidecar path, not a named
/// key. Retract removes only those rows, leaving foreign rows + empty event arrays intact.
///
/// **Inner-granularity removal (C1 fix)**: when the "remove owned rows" step runs, it
/// removes individual inner `hooks[]` entries whose `command == entry.command`, then
/// drops an outer element only when its `hooks` array becomes empty afterward. A foreign
/// command co-located in the same outer element MUST survive with its order preserved.
pub fn inject_spectty_hooks(
    current_json: &str,
    events: &[(String, HookCommandEntry, Option<String>)],
) -> Result<String, ProvisioningError> {
    let mut root = parse_root_object(current_json)?;

    let hooks_map = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks_map = hooks_map
        .as_object_mut()
        .ok_or_else(|| ProvisioningError::Parse("`hooks` is not an object".to_string()))?;

    for (event_name, entry, matcher) in events {
        let event_array = hooks_map
            .entry(event_name.clone())
            .or_insert_with(|| Value::Array(vec![]));
        let event_array = event_array.as_array_mut().ok_or_else(|| {
            ProvisioningError::Parse(format!("`hooks.{event_name}` is not an array"))
        })?;

        // Remove any existing Spectty-owned inner commands at fine-grained level,
        // then drop the outer element only if its hooks[] array becomes empty.
        // This preserves foreign commands co-located in the same matcher group.
        remove_inner_spectty_commands(event_array, &entry.command);

        // Append the new Spectty row (always as its own outer element).
        event_array.push(entry.to_hook_list_element(matcher.as_deref()));
    }

    serialize_pretty(&Value::Object(root))
}

/// Retract ONLY Spectty-owned rows from every `hooks.<EventName>[]`, leaving every
/// foreign hook entry intact. Idempotent: retracting absent rows is a no-op.
///
/// Spectty owns individual inner `hooks[]` entries whose `command` equals
/// `hook_command`. An outer element is removed only when ALL of its inner entries
/// are owned by Spectty (i.e. its `hooks[]` array becomes empty after removal).
/// A foreign command co-located in the same outer element MUST survive.
pub fn retract_spectty_hooks(
    current_json: &str,
    hook_command: &str,
) -> Result<String, ProvisioningError> {
    let mut root = parse_root_object(current_json)?;

    if let Some(hooks_map) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for event_array in hooks_map.values_mut() {
            if let Some(arr) = event_array.as_array_mut() {
                remove_inner_spectty_commands(arr, hook_command);
            }
        }
    }

    serialize_pretty(&Value::Object(root))
}

/// Remove Spectty-owned inner commands at fine-grained granularity.
///
/// For each outer element in `event_array`:
/// 1. Remove inner `hooks[]` entries whose `command == hook_command`.
/// 2. If the outer element's `hooks[]` array is now EMPTY, remove the outer element.
///
/// Foreign commands co-located in the same outer element survive with order preserved.
fn remove_inner_spectty_commands(event_array: &mut Vec<Value>, hook_command: &str) {
    // Mutate each outer element's inner `hooks[]` in-place, then drop elements
    // whose inner array became empty.
    for element in event_array.iter_mut() {
        if let Some(inner_hooks) = element.get_mut("hooks").and_then(Value::as_array_mut) {
            inner_hooks.retain(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c != hook_command)
                    .unwrap_or(true) // keep entries without a command field
            });
        }
    }
    // Drop outer elements whose hooks[] became empty.
    event_array.retain(|element| {
        element
            .get("hooks")
            .and_then(Value::as_array)
            .map(|h| !h.is_empty())
            .unwrap_or(true) // keep elements without a hooks field (shouldn't exist but be safe)
    });
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

    // ── WU-2: inject_spectty_hooks / retract_spectty_hooks ───────────────────

    /// Managed `HookCommandEntry` used in all WU-2 tests: a fictitious spectty-hook
    /// binary path with a `--event Stop` argument.
    fn hook_entry_stop() -> HookCommandEntry {
        HookCommandEntry {
            command: "/usr/local/bin/spectty-hook".to_string(),
            args: vec!["--event".to_string(), "Stop".to_string()],
        }
    }

    fn hook_entry_submit() -> HookCommandEntry {
        HookCommandEntry {
            command: "/usr/local/bin/spectty-hook".to_string(),
            args: vec!["--event".to_string(), "Submit".to_string()],
        }
    }

    /// A HAND-FORMATTED settings.json with:
    /// - a `permissions` key (foreign top-level)
    /// - an `env` key (foreign top-level)
    /// - a `model` key (foreign top-level)
    /// - a user-authored hook on `Stop` (same event Spectty manages — MUST SURVIVE)
    /// - a user-authored hook on `PreToolUse` (different event — MUST SURVIVE)
    ///
    /// This is the realistic shape of `~/.claude/settings.json` and is the fixture
    /// for the HEADLINE R7 generalized test.
    fn hand_formatted_settings() -> &'static str {
        r#"{
    "model": "claude-opus-4-5",
    "permissions": {
        "allow": ["Bash"],
        "deny": []
    },
    "env": {
        "MY_VAR": "hello"
    },
    "hooks": {
        "Stop": [
            {
                "matcher": "error",
                "hooks": [
                    { "type": "command", "command": "/usr/local/bin/user-notify", "args": ["--on-error"] }
                ]
            }
        ],
        "PreToolUse": [
            {
                "hooks": [
                    { "type": "command", "command": "/usr/bin/logger", "args": ["-t", "claude"] }
                ]
            }
        ]
    }
}"#
    }

    /// THE HEADLINE R7 GENERALIZED TEST.
    ///
    /// Starts from genuinely HAND-FORMATTED `settings.json` carrying foreign
    /// top-level keys (`permissions`, `env`, `model`) AND a foreign user hook on
    /// the SAME event (`Stop`) that Spectty manages. After inject→retract:
    ///
    /// 1. No Spectty rows remain.
    /// 2. Every foreign top-level key keeps its VALUE.
    /// 3. The foreign `Stop` hook (user-authored, different command path) survives
    ///    both the inject AND the retract — value + position preserved.
    /// 4. The foreign `PreToolUse` hook survives.
    /// 5. With `preserve_order`, the original relative key ORDER is preserved
    ///    (asserted against RAW serialized text, not re-parsed Values, so an
    ///    alphabetical re-sort is caught).
    ///
    /// This is the ONLY test that proves the "foreign hook on same event" survival
    /// property — the central correctness invariant of R7 generalized.
    #[test]
    fn hooks_inject_then_retract_foreign_hook_on_same_event_survives() {
        let original = hand_formatted_settings();
        let original_value: Value = serde_json::from_str(original).expect("valid input");

        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let injected = inject_spectty_hooks(original, &events).expect("inject ok");
        let injected_value: Value = serde_json::from_str(&injected).expect("valid after inject");

        // After inject: the Spectty row is present in Stop[].
        let stop_hooks = injected_value["hooks"]["Stop"]
            .as_array()
            .expect("Stop is array");
        let has_spectty = stop_hooks.iter().any(|el| {
            el.get("hooks")
                .and_then(Value::as_array)
                .map(|h| {
                    h.iter()
                        .any(|inner| inner["command"] == "/usr/local/bin/spectty-hook")
                })
                .unwrap_or(false)
        });
        assert!(has_spectty, "Spectty row added to Stop after inject");

        // After inject: the foreign user Stop row SURVIVES with its value intact.
        let foreign_stop_survives = stop_hooks.iter().any(|el| {
            el.get("hooks")
                .and_then(Value::as_array)
                .map(|h| {
                    h.iter()
                        .any(|inner| inner["command"] == "/usr/local/bin/user-notify")
                })
                .unwrap_or(false)
        });
        assert!(
            foreign_stop_survives,
            "foreign user hook on Stop survives inject (same event, different command)"
        );

        // Retract.
        let retracted =
            retract_spectty_hooks(&injected, "/usr/local/bin/spectty-hook").expect("retract ok");
        let retracted_value: Value = serde_json::from_str(&retracted).expect("valid after retract");

        // No Spectty row remains after retract.
        let spectty_remains = retracted_value["hooks"]["Stop"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|el| {
                    el.get("hooks")
                        .and_then(Value::as_array)
                        .map(|h| {
                            h.iter()
                                .any(|inner| inner["command"] == "/usr/local/bin/spectty-hook")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(!spectty_remains, "no Spectty row remains after retract");

        // The foreign Stop hook is back, value unchanged.
        let foreign_after_retract = retracted_value["hooks"]["Stop"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|el| {
                    el.get("hooks")
                        .and_then(Value::as_array)
                        .map(|h| {
                            h.iter()
                                .any(|inner| inner["command"] == "/usr/local/bin/user-notify")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(
            foreign_after_retract,
            "foreign user hook on Stop survives retract"
        );

        // Foreign Stop hook VALUE matches the original.
        assert_eq!(
            retracted_value["hooks"]["Stop"], original_value["hooks"]["Stop"],
            "foreign Stop hook value round-trips byte-meaningfully"
        );

        // Foreign top-level keys keep their values.
        for key in ["model", "permissions", "env"] {
            assert_eq!(
                retracted_value[key], original_value[key],
                "foreign top-level value preserved: {key}"
            );
        }

        // PreToolUse hook survives.
        assert_eq!(
            retracted_value["hooks"]["PreToolUse"], original_value["hooks"]["PreToolUse"],
            "foreign PreToolUse hook survives"
        );

        // With preserve_order, top-level key order is preserved (TEXT comparison).
        let top = ["model", "permissions", "env", "hooks"];
        assert_eq!(
            text_key_order(&retracted, &top),
            text_key_order(original, &top),
            "top-level key TEXT order unchanged after inject→retract"
        );
    }

    #[test]
    fn hooks_inject_is_idempotent() {
        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let once = inject_spectty_hooks("{}", &events).expect("inject 1");
        let twice = inject_spectty_hooks(&once, &events).expect("inject 2");
        let once_value: Value = serde_json::from_str(&once).expect("valid");
        let twice_value: Value = serde_json::from_str(&twice).expect("valid");
        // Idempotent: exact same Stop array length (one Spectty row).
        assert_eq!(
            once_value["hooks"]["Stop"].as_array().map(|a| a.len()),
            twice_value["hooks"]["Stop"].as_array().map(|a| a.len()),
            "double inject results in same number of Stop rows (idempotent)"
        );
    }

    #[test]
    fn hooks_inject_into_empty_document_creates_hooks_section() {
        let events = vec![
            ("Stop".to_string(), hook_entry_stop(), None),
            ("UserPromptSubmit".to_string(), hook_entry_submit(), None),
        ];
        let injected = inject_spectty_hooks("{}", &events).expect("inject into empty");
        let parsed: Value = serde_json::from_str(&injected).expect("valid JSON");

        assert!(
            parsed["hooks"]["Stop"].as_array().is_some(),
            "Stop event created"
        );
        assert!(
            parsed["hooks"]["UserPromptSubmit"].as_array().is_some(),
            "UserPromptSubmit event created"
        );
    }

    #[test]
    fn hooks_retract_when_absent_is_a_noop() {
        // A settings.json with no hooks key: retract is a no-op.
        let no_hooks = serde_json::to_string_pretty(&serde_json::json!({
            "model": "claude-opus-4-5"
        }))
        .expect("fixture");

        let retracted = retract_spectty_hooks(&no_hooks, "/usr/local/bin/spectty-hook")
            .expect("retract absent");
        // Value must be unchanged (modulo canonical reserialize).
        let orig_v: Value = serde_json::from_str(&no_hooks).expect("valid");
        let ret_v: Value = serde_json::from_str(&retracted).expect("valid");
        assert_eq!(orig_v, ret_v, "retract with no hooks key is a no-op");
    }

    #[test]
    fn hooks_non_object_hooks_key_is_a_parse_error_not_data_loss() {
        let config = r#"{ "hooks": [] }"#;
        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let err = inject_spectty_hooks(config, &events).expect_err("must error");
        assert!(
            matches!(err, ProvisioningError::Parse(_)),
            "non-object hooks surfaces a Parse error instead of clobbering it"
        );
    }

    #[test]
    fn hooks_no_matcher_entry_has_no_matcher_field() {
        // A Stop hook entry (no matcher) MUST NOT contain a `matcher` field (absent, not null).
        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let injected = inject_spectty_hooks("{}", &events).expect("inject");
        let parsed: Value = serde_json::from_str(&injected).expect("valid JSON");

        let stop_entry = &parsed["hooks"]["Stop"][0];
        assert!(
            stop_entry.get("matcher").is_none(),
            "no-matcher event MUST NOT have a matcher field; got: {stop_entry}"
        );
    }

    #[test]
    fn hooks_matcher_entry_has_matcher_field() {
        // A Notification hook entry (permission-prompt matcher) MUST contain a `matcher` field.
        let entry = HookCommandEntry {
            command: "/usr/local/bin/spectty-hook".to_string(),
            args: vec!["--event".to_string(), "Permission".to_string()],
        };
        let matcher = Some("Do you want to proceed".to_string());
        let events = vec![("Notification".to_string(), entry, matcher)];
        let injected = inject_spectty_hooks("{}", &events).expect("inject");
        let parsed: Value = serde_json::from_str(&injected).expect("valid JSON");

        let notif_entry = &parsed["hooks"]["Notification"][0];
        assert!(
            notif_entry.get("matcher").is_some(),
            "Notification event MUST have a matcher field; got: {notif_entry}"
        );
        assert!(
            !notif_entry["matcher"].as_str().unwrap_or("").is_empty(),
            "matcher field must be non-empty"
        );
    }

    #[test]
    fn hooks_inject_invalid_json_is_a_parse_error_not_a_panic() {
        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let err = inject_spectty_hooks("{not json", &events).expect_err("must error");
        assert!(matches!(err, ProvisioningError::Parse(_)));
    }

    // ── C1 RED TEST: retract must operate at inner-command granularity ─────────
    //
    // When a user's notifier and the spectty-hook share ONE outer matcher-group
    // element (same `hooks[]` array), retract MUST remove ONLY the inner entry
    // whose command == spectty path, and MUST NOT delete the outer element.
    // The user's notifier command MUST survive with its value intact.
    //
    // This test is RED against the CURRENT code because `is_spectty_hook_element`
    // returns true for the whole outer element (any inner command matches), and
    // `retain(!is_spectty_hook_element)` then drops the ENTIRE element.
    #[test]
    fn retract_removes_only_inner_spectty_command_keeps_foreign_co_located_in_same_group() {
        // Config: ONE outer element under Stop containing two inner hook commands —
        // a foreign user-notify AND the spectty-hook co-located in the same `hooks[]`.
        let config = serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "error",
                        "hooks": [
                            { "type": "command", "command": "/usr/local/bin/user-notify", "args": ["--on-error"] },
                            { "type": "command", "command": "/usr/local/bin/spectty-hook", "args": ["--event", "Stop"] }
                        ]
                    }
                ]
            }
        }))
        .expect("fixture serializes");

        let retracted =
            retract_spectty_hooks(&config, "/usr/local/bin/spectty-hook").expect("retract ok");
        let retracted_value: Value = serde_json::from_str(&retracted).expect("valid after retract");

        // The outer element MUST NOT be deleted — Stop array still has one element.
        let stop_arr = retracted_value["hooks"]["Stop"]
            .as_array()
            .expect("Stop must still be an array");
        assert_eq!(
            stop_arr.len(),
            1,
            "outer element must survive (hooks array has foreign command); got: {stop_arr:?}"
        );

        // The foreign user-notify command MUST still exist inside that element.
        let foreign_survives = stop_arr.iter().any(|el| {
            el.get("hooks")
                .and_then(Value::as_array)
                .map(|h| {
                    h.iter().any(|inner| {
                        inner
                            .get("command")
                            .and_then(Value::as_str)
                            .map(|c| c == "/usr/local/bin/user-notify")
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });
        assert!(
            foreign_survives,
            "foreign user-notify command MUST survive retract (co-located in same group)"
        );

        // The spectty-hook command MUST be gone from the inner hooks[].
        let spectty_gone = stop_arr.iter().all(|el| {
            el.get("hooks")
                .and_then(Value::as_array)
                .map(|h| {
                    h.iter().all(|inner| {
                        inner
                            .get("command")
                            .and_then(Value::as_str)
                            .map(|c| c != "/usr/local/bin/spectty-hook")
                            .unwrap_or(true)
                    })
                })
                .unwrap_or(true)
        });
        assert!(
            spectty_gone,
            "spectty-hook inner command MUST be removed after retract"
        );
    }

    // Also verify inject's "remove owned rows first" step uses inner-granularity
    // so re-inject on a co-located group is idempotent and never strips foreign cmd.
    #[test]
    fn inject_retains_foreign_co_located_command_when_reinserting_spectty() {
        // Start with a group that already has user-notify + spectty-hook together.
        let config = serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "error",
                        "hooks": [
                            { "type": "command", "command": "/usr/local/bin/user-notify", "args": [] },
                            { "type": "command", "command": "/usr/local/bin/spectty-hook", "args": ["--event", "Stop"] }
                        ]
                    }
                ]
            }
        }))
        .expect("fixture serializes");

        let events = vec![("Stop".to_string(), hook_entry_stop(), None)];
        let injected = inject_spectty_hooks(&config, &events).expect("inject ok");
        let injected_value: Value = serde_json::from_str(&injected).expect("valid after inject");

        // Foreign user-notify must still exist after re-inject.
        let stop_arr = injected_value["hooks"]["Stop"]
            .as_array()
            .expect("Stop must be array");
        let notify_survives = stop_arr.iter().any(|el| {
            el.get("hooks")
                .and_then(Value::as_array)
                .map(|h| {
                    h.iter().any(|inner| {
                        inner
                            .get("command")
                            .and_then(Value::as_str)
                            .map(|c| c == "/usr/local/bin/user-notify")
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });
        assert!(
            notify_survives,
            "foreign user-notify must survive re-inject on co-located group"
        );
    }
}
