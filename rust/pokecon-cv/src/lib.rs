pub mod camera;
pub mod image_processing;

pub use camera::{
    Camera, CameraBackend, CameraConfig, CameraError, Frame, MockCameraBackend, PixelFormat,
};
pub use image_processing::{
    BinarizationConfig, CropFormat, ImageError, ImageProcessor, MatchResult, MultiMatchResult,
    Point, PreprocessConfig, Region,
};
