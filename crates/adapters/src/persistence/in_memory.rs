use std::collections::HashMap;
use std::sync::Mutex;

use spectty_core::ports::{PersistenceError, PersistencePort};

/// A REAL (not mock) in-memory implementation of [`PersistencePort`].
///
/// The port exposes `&self` methods so the adapter can be shared across
/// concurrent Sessions behind an `Arc<dyn PersistencePort>`. Mutation is
/// therefore encapsulated via INTERIOR MUTABILITY: the map lives behind a
/// [`Mutex`], keeping the adapter `Send + Sync` while honoring the `&self`
/// contract. Used to prove the persistence round-trip without any external
/// dependency.
#[derive(Debug, Default)]
pub struct InMemoryPersistenceAdapter {
    store: Mutex<HashMap<String, String>>,
}

impl InMemoryPersistenceAdapter {
    /// Create an empty adapter.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PersistencePort for InMemoryPersistenceAdapter {
    fn upsert(&self, topic_key: &str, payload: String) -> Result<(), PersistenceError> {
        self.store
            .lock()
            .expect("in-memory persistence mutex poisoned")
            .insert(topic_key.to_owned(), payload);
        Ok(())
    }

    fn get(&self, topic_key: &str) -> Result<Option<String>, PersistenceError> {
        Ok(self
            .store
            .lock()
            .expect("in-memory persistence mutex poisoned")
            .get(topic_key)
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_in_memory_persistence_round_trips() {
        let adapter = InMemoryPersistenceAdapter::new();
        let key = "spectty/sessions/abc";
        let payload = r#"{"id":"abc","title":"demo"}"#.to_string();

        adapter
            .upsert(key, payload.clone())
            .expect("upsert should succeed");

        let read = adapter.get(key).expect("get should not error");
        assert_eq!(
            read,
            Some(payload),
            "value must round-trip unchanged as Some"
        );
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let adapter = InMemoryPersistenceAdapter::new();

        let result = adapter.get("does/not/exist").expect("get should not error");

        assert_eq!(
            result, None,
            "missing key must return Ok(None), not an error"
        );
    }

    #[test]
    fn test_usable_behind_arc_dyn_port() {
        // Proves the &self contract: the adapter is shareable across Sessions
        // behind an Arc<dyn PersistencePort> with no exclusive mutable borrow.
        let port: Arc<dyn PersistencePort> = Arc::new(InMemoryPersistenceAdapter::new());

        port.upsert("k", "v".to_string())
            .expect("upsert through Arc should succeed");

        let read = port.get("k").expect("get through Arc should not error");
        assert_eq!(read, Some("v".to_string()));
    }
}
