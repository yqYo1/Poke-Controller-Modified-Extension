//! Single-global `auto_reload_config` file watcher for the global init files.
//!
//! The authoritative backend spec (§11.5.1) requires three dynamic
//! configuration load timings: startup, manual menu reload, and opt-in
//! automatic reload on file change (default off). Startup and manual reload
//! already exist; this module provides the missing automatic leg on top of
//! OS-native filesystem notifications (`notify` 7).
//!
//! Design notes, all derived from the existing contracts rather than new
//! semantics:
//! - **Single global watcher**: exactly one [`DynamicConfigWatcher`] is owned
//!   by [`crate::application_backend::ApplicationBackend`]. [`start`] is
//!   idempotent for the active config root, so enabling the flag twice (or
//!   racing a PATCH with startup) never spawns a second task.
//! - **Watched files**: the global `init.py` / `init.lua` selected by the
//!   Neovim-style single-selection rule (SPEC §11.5.2). The config *directory*
//!   is watched (non-recursive) and events are filtered by target basename,
//!   so editor atomic-saves (temporary file + rename) surface as
//!   create/rename events on the target name and are never missed by watching
//!   a possibly-replaced inode.
//! - **Change detection**: OS-native events arm a debounce; at fire time the
//!   current content hash is compared against the last acted-on hash. Pure
//!   metadata/duplicate events with identical content are skipped, while
//!   same-size rapid writes with different bytes still reload. No polling.
//!   Hashing runs on the blocking pool (`spawn_blocking`), never on the async
//!   executor.
//! - **Startup/arm race**: the pre-arm content snapshot is taken before the
//!   OS watch is armed and compared with a post-arm snapshot; a difference
//!   schedules exactly one debounced catch-up reload. Writes after the second
//!   snapshot stay covered by native events.
//! - **Debounce + single-flight** (frontend §11.5.7.3): bursts collapse into
//!   one reload. Reloads run inline in the single owned loop task, so a
//!   reload already in flight never overlaps with another — events arriving
//!   mid-reload buffer in the channel and collapse into exactly one
//!   coalesced follow-up.
//! - **No ownership cycle**: the watcher keeps only a [`Weak`] reload target.
//!   The backend registers `Arc::downgrade` of itself, so the watcher can
//!   never keep the backend alive. A dropped owner only drops change events
//!   with a diagnostic.
//! - **Worker-absent behavior** (frontend §11.5.7.3): the watcher stays armed
//!   when no dynamic worker exists; change events are dropped with a
//!   diagnostic and retried on the next change, manual reload, or restart.
//! - **Parse/error policy** (`DYNAMIC_CONFIGURATION.md`): a rejected
//!   generation (`loaded=false`) or transport failure keeps the prior
//!   generation and never stops the watcher. Rejected generations count as
//!   acted-on (the fingerprint advances); retryable transport/projection
//!   failures do not, so identical bytes retry on the next trigger without a
//!   busy loop.
//! - **Stop/lifecycle**: [`shutdown`] stops OS delivery first, then awaits
//!   the loop task (which includes any in-flight reload) with a bounded
//!   join; on timeout the task is aborted *and reaped*, so shutdown never
//!   returns while a task is still live. Dropping the watcher aborts the
//!   task as a safety net so a failed composition root can never leak a
//!   task holding the backend alive.
//!
//! [`start`]: DynamicConfigWatcher::start
//! [`shutdown`]: DynamicConfigWatcher::shutdown

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;

use async_trait::async_trait;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use sha2::{Digest as _, Sha256};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

/// Global init file names covered by the single watcher.
pub(crate) const WATCHED_INIT_FILES: [&str; 2] = ["init.py", "init.lua"];

/// Quiet period after the last observed native event before a reload fires.
const DEBOUNCE_DELAY: Duration = Duration::from_millis(300);
/// Bound for graceful task join during [`DynamicConfigWatcher::shutdown`].
/// On timeout the task is aborted and reaped, never detached.
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Outcome of one watcher-triggered reload, reported by the reload target.
///
/// Every variant keeps the watcher armed; the distinction controls whether
/// the observed content counts as acted-on (`last_applied` advances) and
/// which diagnostic is emitted. A rejected or failed reload keeps the prior
/// generation active per the transaction contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AutoReloadOutcome {
    /// The worker committed a new generation.
    Reloaded,
    /// The reload was attempted and the content was acted on, but the prior
    /// generation stays active because the worker evaluated and rejected the
    /// new source (`loaded=false`). The fingerprint advances so identical
    /// bytes are not retried in a busy loop; only a new content change,
    /// manual reload, or restart retries.
    KeptPriorGeneration,
    /// A retryable failure (transport error reaching the worker, or a
    /// post-load projection-commit failure). The prior generation stays
    /// active, but the fingerprint does NOT advance, so the same content is
    /// retried on the next debounced trigger instead of being suppressed.
    /// No busy loop is introduced: a retry only fires on the next native
    /// event or pending reload, never by spinning here.
    RetryableFailure,
    /// No dynamic worker exists; the change was dropped with a diagnostic.
    NoWorker,
}

/// Async boundary implemented by the backend: one debounced change batch
/// becomes one worker `Reload` plus settings adopt/projection commit.
#[async_trait]
pub(crate) trait AutoReloadTarget: Send + Sync {
    /// Performs one watcher-triggered reload and reports its outcome.
    async fn reload_dynamic_config(&self) -> AutoReloadOutcome;
}

/// Content identity of the selected watched init file: SHA-256 of the bytes, or
/// `None` when the file is missing or unreadable.
///
/// Hashing (rather than `exists`/`len`/`mtime`) is what makes same-size rapid
/// writes reliably detectable while pure metadata/duplicate OS events are
/// skipped without a reload.
type ContentsFingerprint = Option<[u8; 32]>;

fn file_content_hash(path: &Path) -> ContentsFingerprint {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).into())
}

fn content_fingerprint(config_root: &Path, watched_file: &str) -> ContentsFingerprint {
    file_content_hash(&config_root.join(watched_file))
}

/// Async wrapper that moves blocking file I/O + hashing off the async
/// executor via `spawn_blocking`. Fail-closed: a join failure yields `None`
/// (missing/unreadable), which the fingerprint comparison treats as a
/// distinct content state without ever logging file bytes.
async fn content_fingerprint_async(
    config_root: PathBuf,
    watched_file: String,
) -> ContentsFingerprint {
    tokio::task::spawn_blocking(move || content_fingerprint(&config_root, &watched_file))
        .await
        .unwrap_or(None)
}

/// Returns `true` when a native event may affect the selected watched init file.
///
/// The config directory is watched, so every event path is filtered by the
/// selected target basename; this is what makes atomic-save (temporary file +
/// rename onto the selected target) work. Only access-type events are
/// excluded — unknown/imprecise kinds fail open when the path matches so a
/// change is never silently missed; the content-hash check at fire time
/// suppresses any event that left the bytes untouched.
pub(crate) fn event_targets_init(event: &Event, watched_file: &str) -> bool {
    if !WATCHED_INIT_FILES.contains(&watched_file) || matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| {
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name == watched_file)
    })
}

struct WatcherTask {
    handle: JoinHandle<()>,
    stop: watch::Sender<bool>,
    /// Keeps the OS registration alive. Dropping it stops event delivery,
    /// which is why shutdown drops it before reaping the loop task.
    native: RecommendedWatcher,
}

struct WatcherInner {
    task: Option<WatcherTask>,
    root: Option<PathBuf>,
    watched_file: Option<String>,
}

/// Single-global automatic-reload watcher. Owned by the application backend;
/// at most one loop task exists per handle, and the reload target is held
/// weakly so the watcher can never own the backend.
pub(crate) struct DynamicConfigWatcher {
    /// Serializes start/stop/shutdown so replacing a root always reaps the
    /// previous task before a new task is armed.
    lifecycle: tokio::sync::Mutex<()>,
    inner: Mutex<WatcherInner>,
    timings: WatcherTimings,
}

/// Debounce cadence. Production uses [`WatcherTimings::default`]; tests
/// inject short deterministic cadences.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WatcherTimings {
    debounce: Duration,
}

impl Default for WatcherTimings {
    fn default() -> Self {
        Self {
            debounce: DEBOUNCE_DELAY,
        }
    }
}

impl DynamicConfigWatcher {
    /// Creates an idle watcher. No filesystem observation happens until
    /// [`start`](Self::start).
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::with_timings(WatcherTimings::default())
    }

    /// Creates an idle watcher with an explicit cadence (test seam).
    #[must_use]
    pub(crate) fn with_timings(timings: WatcherTimings) -> Self {
        Self {
            lifecycle: tokio::sync::Mutex::new(()),
            inner: Mutex::new(WatcherInner {
                task: None,
                root: None,
                watched_file: None,
            }),
            timings,
        }
    }

    /// Returns `true` while a watcher task is active.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_watching(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .task
            .as_ref()
            .is_some_and(|task| !task.handle.is_finished())
    }

    /// Returns the config root currently watched, if any.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn watched_root(&self) -> Option<PathBuf> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .root
            .clone()
    }

    /// Starts watching `config_root` and the selected global init file, or
    /// no-ops when that exact root/file pair is already watched.
    ///
    /// Returns `true` when a new OS-native watch was armed. A different root
    /// or selected init file replaces the previous task only after that task
    /// has been cancelled and reaped, preserving the single-global-watcher
    /// invariant. The reload target is held weakly: the watcher never keeps
    /// the backend alive.
    ///
    /// Returns `false` (with an error diagnostic) when the OS watch cannot
    /// be established, so PATCH-time reconciliation can retry later.
    pub(crate) async fn start(
        &self,
        config_root: PathBuf,
        watched_file: &str,
        target: Weak<dyn AutoReloadTarget>,
    ) -> bool {
        if !WATCHED_INIT_FILES.contains(&watched_file) {
            tracing::error!(
                diagnostic_id = "DYNAMIC_WATCHER_INVALID_TARGET",
                path = watched_file,
                "automatic config reload received an unsupported init file"
            );
            return false;
        }
        let _lifecycle = self.lifecycle.lock().await;
        let previous = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let same_watch = inner
                .task
                .as_ref()
                .is_some_and(|task| !task.handle.is_finished())
                && inner.root.as_ref() == Some(&config_root)
                && inner.watched_file.as_deref() == Some(watched_file);
            if same_watch {
                return false;
            }
            inner.root = None;
            inner.watched_file = None;
            inner.task.take()
        };
        if let Some(previous) = previous {
            reap_task(previous, false).await;
        }

        // Close the arm/baseline race: snapshot before arming the OS watch,
        // arm, then snapshot again. When the two differ, a write landed while
        // arming and native delivery may already have coalesced or missed it,
        // so one debounced initial reload is scheduled. The baseline stored
        // in the loop is the pre-arm snapshot, so the post-arm content (or any
        // later write) differs from it and reliably fires. Writes after the
        // second snapshot remain covered by native events as usual.
        let pre_watch =
            content_fingerprint_async(config_root.clone(), watched_file.to_owned()).await;
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let mut native = match RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                let _ignored = event_sender.send(result);
            },
            notify::Config::default(),
        ) {
            Ok(native) => native,
            Err(error) => {
                tracing::error!(
                    diagnostic_id = "DYNAMIC_WATCHER_OS_WATCH_FAILED",
                    error = %error,
                    path = %config_root.display(),
                    "automatic config reload could not establish an OS-native file watch"
                );
                return false;
            }
        };
        if let Err(error) = native.watch(&config_root, RecursiveMode::NonRecursive) {
            tracing::error!(
                diagnostic_id = "DYNAMIC_WATCHER_OS_WATCH_FAILED",
                error = %error,
                path = %config_root.display(),
                "automatic config reload could not watch the config root"
            );
            return false;
        }
        let post_watch =
            content_fingerprint_async(config_root.clone(), watched_file.to_owned()).await;
        let initial_pending = pre_watch != post_watch;
        let baseline = pre_watch;
        let (stop_sender, stop_receiver) = watch::channel(false);
        let timings = self.timings;
        let handle = tokio::spawn(watch_loop(
            config_root.clone(),
            watched_file.to_owned(),
            stop_receiver,
            event_receiver,
            timings,
            baseline,
            initial_pending,
            target,
        ));
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.task = Some(WatcherTask {
            handle,
            stop: stop_sender,
            native,
        });
        inner.root = Some(config_root);
        inner.watched_file = Some(watched_file.to_owned());
        true
    }

    /// Stops delivery of further change events and reaps the task. In-flight
    /// reloads are cancelled; use [`shutdown`](Self::shutdown) at
    /// composition-root teardown to let an in-flight reload finish first.
    pub(crate) async fn stop(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        let task = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.root = None;
            inner.watched_file = None;
            inner.task.take()
        };
        if let Some(task) = task {
            reap_task(task, false).await;
        }
    }

    /// Stops the watcher and awaits task exit, including an in-flight
    /// reload. Idempotent. Bounded: on timeout the task is aborted *and
    /// reaped*, so no live task remains when this returns.
    pub(crate) async fn shutdown(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        let task = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.root = None;
            inner.watched_file = None;
            inner.task.take()
        };
        if let Some(task) = task {
            reap_task(task, true).await;
        }
    }

    /// Requests the loop to stop after the current inline reload returns.
    /// This is used when a successful dynamic reload turns the option off from
    /// inside the watcher task itself, where awaiting `shutdown` would await
    /// the current task.
    pub(crate) fn request_stop(&self) {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(task) = inner.task.as_ref() {
            let _ignored = task.stop.send(true);
        }
    }

    fn stop_locked(inner: &mut WatcherInner) {
        if let Some(task) = inner.task.take() {
            let _ignored = task.stop.send(true);
            drop(task.native);
            task.handle.abort();
        }
        inner.root = None;
        inner.watched_file = None;
    }
}

async fn reap_task(task: WatcherTask, graceful: bool) {
    let _ignored = task.stop.send(true);
    // Stop OS delivery before reaping so no new trigger can arm while the
    // loop drains or cancellation is delivered.
    drop(task.native);
    let mut handle = task.handle;
    if !graceful {
        handle.abort();
        let _ignored = (&mut handle).await;
        return;
    }
    let finished = tokio::select! {
        result = &mut handle => {
            let _ignored = result;
            true
        }
        () = tokio::time::sleep(SHUTDOWN_JOIN_TIMEOUT) => false,
    };
    if !finished {
        handle.abort();
        let _ignored = (&mut handle).await;
        tracing::warn!(
            diagnostic_id = "DYNAMIC_WATCHER_SHUTDOWN_TIMEOUT",
            "dynamic config watcher task did not exit in time; aborted and reaped"
        );
    }
}

impl Default for DynamicConfigWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DynamicConfigWatcher {
    fn drop(&mut self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Safety net: a failed composition root drops the backend without
        // reaching shutdown. Aborting here guarantees no leaked task can keep
        // the backend (via the reload target) alive — and the target is only
        // held weakly regardless.
        Self::stop_locked(&mut inner);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "single-global watcher loop takes its full spawn context once"
)]
async fn watch_loop(
    config_root: PathBuf,
    watched_file: String,
    mut stop: watch::Receiver<bool>,
    mut events: mpsc::UnboundedReceiver<notify::Result<Event>>,
    timings: WatcherTimings,
    mut last_applied: ContentsFingerprint,
    initial_pending: bool,
    target: Weak<dyn AutoReloadTarget>,
) {
    // A write that landed between the pre-arm snapshot and the post-arm
    // snapshot schedules exactly one debounced catch-up reload. Later writes
    // stay covered by native events, which simply re-arm this deadline.
    let mut debounce_deadline: Option<Instant> =
        initial_pending.then(|| Instant::now() + timings.debounce);
    loop {
        tokio::select! {
            result = stop.changed() => {
                if result.is_err() || *stop.borrow() {
                    break;
                }
            }
            received = events.recv() => {
                match received {
                    None => break,
                    Some(Err(error)) => {
                        tracing::warn!(
                            diagnostic_id = "DYNAMIC_WATCHER_EVENT_ERROR",
                            error = %error,
                            "dynamic config file watch reported an error; watcher stays armed"
                        );
                    }
                    Some(Ok(event)) => {
                        if *stop.borrow() {
                            break;
                        }
                        if event_targets_init(&event, &watched_file) {
                            debounce_deadline = Some(Instant::now() + timings.debounce);
                        }
                    }
                }
            }
            () = debounce_wait(debounce_deadline) => {
                debounce_deadline = None;
                if *stop.borrow() {
                    break;
                }
                fire_reload(&config_root, &watched_file, &target, &mut last_applied).await;
                if *stop.borrow() {
                    break;
                }
            }
        }
    }
}

async fn debounce_wait(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Fires one debounced reload when the selected on-disk content actually moved.
///
/// Runs inline in the single owned loop task, which is the single-flight
/// mechanism: no second reload can start while this one is awaited, and
/// events arriving meanwhile buffer in the channel and collapse into one
/// coalesced follow-up after this returns.
///
/// Hashing runs on the blocking pool. `last_applied` advances only for
/// acted-on outcomes (`Reloaded`, `KeptPriorGeneration` for a rejected
/// generation, `NoWorker` for a dropped change); a `RetryableFailure`
/// (transport/projection) leaves it untouched so the same bytes retry on the
/// next trigger instead of being suppressed.
async fn fire_reload(
    config_root: &Path,
    watched_file: &str,
    target: &Weak<dyn AutoReloadTarget>,
    last_applied: &mut ContentsFingerprint,
) {
    let current =
        content_fingerprint_async(config_root.to_path_buf(), watched_file.to_owned()).await;
    if current == *last_applied {
        // Native event without a content change (metadata/duplicate
        // delivery): stay armed, reload nothing.
        return;
    }
    let Some(target) = target.upgrade() else {
        tracing::debug!(
            diagnostic_id = "DYNAMIC_CONFIG_AUTO_RELOAD_NO_OWNER",
            "dynamic configuration file changed but the reload owner is gone; watcher stays armed"
        );
        return;
    };
    let outcome = target.reload_dynamic_config().await;
    // Record what was acted on (not a post-reload re-read): a change that
    // lands mid-reload must still differ at the follow-up check, otherwise
    // the coalesced follow-up would be skipped and the change lost.
    // Retryable transport/projection failures deliberately do not advance,
    // so the unchanged bytes remain retryable on the next trigger.
    match outcome {
        AutoReloadOutcome::Reloaded => {
            *last_applied = current;
            tracing::info!(
                diagnostic_id = "DYNAMIC_CONFIG_AUTO_RELOADED",
                "dynamic configuration file change was reloaded"
            );
        }
        AutoReloadOutcome::KeptPriorGeneration => {
            *last_applied = current;
            tracing::warn!(
                diagnostic_id = "DYNAMIC_CONFIG_AUTO_RELOAD_REJECTED",
                "automatic reload was rejected; keeping the prior generation"
            );
        }
        AutoReloadOutcome::RetryableFailure => {
            tracing::warn!(
                diagnostic_id = "DYNAMIC_CONFIG_AUTO_RELOAD_RETRYABLE",
                "automatic reload hit a retryable failure; keeping the prior generation without advancing"
            );
        }
        AutoReloadOutcome::NoWorker => {
            *last_applied = current;
            tracing::debug!(
                diagnostic_id = "DYNAMIC_CONFIG_AUTO_RELOAD_NO_WORKER",
                "dynamic configuration file changed but no worker is running; watcher stays armed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind};
    use sha2::{Digest as _, Sha256};
    use tempfile::TempDir;

    use super::*;

    const TEST_DEBOUNCE: Duration = Duration::from_millis(40);
    const TEST_TIMINGS: WatcherTimings = WatcherTimings {
        debounce: TEST_DEBOUNCE,
    };
    /// Generous settle bound: a multiple of the test debounce, not a timing
    /// assumption about OS delivery.
    const SETTLE: Duration = Duration::from_millis(400);

    #[derive(Debug)]
    struct FakeTarget {
        calls: AtomicUsize,
        live: AtomicUsize,
        max_live: AtomicUsize,
        release: AtomicBool,
        outcome: Mutex<AutoReloadOutcome>,
    }

    impl FakeTarget {
        fn new(outcome: AutoReloadOutcome) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                live: AtomicUsize::new(0),
                max_live: AtomicUsize::new(0),
                release: AtomicBool::new(true),
                outcome: Mutex::new(outcome),
            }
        }

        fn hold(&self) {
            self.release.store(false, Ordering::Release);
        }

        fn release(&self) {
            self.release.store(true, Ordering::Release);
        }

        fn set_outcome(&self, outcome: AutoReloadOutcome) {
            *self
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = outcome;
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }

        fn max_live(&self) -> usize {
            self.max_live.load(Ordering::Acquire)
        }
    }

    #[async_trait]
    impl AutoReloadTarget for FakeTarget {
        async fn reload_dynamic_config(&self) -> AutoReloadOutcome {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let live = self.live.fetch_add(1, Ordering::AcqRel) + 1;
            self.max_live.fetch_max(live, Ordering::AcqRel);
            while !self.release.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            self.live.fetch_sub(1, Ordering::AcqRel);
            *self
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    fn test_event(kind: EventKind, names: &[&str], temporary: &TempDir) -> Event {
        Event {
            kind,
            paths: names
                .iter()
                .map(|name| temporary.path().join(name))
                .collect(),
            attrs: notify::event::EventAttributes::default(),
        }
    }

    fn write_init(temporary: &TempDir, name: &str, body: &str) {
        std::fs::write(temporary.path().join(name), body).expect("test init file must be writable");
    }

    fn weak_target(target: &Arc<FakeTarget>) -> Weak<dyn AutoReloadTarget> {
        let wide: Arc<dyn AutoReloadTarget> = target.clone();
        Arc::downgrade(&wide)
    }

    async fn wait_for_calls(target: &FakeTarget, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while target.calls() < expected {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {expected} reloads (saw {})",
                target.calls()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn native_event_filter_matches_selected_target_basename() {
        let temporary = TempDir::new().unwrap();
        assert!(event_targets_init(
            &test_event(
                EventKind::Create(CreateKind::File),
                &["init.py"],
                &temporary
            ),
            "init.py"
        ));
        assert!(!event_targets_init(
            &test_event(
                EventKind::Remove(RemoveKind::File),
                &["init.lua"],
                &temporary
            ),
            "init.py"
        ));
        assert!(event_targets_init(
            &test_event(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                &["init.py"],
                &temporary
            ),
            "init.py"
        ));
        // Atomic-save rename onto the selected target name.
        assert!(event_targets_init(
            &test_event(
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::To)),
                &["init.py"],
                &temporary
            ),
            "init.py"
        ));
        // Unrelated files never trigger, whatever the kind.
        assert!(!event_targets_init(
            &test_event(
                EventKind::Create(CreateKind::File),
                &["settings.toml"],
                &temporary
            ),
            "init.py"
        ));
        assert!(!event_targets_init(
            &test_event(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                &["init.py.swp"],
                &temporary
            ),
            "init.py"
        ));
        // Access (atime/read) events never trigger.
        assert!(!event_targets_init(
            &test_event(
                EventKind::Access(notify::event::AccessKind::Read),
                &["init.py"],
                &temporary
            ),
            "init.py"
        ));
        // The same event is accepted when Lua is the selected language.
        assert!(event_targets_init(
            &test_event(
                EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
                &["init.lua"],
                &temporary
            ),
            "init.lua"
        ));
    }

    #[test]
    fn content_hash_detects_same_length_writes_and_missing_files() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("init.py");
        assert!(file_content_hash(&path).is_none());
        write_init(&temporary, "init.py", "abcd");
        let created = file_content_hash(&path).expect("hash must exist");
        write_init(&temporary, "init.py", "abce");
        let same_length = file_content_hash(&path).expect("hash must exist");
        assert_ne!(created, same_length, "same-size writes must differ");
        write_init(&temporary, "init.py", "abce");
        assert_eq!(
            same_length,
            file_content_hash(&path).expect("identical bytes hash identically")
        );
        std::fs::remove_file(&path).unwrap();
        assert!(file_content_hash(&path).is_none());
    }

    #[tokio::test]
    async fn debounce_coalesces_rapid_changes_into_one_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        for version in 2..=6 {
            write_init(&temporary, "init.py", &format!("v = {version}\n"));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        wait_for_calls(&target, 1).await;
        // A coalesced burst must settle: no second reload follows.
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 1);
        watcher.shutdown().await;
        assert!(!watcher.is_watching());
    }

    #[tokio::test]
    async fn same_size_rapid_writes_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "aaaa");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        // Same byte length, different bytes: mtime/len polling would miss
        // this; native events plus content hashing must not.
        write_init(&temporary, "init.py", "bbbb");
        wait_for_calls(&target, 1).await;
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn identical_rewrite_does_not_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 2 with a longer body\n");
        wait_for_calls(&target, 1).await;
        // Rewriting byte-identical content emits a native event but must not
        // produce a reload.
        write_init(&temporary, "init.py", "v = 2 with a longer body\n");
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 1);
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn atomic_save_via_rename_triggers_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        let pending = temporary.path().join("init.py.tmp-atomic");
        std::fs::write(&pending, "v = 2 atomic with a longer body\n").unwrap();
        std::fs::rename(&pending, temporary.path().join("init.py")).unwrap();
        wait_for_calls(&target, 1).await;
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn single_flight_serializes_overlapping_reloads() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.lua", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        target.hold();
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.lua",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.lua", "v = 2\n");
        wait_for_calls(&target, 1).await;
        // Change again while the first reload is held: arms one follow-up.
        write_init(&temporary, "init.lua", "v = 3 with a longer body\n");
        write_init(&temporary, "init.lua", "v = 4 with an even longer body\n");
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 1, "overlapping reload must not start");
        target.release();
        wait_for_calls(&target, 2).await;
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 2, "only one coalesced follow-up may fire");
        assert_eq!(target.max_live(), 1, "reloads must never overlap");
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn rejected_generation_keeps_the_watcher_armed() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::KeptPriorGeneration));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 2 broken (((\n");
        wait_for_calls(&target, 1).await;
        target.set_outcome(AutoReloadOutcome::Reloaded);
        write_init(&temporary, "init.py", "v = 3 fixed with a longer body\n");
        wait_for_calls(&target, 2).await;
        assert!(watcher.is_watching());
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn missing_worker_keeps_the_watcher_armed() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::NoWorker));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 2\n");
        wait_for_calls(&target, 1).await;
        // The watcher survives worker absence and reloads once one exists.
        target.set_outcome(AutoReloadOutcome::Reloaded);
        write_init(&temporary, "init.py", "v = 3 with a much longer body\n");
        wait_for_calls(&target, 2).await;
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn dropped_owner_drops_events_without_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let weak = weak_target(&target);
        drop(target);
        assert!(weak.upgrade().is_none());
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(temporary.path().to_path_buf(), "init.py", weak)
                .await
        );
        write_init(&temporary, "init.py", "v = 2 with a longer body\n");
        // No owner exists to count calls; assert the watcher simply stays
        // armed and shuts down cleanly instead of panicking.
        tokio::time::sleep(SETTLE).await;
        assert!(watcher.is_watching());
        watcher.shutdown().await;
        assert!(!watcher.is_watching());
    }

    #[tokio::test]
    async fn shutdown_awaits_in_flight_reload() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        target.hold();
        let watcher = Arc::new(DynamicConfigWatcher::with_timings(TEST_TIMINGS));
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 2 with a longer body\n");
        wait_for_calls(&target, 1).await;
        // Shutdown must not return while the reload is held: delivery stops
        // first, then the in-flight reload is awaited.
        let for_shutdown = Arc::clone(&watcher);
        let shutdown = tokio::spawn(async move { for_shutdown.shutdown().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !shutdown.is_finished(),
            "shutdown must await the held reload"
        );
        target.release();
        tokio::time::timeout(Duration::from_secs(10), shutdown)
            .await
            .expect("shutdown must complete after release")
            .expect("shutdown task must not panic");
        assert_eq!(target.calls(), 1);
        assert!(!watcher.is_watching());
    }

    #[tokio::test]
    async fn stop_prevents_further_reloads() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        watcher.stop().await;
        assert!(!watcher.is_watching());
        write_init(&temporary, "init.py", "v = 2\n");
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 0);
        // Re-enabling after disable rearms exactly one watcher.
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.py",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 3 with a much longer body\n");
        wait_for_calls(&target, 1).await;
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn same_root_start_is_idempotent_and_root_change_restarts() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(first.path().to_path_buf(), "init.py", weak_target(&target))
                .await
        );
        assert!(
            !watcher
                .start(first.path().to_path_buf(), "init.py", weak_target(&target))
                .await
        );
        assert_eq!(watcher.watched_root(), Some(first.path().to_path_buf()));
        assert!(
            watcher
                .start(second.path().to_path_buf(), "init.py", weak_target(&target))
                .await
        );
        assert_eq!(watcher.watched_root(), Some(second.path().to_path_buf()));
        // The old root no longer triggers; the new one does.
        write_init(&first, "init.py", "stale root change\n");
        write_init(&second, "init.py", "new root change\n");
        wait_for_calls(&target, 1).await;
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 1);
        watcher.shutdown().await;
        assert_eq!(watcher.watched_root(), None);
    }

    #[tokio::test]
    async fn selected_init_name_only_triggers_reload() {
        let temporary = TempDir::new().unwrap();
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let watcher = DynamicConfigWatcher::with_timings(TEST_TIMINGS);
        assert!(
            watcher
                .start(
                    temporary.path().to_path_buf(),
                    "init.lua",
                    weak_target(&target)
                )
                .await
        );
        write_init(&temporary, "init.py", "v = 1\n");
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 0, "non-selected init file must be ignored");
        write_init(&temporary, "init.lua", "-- v = 1\n");
        wait_for_calls(&target, 1).await;
        // Unrelated files in the config root must not trigger reloads.
        write_init(&temporary, "settings.toml", "x = 1\n");
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 1);
        watcher.shutdown().await;
    }

    #[tokio::test]
    async fn retryable_failure_leaves_fingerprint_retryable_while_rejection_advances() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let root = temporary.path().to_path_buf();

        // Rejected generation counts as acted-on: identical bytes do not retry.
        let rejected = Arc::new(FakeTarget::new(AutoReloadOutcome::KeptPriorGeneration));
        let mut last_applied = content_fingerprint(&root, "init.py");
        write_init(&temporary, "init.py", "v = 2 rejected\n");
        fire_reload(&root, "init.py", &weak_target(&rejected), &mut last_applied).await;
        assert_eq!(rejected.calls(), 1);
        let after_reject = last_applied;
        assert_eq!(after_reject, content_fingerprint(&root, "init.py"));
        // Second trigger with unchanged bytes: skipped, no second attempt.
        fire_reload(&root, "init.py", &weak_target(&rejected), &mut last_applied).await;
        assert_eq!(rejected.calls(), 1, "rejected bytes must not busy-retry");
        assert_eq!(last_applied, after_reject);

        // Retryable failure does not advance: identical bytes retry on the
        // next trigger instead of being suppressed.
        let retryable = Arc::new(FakeTarget::new(AutoReloadOutcome::RetryableFailure));
        // Reset to a known baseline, then change.
        write_init(&temporary, "init.py", "v = 1\n");
        let mut last_applied = content_fingerprint(&root, "init.py");
        write_init(&temporary, "init.py", "v = 2 flaky\n");
        let baseline = last_applied;
        fire_reload(
            &root,
            "init.py",
            &weak_target(&retryable),
            &mut last_applied,
        )
        .await;
        assert_eq!(retryable.calls(), 1);
        assert_eq!(
            last_applied, baseline,
            "retryable failure must not advance the fingerprint"
        );
        // Unchanged bytes trigger a second attempt (retry), with no busy
        // loop: the retry only happens when the caller fires again.
        fire_reload(
            &root,
            "init.py",
            &weak_target(&retryable),
            &mut last_applied,
        )
        .await;
        assert_eq!(
            retryable.calls(),
            2,
            "identical bytes must retry after failure"
        );
        assert_eq!(last_applied, baseline);
    }

    #[tokio::test]
    async fn arm_race_catch_up_fires_without_native_event() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 2 post-arm\n");
        // Baseline is the pre-arm snapshot; on-disk content already moved.
        let stale: ContentsFingerprint = Some(Sha256::digest(b"v = 1\n").into());
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let (_stop_sender, stop_receiver) = watch::channel(false);
        let (_event_sender, event_receiver) = mpsc::unbounded_channel();
        let loop_task = tokio::spawn(watch_loop(
            temporary.path().to_path_buf(),
            "init.py".to_owned(),
            stop_receiver,
            event_receiver,
            TEST_TIMINGS,
            stale,
            true,
            weak_target(&target),
        ));
        wait_for_calls(&target, 1).await;
        loop_task.abort();
        let _ignored = loop_task.await;
    }

    #[tokio::test]
    async fn clean_arm_does_not_fire_without_native_event() {
        let temporary = TempDir::new().unwrap();
        write_init(&temporary, "init.py", "v = 1\n");
        let baseline = content_fingerprint(temporary.path(), "init.py");
        let target = Arc::new(FakeTarget::new(AutoReloadOutcome::Reloaded));
        let (_stop_sender, stop_receiver) = watch::channel(false);
        let (_event_sender, event_receiver) = mpsc::unbounded_channel();
        let loop_task = tokio::spawn(watch_loop(
            temporary.path().to_path_buf(),
            "init.py".to_owned(),
            stop_receiver,
            event_receiver,
            TEST_TIMINGS,
            baseline,
            false,
            weak_target(&target),
        ));
        tokio::time::sleep(SETTLE).await;
        assert_eq!(target.calls(), 0, "clean arm must not spuriously reload");
        loop_task.abort();
        let _ignored = loop_task.await;
    }
}
