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
//! `$HOME/.local/share`; Windows: `%LOCALAPPDATA%`).
//!
//! Directory: `{app_local_data_dir}/app.spectty.desktop/runtime`
//!
//! This matches Tauri's `app_local_data_dir()` which is defined as:
//! `dirs::data_local_dir().join(config().identifier)` where the bundle identifier
//! is `app.spectty.desktop` (from `src-tauri/tauri.conf.json`).
//!
//! Resolved paths per OS:
//! - macOS:   `$HOME/Library/Application Support/app.spectty.desktop/runtime`
//! - Linux:   `$XDG_DATA_HOME/app.spectty.desktop/runtime`
//!   (or `$HOME/.local/share/app.spectty.desktop/runtime`)
//! - Windows: `%LOCALAPPDATA%\app.spectty.desktop\runtime`

use std::path::PathBuf;

/// The Tauri bundle identifier from `src-tauri/tauri.conf.json`.
///
/// Tauri's `app_local_data_dir()` is `data_local_dir().join(identifier)`.
/// This constant is the single source of truth for the sidecar — WU-8's
/// src-tauri resolver MUST use the same value.
const BUNDLE_IDENTIFIER: &str = "app.spectty.desktop";

/// Resolve the Spectty sidecar runtime directory.
///
/// Returns `None` when the platform data directory cannot be determined (e.g. `$HOME`
/// unset on macOS/Linux, `%LOCALAPPDATA%` unset on Windows). The caller exits non-zero
/// on `None`.
///
/// The path MUST match `spectty_runtime_dir()` in `src-tauri/src/lib.rs` (WU-8).
/// Both are pinned to the same value by the WU-9 path-agreement integration test.
///
/// The directory is `{data_local_dir}/{BUNDLE_IDENTIFIER}/runtime`, matching Tauri's
/// `app_local_data_dir()` = `data_local_dir().join(config().identifier)`.
pub fn spectty_runtime_dir() -> Option<PathBuf> {
    platform_data_dir().map(|base| base.join(BUNDLE_IDENTIFIER).join("runtime"))
}

/// Return the OS app-local-data directory (no app-name subfolder).
///
/// Platform mapping (mirrors Tauri's `app_local_data_dir()` / `dirs::data_local_dir()`):
/// - macOS:   `$HOME/Library/Application Support`
/// - Linux:   `$XDG_DATA_HOME`  OR  `$HOME/.local/share`
/// - Windows: `%LOCALAPPDATA%`
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
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runtime dir must contain the Tauri bundle identifier `app.spectty.desktop`
    /// as a path component — NOT `Spectty` (the productName). Tauri's
    /// `app_local_data_dir()` joins `data_local_dir()` with `config().identifier`,
    /// i.e. `app.spectty.desktop`, not `productName`.
    ///
    /// This test pins the contract so WU-8's src-tauri resolver must match.
    #[test]
    fn spectty_runtime_dir_contains_bundle_identifier_not_product_name() {
        let dir = spectty_runtime_dir();
        // On CI / dev machines $HOME / $LOCALAPPDATA is always set.
        if let Some(path) = dir {
            let path_str = path.to_string_lossy();
            assert!(
                path_str.contains("app.spectty.desktop"),
                "runtime dir must contain the bundle identifier 'app.spectty.desktop', got: {path:?}"
            );
            assert!(
                !path_str.contains("/Spectty/") && !path_str.ends_with("/Spectty"),
                "runtime dir must NOT contain 'Spectty' as a path segment (productName is wrong), got: {path:?}"
            );
            assert!(
                path_str.ends_with("app.spectty.desktop/runtime")
                    || path_str.ends_with("app.spectty.desktop\\runtime"),
                "runtime dir must end with 'app.spectty.desktop/runtime', got: {path:?}"
            );
        }
        // If $HOME is unset, None is acceptable — the binary exits non-zero.
    }
}
