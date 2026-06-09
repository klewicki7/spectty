//! Runtime-directory resolver for the `spectty-hook` sidecar.
//!
//! This is a ~10-line duplicate of `src-tauri/src/lib.rs`'s `spectty_runtime_dir()`
//! (D25). The two MUST resolve to the same path — pinned by the WU-9 integration
//! test (`spectty_hook_end_to_end_monotonic_ts_and_path_agreement`).
//!
//! The resolver intentionally does NOT depend on `tauri`, `dirs`, or `spectty-core`:
//! the sidecar is a standalone binary (serde/serde_json only). It uses the same
//! platform environment variables that Tauri's `app_local_data_dir()` would use
//! (macOS: `$HOME/Library/Application Support`; Linux: `$XDG_DATA_HOME` or
//! `$HOME/.local/share`; Windows: `%APPDATA%`).
//!
//! Directory: `{app_local_data_dir}/Spectty/runtime`
//! (matches the Tauri productName "Spectty" in `tauri.conf.json`).

use std::path::PathBuf;

/// Resolve the Spectty sidecar runtime directory.
///
/// Returns `None` when the platform data directory cannot be determined (e.g. `$HOME`
/// unset on macOS/Linux, `%APPDATA%` unset on Windows). The caller exits non-zero on
/// `None`.
///
/// The path MUST match `spectty_runtime_dir()` in `src-tauri/src/lib.rs` (WU-8).
/// Both are pinned to the same value by the WU-9 path-agreement integration test.
pub fn spectty_runtime_dir() -> Option<PathBuf> {
    platform_data_dir().map(|base| base.join("Spectty").join("runtime"))
}

/// Return the OS app-local-data directory (no app-name subfolder).
///
/// Platform mapping (mirrors Tauri's `app_local_data_dir()` logic):
/// - macOS:   `$HOME/Library/Application Support`
/// - Linux:   `$XDG_DATA_HOME`  OR  `$HOME/.local/share`
/// - Windows: `%APPDATA%`
fn platform_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME").ok().map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        })
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            Some(PathBuf::from(xdg))
        } else {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        }
    }

    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a system with $HOME set the resolver returns a non-None path
    /// that ends in `Spectty/runtime`. This doesn't assert the exact prefix
    /// (that's the WU-9 path-agreement test's job) but confirms the suffix shape.
    #[test]
    fn spectty_runtime_dir_ends_in_spectty_runtime() {
        let dir = spectty_runtime_dir();
        // On CI / dev machines $HOME is always set.
        if let Some(path) = dir {
            assert!(
                path.ends_with("Spectty/runtime"),
                "runtime dir must end with Spectty/runtime, got: {path:?}"
            );
        }
        // If $HOME is unset, None is acceptable — the binary exits non-zero.
    }
}
