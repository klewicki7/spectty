//! The `portable-pty`-backed [`PtyAdapter`] and its error type.
//!
//! This is the only place in the adapter layer that touches a real
//! pseudo-terminal. It opens a PTY pair, spawns the configured shell into it,
//! and exposes write/resize/kill via the [`PtyTransport`] seam plus a reader for
//! the dedicated read loop (wired in src-tauri, WU-4). It is NOT a `spectty-core`
//! port — the hexagonal core never depends on `portable-pty`.

use std::io::{Read, Write};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use super::config::PtySpawnConfig;
use super::transport::PtyTransport;

/// Errors raised while opening, driving, or tearing down a PTY.
///
/// These stay inside the adapter layer; the command boundary (src-tauri) maps
/// them to `String` via `.to_string()` (the M0 ping convention) so a PTY failure
/// never panics the UI.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// Opening the PTY pair failed.
    #[error("failed to open pty: {0}")]
    Open(String),

    /// Spawning the child process into the PTY failed.
    #[error("failed to spawn shell in pty: {0}")]
    Spawn(String),

    /// An I/O error while reading from or writing to the PTY.
    #[error("pty io error: {0}")]
    Io(#[from] std::io::Error),

    /// Resizing the PTY window failed.
    #[error("failed to resize pty: {0}")]
    Resize(String),

    /// No PTY is registered under the requested id.
    #[error("unknown pty id: {0}")]
    UnknownId(String),

    /// A shared lock guarding the PTY was poisoned by a panicked thread.
    #[error("pty registry lock poisoned")]
    Poisoned,
}

/// A live PTY: the master side, the input writer, and the child handle.
///
/// The cloned reader is returned separately from [`PtyAdapter::spawn`] so the
/// read loop can own it on a dedicated thread while this struct owns the
/// write/resize/kill side behind [`PtyTransport`].
pub struct PtyAdapter {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

// Manual Debug: the master and writer trait objects do not implement Debug, so
// derive cannot be used. The opaque fields are not useful to print anyway.
impl std::fmt::Debug for PtyAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyAdapter")
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl PtyAdapter {
    /// Open a PTY, spawn the configured program into it, and return the adapter
    /// together with a reader for the PTY's output stream.
    ///
    /// The reader is handed back to the caller (rather than stored) because the
    /// M1 read loop lives on a dedicated `std::thread` and must own it for the
    /// PTY's whole lifetime.
    pub fn spawn(cfg: &PtySpawnConfig) -> Result<(Self, Box<dyn Read + Send>), PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: cfg.rows,
                cols: cfg.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let mut command = CommandBuilder::new(&cfg.program);
        for arg in &cfg.args {
            command.arg(arg);
        }
        if let Some(cwd) = &cfg.cwd {
            command.cwd(cwd);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;

        // Reader must be cloned before the writer is taken; once the slave is
        // dropped the master alone keeps the PTY open.
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Open(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let adapter = Self {
            master: pair.master,
            writer,
            child,
        };
        Ok((adapter, reader))
    }
}

impl PtyTransport for PtyAdapter {
    fn write(&mut self, data: &[u8]) -> Result<(), PtyError> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
        // Resizing the master raises SIGWINCH so the child program reflows.
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Resize(e.to_string()))
    }

    fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill()?;
        Ok(())
    }
}
