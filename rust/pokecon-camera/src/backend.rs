use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{BgrFrame, CameraDevice, CameraSelector, CaptureResolution};

/// Requested device, rate, and closed capture resolution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CameraConfig {
    selector: CameraSelector,
    requested_fps: u32,
    resolution: CaptureResolution,
}

impl CameraConfig {
    /// Creates a capture configuration.
    ///
    /// # Errors
    ///
    /// Rejects zero FPS.
    pub fn new(
        selector: CameraSelector,
        requested_fps: u32,
        resolution: CaptureResolution,
    ) -> Result<Self, CameraError> {
        if requested_fps == 0 {
            return Err(CameraError::InvalidFps);
        }
        Ok(Self {
            selector,
            requested_fps,
            resolution,
        })
    }

    #[must_use]
    pub const fn selector(&self) -> &CameraSelector {
        &self.selector
    }

    #[must_use]
    pub const fn requested_fps(&self) -> u32 {
        self.requested_fps
    }

    #[must_use]
    pub const fn resolution(&self) -> CaptureResolution {
        self.resolution
    }

    /// Returns a copy with new acquisition values.
    ///
    /// # Errors
    ///
    /// Rejects zero FPS.
    pub fn with_acquisition(
        &self,
        requested_fps: u32,
        resolution: CaptureResolution,
    ) -> Result<Self, CameraError> {
        Self::new(self.selector.clone(), requested_fps, resolution)
    }
}

/// Capture settings actually accepted by the source. Resolution is always
/// exact; effective FPS may be lower than requested.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EffectiveCameraConfig {
    requested: CameraConfig,
    effective_fps: u32,
}

impl EffectiveCameraConfig {
    /// Validates backend-reported effective settings.
    ///
    /// # Errors
    ///
    /// Rejects zero or higher-than-requested effective FPS.
    pub fn new(requested: CameraConfig, effective_fps: u32) -> Result<Self, CameraError> {
        if effective_fps == 0 || effective_fps > requested.requested_fps {
            return Err(CameraError::FpsRejected);
        }
        Ok(Self {
            requested,
            effective_fps,
        })
    }

    #[must_use]
    pub const fn requested(&self) -> &CameraConfig {
        &self.requested
    }

    #[must_use]
    pub const fn effective_fps(&self) -> u32 {
        self.effective_fps
    }
}

/// One open capture handle. Reconfiguration must operate on this same handle;
/// device replacement is owned by the manager transaction.
pub trait CameraSession: Send {
    #[must_use]
    fn effective_config(&self) -> &EffectiveCameraConfig;

    /// Applies FPS and resolution to this handle and reads the values back.
    ///
    /// # Errors
    ///
    /// Returns a fixed setting or device failure.
    fn reconfigure(
        &mut self,
        requested_fps: u32,
        resolution: CaptureResolution,
    ) -> Result<EffectiveCameraConfig, CameraError>;

    /// Reads and decodes one complete tightly packed BGR frame.
    ///
    /// # Errors
    ///
    /// Returns a fixed capture or decode failure.
    fn read_frame(&mut self) -> Result<BgrFrame, CameraError>;

    /// Stops capture and releases the native handle.
    fn close(&mut self);
}

/// Opens native or deterministic virtual camera sessions.
pub trait CameraBackend: Send + Sync {
    /// Re-enumerates devices and retains a missing configured selector.
    ///
    /// # Errors
    ///
    /// Returns a fixed enumeration failure.
    fn enumerate(
        &self,
        configured: Option<&CameraSelector>,
    ) -> Result<Vec<CameraDevice>, CameraError>;

    /// Opens exactly the requested raw selector without fallback.
    ///
    /// # Errors
    ///
    /// Returns a fixed open or acquisition-setting failure.
    fn open(&self, config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError>;
}

impl<T> CameraBackend for Arc<T>
where
    T: CameraBackend + ?Sized,
{
    fn enumerate(
        &self,
        configured: Option<&CameraSelector>,
    ) -> Result<Vec<CameraDevice>, CameraError> {
        (**self).enumerate(configured)
    }

    fn open(&self, config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        (**self).open(config)
    }
}

/// Fixed camera failures safe for APIs and logs without raw OS diagnostics.
#[derive(Clone, Copy, Debug, Deserialize, Error, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraError {
    #[error("camera FPS must be positive")]
    InvalidFps,
    #[error("camera settings contain an invalid normalized value")]
    InvalidSettingsValue,
    #[error("native camera enumeration failed")]
    EnumerationFailed,
    #[error("the requested camera could not be opened")]
    OpenFailed,
    #[error("camera acquisition settings could not be applied")]
    ApplyFailed,
    #[error("the camera rejected the exact requested resolution")]
    ResolutionRejected,
    #[error("the camera returned an invalid effective frame rate")]
    FpsRejected,
    #[error("a complete camera frame could not be read")]
    ReadFailed,
    #[error("camera frame decoding failed")]
    DecodeFailed,
    #[error("captured frame dimensions differ from the active transaction")]
    FrameSizeMismatch,
    #[error("camera replacement failed and the exact old camera was restored")]
    TransactionRolledBack,
    #[error("camera replacement and rollback restoration both failed")]
    RollbackFailed,
    #[error("camera command channel is closed")]
    CommandChannelClosed,
    #[error("camera is not open")]
    NotOpen,
    #[error("no valid camera frame is published")]
    NoPublishedFrame,
    #[error("camera shared-memory publication failed")]
    PublicationFailed,
    #[error("camera writer did not stop before the shutdown deadline")]
    ShutdownTimedOut,
}
