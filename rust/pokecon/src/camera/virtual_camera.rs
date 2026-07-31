use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::camera::backend::{
    CameraBackend, CameraConfig, CameraError, CameraSession, EffectiveCameraConfig,
};
use crate::camera::frame::{BgrFrame, CaptureResolution};
use crate::camera::selector::{CameraDevice, CameraSelector};

/// One deterministic virtual frame read.
#[derive(Clone, Debug)]
pub enum RecordedFrame {
    Frame(BgrFrame),
    /// Generates a solid frame at the handle's currently active resolution.
    Solid([u8; 3]),
    Fail(CameraError),
    /// Blocks permanently to model a native driver that ignores shutdown.
    Hang,
}

/// Recorded source that repeats its final complete frame after the queue is
/// exhausted. This mirrors a finite fixture file feeding a live source.
#[derive(Clone, Debug)]
pub struct RecordedFrameSource {
    frames: VecDeque<RecordedFrame>,
    last_complete: Option<RecordedFrame>,
}

impl RecordedFrameSource {
    #[must_use]
    pub fn new(frames: impl IntoIterator<Item = RecordedFrame>) -> Self {
        Self {
            frames: frames.into_iter().collect(),
            last_complete: None,
        }
    }

    fn read(&mut self, resolution: CaptureResolution) -> Result<BgrFrame, CameraError> {
        let next = self
            .frames
            .pop_front()
            .or_else(|| self.last_complete.clone());
        match next {
            Some(RecordedFrame::Frame(frame)) => {
                self.last_complete = Some(RecordedFrame::Frame(frame.clone()));
                Ok(frame)
            }
            Some(RecordedFrame::Solid(color)) => {
                self.last_complete = Some(RecordedFrame::Solid(color));
                Ok(BgrFrame::solid(resolution, color))
            }
            Some(RecordedFrame::Fail(error)) => Err(error),
            Some(RecordedFrame::Hang) => loop {
                std::thread::park();
            },
            None => Err(CameraError::ReadFailed),
        }
    }
}

/// One same-handle acquisition-setting result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtualReconfigurePlan {
    Accept { effective_fps: u32 },
    Reject,
}

/// Complete plan installed into one successfully opened virtual handle.
#[derive(Clone, Debug)]
pub struct VirtualSessionPlan {
    pub effective_fps: u32,
    pub frames: RecordedFrameSource,
    pub reconfigurations: VecDeque<VirtualReconfigurePlan>,
}

impl VirtualSessionPlan {
    #[must_use]
    pub fn recorded(effective_fps: u32, frames: impl IntoIterator<Item = RecordedFrame>) -> Self {
        Self {
            effective_fps,
            frames: RecordedFrameSource::new(frames),
            reconfigurations: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn with_reconfigurations(
        mut self,
        plans: impl IntoIterator<Item = VirtualReconfigurePlan>,
    ) -> Self {
        self.reconfigurations = plans.into_iter().collect();
        self
    }
}

/// One virtual open attempt.
#[derive(Clone, Debug)]
pub enum VirtualOpenPlan {
    Success(VirtualSessionPlan),
    Fail,
}

#[derive(Debug, Default)]
struct VirtualBackendState {
    devices: Vec<CameraDevice>,
    opens: VecDeque<VirtualOpenPlan>,
    opened_configs: Vec<CameraConfig>,
    reconfigured_values: Vec<(u32, CaptureResolution)>,
}

/// Queue-driven camera backend for transactions, recorded frames, Windows
/// native-ID mocks, and shutdown stress tests.
#[derive(Clone, Debug, Default)]
pub struct VirtualCameraBackend {
    state: Arc<Mutex<VirtualBackendState>>,
}

impl VirtualCameraBackend {
    pub fn set_devices(&self, devices: Vec<CameraDevice>) {
        self.lock_state().devices = devices;
    }

    pub fn push_open(&self, plan: VirtualOpenPlan) {
        self.lock_state().opens.push_back(plan);
    }

    #[must_use]
    pub fn opened_configs(&self) -> Vec<CameraConfig> {
        self.lock_state().opened_configs.clone()
    }

    #[must_use]
    pub fn reconfigured_values(&self) -> Vec<(u32, CaptureResolution)> {
        self.lock_state().reconfigured_values.clone()
    }

    fn lock_state(&self) -> MutexGuard<'_, VirtualBackendState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl CameraBackend for VirtualCameraBackend {
    fn enumerate(
        &self,
        configured: Option<&CameraSelector>,
    ) -> Result<Vec<CameraDevice>, CameraError> {
        let mut devices = self.lock_state().devices.clone();
        if let Some(configured) = configured
            && !devices.iter().any(|device| &device.selector == configured)
        {
            devices.push(CameraDevice::unavailable(configured.clone()));
        }
        Ok(devices)
    }

    fn open(&self, config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        let plan = {
            let mut state = self.lock_state();
            state.opened_configs.push(config.clone());
            state.opens.pop_front()
        };
        match plan {
            Some(VirtualOpenPlan::Success(plan)) => {
                let effective = EffectiveCameraConfig::new(config.clone(), plan.effective_fps)?;
                Ok(Box::new(VirtualCameraSession {
                    effective,
                    source: plan.frames,
                    reconfigurations: plan.reconfigurations,
                    backend_state: self.state.clone(),
                    closed: false,
                }))
            }
            Some(VirtualOpenPlan::Fail) | None => Err(CameraError::OpenFailed),
        }
    }
}

#[derive(Debug)]
struct VirtualCameraSession {
    effective: EffectiveCameraConfig,
    source: RecordedFrameSource,
    reconfigurations: VecDeque<VirtualReconfigurePlan>,
    backend_state: Arc<Mutex<VirtualBackendState>>,
    closed: bool,
}

impl CameraSession for VirtualCameraSession {
    fn effective_config(&self) -> &EffectiveCameraConfig {
        &self.effective
    }

    fn reconfigure(
        &mut self,
        requested_fps: u32,
        resolution: CaptureResolution,
    ) -> Result<EffectiveCameraConfig, CameraError> {
        self.backend_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reconfigured_values
            .push((requested_fps, resolution));
        match self.reconfigurations.pop_front() {
            Some(VirtualReconfigurePlan::Accept { effective_fps }) => {
                let config = CameraConfig::new(
                    self.effective.requested().selector().clone(),
                    requested_fps,
                    resolution,
                )?;
                let effective = EffectiveCameraConfig::new(config, effective_fps)?;
                self.effective = effective.clone();
                Ok(effective)
            }
            Some(VirtualReconfigurePlan::Reject) | None => Err(CameraError::ApplyFailed),
        }
    }

    fn read_frame(&mut self) -> Result<BgrFrame, CameraError> {
        if self.closed {
            return Err(CameraError::NotOpen);
        }
        self.source.read(self.effective.requested().resolution())
    }

    fn close(&mut self) {
        self.closed = true;
    }
}
