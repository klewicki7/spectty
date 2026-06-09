//! `hook` — the M3 IPC reader sub-system (WU-3).
//!
//! All types here are PURE-testable or use an injected seam — NO direct filesystem
//! access escapes the [`reader`] module's `read` closure boundary.
//!
//! Layout:
//! - [`state`]: [`HookEvent`] enum, [`HookState`] struct, `parse_state_file` + `event_to_observed` — all PURE.
//! - [`reader`]: [`StateFileReader`] consume-once reader (last_ts strict-greater predicate, D22).

pub mod reader;
pub mod state;

pub use reader::StateFileReader;
pub use state::{
    event_to_observed, parse_state_file, HookEvent, HookState, PERMISSION_PROMPT_MATCHER,
};
