use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum CameraError {
    #[error("Camera not initialized")]
    NotInitialized,
    #[error("Failed to open camera device {0}: {1}")]
    OpenError(i32, String),
    #[error("Capture error: {0}")]
    CaptureError(String),
    #[error("Invalid resolution: {0}x{1}")]
    InvalidResolution(u32, u32),
    #[error("Flip error: unsupported pixel format")]
    FlipUnsupportedFormat,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub format: PixelFormat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    Rgba,
    Gray,
}

impl PixelFormat {
    /// Number of channels (bytes per pixel) for this format.
    pub fn channels(&self) -> usize {
        match self {
            PixelFormat::Rgb | PixelFormat::Bgr => 3,
            PixelFormat::Rgba => 4,
            PixelFormat::Gray => 1,
        }
    }
}

/// Flip mode for camera image transformation.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlipMode {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

impl FlipMode {
    /// Parse a string to a flip mode.
    /// Accepts: "none", "horizontal", "vertical", "both" (case-insensitive).
    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "horizontal" => FlipMode::Horizontal,
            "vertical" => FlipMode::Vertical,
            "both" => FlipMode::Both,
            _ => FlipMode::None,
        }
    }

    /// Return the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            FlipMode::None => "none",
            FlipMode::Horizontal => "horizontal",
            FlipMode::Vertical => "vertical",
            FlipMode::Both => "both",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraConfig {
    pub device_index: i32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub flip: FlipMode,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            device_index: 0,
            width: 1280,
            height: 720,
            fps: 30,
            flip: FlipMode::None,
        }
    }
}

// ── Flip helper functions ───────────────────────────────────────────────────

/// Flip a frame horizontally (mirror along vertical axis).
fn flip_frame_horizontal(frame: &mut Frame) {
    let channels = frame.format.channels();
    let row_bytes = frame.width as usize * channels;
    for row in frame.data.chunks_mut(row_bytes) {
        for x in 0..frame.width as usize / 2 {
            let left = x * channels;
            let right = (frame.width as usize - 1 - x) * channels;
            for c in 0..channels {
                row.swap(left + c, right + c);
            }
        }
    }
}

/// Flip a frame vertically (mirror along horizontal axis).
fn flip_frame_vertical(frame: &mut Frame) {
    let channels = frame.format.channels();
    let row_bytes = frame.width as usize * channels;
    let half_height = frame.height as usize / 2;
    for y in 0..half_height {
        let top = y * row_bytes;
        let bottom = (frame.height as usize - 1 - y) * row_bytes;
        for offset in 0..row_bytes {
            frame.data.swap(top + offset, bottom + offset);
        }
    }
}

#[async_trait::async_trait]
pub trait CameraBackend: Send + Sync {
    async fn open(&mut self, config: &CameraConfig) -> Result<(), CameraError>;
    async fn capture(&mut self) -> Result<Frame, CameraError>;
    async fn close(&mut self);
    fn is_open(&self) -> bool;
}

pub struct Camera {
    backend: Mutex<Box<dyn CameraBackend>>,
    config: CameraConfig,
}

impl Camera {
    pub fn new(backend: Box<dyn CameraBackend>) -> Self {
        Self {
            backend: Mutex::new(backend),
            config: CameraConfig::default(),
        }
    }

    pub async fn open(&mut self, config: CameraConfig) -> Result<(), CameraError> {
        let mut backend = self.backend.lock().await;
        backend.open(&config).await?;
        self.config = config;
        Ok(())
    }

    pub async fn capture(&self) -> Result<Frame, CameraError> {
        let mut backend = self.backend.lock().await;
        let mut frame = backend.capture().await?;
        // Apply flip transformation based on current config
        apply_flip(&mut frame, self.config.flip)?;
        Ok(frame)
    }

    pub async fn close(&self) {
        let mut backend = self.backend.lock().await;
        backend.close().await;
    }

    pub fn config(&self) -> &CameraConfig {
        &self.config
    }

    /// Update the camera configuration in-place.
    pub fn update_config(&mut self, config: CameraConfig) {
        self.config = config;
    }

    /// Set the flip mode only (no backend restart needed).
    pub fn set_flip(&mut self, flip: FlipMode) {
        self.config.flip = flip;
    }
}

/// Apply a flip transformation to a frame based on the given mode.
pub fn apply_flip(frame: &mut Frame, flip: FlipMode) -> Result<(), CameraError> {
    match flip {
        FlipMode::None => {}
        FlipMode::Horizontal => flip_frame_horizontal(frame),
        FlipMode::Vertical => flip_frame_vertical(frame),
        FlipMode::Both => {
            flip_frame_horizontal(frame);
            flip_frame_vertical(frame);
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct MockCameraBackend {
    open_state: bool,
    mock_frame: Option<Frame>,
}

impl MockCameraBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_frame(mut self, frame: Frame) -> Self {
        self.mock_frame = Some(frame);
        self
    }
}

#[async_trait::async_trait]
impl CameraBackend for MockCameraBackend {
    async fn open(&mut self, _config: &CameraConfig) -> Result<(), CameraError> {
        self.open_state = true;
        Ok(())
    }

    async fn capture(&mut self) -> Result<Frame, CameraError> {
        if !self.open_state {
            return Err(CameraError::NotInitialized);
        }
        self.mock_frame
            .clone()
            .ok_or_else(|| CameraError::CaptureError("No mock frame set".to_string()))
    }

    async fn close(&mut self) {
        self.open_state = false;
    }

    fn is_open(&self) -> bool {
        self.open_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_frame(width: u32, height: u32, format: PixelFormat, value: u8) -> Frame {
        let channels = match format {
            PixelFormat::Rgb | PixelFormat::Bgr => 3,
            PixelFormat::Rgba => 4,
            PixelFormat::Gray => 1,
        };
        Frame {
            width,
            height,
            data: vec![value; (width * height) as usize * channels],
            format,
        }
    }

    #[tokio::test]
    async fn test_mock_camera() {
        let backend = MockCameraBackend::new().with_frame(Frame {
            width: 100,
            height: 100,
            data: vec![0; 300],
            format: PixelFormat::Rgb,
        });
        let mut camera = Camera::new(Box::new(backend));

        camera.open(CameraConfig::default()).await.unwrap();
        let frame = camera.capture().await.unwrap();
        assert_eq!(frame.width, 100);
        assert_eq!(frame.height, 100);

        camera.close().await;
    }

    #[tokio::test]
    async fn test_capture_without_open() {
        let backend = MockCameraBackend::new();
        let camera = Camera::new(Box::new(backend));

        let result = camera.capture().await;
        assert!(matches!(result, Err(CameraError::NotInitialized)));
    }

    #[test]
    fn test_flip_horizontal_rgb() {
        // 2x2 RGB frame with distinct pixel values
        // Pixels: [R0,G0,B0, R1,G1,B1, R2,G2,B2, R3,G3,B3]
        // Row 0: (10,20,30), (40,50,60)
        // Row 1: (70,80,90), (100,110,120)
        let mut frame = Frame {
            width: 2,
            height: 2,
            data: vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
            format: PixelFormat::Rgb,
        };
        let original = frame.data.clone();
        flip_frame_horizontal(&mut frame);
        // After horizontal flip: each row is mirrored
        // Row 0: (40,50,60), (10,20,30)
        // Row 1: (100,110,120), (70,80,90)
        assert_eq!(frame.data[0..3], [40, 50, 60]);
        assert_eq!(frame.data[3..6], [10, 20, 30]);
        assert_eq!(frame.data[6..9], [100, 110, 120]);
        assert_eq!(frame.data[9..12], [70, 80, 90]);
        // Ensure original data unchanged reference
        assert_eq!(original[0..3], [10, 20, 30]);
    }

    #[test]
    fn test_flip_vertical_rgb() {
        let mut frame = Frame {
            width: 2,
            height: 2,
            data: vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
            format: PixelFormat::Rgb,
        };
        flip_frame_vertical(&mut frame);
        // After vertical flip: rows are swapped
        // Row 0: (70,80,90), (100,110,120)
        // Row 1: (10,20,30), (40,50,60)
        assert_eq!(frame.data[0..3], [70, 80, 90]);
        assert_eq!(frame.data[3..6], [100, 110, 120]);
        assert_eq!(frame.data[6..9], [10, 20, 30]);
        assert_eq!(frame.data[9..12], [40, 50, 60]);
    }

    #[test]
    fn test_flip_both_rgb() {
        let mut frame = Frame {
            width: 2,
            height: 2,
            data: vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
            format: PixelFormat::Rgb,
        };
        flip_frame_horizontal(&mut frame);
        flip_frame_vertical(&mut frame);
        // Both = rotate 180 degrees
        // Original bottom-right becomes top-left
        assert_eq!(frame.data[0..3], [100, 110, 120]);
        assert_eq!(frame.data[3..6], [70, 80, 90]);
        assert_eq!(frame.data[6..9], [40, 50, 60]);
        assert_eq!(frame.data[9..12], [10, 20, 30]);
    }

    #[test]
    fn test_flip_none() {
        let mut frame = create_test_frame(4, 4, PixelFormat::Rgb, 100);
        let original = frame.data.clone();
        apply_flip(&mut frame, FlipMode::None).unwrap();
        assert_eq!(frame.data, original);
    }

    #[test]
    fn test_flip_horizontal_gray() {
        let mut frame = Frame {
            width: 3,
            height: 1,
            data: vec![10, 20, 30],
            format: PixelFormat::Gray,
        };
        flip_frame_horizontal(&mut frame);
        assert_eq!(frame.data, vec![30, 20, 10]);
    }

    #[tokio::test]
    async fn test_set_flip() {
        let backend = MockCameraBackend::new().with_frame(Frame {
            width: 4,
            height: 4,
            data: vec![100; 48], // 4*4*3 = 48 (RGB)
            format: PixelFormat::Rgb,
        });
        let mut camera = Camera::new(Box::new(backend));
        camera.open(CameraConfig::default()).await.unwrap();
        assert_eq!(camera.config().flip, FlipMode::None);
        camera.set_flip(FlipMode::Horizontal);
        assert_eq!(camera.config().flip, FlipMode::Horizontal);
    }

    #[test]
    fn test_update_config() {
        let backend = MockCameraBackend::new();
        let mut camera = Camera::new(Box::new(backend));
        let new_config = CameraConfig {
            device_index: 1,
            width: 640,
            height: 480,
            fps: 60,
            flip: FlipMode::Vertical,
        };
        camera.update_config(new_config.clone());
        assert_eq!(camera.config().width, 640);
        assert_eq!(camera.config().flip, FlipMode::Vertical);
    }

    #[test]
    fn test_flip_mode_parse_str() {
        assert_eq!(FlipMode::parse_str("none"), FlipMode::None);
        assert_eq!(FlipMode::parse_str("horizontal"), FlipMode::Horizontal);
        assert_eq!(FlipMode::parse_str("vertical"), FlipMode::Vertical);
        assert_eq!(FlipMode::parse_str("both"), FlipMode::Both);
        assert_eq!(FlipMode::parse_str("HORIZONTAL"), FlipMode::Horizontal);
        assert_eq!(FlipMode::parse_str("unknown"), FlipMode::None);
    }

    #[test]
    fn test_flip_mode_as_str() {
        assert_eq!(FlipMode::None.as_str(), "none");
        assert_eq!(FlipMode::Horizontal.as_str(), "horizontal");
        assert_eq!(FlipMode::Vertical.as_str(), "vertical");
        assert_eq!(FlipMode::Both.as_str(), "both");
    }

    #[test]
    fn test_pixel_format_channels() {
        assert_eq!(PixelFormat::Rgb.channels(), 3);
        assert_eq!(PixelFormat::Bgr.channels(), 3);
        assert_eq!(PixelFormat::Rgba.channels(), 4);
        assert_eq!(PixelFormat::Gray.channels(), 1);
    }

    #[tokio::test]
    async fn test_capture_with_flip() {
        // Create a frame with distinct rows to verify flip in capture
        let frame_data = vec![10, 10, 10, 20, 20, 20, 30, 30, 30];

        let frame = Frame {
            width: 1,
            height: 3,
            data: frame_data,
            format: PixelFormat::Rgb,
        };

        let backend = MockCameraBackend::new().with_frame(frame);
        let mut camera = Camera::new(Box::new(backend));
        camera.open(CameraConfig::default()).await.unwrap();

        // No flip
        let f = camera.capture().await.unwrap();
        assert_eq!(f.data[0..3], [10, 10, 10]);

        // With vertical flip
        camera.set_flip(FlipMode::Vertical);
        let f = camera.capture().await.unwrap();
        // After vertical flip, row 0 should be row 2
        assert_eq!(f.data[0..3], [30, 30, 30]);
    }
}
