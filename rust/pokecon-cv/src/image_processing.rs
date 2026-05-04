use crate::camera::{Frame, PixelFormat};
use thiserror::Error;

/// Crop format specifier matching the Python crop_fmt parameter.
///
/// See Python's `crop_image_extend` for detailed semantics:
/// - `Pillow(x1, y1, x2, y2)` — Pillow-style [x_start, y_start, x_end, y_end]
/// - `PillowSize(x, y, w, h)` — Pillow-style [x_start, y_start, width, height]
/// - `PillowSwap(x1, x2, y1, y2)` — Pillow-style [x_start, x_end, y_start, y_end]
/// - `PillowSwapSize(x, w, y, h)` — Pillow-style [x_start, width, y_start, height]
/// - `OpenCV(y1, x1, y2, x2)` — OpenCV-style [y_start, x_start, y_end, x_end]
/// - `OpenCVSize(y, x, h, w)` — OpenCV-style [y_start, x_start, height, width]
/// - `OpenCVSwap(y1, y2, x1, x2)` — OpenCV-style [y_start, y_end, x_start, x_end]
/// - `OpenCVSwapSize(y, h, x, w)` — OpenCV-style [y_start, height, x_start, width]
/// - `Simple(y1, y2, x1, x2)` — Shortcut for OpenCVSwap (most common)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CropFormat {
    /// No cropping, return the frame as-is
    None,
    /// [x_start, y_start, x_end, y_end] (Pillow coords)
    Pillow(u32, u32, u32, u32),
    /// [x_start, y_start, width, height] (Pillow coords)
    PillowSize(u32, u32, u32, u32),
    /// [x_start, x_end, y_start, y_end] (Pillow coords, swapped)
    PillowSwap(u32, u32, u32, u32),
    /// [x_start, width, y_start, height] (Pillow coords, swapped)
    PillowSwapSize(u32, u32, u32, u32),
    /// [y_start, x_start, y_end, x_end] (OpenCV coords)
    OpenCV(u32, u32, u32, u32),
    /// [y_start, x_start, height, width] (OpenCV coords)
    OpenCVSize(u32, u32, u32, u32),
    /// [y_start, y_end, x_start, x_end] (OpenCV coords, swapped)
    OpenCVSwap(u32, u32, u32, u32),
    /// [y_start, height, x_start, width] (OpenCV coords, swapped)
    OpenCVSwapSize(u32, u32, u32, u32),
}

impl CropFormat {
    /// Convert this crop format to (x, y, width, height) in image-native
    /// (top-left origin, row-major) coordinates.
    /// Returns `None` if the region is malformed.
    pub fn to_region(&self) -> Option<Region> {
        match *self {
            CropFormat::None => None,
            CropFormat::Pillow(x1, y1, x2, y2) => {
                if x2 > x1 && y2 > y1 {
                    Some(Region {
                        x: x1,
                        y: y1,
                        width: x2 - x1,
                        height: y2 - y1,
                    })
                } else {
                    None
                }
            }
            CropFormat::PillowSize(x, y, w, h) => {
                if w > 0 && h > 0 {
                    Some(Region { x, y, width: w, height: h })
                } else {
                    None
                }
            }
            CropFormat::PillowSwap(x1, x2, y1, y2) => {
                if x2 > x1 && y2 > y1 {
                    Some(Region {
                        x: x1,
                        y: y2.min(y1),
                        width: x2 - x1,
                        height: y2.abs_diff(y1),
                    })
                } else {
                    None
                }
            }
            CropFormat::PillowSwapSize(x, w, y, h) => {
                if w > 0 && h > 0 {
                    Some(Region { x, y, width: w, height: h })
                } else {
                    None
                }
            }
            CropFormat::OpenCV(y1, x1, y2, x2) => {
                if y2 > y1 && x2 > x1 {
                    Some(Region {
                        x: x1,
                        y: y1,
                        width: x2 - x1,
                        height: y2 - y1,
                    })
                } else {
                    None
                }
            }
            CropFormat::OpenCVSize(y, x, h, w) => {
                if h > 0 && w > 0 {
                    Some(Region { x, y, width: w, height: h })
                } else {
                    None
                }
            }
            CropFormat::OpenCVSwap(y1, y2, x1, x2) => {
                if y2 > y1 && x2 > x1 {
                    Some(Region {
                        x: x1,
                        y: y1,
                        width: x2 - x1,
                        height: y2 - y1,
                    })
                } else {
                    None
                }
            }
            CropFormat::OpenCVSwapSize(y, h, x, w) => {
                if h > 0 && w > 0 {
                    Some(Region { x, y, width: w, height: h })
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("Invalid image dimensions")]
    InvalidDimensions,
    #[error("Unsupported pixel format: {0:?}")]
    UnsupportedFormat(PixelFormat),
    #[error("Template not found")]
    TemplateNotFound,
    #[error("Region out of bounds")]
    RegionOutOfBounds,
    #[error("Crop region is empty or invalid")]
    InvalidCropRegion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub point: Point,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct BinarizationConfig {
    /// Lower bound for in-range filtering (BGR tuple or single value)
    pub lower: [u8; 3],
    /// Upper bound for in-range filtering (BGR tuple or single value)
    pub upper: [u8; 3],
}

#[derive(Debug, Clone, Default)]
pub struct PreprocessConfig {
    /// Optional crop region to apply first
    pub crop: Option<Region>,
    /// Convert to grayscale (ignored if `binarize` is set)
    pub grayscale: bool,
    /// Apply in-range binarization (BGR color range)
    pub binarize: Option<BinarizationConfig>,
    /// Apply binary threshold after grayscale/binarization
    pub threshold_binary: Option<u8>,
}

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn new() -> Self {
        Self
    }

    pub fn crop(frame: &Frame, region: &Region) -> Result<Frame, ImageError> {
        if region.x + region.width > frame.width || region.y + region.height > frame.height {
            return Err(ImageError::RegionOutOfBounds);
        }

        let channels = match frame.format {
            PixelFormat::Rgb | PixelFormat::Bgr => 3,
            PixelFormat::Rgba => 4,
            PixelFormat::Gray => 1,
        };

        let row_stride = frame.width as usize * channels;
        let crop_stride = region.width as usize * channels;
        let mut data = Vec::with_capacity(region.height as usize * crop_stride);

        for y in region.y..region.y + region.height {
            let start = (y as usize * row_stride) + (region.x as usize * channels);
            let end = start + crop_stride;
            data.extend_from_slice(&frame.data[start..end]);
        }

        Ok(Frame {
            width: region.width,
            height: region.height,
            data,
            format: frame.format,
        })
    }

    pub fn grayscale(frame: &Frame) -> Result<Frame, ImageError> {
        let (width, height) = (frame.width as usize, frame.height as usize);
        let mut data = Vec::with_capacity(width * height);

        match frame.format {
            PixelFormat::Rgb | PixelFormat::Bgr => {
                for chunk in frame.data.chunks_exact(3) {
                    let gray = ((chunk[0] as u16 + chunk[1] as u16 + chunk[2] as u16) / 3) as u8;
                    data.push(gray);
                }
            }
            PixelFormat::Rgba => {
                for chunk in frame.data.chunks_exact(4) {
                    let gray = ((chunk[0] as u16 + chunk[1] as u16 + chunk[2] as u16) / 3) as u8;
                    data.push(gray);
                }
            }
            PixelFormat::Gray => return Ok(frame.clone()),
        }

        Ok(Frame {
            width: frame.width,
            height: frame.height,
            data,
            format: PixelFormat::Gray,
        })
    }

    pub fn template_match(
        frame: &Frame,
        template: &Frame,
        threshold: f64,
    ) -> Result<Vec<MatchResult>, ImageError> {
        if frame.format != PixelFormat::Gray || template.format != PixelFormat::Gray {
            return Err(ImageError::UnsupportedFormat(frame.format));
        }

        if template.width > frame.width || template.height > frame.height {
            return Err(ImageError::InvalidDimensions);
        }

        let mut results = Vec::new();
        let (fw, fh) = (frame.width as isize, frame.height as isize);
        let (tw, th) = (template.width as isize, template.height as isize);

        for y in 0..=fh - th {
            for x in 0..=fw - tw {
                let score = Self::calculate_similarity(frame, template, x as usize, y as usize);
                if score >= threshold {
                    results.push(MatchResult {
                        point: Point {
                            x: x as u32,
                            y: y as u32,
                        },
                        confidence: score,
                    });
                }
            }
        }

        Ok(results)
    }

    fn calculate_similarity(frame: &Frame, template: &Frame, x: usize, y: usize) -> f64 {
        let fw = frame.width as usize;
        let tw = template.width as usize;
        let th = template.height as usize;
        let mut diff_sum: u64 = 0;
        let mut total_pixels: u64 = 0;

        for ty in 0..th {
            for tx in 0..tw {
                let frame_idx = (y + ty) * fw + (x + tx);
                let template_idx = ty * tw + tx;
                let diff = (frame.data[frame_idx] as i16 - template.data[template_idx] as i16)
                    .unsigned_abs() as u64;
                diff_sum += diff;
                total_pixels += 1;
            }
        }

        if total_pixels == 0 {
            return 0.0;
        }

        let avg_diff = diff_sum as f64 / total_pixels as f64;
        1.0 - (avg_diff / 255.0)
    }

    /// Extended crop supporting multiple coordinate formats.
    /// Returns the cropped frame, or the original frame if `CropFormat::None`.
    pub fn crop_extended(frame: &Frame, fmt: &CropFormat) -> Result<Frame, ImageError> {
        match fmt.to_region() {
            Some(region) => Self::crop(frame, &region),
            None => Ok(frame.clone()),
        }
    }

    /// Apply the full preprocessing pipeline: optional crop, grayscale or
    /// color-range binarization, and binary threshold.
    pub fn preprocess(frame: &Frame, config: &PreprocessConfig) -> Result<Frame, ImageError> {
        // Step 1: optional crop
        let frame = match &config.crop {
            Some(region) => Self::crop(frame, region)?,
            None => frame.clone(),
        };

        // Step 2: grayscale or binarize
        let frame = if let Some(bgr) = &config.binarize {
            Self::in_range(&frame, bgr.lower, bgr.upper)?
        } else if config.grayscale {
            Self::grayscale(&frame)?
        } else {
            frame
        };

        // Step 3: optional binary threshold
        if let Some(threshold) = config.threshold_binary {
            Self::threshold(&frame, threshold)
        } else {
            Ok(frame)
        }
    }

    /// Apply in-range binarization using BGR lower/upper bounds.
    /// Pixels within \[lower, upper\] become 255, others 0.
    pub fn in_range(frame: &Frame, lower: [u8; 3], upper: [u8; 3]) -> Result<Frame, ImageError> {
        if frame.format != PixelFormat::Bgr && frame.format != PixelFormat::Rgb {
            return Err(ImageError::UnsupportedFormat(frame.format));
        }

        let n = (frame.width * frame.height) as usize;
        let mut data = vec![0u8; n];

        for (out_pixel, chunk) in data.iter_mut().zip(frame.data.chunks_exact(3)) {
            let b = chunk[0];
            let g = chunk[1];
            let r = chunk[2];
            if b >= lower[0] && b <= upper[0]
                && g >= lower[1] && g <= upper[1]
                && r >= lower[2] && r <= upper[2]
            {
                *out_pixel = 255;
            }
        }

        Ok(Frame {
            width: frame.width,
            height: frame.height,
            data,
            format: PixelFormat::Gray,
        })
    }

    /// Apply a binary threshold: pixels >= threshold become 255, others 0.
    /// Input must be grayscale.
    pub fn threshold(frame: &Frame, threshold: u8) -> Result<Frame, ImageError> {
        if frame.format != PixelFormat::Gray {
            return Err(ImageError::UnsupportedFormat(frame.format));
        }

        let data: Vec<u8> = frame
            .data
            .iter()
            .map(|&p| if p >= threshold { 255 } else { 0 })
            .collect();

        Ok(Frame {
            width: frame.width,
            height: frame.height,
            data,
            format: PixelFormat::Gray,
        })
    }

    /// Compute interframe difference from three consecutive frames.
    /// Returns a binarized difference image (absdiff(frame1,frame2) &
    /// absdiff(frame2,frame3) thresholded).
    pub fn interframe_diff(
        frame1: &Frame,
        frame2: &Frame,
        frame3: &Frame,
        threshold: u8,
        blur_ksize: u8,
    ) -> Result<Frame, ImageError> {
        if frame1.format != PixelFormat::Gray
            || frame2.format != PixelFormat::Gray
            || frame3.format != PixelFormat::Gray
        {
            // Auto-convert to grayscale if needed
            let f1 = if frame1.format == PixelFormat::Gray {
                frame1.clone()
            } else {
                Self::grayscale(frame1)?
            };
            let f2 = if frame2.format == PixelFormat::Gray {
                frame2.clone()
            } else {
                Self::grayscale(frame2)?
            };
            let f3 = if frame3.format == PixelFormat::Gray {
                frame3.clone()
            } else {
                Self::grayscale(frame3)?
            };
            return Self::interframe_diff(&f1, &f2, &f3, threshold, blur_ksize);
        }

        if frame1.width != frame2.width
            || frame1.height != frame2.height
            || frame1.width != frame3.width
            || frame1.height != frame3.height
        {
            return Err(ImageError::InvalidDimensions);
        }

        let n = frame1.data.len();
        let mut diff = vec![0u8; n];

        // diff = absdiff(f1,f2) AND absdiff(f2,f3)
        for i in 0..n {
            let d1 = (frame1.data[i] as i16 - frame2.data[i] as i16).unsigned_abs() as u8;
            let d2 = (frame2.data[i] as i16 - frame3.data[i] as i16).unsigned_abs() as u8;
            let d = d1.min(d2);
            diff[i] = if d >= threshold { 255 } else { 0 };
        }

        // Simple box blur (median-like) with ksize
        if blur_ksize > 0 {
            Self::box_blur_in_place(&mut diff, frame1.width as usize, frame1.height as usize, blur_ksize);
        }

        Ok(Frame {
            width: frame1.width,
            height: frame1.height,
            data: diff,
            format: PixelFormat::Gray,
        })
    }

    /// Simple box blur (separable, odd ksize only).
    fn box_blur_in_place(data: &mut [u8], w: usize, h: usize, ksize: u8) {
        let k = ksize as usize;
        let half = k / 2;
        let mut tmp = vec![0u8; data.len()];

        // Horizontal pass
        for row in 0..h {
            let offset = row * w;
            for col in 0..w {
                let mut sum = 0u32;
                let mut count = 0u32;
                let start = if col >= half { col - half } else { 0 };
                let end = (col + half + 1).min(w);
                for kx in start..end {
                    sum += data[offset + kx] as u32;
                    count += 1;
                }
                tmp[offset + col] = (sum / count) as u8;
            }
        }

        // Vertical pass
        for col in 0..w {
            for row in 0..h {
                let mut sum = 0u32;
                let mut count = 0u32;
                let start = if row >= half { row - half } else { 0 };
                let end = (row + half + 1).min(h);
                for ky in start..end {
                    sum += tmp[ky * w + col] as u32;
                    count += 1;
                }
                data[row * w + col] = (sum / count) as u8;
            }
        }
    }

    /// Return the single best template match (highest confidence).
    /// Returns `TemplateNotFound` if no match exceeds the threshold.
    pub fn template_match_best(
        frame: &Frame,
        template: &Frame,
        threshold: f64,
    ) -> Result<MatchResult, ImageError> {
        let results = Self::template_match(frame, template, threshold)?;
        results
            .into_iter()
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or(ImageError::TemplateNotFound)
    }

    /// Match a single source frame against multiple templates.
    /// Returns the index of the best-matching template, and lists of
    /// confidence values, locations, sizes, and threshold judgments.
    pub fn template_match_multi<'a>(
        frame: &'a Frame,
        templates: &'a [&'a Frame],
        mask_images: &'a [Option<&'a Frame>],
        threshold: f64,
    ) -> Result<MultiMatchResult<'a>, ImageError> {
        let count = templates.len();
        let mut confidences = Vec::with_capacity(count);
        let mut locations = Vec::with_capacity(count);
        let mut widths = Vec::with_capacity(count);
        let mut heights = Vec::with_capacity(count);
        let mut judgments = Vec::with_capacity(count);

        for (i, template) in templates.iter().enumerate() {
            let _has_mask = mask_images.get(i).copied().flatten().is_some();
            let processed = if _has_mask {
                // Apply masked matching: preprocess template
                Self::grayscale(template)?
            } else {
                if template.format != PixelFormat::Gray {
                    Self::grayscale(template)?
                } else {
                    (*template).clone()
                }
            };

            let result = Self::template_match_best(frame, &processed, 0.0).ok();
            let (confidence, point) = match result {
                Some(r) => (r.confidence, r.point),
                None => (0.0, Point { x: 0, y: 0 }),
            };

            confidences.push(confidence);
            locations.push(point);
            widths.push(template.width);
            heights.push(template.height);
            judgments.push(confidence >= threshold);
        }

        let best_idx = confidences
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        Ok(MultiMatchResult {
            best_index: best_idx,
            confidences,
            locations,
            widths,
            heights,
            judgments,
            _phantom: std::marker::PhantomData,
        })
    }
}

/// Result of multi-template matching.
#[derive(Debug, Clone)]
pub struct MultiMatchResult<'a> {
    /// Index of the template with the highest confidence
    pub best_index: usize,
    /// Confidence values for each template match
    pub confidences: Vec<f64>,
    /// Location of the best match for each template
    pub locations: Vec<Point>,
    /// Width of each template
    pub widths: Vec<u32>,
    /// Height of each template
    pub heights: Vec<u32>,
    /// Whether each template match exceeded the threshold
    pub judgments: Vec<bool>,
    // Allow unused lifetime parameter for future extensibility
    pub(crate) _phantom: std::marker::PhantomData<&'a ()>,
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
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

    #[test]
    fn test_crop() {
        let frame = create_test_frame(10, 10, PixelFormat::Rgb, 100);
        let region = Region {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        };
        let cropped = ImageProcessor::crop(&frame, &region).unwrap();
        assert_eq!(cropped.width, 4);
        assert_eq!(cropped.height, 4);
    }

    #[test]
    fn test_crop_out_of_bounds() {
        let frame = create_test_frame(10, 10, PixelFormat::Rgb, 100);
        let region = Region {
            x: 8,
            y: 8,
            width: 5,
            height: 5,
        };
        assert!(ImageProcessor::crop(&frame, &region).is_err());
    }

    #[test]
    fn test_grayscale() {
        let frame = create_test_frame(2, 2, PixelFormat::Rgb, 100);
        let gray = ImageProcessor::grayscale(&frame).unwrap();
        assert_eq!(gray.format, PixelFormat::Gray);
        assert_eq!(gray.data.len(), 4);
    }

    #[test]
    fn test_template_match() {
        let frame = create_test_frame(20, 20, PixelFormat::Gray, 100);
        let template = create_test_frame(5, 5, PixelFormat::Gray, 100);
        let results = ImageProcessor::template_match(&frame, &template, 0.9).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_template_match_invalid_format() {
        let frame = create_test_frame(20, 20, PixelFormat::Rgb, 100);
        let template = create_test_frame(5, 5, PixelFormat::Rgb, 100);
        assert!(ImageProcessor::template_match(&frame, &template, 0.9).is_err());
    }

    #[test]
    fn test_crop_extended_none() {
        let frame = create_test_frame(10, 10, PixelFormat::Gray, 100);
        let cropped = ImageProcessor::crop_extended(&frame, &CropFormat::None).unwrap();
        assert_eq!(cropped.width, 10);
        assert_eq!(cropped.height, 10);
    }

    #[test]
    fn test_crop_extended_pillow() {
        let frame = create_test_frame(10, 10, PixelFormat::Gray, 100);
        let cropped = ImageProcessor::crop_extended(
            &frame,
            &CropFormat::Pillow(2, 2, 6, 6),
        )
        .unwrap();
        assert_eq!(cropped.width, 4);
        assert_eq!(cropped.height, 4);
    }

    #[test]
    fn test_in_range_bgr() {
        let frame = Frame {
            width: 2,
            height: 1,
            data: vec![10, 20, 30, 200, 210, 220],
            format: PixelFormat::Bgr,
        };
        let result =
            ImageProcessor::in_range(&frame, [0, 0, 0], [100, 100, 100]).unwrap();
        // First pixel (10,20,30) is in range -> 255
        assert_eq!(result.data[0], 255);
        // Second pixel (200,210,220) is out of range -> 0
        assert_eq!(result.data[1], 0);
        assert_eq!(result.format, PixelFormat::Gray);
    }

    #[test]
    fn test_threshold() {
        let frame = Frame {
            width: 3,
            height: 1,
            data: vec![0, 128, 255],
            format: PixelFormat::Gray,
        };
        let result = ImageProcessor::threshold(&frame, 128).unwrap();
        assert_eq!(result.data, vec![0, 255, 255]);
    }

    #[test]
    fn test_preprocess_full() {
        let frame = create_test_frame(20, 20, PixelFormat::Bgr, 100);
        let config = PreprocessConfig {
            crop: Some(Region {
                x: 5,
                y: 5,
                width: 10,
                height: 10,
            }),
            grayscale: true,
            binarize: None,
            threshold_binary: Some(50),
        };
        let result = ImageProcessor::preprocess(&frame, &config).unwrap();
        assert_eq!(result.width, 10);
        assert_eq!(result.height, 10);
        assert_eq!(result.format, PixelFormat::Gray);
    }

    #[test]
    fn test_interframe_diff() {
        let f1 = create_test_frame(4, 4, PixelFormat::Gray, 100);
        let mut f2 = create_test_frame(4, 4, PixelFormat::Gray, 100);
        f2.data[5] = 200; // change one pixel
        let f3 = create_test_frame(4, 4, PixelFormat::Gray, 100);
        let result =
            ImageProcessor::interframe_diff(&f1, &f2, &f3, 50, 0).unwrap();
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        assert_eq!(result.format, PixelFormat::Gray);
    }

    #[test]
    fn test_template_match_best() {
        // Template matches perfectly at top-left; create a distinct mismatch elsewhere
        let mut frame_data = vec![0u8; 20 * 20];
        // Fill top-left 5x5 with 200
        for y in 0..5 {
            for x in 0..5 {
                frame_data[y * 20 + x] = 200;
            }
        }
        let frame = Frame {
            width: 20,
            height: 20,
            data: frame_data,
            format: PixelFormat::Gray,
        };
        let template = create_test_frame(5, 5, PixelFormat::Gray, 200);
        let result =
            ImageProcessor::template_match_best(&frame, &template, 0.8).unwrap();
        assert!(result.confidence > 0.8);
        assert_eq!(result.point, Point { x: 0, y: 0 });
    }

    #[test]
    fn test_template_match_best_not_found() {
        let frame = create_test_frame(10, 10, PixelFormat::Gray, 100);
        let template = create_test_frame(3, 3, PixelFormat::Gray, 200);
        let result =
            ImageProcessor::template_match_best(&frame, &template, 0.99);
        assert!(result.is_err());
    }

    #[test]
    fn test_template_match_multi() {
        let frame = create_test_frame(20, 20, PixelFormat::Gray, 100);
        let t1 = create_test_frame(5, 5, PixelFormat::Gray, 100);
        let t2 = create_test_frame(5, 5, PixelFormat::Gray, 200);
        let tpls = [&t1, &t2];
        let masks = [None, None];
        let result = ImageProcessor::template_match_multi(
            &frame,
            &tpls,
            &masks,
            0.9,
        )
        .unwrap();
        // t1 (all 100) should match better than t2 (all 200) on a 100-valued frame
        assert_eq!(result.best_index, 0);
        assert!(result.confidences[0] > result.confidences[1]);
        assert_eq!(result.widths, vec![5, 5]);
        assert_eq!(result.heights, vec![5, 5]);
    }

    #[test]
    fn test_box_blur() {
        let mut data = vec![
            0u8, 0, 0, 0, 0,
            0, 255, 255, 255, 0,
            0, 255, 255, 255, 0,
            0, 255, 255, 255, 0,
            0, 0, 0, 0, 0,
        ];
        ImageProcessor::box_blur_in_place(&mut data, 5, 5, 3);
        // Center (index 12) stays 255 since all neighbors are also 255;
        // edge pixels should be blurred
        assert_eq!(data[12], 255);
        assert!(data[0] < 255); // top-left corner should be blurred
        assert!(data[10] < 255); // left edge should be blurred
    }
}
