use pyo3::prelude::*;
use numpy::{PyArray3, PyArrayMethods, PyUntypedArrayMethods};

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
