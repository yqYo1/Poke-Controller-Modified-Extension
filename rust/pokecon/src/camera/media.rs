use std::sync::Arc;

use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, ImageEncoder as _};
use tokio::sync::watch;

use crate::camera::frame::BgrFrame;
use crate::camera::screenshot::{ScreenshotError, ScreenshotRuntimeSettings};

/// One in-process mirror of a successfully published shared-memory frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaFrame {
    pub frame_sequence: u64,
    pub frame: Arc<BgrFrame>,
}

/// Writer-owned latest-frame broadcaster. This is deliberately separate from
/// the single-reader shared-memory pin protocol.
#[derive(Clone, Debug)]
pub struct LatestFrameSource {
    sender: watch::Sender<Option<Arc<MediaFrame>>>,
}

impl LatestFrameSource {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = watch::channel(None);
        Self { sender }
    }

    /// Mirrors a frame only after its shared-memory publication succeeds.
    pub fn publish(&self, frame_sequence: u64, frame: BgrFrame) {
        self.sender.send_replace(Some(Arc::new(MediaFrame {
            frame_sequence,
            frame: Arc::new(frame),
        })));
    }

    /// Invalidates all main-process media consumers when capture closes.
    pub fn clear(&self) {
        self.sender.send_replace(None);
    }

    #[must_use]
    pub fn latest(&self) -> Option<Arc<MediaFrame>> {
        self.sender.borrow().clone()
    }

    #[must_use]
    pub fn webrtc(&self) -> WebRtcFrameSource {
        WebRtcFrameSource {
            receiver: self.sender.subscribe(),
        }
    }

    #[must_use]
    pub fn motion_jpeg(&self, settings: ScreenshotRuntimeSettings) -> MotionJpegSource {
        MotionJpegSource {
            receiver: self.sender.subscribe(),
            settings,
        }
    }
}

impl Default for LatestFrameSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw frame stream handed to the WebRTC track implementation.
#[derive(Clone, Debug)]
pub struct WebRtcFrameSource {
    receiver: watch::Receiver<Option<Arc<MediaFrame>>>,
}

impl WebRtcFrameSource {
    #[must_use]
    pub fn latest(&self) -> Option<Arc<MediaFrame>> {
        self.receiver.borrow().clone()
    }

    /// Waits for publication or invalidation.
    ///
    /// # Errors
    ///
    /// Returns when the owning camera source has been dropped.
    pub async fn changed(&mut self) -> Result<Option<Arc<MediaFrame>>, MediaSourceError> {
        self.receiver
            .changed()
            .await
            .map_err(|_| MediaSourceError::Closed)?;
        Ok(self.receiver.borrow_and_update().clone())
    }
}

/// Independently encoded Motion JPEG fallback source.
#[derive(Clone, Debug)]
pub struct MotionJpegSource {
    receiver: watch::Receiver<Option<Arc<MediaFrame>>>,
    settings: ScreenshotRuntimeSettings,
}

impl MotionJpegSource {
    /// Waits for the next raw source change without encoding on the async
    /// executor. Callers may hand the returned frame to a blocking encoder.
    ///
    /// # Errors
    ///
    /// Returns when the owning camera source has been dropped.
    pub async fn changed(&mut self) -> Result<Option<Arc<MediaFrame>>, MediaSourceError> {
        self.receiver
            .changed()
            .await
            .map_err(|_| MediaSourceError::Closed)?;
        Ok(self.receiver.borrow_and_update().clone())
    }

    /// Encodes a specific source snapshot using the current shared quality.
    ///
    /// # Errors
    ///
    /// Returns a fixed encoding or quality failure.
    pub fn encode_jpeg(&self, snapshot: &MediaFrame) -> Result<EncodedMotionJpeg, ScreenshotError> {
        encode_motion_jpeg(snapshot, self.settings.jpeg_quality())
    }

    /// Encodes the latest frame using the current shared JPEG quality.
    ///
    /// # Errors
    ///
    /// Returns no-frame or fixed encoding failures.
    pub fn latest_jpeg(&self) -> Result<EncodedMotionJpeg, ScreenshotError> {
        let snapshot = self
            .receiver
            .borrow()
            .clone()
            .ok_or(ScreenshotError::NoPublishedFrame)?;
        self.encode_jpeg(&snapshot)
    }

    /// Waits for the next source change and encodes it when still valid.
    ///
    /// # Errors
    ///
    /// Returns a closed, no-frame, or encoding failure.
    pub async fn changed_jpeg(&mut self) -> Result<EncodedMotionJpeg, MediaSourceError> {
        let snapshot = self.changed().await?.ok_or(MediaSourceError::NoFrame)?;
        self.encode_jpeg(&snapshot)
            .map_err(|_| MediaSourceError::EncodingFailed)
    }
}

/// One complete JPEG fallback frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedMotionJpeg {
    pub frame_sequence: u64,
    pub bytes: Vec<u8>,
}

fn encode_motion_jpeg(
    snapshot: &MediaFrame,
    quality: u8,
) -> Result<EncodedMotionJpeg, ScreenshotError> {
    if quality == 0 || quality > 100 {
        return Err(ScreenshotError::InvalidJpegQuality);
    }
    let mut rgb = snapshot.frame.pixels().to_vec();
    for pixel in rgb.chunks_exact_mut(3) {
        pixel.swap(0, 2);
    }
    let size = snapshot.frame.size();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .write_image(&rgb, size.width(), size.height(), ColorType::Rgb8.into())
        .map_err(|_| ScreenshotError::EncodingFailed)?;
    Ok(EncodedMotionJpeg {
        frame_sequence: snapshot.frame_sequence,
        bytes,
    })
}

/// Media subscription failure.
#[derive(Clone, Copy, Debug, thiserror::Error, Eq, PartialEq)]
pub enum MediaSourceError {
    #[error("camera media source is closed")]
    Closed,
    #[error("camera media source has no current frame")]
    NoFrame,
    #[error("Motion JPEG encoding failed")]
    EncodingFailed,
}

#[cfg(test)]
mod tests {
    use image::GenericImageView as _;

    use super::LatestFrameSource;
    use crate::camera::frame::{BgrFrame, CaptureResolution};
    use crate::camera::screenshot::ScreenshotRuntimeSettings;

    #[tokio::test]
    async fn webrtc_raw_and_motion_jpeg_sources_follow_resolution_without_reconnect() {
        let source = LatestFrameSource::new();
        let mut webrtc = source.webrtc();
        let mut mjpeg = source.motion_jpeg(ScreenshotRuntimeSettings::default());
        source.publish(
            7,
            BgrFrame::solid(CaptureResolution::R640x360, [10, 20, 30]),
        );
        let raw = webrtc.changed().await.unwrap().unwrap();
        let encoded = mjpeg.changed_jpeg().await.unwrap();
        assert_eq!(raw.frame_sequence, 7);
        assert_eq!(raw.frame.pixels()[..3], [10, 20, 30]);
        assert_eq!(encoded.frame_sequence, 7);
        assert_eq!(
            image::load_from_memory(&encoded.bytes)
                .unwrap()
                .dimensions(),
            (640, 360)
        );

        source.publish(8, BgrFrame::solid(CaptureResolution::R1280x720, [1, 2, 3]));
        assert_eq!(
            webrtc.changed().await.unwrap().unwrap().frame.size(),
            CaptureResolution::R1280x720.size()
        );
        assert_eq!(
            image::load_from_memory(&mjpeg.changed_jpeg().await.unwrap().bytes)
                .unwrap()
                .dimensions(),
            (1280, 720)
        );
        source.clear();
        assert!(webrtc.changed().await.unwrap().is_none());
    }
}
