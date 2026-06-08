use thiserror::Error;

/// Errors returned by a [`PersistencePort`] implementation.
///
/// A missing key is NOT an error: [`PersistencePort::get`] returns `Ok(None)`
/// for an absent `topic_key`. This enum is reserved for genuine backend
/// failures (network, serialization, IO, ...).
#[derive(Debug, Error)]
pub enum PersistenceError {
    /// The underlying backend failed (network, serialization, IO, ...).
    #[error("persistence backend error: {0}")]
    Backend(String),
}

/// Port for persisting and retrieving opaque serialized payloads by topic key.
///
/// This is the sole behavior-bearing contract in the M0 core. It is a PURE,
/// SYNC contract: no engram, HTTP, tauri, adapter, or `serde_json` references
/// leak in here. The payload is an already-serialized `String` — (de)serialization
/// is owned by the adapter, which keeps `serde_json` out of the core boundary.
///
/// Both methods take `&self` so a single adapter can be shared across multiple
/// concurrent Sessions behind an `Arc<dyn PersistencePort>` without an exclusive
/// mutable borrow. Any mutability is encapsulated INSIDE the adapter (interior
/// mutability), never exposed through the port. `Send + Sync` makes that sharing
/// safe across threads.
pub trait PersistencePort: Send + Sync {
    /// Insert or replace the payload stored under `topic_key`.
    fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError>;

    /// Retrieve the payload stored under `topic_key`.
    ///
    /// Returns `Ok(None)` when no value exists for the key — a missing key is a
    /// normal, expected outcome, not a [`PersistenceError`].
    fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError>;
}
