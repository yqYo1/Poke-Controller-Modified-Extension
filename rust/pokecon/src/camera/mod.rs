//! Camera capture, shared-frame publication, screenshots, and media sources.

pub mod backend;
pub mod frame;
pub mod manager;
pub mod media;
pub mod native;
pub mod screenshot;
pub mod selector;
pub mod shared_ring;
pub mod virtual_camera;

pub use backend::{CameraBackend, CameraConfig, CameraError, CameraSession, EffectiveCameraConfig};
pub use frame::{
    BgrFrame, CaptureResolution, FlipMode, FrameError, FrameSize, NormalizedRegion, PixelRegion,
};
pub use manager::{CameraManager, CameraRuntimeStatus, UnstoppedCameraWriter};
pub use media::{
    EncodedMotionJpeg, LatestFrameSource, MediaFrame, MediaSourceError, MotionJpegSource,
    WebRtcFrameSource,
};
pub use native::NativeCameraBackend;
pub use screenshot::{
    DownloadScreenshot, SavedScreenshot, ScreenshotClock, ScreenshotDestination, ScreenshotError,
    ScreenshotFormat, ScreenshotMode, ScreenshotRequest, ScreenshotResult,
    ScreenshotRuntimeSettings, ScreenshotService, SystemScreenshotClock,
};
pub use selector::{
    CameraDevice, CameraEnumerationError, CameraSelector, CameraSelectorError,
    WindowsCameraDescriptor, enumerate_native_cameras, select_windows_candidates,
};
pub use shared_ring::{
    DebugPinnedFrame, MappingDescriptor, Publication, RingError, RingReader, SLOT_BYTE_SIZE,
    SLOT_COUNT, SharedFrameRing,
};
pub use virtual_camera::{
    RecordedFrame, RecordedFrameSource, VirtualCameraBackend, VirtualOpenPlan,
    VirtualReconfigurePlan, VirtualSessionPlan,
};
