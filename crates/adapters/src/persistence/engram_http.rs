//! The thin HTTP seam behind [`EngramAdapter`](super::engram::EngramAdapter) (D26).
//!
//! `EngramAdapter` implements the SYNC, `String`-payload
//! [`PersistencePort`](spectty_core::ports::PersistencePort) UNCHANGED. To keep the
//! port pure and the async `reqwest` machinery out of both Core AND the port shape,
//! the network call is isolated behind [`EngramHttp`] — a small `pub(crate)` trait
//! whose ONLY real impl is [`ReqwestEngramHttp`]. The adapter owns an
//! `Arc<dyn EngramHttp>` and `block_on`-bridges its sync methods to the async client
//! on a DEDICATED Tokio runtime (never the Tauri main runtime).
//!
//! This split is what makes the slice ship green BEFORE the real `:7437` wire shapes
//! are pinned: [`FakeEngramHttp`] is an in-memory double used by the contract tests,
//! and the one real-`:7437` test stays `#[ignore]`d.
//!
//! ## G1-verified wire shapes (2026-06-11, against the running daemon)
//!
//! - Read: `GET {base}/observations` → `200` JSON array of observations; each carries
//!   `topic_key`, `content`, and `updated_at` (a string like `"2026-06-11 03:07:16"`).
//!   `?topic_key=`/`?since=` are NOT honored server-side, so we fetch the list and
//!   filter by `topic_key` CLIENT-SIDE (case-insensitively — the server lowercases it).
//! - Write: `POST {base}/observations` with `{session_id, topic_key, project, scope,
//!   content, type, title}` → `201`. The `session_id` must already exist, so we
//!   `POST {base}/sessions {id, project}` (idempotent) first.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use thiserror::Error;

/// The engram HTTP base URL (local daemon). Centralized so the real impl and any
/// future override share one source of truth.
pub const ENGRAM_BASE_URL: &str = "http://localhost:7437";

/// One observation as read back from engram, reduced to the two fields the
/// persistence/poll layer needs (G1-verified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obs {
    /// The stored payload (already-serialized `String`, opaque to this layer).
    pub content: String,
    /// The engram `updated_at` timestamp. A space-separated string
    /// (`"YYYY-MM-DD HH:MM:SS"`) that is lexicographically monotonic, so the poll
    /// loop change-detects with a plain string `>` comparison (D28 fallback — engram
    /// does not honor `?since=`).
    pub updated_at: String,
}

/// Errors from the [`EngramHttp`] transport. Distinct from
/// [`PersistenceError`](spectty_core::ports::PersistenceError): the adapter maps every
/// variant to `PersistenceError::Backend` so the port surface stays unchanged.
#[derive(Debug, Error)]
pub enum EngramHttpError {
    /// The HTTP request failed (connection refused, timeout, non-success status, ...).
    #[error("engram http transport error: {0}")]
    Transport(String),
}

/// The thin HTTP seam (D26). `pub(crate)` so it never leaks past the adapter boundary.
///
/// Sync signatures: the async `reqwest` work is bridged INSIDE `ReqwestEngramHttp`
/// (it uses `reqwest::blocking` on a dedicated runtime), keeping this trait — and the
/// `PersistencePort` it backs — free of `async-trait` and any Core dependency.
pub(crate) trait EngramHttp: Send + Sync {
    /// Create-or-update the observation under `topic_key` with `content`.
    fn post_observation(&self, topic_key: &str, content: &str) -> Result<(), EngramHttpError>;

    /// Read the latest observation for `topic_key`, or `None` if absent.
    ///
    /// `since` is accepted for forward-compatibility but currently ignored: the
    /// G1 verification showed engram does not honor `?since=`, so the poll loop
    /// change-detects on the returned [`Obs::updated_at`] instead.
    fn get_observation(
        &self,
        topic_key: &str,
        since: Option<&str>,
    ) -> Result<Option<Obs>, EngramHttpError>;
}

/// In-memory [`EngramHttp`] double for contract tests. NOT a mock — it really stores
/// payloads and round-trips them, so the adapter's mapping logic is exercised end to
/// end without a daemon. A scripted transport error models "engram down" (degrade path).
///
/// `#[cfg(test)]`-only: it exists solely for the adapter/HTTP contract tests, so it is
/// compiled out of release builds (keeping `-D dead_code` green).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeEngramHttp {
    store: Mutex<HashMap<String, Obs>>,
    /// When `true`, every call returns [`EngramHttpError::Transport`] (engram-down sim).
    fail: Mutex<bool>,
    /// Monotonic counter feeding a synthetic, strictly-increasing `updated_at`.
    tick: Mutex<u64>,
}

#[cfg(test)]
impl FakeEngramHttp {
    /// A healthy in-memory transport.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A transport that always fails — models the engram daemon being unreachable.
    pub(crate) fn failing() -> Self {
        let f = Self::default();
        *f.fail.lock().expect("fake engram fail flag poisoned") = true;
        f
    }
}

#[cfg(test)]
impl EngramHttp for FakeEngramHttp {
    fn post_observation(&self, topic_key: &str, content: &str) -> Result<(), EngramHttpError> {
        if *self.fail.lock().expect("fake engram fail flag poisoned") {
            return Err(EngramHttpError::Transport("fake: engram down".to_string()));
        }
        let mut tick = self.tick.lock().expect("fake engram tick poisoned");
        *tick += 1;
        // Zero-padded so the synthetic timestamp stays lexicographically monotonic
        // exactly like engram's real `"YYYY-MM-DD HH:MM:SS"` string.
        let updated_at = format!("2026-06-11 00:00:{:02}", *tick);
        self.store
            .lock()
            .expect("fake engram store poisoned")
            .insert(
                topic_key.to_ascii_lowercase(),
                Obs {
                    content: content.to_string(),
                    updated_at,
                },
            );
        Ok(())
    }

    fn get_observation(
        &self,
        topic_key: &str,
        _since: Option<&str>,
    ) -> Result<Option<Obs>, EngramHttpError> {
        if *self.fail.lock().expect("fake engram fail flag poisoned") {
            return Err(EngramHttpError::Transport("fake: engram down".to_string()));
        }
        Ok(self
            .store
            .lock()
            .expect("fake engram store poisoned")
            .get(&topic_key.to_ascii_lowercase())
            .cloned())
    }
}

/// The real engram HTTP client (D26, G1-verified wire shapes).
///
/// Owns an async `reqwest::Client` and a [`tokio::runtime::Handle`] to a DEDICATED
/// runtime (supplied by [`EngramAdapter`](super::engram::EngramAdapter)); each sync
/// trait method `block_on`s the async request on that runtime. This is the D26 bridge
/// from the SYNC `PersistencePort` to async `reqwest` WITHOUT touching the Tauri main
/// runtime. Lives in ADAPTERS only — Core never sees `reqwest`/`tokio`.
pub(crate) struct ReqwestEngramHttp {
    base_url: String,
    project: String,
    client: reqwest::Client,
    rt: tokio::runtime::Handle,
}

impl ReqwestEngramHttp {
    /// Build a client against `base_url` (e.g. [`ENGRAM_BASE_URL`]) for `project`,
    /// bridging async requests onto the dedicated runtime behind `rt`.
    pub(crate) fn new(
        base_url: impl Into<String>,
        project: impl Into<String>,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            project: project.into(),
            client: reqwest::Client::new(),
            rt,
        }
    }

    /// Ensure the engram session row exists before posting an observation. Engram's
    /// `POST /sessions` is idempotent (INSERT-OR-IGNORE), so calling it per upsert is
    /// safe; a non-success here surfaces as a transport error like any other.
    async fn ensure_session(&self, session_id: &str) -> Result<(), EngramHttpError> {
        let resp = self
            .client
            .post(format!("{}/sessions", self.base_url))
            .json(&serde_json::json!({ "id": session_id, "project": self.project }))
            .send()
            .await
            .map_err(|e| EngramHttpError::Transport(e.to_string()))?;
        // 200/201 = created or already-present (idempotent). Anything else is a fault.
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(EngramHttpError::Transport(format!(
                "POST /sessions returned {}",
                resp.status()
            )))
        }
    }

    async fn post_observation_async(
        &self,
        topic_key: &str,
        content: &str,
    ) -> Result<(), EngramHttpError> {
        // The engram session id is derived from the topic_key's session segment when
        // present (`spectty/{session_id}/...`); fall back to a stable per-project id.
        let session_id = engram_session_id(topic_key);
        self.ensure_session(&session_id).await?;

        let resp = self
            .client
            .post(format!("{}/observations", self.base_url))
            .json(&serde_json::json!({
                "session_id": session_id,
                "topic_key": topic_key,
                "project": self.project,
                "scope": "project",
                "content": content,
                "type": "architecture",
                "title": topic_key,
            }))
            .send()
            .await
            .map_err(|e| EngramHttpError::Transport(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(EngramHttpError::Transport(format!(
                "POST /observations returned {}",
                resp.status()
            )))
        }
    }

    async fn get_observation_async(&self, topic_key: &str) -> Result<Option<Obs>, EngramHttpError> {
        // G1: `?topic_key=`/`?since=` are not honored server-side. Fetch the list and
        // filter CLIENT-SIDE, matching `topic_key` case-insensitively (server lowercases).
        let resp = self
            .client
            .get(format!("{}/observations", self.base_url))
            .send()
            .await
            .map_err(|e| EngramHttpError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(EngramHttpError::Transport(format!(
                "GET /observations returned {}",
                resp.status()
            )));
        }
        let rows: Vec<RawObs> = resp
            .json()
            .await
            .map_err(|e| EngramHttpError::Transport(e.to_string()))?;
        let wanted = topic_key.to_ascii_lowercase();
        // Pick the most recently updated matching row (engram may keep revisions).
        let latest = rows
            .into_iter()
            .filter(|r| {
                r.topic_key
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(&wanted))
                    .unwrap_or(false)
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|r| Obs {
                content: r.content,
                updated_at: r.updated_at,
            });
        Ok(latest)
    }
}

impl EngramHttp for ReqwestEngramHttp {
    fn post_observation(&self, topic_key: &str, content: &str) -> Result<(), EngramHttpError> {
        // D26 bridge: drive the async request to completion on the dedicated runtime.
        // `block_on` here is safe because the trait methods are SYNC and never called
        // from inside that runtime's own worker threads (the poll loop/commands call
        // them from non-async context).
        self.rt
            .block_on(self.post_observation_async(topic_key, content))
    }

    fn get_observation(
        &self,
        topic_key: &str,
        _since: Option<&str>,
    ) -> Result<Option<Obs>, EngramHttpError> {
        self.rt.block_on(self.get_observation_async(topic_key))
    }
}

/// The subset of an engram observation row we deserialize (G1-verified field names).
#[derive(serde::Deserialize)]
struct RawObs {
    topic_key: Option<String>,
    content: String,
    updated_at: String,
}

/// Derive the engram session id from a `spectty/{session_id}/...` topic_key. Falls
/// back to `"spectty"` for non-namespaced keys so the upsert still has a valid session.
fn engram_session_id(topic_key: &str) -> String {
    let mut parts = topic_key.split('/');
    match (parts.next(), parts.next()) {
        (Some("spectty"), Some(sid)) if !sid.is_empty() => sid.to_string(),
        _ => "spectty".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WU-1.6 GREEN (contract of the test double itself): the in-memory FakeEngramHttp
    // really round-trips a payload by topic_key.
    #[test]
    fn fake_engram_http_round_trips_by_topic_key() {
        let http = FakeEngramHttp::new();
        http.post_observation("spectty/7/spec", "payload-1")
            .expect("post should succeed");

        let got = http
            .get_observation("spectty/7/spec", None)
            .expect("get should not error");
        assert_eq!(got.map(|o| o.content), Some("payload-1".to_string()));
    }

    // WU-1.6: matching is case-insensitive (engram lowercases topic_key server-side).
    #[test]
    fn fake_engram_http_matches_topic_key_case_insensitively() {
        let http = FakeEngramHttp::new();
        http.post_observation("Spectty/7/Spec", "payload")
            .expect("post should succeed");

        let got = http
            .get_observation("spectty/7/spec", None)
            .expect("get should not error");
        assert_eq!(got.map(|o| o.content), Some("payload".to_string()));
    }

    // WU-1.6: absent key → Ok(None), not an error.
    #[test]
    fn fake_engram_http_absent_key_is_none() {
        let http = FakeEngramHttp::new();
        let got = http
            .get_observation("spectty/missing/spec", None)
            .expect("get should not error");
        assert_eq!(got, None);
    }

    // WU-1.6: updated_at advances strictly across writes (monotonic change-detect feed).
    #[test]
    fn fake_engram_http_updated_at_is_monotonic() {
        let http = FakeEngramHttp::new();
        http.post_observation("k", "v1").unwrap();
        let first = http.get_observation("k", None).unwrap().unwrap().updated_at;
        http.post_observation("k", "v2").unwrap();
        let second = http.get_observation("k", None).unwrap().unwrap().updated_at;
        assert!(
            second > first,
            "updated_at must advance: {first} -> {second}"
        );
    }

    // WU-1.3 feeder: a failing transport surfaces a Transport error on both verbs.
    #[test]
    fn fake_engram_http_failing_errors_both_verbs() {
        let http = FakeEngramHttp::failing();
        assert!(http.post_observation("k", "v").is_err());
        assert!(http.get_observation("k", None).is_err());
    }

    // G1: the session-id derivation extracts the `spectty/{sid}/...` segment.
    #[test]
    fn engram_session_id_extracts_spectty_session_segment() {
        assert_eq!(engram_session_id("spectty/42/spec"), "42");
        assert_eq!(engram_session_id("spectty//spec"), "spectty");
        assert_eq!(engram_session_id("other/key"), "spectty");
    }
}
