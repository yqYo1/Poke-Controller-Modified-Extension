//! Camera capture, shared-frame publication, screenshots, and media sources.

pub mod backend;
pub mod frame;
pub mod manager;
pub mod media;
pub mod native;
pub mod screenshot;
pub mod selector;
pub mod shared_ring;
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "virtual camera fixtures are unit-test-only")
)]
pub mod virtual_camera;

pub use backend::{CameraBackend, CameraConfig, CameraError};
pub use frame::{BgrFrame, CaptureResolution, FlipMode, NormalizedRegion};
pub use manager::CameraManager;
pub use media::{LatestFrameSource, MediaFrame, MotionJpegSource, WebRtcFrameSource};
pub use native::NativeCameraBackend;
pub use screenshot::{
    ScreenshotDestination, ScreenshotError, ScreenshotFormat, ScreenshotMode, ScreenshotRequest,
    ScreenshotResult, ScreenshotRuntimeSettings, ScreenshotService,
};
pub use selector::{CameraDevice, CameraSelector};
pub use shared_ring::{MappingDescriptor, RingReader, SharedFrameRing};
