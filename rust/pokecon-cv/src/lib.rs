pub mod backends;
pub mod camera;
pub mod image_processing;

#[cfg(feature = "v4l")]
pub use backends::V4lCameraBackend;
pub use backends::list_cameras;
pub use camera::{
    Camera, CameraBackend, CameraConfig, CameraError, FlipMode, Frame, MockCameraBackend,
    PixelFormat, apply_flip,
};
pub use image_processing::{
    BinarizationConfig, CropFormat, ImageError, ImageProcessor, MatchResult, MultiMatchResult,
    Point, PreprocessConfig, Region,
};
