//! The in-adapter `PtyTransport` seam.
//!
//! `PtyTransport` abstracts the write/resize/kill side of a live PTY so the
//! command layer (src-tauri, M1 WU-4) can be unit-tested against a fake instead
//! of opening a real pseudo-terminal. It is an ADAPTER-INTERNAL trait, NOT a
//! `spectty-core` port: the hexagonal core knows nothing about PTYs, and this
//! seam exists purely to make the side effects substitutable in tests.
//!
//! The real implementation is [`super::adapter::PtyAdapter`]; tests provide their
//! own recording fake.

use super::adapter::PtyError;

/// Write/resize/kill operations on a live PTY.
///
/// Object-safe (`&mut dyn PtyTransport`) so the command layer can hold a boxed
/// transport in its registry and swap in a fake for tests. `Send` so the owning
/// handle can move across the spawn boundary.
pub trait PtyTransport: Send {
    /// Forward bytes (typically keystrokes) to the PTY's input.
    fn write(&mut self, data: &[u8]) -> Result<(), PtyError>;

    /// Resize the PTY window, raising `SIGWINCH` for the child process.
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError>;

    /// Terminate the child process attached to the PTY.
    fn kill(&mut self) -> Result<(), PtyError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording fake proving `PtyTransport` is object-safe and substitutable.
    /// The real command-layer fake (driving `send_input`/`pty_resize`/`pty_kill`)
    /// lives with those commands in src-tauri (WU-4); this minimal one only
    /// guards the seam's shape here in the adapter crate.
    #[derive(Default)]
    struct RecordingTransport {
        writes: Vec<Vec<u8>>,
        resizes: Vec<(u16, u16)>,
        kills: u32,
    }

    impl PtyTransport for RecordingTransport {
        fn write(&mut self, data: &[u8]) -> Result<(), PtyError> {
            self.writes.push(data.to_vec());
            Ok(())
        }
        fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
            self.resizes.push((cols, rows));
            Ok(())
        }
        fn kill(&mut self) -> Result<(), PtyError> {
            self.kills += 1;
            Ok(())
        }
    }

    #[test]
    fn pty_transport_is_object_safe_and_records_calls() {
        let mut fake = RecordingTransport::default();
        let transport: &mut dyn PtyTransport = &mut fake;

        transport.write(b"ls\n").expect("write");
        transport.resize(100, 30).expect("resize");
        transport.kill().expect("kill");

        assert_eq!(fake.writes, vec![b"ls\n".to_vec()], "write is forwarded");
        assert_eq!(fake.resizes, vec![(100, 30)], "resize cols/rows forwarded");
        assert_eq!(fake.kills, 1, "kill is forwarded once");
    }
}
