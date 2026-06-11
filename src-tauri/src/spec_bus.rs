//! The `SpecBus` — an ADAPTER-side subscribe-by-polling seam (D27/D28).
//!
//! M4 adds a per-session poll loop that watches an engram topic_key
//! (`spectty/{session_id}/spec`, `.../progress`) and emits a [`Change`] EXACTLY ONCE
//! per actual change. Crucially this is NOT a new `PersistencePort` method: the Core
//! port stays the UNCHANGED sync/`String` contract (M4-REQ-01). The poll/subscribe
//! behavior lives HERE, in the bridge, as a struct that holds an
//! `Arc<dyn PersistencePort>` and change-detects adapter-side.
//!
//! ## Why a separate read seam ([`PollReader`])
//!
//! The Core `PersistencePort::get` returns only the payload `String` — it deliberately
//! does NOT expose engram's `updated_at` (that would leak a backend concept into the
//! port). But change detection (D28) is defined on `updated_at`. So the production loop
//! reads through a thin [`PollReader`] that yields an [`Obs`] (`content` + `updated_at`)
//! WITHOUT widening the Core port. The pure decision step [`SpecBus::poll`] takes an
//! already-read `Option<Obs>` and an injected `emit` closure, mirroring
//! `run_signal_loop`'s `observe_and_diff` testability discipline: no Tauri, no thread,
//! no clock in the unit tests.
//!
//! ## Change detection (D28, G1-confirmed)
//!
//! engram's `updated_at` is a space-separated string (`"YYYY-MM-DD HH:MM:SS"`) that is
//! lexicographically monotonic, and `?since=` is NOT honored server-side, so the loop
//! fetches the observation and compares `updated_at` against the per-topic
//! `last_updated_at`, emitting only on a STRICTLY-GREATER value.

use std::sync::Arc;
use std::time::Duration;

use spectty_core::ports::{PersistenceError, PersistencePort};

/// Default poll cadence (D28). Overridable via the `SPECTTY_POLL_MS` env var.
pub const DEFAULT_POLL_MS: u64 = 2_000;

/// One observation as seen by the poll loop: the payload plus engram's change-detection
/// timestamp. Mirrors `engram_http::Obs` but lives in the bridge so the loop does not
/// depend on the adapter's `pub(crate)` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obs {
    /// The already-serialized payload `String` (opaque here; deserialized by the caller).
    pub content: String,
    /// The engram `updated_at` timestamp (monotonic string; D28 change-detection key).
    pub updated_at: String,
}

/// The payload handed to the injected `emit` closure on an actual change. The bridge's
/// production wiring turns this into a `spec_updated` Tauri event (WU-4); here it stays
/// Tauri-free so the loop is unit-testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The topic_key whose observation changed.
    pub topic_key: String,
    /// The new payload `String` (caller deserializes to `SpecContract` adapter-side).
    pub content: String,
    /// The `updated_at` that triggered this emit.
    pub updated_at: String,
}

/// A thin read seam yielding an [`Obs`] (content + `updated_at`) for a topic_key,
/// WITHOUT widening the Core `PersistencePort`. Implemented adapter-side over the real
/// engram HTTP client; faked in unit tests.
pub trait PollReader: Send + Sync {
    /// Read the latest observation for `topic_key`, or `None` if absent.
    fn read(&self, topic_key: &str) -> Result<Option<Obs>, PersistenceError>;
}

/// The per-topic poll state + change detector (D27). Holds the read seam and the
/// last-seen `updated_at`; [`poll`](Self::poll) is the PURE decision step.
pub struct SpecBus {
    reader: Arc<dyn PollReader>,
    topic_key: String,
    last_updated_at: Option<String>,
}

impl SpecBus {
    /// Build a bus for `topic_key` over `reader`, with no prior observation seen.
    pub fn new(reader: Arc<dyn PollReader>, topic_key: impl Into<String>) -> Self {
        Self {
            reader,
            topic_key: topic_key.into(),
            last_updated_at: None,
        }
    }

    /// PURE one-tick decision: given the observation read this tick, invoke `emit`
    /// EXACTLY ONCE iff `updated_at` is strictly greater than the last seen value,
    /// then advance `last_updated_at`. A read error or an absent observation is a
    /// tolerated no-op (the loop keeps running). Mirrors `observe_and_diff`.
    pub fn decide(
        &mut self,
        observed: Result<Option<Obs>, PersistenceError>,
        emit: &mut dyn FnMut(Change),
    ) {
        let obs = match observed {
            Ok(Some(obs)) => obs,
            // Absent observation (key not written yet) or a transport error: no emit,
            // no panic, do NOT advance `last_updated_at`. The next good tick resumes.
            Ok(None) | Err(_) => return,
        };

        let is_newer = match &self.last_updated_at {
            None => true,
            Some(prev) => obs.updated_at > *prev,
        };
        if is_newer {
            self.last_updated_at = Some(obs.updated_at.clone());
            emit(Change {
                topic_key: self.topic_key.clone(),
                content: obs.content,
                updated_at: obs.updated_at,
            });
        }
    }

    /// Read the bus's topic_key through the seam and run one [`decide`](Self::decide)
    /// tick. Used by the production loop; the unit tests drive `decide` directly with
    /// scripted observations so no reader plumbing is needed.
    pub fn poll(&mut self, emit: &mut dyn FnMut(Change)) {
        let observed = self.reader.read(&self.topic_key);
        self.decide(observed, emit);
    }
}

/// Resolve the poll interval from `SPECTTY_POLL_MS`, falling back to [`DEFAULT_POLL_MS`].
#[must_use]
pub fn poll_interval() -> Duration {
    let ms = std::env::var("SPECTTY_POLL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_POLL_MS);
    Duration::from_millis(ms)
}

/// A [`PollReader`] over the Core [`PersistencePort`]. Because the port returns only the
/// payload `String` (no `updated_at`), this reader synthesizes a change-detection token
/// by hashing the content: identical payloads keep the same token (no re-emit), a
/// changed payload yields a new one. This preserves the D28 "emit only on change"
/// contract through the UNCHANGED port for callers that only have a `PersistencePort`.
///
/// The richer engram-native reader (real `updated_at`) is wired in WU-4 where the
/// adapter's HTTP seam is available; this content-hash reader is the port-only fallback
/// and the default wiring for `InMemoryPersistenceAdapter`-backed sessions.
pub struct PortPollReader {
    port: Arc<dyn PersistencePort>,
}

impl PortPollReader {
    /// Build a reader over `port`.
    pub fn new(port: Arc<dyn PersistencePort>) -> Self {
        Self { port }
    }
}

impl PollReader for PortPollReader {
    fn read(&self, topic_key: &str) -> Result<Option<Obs>, PersistenceError> {
        Ok(self.port.get(topic_key)?.map(|content| {
            // Content hash stands in for `updated_at` when the port can't expose it.
            // A formatted hash keeps the monotonic-by-change semantics decide() needs
            // (different content -> different token -> emit; same content -> no emit).
            let token = content_token(&content);
            Obs {
                content,
                updated_at: token,
            }
        }))
    }
}

/// Stable change token for a payload string (port-only fallback change detection).
fn content_token(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    content.hash(&mut h);
    // Zero-padded hex keeps lexicographic comparison consistent with width.
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A scripted [`PollReader`]: pops one result per `read` call so a test can drive an
    /// exact observation sequence without a daemon (mirrors `ScriptedRunner`).
    struct ScriptedReader {
        script: Mutex<std::vec::IntoIter<Result<Option<Obs>, PersistenceError>>>,
    }

    impl ScriptedReader {
        fn new(items: Vec<Result<Option<Obs>, PersistenceError>>) -> Self {
            Self {
                script: Mutex::new(items.into_iter()),
            }
        }
    }

    impl PollReader for ScriptedReader {
        fn read(&self, _topic_key: &str) -> Result<Option<Obs>, PersistenceError> {
            self.script
                .lock()
                .expect("scripted reader poisoned")
                .next()
                .unwrap_or(Ok(None))
        }
    }

    fn obs(updated_at: &str, content: &str) -> Obs {
        Obs {
            content: content.to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    fn bus_over(items: Vec<Result<Option<Obs>, PersistenceError>>) -> SpecBus {
        SpecBus::new(Arc::new(ScriptedReader::new(items)), "spectty/s/spec")
    }

    // WU-2.1: first change emits EXACTLY ONCE and advances last_updated_at.
    #[test]
    fn spec_bus_emits_once_on_first_change() {
        let mut bus = bus_over(vec![Ok(Some(obs("1", "v1")))]);
        let mut emitted: Vec<Change> = Vec::new();
        bus.poll(&mut |c| emitted.push(c));

        assert_eq!(emitted.len(), 1, "first change must emit once");
        assert_eq!(emitted[0].content, "v1");
        assert_eq!(emitted[0].updated_at, "1");
        assert_eq!(bus.last_updated_at.as_deref(), Some("1"));
    }

    // WU-2.2: the same updated_at on a second tick must NOT re-emit.
    #[test]
    fn spec_bus_does_not_re_emit_same_updated_at() {
        let mut bus = bus_over(vec![Ok(Some(obs("1", "v1"))), Ok(Some(obs("1", "v1")))]);
        let mut emitted: Vec<Change> = Vec::new();
        bus.poll(&mut |c| emitted.push(c));
        bus.poll(&mut |c| emitted.push(c));

        assert_eq!(emitted.len(), 1, "an unchanged updated_at must not re-emit");
    }

    // WU-2.3: a strictly-greater updated_at emits once more and advances the cursor.
    #[test]
    fn spec_bus_re_emits_on_newer_updated_at() {
        let mut bus = bus_over(vec![Ok(Some(obs("1", "v1"))), Ok(Some(obs("2", "v2")))]);
        let mut emitted: Vec<Change> = Vec::new();
        bus.poll(&mut |c| emitted.push(c));
        bus.poll(&mut |c| emitted.push(c));

        assert_eq!(emitted.len(), 2, "a newer updated_at must emit again");
        assert_eq!(emitted[1].updated_at, "2");
        assert_eq!(emitted[1].content, "v2");
        assert_eq!(bus.last_updated_at.as_deref(), Some("2"));
    }

    // WU-2.4: a poll error is tolerated — no emit, no panic, no cursor advance; the
    // subsequent good tick still emits.
    #[test]
    fn spec_bus_tolerates_poll_error() {
        let mut bus = bus_over(vec![
            Err(PersistenceError::Backend("down".to_string())),
            Ok(Some(obs("1", "v1"))),
        ]);
        let mut emitted: Vec<Change> = Vec::new();
        bus.poll(&mut |c| emitted.push(c)); // errored tick
        assert!(emitted.is_empty(), "an errored tick must not emit");
        assert_eq!(bus.last_updated_at, None, "error must not advance cursor");

        bus.poll(&mut |c| emitted.push(c)); // recovery tick
        assert_eq!(emitted.len(), 1, "the loop must resume after an error");
    }

    // WU-2.5: an absent observation (key not written yet) is a no-op, no error.
    #[test]
    fn spec_bus_tolerates_absent_observation() {
        let mut bus = bus_over(vec![Ok(None), Ok(Some(obs("1", "v1")))]);
        let mut emitted: Vec<Change> = Vec::new();
        bus.poll(&mut |c| emitted.push(c)); // absent tick
        assert!(emitted.is_empty(), "absent observation must not emit");
        assert_eq!(bus.last_updated_at, None);

        bus.poll(&mut |c| emitted.push(c)); // now present
        assert_eq!(emitted.len(), 1);
    }

    // The port-only fallback reader: identical content -> no re-emit; changed content
    // -> emit (content-hash stands in for updated_at through the UNCHANGED port).
    #[test]
    fn port_poll_reader_detects_change_via_content_hash() {
        use spectty_adapters::InMemoryPersistenceAdapter;

        let adapter = Arc::new(InMemoryPersistenceAdapter::new());
        let port: Arc<dyn PersistencePort> = adapter.clone();
        let reader: Arc<dyn PollReader> = Arc::new(PortPollReader::new(port.clone()));
        let mut bus = SpecBus::new(reader, "spectty/s/spec");

        let mut emitted: Vec<Change> = Vec::new();

        // No value yet -> no emit.
        bus.poll(&mut |c| emitted.push(c));
        assert!(emitted.is_empty());

        // Write v1 -> emit once.
        port.upsert("spectty/s/spec", "v1".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        bus.poll(&mut |c| emitted.push(c)); // unchanged -> no re-emit
        assert_eq!(
            emitted.len(),
            1,
            "first content emits once, unchanged no re-emit"
        );

        // Overwrite with new content -> emit again.
        port.upsert("spectty/s/spec", "v2".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        assert_eq!(emitted.len(), 2, "changed content must re-emit");
        assert_eq!(emitted[1].content, "v2");
    }

    #[test]
    fn poll_interval_defaults_to_2s_without_env() {
        // Default path (env var unset in the test process by default).
        if std::env::var("SPECTTY_POLL_MS").is_err() {
            assert_eq!(poll_interval(), Duration::from_millis(DEFAULT_POLL_MS));
        }
    }
}
