use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio_util::sync::CancellationToken;

use crate::worker::WorkerKind;

/// Monotonic identity assigned to one operating-system worker instance.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GenerationId(u64);

impl GenerationId {
    /// Returns the process-local numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Atomic lifecycle phase for a worker generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationPhase {
    /// Normal operations are accepted.
    Running,
    /// Mutating/resource operations are rejected and queued work is cancelled.
    Stopping,
    /// The operating-system process has been reaped.
    Stopped,
}

impl GenerationPhase {
    const fn encoded(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Stopping => 1,
            Self::Stopped => 2,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Stopping,
            _ => Self::Stopped,
        }
    }
}

/// Operation classification used at the Rust-owned stopping gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationClass {
    /// Controller, touch, settings, serial, or other side-effecting operation.
    MutatingResource,
    /// State reads and camera-frame acquisition allowed during cooperative stop.
    ReadOnly,
    /// Structured diagnostic output allowed until process exit.
    Diagnostic,
    /// Cooperative stop protocol itself.
    CooperativeStop,
}

/// Generation activation or operation-gate failure.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
pub enum GenerationError {
    /// Mutating work cannot begin once stopping starts.
    #[error("worker generation {generation} is stopping")]
    Stopping {
        /// Rejected generation.
        generation: u64,
    },
    /// No operations can begin after the process is reaped.
    #[error("worker generation {generation} has stopped")]
    Stopped {
        /// Rejected generation.
        generation: u64,
    },
    /// A stopped dynamic worker remains terminal for the application lifetime.
    #[error("dynamic worker generation cannot be regenerated during normal operation")]
    DynamicRestartForbidden,
    /// Script replacement must wait for the old OS process to be reaped.
    #[error("script worker replacement is blocked until the stopping generation is reaped")]
    ScriptStillStopping,
}

/// Shared gate and cancellation scope for one OS worker process.
#[derive(Debug)]
pub struct WorkerGeneration {
    id: GenerationId,
    kind: WorkerKind,
    phase: AtomicU8,
    cancellation: CancellationToken,
}

impl WorkerGeneration {
    fn new(id: GenerationId, kind: WorkerKind) -> Self {
        Self {
            id,
            kind,
            phase: AtomicU8::new(GenerationPhase::Running.encoded()),
            cancellation: CancellationToken::new(),
        }
    }

    /// Returns the generation identity.
    #[must_use]
    pub const fn id(&self) -> GenerationId {
        self.id
    }

    /// Returns the worker role.
    #[must_use]
    pub const fn kind(&self) -> WorkerKind {
        self.kind
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    pub fn phase(&self) -> GenerationPhase {
        GenerationPhase::decode(self.phase.load(Ordering::Acquire))
    }

    /// Returns the token cancelled when this generation begins stopping.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    /// Checks whether an operation may begin in the current phase.
    ///
    /// # Errors
    ///
    /// Rejects mutating work in `stopping` and all work after `stopped`.
    pub fn permit(&self, class: OperationClass) -> Result<(), GenerationError> {
        match self.phase() {
            GenerationPhase::Running => Ok(()),
            GenerationPhase::Stopping
                if matches!(
                    class,
                    OperationClass::ReadOnly
                        | OperationClass::Diagnostic
                        | OperationClass::CooperativeStop
                ) =>
            {
                Ok(())
            }
            GenerationPhase::Stopping => Err(GenerationError::Stopping {
                generation: self.id.get(),
            }),
            GenerationPhase::Stopped => Err(GenerationError::Stopped {
                generation: self.id.get(),
            }),
        }
    }

    /// Atomically enters `stopping` and cancels queued mutable operations.
    ///
    /// Returns `true` only for the first transition.
    pub fn begin_stopping(&self) -> bool {
        if self
            .phase
            .compare_exchange(
                GenerationPhase::Running.encoded(),
                GenerationPhase::Stopping.encoded(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.cancellation.cancel();
            true
        } else {
            false
        }
    }

    /// Marks the generation stopped after OS process reap.
    pub fn mark_stopped(&self) {
        self.phase
            .store(GenerationPhase::Stopped.encoded(), Ordering::Release);
        self.cancellation.cancel();
    }
}

/// Registry enforcing per-worker replacement rules.
#[derive(Debug, Default)]
pub struct GenerationManager {
    next_id: AtomicU64,
    current: Mutex<HashMap<WorkerKind, Arc<WorkerGeneration>>>,
}

impl GenerationManager {
    /// Creates an empty generation registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_current(&self) -> MutexGuard<'_, HashMap<WorkerKind, Arc<WorkerGeneration>>> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns the current generation for a role, including terminal dynamic
    /// generations retained to prevent accidental regeneration.
    #[must_use]
    pub fn current(&self, kind: WorkerKind) -> Option<Arc<WorkerGeneration>> {
        self.lock_current().get(&kind).cloned()
    }

    /// Returns an existing running generation or atomically activates a new one.
    ///
    /// Script generations may be replaced only after OS reap. Dynamic
    /// generations are never replaced during normal application operation.
    ///
    /// # Errors
    ///
    /// Returns an error for a stopping script generation or any attempted
    /// dynamic-worker regeneration.
    pub fn activate(&self, kind: WorkerKind) -> Result<Arc<WorkerGeneration>, GenerationError> {
        let mut current = self.lock_current();
        if let Some(existing) = current.get(&kind) {
            match (kind, existing.phase()) {
                (_, GenerationPhase::Running) => return Ok(existing.clone()),
                (WorkerKind::Script, GenerationPhase::Stopping) => {
                    return Err(GenerationError::ScriptStillStopping);
                }
                (WorkerKind::Script, GenerationPhase::Stopped) => {}
                (WorkerKind::Dynamic, GenerationPhase::Stopping | GenerationPhase::Stopped) => {
                    return Err(GenerationError::DynamicRestartForbidden);
                }
            }
        }
        let id = GenerationId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let generation = Arc::new(WorkerGeneration::new(id, kind));
        current.insert(kind, generation.clone());
        Ok(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::{GenerationError, GenerationManager, GenerationPhase, OperationClass};
    use crate::worker::WorkerKind;

    #[test]
    fn stopping_cancels_and_rejects_mutating_operations() {
        let manager = GenerationManager::new();
        let generation = manager.activate(WorkerKind::Script).unwrap();
        let cancellation = generation.cancellation_token();
        assert!(generation.begin_stopping());
        assert!(cancellation.is_cancelled());
        assert!(matches!(
            generation.permit(OperationClass::MutatingResource),
            Err(GenerationError::Stopping { .. })
        ));
        assert!(generation.permit(OperationClass::ReadOnly).is_ok());
        assert!(generation.permit(OperationClass::Diagnostic).is_ok());
    }

    #[test]
    fn script_replacement_waits_for_reap_but_dynamic_never_regenerates() {
        let manager = GenerationManager::new();
        let script = manager.activate(WorkerKind::Script).unwrap();
        script.begin_stopping();
        assert_eq!(
            manager.activate(WorkerKind::Script).unwrap_err(),
            GenerationError::ScriptStillStopping
        );
        script.mark_stopped();
        let replacement = manager.activate(WorkerKind::Script).unwrap();
        assert_ne!(script.id(), replacement.id());

        let dynamic = manager.activate(WorkerKind::Dynamic).unwrap();
        dynamic.begin_stopping();
        dynamic.mark_stopped();
        assert_eq!(dynamic.phase(), GenerationPhase::Stopped);
        assert_eq!(
            manager.activate(WorkerKind::Dynamic).unwrap_err(),
            GenerationError::DynamicRestartForbidden
        );
    }
}
