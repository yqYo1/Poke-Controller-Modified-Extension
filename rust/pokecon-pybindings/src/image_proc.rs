use numpy::{PyArray3, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;

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
        return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
            "Expected HxWx3 RGB image",
        ));
    }

    if y + height > shape[0] || x + width > shape[1] {
        return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
            "Crop region out of bounds",
        ));
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

#[pyfunction]
fn grayscale<'py>(
    py: Python<'py>,
    image: &Bound<'py, PyArray3<u8>>,
) -> PyResult<Bound<'py, PyArray3<u8>>> {
    let array = image.readonly();
    let shape = array.shape();
    if shape.len() != 3 || shape[2] != 3 {
        return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
            "Expected HxWx3 RGB image",
        ));
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(crop, m)?)?;
    m.add_function(wrap_pyfunction!(grayscale, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crop_valid() {
        Python::with_gil(|py| {
            // Create a 4x4 RGB image with distinct values
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
            // Create a 2D array (not 3D) — use single-channel 3D shape
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

    #[test]
    fn test_grayscale_valid() {
        Python::with_gil(|py| {
            // Create a 2x2 RGB image
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

            // First pixel: (10+20+30)/3 = 20
            let gray_data = gray.readonly();
            assert_eq!(gray_data.as_array()[[0, 0, 0]], 20);
        });
    }

    #[test]
    fn test_grayscale_invalid_format() {
        Python::with_gil(|py| {
            // 2D array should fail — use single-channel 3D shape
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
}
