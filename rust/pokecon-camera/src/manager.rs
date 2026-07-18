use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::backend::{
    CameraBackend, CameraConfig, CameraError, CameraSession, EffectiveCameraConfig,
};
use crate::frame::{BgrFrame, FlipMode};
use crate::media::LatestFrameSource;
use crate::selector::{CameraDevice, CameraSelector};
use crate::shared_ring::{MappingDescriptor, RingError, SharedFrameRing};

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
    },
    Retry {
        response: SyncSender<Result<(), CameraError>>,
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
    commands: Sender<WriterCommand>,
    status: Arc<Mutex<CameraRuntimeStatus>>,
    flip: Arc<AtomicU8>,
    frames: LatestFrameSource,
    writer: Mutex<WriterOwnership>,
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
        let flip = Arc::new(AtomicU8::new(encode_flip(flip)));
        let frames = LatestFrameSource::new();
        let (commands, command_receiver) = std::sync::mpsc::channel();
        let (stopped_sender, stopped) = std::sync::mpsc::channel();
        let (startup_sender, startup) = sync_channel(1);
        let writer_backend = backend.clone();
        let writer_ring = ring.clone();
        let writer_status = status.clone();
        let writer_flip = flip.clone();
        let writer_frames = frames.clone();
        let writer = WriterState {
            backend: writer_backend,
            ring: writer_ring,
            status: writer_status,
            flip: writer_flip,
            frames: writer_frames,
            desired: config,
            session: None,
        };
        let join = std::thread::Builder::new()
            .name("pokecon-camera-writer".to_owned())
            .spawn(move || {
                writer_main(writer, &command_receiver, &startup_sender);
                let _ = stopped_sender.send(());
            })
            .map_err(|_| CameraError::CommandChannelClosed)?;
        // The inner result is intentionally status-only: camera-open failure
        // must not abort application startup.
        let _startup_result = startup
            .recv()
            .map_err(|_| CameraError::CommandChannelClosed)?;
        Ok(Self {
            inner: Arc::new(ManagerInner {
                backend,
                ring,
                commands,
                status,
                flip,
                frames,
                writer: Mutex::new(WriterOwnership {
                    join: Some(join),
                    stopped: Some(stopped),
                }),
            }),
        })
    }

    #[must_use]
    pub fn mapping_descriptor(&self) -> MappingDescriptor {
        self.inner.ring.descriptor()
    }

    #[must_use]
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
        self.request(|response| WriterCommand::Apply { config, response })
    }

    /// Reopens only the current raw selector when closed or errored. Calling
    /// this while open is a successful no-op.
    ///
    /// # Errors
    ///
    /// Returns the exact selector's open or first-frame failure.
    pub fn retry(&self) -> Result<(), CameraError> {
        self.request(|response| WriterCommand::Retry { response })
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
    /// native writer cannot cause an unsafe unmap.
    ///
    /// # Errors
    ///
    /// Returns [`UnstoppedCameraWriter`] when the writer remains alive.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), UnstoppedCameraWriter> {
        let mut ownership = self.inner.lock_writer();
        let Some(join) = ownership.join.take() else {
            return Ok(());
        };
        let Some(stopped) = ownership.stopped.take() else {
            return Err(UnstoppedCameraWriter {
                ring: self.inner.ring.clone(),
                join: Some(join),
                stopped: None,
            });
        };
        let _ = self.inner.commands.send(WriterCommand::Shutdown);
        match stopped.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if join.join().is_err() {
                    tracing::error!(
                        diagnostic_id = "CAMERA_WRITER_PANICKED",
                        "camera writer terminated unexpectedly"
                    );
                }
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => Err(UnstoppedCameraWriter {
                ring: self.inner.ring.clone(),
                join: Some(join),
                stopped: Some(stopped),
            }),
        }
    }

    fn request(
        &self,
        command: impl FnOnce(SyncSender<Result<(), CameraError>>) -> WriterCommand,
    ) -> Result<(), CameraError> {
        let (response, receiver) = sync_channel(1);
        self.inner
            .commands
            .send(command(response))
            .map_err(|_| CameraError::CommandChannelClosed)?;
        receiver
            .recv()
            .map_err(|_| CameraError::CommandChannelClosed)?
    }
}

/// Ownership returned when a driver read prevents bounded shutdown.
pub struct UnstoppedCameraWriter {
    ring: SharedFrameRing,
    join: Option<JoinHandle<()>>,
    stopped: Option<Receiver<()>>,
}

impl std::fmt::Debug for UnstoppedCameraWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UnstoppedCameraWriter")
            .field("writer_finished", &self.is_finished())
            .field("mapping", &self.ring.descriptor())
            .finish_non_exhaustive()
    }
}

impl UnstoppedCameraWriter {
    #[must_use]
    pub fn mapping_descriptor(&self) -> MappingDescriptor {
        self.ring.descriptor()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits without a deadline after an external driver/device recovery.
    pub fn wait(mut self) {
        if let Some(stopped) = self.stopped.take() {
            let _ = stopped.recv();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct WriterState {
    backend: Arc<dyn CameraBackend>,
    ring: SharedFrameRing,
    status: Arc<Mutex<CameraRuntimeStatus>>,
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
            if let Ok(publication) = self.ring.publish(&frame) {
                self.frames.publish(publication.frame_sequence, frame);
                self.desired = replacement;
                self.set_status(CameraRuntimeStatus::opened(&effective, None));
                return Ok(());
            }
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
        if let Some(mut session) = self.session.take() {
            session.close();
        }
        self.invalidate_publication();
        self.set_status(CameraRuntimeStatus::closed(&self.desired));
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
        *self.lock_status() = status;
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
                WriterCommand::Apply { config, response } => {
                    let _ = response.send(writer.apply(config));
                }
                WriterCommand::Retry { response } => {
                    let _ = response.send(writer.retry());
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
    resolution: crate::CaptureResolution,
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
    use crate::backend::{CameraConfig, CameraError};
    use crate::frame::{CaptureResolution, FlipMode};
    use crate::selector::CameraSelector;
    use crate::virtual_camera::{
        RecordedFrame, VirtualCameraBackend, VirtualOpenPlan, VirtualReconfigurePlan,
        VirtualSessionPlan,
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
            frames: crate::RecordedFrameSource::new([RecordedFrame::Solid([10, 20, 30])]),
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
