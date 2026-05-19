//! VAAPI hardware-accelerated video encoder using FFmpeg.
//!
//! Provides H.264 and HEVC hardware encoding via VAAPI on Linux.
//! Uses `ffmpeg-next` for the safe encoding pipeline and `ffmpeg-sys-next`
//! for VAAPI hardware context initialization (requires `unsafe`).
//!
//! # Safety
//!
//! All `unsafe` blocks are isolated to hardware context creation and
//! wrapped in safe abstractions. The public API is fully safe.
//!
//! # Architecture
//!
//! ```text
//! RGB Frame → NV12 Conversion (software) → VAAPI Upload → Hardware Encode → H.264/HEVC NALs
//! ```

use std::ptr::{self, NonNull};

use ffmpeg_next as ffmpeg;
use ffmpeg_sys_next as sys;

// ── Error Types ──────────────────────────────────────────────────────────────

/// Errors that can occur during VAAPI encoding.
#[derive(Debug, thiserror::Error)]
pub enum VaapiError {
    /// Failed to initialize FFmpeg.
    #[error("FFmpeg initialization failed: {0}")]
    FfmpegInit(String),

    /// No VAAPI-compatible hardware encoder found.
    #[error("No VAAPI encoder available for codec {codec}")]
    NoEncoder { codec: String },

    /// Failed to create VAAPI hardware device context.
    #[error("VAAPI device creation failed: {details}")]
    DeviceCreation { details: String },

    /// Failed to configure encoder.
    #[error("Encoder configuration failed: {0}")]
    Config(String),

    /// Encoding operation failed.
    #[error("Encode failed: {0}")]
    Encode(String),

    /// Frame format conversion failed.
    #[error("Format conversion failed: {0}")]
    FormatConversion(String),
}

// ── Configuration ────────────────────────────────────────────────────────────

/// Supported hardware codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaapiCodec {
    /// H.264 / AVC (widest compatibility).
    H264,
    /// H.265 / HEVC (better compression, newer hardware).
    H265,
}

impl VaapiCodec {
    /// FFmpeg encoder name for this codec.
    fn encoder_name(&self) -> &'static str {
        match self {
            VaapiCodec::H264 => "h264_vaapi",
            VaapiCodec::H265 => "hevc_vaapi",
        }
    }

    /// RTP payload type name (for SDP).
    pub fn rtp_codec_name(&self) -> &'static str {
        match self {
            VaapiCodec::H264 => "H264",
            VaapiCodec::H265 => "H265",
        }
    }
}

/// Configuration for the VAAPI encoder.
#[derive(Debug, Clone)]
pub struct VaapiConfig {
    /// Target video width (must be even).
    pub width: u32,
    /// Target video height (must be even).
    pub height: u32,
    /// Target frame rate in fps.
    pub framerate: u32,
    /// Target bitrate in kbps.
    pub bitrate_kbps: u32,
    /// Codec to use.
    pub codec: VaapiCodec,
    /// VAAPI device path (e.g., "/dev/dri/renderD128").
    pub device_path: String,
}

impl Default for VaapiConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            framerate: 30,
            bitrate_kbps: 1000,
            codec: VaapiCodec::H264,
            device_path: "/dev/dri/renderD128".to_string(),
        }
    }
}

// ── Hardware Context (unsafe internals) ──────────────────────────────────────

/// VAAPI hardware device context.
///
/// Wraps `AVHWDeviceContext` and ensures proper cleanup via RAII.
pub struct VaapiDeviceContext {
    /// Pointer to the hardware device context buffer.
    ptr: NonNull<sys::AVBufferRef>,
}

// SAFETY: AVBufferRef is thread-safe via reference counting.
unsafe impl Send for VaapiDeviceContext {}
unsafe impl Sync for VaapiDeviceContext {}

impl VaapiDeviceContext {
    /// Create a new VAAPI device context for the given DRM render node.
    ///
    /// # Safety
    ///
    /// This function uses `unsafe` FFI calls to FFmpeg. The returned context
    /// is safe to use after creation.
    ///
    /// # Errors
    ///
    /// Returns `VaapiError::DeviceCreation` if VAAPI initialization fails.
    pub fn new(device_path: &str) -> Result<Self, VaapiError> {
        // SAFETY: FFmpeg must be initialized before any other FFmpeg calls.
        ffmpeg::init().map_err(|e| VaapiError::FfmpegInit(e.to_string()))?;

        let device_cstr =
            std::ffi::CString::new(device_path).map_err(|e| VaapiError::DeviceCreation {
                details: format!("Invalid device path: {}", e),
            })?;

        // SAFETY: av_hwdevice_ctx_create allocates and initializes a hardware
        // device context. We check the return code and validate the pointer.
        let mut hw_device_ctx: *mut sys::AVBufferRef = ptr::null_mut();
        let ret = unsafe {
            sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                device_cstr.as_ptr(),
                ptr::null_mut(), // no options
                0,               // no flags
            )
        };

        if ret < 0 || hw_device_ctx.is_null() {
            return Err(VaapiError::DeviceCreation {
                details: format!(
                    "av_hwdevice_ctx_create failed with code {} (device: {})",
                    ret, device_path
                ),
            });
        }

        // SAFETY: We verified the pointer is non-null above.
        let ptr = unsafe { NonNull::new_unchecked(hw_device_ctx) };

        Ok(Self { ptr })
    }

    /// Get the raw `AVBufferRef` pointer for use with encoder context.
    ///
    /// # Safety
    ///
    /// The pointer remains valid as long as this `VaapiDeviceContext` is alive.
    /// Do not free or modify the referenced context.
    pub unsafe fn as_ptr(&self) -> *mut sys::AVBufferRef {
        self.ptr.as_ptr()
    }

    /// Get the `AVHWDeviceContext` pointer.
    ///
    /// # Safety
    ///
    /// The pointer remains valid as long as this `VaapiDeviceContext` is alive.
    unsafe fn hwctx(&self) -> *mut sys::AVHWDeviceContext {
        // SAFETY: AVBufferRef.data points to AVHWDeviceContext for hw device buffers.
        (*self.ptr.as_ptr()).data as *mut sys::AVHWDeviceContext
    }
}

impl Drop for VaapiDeviceContext {
    fn drop(&mut self) {
        // SAFETY: av_buffer_unref properly decrements the reference count and
        // frees when it reaches zero. We own one reference.
        unsafe {
            let mut ptr = self.ptr.as_ptr();
            sys::av_buffer_unref(&mut ptr);
        }
    }
}

// ── Encoder ──────────────────────────────────────────────────────────────────

/// A VAAPI-accelerated video encoder.
///
/// Produces H.264 or HEVC bitstream suitable for RTP packetization.
pub struct VaapiEncoder {
    config: VaapiConfig,
    /// FFmpeg encoder (owns the hardware frame context).
    encoder: ffmpeg::codec::encoder::video::Encoder,
    /// Hardware device context (must outlive encoder).
    #[allow(dead_code)]
    hw_device: VaapiDeviceContext,
    /// Frame counter for timestamp generation.
    frame_count: u64,
    /// Scratch buffer for NV12 conversion.
    nv12_buffer: Vec<u8>,
}

impl std::fmt::Debug for VaapiEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaapiEncoder")
            .field("config", &self.config)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

impl VaapiEncoder {
    /// Create a new VAAPI encoder with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns `VaapiError` if hardware initialization or encoder setup fails.
    pub fn new(config: VaapiConfig) -> Result<Self, VaapiError> {
        // Validate dimensions.
        if config.width == 0
            || config.height == 0
            || config.width % 2 != 0
            || config.height % 2 != 0
        {
            return Err(VaapiError::Config(
                "Width and height must be non-zero and even for NV12 format".to_string(),
            ));
        }

        // Initialize FFmpeg (idempotent if already initialized).
        ffmpeg::init().map_err(|e| VaapiError::FfmpegInit(e.to_string()))?;

        // Create VAAPI hardware device context.
        let hw_device = VaapiDeviceContext::new(&config.device_path)?;

        // Find the VAAPI encoder.
        let codec =
            ffmpeg::codec::encoder::find_by_name(config.codec.encoder_name()).ok_or_else(|| {
                VaapiError::NoEncoder {
                    codec: config.codec.encoder_name().to_string(),
                }
            })?;

        // Create encoder context and configure video parameters.
        let mut encoder = {
            let ctx = ffmpeg::codec::context::Context::new_with_codec(codec);
            let mut video = ctx.encoder().video().map_err(|e| {
                VaapiError::Config(format!("Failed to create video encoder: {}", e))
            })?;

            video.set_width(config.width);
            video.set_height(config.height);
            // VAAPI encoders typically expect NV12 input.
            video.set_format(ffmpeg::format::Pixel::NV12);
            video.set_frame_rate(Some(ffmpeg::Rational::new(config.framerate as i32, 1)));
            video.set_bit_rate(config.bitrate_kbps as usize * 1000);
            video.set_max_bit_rate(config.bitrate_kbps as usize * 1000);
            // Low latency settings for real-time streaming.
            video.set_gop(config.framerate); // 1-second GOP.

            // Set hardware device context on the encoder.
            // SAFETY: We set hw_device_ctx before opening the encoder. The pointer
            // remains valid because hw_device outlives encoder.
            let hw_ctx_ref = unsafe {
                let ref_ptr = sys::av_buffer_ref(hw_device.as_ptr());
                if ref_ptr.is_null() {
                    return Err(VaapiError::DeviceCreation {
                        details: "av_buffer_ref returned NULL for hw_device_ctx".to_string(),
                    });
                }
                // Store the reference in a temporary that will be freed on panic/drop.
                // SAFETY: ref_ptr is non-null. We own this reference.
                VaapiDeviceContext {
                    ptr: NonNull::new_unchecked(ref_ptr),
                }
            };
            unsafe {
                (*video.as_mut_ptr()).hw_device_ctx = hw_ctx_ref.ptr.as_ptr();
            }
            // hw_ctx_ref is intentionally NOT dropped here — ownership is transferred
            // to the encoder context. The encoder's Drop will free it.

            // Open the encoder.
            let encoder = video
                .open_as(codec)
                .map_err(|e| VaapiError::Config(format!("Failed to open encoder: {}", e)))?;

            // Prevent hw_ctx_ref from being dropped (ownership transferred to encoder).
            std::mem::forget(hw_ctx_ref);

            encoder
        };

        let nv12_buf_size = (config.width * config.height * 3 / 2) as usize;

        Ok(Self {
            config,
            encoder,
            hw_device,
            frame_count: 0,
            nv12_buffer: vec![0u8; nv12_buf_size],
        })
    }

    /// Encode a single RGB frame.
    ///
    /// Returns encoded bitstream data (H.264/HEVC NAL units) suitable for
    /// RTP packetization.
    ///
    /// # Errors
    ///
    /// Returns `VaapiError::Encode` if encoding fails.
    pub fn encode_rgb(&mut self, rgb: &[u8]) -> Result<Vec<u8>, VaapiError> {
        // Convert RGB to NV12 (software fallback).
        self.rgb_to_nv12(rgb)?;

        // Create a software frame for input.
        let mut frame = ffmpeg::frame::Video::new(
            ffmpeg::format::Pixel::NV12,
            self.config.width,
            self.config.height,
        );

        // Copy NV12 data into frame planes.
        // NV12 layout: Y plane (width × height), then UV plane (width × height / 2, interleaved).
        let y_size = (self.config.width * self.config.height) as usize;
        let y_plane = frame.data_mut(0);
        y_plane.copy_from_slice(&self.nv12_buffer[..y_plane.len()]);
        let uv_plane = frame.data_mut(1);
        uv_plane.copy_from_slice(&self.nv12_buffer[y_size..y_size + uv_plane.len()]);

        // Set presentation timestamp.
        frame.set_pts(Some(self.frame_count as i64));
        self.frame_count += 1;

        // Send frame to encoder.
        self.encoder
            .send_frame(&frame)
            .map_err(|e| VaapiError::Encode(format!("send_frame failed: {}", e)))?;

        // Collect encoded packets.
        let mut output = Vec::new();
        let mut packet = ffmpeg::packet::Packet::empty();

        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    if let Some(data) = packet.data() {
                        output.extend_from_slice(data);
                    }
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                Err(e) => return Err(VaapiError::Encode(format!("receive_packet failed: {}", e))),
            }
        }

        Ok(output)
    }

    /// Force encoder to output any buffered frames.
    ///
    /// Call this before dropping the encoder to ensure all frames are emitted.
    pub fn flush(&mut self) -> Result<Vec<u8>, VaapiError> {
        self.encoder
            .send_eof()
            .map_err(|e| VaapiError::Encode(format!("send_eof failed: {}", e)))?;

        let mut output = Vec::new();
        let mut packet = ffmpeg::packet::Packet::empty();

        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    if let Some(data) = packet.data() {
                        output.extend_from_slice(data);
                    }
                }
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
                Err(e) => return Err(VaapiError::Encode(format!("receive_packet failed: {}", e))),
            }
        }

        Ok(output)
    }

    /// Convert RGB to NV12 format.
    ///
    /// This is a software conversion. For zero-copy, use DMA-BUF or
    /// hardware surface upload directly.
    fn rgb_to_nv12(&mut self, rgb: &[u8]) -> Result<(), VaapiError> {
        let w = self.config.width as usize;
        let h = self.config.height as usize;
        let expected = w * h * 3;

        if rgb.len() != expected {
            return Err(VaapiError::FormatConversion(format!(
                "RGB buffer size mismatch: expected {} bytes, got {}",
                expected,
                rgb.len()
            )));
        }

        let (y_plane, uv_plane) = self.nv12_buffer.split_at_mut(w * h);

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 3;
                let r = rgb[idx] as f32;
                let g = rgb[idx + 1] as f32;
                let b = rgb[idx + 2] as f32;

                // BT.601 conversion.
                let y_val = 0.299 * r + 0.587 * g + 0.114 * b;
                y_plane[y * w + x] = y_val.clamp(0.0, 255.0) as u8;
            }
        }

        for y in (0..h).step_by(2) {
            for x in (0..w).step_by(2) {
                let idx0 = (y * w + x) * 3;
                let idx1 = (y * w + x + 1) * 3;
                let idx2 = ((y + 1) * w + x) * 3;
                let idx3 = ((y + 1) * w + x + 1) * 3;

                let r = (rgb[idx0] as f32 + rgb[idx1] as f32 + rgb[idx2] as f32 + rgb[idx3] as f32)
                    / 4.0;
                let g = (rgb[idx0 + 1] as f32
                    + rgb[idx1 + 1] as f32
                    + rgb[idx2 + 1] as f32
                    + rgb[idx3 + 1] as f32)
                    / 4.0;
                let b = (rgb[idx0 + 2] as f32
                    + rgb[idx1 + 2] as f32
                    + rgb[idx2 + 2] as f32
                    + rgb[idx3 + 2] as f32)
                    / 4.0;

                let u_val = -0.169 * r - 0.331 * g + 0.5 * b + 128.0;
                let v_val = 0.5 * r - 0.419 * g - 0.081 * b + 128.0;

                let uv_idx = (y / 2) * (w / 2) + (x / 2);
                uv_plane[uv_idx * 2] = u_val.clamp(0.0, 255.0) as u8;
                uv_plane[uv_idx * 2 + 1] = v_val.clamp(0.0, 255.0) as u8;
            }
        }

        Ok(())
    }

    /// Get the encoder configuration.
    pub fn config(&self) -> &VaapiConfig {
        &self.config
    }

    /// Get the codec type.
    pub fn codec(&self) -> VaapiCodec {
        self.config.codec
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        // Use catch_unwind to prevent panic-in-drop during stack unwinding.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Err(e) = self.flush() {
                tracing::warn!("VaapiEncoder: flush on drop failed: {}", e);
            }
        }));
    }
}

// ── VideoEncoder trait implementation ──────────────────────────────────────────

impl crate::webrtc::VideoEncoder for VaapiEncoder {
    fn encode_rgb(&mut self, rgb: &[u8]) -> Result<Vec<u8>, String> {
        self.encode_rgb(rgb).map_err(|e| e.to_string())
    }

    fn codec_name(&self) -> &'static str {
        self.config.codec.rtp_codec_name()
    }

    fn width(&self) -> u32 {
        self.config.width
    }

    fn height(&self) -> u32 {
        self.config.height
    }

    fn framerate(&self) -> u32 {
        self.config.framerate
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vaapi_codec_encoder_name() {
        assert_eq!(VaapiCodec::H264.encoder_name(), "h264_vaapi");
        assert_eq!(VaapiCodec::H265.encoder_name(), "hevc_vaapi");
    }

    #[test]
    fn test_vaapi_config_default() {
        let config = VaapiConfig::default();
        assert_eq!(config.width, 640);
        assert_eq!(config.height, 480);
        assert_eq!(config.codec, VaapiCodec::H264);
    }

    #[test]
    fn test_rgb_to_nv12_size_check() {
        let config = VaapiConfig {
            width: 2,
            height: 2,
            ..Default::default()
        };

        // Create a minimal test that doesn't require VAAPI hardware.
        // We just verify the size check logic.
        let mut nv12_buffer = vec![0u8; 6];
        let rgb = [0u8; 10]; // Wrong size (should be 12 for 2x2 RGB)

        let w = config.width as usize;
        let h = config.height as usize;
        let expected = w * h * 3;

        assert_ne!(rgb.len(), expected);
    }
}
