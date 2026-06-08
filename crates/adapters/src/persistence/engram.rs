use spectty_core::ports::{PersistenceError, PersistencePort};

/// Skeleton adapter for the engram backend.
///
/// M0 proves only the adapter SHAPE: it satisfies [`PersistencePort`] and
/// compiles with no running daemon or network. The real transport (POST/GET to
/// engram on `:7437` `/api/observations`, plus the 2s poll loop and subscribe)
/// arrives in M3, at which point this becomes async over `reqwest`.
#[derive(Debug, Default)]
pub struct EngramAdapter;

impl PersistencePort for EngramAdapter {
    fn upsert(&self, _topic_key: &str, _payload: String) -> Result<(), PersistenceError> {
        todo!("M3: POST engram :7437 /api/observations")
    }

    fn get(&self, _topic_key: &str) -> Result<Option<String>, PersistenceError> {
        todo!("M3: GET engram :7437 /api/observations")
    }
}
