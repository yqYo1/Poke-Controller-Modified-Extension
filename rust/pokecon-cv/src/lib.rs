pub mod camera;
pub mod image_processing;

pub use camera::{
    Camera, CameraBackend, CameraConfig, CameraError, FlipMode, Frame, MockCameraBackend,
    PixelFormat, apply_flip,
};
pub use image_processing::{
    BinarizationConfig, CropFormat, ImageError, ImageProcessor, MatchResult, MultiMatchResult,
    Point, PreprocessConfig, Region,
};
