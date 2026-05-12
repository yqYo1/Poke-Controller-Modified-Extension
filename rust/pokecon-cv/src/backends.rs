// Camera backend types used conditionally via cfg-gated modules.
// Top-level imports are for the `list_cameras` stub and re-exports.

#[cfg(feature = "v4l")]
mod v4l_impl {
    use tracing;
    use v4l::buffer::Type;
    use v4l::io::traits::CaptureStream;
    use v4l::prelude::*;
    use v4l::video::Capture;
    use v4l::video::capture::Parameters;

    use crate::camera::{CameraBackend, CameraConfig, CameraError, Frame, PixelFormat};

    /// Wraps a V4L2 Device and its associated MmapStream together.
    ///
    /// `MmapStream` internally holds an `Arc<Handle>` cloned from the `Device`,
    /// so the stream's mmap'd buffers remain valid independently of the `Device`.
    /// The lifetime parameter in `MmapStream<'a>` is only a phantom marker for
    /// the mmap'd buffer regions — it does not actually borrow from the `Device`
    /// struct. By owning both in a single struct, we enforce drop order
    /// (stream before device) at the type level, making the `'static` lifetime sound.
    struct CameraInner {
        /// Must be declared before `device` so it is dropped first
        /// (Rust drops fields in declaration order).
        stream: v4l::io::mmap::Stream<'static>,
        device: Device,
    }

    impl CameraInner {
        fn new(device: Device, stream: v4l::io::mmap::Stream<'_>) -> Self {
            // Safety: MmapStream does not actually borrow from the Device —
            // it only holds an Arc<Handle> which keeps the V4L2 file descriptor
            // alive. The mmap'd buffers are backed by this fd and remain valid
            // as long as the Handle (Arc) lives. Since Stream already owns its
            // own Arc<Handle>, the buffers survive the Device's lifetime.
            // By wrapping both in CameraInner with stream before device,
            // we guarantee stream is dropped first, which is always valid.
            let stream = unsafe {
                std::mem::transmute::<v4l::io::mmap::Stream<'_>, v4l::io::mmap::Stream<'static>>(
                    stream,
                )
            };
            Self { stream, device }
        }
    }

    /// Video4Linux2 camera backend for Linux.
    ///
    /// Uses the `v4l` crate to access camera devices via V4L2.
    /// Supports mmap-based streaming and negotiates the best pixel format.
    pub struct V4lCameraBackend {
        inner: Option<CameraInner>,
        config: Option<CameraConfig>,
        is_open: bool,
        negotiated_fourcc: Option<v4l::FourCC>,
    }

    impl V4lCameraBackend {
        pub fn new() -> Self {
            Self {
                inner: None,
                config: None,
                is_open: false,
                negotiated_fourcc: None,
            }
        }

        /// Try to set the best available pixel format on the device.
        fn negotiate_format(
            device: &Device,
            width: u32,
            height: u32,
        ) -> Result<v4l::FourCC, CameraError> {
            let preferred_formats = [
                v4l::FourCC::new(b"RGB3"),
                v4l::FourCC::new(b"MJPG"),
                v4l::FourCC::new(b"YUYV"),
                v4l::FourCC::new(b"GREY"),
            ];

            for pref in &preferred_formats {
                let fmt = v4l::Format::new(width, height, *pref);
                match device.set_format(&fmt) {
                    Ok(actual_fmt) => {
                        tracing::info!(
                            "V4L: negotiated format {:?} (actual {}x{})",
                            actual_fmt.fourcc,
                            actual_fmt.width,
                            actual_fmt.height,
                        );
                        return Ok(actual_fmt.fourcc);
                    }
                    Err(e) => {
                        tracing::debug!("V4L: format {:?} not available: {}", pref, e);
                        continue;
                    }
                }
            }

            Err(CameraError::OpenError(
                0,
                "No supported pixel format found on device".to_string(),
            ))
        }

        /// Convert a v4l buffer to RGB Frame.
        fn convert_frame(
            buf_data: &[u8],
            width: u32,
            height: u32,
            fourcc: v4l::FourCC,
        ) -> Result<Frame, CameraError> {
            let frame_size = (width * height) as usize;

            match fourcc.to_string().as_str() {
                "RGB3" => Ok(Frame {
                    width,
                    height,
                    data: buf_data.to_vec(),
                    format: PixelFormat::Rgb,
                }),
                "GREY" => {
                    let mut rgb = Vec::with_capacity(frame_size * 3);
                    for &g in buf_data.iter().take(frame_size) {
                        rgb.extend_from_slice(&[g, g, g]);
                    }
                    Ok(Frame {
                        width,
                        height,
                        data: rgb,
                        format: PixelFormat::Rgb,
                    })
                }
                "YUYV" => {
                    let mut rgb = Vec::with_capacity(frame_size * 3);
                    for chunk in buf_data.chunks(4) {
                        if chunk.len() < 4 {
                            break;
                        }
                        let y0 = chunk[0] as i32;
                        let u = chunk[1] as i32;
                        let y1 = chunk[2] as i32;
                        let v = chunk[3] as i32;

                        for &y in &[y0, y1] {
                            let c = y - 16;
                            let d = u - 128;
                            let e = v - 128;
                            let r = (298 * c + 409 * e + 128) >> 8;
                            let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
                            let b = (298 * c + 516 * d + 128) >> 8;
                            rgb.push(r.clamp(0, 255) as u8);
                            rgb.push(g.clamp(0, 255) as u8);
                            rgb.push(b.clamp(0, 255) as u8);
                        }
                    }
                    rgb.truncate(frame_size * 3);
                    Ok(Frame {
                        width,
                        height,
                        data: rgb,
                        format: PixelFormat::Rgb,
                    })
                }
                "MJPG" => {
                    let decoder = jpeg_decoder::Decoder::new(buf_data);
                    let mut reader = decoder;
                    let pixels = reader.decode().map_err(|e| {
                        CameraError::CaptureError(format!("MJPEG decode error: {:?}", e))
                    })?;
                    let info = reader
                        .info()
                        .ok_or_else(|| CameraError::CaptureError("No JPEG info".to_string()))?;
                    Ok(Frame {
                        width: info.width as u32,
                        height: info.height as u32,
                        data: pixels,
                        format: PixelFormat::Rgb,
                    })
                }
                _ => Err(CameraError::CaptureError(format!(
                    "Unsupported pixel format: {}",
                    fourcc
                ))),
            }
        }

        /// Try to set frame rate via V4L2 streaming parameters.
        fn set_fps(device: &Device, fps: u32) {
            let params = Parameters::with_fps(fps);
            if let Err(e) = device.set_params(&params) {
                tracing::warn!("V4L: could not set frame rate to {} fps: {}", fps, e);
            }
        }
    }

    impl Default for V4lCameraBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait::async_trait]
    impl CameraBackend for V4lCameraBackend {
        async fn open(&mut self, config: &CameraConfig) -> Result<(), CameraError> {
            let dev_index = config.device_index as usize;

            let device = Device::new(dev_index).map_err(|e| {
                CameraError::OpenError(
                    config.device_index,
                    format!("Failed to open /dev/video{}: {}", config.device_index, e),
                )
            })?;

            let fourcc = Self::negotiate_format(&device, config.width, config.height)?;
            Self::set_fps(&device, config.fps);

            let stream = MmapStream::with_buffers(&device, Type::VideoCapture, 4).map_err(|e| {
                CameraError::OpenError(
                    config.device_index,
                    format!("Failed to create stream: {}", e),
                )
            })?;

            self.inner = Some(CameraInner::new(device, stream));
            self.config = Some(config.clone());
            self.negotiated_fourcc = Some(fourcc);
            self.is_open = true;

            tracing::info!(
                "V4L camera opened: /dev/video{} ({}x{} @ {} fps, format: {:?})",
                config.device_index,
                config.width,
                config.height,
                config.fps,
                fourcc,
            );

            Ok(())
        }

        async fn capture(&mut self) -> Result<Frame, CameraError> {
            if !self.is_open {
                return Err(CameraError::NotInitialized);
            }

            let inner = self.inner.as_mut().ok_or(CameraError::CaptureError(
                "Stream not initialized".to_string(),
            ))?;

            let fourcc = self.negotiated_fourcc.ok_or(CameraError::CaptureError(
                "No format negotiated".to_string(),
            ))?;

            let config = self.config.as_ref().ok_or(CameraError::CaptureError(
                "No configuration set".to_string(),
            ))?;

            let (buf, _meta) = inner
                .stream
                .next()
                .map_err(|e| CameraError::CaptureError(format!("V4L capture error: {}", e)))?;

            Self::convert_frame(buf, config.width, config.height, fourcc)
        }

        async fn close(&mut self) {
            self.is_open = false;
            self.inner = None;
            self.config = None;
            self.negotiated_fourcc = None;
            tracing::info!("V4L camera closed");
        }

        fn is_open(&self) -> bool {
            self.is_open
        }
    }

    /// List available V4L2 camera devices by probing device indices.
    pub fn list_cameras() -> Vec<(i32, String)> {
        let mut cameras: Vec<(i32, String)> = Vec::new();

        for idx in 0..64 {
            match Device::new(idx) {
                Ok(device) => {
                    if let Ok(caps) = device.query_caps() {
                        if caps
                            .capabilities
                            .contains(v4l::capability::Flags::VIDEO_CAPTURE)
                        {
                            cameras.push((idx as i32, caps.card));
                        } else {
                            tracing::debug!("V4L: /dev/video{} is not a video capture device", idx);
                        }
                    } else {
                        cameras.push((idx as i32, format!("/dev/video{}", idx)));
                    }
                }
                Err(_) => {
                    // Stop probing after first miss if we already found cameras
                    if !cameras.is_empty() && idx > 4 {
                        break;
                    }
                }
            }
        }

        cameras
    }
}

// ── Re-exports ────────────────────────────────────────────────────────────────

#[cfg(feature = "v4l")]
pub use v4l_impl::*;

// ── Stub when v4l feature is disabled ─────────────────────────────────────────

#[cfg(not(feature = "v4l"))]
pub fn list_cameras() -> Vec<(i32, String)> {
    vec![(0, "Default Camera".to_string())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_cameras_no_panic() {
        let cams = list_cameras();
        assert!(cams.iter().all(|(idx, name)| *idx >= 0 && !name.is_empty()));
    }
}
