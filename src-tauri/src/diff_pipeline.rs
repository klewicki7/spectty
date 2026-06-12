//! The diff pipeline — VibeLens's trigger arbitration + hash-dedup core (D37, WU-8).
//!
//! When a workspace changes, the pipeline runs `git diff HEAD`, dedups it against the last
//! seen diff, and (on a genuine change) builds + pushes a [`DiffExplanation`] and emits a
//! `diff_updated` event EXACTLY ONCE. Two triggers feed the SAME pipeline (D37):
//!
//! - **Cooperative** (`spectty_diff` signal): fires immediately, bypassing the FileWatch
//!   debounce — low latency for agents that announce their own edits.
//! - **Generic** (debounced `FileWatchPort`): the fallback for agents with
//!   `emits_diff_signals == false` (every agent today), so a generic agent still gets
//!   VibeLens via file-system watching.
//!
//! ```text
//! FileWatch(debounced, .git/-filtered) | spectty_diff ─▶ GitPort::diff_head
//!     ─▶ hash == last? skip : build+push explain ─▶ emit diff_updated (once)
//! ```
//!
//! ## Why the decision step is pure
//!
//! [`DiffPipeline::run_once`] takes the three Core ports plus an injected `emit` closure and
//! returns the outcome — no `AppHandle`, no thread, no clock — so the dedup/skip/degrade
//! policy is unit-testable against fakes, mirroring `spec_bus::SpecBus` and
//! `session_runtime::observe_and_diff`.
//!
//! ## Dedup state ownership (DEVIATION from the D34 sketch)
//!
//! The design sketched the dedup hash living on the `Session` aggregate (`last_diff_hash`).
//! This slice keeps it on the per-session [`DiffPipeline`] instead: the pipeline is the
//! SOLE owner of a session's diff cadence, so co-locating the hash there is behaviourally
//! identical AND avoids adding a `Session` mutation seam to Core (the R6 quarantine forbids
//! new Core code this slice — adapters + src-tauri + mcp only). The Core `Session::update_diff`
//! / `last_diff_hash` fields from PR-4 stay available for a later registry-backed wiring.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use spectty_core::entities::diff::DiffExplanation;
use spectty_core::ports::{DiffExplainerPort, GitError, GitPort};

/// The `diff_updated` event payload (D29/M4-REQ-17): the session whose worktree was
/// re-explained plus the new [`DiffExplanation`]. Emitted via the Tauri v2 `Emitter` only on
/// an ACTUAL change. Kept here (not in `commands/`) so the pipeline owns its output type and
/// stays Tauri-free; `commands/session.rs` supplies the `app.emit` closure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiffUpdated {
    /// The session this diff belongs to.
    pub session_id: String,
    /// The freshly built explanation (per-file rationale + summary).
    pub explanation: DiffExplanation,
}

/// The outcome of one pipeline run — returned so callers (and tests) can see what happened
/// without inspecting side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// A genuine change: the explainer ran and `diff_updated` was emitted with this diff hash.
    Emitted(u64),
    /// The diff hash matched the last seen one: no explain call, no emit.
    SkippedUnchanged,
    /// The diff was empty (clean tree): `DiffExplanation::empty()` cached, no explain call,
    /// no emit (M4-REQ-13: a truly empty diff yields `empty()` with NO MCP call).
    Empty,
    /// A failure mode (git error, or an already-in-flight run): degraded, previous retained,
    /// no emit, session stays alive (M4-REQ-14).
    Degraded,
}

/// Whether a changed path should TRIGGER a re-diff. Excludes everything under a `.git/`
/// directory (WU-8.0): without this the pipeline self-triggers — running `git diff` churns
/// `.git/index`, which fires a file event, which re-diffs, forever. Hash-dedup blunts the
/// explain cost but the watcher still wakes per internal git write, so we filter at the
/// source.
#[must_use]
pub fn is_triggering_path(path: &Path) -> bool {
    !path
        .components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new(".git"))
}

/// Filter a debounced [`FileChanged`](spectty_core::ports::FileChanged) batch's paths to the
/// ones that should trigger a re-diff (WU-8.0). Returns `true` if ANY path survives the
/// `.git/` filter — i.e. the batch represents a real workspace edit, not git's own churn.
#[must_use]
pub fn batch_should_trigger(paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| is_triggering_path(p))
}

/// Hash a unified diff string for dedup (std-only `DefaultHasher`, D34). Equal hashes ⇒
/// treat as unchanged.
#[must_use]
pub fn hash_diff(diff: &str) -> u64 {
    let mut h = DefaultHasher::new();
    diff.hash(&mut h);
    h.finish()
}

/// The per-session diff pipeline (D37). Holds the workspace + the dedup hash + an in-flight
/// guard; `run_once` is the pure trigger→diff→dedup→explain→emit step over the injected
/// ports. Shared behind `Arc` across the cooperative-trigger and FileWatch paths so both
/// triggers fan into ONE deduped pipeline.
pub struct DiffPipeline {
    session_id: String,
    workspace: PathBuf,
    /// Dedup + in-flight state, behind one `Mutex` so concurrent triggers (cooperative +
    /// FileWatch) serialize: the SECOND of two simultaneous fires sees `in_flight` and
    /// degrades to a no-op rather than double-explaining (D37 shared in-flight guard).
    state: Mutex<PipelineState>,
}

#[derive(Default)]
struct PipelineState {
    /// Hash of the last diff that produced an emit (or the empty marker). `None` until the
    /// first run.
    last_hash: Option<u64>,
    /// The last explanation produced (or `empty()` for a clean tree). Served by
    /// [`DiffPipeline::current_explanation`] so the `get_diff_explanation` command can read
    /// the latest without re-running the pipeline. `None` until the first run.
    last_explanation: Option<DiffExplanation>,
    /// `true` while a `run_once` is mid-flight, so a concurrent trigger short-circuits.
    in_flight: bool,
}

impl DiffPipeline {
    /// Build a pipeline for `session_id` watching `workspace`.
    pub fn new(session_id: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            session_id: session_id.into(),
            workspace: workspace.into(),
            state: Mutex::new(PipelineState::default()),
        }
    }

    /// Run ONE pipeline pass (a trigger fired). Pure over the injected ports + `emit`:
    ///
    /// 1. In-flight guard: if another pass is running, degrade to a no-op (the running pass
    ///    will pick up the latest tree; hash-dedup makes a double-trigger harmless).
    /// 2. `git diff HEAD` → on error, degrade (log via the caller; retain previous), no emit.
    /// 3. Empty diff → cache the empty marker, NO explain call, NO emit (M4-REQ-13).
    /// 4. Hash unchanged vs `last_hash` → skip (no explain, no emit).
    /// 5. Changed → `explain` (build + push), advance `last_hash`, emit `diff_updated` ONCE.
    ///
    /// The `explain` failure mode is owned by the adapter (VibeLens unreachable still returns
    /// the locally-built explanation — M4-REQ-14), so an `Ok` explanation always emits; an
    /// `Err` from the port degrades (retain previous, no emit).
    pub fn run_once(
        &self,
        git: &dyn GitPort,
        explainer: &dyn DiffExplainerPort,
        emit: &mut dyn FnMut(DiffUpdated),
    ) -> RunOutcome {
        // 1. In-flight guard (D37): claim the slot or bail.
        {
            let mut state = self.state.lock().expect("diff pipeline state poisoned");
            if state.in_flight {
                return RunOutcome::Degraded;
            }
            state.in_flight = true;
        }
        // Always release the in-flight flag, even on an early return.
        let outcome = self.run_inner(git, explainer, emit);
        self.state
            .lock()
            .expect("diff pipeline state poisoned")
            .in_flight = false;
        outcome
    }

    /// The body of [`run_once`](Self::run_once), run with the in-flight slot held.
    fn run_inner(
        &self,
        git: &dyn GitPort,
        explainer: &dyn DiffExplainerPort,
        emit: &mut dyn FnMut(DiffUpdated),
    ) -> RunOutcome {
        // 2. Read the working-tree diff. A git failure degrades (retain previous, no crash).
        let diff = match git.diff_head(&self.workspace) {
            Ok(diff) => diff,
            Err(GitError::Backend(_)) => return RunOutcome::Degraded,
        };

        // 3. Empty diff: cache the empty marker + explanation WITHOUT calling the explainer
        //    (M4-REQ-13). The cached empty explanation is what `get_diff_explanation` serves
        //    for a clean tree (the VibeLens panel renders its "no changes" state).
        if diff.trim().is_empty() {
            let empty_hash = hash_diff("");
            let mut state = self.state.lock().expect("diff pipeline state poisoned");
            state.last_hash = Some(empty_hash);
            state.last_explanation = Some(DiffExplanation::empty());
            return RunOutcome::Empty;
        }

        // 4. Hash-dedup: an unchanged hash skips the explainer + emit (M4-REQ-13).
        let hash = hash_diff(&diff);
        if self
            .state
            .lock()
            .expect("diff pipeline state poisoned")
            .last_hash
            == Some(hash)
        {
            return RunOutcome::SkippedUnchanged;
        }

        // 5. Changed: explain (build + best-effort push). A port Err degrades; an Ok
        //    explanation (even a degraded VibeLens push) advances the hash and emits once.
        match explainer.explain(&diff, &self.workspace) {
            Ok(explanation) => {
                {
                    let mut state = self.state.lock().expect("diff pipeline state poisoned");
                    state.last_hash = Some(hash);
                    state.last_explanation = Some(explanation.clone());
                }
                emit(DiffUpdated {
                    session_id: self.session_id.clone(),
                    explanation,
                });
                RunOutcome::Emitted(hash)
            }
            // The adapter returns Ok even when VibeLens is unreachable (M4-REQ-14), so an
            // Err here is a genuine explainer failure: degrade, retain previous, no emit.
            Err(_) => RunOutcome::Degraded,
        }
    }

    /// The last [`DiffExplanation`] this pipeline produced, or `None` if it has not run yet
    /// (served by the `get_diff_explanation` command — M4-REQ-16). A clean tree caches
    /// `DiffExplanation::empty()`, so a "no changes" state is distinguishable from "not yet
    /// run".
    #[must_use]
    pub fn current_explanation(&self) -> Option<DiffExplanation> {
        self.state
            .lock()
            .expect("diff pipeline state poisoned")
            .last_explanation
            .clone()
    }

    /// The session this pipeline belongs to.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// A shared, per-session pipeline handle the trigger sites clone (FileWatch callback +
/// cooperative `spectty_diff` poll both hold one).
pub type SharedPipeline = Arc<DiffPipeline>;

/// Managed Tauri state: the live diff pipelines keyed by session id. `spawn_session`
/// registers a session's pipeline here; `get_diff_explanation` reads the latest explanation
/// from it; `close_session` removes it. The `Mutex` guards concurrent access from command
/// handlers and the per-session trigger threads.
#[derive(Default)]
pub struct DiffPipelines(pub Mutex<std::collections::HashMap<String, SharedPipeline>>);

impl DiffPipelines {
    /// Register `pipeline` for its session id.
    pub fn insert(&self, pipeline: SharedPipeline) {
        self.0
            .lock()
            .expect("diff pipelines mutex poisoned")
            .insert(pipeline.session_id().to_string(), pipeline);
    }

    /// The pipeline for `session_id`, if registered.
    #[must_use]
    pub fn get(&self, session_id: &str) -> Option<SharedPipeline> {
        self.0
            .lock()
            .expect("diff pipelines mutex poisoned")
            .get(session_id)
            .cloned()
    }

    /// Remove (and return) the pipeline for `session_id` on session close.
    pub fn remove(&self, session_id: &str) -> Option<SharedPipeline> {
        self.0
            .lock()
            .expect("diff pipelines mutex poisoned")
            .remove(session_id)
    }

    /// The current explanation for `session_id`, or `None` when the session has no pipeline
    /// or the pipeline has not run yet. The `get_diff_explanation` command body (M4-REQ-16).
    #[must_use]
    pub fn current_explanation(&self, session_id: &str) -> Option<DiffExplanation> {
        self.get(session_id).and_then(|p| p.current_explanation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectty_core::entities::diff::FileExplanation;
    use spectty_core::ports::ExplainError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A `GitPort` fake: returns a scripted diff string (or a backend error).
    struct FakeGit {
        result: Result<String, ()>,
        calls: AtomicUsize,
    }
    impl FakeGit {
        fn ok(diff: &str) -> Self {
            Self {
                result: Ok(diff.to_string()),
                calls: AtomicUsize::new(0),
            }
        }
        fn err() -> Self {
            Self {
                result: Err(()),
                calls: AtomicUsize::new(0),
            }
        }
    }
    impl GitPort for FakeGit {
        fn diff_head(&self, _ws: &Path) -> Result<String, GitError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result
                .clone()
                .map_err(|()| GitError::Backend("fake git failure".to_string()))
        }
    }

    /// A `DiffExplainerPort` fake: counts calls and returns a built explanation (or an error).
    struct FakeExplainer {
        calls: AtomicUsize,
        fail: bool,
    }
    impl FakeExplainer {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: true,
            }
        }
        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }
    impl DiffExplainerPort for FakeExplainer {
        fn explain(&self, diff: &str, _ws: &Path) -> Result<DiffExplanation, ExplainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(ExplainError::Unavailable("fake".to_string()));
            }
            Ok(DiffExplanation {
                files: vec![FileExplanation {
                    path: "f.rs".to_string(),
                    rationale: format!("explained {} bytes", diff.len()),
                }],
                summary: "fake summary".to_string(),
            })
        }
    }

    fn pipeline() -> DiffPipeline {
        DiffPipeline::new("s1", "/ws")
    }

    // WU-8.3: an unchanged diff hash skips the explainer and emits nothing.
    #[test]
    fn pipeline_skips_explain_when_hash_unchanged() {
        let p = pipeline();
        let git = FakeGit::ok("diff --git a/f.rs b/f.rs\n+x\n");
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        // First run: change → explain + emit.
        let first = p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        assert!(matches!(first, RunOutcome::Emitted(_)));
        assert_eq!(emitted.len(), 1);
        assert_eq!(explainer.call_count(), 1);

        // Second run, SAME diff → skip (no explain, no emit).
        let second = p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        assert_eq!(second, RunOutcome::SkippedUnchanged);
        assert_eq!(emitted.len(), 1, "unchanged hash must not re-emit");
        assert_eq!(
            explainer.call_count(),
            1,
            "unchanged hash must not re-explain"
        );
    }

    // WU-8.4: a changed diff explains once and emits exactly one diff_updated with the id.
    #[test]
    fn pipeline_explains_and_emits_once_on_change() {
        let p = pipeline();
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        let git1 = FakeGit::ok("diff --git a/f.rs b/f.rs\n+one\n");
        p.run_once(&git1, &explainer, &mut |e| emitted.push(e));

        // A genuinely different diff → another explain + emit.
        let git2 = FakeGit::ok("diff --git a/f.rs b/f.rs\n+one\n+two\n");
        let outcome = p.run_once(&git2, &explainer, &mut |e| emitted.push(e));

        assert!(matches!(outcome, RunOutcome::Emitted(_)));
        assert_eq!(emitted.len(), 2, "each change emits exactly once");
        assert_eq!(explainer.call_count(), 2);
        assert!(emitted.iter().all(|e| e.session_id == "s1"));
        assert_eq!(emitted[1].explanation.summary, "fake summary");
    }

    // WU-8.5: a truly empty diff yields empty() with NO explainer call and NO emit.
    #[test]
    fn pipeline_truly_empty_diff_is_empty_no_mcp_call() {
        let p = pipeline();
        let git = FakeGit::ok("   \n"); // clean tree
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        let outcome = p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        assert_eq!(outcome, RunOutcome::Empty);
        assert_eq!(
            explainer.call_count(),
            0,
            "empty diff must not call the explainer"
        );
        assert!(emitted.is_empty(), "empty diff must not emit");
    }

    // WU-8.6: a git failure degrades — no panic, no emit, session survives.
    #[test]
    fn pipeline_degrades_on_git_failure() {
        let p = pipeline();
        let git = FakeGit::err();
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        let outcome = p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        assert_eq!(outcome, RunOutcome::Degraded);
        assert_eq!(explainer.call_count(), 0);
        assert!(emitted.is_empty());
    }

    // M4-REQ-14: an explainer Err (a genuine failure, not a best-effort VibeLens push
    // degrade) degrades — previous retained, no emit, no crash.
    #[test]
    fn pipeline_degrades_on_explainer_error() {
        let p = pipeline();
        let git = FakeGit::ok("diff --git a/f.rs b/f.rs\n+x\n");
        let explainer = FakeExplainer::failing();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        let outcome = p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        assert_eq!(outcome, RunOutcome::Degraded);
        assert!(emitted.is_empty(), "an explainer error must not emit");
    }

    // WU-8.0: .git/ paths must NOT trigger a re-diff (the pipeline would self-trigger on
    // git's own index churn otherwise). Real workspace paths DO trigger.
    #[test]
    fn git_internal_paths_do_not_trigger_workspace_edits_do() {
        assert!(!is_triggering_path(Path::new("/ws/.git/index")));
        assert!(!is_triggering_path(Path::new("/ws/.git/objects/ab/cd")));
        assert!(is_triggering_path(Path::new("/ws/src/lib.rs")));
        assert!(is_triggering_path(Path::new("/ws/README.md")));

        // A batch of ONLY .git/ churn must not trigger; a batch with a real edit must.
        assert!(!batch_should_trigger(&[
            PathBuf::from("/ws/.git/index"),
            PathBuf::from("/ws/.git/HEAD"),
        ]));
        assert!(batch_should_trigger(&[
            PathBuf::from("/ws/.git/index"),
            PathBuf::from("/ws/src/main.rs"),
        ]));
    }

    // M4-REQ-16: get_diff_explanation reads the pipeline's cached explanation. None before
    // the first run; the built explanation after a change; empty() after a clean tree.
    #[test]
    fn current_explanation_serves_the_last_built_explanation() {
        let p = pipeline();
        assert!(
            p.current_explanation().is_none(),
            "none before the first run"
        );

        let explainer = FakeExplainer::new();
        let git = FakeGit::ok("diff --git a/f.rs b/f.rs\n+x\n");
        p.run_once(&git, &explainer, &mut |_| {});
        let after = p.current_explanation().expect("cached after a change");
        assert_eq!(after.summary, "fake summary");

        // A clean tree caches the empty explanation (the panel's "no changes" state).
        let clean = FakeGit::ok("");
        p.run_once(&clean, &explainer, &mut |_| {});
        assert!(
            p.current_explanation().expect("cached").is_empty(),
            "a clean tree caches empty(), distinguishable from not-yet-run"
        );
    }

    // ── F4 (PR-5 review) — app-side cross-seam fixture test for the `spectty_diff` trigger
    //    doc. The MCP side pins the trigger-doc SHAPE (spectty-mcp wire test); this pins the
    //    APP side: the EXACT JSON literal the MCP `spectty_diff` effect upserts to
    //    `spectty/{sid}/diff` (session_id + hint + nonce) must, when driven through the real
    //    PortPollReader + SpecBus poll seam ONE tick, fire the diff pipeline — and a SECOND
    //    distinct-nonce doc must fire it AGAIN (the consume-once/nonce contract: a rapid
    //    re-edit with the same hint still re-triggers because the nonce makes the doc differ).
    //
    //    The trigger doc literal is copied from crates/spectty-mcp/src/main.rs (the
    //    `spectty_diff_effect` `trigger` json!{} at ~:235-239), INCLUDING the `nonce` field.
    #[test]
    fn spectty_diff_trigger_doc_drives_pipeline_through_app_poll_seam() {
        use crate::spec_bus::{PollReader, PortPollReader, SpecBus};
        use spectty_adapters::InMemoryPersistenceAdapter;
        use spectty_core::ports::PersistencePort;

        let sid = "s-cross-seam";
        let topic_key = format!("spectty/{sid}/diff");

        // The exact trigger-doc shape the MCP `spectty_diff` effect writes (session_id + hint
        // + nonce). The nonce is what makes a back-to-back same-hint trigger a distinct doc.
        let trigger_doc = |nonce: &str| {
            serde_json::json!({
                "session_id": sid,
                "hint": "edited src/lib.rs",
                "nonce": nonce,
            })
            .to_string()
        };

        // The app side: a PortPollReader + SpecBus over the SAME persistence the MCP effect
        // upserts into, plus the per-session pipeline the poll's emit closure runs.
        let adapter = Arc::new(InMemoryPersistenceAdapter::new());
        let port: Arc<dyn PersistencePort> = adapter.clone();
        let reader: Arc<dyn PollReader> = Arc::new(PortPollReader::new(port.clone()));
        let mut bus = SpecBus::new(reader, topic_key.clone());

        let pipeline: SharedPipeline = Arc::new(DiffPipeline::new(sid, "/ws"));
        // A diff that changes between the two triggers so each fired run actually emits
        // (the trigger doc only SIGNALS "go look"; the pipeline reads git itself).
        let git = Mutex::new(FakeGit::ok("diff --git a/f.rs b/f.rs\n+one\n"));
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();
        let mut runs = 0usize;

        // One poll-tick driver: on a detected change, run the pipeline once (this is exactly
        // what the production cooperative poll loop's emit closure does — minus spawn_blocking).
        let tick = |bus: &mut SpecBus,
                    git: &Mutex<FakeGit>,
                    runs: &mut usize,
                    emitted: &mut Vec<DiffUpdated>| {
            bus.poll(&mut |_change| {
                *runs += 1;
                let g = git.lock().unwrap();
                pipeline.run_once(&*g, &explainer, &mut |e| emitted.push(e));
            });
        };

        // No trigger doc yet → poll is a no-op (the pipeline does not fire).
        tick(&mut bus, &git, &mut runs, &mut emitted);
        assert_eq!(runs, 0, "no trigger doc → pipeline must not fire");

        // MCP `spectty_diff` upserts the trigger doc → the app poll detects it and fires.
        port.upsert(&topic_key, trigger_doc("100")).unwrap();
        tick(&mut bus, &git, &mut runs, &mut emitted);
        assert_eq!(
            runs, 1,
            "the trigger doc must fire the pipeline exactly once"
        );
        assert_eq!(emitted.len(), 1, "the fired pipeline emitted diff_updated");

        // A SECOND, DISTINCT-NONCE doc for the SAME hint (a rapid re-edit). Change the diff so
        // the pipeline's own hash-dedup does not swallow the second fire — we are asserting the
        // TRIGGER seam re-fires, which the nonce guarantees even for an identical hint.
        *git.lock().unwrap() = FakeGit::ok("diff --git a/f.rs b/f.rs\n+one\n+two\n");
        port.upsert(&topic_key, trigger_doc("200")).unwrap();
        tick(&mut bus, &git, &mut runs, &mut emitted);
        assert_eq!(
            runs, 2,
            "a distinct-nonce trigger doc must re-fire the pipeline (consume-once/nonce contract)"
        );
        assert_eq!(emitted.len(), 2, "the second fire emitted again");
    }

    // WU-8.7: cooperative and generic triggers feed the SAME pipeline and the in-flight
    // guard makes a double-fire harmless. Two runs over the same shared pipeline with the
    // same diff: the first emits, the second dedups — exactly one emit total, regardless of
    // which trigger fired it (both call `run_once` on the shared Arc).
    #[test]
    fn cooperative_and_generic_share_one_deduped_pipeline() {
        let p: SharedPipeline = Arc::new(pipeline());
        let git = FakeGit::ok("diff --git a/f.rs b/f.rs\n+x\n");
        let explainer = FakeExplainer::new();
        let mut emitted: Vec<DiffUpdated> = Vec::new();

        // "Cooperative" trigger.
        p.run_once(&git, &explainer, &mut |e| emitted.push(e));
        // "Generic" (FileWatch) trigger, same tree → deduped.
        p.run_once(&git, &explainer, &mut |e| emitted.push(e));

        assert_eq!(
            emitted.len(),
            1,
            "both triggers share one pipeline; the unchanged second is deduped"
        );
        assert_eq!(explainer.call_count(), 1);
    }
}
