use numpy::{PyArray3, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

use pokecon_core::cv::camera::{Frame, PixelFormat};
use pokecon_core::cv::image_processing::ImageProcessor as CoreImageProcessor;

// ---------------------------------------------------------------------------
// Helper: convert a numpy ndarray (u8, 3D) to a pokecon-core Frame
// ---------------------------------------------------------------------------

fn ndarray_to_frame(arr: &Bound<'_, PyArray3<u8>>) -> PyResult<Frame> {
    let readonly = arr.readonly();
    let shape = readonly.shape();
    let (height, width, channels) = match shape.len() {
        3 => (shape[0], shape[1], shape[2]),
        _ => {
            return Err(PyValueError::new_err(
                "Expected 3D array with shape (H, W, C)",
            ));
        }
    };

    let format = match channels {
        1 => PixelFormat::Gray,
        3 => PixelFormat::Rgb,
        4 => PixelFormat::Rgba,
        n => {
            return Err(PyValueError::new_err(format!(
                "Unsupported number of channels: {n}. Expected 1, 3, or 4."
            )));
        }
    };

    let data = readonly
        .as_slice()
        .map_err(|e| PyValueError::new_err(format!("Array must be C-contiguous: {e}")))?
        .to_vec();

    Ok(Frame {
        width: width as u32,
        height: height as u32,
        data,
        format,
    })
}

// ---------------------------------------------------------------------------
// Helper: convert a pokecon-core Frame to a numpy ndarray (u8, 3D)
// ---------------------------------------------------------------------------

fn frame_to_ndarray<'py>(py: Python<'py>, frame: &Frame) -> Bound<'py, PyArray3<u8>> {
    let channels = frame.format.channels();
    let shape = [frame.height as usize, frame.width as usize, channels];
    let array = ndarray::Array3::from_shape_vec(shape, frame.data.clone())
        .expect("Frame dimensions must match data length");
    PyArray3::from_owned_array(py, array)
}

// ---------------------------------------------------------------------------
// Helper: ensure a Frame is grayscale (converting if necessary)
// ---------------------------------------------------------------------------

fn ensure_grayscale(frame: &Frame) -> Result<Frame, PyErr> {
    if frame.format == PixelFormat::Gray {
        return Ok(frame.clone());
    }
    CoreImageProcessor::grayscale(frame)
        .map_err(|e| PyValueError::new_err(format!("Grayscale conversion failed: {e}")))
}

// ---------------------------------------------------------------------------
// load — load an image from a file path, returned as HxWx3 RGB ndarray
// ---------------------------------------------------------------------------

/// Load an image from a file path and return it as an HxWx3 RGB numpy array.
///
/// Supported formats: PNG, JPEG, GIF, BMP, and others supported by the `image` crate.
#[pyfunction]
fn load<'py>(py: Python<'py>, path: &str) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let img = image::open(path)
        .map_err(|e| PyValueError::new_err(format!("Failed to load image '{path}': {e}")))?;
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let raw_data = rgb.into_raw();
    let shape = [h as usize, w as usize, 3usize];
    let array = ndarray::Array3::from_shape_vec(shape, raw_data)
        .map_err(|e| PyValueError::new_err(format!("Failed to construct array: {e}")))?;
    Ok(PyArray3::from_owned_array(py, array))
}

// ---------------------------------------------------------------------------
// save — save an HxWx3 RGB ndarray to a file
// ---------------------------------------------------------------------------

/// Save an HxWx3 RGB numpy array to an image file.
///
/// Format is inferred from the file extension (".png", ".jpg", etc.).
#[pyfunction]
fn save(path: &str, image: &Bound<'_, PyArray3<u8>>) -> PyResult<()> {
    let readonly = image.readonly();
    let shape = readonly.shape();
    if shape.len() != 3 || shape[2] != 3 {
        return Err(PyValueError::new_err(
            "Expected HxWx3 RGB image array for saving",
        ));
    }
    let (h, w) = (shape[0] as u32, shape[1] as u32);

    let data = readonly
        .as_slice()
        .map_err(|e| PyValueError::new_err(format!("Array must be C-contiguous: {e}")))?
        .to_vec();

    let img: image::RgbImage = image::RgbImage::from_raw(w, h, data).ok_or_else(|| {
        PyValueError::new_err("Failed to create image from raw data (dimension mismatch)")
    })?;

    img.save(path)
        .map_err(|e| PyValueError::new_err(format!("Failed to save image '{path}': {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// resize — resize an image using nearest-neighbor interpolation
// ---------------------------------------------------------------------------

/// Resize an HxWxC image to new dimensions using nearest-neighbor interpolation.
#[pyfunction]
fn resize<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    width: u32,
    height: u32,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let readonly = image.readonly();
    let shape = readonly.shape();
    if shape.len() != 3 {
        return Err(PyValueError::new_err(
            "Expected 3D array with shape (H, W, C)",
        ));
    }
    let (src_h, src_w, channels) = (shape[0] as u32, shape[1] as u32, shape[2]);

    if width == 0 || height == 0 {
        return Err(PyValueError::new_err("Target width and height must be > 0"));
    }

    let mut result = ndarray::Array3::zeros((height as usize, width as usize, channels));

    for y in 0..height {
        for x in 0..width {
            let src_y = (y * src_h / height) as usize;
            let src_x = (x * src_w / width) as usize;
            for c in 0..channels {
                result[[y as usize, x as usize, c]] = readonly.as_array()[[src_y, src_x, c]];
            }
        }
    }

    Ok(PyArray3::from_owned_array(py, result))
}

// ---------------------------------------------------------------------------
// crop — crop a region from an image
// ---------------------------------------------------------------------------

/// Crop a rectangular region from an HxWx3 RGB image.
#[pyfunction]
fn crop<'py>(
    py: Python<'py>,
    image: &Bound<'py, PyArray3<u8>>,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let array = image.readonly();
    let shape = array.shape();
    if shape.len() != 3 || shape[2] != 3 {
        return Err(PyValueError::new_err("Expected HxWx3 RGB image"));
    }

    if y + height > shape[0] || x + width > shape[1] {
        return Err(PyValueError::new_err("Crop region out of bounds"));
    }

    let mut result = ndarray::Array3::zeros((height, width, 3));
    for dy in 0..height {
        for dx in 0..width {
            for c in 0..3 {
                result[[dy, dx, c]] = array.as_array()[[y + dy, x + dx, c]];
            }
        }
    }

    Ok(PyArray3::from_owned_array(py, result))
}

// ---------------------------------------------------------------------------
// grayscale — convert RGB image to grayscale
// ---------------------------------------------------------------------------

/// Convert an HxWx3 RGB image to grayscale (HxWx1).
#[pyfunction]
fn grayscale<'py>(
    py: Python<'py>,
    image: &Bound<'py, PyArray3<u8>>,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let array = image.readonly();
    let shape = array.shape();
    if shape.len() != 3 || shape[2] != 3 {
        return Err(PyValueError::new_err("Expected HxWx3 RGB image"));
    }

    let (h, w) = (shape[0], shape[1]);
    let mut result = ndarray::Array3::zeros((h, w, 1));
    for y in 0..h {
        for x in 0..w {
            let gray = ((array.as_array()[[y, x, 0]] as u16
                + array.as_array()[[y, x, 1]] as u16
                + array.as_array()[[y, x, 2]] as u16)
                / 3) as u8;
            result[[y, x, 0]] = gray;
        }
    }

    Ok(PyArray3::from_owned_array(py, result))
}

// ---------------------------------------------------------------------------
// template_match — find all template matches above a threshold
// ---------------------------------------------------------------------------

/// Find all occurrences of *template* in *image* with confidence >= *threshold*.
///
/// Both images are converted to grayscale internally before matching.
/// Returns a list of ``(x, y, confidence)`` tuples.
#[pyfunction]
fn template_match<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    template: &Bound<'_, PyArray3<u8>>,
    threshold: f64,
) -> PyResult<Vec<Bound<'py, PyTuple>>> {
    let frame = ndarray_to_frame(image)?;
    let tpl = ndarray_to_frame(template)?;

    let gray_frame = ensure_grayscale(&frame)?;
    let gray_tpl = ensure_grayscale(&tpl)?;

    let matches = CoreImageProcessor::template_match(&gray_frame, &gray_tpl, threshold)
        .map_err(|e| PyValueError::new_err(format!("Template matching failed: {e}")))?;

    let mut result: Vec<Bound<'py, PyTuple>> = Vec::with_capacity(matches.len());
    for m in matches {
        let items: Vec<pyo3::PyObject> = vec![
            (m.point.x as i32).into_pyobject(py)?.unbind().into(),
            (m.point.y as i32).into_pyobject(py)?.unbind().into(),
            m.confidence.into_pyobject(py)?.unbind().into(),
        ];
        let tup = PyTuple::new(py, items)?;
        result.push(tup);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// template_match_best — single best template match (highest confidence)
// ---------------------------------------------------------------------------

/// Find the single best match of *template* in *image*.
///
/// Returns ``(x, y, confidence)`` if a match above *threshold* is found,
/// or ``None`` otherwise.
#[pyfunction]
fn template_match_best<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    template: &Bound<'_, PyArray3<u8>>,
    threshold: f64,
) -> PyResult<Option<Bound<'py, PyTuple>>> {
    let frame = ndarray_to_frame(image)?;
    let tpl = ndarray_to_frame(template)?;

    let gray_frame = ensure_grayscale(&frame)?;
    let gray_tpl = ensure_grayscale(&tpl)?;

    match CoreImageProcessor::template_match_best(&gray_frame, &gray_tpl, threshold) {
        Ok(m) => {
            let items: Vec<pyo3::PyObject> = vec![
                (m.point.x as i32).into_pyobject(py)?.unbind().into(),
                (m.point.y as i32).into_pyobject(py)?.unbind().into(),
                m.confidence.into_pyobject(py)?.unbind().into(),
            ];
            let tup = PyTuple::new(py, items)?;
            Ok(Some(tup))
        }
        Err(_) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// in_range — color range detection (BGR)
// ---------------------------------------------------------------------------

/// Detect pixels within a BGR color range.
///
/// *lower* and *upper* are 3-element lists/tuples ``[B, G, R]``.
/// Returns a binary mask (HxWx1, values 0 or 255).
#[pyfunction]
fn in_range<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    if lower.len() != 3 || upper.len() != 3 {
        return Err(PyValueError::new_err(
            "lower and upper must have exactly 3 elements [B, G, R]",
        ));
    }
    let lower_arr: [u8; 3] = [lower[0], lower[1], lower[2]];
    let upper_arr: [u8; 3] = [upper[0], upper[1], upper[2]];

    let frame = ndarray_to_frame(image)?;

    // The core's in_range uses BGR ordering; if the frame is RGB we swap
    let in_range_frame = if frame.format == PixelFormat::Rgb {
        // Swap channel order: Frame uses data in native order, in_range
        // interprets it as BGR.  Since our data is actually RGB we pass a
        // Bgr-tagged frame by swapping lower/upper B<->R channels.
        let swapped_lower = [lower_arr[2], lower_arr[1], lower_arr[0]];
        let swapped_upper = [upper_arr[2], upper_arr[1], upper_arr[0]];
        CoreImageProcessor::in_range(&frame, swapped_lower, swapped_upper)
    } else {
        CoreImageProcessor::in_range(&frame, lower_arr, upper_arr)
    };

    let result =
        in_range_frame.map_err(|e| PyValueError::new_err(format!("in_range failed: {e}")))?;

    Ok(frame_to_ndarray(py, &result))
}

// ---------------------------------------------------------------------------
// threshold — binary threshold
// ---------------------------------------------------------------------------

/// Apply a binary threshold to a grayscale image.
///
/// Pixels >= *value* become 255, others 0. The input is converted to
/// grayscale automatically if it has multiple channels.
/// Returns a binary mask (HxWx1).
#[pyfunction]
fn threshold<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    value: u8,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let frame = ndarray_to_frame(image)?;
    let gray = ensure_grayscale(&frame)?;

    let result = CoreImageProcessor::threshold(&gray, value)
        .map_err(|e| PyValueError::new_err(format!("Threshold failed: {e}")))?;

    Ok(frame_to_ndarray(py, &result))
}

// ---------------------------------------------------------------------------
// preprocess — full preprocessing pipeline
// ---------------------------------------------------------------------------

/// Apply the full preprocessing pipeline:
/// optional crop → grayscale/binarize → binary threshold.
///
/// Parameters (all optional keyword-only):
///   * ``crop_region``: ``(x, y, w, h)`` — crop rectangle
///   * ``grayscale``: ``bool`` — convert to grayscale (default: ``False``)
///   * ``lower``, ``upper``: ``[B, G, R]`` — in-range binarization (overrides grayscale)
///   * ``threshold``: ``int`` — binary threshold value
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn preprocess<'py>(
    py: Python<'py>,
    image: &Bound<'_, PyArray3<u8>>,
    crop_region: Option<(u32, u32, u32, u32)>,
    grayscale: Option<bool>,
    lower: Option<Vec<u8>>,
    upper: Option<Vec<u8>>,
    threshold_val: Option<u8>,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let mut frame = ndarray_to_frame(image)?;

    // Step 1: optional crop
    if let Some((x, y, w, h)) = crop_region {
        let region = pokecon_core::cv::image_processing::Region {
            x,
            y,
            width: w,
            height: h,
        };
        frame = CoreImageProcessor::crop(&frame, &region)
            .map_err(|e| PyValueError::new_err(format!("Crop failed: {e}")))?;
    }

    // Step 2: grayscale or binarize
    if let (Some(lower_vec), Some(upper_vec)) = (&lower, &upper) {
        if lower_vec.len() != 3 || upper_vec.len() != 3 {
            return Err(PyValueError::new_err(
                "lower and upper must have exactly 3 elements [B, G, R]",
            ));
        }
        let lower_arr: [u8; 3] = [lower_vec[0], lower_vec[1], lower_vec[2]];
        let upper_arr: [u8; 3] = [upper_vec[0], upper_vec[1], upper_vec[2]];
        if frame.format == PixelFormat::Rgb {
            let swapped_lower = [lower_arr[2], lower_arr[1], lower_arr[0]];
            let swapped_upper = [upper_arr[2], upper_arr[1], upper_arr[0]];
            frame = CoreImageProcessor::in_range(&frame, swapped_lower, swapped_upper)
                .map_err(|e| PyValueError::new_err(format!("in_range failed: {e}")))?;
        } else {
            frame = CoreImageProcessor::in_range(&frame, lower_arr, upper_arr)
                .map_err(|e| PyValueError::new_err(format!("in_range failed: {e}")))?;
        }
    } else if grayscale.unwrap_or(false) {
        frame = ensure_grayscale(&frame)?;
    }

    // Step 3: optional binary threshold
    if let Some(t) = threshold_val {
        frame = CoreImageProcessor::threshold(&frame, t)
            .map_err(|e| PyValueError::new_err(format!("Threshold failed: {e}")))?;
    }

    Ok(frame_to_ndarray(py, &frame))
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(crop, m)?)?;
    m.add_function(wrap_pyfunction!(grayscale, m)?)?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(save, m)?)?;
    m.add_function(wrap_pyfunction!(resize, m)?)?;
    m.add_function(wrap_pyfunction!(template_match, m)?)?;
    m.add_function(wrap_pyfunction!(template_match_best, m)?)?;
    m.add_function(wrap_pyfunction!(in_range, m)?)?;
    m.add_function(wrap_pyfunction!(threshold, m)?)?;
    m.add_function(wrap_pyfunction!(preprocess, m)?)?;
    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- crop tests (existing) ----

    #[test]
    fn test_crop_valid() {
        Python::with_gil(|py| {
            let shape = [4usize, 4, 3];
            let data: Vec<u8> = (0..48).map(|i| i as u8).collect();
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let cropped = crop(py, &array.as_borrowed(), 1, 1, 2, 2).unwrap();
            let cropped_shape = cropped.shape();
            assert_eq!(cropped_shape[0], 2);
            assert_eq!(cropped_shape[1], 2);
            assert_eq!(cropped_shape[2], 3);
        });
    }

    #[test]
    fn test_crop_out_of_bounds() {
        Python::with_gil(|py| {
            let shape = [4usize, 4, 3];
            let data = vec![0u8; 48];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let result = crop(py, &array.as_borrowed(), 3, 3, 5, 5);
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_crop_invalid_format() {
        Python::with_gil(|py| {
            let shape = [4usize, 4, 1];
            let data = vec![0u8; 16];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let result = crop(py, &array.as_borrowed(), 0, 0, 2, 2);
            assert!(result.is_err());
        });
    }

    // ---- grayscale tests (existing) ----

    #[test]
    fn test_grayscale_valid() {
        Python::with_gil(|py| {
            let shape = [2usize, 2, 3];
            let data: Vec<u8> = vec![10, 20, 30, 40, 50, 60, 100, 150, 200, 200, 210, 220];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let gray = grayscale(py, &array.as_borrowed()).unwrap();
            let gray_shape = gray.shape();
            assert_eq!(gray_shape[0], 2);
            assert_eq!(gray_shape[1], 2);
            assert_eq!(gray_shape[2], 1);

            let gray_data = gray.readonly();
            assert_eq!(gray_data.as_array()[[0, 0, 0]], 20);
        });
    }

    #[test]
    fn test_grayscale_invalid_format() {
        Python::with_gil(|py| {
            let shape = [4usize, 4, 1];
            let data = vec![0u8; 16];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let result = grayscale(py, &array.as_borrowed());
            assert!(result.is_err());
        });
    }

    // ---- resize tests ----

    #[test]
    fn test_resize_downscale() {
        Python::with_gil(|py| {
            let shape = [10usize, 10, 3];
            let data: Vec<u8> = (0..300).map(|i| i as u8).collect();
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let resized = resize(py, &array.as_borrowed(), 5, 5).unwrap();
            let rshape = resized.shape();
            assert_eq!(rshape[0], 5);
            assert_eq!(rshape[1], 5);
            assert_eq!(rshape[2], 3);
        });
    }

    #[test]
    fn test_resize_upscale() {
        Python::with_gil(|py| {
            let shape = [2usize, 2, 3];
            let data: Vec<u8> = (0..12).map(|i| i as u8).collect();
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let resized = resize(py, &array.as_borrowed(), 4, 4).unwrap();
            let rshape = resized.shape();
            assert_eq!(rshape[0], 4);
            assert_eq!(rshape[1], 4);
            assert_eq!(rshape[2], 3);
        });
    }

    #[test]
    fn test_resize_zero_dimension() {
        Python::with_gil(|py| {
            let shape = [4usize, 4, 3];
            let data = vec![0u8; 48];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let result = resize(py, &array.as_borrowed(), 0, 4);
            assert!(result.is_err());
        });
    }

    // ---- template_match tests ----

    #[test]
    fn test_template_match_identical() {
        Python::with_gil(|py| {
            // Create a 10x10 RGB image where the top-left 5x5 is all 255
            let mut data = vec![0u8; 10 * 10 * 3];
            for y in 0..5 {
                for x in 0..5 {
                    let idx = (y * 10 + x) * 3;
                    data[idx] = 255;
                    data[idx + 1] = 255;
                    data[idx + 2] = 255;
                }
            }
            let shape = [10usize, 10, 3];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            // Create a 3x3 template from the same white region
            let mut tpl_data = vec![0u8; 3 * 3 * 3];
            for y in 0..3 {
                for x in 0..3 {
                    let idx = (y * 3 + x) * 3;
                    tpl_data[idx] = 255;
                    tpl_data[idx + 1] = 255;
                    tpl_data[idx + 2] = 255;
                }
            }
            let tpl_shape = [3usize, 3, 3];
            let tpl_array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(tpl_shape, tpl_data).unwrap(),
            );

            let matches =
                template_match(py, &array.as_borrowed(), &tpl_array.as_borrowed(), 0.95).unwrap();
            assert!(!matches.is_empty(), "Expected at least one match");
        });
    }

    #[test]
    fn test_template_match_no_match() {
        Python::with_gil(|py| {
            let image_data = vec![0u8; 10 * 10 * 3];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([10usize, 10, 3], image_data).unwrap(),
            );

            let tpl_data = vec![255u8; 3 * 3 * 3];
            let tpl_array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([3usize, 3, 3], tpl_data).unwrap(),
            );

            let matches =
                template_match(py, &array.as_borrowed(), &tpl_array.as_borrowed(), 0.99).unwrap();
            assert!(matches.is_empty(), "Expected no matches above 0.99");
        });
    }

    // ---- template_match_best tests ----

    #[test]
    fn test_template_match_best_found() {
        Python::with_gil(|py| {
            // Image: 20x20 with a 5x5 white block at (5,5)
            let mut img_data = vec![0u8; 20 * 20 * 3];
            for y in 5..10 {
                for x in 5..10 {
                    let idx = (y * 20 + x) * 3;
                    img_data[idx] = 255;
                    img_data[idx + 1] = 255;
                    img_data[idx + 2] = 255;
                }
            }
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([20usize, 20, 3], img_data).unwrap(),
            );

            // Template: 3x3 white
            let tpl_data = vec![255u8; 3 * 3 * 3];
            let tpl_array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([3usize, 3, 3], tpl_data).unwrap(),
            );

            let result =
                template_match_best(py, &array.as_borrowed(), &tpl_array.as_borrowed(), 0.9)
                    .unwrap();
            assert!(result.is_some(), "Expected a match");

            let tup = result.unwrap();
            let x: i32 = tup.get_item(0).unwrap().extract().unwrap();
            let y: i32 = tup.get_item(1).unwrap().extract().unwrap();
            assert!(x >= 5 && x < 7, "Expected x near 5, got {x}");
            assert!(y >= 5 && y < 7, "Expected y near 5, got {y}");
        });
    }

    #[test]
    fn test_template_match_best_not_found() {
        Python::with_gil(|py| {
            let img_data = vec![0u8; 10 * 10 * 3];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([10usize, 10, 3], img_data).unwrap(),
            );

            let tpl_data = vec![255u8; 3 * 3 * 3];
            let tpl_array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([3usize, 3, 3], tpl_data).unwrap(),
            );

            let result =
                template_match_best(py, &array.as_borrowed(), &tpl_array.as_borrowed(), 0.99)
                    .unwrap();
            assert!(result.is_none(), "Expected no match");
        });
    }

    // ---- in_range tests ----

    #[test]
    fn test_in_range_rgb() {
        Python::with_gil(|py| {
            // Create a 2x1 RGB image: pixel0=(10,20,30), pixel1=(200,210,220)
            let data = vec![10u8, 20, 30, 200, 210, 220];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([1usize, 2, 3], data).unwrap(),
            );

            // Detect pixels where R in [0,100], G in [0,100], B in [0,100]
            // Since data is RGB and in_range expects BGR, we pass lower/upper as [B,G,R]
            // The in_range function will detect: data's R channel compared to lower[2], etc.
            // Actually the function handles RGB->BGR swap internally.
            let result =
                in_range(py, &array.as_borrowed(), vec![0, 0, 0], vec![100, 100, 100]).unwrap();
            let rshape = result.shape();
            assert_eq!(rshape[0], 1);
            assert_eq!(rshape[1], 2);
            assert_eq!(rshape[2], 1);

            let rdata = result.readonly();
            // First pixel (R=10,G=20,B=30) is within [0,100] range -> 255
            assert_eq!(rdata.as_array()[[0, 0, 0]], 255);
            // Second pixel (R=200,G=210,B=220) is outside -> 0
            assert_eq!(rdata.as_array()[[0, 1, 0]], 0);
        });
    }

    #[test]
    fn test_in_range_invalid_lower_upper_length() {
        Python::with_gil(|py| {
            let data = vec![0u8; 12];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([2usize, 2, 3], data).unwrap(),
            );

            let result = in_range(py, &array.as_borrowed(), vec![0, 0], vec![255, 255, 255]);
            assert!(result.is_err());
        });
    }

    // ---- threshold tests ----

    #[test]
    fn test_threshold_rgb() {
        Python::with_gil(|py| {
            // 2x1 RGB: pixel values (50, 150, 200)
            let data = vec![50u8, 150, 200];
            let shape = [1usize, 1, 3];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec(shape, data).unwrap(),
            );

            let result = threshold(py, &array.as_borrowed(), 128).unwrap();
            let rshape = result.shape();
            assert_eq!(rshape[0], 1);
            assert_eq!(rshape[1], 1);
            assert_eq!(rshape[2], 1);

            let rdata = result.readonly();
            // Grayscale = (50+150+200)/3 = 133, which is >= 128 -> 255
            assert_eq!(rdata.as_array()[[0, 0, 0]], 255);
        });
    }

    // ---- load / save tests (round-trip via temp files) ----

    #[test]
    fn test_save_and_load_roundtrip() {
        Python::with_gil(|py| {
            let dir = tempfile::TempDir::new().unwrap();
            let path = dir.path().join("test_roundtrip.png");
            let path_str = path.to_str().unwrap().to_string();

            // Create a 4x4 RGB gradient
            let mut data = Vec::with_capacity(4 * 4 * 3);
            for y in 0..4 {
                for x in 0..4 {
                    data.push((y * 64) as u8);
                    data.push((x * 64) as u8);
                    data.push(128u8);
                }
            }
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([4usize, 4, 3], data).unwrap(),
            );

            // Save
            save(&path_str, &array.as_borrowed()).unwrap();

            // Load back
            let loaded = load(py, &path_str).unwrap();
            let lshape = loaded.shape();
            assert_eq!(lshape[0], 4);
            assert_eq!(lshape[1], 4);
            assert_eq!(lshape[2], 3);

            // Verify pixel data matches
            let original = array.readonly();
            let loaded_r = loaded.readonly();
            for y in 0..4 {
                for x in 0..4 {
                    for c in 0..3 {
                        assert_eq!(
                            original.as_array()[[y, x, c]],
                            loaded_r.as_array()[[y, x, c]],
                            "Mismatch at ({y},{x},{c})"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn test_load_nonexistent() {
        Python::with_gil(|py| {
            let result = load(py, "/nonexistent/path/to/image.png");
            assert!(result.is_err());
        });
    }

    // ---- preprocess tests ----

    #[test]
    fn test_preprocess_grayscale_only() {
        Python::with_gil(|py| {
            let data = vec![10u8, 20, 30, 40, 50, 60];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([1usize, 2, 3], data).unwrap(),
            );

            let result = preprocess(
                py,
                &array.as_borrowed(),
                None,
                Some(true),
                None::<Vec<u8>>,
                None::<Vec<u8>>,
                None,
            )
            .unwrap();
            let rshape = result.shape();
            assert_eq!(rshape[2], 1);
        });
    }

    #[test]
    fn test_preprocess_crop_and_threshold() {
        Python::with_gil(|py| {
            let data = vec![100u8; 4 * 4 * 3];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([4usize, 4, 3], data).unwrap(),
            );

            let result = preprocess(
                py,
                &array.as_borrowed(),
                Some((0, 0, 2, 2)),
                Some(true),
                None::<Vec<u8>>,
                None::<Vec<u8>>,
                Some(50u8),
            )
            .unwrap();
            let rshape = result.shape();
            assert_eq!(rshape[0], 2);
            assert_eq!(rshape[1], 2);
            assert_eq!(rshape[2], 1);
        });
    }

    #[test]
    fn test_preprocess_binarize() {
        Python::with_gil(|py| {
            let data = vec![10u8, 20, 30, 200, 210, 220];
            let array = numpy::PyArray3::from_owned_array(
                py,
                ndarray::Array3::from_shape_vec([1usize, 2, 3], data).unwrap(),
            );

            let result = preprocess(
                py,
                &array.as_borrowed(),
                None,
                None,
                Some(vec![0u8, 0, 0]),
                Some(vec![100u8, 100, 100]),
                None,
            )
            .unwrap();
            let rshape = result.shape();
            assert_eq!(rshape[2], 1);

            let rdata = result.readonly();
            // First pixel (R=10,G=20,B=30) in range -> 255
            assert_eq!(rdata.as_array()[[0, 0, 0]], 255);
            // Second pixel outside -> 0
            assert_eq!(rdata.as_array()[[0, 1, 0]], 0);
        });
    }
}
