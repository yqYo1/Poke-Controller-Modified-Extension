use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::camera::backend::{
    CameraBackend, CameraConfig, CameraError, CameraSession, EffectiveCameraConfig,
};
use crate::camera::frame::{BgrFrame, FlipMode};
use crate::camera::media::LatestFrameSource;
use crate::camera::selector::{CameraDevice, CameraSelector};
use crate::camera::shared_ring::{MappingDescriptor, RingError, SharedFrameRing};

const CAMERA_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Public camera fields used by the canonical state snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraRuntimeStatus {
    pub camera_opened: bool,
    pub camera_fps: u32,
    pub camera_resolution: String,
    pub camera_device: CameraSelector,
    pub last_error: Option<CameraError>,
}

impl CameraRuntimeStatus {
    fn closed(config: &CameraConfig) -> Self {
        Self {
            camera_opened: false,
            camera_fps: config.requested_fps(),
            camera_resolution: config.resolution().as_str().to_owned(),
            camera_device: config.selector().clone(),
            last_error: None,
        }
    }

    fn opened(effective: &EffectiveCameraConfig, last_error: Option<CameraError>) -> Self {
        Self {
            camera_opened: true,
            camera_fps: effective.effective_fps(),
            camera_resolution: effective.requested().resolution().as_str().to_owned(),
            camera_device: effective.requested().selector().clone(),
            last_error,
        }
    }
}

enum WriterCommand {
    Apply {
        config: CameraConfig,
        response: SyncSender<Result<(), CameraError>>,
        cancelled: Arc<AtomicBool>,
    },
    Retry {
        response: SyncSender<Result<(), CameraError>>,
        cancelled: Arc<AtomicBool>,
    },
    Close {
        response: SyncSender<Result<(), CameraError>>,
        cancelled: Arc<AtomicBool>,
    },
    Shutdown,
}

struct WriterOwnership {
    join: Option<JoinHandle<()>>,
    stopped: Option<Receiver<()>>,
}

struct ManagerInner {
    backend: Arc<dyn CameraBackend>,
    ring: SharedFrameRing,
    commands: SyncSender<WriterCommand>,
    status: Arc<Mutex<CameraRuntimeStatus>>,
    status_events: watch::Sender<CameraRuntimeStatus>,
    flip: Arc<AtomicU8>,
    frames: LatestFrameSource,
    writer: Mutex<WriterOwnership>,
    /// Durable §15.6 `camera_writer_unstopped` state. Set once when
    /// [`CameraManager::shutdown`] cannot confirm writer termination before
    /// its deadline; never cleared, so steps 5/9 observe the timeout for the
    /// rest of the shutdown transaction.
    writer_unstopped: AtomicBool,
}

impl std::fmt::Debug for ManagerInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagerInner")
            .field("status", &self.lock_status())
            .field("published_token", &self.ring.published_token())
            .finish_non_exhaustive()
    }
}

impl ManagerInner {
    fn lock_status(&self) -> MutexGuard<'_, CameraRuntimeStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_writer(&self) -> MutexGuard<'_, WriterOwnership> {
        self.writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records the durable §15.6 `camera_writer_unstopped` state with a
    /// critical diagnostic. This path leaves `published_token`, slot state,
    /// and the shared mapping untouched; step 3 proceeds with the mapping
    /// retained.
    fn record_writer_unstopped(&self) {
        if !self.writer_unstopped.swap(true, Ordering::SeqCst) {
            tracing::error!(
                diagnostic_id = "CAMERA_WRITER_UNSTOPPED",
                "camera writer did not stop before its deadline; published token, slot state, and shared mapping left unchanged"
            );
        }
        if self.ring.unlink_name_for_shutdown().is_err() {
            tracing::error!(
                diagnostic_id = "CAMERA_MAPPING_UNLINK_FAILED",
                "camera writer fallback could not unlink the shared-memory name; retaining the existing mapping until process exit"
            );
        }
    }
}

/// Dedicated capture-thread manager. Every command is processed only between
/// complete frame reads/publications.
#[derive(Clone, Debug)]
pub struct CameraManager {
    inner: Arc<ManagerInner>,
}

impl CameraManager {
    /// Creates the lifetime-fixed ring and starts one capture writer. Failure to
    /// open the configured camera is reflected in status rather than preventing
    /// the rest of the application from starting.
    ///
    /// # Errors
    ///
    /// Returns an error only when mapping or writer-thread creation fails.
    pub fn start(
        backend: Arc<dyn CameraBackend>,
        config: CameraConfig,
        flip: FlipMode,
    ) -> Result<Self, CameraError> {
        let ring = SharedFrameRing::create(config.resolution())
            .map_err(|_| CameraError::PublicationFailed)?;
        let status = Arc::new(Mutex::new(CameraRuntimeStatus::closed(&config)));
        let (status_events, _status_receiver) =
            watch::channel(CameraRuntimeStatus::closed(&config));
        let flip = Arc::new(AtomicU8::new(encode_flip(flip)));
        let frames = LatestFrameSource::new();
        // A command that timed out at the caller must not remain in an
        // unbounded queue and retain its response channel indefinitely.
        let (commands, command_receiver) = sync_channel(1);
        let (stopped_sender, stopped) = std::sync::mpsc::channel();
        let (startup_sender, startup) = sync_channel(1);
        let writer_backend = backend.clone();
        let writer_ring = ring.clone();
        let writer_status = status.clone();
        let writer_status_events = status_events.clone();
        let writer_flip = flip.clone();
        let writer_frames = frames.clone();
        let join = std::thread::Builder::new()
            .name("pokecon-camera-writer".to_owned())
            .spawn(move || {
                // Some native capture handles are thread-affine. Construct the
                // state in the writer so no session ever crosses a thread.
                let writer = WriterState {
                    backend: writer_backend,
                    ring: writer_ring,
                    status: writer_status,
                    status_events: writer_status_events,
                    flip: writer_flip,
                    frames: writer_frames,
                    desired: config,
                    session: None,
                };
                writer_main(writer, &command_receiver, &startup_sender);
                let _ = stopped_sender.send(());
            })
            .map_err(|_| CameraError::CommandChannelClosed)?;
        let manager = Self {
            inner: Arc::new(ManagerInner {
                backend,
                ring,
                commands,
                status,
                status_events,
                flip,
                frames,
                writer: Mutex::new(WriterOwnership {
                    join: Some(join),
                    stopped: Some(stopped),
                }),
                writer_unstopped: AtomicBool::new(false),
            }),
        };
        // The inner result is intentionally status-only: camera-open failure
        // must not abort application startup. A native open/read that exceeds
        // the startup deadline also must not block the application forever; the
        // writer publishes the eventual status when it completes.
        let _ = startup.recv_timeout(CAMERA_COMMAND_TIMEOUT);
        Ok(manager)
    }

    #[must_use]
    pub fn mapping_descriptor(&self) -> MappingDescriptor {
        self.inner.ring.descriptor()
    }

    #[must_use]
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the shared-ring accessor is exercised by camera unit tests"
        )
    )]
    pub fn ring(&self) -> SharedFrameRing {
        self.inner.ring.clone()
    }

    #[must_use]
    pub fn frame_source(&self) -> LatestFrameSource {
        self.inner.frames.clone()
    }

    #[must_use]
    pub fn status(&self) -> CameraRuntimeStatus {
        self.inner.lock_status().clone()
    }

    pub fn subscribe_status(&self) -> watch::Receiver<CameraRuntimeStatus> {
        self.inner.status_events.subscribe()
    }

    /// Re-enumerates cameras and includes the current raw selector if missing.
    ///
    /// # Errors
    ///
    /// Returns a fixed native enumeration failure.
    pub fn enumerate(&self) -> Result<Vec<CameraDevice>, CameraError> {
        let selector = self.status().camera_device;
        self.inner.backend.enumerate(Some(&selector))
    }

    /// Applies a full validated acquisition tuple. An open camera uses the
    /// frame-boundary transaction; a closed camera only changes the next-open
    /// target.
    ///
    /// # Errors
    ///
    /// Distinguishes successful rollback from rollback failure.
    pub fn apply_config(&self, config: CameraConfig) -> Result<(), CameraError> {
        self.request(|response, cancelled| WriterCommand::Apply {
            config,
            response,
            cancelled,
        })
    }

    /// Reopens only the current raw selector when closed or errored. Calling
    /// this while open is a successful no-op.
    ///
    /// # Errors
    ///
    /// Returns the exact selector's open or first-frame failure.
    pub fn retry(&self) -> Result<(), CameraError> {
        self.request(|response, cancelled| WriterCommand::Retry {
            response,
            cancelled,
        })
    }

    /// Closes the active capture session while retaining the desired selector
    /// and lifetime-fixed shared mapping for a later explicit retry.
    ///
    /// # Errors
    ///
    /// Returns a fixed command-channel failure if the writer has stopped.
    pub fn close(&self) -> Result<(), CameraError> {
        self.request(|response, cancelled| WriterCommand::Close {
            response,
            cancelled,
        })
    }

    /// Updates live flip processing for the next complete frame.
    pub fn set_flip(&self, flip: FlipMode) {
        self.inner.flip.store(encode_flip(flip), Ordering::Release);
    }

    #[must_use]
    pub fn flip(&self) -> FlipMode {
        decode_flip(self.inner.flip.load(Ordering::Acquire))
    }

    /// Requests writer termination and waits only to the supplied deadline.
    /// A timed-out guard retains the mapping and join handle so an unresponsive
    /// native writer cannot cause an unsafe unmap. On timeout the durable
    /// §15.6 `camera_writer_unstopped` state (see [`CameraManager::writer_unstopped`])
    /// is recorded with a critical diagnostic, and `published_token`, slot
    /// state, and the shared mapping are left untouched.
    ///
    /// # Errors
    ///
    /// Returns [`UnstoppedCameraWriter`] when the writer remains alive.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), UnstoppedCameraWriter> {
        let mut ownership = self.inner.lock_writer();
        if ownership.join.is_none() && ownership.stopped.is_none() {
            return Ok(());
        }
        if let Err(TrySendError::Full(_)) = self.inner.commands.try_send(WriterCommand::Shutdown) {
            drop(ownership);
            self.inner.record_writer_unstopped();
            return Err(UnstoppedCameraWriter {
                manager: Arc::clone(&self.inner),
            });
        }
        let Some(stopped) = ownership.stopped.as_ref() else {
            drop(ownership);
            self.inner.record_writer_unstopped();
            return Err(UnstoppedCameraWriter {
                manager: Arc::clone(&self.inner),
            });
        };
        match stopped.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = ownership.stopped.take();
                let join = ownership.join.take();
                drop(ownership);
                if join.is_some_and(|join| join.join().is_err()) {
                    tracing::error!(
                        diagnostic_id = "CAMERA_WRITER_PANICKED",
                        "camera writer terminated unexpectedly"
                    );
                }
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => {
                drop(ownership);
                self.inner.record_writer_unstopped();
                Err(UnstoppedCameraWriter {
                    manager: Arc::clone(&self.inner),
                })
            }
        }
    }

    /// Durable §15.6 `camera_writer_unstopped` state for the shutdown
    /// transaction. `true` once [`CameraManager::shutdown`] has timed out;
    /// never reset, so steps 5/9 keep refusing shared-memory release.
    #[must_use]
    pub fn writer_unstopped(&self) -> bool {
        self.inner.writer_unstopped.load(Ordering::SeqCst)
    }

    fn request(
        &self,
        command: impl FnOnce(SyncSender<Result<(), CameraError>>, Arc<AtomicBool>) -> WriterCommand,
    ) -> Result<(), CameraError> {
        let (response, receiver) = sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.inner
            .commands
            .try_send(command(response, Arc::clone(&cancelled)))
            .map_err(|error| match error {
                TrySendError::Full(_) => CameraError::CommandTimedOut,
                TrySendError::Disconnected(_) => CameraError::CommandChannelClosed,
            })?;
        match receiver.recv_timeout(CAMERA_COMMAND_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Disconnected) => Err(CameraError::CommandChannelClosed),
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Release);
                Err(CameraError::CommandTimedOut)
            }
        }
    }
}

/// Ownership returned when a driver read prevents bounded shutdown (§15.6
/// steps 2/5/9 fallback).
///
/// Holding this guard (or any [`CameraManager`] clone over the same inner
/// state) retains the shared mapping and the writer join handle so an
/// unresponsive native writer cannot cause a write-after-unmap: POSIX unlinks
/// only the name while the existing mapping stays until process exit, and
/// Windows retains the mapping handle until process exit. This path never
/// claims normal unmap completion; OS process exit reclaims the mapping.
pub struct UnstoppedCameraWriter {
    manager: Arc<ManagerInner>,
}

impl std::fmt::Debug for UnstoppedCameraWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UnstoppedCameraWriter")
            .field("writer_finished", &self.is_finished())
            .field("mapping", &self.manager.ring.descriptor())
            .finish_non_exhaustive()
    }
}

impl UnstoppedCameraWriter {
    #[must_use]
    pub fn mapping_descriptor(&self) -> MappingDescriptor {
        self.manager.ring.descriptor()
    }

    /// Durable §15.6 `camera_writer_unstopped` state behind this guard.
    /// Always `true`: the guard exists only after [`CameraManager::shutdown`]
    /// recorded the timeout.
    #[must_use]
    pub fn writer_unstopped(&self) -> bool {
        self.manager.writer_unstopped.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.manager
            .lock_writer()
            .join
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }

    /// Waits without a deadline after an external driver/device recovery.
    #[allow(
        dead_code,
        reason = "callers may choose retention-only shutdown while this explicit blocking recovery API remains available"
    )]
    pub fn wait(self) {
        let mut ownership = self.manager.lock_writer();
        if let Some(stopped) = ownership.stopped.take() {
            let _ = stopped.recv();
        }
        if let Some(join) = ownership.join.take() {
            let _ = join.join();
        }
    }
}

struct WriterState {
    backend: Arc<dyn CameraBackend>,
    ring: SharedFrameRing,
    status: Arc<Mutex<CameraRuntimeStatus>>,
    status_events: watch::Sender<CameraRuntimeStatus>,
    flip: Arc<AtomicU8>,
    frames: LatestFrameSource,
    desired: CameraConfig,
    session: Option<Box<dyn CameraSession>>,
}

impl WriterState {
    fn startup(&mut self) -> Result<(), CameraError> {
        match self.open_and_validate(&self.desired.clone(), true) {
            Ok((session, effective)) => {
                self.session = Some(session);
                self.set_status(CameraRuntimeStatus::opened(&effective, None));
                Ok(())
            }
            Err(error) => {
                self.invalidate_publication();
                let mut status = CameraRuntimeStatus::closed(&self.desired);
                status.last_error = Some(error);
                self.set_status(status);
                Err(error)
            }
        }
    }

    fn capture_one(&mut self) {
        let result = self
            .session
            .as_mut()
            .map_or(Err(CameraError::NotOpen), |session| {
                let expected = session.effective_config().requested().resolution();
                let frame = session.read_frame()?;
                validate_frame(&frame, expected)?;
                Ok(frame)
            });
        match result {
            Ok(mut frame) => {
                frame.apply_flip(decode_flip(self.flip.load(Ordering::Acquire)));
                match self.ring.publish(&frame) {
                    Ok(publication) => self.frames.publish(publication.frame_sequence, frame),
                    Err(RingError::NoWritableSlot) => {}
                    Err(_) => self.fail_open_session(CameraError::PublicationFailed),
                }
            }
            Err(error) => self.fail_open_session(error),
        }
    }

    fn apply(&mut self, config: CameraConfig) -> Result<(), CameraError> {
        if self.session.is_none() {
            self.desired = config;
            let previous_error = self.lock_status().last_error;
            let mut status = CameraRuntimeStatus::closed(&self.desired);
            status.last_error = previous_error;
            self.set_status(status);
            return Ok(());
        }
        if config == self.desired {
            return Ok(());
        }
        if config.selector() == self.desired.selector() {
            self.reconfigure(config)
        } else {
            self.switch_device(config)
        }
    }

    fn retry(&mut self) -> Result<(), CameraError> {
        if self.session.is_some() {
            return Ok(());
        }
        match self.open_and_validate(&self.desired.clone(), true) {
            Ok((session, effective)) => {
                self.session = Some(session);
                self.set_status(CameraRuntimeStatus::opened(&effective, None));
                Ok(())
            }
            Err(error) => {
                let mut status = CameraRuntimeStatus::closed(&self.desired);
                status.last_error = Some(error);
                self.set_status(status);
                Err(error)
            }
        }
    }

    fn close(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.close();
        }
        self.invalidate_publication();
        self.set_status(CameraRuntimeStatus::closed(&self.desired));
    }

    fn switch_device(&mut self, replacement: CameraConfig) -> Result<(), CameraError> {
        let previous = self.desired.clone();
        if let Some(mut old_session) = self.session.take() {
            old_session.close();
        }
        match self.open_and_validate(&replacement, true) {
            Ok((session, effective)) => {
                self.desired = replacement;
                self.session = Some(session);
                self.set_status(CameraRuntimeStatus::opened(&effective, None));
                Ok(())
            }
            Err(_) => {
                if let Ok((session, effective)) = self.open_and_validate(&previous, false) {
                    self.session = Some(session);
                    self.set_status(CameraRuntimeStatus::opened(
                        &effective,
                        Some(CameraError::TransactionRolledBack),
                    ));
                    Err(CameraError::TransactionRolledBack)
                } else {
                    self.invalidate_publication();
                    let mut status = CameraRuntimeStatus::closed(&previous);
                    status.last_error = Some(CameraError::RollbackFailed);
                    self.set_status(status);
                    Err(CameraError::RollbackFailed)
                }
            }
        }
    }

    fn reconfigure(&mut self, replacement: CameraConfig) -> Result<(), CameraError> {
        let previous = self.desired.clone();
        let attempt = self
            .session
            .as_mut()
            .map_or(Err(CameraError::NotOpen), |session| {
                let effective =
                    session.reconfigure(replacement.requested_fps(), replacement.resolution())?;
                validate_effective(&effective, &replacement)?;
                let frame = session.read_frame()?;
                validate_frame(&frame, replacement.resolution())?;
                Ok((effective, frame))
            });
        if let Ok((effective, mut frame)) = attempt {
            frame.apply_flip(decode_flip(self.flip.load(Ordering::Acquire)));
            match self.ring.publish(&frame) {
                Ok(publication) => self.frames.publish(publication.frame_sequence, frame),
                Err(RingError::NoWritableSlot) => {
                    // The driver transaction succeeded. A transient reader
                    // lease must not cause a successful reconfigure to be
                    // rolled back; the next frame will publish normally.
                }
                Err(_) => return self.rollback_same_handle(&previous),
            }
            self.desired = replacement;
            self.set_status(CameraRuntimeStatus::opened(&effective, None));
            return Ok(());
        }
        self.rollback_same_handle(&previous)
    }

    fn rollback_same_handle(&mut self, previous: &CameraConfig) -> Result<(), CameraError> {
        let restored = self
            .session
            .as_mut()
            .map_or(Err(CameraError::NotOpen), |session| {
                let effective =
                    session.reconfigure(previous.requested_fps(), previous.resolution())?;
                validate_effective(&effective, previous)?;
                let frame = session.read_frame()?;
                validate_frame(&frame, previous.resolution())?;
                Ok(effective)
            });
        if let Ok(effective) = restored {
            self.set_status(CameraRuntimeStatus::opened(
                &effective,
                Some(CameraError::TransactionRolledBack),
            ));
            Err(CameraError::TransactionRolledBack)
        } else {
            if let Some(mut session) = self.session.take() {
                session.close();
            }
            self.invalidate_publication();
            let mut status = CameraRuntimeStatus::closed(previous);
            status.last_error = Some(CameraError::RollbackFailed);
            self.set_status(status);
            Err(CameraError::RollbackFailed)
        }
    }

    fn open_and_validate(
        &self,
        config: &CameraConfig,
        publish: bool,
    ) -> Result<(Box<dyn CameraSession>, EffectiveCameraConfig), CameraError> {
        let mut session = self.backend.open(config)?;
        let effective = session.effective_config().clone();
        validate_effective(&effective, config)?;
        let mut frame = session.read_frame()?;
        validate_frame(&frame, config.resolution())?;
        frame.apply_flip(decode_flip(self.flip.load(Ordering::Acquire)));
        if publish {
            let publication = self
                .ring
                .publish(&frame)
                .map_err(|_| CameraError::PublicationFailed)?;
            self.frames.publish(publication.frame_sequence, frame);
        }
        Ok((session, effective))
    }

    fn fail_open_session(&mut self, error: CameraError) {
        if let Some(mut session) = self.session.take() {
            session.close();
        }
        self.invalidate_publication();
        let mut status = CameraRuntimeStatus::closed(&self.desired);
        status.last_error = Some(error);
        self.set_status(status);
    }

    fn shutdown(&mut self) {
        self.close();
    }

    fn invalidate_publication(&self) {
        self.frames.clear();
        if self.ring.stop_publication(true).is_err() {
            tracing::error!(
                diagnostic_id = "CAMERA_PUBLICATION_INVALIDATE_FAILED",
                "camera publication could not be invalidated after writer stop"
            );
        }
    }

    fn lock_status(&self) -> MutexGuard<'_, CameraRuntimeStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_status(&self, status: CameraRuntimeStatus) {
        *self.lock_status() = status.clone();
        self.status_events.send_replace(status);
    }
}

fn writer_main(
    mut writer: WriterState,
    commands: &Receiver<WriterCommand>,
    startup: &SyncSender<Result<(), CameraError>>,
) {
    let startup_result = writer.startup();
    let _ = startup.send(startup_result);
    loop {
        let command = if writer.session.is_some() {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    writer.shutdown();
                    break;
                }
            }
        } else if let Ok(command) = commands.recv() {
            Some(command)
        } else {
            writer.shutdown();
            break;
        };
        if let Some(command) = command {
            match command {
                WriterCommand::Apply {
                    config,
                    response,
                    cancelled,
                } => {
                    let result = if cancelled.load(Ordering::Acquire) {
                        Err(CameraError::CommandTimedOut)
                    } else {
                        writer.apply(config)
                    };
                    let _ = response.send(result);
                }
                WriterCommand::Retry {
                    response,
                    cancelled,
                } => {
                    let result = if cancelled.load(Ordering::Acquire) {
                        Err(CameraError::CommandTimedOut)
                    } else {
                        writer.retry()
                    };
                    let _ = response.send(result);
                }
                WriterCommand::Close {
                    response,
                    cancelled,
                } => {
                    let result = if cancelled.load(Ordering::Acquire) {
                        Err(CameraError::CommandTimedOut)
                    } else {
                        writer.close();
                        Ok(())
                    };
                    let _ = response.send(result);
                }
                WriterCommand::Shutdown => {
                    writer.shutdown();
                    break;
                }
            }
        } else {
            writer.capture_one();
        }
    }
}

fn validate_effective(
    effective: &EffectiveCameraConfig,
    requested: &CameraConfig,
) -> Result<(), CameraError> {
    if effective.requested() != requested {
        return Err(CameraError::ApplyFailed);
    }
    EffectiveCameraConfig::new(requested.clone(), effective.effective_fps()).map(|_| ())
}

fn validate_frame(
    frame: &BgrFrame,
    resolution: crate::camera::CaptureResolution,
) -> Result<(), CameraError> {
    if frame.size() == resolution.size() {
        Ok(())
    } else {
        Err(CameraError::FrameSizeMismatch)
    }
}

const fn encode_flip(flip: FlipMode) -> u8 {
    match flip {
        FlipMode::None => 0,
        FlipMode::Vertical => 1,
        FlipMode::Horizontal => 2,
        FlipMode::Both => 3,
    }
}

const fn decode_flip(value: u8) -> FlipMode {
    match value {
        1 => FlipMode::Vertical,
        2 => FlipMode::Horizontal,
        3 => FlipMode::Both,
        _ => FlipMode::None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::time::Duration;

    use super::CameraManager;
    use crate::camera::backend::{CameraConfig, CameraError};
    use crate::camera::frame::{CaptureResolution, FlipMode};
    use crate::camera::selector::CameraSelector;
    use crate::camera::shared_ring::INVALID_PUBLISHED_TOKEN;
    #[cfg(unix)]
    use crate::camera::shared_ring::SharedFrameRing;
    use crate::camera::virtual_camera::{
        RecordedFrame, RecordedFrameSource, VirtualCameraBackend, VirtualOpenPlan,
        VirtualReconfigurePlan, VirtualSessionPlan,
    };

    fn config(selector: u32, fps: u32, resolution: CaptureResolution) -> CameraConfig {
        CameraConfig::new(CameraSelector::Index(selector), fps, resolution).unwrap()
    }

    fn open_plan(color: [u8; 3], fps: u32) -> VirtualOpenPlan {
        VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            fps,
            [RecordedFrame::Solid(color)],
        ))
    }

    #[test]
    fn startup_publishes_complete_frames_and_shutdown_invalidates_mapping() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(open_plan([1, 2, 3], 30));
        let manager = CameraManager::start(
            Arc::new(backend),
            config(0, 60, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        assert!(manager.status().camera_opened);
        assert_eq!(manager.status().camera_fps, 30);
        assert_eq!(
            manager.ring().read_published().unwrap().unwrap().pixels()[..3],
            [1, 2, 3]
        );
        let status_events = manager.subscribe_status();
        manager.close().unwrap();
        assert!(status_events.has_changed().unwrap());
        assert!(!status_events.borrow().camera_opened);
        manager.shutdown(Duration::from_secs(1)).unwrap();
        assert!(!manager.status().camera_opened);
        assert!(manager.ring().read_published().unwrap().is_none());
    }

    #[test]
    fn replacement_failure_reopens_only_the_exact_old_selector() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(open_plan([5, 6, 7], 30));
        backend.push_open(VirtualOpenPlan::Fail);
        backend.push_open(open_plan([5, 6, 7], 30));
        let manager = CameraManager::start(
            Arc::new(backend.clone()),
            config(4, 30, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        assert_eq!(
            manager.apply_config(config(9, 30, CaptureResolution::R640x360)),
            Err(CameraError::TransactionRolledBack)
        );
        assert_eq!(manager.status().camera_device, CameraSelector::Index(4));
        assert_eq!(
            backend
                .opened_configs()
                .iter()
                .map(|config| config.selector().clone())
                .collect::<Vec<_>>(),
            vec![
                CameraSelector::Index(4),
                CameraSelector::Index(9),
                CameraSelector::Index(4),
            ]
        );
        manager.shutdown(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn resolution_commit_preserves_pinned_old_slot_and_failure_rolls_back_handle() {
        let backend = VirtualCameraBackend::default();
        let session = VirtualSessionPlan {
            effective_fps: 30,
            frames: RecordedFrameSource::new([RecordedFrame::Solid([10, 20, 30])]),
            reconfigurations: VecDeque::from([
                VirtualReconfigurePlan::Accept { effective_fps: 24 },
                VirtualReconfigurePlan::Reject,
                VirtualReconfigurePlan::Accept { effective_fps: 24 },
            ]),
        };
        backend.push_open(VirtualOpenPlan::Success(session));
        let manager = CameraManager::start(
            Arc::new(backend.clone()),
            config(0, 30, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        let pinned = manager
            .ring()
            .pin_current_for_diagnostics()
            .unwrap()
            .unwrap();
        let old = pinned.copy_frame().unwrap();
        manager
            .apply_config(config(0, 25, CaptureResolution::R1280x720))
            .unwrap();
        assert_eq!(pinned.copy_frame().unwrap(), old);
        assert_eq!(manager.status().camera_resolution, "1280x720");
        assert_eq!(manager.status().camera_fps, 24);
        assert_eq!(
            manager.apply_config(config(0, 20, CaptureResolution::R1920x1080)),
            Err(CameraError::TransactionRolledBack)
        );
        assert_eq!(manager.status().camera_resolution, "1280x720");
        assert!(manager.status().camera_opened);
        assert_eq!(
            backend.reconfigured_values(),
            vec![
                (25, CaptureResolution::R1280x720),
                (20, CaptureResolution::R1920x1080),
                (25, CaptureResolution::R1280x720),
            ]
        );
        drop(pinned);
        manager.shutdown(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn hung_writer_returns_mapping_retention_guard_at_deadline() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([1, 1, 1]), RecordedFrame::Hang],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            config(0, 30, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(10));
        let guard = manager
            .shutdown(Duration::from_millis(20))
            .expect_err("hung native read must retain mapping");
        assert_eq!(guard.mapping_descriptor(), manager.mapping_descriptor());
        assert!(!guard.is_finished());
        drop(guard);
    }

    #[test]
    fn shutdown_timeout_records_durable_flag_and_preserves_publication() {
        let backend = VirtualCameraBackend::default();
        // The startup read publishes one solid frame; the next native read
        // hangs, so the writer is still alive when the deadline expires.
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([9, 8, 7]), RecordedFrame::Hang],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            config(0, 30, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        for _ in 0..1_000 {
            if manager.ring().read_published().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let token_before = manager.ring().published_token();
        assert_ne!(token_before, INVALID_PUBLISHED_TOKEN);
        assert!(!manager.writer_unstopped());
        // Let the writer block inside the hanging native read so the
        // shutdown deadline deterministically expires first.
        std::thread::sleep(Duration::from_millis(50));
        let guard = manager
            .shutdown(Duration::from_millis(20))
            .expect_err("hung native read must retain mapping");
        assert!(manager.writer_unstopped());
        assert!(guard.writer_unstopped());
        // §15.6 step 2: the timeout path changes neither the publication nor
        // the shared mapping.
        assert_eq!(manager.ring().published_token(), token_before);
        assert!(manager.ring().read_published().unwrap().is_some());
        assert_eq!(guard.mapping_descriptor(), manager.mapping_descriptor());
        // POSIX fallback removes only the name; the existing mapping remains
        // readable while the retained writer guard is alive.
        #[cfg(unix)]
        assert!(SharedFrameRing::open(manager.mapping_descriptor()).is_err());
        // The flag is durable: a second attempt still fails and still reports it.
        let retry = manager
            .shutdown(Duration::from_millis(5))
            .expect_err("writer is still hung");
        assert!(manager.writer_unstopped());
        assert!(retry.writer_unstopped());
        drop(guard);
        drop(retry);
    }

    #[test]
    fn successful_shutdown_leaves_unstopped_flag_clear() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(open_plan([1, 2, 3], 30));
        let manager = CameraManager::start(
            Arc::new(backend),
            config(0, 60, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        assert!(!manager.writer_unstopped());
        manager.shutdown(Duration::from_secs(1)).unwrap();
        assert!(!manager.writer_unstopped());
        assert_eq!(manager.ring().published_token(), INVALID_PUBLISHED_TOKEN);
    }

    #[test]
    fn writer_read_failure_closes_capture_and_invalidates_all_frame_sources() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [
                RecordedFrame::Solid([1, 2, 3]),
                RecordedFrame::Fail(CameraError::ReadFailed),
            ],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            config(0, 30, CaptureResolution::R640x360),
            FlipMode::None,
        )
        .unwrap();
        for _ in 0..100 {
            if !manager.status().camera_opened {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!manager.status().camera_opened);
        assert_eq!(manager.status().last_error, Some(CameraError::ReadFailed));
        assert!(manager.frame_source().latest().is_none());
        assert!(manager.ring().read_published().unwrap().is_none());
        manager.shutdown(Duration::from_secs(1)).unwrap();
    }
}
