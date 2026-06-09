//! WU-9 — `#[cfg(unix)]` integration tests: path-agreement and real-PTY hook→status.
//!
//! ## 9.1 `spectty_hook_end_to_end_monotonic_ts_and_path_agreement`
//!
//! Asserts ALL of:
//! (a) spawn the built `spectty-hook --event Stop` binary in a temp runtime dir
//!     with `SPECTTY_SESSION_ID=itest`; assert the written `.state` parses to
//!     `{event: "Stop", ts: 1, session_id: "itest"}`.
//! (b) Run it again → `ts: 2` (monotonic counter confirmed across invocations).
//! (c) PATH AGREEMENT (D25, LOAD-BEARING): assert `spectty_lib::spectty_runtime_dir()`
//!     and `spectty_hook::spectty_runtime_dir()` resolve to the SAME path. A SINGLE
//!     test calling both functions directly makes this divergence IMPOSSIBLE to land
//!     undetected: if either formula changes without the other, this test fails.
//!     `spectty-hook` is added as `[dev-dependencies]` in `src-tauri/Cargo.toml`
//!     with a thin `[lib]` target re-exporting `spectty_runtime_dir`.
//!
//! ## 9.2 `real_pty_hook_sourced_stop_emits_idle`
//!
//! Runs `run_signal_loop` over a REAL PTY with `hooks_active = true`. Writes a
//! `.state` file out-of-band (`{Stop, ts:1, session_id}`) into the reader's runtime
//! dir while the loop is live. Asserts `StatusChanged(Idle)` is emitted via the
//! hook-sourced path (NOT scraping — hooks_active=true gates scraping-derived Ready
//! from Running, so Idle MUST come from the hook).
//!
//! ## Flakiness rule
//! All timing-dependent waits use bounded polls (≥5s deadline, 10ms interval).
//! Never a single fixed sleep — see commit b62826a for the de-flake pattern.

// Only compiled + run on Unix (the PTY APIs and the hook binary are Unix-only).
#[cfg(unix)]
mod tests {
    use spectty_adapters::StateFileReader;
    use spectty_core::entities::agent_spec::{AgentKind, AgentSpec, AgentTier};
    use spectty_core::entities::session::Session;
    use spectty_core::entities::workspace::WorkspaceId;
    use spectty_core::ports::clock::Timestamp;
    use spectty_core::{AgentStatus, SessionId, SessionRegistry};
    use spectty_lib::session_runtime::{
        run_signal_loop, signal_channel, signal_try_send, StatusChanged, SIGNAL_CHANNEL_CAP,
    };
    use std::time::Duration;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Path to the built `spectty-hook` binary, derived from the workspace root.
    ///
    /// When `cargo test --workspace` runs, all workspace binaries are compiled
    /// before integration tests execute, so `target/debug/spectty-hook` exists.
    /// Derived from `CARGO_MANIFEST_DIR` (src-tauri) → parent (workspace root).
    fn hook_bin_path() -> std::path::PathBuf {
        let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR has a parent (workspace root)");
        let bin = ws_root.join("target").join("debug").join("spectty-hook");
        assert!(
            bin.exists(),
            "spectty-hook binary not found at {bin:?}. \
             Ensure `cargo build -p spectty-hook` ran before this integration test."
        );
        bin
    }

    /// Create a unique temp runtime dir for this test invocation (uses PID to
    /// avoid collisions with parallel test runs).
    fn unique_runtime_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("spectty-wu9-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp runtime dir creatable");
        dir
    }

    fn registry_with(status: AgentStatus) -> (SessionRegistry, SessionId) {
        let registry = SessionRegistry::default();
        let id = registry.mint_id();
        registry.insert(Session {
            id: id.clone(),
            workspace: WorkspaceId("/repo".to_string()),
            agent: AgentSpec {
                kind: AgentKind("claude-code".to_string()),
                command: None,
                tier: AgentTier::Cooperative,
            },
            status,
            title: "integration-test".to_string(),
            created_at: Timestamp(0),
        });
        (registry, id)
    }

    // ── WU-9.1 ───────────────────────────────────────────────────────────────

    /// WU-9.1 — all three assertions: (a) spawn→ts:1, (b) respawn→ts:2, (c) path
    /// agreement.
    ///
    /// ## RED proof
    ///
    /// Before adding the `spectty-hook` `[lib]` target + dev-dep:
    ///   `spectty_hook::spectty_runtime_dir` does not exist → compile error → RED.
    ///
    /// After setup, to confirm parts (a)+(b) go RED:
    ///   Temporarily change the assertion to `assert_eq!(val2["ts"], 99u64)` and
    ///   run `cargo test -p spectty spectty_hook_end_to_end` — the test fails with
    ///   `left: 2, right: 99`.
    ///
    /// To confirm part (c) goes RED:
    ///   Temporarily change `spectty_hook::spectty_runtime_dir()` to return a
    ///   different suffix (e.g. `"wrong"`) and re-run — the path-agreement assert
    ///   fires immediately.
    ///
    /// ## SPECTTY_RUNTIME_DIR env var
    ///
    /// `spectty-hook` binary honours `SPECTTY_RUNTIME_DIR` as an override for
    /// the resolved `spectty_runtime_dir()` path (added in this WU for the
    /// integration test seam — consistent with the existing `run_with` DI pattern).
    /// It allows pointing the binary at a PID-unique temp dir rather than the real
    /// `~/Library/Application Support/app.spectty.desktop/runtime`.
    #[test]
    fn spectty_hook_end_to_end_monotonic_ts_and_path_agreement() {
        // ── (a) First invocation: ts must be 1 ───────────────────────────────
        let runtime_dir = unique_runtime_dir("91a");
        let session_id = "itest";
        let hook_bin = hook_bin_path();

        let status = std::process::Command::new(&hook_bin)
            .arg("--event")
            .arg("Stop")
            .env("SPECTTY_SESSION_ID", session_id)
            .env("SPECTTY_RUNTIME_DIR", runtime_dir.to_str().unwrap())
            .stdin(std::process::Stdio::null())
            .status()
            .expect("spectty-hook first spawn");
        assert!(
            status.success(),
            "(a) first invocation must exit 0; got {status:?}"
        );

        let state_path = runtime_dir.join(format!("spectty-{session_id}.state"));
        let raw = std::fs::read_to_string(&state_path)
            .expect("state file must exist after first invocation");
        let val: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");

        assert_eq!(val["event"], "Stop", "(a) event must be 'Stop'");
        assert_eq!(val["ts"], 1u64, "(a) first invocation ts must be 1");
        assert_eq!(
            val["session_id"], session_id,
            "(a) session_id must match SPECTTY_SESSION_ID"
        );

        // ── (b) Second invocation: ts must be 2 ──────────────────────────────
        let status2 = std::process::Command::new(&hook_bin)
            .arg("--event")
            .arg("Stop")
            .env("SPECTTY_SESSION_ID", session_id)
            .env("SPECTTY_RUNTIME_DIR", runtime_dir.to_str().unwrap())
            .stdin(std::process::Stdio::null())
            .status()
            .expect("spectty-hook second spawn");
        assert!(
            status2.success(),
            "(b) second invocation must exit 0; got {status2:?}"
        );

        let raw2 = std::fs::read_to_string(&state_path).expect("state file after second run");
        let val2: serde_json::Value = serde_json::from_str(&raw2).expect("valid JSON");
        assert_eq!(
            val2["ts"], 2u64,
            "(b) monotonic counter: second invocation ts must be 2"
        );

        // ── (c) PATH AGREEMENT (D25, LOAD-BEARING) ────────────────────────────
        // Both resolvers must return the identical path. If they diverge, the
        // sidecar writes state files to a dir the reader never polls — status
        // updates silently stop working. This single call-site comparison makes
        // that class of divergence IMPOSSIBLE to merge undetected.
        let hook_dir = spectty_hook::spectty_runtime_dir();
        let tauri_str = spectty_lib::spectty_runtime_dir();
        let tauri_dir = std::path::PathBuf::from(&tauri_str);

        let hook_dir =
            hook_dir.expect("spectty_hook::spectty_runtime_dir() must resolve (HOME must be set)");
        assert_ne!(
            tauri_str, "",
            "(c) spectty_lib::spectty_runtime_dir() must return non-empty (HOME must be set)"
        );
        assert_eq!(
            hook_dir, tauri_dir,
            "(c) D25 PATH AGREEMENT VIOLATED: \
             spectty-hook resolves {hook_dir:?} but src-tauri resolves {tauri_dir:?}; \
             hook state files would be written to a path the reader never polls"
        );

        // Cleanup: remove the temp runtime dir.
        let _ = std::fs::remove_dir_all(&runtime_dir);
    }

    // ── WU-9.2 ───────────────────────────────────────────────────────────────

    /// WU-9.2 — write a `.state` file out-of-band while `run_signal_loop` is running
    /// over a real PTY; assert `StatusChanged(Idle)` is emitted via the hook-sourced
    /// path.
    ///
    /// `hooks_active = true` suppresses scraping-derived `Ready` from `Running`, so
    /// the ONLY way `Running → Idle` can occur is via a hook `Stop` event. This
    /// proves the hook-sourced path works end-to-end.
    ///
    /// ## RED proof
    ///
    /// Comment out the `std::fs::write` call that injects the state file and run
    /// the test. Without the hook file the loop never fires `emit_hook_if_present`,
    /// and with `hooks_active=true` scraping is suppressed — so no `Idle` is
    /// emitted within the 5s window. The assertion fires: "hook-sourced Stop must
    /// emit StatusChanged(Idle) within 5s; got []" → RED confirmed.
    ///
    /// Restore the `std::fs::write` line → GREEN.
    ///
    /// ## Flakiness rule (hard requirement — see commit b62826a)
    ///
    /// Timing-dependent wait uses a bounded poll (5s deadline, 10ms interval).
    #[test]
    fn real_pty_hook_sourced_stop_emits_idle() {
        use spectty_adapters::{PtyAdapter, PtySpawnConfig, PtyTransport, SystemClock};
        use std::io::Read;
        use std::sync::{Arc, Mutex};

        // ── PTY setup ────────────────────────────────────────────────────────
        // Long-running but deterministic command so the PTY stays alive
        // throughout the test. Killed explicitly at the end.
        let cfg = PtySpawnConfig {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "sleep 30".to_string()],
            cwd: None,
            cols: 80,
            rows: 24,
            env: Vec::new(),
        };
        let (mut adapter, mut reader) = PtyAdapter::spawn(&cfg).expect("real PTY spawns");

        // ── Session in Running state (hook will drive Running → Idle) ─────────
        let (sessions, id) = registry_with(AgentStatus::Running);
        let sessions = Arc::new(sessions);
        let id_arc = Arc::new(id);
        let emitted: Arc<Mutex<Vec<StatusChanged>>> = Arc::new(Mutex::new(Vec::new()));

        // ── Temp runtime dir ──────────────────────────────────────────────────
        let runtime_dir = unique_runtime_dir("92");
        let session_id_str = id_arc.0.clone();
        let state_path = runtime_dir.join(format!("spectty-{session_id_str}.state"));

        // ── Signal channel ────────────────────────────────────────────────────
        let (tx, rx) = signal_channel(SIGNAL_CHANNEL_CAP);
        let tx_reader = tx.clone(); // held by the reader thread

        // ── Reader thread: tee PTY output onto the signal channel ─────────────
        let reader_handle = std::thread::spawn(move || {
            let mut buf = [0u8; 8 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = signal_try_send(&tx_reader, buf[..n].to_vec());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            // Drop tx_reader → eventually the signal loop gets Disconnected
            // after the main tx is also dropped.
        });

        // ── Signal loop thread ────────────────────────────────────────────────
        let sessions_t = Arc::clone(&sessions);
        let id_t = Arc::clone(&id_arc);
        let emitted_t = Arc::clone(&emitted);
        let runtime_for_loop = runtime_dir.to_string_lossy().into_owned();
        let sid_for_loop = session_id_str.clone();

        let loop_handle = std::thread::spawn(move || {
            let runner = spectty_adapters::ClaudeCodeRunner::new();
            let clock = SystemClock::new();
            // hooks_active = true: scraping-derived Ready from Running is suppressed.
            // The only way Running→Idle is via hook_reader.poll() returning a Stop event.
            let mut hook_reader = StateFileReader::new(&runtime_for_loop, &sid_for_loop);

            run_signal_loop(
                &rx,
                &runner,
                &sessions_t,
                &id_t,
                &clock,
                &mut hook_reader,
                true, // hooks_active — gate is ON
                || 0,
                |sc| emitted_t.lock().unwrap().push(sc),
            );
        });

        // ── Give the loop a brief startup grace ───────────────────────────────
        // Wait long enough for the signal loop thread to be scheduled and reach
        // its first recv_timeout call. 100ms is generous; the thread typically
        // starts in < 1ms.
        std::thread::sleep(Duration::from_millis(100));

        // ── ACT: write the state file out-of-band ─────────────────────────────
        // The signal loop's Quiesce arm polls the hook reader on every 200ms tick.
        // After this write the NEXT Quiesce tick (within 200ms) will call
        // StateFileReader::poll(), parse the file, and emit StatusChanged(Idle).
        //
        // RED proof: comment this write out → no Idle emitted → assertion fails.
        let state_json = format!(r#"{{"event":"Stop","ts":1,"session_id":"{session_id_str}"}}"#);
        std::fs::write(&state_path, &state_json)
            .expect("out-of-band state file write must succeed");

        // ── ASSERT: bounded poll for Idle (de-flake rule, commit b62826a) ─────
        // Deadline ≥ 5s so a loaded CI runner (slow recv_timeout) cannot flake.
        // Interval 10ms keeps the test fast when the hook fires quickly.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut idle_emitted = false;
        while std::time::Instant::now() < deadline {
            if emitted
                .lock()
                .unwrap()
                .iter()
                .any(|sc| sc.status == AgentStatus::Idle)
            {
                idle_emitted = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Snapshot statuses BEFORE teardown for the diagnostic message — after
        // kill/EOF the loop emits Idle+Completed via the EOF arm (ungated), which
        // would obscure the failure message when the hook path is the one under test.
        let statuses_before_teardown: Vec<AgentStatus> =
            emitted.lock().unwrap().iter().map(|sc| sc.status).collect();

        // ── Teardown ──────────────────────────────────────────────────────────
        let _ = adapter.kill();
        // Drop the main tx so the signal loop exits on Disconnected (EOF path).
        drop(tx);
        let _ = reader_handle.join();
        let _ = loop_handle.join();
        let _ = std::fs::remove_dir_all(&runtime_dir);

        // ── Verify ────────────────────────────────────────────────────────────
        // Confirm Idle was emitted WITHIN the 5s poll window (before teardown).
        // Because hooks_active=true, scraping-derived Ready is suppressed from
        // Running — the ONLY source of Running→Idle within the poll window is
        // the hook Stop event we wrote out-of-band above. This proves the
        // hook-sourced path works end-to-end through the real pipeline.
        assert!(
            idle_emitted,
            "9.2: hook-sourced Stop must emit StatusChanged(Idle) within 5s \
             (hooks_active=true, scraping-derived Ready suppressed from Running); \
             statuses during poll window: {statuses_before_teardown:?}"
        );
    }
}
