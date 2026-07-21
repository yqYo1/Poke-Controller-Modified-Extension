use std::cmp::Ordering;
use std::collections::HashSet;
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering as AtomicOrdering};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use crate::event::HandlerId;

const COMMAND_CAPACITY: usize = 64;

/// Global callback execution settings from the canonical settings registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackSettings {
    pub soft_timeout_ms: u64,
    pub soft_timeout_grace_ms: u64,
    pub hard_timeout_ms: u64,
    pub max_concurrency: usize,
    pub queue_capacity: usize,
}

impl Default for CallbackSettings {
    fn default() -> Self {
        Self {
            soft_timeout_ms: 2_000,
            soft_timeout_grace_ms: 1_000,
            hard_timeout_ms: 5_000,
            max_concurrency: 8,
            queue_capacity: 1_024,
        }
    }
}

impl CallbackSettings {
    /// Validates the global scheduler settings.
    ///
    /// # Errors
    ///
    /// Rejects zero capacities and an enabled hard deadline earlier than the
    /// enabled soft deadline plus its grace period.
    pub fn validate(self) -> Result<(), CallbackError> {
        if self.max_concurrency == 0 {
            return Err(CallbackError::configuration(
                "callback max concurrency must be positive",
            ));
        }
        if self.queue_capacity == 0 {
            return Err(CallbackError::configuration(
                "callback queue capacity must be positive",
            ));
        }
        validate_timeout_combination(
            self.soft_timeout_ms,
            self.soft_timeout_grace_ms,
            self.hard_timeout_ms,
        )
    }
}

/// Per-registration optional timeout overrides. `None` inherits the global
/// value and `Some(0)` disables the corresponding stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CallbackLimits {
    pub soft_timeout_ms: Option<u64>,
    pub soft_timeout_grace_ms: Option<u64>,
    pub hard_timeout_ms: Option<u64>,
}

impl CallbackLimits {
    /// Resolves this registration against one immutable call-start snapshot.
    ///
    /// # Errors
    ///
    /// Rejects an invalid effective soft/grace/hard combination.
    pub fn resolve(self, global: CallbackSettings) -> Result<EffectiveLimits, CallbackError> {
        let effective = EffectiveLimits {
            soft_timeout_ms: self.soft_timeout_ms.unwrap_or(global.soft_timeout_ms),
            soft_timeout_grace_ms: self
                .soft_timeout_grace_ms
                .unwrap_or(global.soft_timeout_grace_ms),
            hard_timeout_ms: self.hard_timeout_ms.unwrap_or(global.hard_timeout_ms),
        };
        validate_timeout_combination(
            effective.soft_timeout_ms,
            effective.soft_timeout_grace_ms,
            effective.hard_timeout_ms,
        )?;
        Ok(effective)
    }
}

fn validate_timeout_combination(
    soft_timeout_ms: u64,
    soft_timeout_grace_ms: u64,
    hard_timeout_ms: u64,
) -> Result<(), CallbackError> {
    if soft_timeout_ms > 0
        && hard_timeout_ms > 0
        && hard_timeout_ms < soft_timeout_ms.saturating_add(soft_timeout_grace_ms)
    {
        return Err(CallbackError::configuration(
            "callback hard timeout must be at least soft timeout plus grace",
        ));
    }
    Ok(())
}

/// Fully resolved timeout values captured when a callback actually starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveLimits {
    pub soft_timeout_ms: u64,
    pub soft_timeout_grace_ms: u64,
    pub hard_timeout_ms: u64,
}

/// Timeout state visible to a language-specific checkpoint implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TimeoutStage {
    Running = 0,
    Soft = 1,
    Hard = 2,
}

/// Monotonic timeout signal shared with one actual callback execution.
#[derive(Debug, Default)]
pub struct DeadlineSignal(AtomicU8);

impl DeadlineSignal {
    #[must_use]
    pub fn stage(&self) -> TimeoutStage {
        match self.0.load(AtomicOrdering::Acquire) {
            0 => TimeoutStage::Running,
            1 => TimeoutStage::Soft,
            _ => TimeoutStage::Hard,
        }
    }

    fn enter_soft(&self) {
        let _result = self.0.compare_exchange(
            TimeoutStage::Running as u8,
            TimeoutStage::Soft as u8,
            AtomicOrdering::AcqRel,
            AtomicOrdering::Acquire,
        );
    }

    fn enter_hard(&self) {
        self.0
            .store(TimeoutStage::Hard as u8, AtomicOrdering::Release);
    }
}

/// Callback input supplied by the event and command dispatchers.
#[derive(Clone, Debug)]
pub struct InvocationContext {
    pub handler_id: HandlerId,
    pub event: Option<String>,
    pub arguments: Vec<Value>,
    pub limits: EffectiveLimits,
    pub started_at: Instant,
    pub deadline: Arc<DeadlineSignal>,
}

/// Closed callback return representation shared by both languages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackReturn {
    None,
    Boolean(bool),
    Value(Value),
}

/// Stable callback failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackErrorKind {
    User,
    SoftTimeout,
    HardTimeout,
    Configuration,
    Disconnected,
    Internal,
}

/// Secret-safe callback failure.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
#[error("{message}")]
pub struct CallbackError {
    pub kind: CallbackErrorKind,
    pub message: String,
}

impl CallbackError {
    #[must_use]
    pub fn user(message: impl Into<String>) -> Self {
        Self {
            kind: CallbackErrorKind::User,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn soft_timeout(message: impl Into<String>) -> Self {
        Self {
            kind: CallbackErrorKind::SoftTimeout,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn hard_timeout(message: impl Into<String>) -> Self {
        Self {
            kind: CallbackErrorKind::HardTimeout,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn configuration(message: impl Into<String>) -> Self {
        Self {
            kind: CallbackErrorKind::Configuration,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: CallbackErrorKind::Internal,
            message: message.into(),
        }
    }
}

/// Language-specific callable invoked by the shared executor.
#[async_trait]
pub trait Callback: Send + Sync {
    async fn invoke(&self, context: InvocationContext) -> Result<CallbackReturn, CallbackError>;
}

/// Observable logical result. Logical completion can precede the actual return;
/// the scheduler deliberately retains the lane and slot until actual return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackOutcome {
    Returned(CallbackReturn),
    Failed(CallbackError),
    TimedOut(TimeoutStage),
    Evicted,
    /// The caller requested immediate fallback while this handler's actual
    /// execution lane was still occupied by an earlier invocation.
    LaneBusy,
}

/// Diagnostic severity emitted by the scheduler and event bus.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

/// Structured, secret-safe dynamic-runtime diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
    pub handler_id: Option<HandlerId>,
    pub event: Option<String>,
}

pub trait DiagnosticSink: Send + Sync {
    fn record(&self, diagnostic: Diagnostic);
}

#[derive(Debug, Default)]
pub struct NoopDiagnosticSink;

impl DiagnosticSink for NoopDiagnosticSink {
    fn record(&self, _diagnostic: Diagnostic) {}
}

/// One unstarted callback invocation.
pub struct Invocation {
    pub handler_id: HandlerId,
    pub priority: i32,
    pub event_sequence: u64,
    pub registration_order: u64,
    pub event: Option<String>,
    pub arguments: Vec<Value>,
    pub limits: CallbackLimits,
    pub callback: Arc<dyn Callback>,
    pub on_start: Option<Arc<dyn Fn() + Send + Sync>>,
    pub reject_if_lane_busy: bool,
    pub on_late_return: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Invocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Invocation")
            .field("handler_id", &self.handler_id)
            .field("priority", &self.priority)
            .field("event_sequence", &self.event_sequence)
            .field("registration_order", &self.registration_order)
            .field("event", &self.event)
            .field("argument_count", &self.arguments.len())
            .finish_non_exhaustive()
    }
}

/// Receiver for one invocation's logical completion.
pub struct InvocationHandle {
    receiver: oneshot::Receiver<CallbackOutcome>,
}

impl InvocationHandle {
    /// Waits until the callback returns, fails, is evicted, or reaches logical
    /// timeout. It does not wait for a late actual return after timeout.
    #[must_use]
    pub async fn outcome(self) -> CallbackOutcome {
        self.receiver.await.unwrap_or_else(|_| {
            CallbackOutcome::Failed(CallbackError::internal(
                "callback scheduler stopped before logical completion",
            ))
        })
    }
}

enum SchedulerCommand {
    Submit {
        invocations: Vec<Invocation>,
        response: oneshot::Sender<Vec<InvocationHandle>>,
    },
    Update {
        settings: CallbackSettings,
        response: oneshot::Sender<Result<(), CallbackError>>,
    },
}

struct QueuedInvocation {
    invocation: Invocation,
    insertion_order: u64,
    logical: oneshot::Sender<CallbackOutcome>,
}

struct ActualCompletion {
    handler_id: HandlerId,
    logical: Option<LogicalCompletion>,
    on_late_return: Option<Arc<dyn Fn() + Send + Sync>>,
}

struct LogicalCompletion {
    sender: oneshot::Sender<CallbackOutcome>,
    outcome: CallbackOutcome,
}

/// Cloneable handle to the one global bounded callback scheduler.
#[derive(Clone)]
pub struct CallbackExecutor {
    commands: mpsc::Sender<SchedulerCommand>,
}

impl std::fmt::Debug for CallbackExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CallbackExecutor")
            .finish_non_exhaustive()
    }
}

impl CallbackExecutor {
    /// Starts the global scheduler on the current Tokio runtime.
    ///
    /// # Panics
    ///
    /// Panics when called outside a Tokio runtime. Dynamic workers create the
    /// executor after their runtime is active.
    #[must_use]
    pub fn new(settings: CallbackSettings, diagnostics: Arc<dyn DiagnosticSink>) -> Self {
        settings
            .validate()
            .expect("callback executor requires validated initial settings");
        let (command_sender, command_receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (completion_sender, completion_receiver) = mpsc::unbounded_channel();
        tokio::spawn(run_scheduler(
            settings,
            diagnostics,
            command_receiver,
            completion_sender,
            completion_receiver,
        ));
        Self {
            commands: command_sender,
        }
    }

    /// Atomically submits one event's callback snapshot before dispatching any
    /// member of that snapshot.
    ///
    /// # Errors
    ///
    /// Returns an internal callback error if the scheduler task has stopped.
    pub async fn submit_batch(
        &self,
        invocations: Vec<Invocation>,
    ) -> Result<Vec<InvocationHandle>, CallbackError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(SchedulerCommand::Submit {
                invocations,
                response: sender,
            })
            .await
            .map_err(|_| CallbackError::internal("callback scheduler is unavailable"))?;
        receiver
            .await
            .map_err(|_| CallbackError::internal("callback scheduler is unavailable"))
    }

    /// Applies a validated runtime capacity update. Running callbacks are never
    /// stopped when concurrency is reduced.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for invalid settings or an internal error
    /// if the scheduler task has stopped.
    pub async fn update_settings(&self, settings: CallbackSettings) -> Result<(), CallbackError> {
        settings.validate()?;
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(SchedulerCommand::Update {
                settings,
                response: sender,
            })
            .await
            .map_err(|_| CallbackError::internal("callback scheduler is unavailable"))?;
        receiver
            .await
            .map_err(|_| CallbackError::internal("callback scheduler is unavailable"))?
    }
}

async fn run_scheduler(
    mut settings: CallbackSettings,
    diagnostics: Arc<dyn DiagnosticSink>,
    mut commands: mpsc::Receiver<SchedulerCommand>,
    completion_sender: mpsc::UnboundedSender<ActualCompletion>,
    mut completions: mpsc::UnboundedReceiver<ActualCompletion>,
) {
    let mut queue = Vec::<QueuedInvocation>::new();
    let mut busy_lanes = HashSet::<HandlerId>::new();
    let mut active = 0_usize;
    let mut insertion_order = 0_u64;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    if active == 0 {
                        return;
                    }
                    let Some(completion) = completions.recv().await else {
                        return;
                    };
                    finish_actual(completion, &mut busy_lanes, &mut active);
                    dispatch_ready(
                        &mut queue,
                        &mut busy_lanes,
                        &mut active,
                        settings,
                        &diagnostics,
                        &completion_sender,
                    );
                    continue;
                };
                match command {
                    SchedulerCommand::Submit { invocations, response } => {
                        let mut handles = Vec::with_capacity(invocations.len());
                        for invocation in invocations {
                            let (logical, receiver) = oneshot::channel();
                            handles.push(InvocationHandle { receiver });
                            if invocation.reject_if_lane_busy
                                && busy_lanes.contains(&invocation.handler_id)
                            {
                                let _result = logical.send(CallbackOutcome::LaneBusy);
                                continue;
                            }
                            let queued = QueuedInvocation {
                                invocation,
                                insertion_order,
                                logical,
                            };
                            insertion_order = insertion_order.wrapping_add(1);
                            admit(
                                queued,
                                &mut queue,
                                settings.queue_capacity,
                                diagnostics.as_ref(),
                            );
                        }
                        let _result = response.send(handles);
                    }
                    SchedulerCommand::Update { settings: next, response } => {
                        settings = next;
                        shrink_queue(
                            &mut queue,
                            settings.queue_capacity,
                            diagnostics.as_ref(),
                        );
                        let _result = response.send(Ok(()));
                    }
                }
            }
            completion = completions.recv(), if active > 0 => {
                let Some(completion) = completion else {
                    return;
                };
                finish_actual(completion, &mut busy_lanes, &mut active);
            }
        }
        dispatch_ready(
            &mut queue,
            &mut busy_lanes,
            &mut active,
            settings,
            &diagnostics,
            &completion_sender,
        );
    }
}

fn finish_actual(
    completion: ActualCompletion,
    busy_lanes: &mut HashSet<HandlerId>,
    active: &mut usize,
) {
    busy_lanes.remove(&completion.handler_id);
    *active = active.saturating_sub(1);
    if let Some(logical) = completion.logical {
        let _result = logical.sender.send(logical.outcome);
    }
    if let Some(on_late_return) = completion.on_late_return {
        on_late_return();
    }
}

fn admit(
    queued: QueuedInvocation,
    queue: &mut Vec<QueuedInvocation>,
    capacity: usize,
    diagnostics: &dyn DiagnosticSink,
) {
    if queue.len() < capacity {
        queue.push(queued);
        return;
    }
    let lowest_priority = queue
        .iter()
        .map(|item| item.invocation.priority)
        .min()
        .expect("a full positive-capacity queue cannot be empty");
    if queued.invocation.priority <= lowest_priority {
        evict(
            queued,
            diagnostics,
            "new callback was dropped from the full queue",
        );
        return;
    }
    let oldest_lowest = queue
        .iter()
        .enumerate()
        .filter(|(_, item)| item.invocation.priority == lowest_priority)
        .min_by_key(|(_, item)| item.insertion_order)
        .map(|(index, _)| index)
        .expect("the minimum queue priority must have a member");
    let removed = queue.swap_remove(oldest_lowest);
    evict(
        removed,
        diagnostics,
        "older low-priority callback was evicted from the full queue",
    );
    queue.push(queued);
}

fn shrink_queue(
    queue: &mut Vec<QueuedInvocation>,
    capacity: usize,
    diagnostics: &dyn DiagnosticSink,
) {
    while queue.len() > capacity {
        let index = queue
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| eviction_order(left, right))
            .map(|(index, _)| index)
            .expect("an oversized queue cannot be empty");
        let removed = queue.swap_remove(index);
        evict(
            removed,
            diagnostics,
            "callback was evicted after queue capacity decreased",
        );
    }
}

fn eviction_order(left: &QueuedInvocation, right: &QueuedInvocation) -> Ordering {
    left.invocation
        .priority
        .cmp(&right.invocation.priority)
        .then_with(|| left.insertion_order.cmp(&right.insertion_order))
}

fn evict(queued: QueuedInvocation, diagnostics: &dyn DiagnosticSink, message: &'static str) {
    diagnostics.record(Diagnostic {
        level: DiagnosticLevel::Warning,
        code: "dynamic_callback_queue_evicted".to_owned(),
        message: message.to_owned(),
        handler_id: Some(queued.invocation.handler_id),
        event: queued.invocation.event.clone(),
    });
    let _result = queued.logical.send(CallbackOutcome::Evicted);
}

fn dispatch_ready(
    queue: &mut Vec<QueuedInvocation>,
    busy_lanes: &mut HashSet<HandlerId>,
    active: &mut usize,
    settings: CallbackSettings,
    diagnostics: &Arc<dyn DiagnosticSink>,
    completion_sender: &mpsc::UnboundedSender<ActualCompletion>,
) {
    while *active < settings.max_concurrency {
        let Some(index) = next_ready(queue, busy_lanes) else {
            return;
        };
        let queued = queue.swap_remove(index);
        let handler_id = queued.invocation.handler_id;
        busy_lanes.insert(handler_id);
        *active += 1;
        if let Some(on_start) = &queued.invocation.on_start {
            on_start();
        }
        let completion_sender = completion_sender.clone();
        let diagnostics = diagnostics.clone();
        tokio::spawn(async move {
            let completion = run_invocation(queued, settings, diagnostics).await;
            let _result = completion_sender.send(completion);
        });
    }
}

fn next_ready(queue: &[QueuedInvocation], busy_lanes: &HashSet<HandlerId>) -> Option<usize> {
    queue
        .iter()
        .enumerate()
        .filter(|(_, item)| !busy_lanes.contains(&item.invocation.handler_id))
        .max_by(|(_, left), (_, right)| dispatch_order(left, right))
        .map(|(index, _)| index)
}

fn dispatch_order(left: &QueuedInvocation, right: &QueuedInvocation) -> Ordering {
    left.invocation
        .priority
        .cmp(&right.invocation.priority)
        .then_with(|| {
            right
                .invocation
                .event_sequence
                .cmp(&left.invocation.event_sequence)
        })
        .then_with(|| {
            right
                .invocation
                .registration_order
                .cmp(&left.invocation.registration_order)
        })
        .then_with(|| right.insertion_order.cmp(&left.insertion_order))
}

async fn run_invocation(
    queued: QueuedInvocation,
    settings: CallbackSettings,
    diagnostics: Arc<dyn DiagnosticSink>,
) -> ActualCompletion {
    let handler_id = queued.invocation.handler_id;
    let mut logical = Some(queued.logical);
    let limits = match queued.invocation.limits.resolve(settings) {
        Ok(limits) => limits,
        Err(error) => {
            return complete_actual(
                handler_id,
                &mut logical,
                CallbackOutcome::Failed(error),
                None,
            );
        }
    };
    let started_at = Instant::now();
    let deadline = Arc::new(DeadlineSignal::default());
    let event = queued.invocation.event.clone();
    let context = InvocationContext {
        handler_id: queued.invocation.handler_id,
        event: event.clone(),
        arguments: queued.invocation.arguments,
        limits,
        started_at,
        deadline: deadline.clone(),
    };
    let callback = queued.invocation.callback;
    let mut actual = tokio::spawn(async move { callback.invoke(context).await });
    let soft_at = enabled_deadline(started_at, limits.soft_timeout_ms);
    let logical_soft_at =
        soft_at.map(|instant| instant + Duration::from_millis(limits.soft_timeout_grace_ms));
    let hard_at = enabled_deadline(started_at, limits.hard_timeout_ms);
    let mut soft_applied = false;
    let mut logical_soft_applied = false;
    let mut hard_applied = false;
    let on_late_return = queued.invocation.on_late_return;

    loop {
        tokio::select! {
            biased;
            result = &mut actual => {
                let now = Instant::now();
                if hard_at.is_some_and(|deadline_at| now >= deadline_at) {
                    deadline.enter_hard();
                    return complete_actual(
                        handler_id,
                        &mut logical,
                        CallbackOutcome::TimedOut(TimeoutStage::Hard),
                        on_late_return,
                    );
                } else if logical_soft_at.is_some_and(|deadline_at| now >= deadline_at) {
                    return complete_actual(
                        handler_id,
                        &mut logical,
                        CallbackOutcome::TimedOut(TimeoutStage::Soft),
                        on_late_return,
                    );
                }
                let returned_after_logical_completion = logical.is_none();
                let outcome = callback_outcome(result);
                let late_return = returned_after_logical_completion
                    .then_some(on_late_return)
                    .flatten();
                return complete_actual(handler_id, &mut logical, outcome, late_return);
            }
            () = sleep_optional(soft_at), if !soft_applied => {
                soft_applied = true;
                deadline.enter_soft();
            }
            () = sleep_optional(logical_soft_at), if !logical_soft_applied => {
                logical_soft_applied = true;
                if logical.is_some() {
                    record_timeout_diagnostic(
                        diagnostics.as_ref(),
                        handler_id,
                        event.as_ref(),
                        TimeoutStage::Soft,
                    );
                }
                send_logical(&mut logical, CallbackOutcome::TimedOut(TimeoutStage::Soft));
            }
            () = sleep_optional(hard_at), if !hard_applied => {
                hard_applied = true;
                deadline.enter_hard();
                if logical.is_some() {
                    record_timeout_diagnostic(
                        diagnostics.as_ref(),
                        handler_id,
                        event.as_ref(),
                        TimeoutStage::Hard,
                    );
                }
                send_logical(&mut logical, CallbackOutcome::TimedOut(TimeoutStage::Hard));
            }
        }
    }
}

fn enabled_deadline(started_at: Instant, milliseconds: u64) -> Option<Instant> {
    (milliseconds > 0).then(|| started_at + Duration::from_millis(milliseconds))
}

async fn sleep_optional(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        sleep_until(deadline).await;
    } else {
        pending::<()>().await;
    }
}

fn send_logical(sender: &mut Option<oneshot::Sender<CallbackOutcome>>, outcome: CallbackOutcome) {
    if let Some(sender) = sender.take() {
        let _result = sender.send(outcome);
    }
}

fn take_logical(
    sender: &mut Option<oneshot::Sender<CallbackOutcome>>,
    outcome: CallbackOutcome,
) -> Option<LogicalCompletion> {
    sender
        .take()
        .map(|sender| LogicalCompletion { sender, outcome })
}

fn complete_actual(
    handler_id: HandlerId,
    logical: &mut Option<oneshot::Sender<CallbackOutcome>>,
    outcome: CallbackOutcome,
    on_late_return: Option<Arc<dyn Fn() + Send + Sync>>,
) -> ActualCompletion {
    ActualCompletion {
        handler_id,
        logical: take_logical(logical, outcome),
        on_late_return,
    }
}

fn callback_outcome(
    result: Result<Result<CallbackReturn, CallbackError>, tokio::task::JoinError>,
) -> CallbackOutcome {
    match result {
        Ok(Ok(value)) => CallbackOutcome::Returned(value),
        Ok(Err(error)) if error.kind == CallbackErrorKind::SoftTimeout => {
            CallbackOutcome::TimedOut(TimeoutStage::Soft)
        }
        Ok(Err(error)) if error.kind == CallbackErrorKind::HardTimeout => {
            CallbackOutcome::TimedOut(TimeoutStage::Hard)
        }
        Ok(Err(error)) => CallbackOutcome::Failed(error),
        Err(error) => CallbackOutcome::Failed(CallbackError::internal(format!(
            "callback task failed: {error}"
        ))),
    }
}

fn record_timeout_diagnostic(
    diagnostics: &dyn DiagnosticSink,
    handler_id: HandlerId,
    event: Option<&String>,
    stage: TimeoutStage,
) {
    let (level, code, message) = match stage {
        TimeoutStage::Running => return,
        TimeoutStage::Soft => (
            DiagnosticLevel::Warning,
            "dynamic_callback_soft_timeout",
            "callback exceeded its soft timeout grace period",
        ),
        TimeoutStage::Hard => (
            DiagnosticLevel::Error,
            "dynamic_callback_hard_timeout",
            "callback exceeded its hard timeout",
        ),
    };
    diagnostics.record(Diagnostic {
        level,
        code: code.to_owned(),
        message: message.to_owned(),
        handler_id: Some(handler_id),
        event: event.cloned(),
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use parking_lot::Mutex;
    use tokio::sync::Notify;
    use tokio::time::{Duration, timeout};

    use super::*;

    #[derive(Default)]
    struct RecordingDiagnostics(Mutex<Vec<Diagnostic>>);

    impl DiagnosticSink for RecordingDiagnostics {
        fn record(&self, diagnostic: Diagnostic) {
            self.0.lock().push(diagnostic);
        }
    }

    struct BlockingCallback {
        started: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        release: Arc<Notify>,
        returned: CallbackReturn,
    }

    #[async_trait]
    impl Callback for BlockingCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            self.started.fetch_add(1, Ordering::AcqRel);
            let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.maximum.fetch_max(active, Ordering::AcqRel);
            self.release.notified().await;
            self.active.fetch_sub(1, Ordering::AcqRel);
            Ok(self.returned.clone())
        }
    }

    fn invocation(handler_id: u64, priority: i32, callback: Arc<dyn Callback>) -> Invocation {
        Invocation {
            handler_id: HandlerId::new(handler_id),
            priority,
            event_sequence: 0,
            registration_order: handler_id,
            event: Some("TestPost".to_owned()),
            arguments: Vec::new(),
            limits: CallbackLimits::default(),
            callback,
            on_start: None,
            reject_if_lane_busy: false,
            on_late_return: None,
        }
    }

    #[tokio::test]
    async fn same_lane_is_serial_and_distinct_lanes_reach_the_bound() {
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let executor = CallbackExecutor::new(
            CallbackSettings {
                max_concurrency: 2,
                ..CallbackSettings::default()
            },
            diagnostics,
        );
        let started = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let callback: Arc<dyn Callback> = Arc::new(BlockingCallback {
            started: started.clone(),
            active: active.clone(),
            maximum: maximum.clone(),
            release: release.clone(),
            returned: CallbackReturn::None,
        });
        let handles = executor
            .submit_batch(vec![
                invocation(1, 0, callback.clone()),
                invocation(1, 0, callback.clone()),
                invocation(2, 0, callback),
            ])
            .await
            .unwrap();
        timeout(Duration::from_secs(1), async {
            while started.load(Ordering::Acquire) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(maximum.load(Ordering::Acquire), 2);
        assert_eq!(started.load(Ordering::Acquire), 2);
        release.notify_waiters();
        timeout(Duration::from_secs(1), async {
            while started.load(Ordering::Acquire) < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        release.notify_waiters();
        for handle in handles {
            assert_eq!(
                handle.outcome().await,
                CallbackOutcome::Returned(CallbackReturn::None)
            );
        }
    }

    struct LateCallback {
        actual_ended: Arc<AtomicBool>,
        release: Arc<Notify>,
    }

    struct ImmediateCallback(CallbackReturn);

    #[async_trait]
    impl Callback for ImmediateCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn arbitrary_callback_return_is_preserved() {
        let executor = CallbackExecutor::new(
            CallbackSettings::default(),
            Arc::new(RecordingDiagnostics::default()),
        );
        let expected = CallbackReturn::Value(serde_json::json!({
            "items": [1, true, null]
        }));
        let mut handles = executor
            .submit_batch(vec![invocation(
                1,
                0,
                Arc::new(ImmediateCallback(expected.clone())),
            )])
            .await
            .unwrap();
        assert_eq!(
            handles.remove(0).outcome().await,
            CallbackOutcome::Returned(expected)
        );
    }

    #[tokio::test]
    async fn normal_completion_releases_lane_before_observable_outcome() {
        let executor = CallbackExecutor::new(
            CallbackSettings::default(),
            Arc::new(RecordingDiagnostics::default()),
        );
        let callback: Arc<dyn Callback> = Arc::new(ImmediateCallback(CallbackReturn::None));
        for _ in 0..100 {
            let mut next = invocation(1, 0, callback.clone());
            next.reject_if_lane_busy = true;
            let mut handles = executor.submit_batch(vec![next]).await.unwrap();
            assert_eq!(
                handles.remove(0).outcome().await,
                CallbackOutcome::Returned(CallbackReturn::None)
            );
        }
    }

    #[async_trait]
    impl Callback for LateCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            self.release.notified().await;
            self.actual_ended.store(true, Ordering::Release);
            Ok(CallbackReturn::Boolean(false))
        }
    }

    #[tokio::test]
    async fn logical_soft_completion_does_not_release_lane_or_slot() {
        let executor = CallbackExecutor::new(
            CallbackSettings {
                soft_timeout_ms: 10,
                soft_timeout_grace_ms: 10,
                hard_timeout_ms: 0,
                max_concurrency: 1,
                queue_capacity: 8,
            },
            Arc::new(RecordingDiagnostics::default()),
        );
        let ended = Arc::new(AtomicBool::new(false));
        let first_release = Arc::new(Notify::new());
        let first: Arc<dyn Callback> = Arc::new(LateCallback {
            actual_ended: ended.clone(),
            release: first_release.clone(),
        });
        let second_started = Arc::new(AtomicUsize::new(0));
        let second_release = Arc::new(Notify::new());
        let second: Arc<dyn Callback> = Arc::new(BlockingCallback {
            started: second_started.clone(),
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
            release: second_release.clone(),
            returned: CallbackReturn::None,
        });
        let mut handles = executor
            .submit_batch(vec![invocation(1, 0, first), invocation(2, 0, second)])
            .await
            .unwrap();
        let first_handle = handles.remove(0);
        assert_eq!(
            timeout(Duration::from_secs(1), first_handle.outcome())
                .await
                .unwrap(),
            CallbackOutcome::TimedOut(TimeoutStage::Soft)
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!ended.load(Ordering::Acquire));
        assert_eq!(second_started.load(Ordering::Acquire), 0);
        first_release.notify_one();
        timeout(Duration::from_secs(1), async {
            while second_started.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        second_release.notify_one();
        assert_eq!(
            handles.remove(0).outcome().await,
            CallbackOutcome::Returned(CallbackReturn::None)
        );
    }

    #[tokio::test]
    async fn occupied_lane_rejects_new_work_and_notifies_after_late_return() {
        let executor = CallbackExecutor::new(
            CallbackSettings {
                soft_timeout_ms: 1,
                soft_timeout_grace_ms: 1,
                hard_timeout_ms: 0,
                max_concurrency: 1,
                queue_capacity: 8,
            },
            Arc::new(RecordingDiagnostics::default()),
        );
        let ended = Arc::new(AtomicBool::new(false));
        let release = Arc::new(Notify::new());
        let callback: Arc<dyn Callback> = Arc::new(LateCallback {
            actual_ended: ended.clone(),
            release: release.clone(),
        });
        let late_returns = Arc::new(AtomicUsize::new(0));
        let mut first = invocation(1, 0, callback.clone());
        first.reject_if_lane_busy = true;
        let late_returns_for_hook = late_returns.clone();
        first.on_late_return = Some(Arc::new(move || {
            late_returns_for_hook.fetch_add(1, Ordering::AcqRel);
        }));
        let mut handles = executor.submit_batch(vec![first]).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), handles.remove(0).outcome())
                .await
                .unwrap(),
            CallbackOutcome::TimedOut(TimeoutStage::Soft)
        );

        let mut second = invocation(1, 0, callback);
        second.reject_if_lane_busy = true;
        let mut handles = executor.submit_batch(vec![second]).await.unwrap();
        assert_eq!(handles.remove(0).outcome().await, CallbackOutcome::LaneBusy);

        release.notify_one();
        timeout(Duration::from_secs(1), async {
            while late_returns.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(ended.load(Ordering::Acquire));
        assert_eq!(late_returns.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn full_queue_evicts_oldest_lowest_only_for_higher_priority() {
        let diagnostics = Arc::new(RecordingDiagnostics::default());
        let executor = CallbackExecutor::new(
            CallbackSettings {
                soft_timeout_ms: 0,
                soft_timeout_grace_ms: 0,
                hard_timeout_ms: 0,
                max_concurrency: 1,
                queue_capacity: 2,
            },
            diagnostics.clone(),
        );
        let release = Arc::new(Notify::new());
        let callback: Arc<dyn Callback> = Arc::new(BlockingCallback {
            started: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
            release: release.clone(),
            returned: CallbackReturn::None,
        });
        let mut first = executor
            .submit_batch(vec![invocation(1, 0, callback.clone())])
            .await
            .unwrap();
        tokio::task::yield_now().await;
        let queued = executor
            .submit_batch(vec![
                invocation(2, -1, callback.clone()),
                invocation(3, -1, callback.clone()),
                invocation(4, 5, callback),
            ])
            .await
            .unwrap();
        assert_eq!(
            queued.into_iter().next().unwrap().outcome().await,
            CallbackOutcome::Evicted
        );
        release.notify_waiters();
        assert_eq!(
            first.remove(0).outcome().await,
            CallbackOutcome::Returned(CallbackReturn::None)
        );
        release.notify_waiters();
        release.notify_waiters();
        assert!(!diagnostics.0.lock().is_empty());
    }
}
