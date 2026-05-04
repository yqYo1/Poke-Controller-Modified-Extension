use std::sync::Arc;
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

#[derive(Debug, Clone)]
pub struct CameraConfig {
    pub device_index: i32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            device_index: 0,
            width: 1280,
            height: 720,
            fps: 30,
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
    backend: Arc<Mutex<Box<dyn CameraBackend>>>,
    config: CameraConfig,
}

impl Camera {
    pub fn new(backend: Box<dyn CameraBackend>) -> Self {
        Self {
            backend: Arc::new(Mutex::new(backend)),
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
        backend.capture().await
    }

    pub async fn close(&self) {
        let mut backend = self.backend.lock().await;
        backend.close().await;
    }

    pub fn config(&self) -> &CameraConfig {
        &self.config
    }
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
}
