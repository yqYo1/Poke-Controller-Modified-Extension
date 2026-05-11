use std::sync::Arc;

use tokio::sync::Mutex;

use crate::camera::{CameraBackend, CameraConfig, CameraError, Frame, PixelFormat};

/// OpenCV-based camera backend (cross-platform)
pub struct OpencvCameraBackend {
    // OpenCV VideoCapture is not Send/Safe, so we use a workaround
    // In practice, we use a separate thread for capture
    is_open: bool,
    config: Option<CameraConfig>,
}

impl OpencvCameraBackend {
    pub fn new() -> Self {
        Self {
            is_open: false,
            config: None,
        }
    }
}

impl Default for OpencvCameraBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CameraBackend for OpencvCameraBackend {
    async fn open(&mut self, config: &CameraConfig) -> Result<(), CameraError> {
        // For now, mark as open. Real implementation would use opencv crate
        // or call into Python's cv2 via PyO3
        self.is_open = true;
        self.config = Some(config.clone());
        Ok(())
    }

    async fn capture(&mut self) -> Result<Frame, CameraError> {
        if !self.is_open {
            return Err(CameraError::NotInitialized);
        }

        // Return a placeholder frame for now
        // Real implementation would capture from OpenCV
        let (width, height) = self
            .config
            .as_ref()
            .map(|c| (c.width, c.height))
            .unwrap_or((1280, 720));

        Ok(Frame {
            width,
            height,
            data: vec![128; (width * height * 3) as usize],
            format: PixelFormat::Rgb,
        })
    }

    async fn close(&mut self) {
        self.is_open = false;
        self.config = None;
    }

    fn is_open(&self) -> bool {
        self.is_open
    }
}

/// List available camera devices (placeholder)
pub fn list_cameras() -> Vec<(i32, String)> {
    // In a real implementation, this would enumerate /dev/video* devices
    // or use OpenCV to list available cameras
    vec![(0, "Default Camera".to_string())]
}
