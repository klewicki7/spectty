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

use std::sync::{Arc, Mutex};
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
    ///
    /// # Invariant: `updated_at` MUST be monotonic-per-change
    ///
    /// This comparator uses strict ordering (`>`), so it is ONLY correct when the
    /// supplied `updated_at` token is monotonically non-decreasing across genuine
    /// changes — i.e. a later change always yields a `updated_at` that sorts strictly
    /// above the previous one. That holds for engram's real `updated_at`
    /// (`"YYYY-MM-DD HH:MM:SS"`, lexicographically monotonic) and for the synthetic
    /// monotonic counter that [`PortPollReader`] emits.
    ///
    /// Do NOT feed this a content hash directly: a hash is NOT monotonic across content
    /// changes, so any change whose new hash sorts below the previous one would be
    /// silently swallowed (and an A→B→A revert would be lost). [`PortPollReader`] owns
    /// its own EQUALITY-based change detection precisely to uphold this invariant.
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
/// payload `String` (no `updated_at`), this reader does its OWN equality-based change
/// detection: it remembers the last payload's content hash and, when the current hash
/// DIFFERS, bumps a monotonic revision counter and reports that counter as the synthetic
/// `updated_at`. Identical payloads keep the previous counter (no re-emit downstream).
///
/// # Why equality, not ordering
///
/// A content hash is NOT monotonic across content changes, so a hash CANNOT be fed
/// directly into [`SpecBus::decide`]'s strict `>`-comparison: a change whose new hash
/// sorts below the previous one (e.g. an approval flip `Pending → Approved`) — or any
/// A→B→A revert — would be silently swallowed. By detecting change with EQUALITY here
/// (`Some(h) != last`) and emitting a genuinely monotonic counter, we both preserve the
/// D28 "emit only on change" contract AND uphold `decide()`'s monotonic-token invariant.
///
/// The richer engram-native reader (real `updated_at`) is wired in WU-4 where the
/// adapter's HTTP seam is available; this content-hash reader is the port-only fallback
/// and the default wiring for `InMemoryPersistenceAdapter`-backed sessions.
pub struct PortPollReader {
    port: Arc<dyn PersistencePort>,
    // Equality-based change state. `read(&self)` is shared (`&self`), so interior
    // mutability is required; the poll loop is single-threaded per topic so a plain
    // `Mutex` is sufficient and contention-free.
    state: Mutex<PortReaderState>,
}

/// Last-seen content hash + the monotonic revision counter handed downstream as the
/// synthetic `updated_at`.
#[derive(Default)]
struct PortReaderState {
    last_content_hash: Option<u64>,
    revision: u64,
}

impl PortPollReader {
    /// Build a reader over `port`.
    pub fn new(port: Arc<dyn PersistencePort>) -> Self {
        Self {
            port,
            state: Mutex::new(PortReaderState::default()),
        }
    }
}

impl PollReader for PortPollReader {
    fn read(&self, topic_key: &str) -> Result<Option<Obs>, PersistenceError> {
        let Some(content) = self.port.get(topic_key)? else {
            return Ok(None);
        };
        let hash = content_hash(&content);

        let mut state = self.state.lock().expect("port reader state poisoned");
        // EQUALITY check: only a genuinely different payload bumps the revision. This
        // is what makes reverts and non-monotonic-hash flips emit correctly — the
        // monotonic counter (never the hash) is what reaches `decide()`'s `>` compare.
        if state.last_content_hash != Some(hash) {
            state.last_content_hash = Some(hash);
            state.revision += 1;
        }
        // Zero-pad so the synthetic token compares consistently by width, matching the
        // lexicographic shape engram's real `updated_at` uses.
        let token = format!("{:020}", state.revision);

        Ok(Some(Obs {
            content,
            updated_at: token,
        }))
    }
}

/// Stable content hash for a payload string (port-only fallback change detection). Used
/// ONLY for equality comparison inside [`PortPollReader`] — never as an ordering key.
fn content_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    content.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ── Finding 1 (BLOCKER) RED → GREEN: the port-only reader must change-detect by
    //    INEQUALITY, never by lexicographic ORDERING. A content hash is NOT monotonic
    //    across content changes, so feeding it into `decide()`'s `>`-comparison silently
    //    swallows any change whose new hash sorts below the previous one. ────────────

    /// The exact real-payload pair that proved the defect: a plan-approval flip from
    /// `"Pending"` to `"Approved"`. `hash("...Approved")` sorts BELOW `hash("...Pending")`,
    /// so an ordering comparator would never emit the flip. Equality MUST emit it.
    #[test]
    fn port_poll_reader_emits_on_approval_pending_to_approved_flip() {
        use spectty_adapters::InMemoryPersistenceAdapter;

        let adapter = Arc::new(InMemoryPersistenceAdapter::new());
        let port: Arc<dyn PersistencePort> = adapter.clone();
        let reader: Arc<dyn PollReader> = Arc::new(PortPollReader::new(port.clone()));
        let mut bus = SpecBus::new(reader, "spectty/s/spec");

        let mut emitted: Vec<Change> = Vec::new();

        let pending = r#"{"approval":"Pending"}"#;
        let approved = r#"{"approval":"Approved"}"#;
        // Sanity: this pair is exactly the non-monotonic case (new hash < old hash).
        // An ordering comparator fed the raw hash would swallow the flip; equality won't.
        assert!(
            content_hash(approved) < content_hash(pending),
            "test premise: hash(Approved) must sort below hash(Pending) — the defect case"
        );

        port.upsert("spectty/s/spec", pending.to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        assert_eq!(emitted.len(), 1, "first payload (Pending) must emit");

        port.upsert("spectty/s/spec", approved.to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        assert_eq!(
            emitted.len(),
            2,
            "the Pending -> Approved flip MUST emit even though hash(Approved) < hash(Pending)"
        );
        assert_eq!(emitted[1].content, approved);
    }

    /// An A -> B -> A revert: every transition is a real change and MUST emit, even
    /// though returning to A re-uses a previously-seen token (ordering would swallow it).
    #[test]
    fn port_poll_reader_emits_on_a_b_a_revert() {
        use spectty_adapters::InMemoryPersistenceAdapter;

        let adapter = Arc::new(InMemoryPersistenceAdapter::new());
        let port: Arc<dyn PersistencePort> = adapter.clone();
        let reader: Arc<dyn PollReader> = Arc::new(PortPollReader::new(port.clone()));
        let mut bus = SpecBus::new(reader, "spectty/s/spec");

        let mut emitted: Vec<Change> = Vec::new();

        port.upsert("spectty/s/spec", "A".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        port.upsert("spectty/s/spec", "B".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        port.upsert("spectty/s/spec", "A".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));

        assert_eq!(
            emitted.len(),
            3,
            "A->B->A: every change must emit (revert to A is still a change)"
        );
        assert_eq!(emitted[2].content, "A");
    }

    /// Re-writing the IDENTICAL payload must NOT emit (idempotent upsert, no change).
    #[test]
    fn port_poll_reader_does_not_emit_on_identical_rewrite() {
        use spectty_adapters::InMemoryPersistenceAdapter;

        let adapter = Arc::new(InMemoryPersistenceAdapter::new());
        let port: Arc<dyn PersistencePort> = adapter.clone();
        let reader: Arc<dyn PollReader> = Arc::new(PortPollReader::new(port.clone()));
        let mut bus = SpecBus::new(reader, "spectty/s/spec");

        let mut emitted: Vec<Change> = Vec::new();

        port.upsert("spectty/s/spec", "same".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        // Re-write identical content several times.
        port.upsert("spectty/s/spec", "same".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));
        port.upsert("spectty/s/spec", "same".to_string()).unwrap();
        bus.poll(&mut |c| emitted.push(c));

        assert_eq!(
            emitted.len(),
            1,
            "identical content must emit exactly once (no re-emit on rewrite)"
        );
    }
}
