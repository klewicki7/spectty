//! Pure ANSI-stripping rolling-window producer for the agent `OutputSignal`.
//!
//! The M1 read loop yields raw PTY bytes carrying ANSI escape sequences (colour
//! SGR, cursor CSI moves, OSC title sets). `AgentRunner::detect_status` consumes a
//! NORMALIZED [`OutputSignal`] — plain printable text only — so this producer is the
//! SECOND, independent consumer of the read stream that decodes those bytes into a
//! bounded rolling text window.
//!
//! It mirrors the [`Coalescer`](crate::pty::Coalescer) discipline exactly: a pure
//! state machine with NO clock, NO thread, NO I/O. [`ingest`](OutputSignalProducer::ingest)
//! folds raw chunks into the window; [`snapshot`](OutputSignalProducer::snapshot)
//! builds an `OutputSignal` from the window plus the CALLER-supplied clock fields
//! (the src-tauri read loop owns the `ClockPort` and computes `idle_ms`/`is_active`,
//! D10). The bounded drop-oldest CHANNEL + signal thread that feed this producer are
//! a src-tauri concern (WU-9); this is only the pure half (D9).
//!
//! ANSI handling is hand-rolled (D11 — no `vte`/`strip-ansi` dep): a tiny
//! `Ground | Esc | Csi | Osc` state machine drops escape sequences and keeps the
//! printable text. Because state PERSISTS across [`ingest`](OutputSignalProducer::ingest)
//! calls, an escape split across two chunks (`\x1b[` then `31m`) is stripped correctly.

use spectty_core::ports::clock::Timestamp;
use spectty_core::OutputSignal;

/// Where the hand-rolled ANSI stripper is inside an escape sequence. Persists across
/// `ingest` calls so a sequence split across two raw chunks is still stripped whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    /// Outside any escape — bytes are printable text (until an `ESC` arrives).
    Ground,
    /// Saw `ESC` (`0x1b`); the next byte selects the sequence kind.
    Esc,
    /// Inside a CSI sequence (`ESC [` … final byte in `0x40..=0x7e`).
    Csi,
    /// Inside an OSC sequence (`ESC ]` … terminated by `BEL` or `ESC \`).
    Osc,
    /// Inside an OSC and just saw an `ESC`, awaiting the `\` of the `ESC \` (ST)
    /// terminator.
    OscEsc,
}

/// Stateful ANSI-stripper + bounded rolling-window assembler. PURE: no clock, no
/// thread, no I/O — exactly the [`Coalescer`](crate::pty::Coalescer)'s "inject time,
/// fold bytes" discipline.
///
/// The window holds the last ~`window_bytes` of ANSI-stripped printable text,
/// truncated from the FRONT (drop-oldest) so detection always sees the most recent
/// output. State accumulates across [`ingest`](Self::ingest) calls.
#[derive(Debug)]
pub struct OutputSignalProducer {
    /// ANSI-stripped printable text, bounded to ~`window_bytes` (drop-oldest front).
    window: String,
    /// Current position of the hand-rolled ANSI state machine.
    ansi_state: AnsiState,
    /// Soft byte cap for `window`; the window is truncated from the front once it
    /// grows past this on an `ingest`.
    window_bytes: usize,
    /// Child exit code once the process has exited (`None` while running). Set via
    /// [`mark_exit`](Self::mark_exit); never cleared by `ingest` so the windowed text
    /// survives to be reported alongside the exit code.
    exit_code: Option<i32>,
}

impl OutputSignalProducer {
    /// Create a producer whose rolling window is bounded to `window_bytes` bytes of
    /// ANSI-stripped text.
    #[must_use]
    pub fn new(window_bytes: usize) -> Self {
        Self {
            window: String::with_capacity(window_bytes),
            ansi_state: AnsiState::Ground,
            window_bytes,
            exit_code: None,
        }
    }

    /// Fold one raw PTY chunk in: strip ANSI escape sequences (CSI/OSC/ESC), append
    /// the printable text, then truncate the window from the FRONT to `window_bytes`.
    ///
    /// PURE — no clock, no I/O. State (the ANSI position + the window) accumulates,
    /// so an escape sequence split across two `ingest` calls is still stripped whole.
    pub fn ingest(&mut self, raw: &[u8]) {
        for &byte in raw {
            self.step(byte);
        }
        self.truncate_window();
    }

    /// Record the child's exit code so the next [`snapshot`](Self::snapshot) reports a
    /// terminal status. Does NOT clear the window: the producer keeps the text it has
    /// already accumulated so a `Ready`-producing quiescent snapshot can be observed
    /// BEFORE the terminal-exit snapshot.
    ///
    // WU-9: the read loop calls `mark_exit` on EOF and emits the final (terminal)
    // snapshot only AFTER a quiescent `Ready` snapshot has been observed, so a clean
    // `Starting -> ... -> Idle/Ready -> Completed` path is reachable (the transition
    // table forbids `Starting -> Completed`). The ordering is enforced there; the
    // producer just makes it POSSIBLE by never erasing the window on exit.
    pub fn mark_exit(&mut self, code: i32) {
        self.exit_code = Some(code);
    }

    /// Build an [`OutputSignal`] from the current window plus the CALLER-supplied
    /// clock-derived fields (`last_byte_at`/`idle_ms`/`is_active`). The caller owns the
    /// `ClockPort` and computes those (D10), keeping `detect_status` a pure function of
    /// the signal. `exit_code` comes from [`mark_exit`](Self::mark_exit).
    #[must_use]
    pub fn snapshot(&self, last_byte_at: Timestamp, idle_ms: u64, is_active: bool) -> OutputSignal {
        OutputSignal {
            text_window: self.window.clone(),
            is_active,
            exit_code: self.exit_code,
            last_byte_at,
            idle_ms,
        }
    }

    /// Advance the ANSI state machine by one byte, appending to the window only when
    /// the byte is printable Ground text.
    fn step(&mut self, byte: u8) {
        const ESC: u8 = 0x1b;
        const BEL: u8 = 0x07;

        match self.ansi_state {
            AnsiState::Ground => {
                if byte == ESC {
                    self.ansi_state = AnsiState::Esc;
                } else {
                    self.push_printable(byte);
                }
            }
            AnsiState::Esc => match byte {
                b'[' => self.ansi_state = AnsiState::Csi,
                b']' => self.ansi_state = AnsiState::Osc,
                // Other two-byte escapes (e.g. `ESC c`, `ESC =`) end immediately; the
                // selector byte is consumed and we return to Ground.
                _ => self.ansi_state = AnsiState::Ground,
            },
            AnsiState::Csi => {
                // CSI runs until a final byte in 0x40..=0x7e (`@`..`~`); parameter and
                // intermediate bytes (0x20..=0x3f) stay inside the sequence.
                if (0x40..=0x7e).contains(&byte) {
                    self.ansi_state = AnsiState::Ground;
                }
            }
            AnsiState::Osc => match byte {
                BEL => self.ansi_state = AnsiState::Ground,
                ESC => self.ansi_state = AnsiState::OscEsc,
                _ => {}
            },
            AnsiState::OscEsc => {
                // `ESC \` (String Terminator) ends the OSC; a stray ESC restarts the
                // ESC-wait inside the OSC.
                if byte == b'\\' {
                    self.ansi_state = AnsiState::Ground;
                } else if byte != ESC {
                    self.ansi_state = AnsiState::Osc;
                }
            }
        }
    }

    /// Append a printable byte to the window. Control bytes other than the common
    /// whitespace (`\n`, `\r`, `\t`) are dropped so the scraped text stays clean.
    fn push_printable(&mut self, byte: u8) {
        if byte == b'\n' || byte == b'\r' || byte == b'\t' || byte >= 0x20 {
            // PTY output is UTF-8 in practice; pushing the raw byte as a char keeps the
            // window a `String` while preserving ASCII verbatim. Multi-byte UTF-8 bytes
            // each land as their own scalar, which is acceptable for substring scraping
            // (patterns are ASCII) and never panics.
            self.window.push(byte as char);
        }
    }

    /// Truncate the window from the FRONT (drop-oldest) once it exceeds `window_bytes`,
    /// keeping the most recent text on a char boundary.
    fn truncate_window(&mut self) {
        if self.window.len() <= self.window_bytes {
            return;
        }
        let overflow = self.window.len() - self.window_bytes;
        // Advance to the next char boundary at or after `overflow` so the drain never
        // splits a multi-byte scalar.
        let mut cut = overflow;
        while cut < self.window.len() && !self.window.is_char_boundary(cut) {
            cut += 1;
        }
        self.window.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: usize = 4096;

    fn snapshot_now(p: &OutputSignalProducer) -> OutputSignal {
        p.snapshot(Timestamp(1_000), 0, true)
    }

    #[test]
    fn producer_strips_ansi_csi_sgr_sequences() {
        let mut p = OutputSignalProducer::new(WINDOW);
        // A red "red" wrapped in SGR colour CSI sequences.
        p.ingest(b"\x1b[31mred\x1b[0m");
        assert_eq!(snapshot_now(&p).text_window, "red");
    }

    #[test]
    fn producer_strips_csi_cursor_moves() {
        let mut p = OutputSignalProducer::new(WINDOW);
        // CSI cursor-home + clear-screen around plain text.
        p.ingest(b"\x1b[Hclean\x1b[2J");
        assert_eq!(snapshot_now(&p).text_window, "clean");
    }

    #[test]
    fn producer_strips_osc_title_sequence_bel_terminated() {
        let mut p = OutputSignalProducer::new(WINDOW);
        // OSC set-window-title terminated by BEL, then printable text.
        p.ingest(b"\x1b]0;my title\x07prompt> ");
        assert_eq!(snapshot_now(&p).text_window, "prompt> ");
    }

    #[test]
    fn producer_strips_osc_title_sequence_st_terminated() {
        let mut p = OutputSignalProducer::new(WINDOW);
        // OSC terminated by the ESC-backslash String Terminator.
        p.ingest(b"\x1b]0;title\x1b\\done");
        assert_eq!(snapshot_now(&p).text_window, "done");
    }

    #[test]
    fn producer_handles_escape_split_across_two_ingests() {
        let mut p = OutputSignalProducer::new(WINDOW);
        // A single SGR sequence `\x1b[31m` split between two raw chunks, proving the
        // ANSI state machine persists across ingest calls.
        p.ingest(b"\x1b[");
        p.ingest(b"31mred");
        assert_eq!(snapshot_now(&p).text_window, "red");
    }

    #[test]
    fn producer_window_truncates_from_front_at_window_bytes() {
        let mut p = OutputSignalProducer::new(8);
        // Feed 12 printable bytes into an 8-byte window: the oldest 4 drop, keeping the
        // most recent 8.
        p.ingest(b"0123456789ab");
        assert_eq!(snapshot_now(&p).text_window, "456789ab");
    }

    #[test]
    fn producer_snapshot_carries_caller_supplied_time_fields() {
        let mut p = OutputSignalProducer::new(WINDOW);
        p.ingest(b"hi");
        // The caller (read loop) owns the clock and supplies these (D10).
        let signal = p.snapshot(Timestamp(5_000), 250, false);
        assert_eq!(signal.last_byte_at, Timestamp(5_000));
        assert_eq!(signal.idle_ms, 250);
        assert!(!signal.is_active);
        assert_eq!(signal.text_window, "hi");
    }

    #[test]
    fn producer_mark_exit_sets_exit_code_and_preserves_window() {
        let mut p = OutputSignalProducer::new(WINDOW);
        p.ingest(b"output before exit");
        p.mark_exit(0);
        let signal = snapshot_now(&p);
        // The window text is NOT erased on exit, so the final terminal signal still
        // carries the scraped output alongside the exit code.
        assert_eq!(signal.exit_code, Some(0));
        assert_eq!(signal.text_window, "output before exit");
    }

    #[test]
    fn producer_quiesce_then_exit_makes_ready_before_finished_reachable() {
        // Carry-forward from the PR2a fresh review: a clean exit from `Starting` would
        // no-op (the transition table forbids `Starting -> Completed`). The producer
        // must make a quiescent (non-active) `Ready` snapshot OBSERVABLE before the
        // terminal-exit snapshot.
        let mut p = OutputSignalProducer::new(WINDOW);
        p.ingest(b"work done\n");

        // Quiescent snapshot: is_active=false, no exit code yet -> a runner can observe
        // Ready (and the registry can reach Idle) BEFORE the process exits.
        let quiescent = p.snapshot(Timestamp(2_000), 500, false);
        assert!(
            !quiescent.is_active,
            "quiescent snapshot must be non-active"
        );
        assert_eq!(quiescent.exit_code, None, "no exit code before mark_exit");

        // Later terminal snapshot still carries the windowed text + the exit code, so a
        // `Ready -> Finished` ordering is reachable.
        // WU-9: the read loop enforces emitting the quiescent snapshot first.
        p.mark_exit(0);
        let terminal = p.snapshot(Timestamp(3_000), 1_000, false);
        assert_eq!(terminal.exit_code, Some(0));
        assert_eq!(terminal.text_window, "work done\n");
    }
}
