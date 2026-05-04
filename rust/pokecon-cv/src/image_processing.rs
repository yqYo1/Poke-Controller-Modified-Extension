use crate::camera::{Frame, PixelFormat};
use std::collections::HashMap;
use thiserror::Error;

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
            format: frame.format.clone(),
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
            return Err(ImageError::UnsupportedFormat(frame.format.clone()));
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
                let diff = (frame.data[frame_idx] as i16 - template.data[template_idx] as i16).abs() as u64;
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
}
