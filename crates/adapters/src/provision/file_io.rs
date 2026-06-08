//! The atomic-write file-IO seam (D17, R8).
//!
//! [`ConfigFile`] abstracts reading + atomically writing the agent config so the
//! provisioner is testable without touching the real filesystem (the same seam
//! discipline as `PtyTransport`). The real impl writes via temp-file → fsync →
//! rename and copies the original to `<path>.spectty.bak` on the FIRST write — the
//! documented manual "reset to pre-Spectty config" escape hatch (R8 deferral, D14).
//!
//! Tests substitute [`FakeConfigFile`], an in-memory map that records backups +
//! atomic replaces, so the provisioner's behavior is asserted without real I/O.

use std::io::Write;

/// Substitutable file-IO so the provisioner never opens a real file in unit tests.
///
/// `Send + Sync` so the owning [`ClaudeJsonProvisioner`](super::claude_provisioner::ClaudeJsonProvisioner)
/// shares as `tauri::State`.
pub trait ConfigFile: Send + Sync {
    /// Read the file's contents, or `None` if the file does not exist.
    fn read(&self, path: &str) -> std::io::Result<Option<String>>;

    /// Write `contents` to `path` atomically: write `<path>.tmp` → fsync → rename
    /// over `path`. On the FIRST write (when no `<path>.spectty.bak` exists yet)
    /// the ORIGINAL contents are copied to `<path>.spectty.bak` first.
    fn write_atomic(&self, path: &str, contents: &str) -> std::io::Result<()>;
}

/// Suffix appended to a config path for the one-time pre-Spectty backup.
const BACKUP_SUFFIX: &str = ".spectty.bak";
/// Suffix for the temp file used by the atomic write.
const TEMP_SUFFIX: &str = ".tmp";

/// Production [`ConfigFile`]: real `tmp + fsync + rename` with a one-time backup.
pub struct RealConfigFile;

impl ConfigFile for RealConfigFile {
    fn read(&self, path: &str) -> std::io::Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write_atomic(&self, path: &str, contents: &str) -> std::io::Result<()> {
        // One-time backup of the ORIGINAL before we ever mutate it.
        let backup = format!("{path}{BACKUP_SUFFIX}");
        if !std::path::Path::new(&backup).exists() {
            if let Some(original) = self.read(path)? {
                std::fs::write(&backup, original)?;
            }
        }

        // Atomic replace: write temp → fsync → rename.
        let tmp = format!("{path}{TEMP_SUFFIX}");
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory [`ConfigFile`] for tests. Records each atomic write and the
    /// one-time backup so the provisioner's behavior is assertable without real I/O.
    #[derive(Default)]
    pub(crate) struct FakeConfigFile {
        inner: Mutex<FakeInner>,
    }

    #[derive(Default)]
    struct FakeInner {
        files: HashMap<String, String>,
        /// Ordered record of `(path, contents)` atomic writes.
        writes: Vec<(String, String)>,
    }

    impl FakeConfigFile {
        /// Seed an existing file (the "original" the first write backs up).
        pub(crate) fn with_file(path: &str, contents: &str) -> Self {
            let fake = Self::default();
            fake.inner
                .lock()
                .expect("lock")
                .files
                .insert(path.to_string(), contents.to_string());
            fake
        }

        /// Current contents of `path`, if present.
        pub(crate) fn contents(&self, path: &str) -> Option<String> {
            self.inner.lock().expect("lock").files.get(path).cloned()
        }

        /// How many atomic writes targeted `path`.
        pub(crate) fn write_count(&self, path: &str) -> usize {
            self.inner
                .lock()
                .expect("lock")
                .writes
                .iter()
                .filter(|(p, _)| p == path)
                .count()
        }
    }

    impl ConfigFile for FakeConfigFile {
        fn read(&self, path: &str) -> std::io::Result<Option<String>> {
            Ok(self.inner.lock().expect("lock").files.get(path).cloned())
        }

        fn write_atomic(&self, path: &str, contents: &str) -> std::io::Result<()> {
            let mut inner = self.inner.lock().expect("lock");
            // One-time backup of the original, mirroring RealConfigFile.
            let backup = format!("{path}{BACKUP_SUFFIX}");
            if !inner.files.contains_key(&backup) {
                if let Some(original) = inner.files.get(path).cloned() {
                    inner.files.insert(backup, original);
                }
            }
            inner.files.insert(path.to_string(), contents.to_string());
            inner.writes.push((path.to_string(), contents.to_string()));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeConfigFile;
    use super::*;

    #[test]
    fn first_write_creates_spectty_bak_of_the_original() {
        let path = "/cfg/.claude.json";
        let fake = FakeConfigFile::with_file(path, "ORIGINAL");

        fake.write_atomic(path, "NEXT").expect("write");

        assert_eq!(
            fake.contents(&format!("{path}.spectty.bak")).as_deref(),
            Some("ORIGINAL"),
            "first write backs up the ORIGINAL contents"
        );
        assert_eq!(
            fake.contents(path).as_deref(),
            Some("NEXT"),
            "atomic replace landed"
        );
    }

    #[test]
    fn second_write_does_not_overwrite_the_backup() {
        let path = "/cfg/.claude.json";
        let fake = FakeConfigFile::with_file(path, "ORIGINAL");

        fake.write_atomic(path, "FIRST").expect("write 1");
        fake.write_atomic(path, "SECOND").expect("write 2");

        assert_eq!(
            fake.contents(&format!("{path}.spectty.bak")).as_deref(),
            Some("ORIGINAL"),
            "the backup stays the pre-Spectty original across writes"
        );
        assert_eq!(fake.write_count(path), 2, "both writes recorded");
    }

    #[test]
    fn read_absent_file_is_none() {
        let fake = FakeConfigFile::default();
        assert_eq!(fake.read("/nope").expect("read"), None);
    }
}
