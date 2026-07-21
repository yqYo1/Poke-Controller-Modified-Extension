//! Atomic UI-visible state snapshots and revisioned change publication.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, broadcast};

use crate::api::{
    DecimalString, RevisionedStateChange, SettingsChange, SettingsSnapshot, SettingsWriteValues,
    StateChangeCause, StatePatch, StateSnapshot, UiStateChange,
};

/// One consistent view of both REST snapshot domains.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleSnapshots {
    pub settings: SettingsSnapshot,
    pub state: StateSnapshot,
}

/// One proposed UI-visible transaction.
#[derive(Clone, Debug)]
pub struct StateTransaction {
    pub expected_revision: Option<DecimalString>,
    pub cause: StateChangeCause,
    pub state: StatePatch,
    pub settings: Option<SettingsChange>,
}

impl StateTransaction {
    #[must_use]
    pub fn new(cause: StateChangeCause) -> Self {
        Self {
            expected_revision: None,
            cause,
            state: StatePatch::default(),
            settings: None,
        }
    }
}

/// Result of a successful transaction, including the exact REST snapshots
/// produced under the same revision gate.
#[derive(Clone, Debug)]
pub struct CommitOutcome {
    pub snapshots: VisibleSnapshots,
    pub event: Option<Arc<RevisionedStateChange>>,
}

impl CommitOutcome {
    #[must_use]
    pub fn revision(&self) -> &DecimalString {
        &self.snapshots.state.revision
    }

    #[must_use]
    pub const fn changed(&self) -> bool {
        self.event.is_some()
    }
}

/// A bounded replay attempt. A gap always requires both REST snapshots to be
/// fetched again; callers must not infer the missing changes.
#[derive(Clone, Debug)]
pub enum Replay {
    Complete {
        current_revision: DecimalString,
        events: Vec<Arc<RevisionedStateChange>>,
    },
    Gap {
        current_revision: DecimalString,
    },
}

impl Replay {
    #[must_use]
    pub fn current_revision(&self) -> &DecimalString {
        match self {
            Self::Complete {
                current_revision, ..
            }
            | Self::Gap { current_revision } => current_revision,
        }
    }
}

/// Errors that reject a transaction without changing snapshots or revision.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum StateTransactionError {
    #[error("expected revision {expected}, but current revision is {current}")]
    RevisionConflict {
        expected: DecimalString,
        current: DecimalString,
    },
    #[error("command_candidates, tags, and command_display_lists must be published together")]
    IncompleteCommandDisplayGeneration,
    #[error("camera_fps must be finite")]
    NonFiniteCameraFps,
    #[error("the process revision counter is exhausted")]
    RevisionExhausted,
}

/// Invalid process-start state.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum StateHubBuildError {
    #[error("state history capacity must be greater than zero")]
    ZeroHistoryCapacity,
    #[error("both initial snapshots must have revision zero")]
    NonZeroInitialRevision,
    #[error("initial camera_fps must be finite")]
    NonFiniteCameraFps,
}

#[derive(Debug)]
struct VisibleState {
    revision: u64,
    snapshots: VisibleSnapshots,
    history: VecDeque<Arc<RevisionedStateChange>>,
}

#[derive(Debug)]
struct StateHubInner {
    gate: Mutex<VisibleState>,
    events: broadcast::Sender<Arc<RevisionedStateChange>>,
    history_capacity: usize,
}

/// Process-wide serialization gate for every UI-visible mutation.
#[derive(Clone, Debug)]
pub struct StateHub {
    inner: Arc<StateHubInner>,
}

impl StateHub {
    /// Creates the process state at revision zero.
    ///
    /// # Errors
    ///
    /// Returns an error for an unusable history capacity or invalid initial
    /// wire snapshots.
    pub fn new(
        settings: SettingsSnapshot,
        state: StateSnapshot,
        history_capacity: usize,
    ) -> Result<Self, StateHubBuildError> {
        if history_capacity == 0 {
            return Err(StateHubBuildError::ZeroHistoryCapacity);
        }
        if settings.revision != DecimalString::zero() || state.revision != DecimalString::zero() {
            return Err(StateHubBuildError::NonZeroInitialRevision);
        }
        if !state.camera_fps.is_finite() {
            return Err(StateHubBuildError::NonFiniteCameraFps);
        }
        let (events, _) = broadcast::channel(history_capacity);
        Ok(Self {
            inner: Arc::new(StateHubInner {
                gate: Mutex::new(VisibleState {
                    revision: 0,
                    snapshots: VisibleSnapshots { settings, state },
                    history: VecDeque::with_capacity(history_capacity),
                }),
                events,
                history_capacity,
            }),
        })
    }

    /// Subscribes before fetching REST snapshots, as required by the client
    /// initialization protocol.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<RevisionedStateChange>> {
        self.inner.events.subscribe()
    }

    /// Returns an atomic copy of both snapshot domains.
    pub async fn snapshots(&self) -> VisibleSnapshots {
        self.inner.gate.lock().await.snapshots.clone()
    }

    /// Returns the current settings snapshot.
    pub async fn settings_snapshot(&self) -> SettingsSnapshot {
        self.inner.gate.lock().await.snapshots.settings.clone()
    }

    /// Returns the current state snapshot.
    pub async fn state_snapshot(&self) -> StateSnapshot {
        self.inner.gate.lock().await.snapshots.state.clone()
    }

    /// Atomically applies one sparse transaction, increments the global
    /// revision once, and publishes exactly one event. A semantic no-op keeps
    /// the current revision and publishes nothing.
    ///
    /// # Errors
    ///
    /// Returns an error without side effects for revision conflicts, invalid
    /// command-display generations, non-finite FPS values, or counter
    /// exhaustion.
    pub async fn commit(
        &self,
        transaction: StateTransaction,
    ) -> Result<CommitOutcome, StateTransactionError> {
        validate_transaction(&transaction)?;
        let mut visible = self.inner.gate.lock().await;
        let current_revision = DecimalString::from_u64(visible.revision);
        if let Some(expected) = transaction.expected_revision
            && expected != current_revision
        {
            return Err(StateTransactionError::RevisionConflict {
                expected,
                current: current_revision,
            });
        }

        let mut proposed_snapshots = visible.snapshots.clone();
        let state = apply_state_patch(&mut proposed_snapshots.state, transaction.state);
        let settings = transaction
            .settings
            .and_then(|change| apply_settings_change(&mut proposed_snapshots.settings, change));
        if state == StatePatch::default() && settings.is_none() {
            return Ok(CommitOutcome {
                snapshots: visible.snapshots.clone(),
                event: None,
            });
        }

        let revision = visible
            .revision
            .checked_add(1)
            .ok_or(StateTransactionError::RevisionExhausted)?;
        visible.revision = revision;
        let revision = DecimalString::from_u64(revision);
        proposed_snapshots.settings.revision = revision.clone();
        proposed_snapshots.state.revision = revision.clone();
        visible.snapshots = proposed_snapshots;
        let event = Arc::new(RevisionedStateChange {
            revision,
            data: UiStateChange {
                cause: transaction.cause,
                state,
                settings,
            },
        });
        visible.history.push_back(Arc::clone(&event));
        if visible.history.len() > self.inner.history_capacity {
            visible.history.pop_front();
        }

        // Sending while the gate is held makes publication order identical to
        // commit order. `broadcast::send` is synchronous and never waits for a
        // slow receiver.
        let _receiver_count = self.inner.events.send(Arc::clone(&event));
        Ok(CommitOutcome {
            snapshots: visible.snapshots.clone(),
            event: Some(event),
        })
    }

    /// Replays the retained contiguous suffix after `revision`.
    ///
    /// A future, unrepresentable, or evicted revision returns [`Replay::Gap`]
    /// and requires a fresh settings/state snapshot pair.
    pub async fn replay_after(&self, revision: &DecimalString) -> Replay {
        let visible = self.inner.gate.lock().await;
        let current_revision = DecimalString::from_u64(visible.revision);
        let Ok(requested) = revision.as_str().parse::<u64>() else {
            return Replay::Gap { current_revision };
        };
        if requested > visible.revision {
            return Replay::Gap { current_revision };
        }
        if requested == visible.revision {
            return Replay::Complete {
                current_revision,
                events: Vec::new(),
            };
        }
        let Some(first) = visible.history.front() else {
            return Replay::Gap { current_revision };
        };
        let Ok(first_revision) = first.revision.as_str().parse::<u64>() else {
            return Replay::Gap { current_revision };
        };
        if requested.checked_add(1) != Some(first_revision) && requested < first_revision {
            return Replay::Gap { current_revision };
        }
        let mut events = Vec::new();
        for event in &visible.history {
            let Ok(event_revision) = event.revision.as_str().parse::<u64>() else {
                return Replay::Gap { current_revision };
            };
            if event_revision > requested {
                events.push(Arc::clone(event));
            }
        }
        Replay::Complete {
            current_revision,
            events,
        }
    }
}

fn validate_transaction(transaction: &StateTransaction) -> Result<(), StateTransactionError> {
    let command_generation_fields = [
        transaction.state.command_candidates.is_some(),
        transaction.state.tags.is_some(),
        transaction.state.command_display_lists.is_some(),
    ];
    let present = command_generation_fields
        .into_iter()
        .filter(|field| *field)
        .count();
    if present != 0 && present != command_generation_fields.len() {
        return Err(StateTransactionError::IncompleteCommandDisplayGeneration);
    }
    if transaction
        .state
        .camera_fps
        .is_some_and(|fps| !fps.is_finite())
    {
        return Err(StateTransactionError::NonFiniteCameraFps);
    }
    Ok(())
}

fn apply_value<T: Clone + PartialEq>(target: &mut T, proposed: Option<T>) -> Option<T> {
    proposed.and_then(|value| {
        if *target == value {
            None
        } else {
            *target = value.clone();
            Some(value)
        }
    })
}

#[allow(clippy::too_many_lines)]
fn apply_state_patch(snapshot: &mut StateSnapshot, patch: StatePatch) -> StatePatch {
    StatePatch {
        serial_port: apply_value(&mut snapshot.serial_port, patch.serial_port),
        serial_baud_rate: apply_value(&mut snapshot.serial_baud_rate, patch.serial_baud_rate),
        serial_connected: apply_value(&mut snapshot.serial_connected, patch.serial_connected),
        camera_opened: apply_value(&mut snapshot.camera_opened, patch.camera_opened),
        camera_fps: apply_value(&mut snapshot.camera_fps, patch.camera_fps),
        camera_resolution: apply_value(&mut snapshot.camera_resolution, patch.camera_resolution),
        camera_device: apply_value(&mut snapshot.camera_device, patch.camera_device),
        is_running: apply_value(&mut snapshot.is_running, patch.is_running),
        command_state: apply_value(&mut snapshot.command_state, patch.command_state),
        current_command: apply_value(&mut snapshot.current_command, patch.current_command),
        command_candidates: apply_value(&mut snapshot.command_candidates, patch.command_candidates),
        tags: apply_value(&mut snapshot.tags, patch.tags),
        active_profile: apply_value(&mut snapshot.active_profile, patch.active_profile),
        pending_profile: apply_value(&mut snapshot.pending_profile, patch.pending_profile),
        available_profiles: apply_value(&mut snapshot.available_profiles, patch.available_profiles),
        last_input: apply_value(&mut snapshot.last_input, patch.last_input),
        holding_buttons: apply_value(&mut snapshot.holding_buttons, patch.holding_buttons),
        pid: apply_value(&mut snapshot.pid, patch.pid),
        command_display_lists: apply_value(
            &mut snapshot.command_display_lists,
            patch.command_display_lists,
        ),
        command_display_cache_loading: apply_value(
            &mut snapshot.command_display_cache_loading,
            patch.command_display_cache_loading,
        ),
    }
}

fn merge_changes(
    target: &mut BTreeMap<String, Value>,
    proposed: BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    proposed
        .into_iter()
        .filter_map(|(id, value)| {
            if target.get(&id) == Some(&value) {
                None
            } else {
                target.insert(id.clone(), value.clone());
                Some((id, value))
            }
        })
        .collect()
}

fn apply_settings_change(
    snapshot: &mut SettingsSnapshot,
    change: SettingsChange,
) -> Option<SettingsChange> {
    let values = merge_changes(&mut snapshot.values.0, change.values.0);
    let previous_pending = snapshot.pending_restart_values.0.clone();
    let pending_restart_values = merge_changes(
        &mut snapshot.pending_restart_values.0,
        change.pending_restart_values.0,
    );
    let restart_ids = change
        .restart_required
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    snapshot
        .pending_restart_values
        .0
        .retain(|id, _value| restart_ids.contains(id));
    let pending_changed = snapshot.pending_restart_values.0 != previous_pending;
    let restart_changed = snapshot.restart_required != change.restart_required;
    let failures_changed = snapshot.apply_failures != change.apply_failures;
    snapshot
        .restart_required
        .clone_from(&change.restart_required);
    snapshot.apply_failures.clone_from(&change.apply_failures);

    (!values.is_empty() || pending_changed || restart_changed || failures_changed).then_some(
        SettingsChange {
            values: SettingsWriteValues(values),
            pending_restart_values: SettingsWriteValues(pending_restart_values),
            restart_required: change.restart_required,
            apply_failures: change.apply_failures,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::broadcast::error::TryRecvError;

    use super::{Replay, StateHub, StateTransaction, StateTransactionError, VisibleSnapshots};
    use crate::api::{
        CameraSelector, CommandInfo, CommandState, DecimalString, SettingsChange,
        SettingsReadValues, SettingsSnapshot, SettingsWriteValues, StateChangeCause, StateSnapshot,
    };

    fn snapshots() -> VisibleSnapshots {
        VisibleSnapshots {
            settings: SettingsSnapshot {
                revision: DecimalString::zero(),
                values: SettingsReadValues(BTreeMap::from([(
                    "sample.mode".to_owned(),
                    json!("old"),
                )])),
                pending_restart_values: SettingsWriteValues::default(),
                restart_required: Vec::new(),
                apply_failures: BTreeMap::new(),
            },
            state: StateSnapshot {
                revision: DecimalString::zero(),
                serial_port: None,
                serial_baud_rate: 115_200,
                serial_connected: false,
                camera_opened: false,
                camera_fps: 0.0,
                camera_resolution: "1280x720".to_owned(),
                camera_device: CameraSelector::Index(0),
                is_running: false,
                command_state: CommandState::Stopped,
                current_command: None,
                command_candidates: Vec::new(),
                tags: Vec::new(),
                active_profile: "default".to_owned(),
                pending_profile: None,
                available_profiles: vec!["default".to_owned()],
                last_input: None,
                holding_buttons: Vec::new(),
                pid: 42,
                command_display_lists: BTreeMap::from([("-".to_owned(), Vec::new())]),
                command_display_cache_loading: false,
            },
        }
    }

    fn hub(capacity: usize) -> StateHub {
        let snapshots = snapshots();
        StateHub::new(snapshots.settings, snapshots.state, capacity).expect("valid initial state")
    }

    #[tokio::test]
    async fn settings_state_snapshot_and_event_share_one_revision() {
        let hub = hub(8);
        let mut receiver = hub.subscribe();
        let mut transaction = StateTransaction::new(StateChangeCause::Settings);
        transaction.expected_revision = Some(DecimalString::zero());
        transaction.state.camera_opened = Some(true);
        transaction.settings = Some(SettingsChange {
            values: SettingsWriteValues(BTreeMap::from([("sample.mode".to_owned(), json!("new"))])),
            ..SettingsChange::default()
        });

        let outcome = hub.commit(transaction).await.expect("transaction commits");
        assert!(outcome.changed());
        assert_eq!(outcome.revision().as_str(), "1");
        assert_eq!(outcome.snapshots.settings.revision, *outcome.revision());
        assert_eq!(
            outcome.snapshots.settings.values.0["sample.mode"],
            json!("new")
        );
        assert!(outcome.snapshots.state.camera_opened);

        let event = receiver.recv().await.expect("one event is broadcast");
        assert_eq!(event.revision, *outcome.revision());
        assert_eq!(event.data.state.camera_opened, Some(true));
        assert!(event.data.settings.is_some());
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn semantic_noop_does_not_increment_or_publish() {
        let hub = hub(8);
        let mut receiver = hub.subscribe();
        let mut transaction = StateTransaction::new(StateChangeCause::Camera);
        transaction.state.camera_opened = Some(false);

        let outcome = hub.commit(transaction).await.expect("no-op succeeds");
        assert!(!outcome.changed());
        assert_eq!(outcome.revision().as_str(), "0");
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn conflict_has_no_side_effects() {
        let hub = hub(8);
        let mut receiver = hub.subscribe();
        let mut transaction = StateTransaction::new(StateChangeCause::Serial);
        transaction.expected_revision = Some("18446744073709551616".parse().expect("decimal"));
        transaction.state.serial_connected = Some(true);

        let error = hub
            .commit(transaction)
            .await
            .expect_err("revision conflicts");
        assert!(matches!(
            error,
            StateTransactionError::RevisionConflict { current, .. } if current.as_str() == "0"
        ));
        assert!(!hub.state_snapshot().await.serial_connected);
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn command_display_generation_is_indivisible() {
        let hub = hub(8);
        let mut incomplete = StateTransaction::new(StateChangeCause::Commands);
        incomplete.state.tags = Some(vec!["utility".to_owned()]);
        assert_eq!(
            hub.commit(incomplete)
                .await
                .expect_err("partial generation"),
            StateTransactionError::IncompleteCommandDisplayGeneration
        );

        let command = CommandInfo {
            name: "Sample".to_owned(),
            module_path: "sample".to_owned(),
            class_name: "Sample".to_owned(),
            tags: vec!["utility".to_owned()],
        };
        let mut complete = StateTransaction::new(StateChangeCause::Commands);
        complete.state.command_candidates = Some(vec![command]);
        complete.state.tags = Some(vec!["utility".to_owned()]);
        complete.state.command_display_lists = Some(BTreeMap::from([
            ("-".to_owned(), Vec::new()),
            ("utility".to_owned(), Vec::new()),
        ]));
        let outcome = hub.commit(complete).await.expect("complete generation");
        assert_eq!(outcome.revision().as_str(), "1");
    }

    #[tokio::test]
    async fn pending_restart_removal_changes_the_snapshot() {
        let hub = hub(8);
        let mut add = StateTransaction::new(StateChangeCause::Settings);
        add.settings = Some(SettingsChange {
            pending_restart_values: SettingsWriteValues(BTreeMap::from([(
                "server.bind_address".to_owned(),
                json!("0.0.0.0:8080"),
            )])),
            restart_required: vec!["server.bind_address".to_owned()],
            ..SettingsChange::default()
        });
        hub.commit(add).await.expect("pending value added");

        let mut remove = StateTransaction::new(StateChangeCause::Settings);
        remove.settings = Some(SettingsChange::default());
        let outcome = hub.commit(remove).await.expect("pending value removed");
        assert_eq!(outcome.revision().as_str(), "2");
        assert!(
            outcome
                .snapshots
                .settings
                .pending_restart_values
                .0
                .is_empty()
        );
    }

    #[tokio::test]
    async fn bounded_replay_reports_evicted_and_future_gaps() {
        let hub = hub(2);
        for value in ["one", "two", "three"] {
            let mut transaction = StateTransaction::new(StateChangeCause::Other);
            transaction.state.last_input = Some(Some(value.to_owned()));
            hub.commit(transaction).await.expect("change commits");
        }

        let Replay::Complete { events, .. } = hub.replay_after(&DecimalString::from_u64(1)).await
        else {
            panic!("retained suffix must replay");
        };
        assert_eq!(
            events
                .iter()
                .map(|event| event.revision.as_str())
                .collect::<Vec<_>>(),
            ["2", "3"]
        );
        assert!(matches!(
            hub.replay_after(&DecimalString::zero()).await,
            Replay::Gap { .. }
        ));
        assert!(matches!(
            hub.replay_after(&DecimalString::from_u64(4)).await,
            Replay::Gap { .. }
        ));
    }

    #[tokio::test]
    async fn concurrent_publication_is_unique_and_ordered() {
        let hub = hub(64);
        let mut receiver = hub.subscribe();
        let tasks = (0..32)
            .map(|index| {
                let hub = hub.clone();
                tokio::spawn(async move {
                    let mut transaction = StateTransaction::new(StateChangeCause::Other);
                    transaction.state.last_input = Some(Some(format!("input-{index}")));
                    hub.commit(transaction).await.expect("change commits")
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.await.expect("task does not panic");
        }

        for revision in 1..=32 {
            let event = receiver.recv().await.expect("event retained by receiver");
            assert_eq!(event.revision, DecimalString::from_u64(revision));
        }
        assert_eq!(hub.snapshots().await.state.revision.as_str(), "32");
    }

    #[tokio::test]
    async fn exhausted_counter_never_wraps() {
        let hub = hub(8);
        {
            let mut visible = hub.inner.gate.lock().await;
            visible.revision = u64::MAX;
            let revision = DecimalString::from_u64(u64::MAX);
            visible.snapshots.settings.revision = revision.clone();
            visible.snapshots.state.revision = revision;
        }
        let mut transaction = StateTransaction::new(StateChangeCause::Other);
        transaction.state.last_input = Some(Some("change".to_owned()));
        assert_eq!(
            hub.commit(transaction)
                .await
                .expect_err("counter exhausted"),
            StateTransactionError::RevisionExhausted
        );
        assert_eq!(
            hub.snapshots().await,
            VisibleSnapshots {
                settings: SettingsSnapshot {
                    revision: DecimalString::from_u64(u64::MAX),
                    ..snapshots().settings
                },
                state: StateSnapshot {
                    revision: DecimalString::from_u64(u64::MAX),
                    ..snapshots().state
                },
            }
        );
    }

    #[test]
    fn arc_events_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<crate::api::RevisionedStateChange>>();
    }
}
