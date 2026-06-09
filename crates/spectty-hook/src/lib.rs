//! Public surface of the `spectty-hook` library target.
//!
//! The sidecar is primarily a binary crate (`spectty-hook`). This thin library
//! target exists solely so the WU-9 path-agreement integration test (in
//! `src-tauri/tests/hook_integration.rs`) can call
//! `spectty_hook::spectty_runtime_dir()` alongside
//! `spectty_lib::spectty_runtime_dir()` in a SINGLE test — making D25 silent
//! path divergence impossible to land undetected.
//!
//! Production code MUST NOT import this crate: it intentionally has no
//! dependency on spectty-core, spectty-adapters, or tauri (D25).

mod runtime_dir;

pub use runtime_dir::spectty_runtime_dir;
