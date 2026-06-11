//! [`EngramAdapter`] — the real [`PersistencePort`] backed by engram's local HTTP API.
//!
//! M4 (PR-1, D26) replaces the M0 `todo!()` skeleton. The adapter implements the
//! UNCHANGED sync/`String` [`PersistencePort`] and delegates the network to the thin
//! [`EngramHttp`] seam. It owns either a DEDICATED Tokio runtime (for the real
//! `ReqwestEngramHttp`, which `block_on`-bridges async `reqwest`) or just an
//! `Arc<dyn EngramHttp>` (for the in-memory `FakeEngramHttp` used by contract tests).
//!
//! Degrade-when-down (M4-REQ-02): a transport failure maps to
//! `PersistenceError::Backend(_)` — it NEVER panics, so a flaky/absent engram daemon
//! cannot crash a session.

use std::sync::Arc;

use spectty_core::ports::{PersistenceError, PersistencePort};

use super::engram_http::{EngramHttp, EngramHttpError, ReqwestEngramHttp, ENGRAM_BASE_URL};

/// The engram-backed [`PersistencePort`] implementation.
///
/// `http` is the injectable HTTP seam (real or fake); `_rt` keeps the dedicated Tokio
/// runtime alive for the real client's `block_on` bridge. The runtime is `Option`al so
/// the fake-backed test constructor needs no runtime at all.
pub struct EngramAdapter {
    http: Arc<dyn EngramHttp>,
    // Held to keep the dedicated runtime alive for as long as the adapter (and thus the
    // `ReqwestEngramHttp` handle into it) lives. `None` for fake-backed test adapters.
    _rt: Option<tokio::runtime::Runtime>,
}

impl std::fmt::Debug for EngramAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngramAdapter").finish_non_exhaustive()
    }
}

impl EngramAdapter {
    /// Build the production adapter against engram on `:7437` for `project`.
    ///
    /// Spins up a small DEDICATED multi-thread Tokio runtime (NOT the Tauri main
    /// runtime) that the real `ReqwestEngramHttp` `block_on`s its async requests onto.
    pub fn new(project: impl Into<String>) -> Result<Self, PersistenceError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| PersistenceError::Backend(format!("engram runtime: {e}")))?;
        let http = Arc::new(ReqwestEngramHttp::new(
            ENGRAM_BASE_URL,
            project,
            rt.handle().clone(),
        ));
        Ok(Self {
            http,
            _rt: Some(rt),
        })
    }

    /// Build an adapter over an arbitrary [`EngramHttp`] (the contract-test seam).
    #[cfg(test)]
    fn with_http(http: Arc<dyn EngramHttp>) -> Self {
        Self { http, _rt: None }
    }
}

impl PersistencePort for EngramAdapter {
    fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError> {
        self.http
            .post_observation(topic_key, &payload)
            .map_err(map_http_err)
    }

    fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError> {
        self.http
            .get_observation(topic_key, None)
            .map(|opt| opt.map(|obs| obs.content))
            .map_err(map_http_err)
    }
}

/// Map a transport error to the port's `Backend` variant. This is the degrade-when-down
/// seam: engram unreachable becomes a recoverable `Err`, never a panic (M4-REQ-02).
fn map_http_err(e: EngramHttpError) -> PersistenceError {
    PersistenceError::Backend(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::engram_http::FakeEngramHttp;

    // WU-1.1: upsert then get round-trips the payload through the adapter.
    #[test]
    fn engram_adapter_upsert_then_get_round_trips() {
        let adapter = EngramAdapter::with_http(Arc::new(FakeEngramHttp::new()));
        let payload = r#"{"intent":"demo"}"#.to_string();

        adapter
            .upsert("spectty/s/spec", payload.clone())
            .expect("upsert should succeed");

        let read = adapter.get("spectty/s/spec").expect("get should not error");
        assert_eq!(read, Some(payload), "payload must round-trip unchanged");
    }

    // WU-1.2: get of an absent topic_key returns Ok(None) (absence is not an error).
    #[test]
    fn engram_adapter_get_absent_key_returns_ok_none() {
        let adapter = EngramAdapter::with_http(Arc::new(FakeEngramHttp::new()));

        let read = adapter
            .get("spectty/unknown/spec")
            .expect("get should not error");
        assert_eq!(read, None, "missing key must be Ok(None), not an error");
    }

    // WU-1.3: engram down → both verbs return Err(Backend(_)), NEVER panic.
    #[test]
    fn engram_adapter_degrades_when_backend_down() {
        let adapter = EngramAdapter::with_http(Arc::new(FakeEngramHttp::failing()));

        let upsert_err = adapter.upsert("spectty/s/spec", "x".to_string());
        assert!(
            matches!(upsert_err, Err(PersistenceError::Backend(_))),
            "engram-down upsert must map to Backend error; got {upsert_err:?}"
        );

        let get_err = adapter.get("spectty/s/spec");
        assert!(
            matches!(get_err, Err(PersistenceError::Backend(_))),
            "engram-down get must map to Backend error; got {get_err:?}"
        );
    }

    // WU-1.4: compile-time proof the adapter still satisfies the UNCHANGED port
    // (sync, &self, String/Option<String>). If the port grew an async/subscribe method
    // this would fail to compile — pinning M4-REQ-01.
    #[test]
    fn engram_adapter_implements_persistence_port_unchanged() {
        fn takes_port(_: &dyn PersistencePort) {}
        let adapter = EngramAdapter::with_http(Arc::new(FakeEngramHttp::new()));
        takes_port(&adapter);

        // Also usable behind Arc<dyn PersistencePort> for the SpecBus (WU-2).
        let port: Arc<dyn PersistencePort> =
            Arc::new(EngramAdapter::with_http(Arc::new(FakeEngramHttp::new())));
        port.upsert("k", "v".to_string()).expect("arc upsert ok");
    }

    // WU-1.8: the ONE real-`:7437` contract test. `#[ignore]` because it depends on a
    // running engram daemon (G1-verified shapes) — run manually with
    // `cargo test -p spectty-adapters -- --ignored engram_adapter_real_7437_contract`.
    #[test]
    #[ignore = "requires a running engram daemon on :7437 (G1)"]
    fn engram_adapter_real_7437_contract() {
        let adapter = EngramAdapter::new("spectty").expect("real adapter builds");
        let key = "spectty/__m4_real_contract__/spec";
        let payload = r#"{"intent":"real contract"}"#.to_string();

        adapter
            .upsert(key, payload.clone())
            .expect("real upsert should succeed");
        let read = adapter.get(key).expect("real get should not error");
        assert_eq!(read, Some(payload), "real round-trip must match");
    }
}
