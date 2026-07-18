use crate::backend::{CameraBackend, CameraConfig, CameraError, CameraSession};
use crate::selector::{CameraDevice, CameraSelector, enumerate_native_cameras};

/// Platform-native V4L2 or Media Foundation camera backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeCameraBackend;

impl CameraBackend for NativeCameraBackend {
    fn enumerate(
        &self,
        configured: Option<&CameraSelector>,
    ) -> Result<Vec<CameraDevice>, CameraError> {
        enumerate_native_cameras(configured).map_err(|_| CameraError::EnumerationFailed)
    }

    fn open(&self, config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        platform::open(config)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    use nokhwa::Buffer;
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{FrameFormat, Resolution};
    use v4l::buffer::Type;
    use v4l::capability::Flags;
    use v4l::io::traits::CaptureStream as _;
    use v4l::io::userptr::Stream as UserptrStream;
    use v4l::video::Capture as _;
    use v4l::video::capture::Parameters;
    use v4l::{Device, Format, FourCC};

    use crate::backend::{CameraConfig, CameraError, CameraSession, EffectiveCameraConfig};
    use crate::frame::{BgrFrame, CaptureResolution};
    use crate::selector::CameraSelector;

    const FRAME_TIMEOUT: Duration = Duration::from_secs(2);
    const FOURCC_PREFERENCE: [[u8; 4]; 6] =
        [*b"MJPG", *b"YUYV", *b"NV12", *b"RGB3", *b"BGR3", *b"GREY"];

    pub(super) fn open(config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        let device = match config.selector() {
            CameraSelector::Index(index) => {
                Device::new(usize::try_from(*index).map_err(|_| CameraError::OpenFailed)?)
            }
            CameraSelector::Native(path) => {
                if !Path::new(path).is_absolute() {
                    return Err(CameraError::OpenFailed);
                }
                Device::with_path(path)
            }
        }
        .map_err(|_| CameraError::OpenFailed)?;
        let capabilities = device.query_caps().map_err(|_| CameraError::OpenFailed)?;
        if !capabilities.capabilities.contains(Flags::VIDEO_CAPTURE) {
            return Err(CameraError::OpenFailed);
        }
        let placeholder = EffectiveCameraConfig::new(config.clone(), config.requested_fps())?;
        let mut session = LinuxCameraSession {
            device,
            stream: None,
            source_format: FrameFormat::MJPEG,
            effective: placeholder,
        };
        let effective = session.apply(config.requested_fps(), config.resolution())?;
        session.effective = effective;
        Ok(Box::new(session))
    }

    struct LinuxCameraSession {
        // Keep the device handle alongside the user-pointer stream. The stream
        // clones the same kernel handle and therefore has no self-reference.
        device: Device,
        stream: Option<UserptrStream>,
        source_format: FrameFormat,
        effective: EffectiveCameraConfig,
    }

    impl LinuxCameraSession {
        fn apply(
            &mut self,
            requested_fps: u32,
            resolution: CaptureResolution,
        ) -> Result<EffectiveCameraConfig, CameraError> {
            self.stop_stream();
            let requested_size = resolution.size();
            let supported = self
                .device
                .enum_formats()
                .map_err(|_| CameraError::ApplyFailed)?
                .into_iter()
                .map(|description| description.fourcc)
                .collect::<Vec<_>>();
            let mut accepted = None;
            for code in FOURCC_PREFERENCE {
                let fourcc = FourCC::new(&code);
                if !supported.contains(&fourcc) {
                    continue;
                }
                let Ok(actual) = self.device.set_format(&Format::new(
                    requested_size.width(),
                    requested_size.height(),
                    fourcc,
                )) else {
                    continue;
                };
                if actual.width != requested_size.width()
                    || actual.height != requested_size.height()
                {
                    continue;
                }
                if let Some(source_format) = frame_format(actual.fourcc) {
                    accepted = Some(source_format);
                    break;
                }
            }
            let source_format = accepted.ok_or(CameraError::ResolutionRejected)?;
            let parameters = self
                .device
                .set_params(&Parameters::with_fps(requested_fps))
                .map_err(|_| CameraError::ApplyFailed)?;
            let numerator = parameters.interval.numerator;
            let denominator = parameters.interval.denominator;
            let effective_fps = denominator
                .checked_div(numerator)
                .filter(|fps| *fps > 0 && *fps <= requested_fps)
                .ok_or(CameraError::FpsRejected)?;
            let readback = self.device.format().map_err(|_| CameraError::ApplyFailed)?;
            if readback.width != requested_size.width()
                || readback.height != requested_size.height()
            {
                return Err(CameraError::ResolutionRejected);
            }
            let mut stream = UserptrStream::with_buffers(&self.device, Type::VideoCapture, 4)
                .map_err(|_| CameraError::OpenFailed)?;
            stream.set_timeout(FRAME_TIMEOUT);
            self.stream = Some(stream);
            self.source_format = source_format;
            EffectiveCameraConfig::new(
                CameraConfig::new(
                    self.effective.requested().selector().clone(),
                    requested_fps,
                    resolution,
                )?,
                effective_fps,
            )
        }

        fn stop_stream(&mut self) {
            self.stream.take();
        }
    }

    impl CameraSession for LinuxCameraSession {
        fn effective_config(&self) -> &EffectiveCameraConfig {
            &self.effective
        }

        fn reconfigure(
            &mut self,
            requested_fps: u32,
            resolution: CaptureResolution,
        ) -> Result<EffectiveCameraConfig, CameraError> {
            let effective = self.apply(requested_fps, resolution)?;
            self.effective = effective.clone();
            Ok(effective)
        }

        fn read_frame(&mut self) -> Result<BgrFrame, CameraError> {
            let stream = self.stream.as_mut().ok_or(CameraError::NotOpen)?;
            let (bytes, _) = stream.next().map_err(|_| CameraError::ReadFailed)?;
            let size = self.effective.requested().resolution().size();
            let buffer = Buffer::new(
                Resolution::new(size.width(), size.height()),
                bytes,
                self.source_format,
            );
            let mut pixels = buffer
                .decode_image::<RgbFormat>()
                .map_err(|_| CameraError::DecodeFailed)?
                .into_raw();
            for pixel in pixels.chunks_exact_mut(3) {
                pixel.swap(0, 2);
            }
            BgrFrame::new(size.width(), size.height(), pixels)
                .map_err(|_| CameraError::DecodeFailed)
        }

        fn close(&mut self) {
            self.stop_stream();
        }
    }

    impl Drop for LinuxCameraSession {
        fn drop(&mut self) {
            self.stop_stream();
        }
    }

    fn frame_format(fourcc: FourCC) -> Option<FrameFormat> {
        match fourcc.str().ok()? {
            "MJPG" => Some(FrameFormat::MJPEG),
            "YUYV" => Some(FrameFormat::YUYV),
            "NV12" => Some(FrameFormat::NV12),
            "GREY" => Some(FrameFormat::GRAY),
            "RGB3" => Some(FrameFormat::RAWRGB),
            "BGR3" => Some(FrameFormat::RAWBGR),
            _ => None,
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{
        ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    };
    use nokhwa::{Buffer, Camera};

    use crate::backend::{CameraConfig, CameraError, CameraSession, EffectiveCameraConfig};
    use crate::frame::{BgrFrame, CaptureResolution};
    use crate::selector::CameraSelector;

    const FORMAT_PREFERENCE: [FrameFormat; 5] = [
        FrameFormat::MJPEG,
        FrameFormat::YUYV,
        FrameFormat::NV12,
        FrameFormat::RAWRGB,
        FrameFormat::RAWBGR,
    ];

    pub(super) fn open(config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        let index = match config.selector() {
            CameraSelector::Index(index) => CameraIndex::Index(*index),
            CameraSelector::Native(native) => CameraIndex::String(native.clone()),
        };
        let size = config.resolution().size();
        let mut camera = None;
        for format in FORMAT_PREFERENCE {
            let formats = [format];
            let requested = RequestedFormat::with_formats(
                RequestedFormatType::Closest(CameraFormat::new_from(
                    size.width(),
                    size.height(),
                    format,
                    config.requested_fps(),
                )),
                &formats,
            );
            if let Ok(candidate) =
                Camera::with_backend(index.clone(), requested, ApiBackend::MediaFoundation)
            {
                camera = Some(candidate);
                break;
            }
        }
        let camera = camera.ok_or(CameraError::OpenFailed)?;
        let actual = camera.camera_format();
        let effective = validate_format(config.clone(), actual)?;
        let mut session = WindowsCameraSession { camera, effective };
        session
            .camera
            .open_stream()
            .map_err(|_| CameraError::OpenFailed)?;
        Ok(Box::new(session))
    }

    struct WindowsCameraSession {
        camera: Camera,
        effective: EffectiveCameraConfig,
    }

    impl CameraSession for WindowsCameraSession {
        fn effective_config(&self) -> &EffectiveCameraConfig {
            &self.effective
        }

        fn reconfigure(
            &mut self,
            requested_fps: u32,
            resolution: CaptureResolution,
        ) -> Result<EffectiveCameraConfig, CameraError> {
            let requested = CameraConfig::new(
                self.effective.requested().selector().clone(),
                requested_fps,
                resolution,
            )?;
            let size = resolution.size();
            let formats = self
                .camera
                .compatible_camera_formats()
                .map_err(|_| CameraError::ApplyFailed)?;
            let candidate = FORMAT_PREFERENCE.iter().find_map(|wanted| {
                formats
                    .iter()
                    .filter(|format| {
                        format.format() == *wanted
                            && format.width() == size.width()
                            && format.height() == size.height()
                            && format.frame_rate() <= requested_fps
                    })
                    .max_by_key(|format| format.frame_rate())
                    .copied()
            });
            let candidate = candidate.ok_or(CameraError::ResolutionRejected)?;
            let accepted = self
                .camera
                .set_camera_requset(RequestedFormat::with_formats(
                    RequestedFormatType::Exact(candidate),
                    &[candidate.format()],
                ))
                .map_err(|_| CameraError::ApplyFailed)?;
            let effective = validate_format(requested, accepted)?;
            self.effective = effective.clone();
            Ok(effective)
        }

        fn read_frame(&mut self) -> Result<BgrFrame, CameraError> {
            let frame: Buffer = self.camera.frame().map_err(|_| CameraError::ReadFailed)?;
            let dimensions = frame.resolution();
            let expected = self.effective.requested().resolution().size();
            if dimensions.width() != expected.width() || dimensions.height() != expected.height() {
                return Err(CameraError::FrameSizeMismatch);
            }
            let mut pixels = frame
                .decode_image::<RgbFormat>()
                .map_err(|_| CameraError::DecodeFailed)?
                .into_raw();
            for pixel in pixels.chunks_exact_mut(3) {
                pixel.swap(0, 2);
            }
            BgrFrame::new(expected.width(), expected.height(), pixels)
                .map_err(|_| CameraError::DecodeFailed)
        }

        fn close(&mut self) {
            let _ = self.camera.stop_stream();
        }
    }

    fn validate_format(
        requested: CameraConfig,
        actual: CameraFormat,
    ) -> Result<EffectiveCameraConfig, CameraError> {
        let expected = requested.resolution().size();
        if actual.width() != expected.width() || actual.height() != expected.height() {
            return Err(CameraError::ResolutionRejected);
        }
        EffectiveCameraConfig::new(requested, actual.frame_rate())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use crate::backend::{CameraConfig, CameraError, CameraSession};

    pub(super) fn open(_config: &CameraConfig) -> Result<Box<dyn CameraSession>, CameraError> {
        Err(CameraError::OpenFailed)
    }
}
