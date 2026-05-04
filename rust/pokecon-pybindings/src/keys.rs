use pyo3::prelude::*;

#[pyfunction]
fn convert_button(button: u16) -> PyResult<Vec<String>> {
    let buttons: Vec<String> = (0..16)
        .filter(|x| (button >> x) & 1 == 1)
        .map(|x| format!("Button[{}]", x))
        .collect();
    Ok(buttons)
}

#[pyfunction]
fn get_direction(hat_idx: u8) -> PyResult<String> {
    let names = [
        "TOP", "TOP_RIGHT", "RIGHT", "BTM_RIGHT", "BTM", "BTM_LEFT", "LEFT", "TOP_LEFT", "CENTER",
    ];
    let name = names.get(hat_idx as usize).unwrap_or(&"CENTER");
    Ok(name.to_string())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert_button, m)?)?;
    m.add_function(wrap_pyfunction!(get_direction, m)?)?;
    Ok(())
}
