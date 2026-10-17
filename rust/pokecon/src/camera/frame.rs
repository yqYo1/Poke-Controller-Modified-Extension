use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

pub const CHANNEL_COUNT: u32 = 3;
pub const MAX_FRAME_WIDTH: u32 = 1920;
pub const MAX_FRAME_HEIGHT: u32 = 1080;
pub const MAX_FRAME_BYTES: usize =
    MAX_FRAME_WIDTH as usize * MAX_FRAME_HEIGHT as usize * CHANNEL_COUNT as usize;

/// Closed capture resolutions supported by the fixed shared-memory layout.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum CaptureResolution {
    #[serde(rename = "640x360")]
    R640x360,
    #[serde(rename = "1280x720")]
    R1280x720,
    #[serde(rename = "1920x1080")]
    R1920x1080,
}

impl CaptureResolution {
    pub const ALL: [Self; 3] = [Self::R640x360, Self::R1280x720, Self::R1920x1080];

    #[must_use]
    pub const fn size(self) -> FrameSize {
        match self {
            Self::R640x360 => FrameSize {
                width: 640,
                height: 360,
            },
            Self::R1280x720 => FrameSize {
                width: 1280,
                height: 720,
            },
            Self::R1920x1080 => FrameSize {
                width: 1920,
                height: 1080,
            },
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R640x360 => "640x360",
            Self::R1280x720 => "1280x720",
            Self::R1920x1080 => "1920x1080",
        }
    }
}

impl FromStr for CaptureResolution {
    type Err = FrameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("640x360") {
            Ok(Self::R640x360)
        } else if value.eq_ignore_ascii_case("1280x720") {
            Ok(Self::R1280x720)
        } else if value.eq_ignore_ascii_case("1920x1080") {
            Ok(Self::R1920x1080)
        } else {
            Err(FrameError::UnsupportedCaptureResolution)
        }
    }
}

impl<'de> Deserialize<'de> for CaptureResolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

/// Valid BGR frame dimensions bounded by the lifetime-fixed mapping capacity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct FrameSize {
    width: u32,
    height: u32,
}

impl FrameSize {
    /// Creates bounded nonzero dimensions.
    ///
    /// # Errors
    ///
    /// Rejects zero or dimensions above 1920x1080.
    pub const fn new(width: u32, height: u32) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::ZeroDimension);
        }
        if width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
            return Err(FrameError::FrameTooLarge);
        }
        Ok(Self { width, height })
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    #[must_use]
    pub const fn byte_len(self) -> usize {
        self.width as usize * self.height as usize * CHANNEL_COUNT as usize
    }
}

impl From<CaptureResolution> for FrameSize {
    fn from(value: CaptureResolution) -> Self {
        value.size()
    }
}

/// Runtime image flip mode.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlipMode {
    #[default]
    None,
    Vertical,
    Horizontal,
    Both,
}

impl FromStr for FlipMode {
    type Err = FrameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("none") {
            Ok(Self::None)
        } else if value.eq_ignore_ascii_case("vertical") {
            Ok(Self::Vertical)
        } else if value.eq_ignore_ascii_case("horizontal") {
            Ok(Self::Horizontal)
        } else if value.eq_ignore_ascii_case("both") {
            Ok(Self::Both)
        } else {
            Err(FrameError::InvalidFlipMode)
        }
    }
}

/// Validated 0.0..1.0 normalized crop rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct NormalizedRegion {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl NormalizedRegion {
    /// Creates a finite, nondegenerate normalized rectangle.
    ///
    /// # Errors
    ///
    /// Rejects non-finite, out-of-range, or degenerate coordinates.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Result<Self, FrameError> {
        let values = [left, top, right, bottom];
        if !values.iter().all(|value| value.is_finite()) {
            return Err(FrameError::NonFiniteRegion);
        }
        if !values.iter().all(|value| (0.0..=1.0).contains(value)) {
            return Err(FrameError::RegionOutOfBounds);
        }
        if left >= right || top >= bottom {
            return Err(FrameError::DegenerateRegion);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_pixels(self, size: FrameSize) -> PixelRegion {
        let width = f64::from(size.width);
        let height = f64::from(size.height);
        let left = (self.left * width).floor() as u32;
        let top = (self.top * height).floor() as u32;
        let right = ((self.right * width).ceil() as u32).min(size.width);
        let bottom = ((self.bottom * height).ceil() as u32).min(size.height);
        PixelRegion {
            left,
            top,
            right,
            bottom,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NormalizedRegionWire {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl<'de> Deserialize<'de> for NormalizedRegion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NormalizedRegionWire::deserialize(deserializer)?;
        Self::new(wire.left, wire.top, wire.right, wire.bottom).map_err(D::Error::custom)
    }
}

/// Pixel rectangle with exclusive right and bottom edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRegion {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl PixelRegion {
    /// Creates a nondegenerate rectangle bounded by `frame_size`.
    ///
    /// # Errors
    ///
    /// Rejects reversed, degenerate, or out-of-frame coordinates.
    pub const fn new(
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
        frame_size: FrameSize,
    ) -> Result<Self, FrameError> {
        if left >= right || top >= bottom {
            return Err(FrameError::DegenerateRegion);
        }
        if right > frame_size.width || bottom > frame_size.height {
            return Err(FrameError::RegionOutOfBounds);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    #[must_use]
    pub const fn size(self) -> FrameSize {
        FrameSize {
            width: self.right - self.left,
            height: self.bottom - self.top,
        }
    }
}

/// Owned, tightly packed, mutable BGR uint8 frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BgrFrame {
    size: FrameSize,
    pixels: Vec<u8>,
}

impl BgrFrame {
    /// Validates a tightly packed BGR buffer.
    ///
    /// # Errors
    ///
    /// Rejects dimensions outside the fixed capacity or a byte-length mismatch.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, FrameError> {
        let size = FrameSize::new(width, height)?;
        if pixels.len() != size.byte_len() {
            return Err(FrameError::InvalidByteLength);
        }
        Ok(Self { size, pixels })
    }

    #[must_use]
    pub fn zero(resolution: CaptureResolution) -> Self {
        let size = resolution.size();
        Self {
            size,
            pixels: vec![0; size.byte_len()],
        }
    }

    #[must_use]
    pub fn solid(resolution: CaptureResolution, bgr: [u8; 3]) -> Self {
        let size = resolution.size();
        let mut pixels = Vec::with_capacity(size.byte_len());
        for _ in 0..u64::from(size.width) * u64::from(size.height) {
            pixels.extend_from_slice(&bgr);
        }
        Self { size, pixels }
    }

    #[must_use]
    pub const fn size(&self) -> FrameSize {
        self.size
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Returns a tightly packed independent crop.
    ///
    /// # Errors
    ///
    /// Rejects a rectangle outside this frame.
    pub fn crop(&self, region: PixelRegion) -> Result<Self, FrameError> {
        PixelRegion::new(
            region.left,
            region.top,
            region.right,
            region.bottom,
            self.size,
        )?;
        let crop_size = region.size();
        let source_stride = self.size.width as usize * CHANNEL_COUNT as usize;
        let crop_stride = crop_size.width as usize * CHANNEL_COUNT as usize;
        let mut pixels = Vec::with_capacity(crop_size.byte_len());
        for row in region.top..region.bottom {
            let start =
                row as usize * source_stride + region.left as usize * CHANNEL_COUNT as usize;
            pixels.extend_from_slice(&self.pixels[start..start + crop_stride]);
        }
        Ok(Self {
            size: crop_size,
            pixels,
        })
    }

    /// Applies the configured flip without changing pixel channel order.
    pub fn apply_flip(&mut self, mode: FlipMode) {
        if matches!(mode, FlipMode::Horizontal | FlipMode::Both) {
            self.flip_horizontal();
        }
        if matches!(mode, FlipMode::Vertical | FlipMode::Both) {
            self.flip_vertical();
        }
    }

    fn flip_horizontal(&mut self) {
        let row_bytes = self.size.width as usize * CHANNEL_COUNT as usize;
        for row in self.pixels.chunks_exact_mut(row_bytes) {
            for column in 0..self.size.width as usize / 2 {
                let opposite = self.size.width as usize - column - 1;
                for channel in 0..CHANNEL_COUNT as usize {
                    row.swap(
                        column * CHANNEL_COUNT as usize + channel,
                        opposite * CHANNEL_COUNT as usize + channel,
                    );
                }
            }
        }
    }

    fn flip_vertical(&mut self) {
        let row_bytes = self.size.width as usize * CHANNEL_COUNT as usize;
        for row in 0..self.size.height as usize / 2 {
            let opposite = self.size.height as usize - row - 1;
            let (head, tail) = self.pixels.split_at_mut(opposite * row_bytes);
            head[row * row_bytes..(row + 1) * row_bytes].swap_with_slice(&mut tail[..row_bytes]);
        }
    }
}

/// Frame validation failure with no external path or device detail.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FrameError {
    #[error("frame dimensions must be positive")]
    ZeroDimension,
    #[error("frame exceeds the fixed 1920x1080 BGR capacity")]
    FrameTooLarge,
    #[error("frame byte length does not match tightly packed BGR dimensions")]
    InvalidByteLength,
    #[error("capture resolution is not supported")]
    UnsupportedCaptureResolution,
    #[error("flip mode is invalid")]
    InvalidFlipMode,
    #[error("normalized region contains a non-finite value")]
    NonFiniteRegion,
    #[error("crop region is outside the frame")]
    RegionOutOfBounds,
    #[error("crop region is degenerate")]
    DegenerateRegion,
}

#[cfg(test)]
mod tests {
    use super::{
        BgrFrame, CaptureResolution, FlipMode, FrameError, FrameSize, NormalizedRegion, PixelRegion,
    };

    #[test]
    fn capture_resolution_and_region_wire_shapes_are_closed() {
        assert_eq!(
            serde_json::from_str::<CaptureResolution>("\"1280X720\"").unwrap(),
            CaptureResolution::R1280x720
        );
        assert!(serde_json::from_str::<CaptureResolution>("\"800x600\"").is_err());
        assert!(
            serde_json::from_str::<NormalizedRegion>(
                r#"{"left":0.0,"top":0.0,"right":1.0,"bottom":1.0,"extra":0}"#
            )
            .is_err()
        );
        assert_eq!(
            NormalizedRegion::new(0.25, 0.25, 0.75, 0.75)
                .unwrap()
                .to_pixels(FrameSize::new(8, 4).unwrap()),
            PixelRegion::new(2, 1, 6, 3, FrameSize::new(8, 4).unwrap()).unwrap()
        );
    }

    #[test]
    fn frame_validation_crop_and_flips_preserve_bgr_pixels() {
        assert_eq!(
            BgrFrame::new(2, 2, vec![0; 11]).unwrap_err(),
            FrameError::InvalidByteLength
        );
        let original = BgrFrame::new(
            2,
            2,
            vec![
                1, 2, 3, 4, 5, 6, // top row
                7, 8, 9, 10, 11, 12, // bottom row
            ],
        )
        .unwrap();
        let region = PixelRegion::new(1, 0, 2, 2, original.size()).unwrap();
        assert_eq!(
            original.crop(region).unwrap().pixels(),
            &[4, 5, 6, 10, 11, 12]
        );

        let mut horizontal = original.clone();
        horizontal.apply_flip(FlipMode::Horizontal);
        assert_eq!(
            horizontal.pixels(),
            &[4, 5, 6, 1, 2, 3, 10, 11, 12, 7, 8, 9]
        );
        let mut both = original;
        both.apply_flip(FlipMode::Both);
        assert_eq!(both.pixels(), &[10, 11, 12, 7, 8, 9, 4, 5, 6, 1, 2, 3]);
    }
}
