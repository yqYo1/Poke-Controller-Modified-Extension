use pokecon_core::serial::keys::{
    Button as RustButton, Direction as RustDirection, Hat as RustHat, Stick as RustStick,
    Touchscreen as RustTouchscreen,
};
use pyo3::prelude::*;

// ============================================================
// Button
// ============================================================

/// Bitmask of controller buttons. Supports bitwise OR to combine buttons.
///
/// Python usage::
///
///     from pokecon.keys import Button
///     btn = Button.A | Button.B
///     assert bool(btn)
///     assert btn.bits() == 0x0006
#[pyclass(eq)]
#[derive(Clone, PartialEq)]
pub struct PyButton {
    pub(crate) inner: RustButton,
}

#[pymethods]
#[allow(non_snake_case)]
impl PyButton {
    #[new]
    fn new(value: u16) -> Self {
        Self {
            inner: RustButton::from_bits_truncate(value),
        }
    }

    /// Return the raw bitmask value.
    fn bits(&self) -> u16 {
        self.inner.bits()
    }

    fn __or__(&self, other: &Self) -> Self {
        Self {
            inner: self.inner | other.inner,
        }
    }

    fn __and__(&self, other: &Self) -> Self {
        Self {
            inner: self.inner & other.inner,
        }
    }

    fn __invert__(&self) -> Self {
        Self {
            inner: RustButton::from_bits_truncate(!self.inner.bits()),
        }
    }

    fn __repr__(&self) -> String {
        format!("<Button {:#06x}>", self.inner.bits())
    }

    fn __hash__(&self) -> u64 {
        self.inner.bits() as u64
    }

    fn __bool__(&self) -> bool {
        self.inner.bits() != 0
    }

    // --- Named button constants as class attributes ---

    #[classattr]
    fn Y() -> Self {
        Self {
            inner: RustButton::Y,
        }
    }

    #[classattr]
    fn B() -> Self {
        Self {
            inner: RustButton::B,
        }
    }

    #[classattr]
    fn A() -> Self {
        Self {
            inner: RustButton::A,
        }
    }

    #[classattr]
    fn X() -> Self {
        Self {
            inner: RustButton::X,
        }
    }

    #[classattr]
    fn L() -> Self {
        Self {
            inner: RustButton::L,
        }
    }

    #[classattr]
    fn R() -> Self {
        Self {
            inner: RustButton::R,
        }
    }

    #[classattr]
    fn ZL() -> Self {
        Self {
            inner: RustButton::ZL,
        }
    }

    #[classattr]
    fn ZR() -> Self {
        Self {
            inner: RustButton::ZR,
        }
    }

    #[classattr]
    fn MINUS() -> Self {
        Self {
            inner: RustButton::MINUS,
        }
    }

    #[classattr]
    fn PLUS() -> Self {
        Self {
            inner: RustButton::PLUS,
        }
    }

    #[classattr]
    fn LCLICK() -> Self {
        Self {
            inner: RustButton::LCLICK,
        }
    }

    #[classattr]
    fn RCLICK() -> Self {
        Self {
            inner: RustButton::RCLICK,
        }
    }

    #[classattr]
    fn HOME() -> Self {
        Self {
            inner: RustButton::HOME,
        }
    }

    #[classattr]
    fn CAPTURE() -> Self {
        Self {
            inner: RustButton::CAPTURE,
        }
    }
}

// ============================================================
// Hat (D-Pad)
// ============================================================

/// D-Pad hat direction.
///
/// Python usage::
///
///     from pokecon.keys import Hat
///     h = Hat.TOP
///     assert h.value() == 0
#[pyclass(eq)]
#[derive(Clone, PartialEq)]
pub struct PyHat {
    pub(crate) inner: RustHat,
}

#[pymethods]
#[allow(non_snake_case)]
impl PyHat {
    #[new]
    fn new(value: u8) -> Self {
        let inner = match value {
            0 => RustHat::TOP,
            1 => RustHat::TOP_RIGHT,
            2 => RustHat::RIGHT,
            3 => RustHat::BTM_RIGHT,
            4 => RustHat::BTM,
            5 => RustHat::BTM_LEFT,
            6 => RustHat::LEFT,
            7 => RustHat::TOP_LEFT,
            _ => RustHat::CENTER,
        };
        Self { inner }
    }

    /// Return the raw integer value of the hat direction.
    fn value(&self) -> u8 {
        self.inner as u8
    }

    fn __repr__(&self) -> String {
        format!("<Hat {:?}>", self.inner)
    }

    #[classattr]
    fn TOP() -> Self {
        Self {
            inner: RustHat::TOP,
        }
    }

    #[classattr]
    fn TOP_RIGHT() -> Self {
        Self {
            inner: RustHat::TOP_RIGHT,
        }
    }

    #[classattr]
    fn RIGHT() -> Self {
        Self {
            inner: RustHat::RIGHT,
        }
    }

    #[classattr]
    fn BTM_RIGHT() -> Self {
        Self {
            inner: RustHat::BTM_RIGHT,
        }
    }

    #[classattr]
    fn BTM() -> Self {
        Self {
            inner: RustHat::BTM,
        }
    }

    #[classattr]
    fn BTM_LEFT() -> Self {
        Self {
            inner: RustHat::BTM_LEFT,
        }
    }

    #[classattr]
    fn LEFT() -> Self {
        Self {
            inner: RustHat::LEFT,
        }
    }

    #[classattr]
    fn TOP_LEFT() -> Self {
        Self {
            inner: RustHat::TOP_LEFT,
        }
    }

    #[classattr]
    fn CENTER() -> Self {
        Self {
            inner: RustHat::CENTER,
        }
    }
}

// ============================================================
// Stick
// ============================================================

/// Controller stick identifier (Left or Right).
///
/// Python usage::
///
///     from pokecon.keys import Stick
///     s = Stick.LEFT
///     s = Stick.RIGHT
#[pyclass(eq)]
#[derive(Clone, PartialEq)]
pub struct PyStick {
    pub(crate) inner: RustStick,
}

#[pymethods]
#[allow(non_snake_case)]
impl PyStick {
    #[new]
    fn new(name: &str) -> PyResult<Self> {
        let inner = match name {
            "Left" => RustStick::Left,
            "Right" => RustStick::Right,
            _ => {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "Unknown stick: '{}'. Use 'Left' or 'Right'.",
                    name
                )));
            }
        };
        Ok(Self { inner })
    }

    fn __repr__(&self) -> String {
        format!("<Stick {:?}>", self.inner)
    }

    #[classattr]
    fn LEFT() -> Self {
        Self {
            inner: RustStick::Left,
        }
    }

    #[classattr]
    fn RIGHT() -> Self {
        Self {
            inner: RustStick::Right,
        }
    }
}

// ============================================================
// Direction (stick tilt)
// ============================================================

/// Directional stick input with x/y coordinates.
///
/// Python usage::
///
///     from pokecon.keys import Direction, Stick
///     d = Direction.from_xy(Stick.LEFT, 128, 128)
///     assert d.x == 128
///     assert d.y == 128
///
///     d = Direction.from_angle(Stick.RIGHT, 90.0, 1.0)
#[pyclass]
#[derive(Clone)]
pub struct PyDirection {
    pub(crate) inner: RustDirection,
}

#[pymethods]
impl PyDirection {
    /// Create a Direction from an angle (degrees) and magnitude (0..1).
    #[staticmethod]
    fn from_angle(stick: &PyStick, angle_deg: f64, magnification: f64) -> Self {
        Self {
            inner: RustDirection::from_angle(stick.inner, angle_deg, magnification),
        }
    }

    /// Create a Direction from raw x (0..255) and y (0..255) coordinates.
    #[staticmethod]
    fn from_xy(stick: &PyStick, x: u8, y: u8) -> Self {
        Self {
            inner: RustDirection::from_xy(stick.inner, x, y),
        }
    }

    /// Create a direction pointing straight up.
    #[staticmethod]
    fn up(stick: &PyStick) -> Self {
        Self {
            inner: RustDirection::up(stick.inner),
        }
    }

    /// Create a direction pointing straight down.
    #[staticmethod]
    fn down(stick: &PyStick) -> Self {
        Self {
            inner: RustDirection::down(stick.inner),
        }
    }

    /// Create a direction pointing straight left.
    #[staticmethod]
    fn left(stick: &PyStick) -> Self {
        Self {
            inner: RustDirection::left(stick.inner),
        }
    }

    /// Create a direction pointing straight right.
    #[staticmethod]
    fn right(stick: &PyStick) -> Self {
        Self {
            inner: RustDirection::right(stick.inner),
        }
    }

    /// X coordinate (0..255, center=128).
    #[getter]
    fn x(&self) -> u8 {
        self.inner.x
    }

    /// Y coordinate (0..255, center=128).
    #[getter]
    fn y(&self) -> u8 {
        self.inner.y
    }

    /// Which stick this direction applies to.
    #[getter]
    fn stick(&self) -> PyStick {
        PyStick {
            inner: self.inner.stick,
        }
    }

    /// Optional human-readable name for this direction.
    #[getter]
    fn show_name(&self) -> Option<String> {
        self.inner.show_name.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Direction {:?} x={} y={}>",
            self.inner.stick, self.inner.x, self.inner.y
        )
    }
}

// ============================================================
// Touchscreen
// ============================================================

/// Touchscreen press position.
///
/// Python usage::
///
///     from pokecon.keys import Touchscreen
///     ts = Touchscreen(100, 50)
///     assert ts.x == 100
///     assert ts.y == 50
#[pyclass]
#[derive(Clone, Copy)]
pub struct PyTouchscreen {
    pub(crate) inner: RustTouchscreen,
}

#[pymethods]
impl PyTouchscreen {
    #[new]
    fn new(x: u16, y: u8) -> Self {
        Self {
            inner: RustTouchscreen::new(x, y),
        }
    }

    /// X coordinate of the touch.
    #[getter]
    fn x(&self) -> u16 {
        self.inner.x
    }

    /// Y coordinate of the touch.
    #[getter]
    fn y(&self) -> u8 {
        self.inner.y
    }

    fn __repr__(&self) -> String {
        format!("<Touchscreen x={} y={}>", self.inner.x, self.inner.y)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner.x == other.inner.x && self.inner.y == other.inner.y
    }
}

// ============================================================
// Legacy helper functions
// ============================================================

/// Convert a button bitmask to a list of human-readable ``Button[N]`` strings.
#[pyfunction]
fn convert_button(button: u16) -> PyResult<Vec<String>> {
    let buttons: Vec<String> = (0..16)
        .filter(|x| (button >> x) & 1 == 1)
        .map(|x| format!("Button[{}]", x))
        .collect();
    Ok(buttons)
}

/// Convert a hat index (0..8) to the human-readable direction name.
#[pyfunction]
fn get_direction(hat_idx: u8) -> PyResult<String> {
    let names = [
        "TOP",
        "TOP_RIGHT",
        "RIGHT",
        "BTM_RIGHT",
        "BTM",
        "BTM_LEFT",
        "LEFT",
        "TOP_LEFT",
        "CENTER",
    ];
    let name = names.get(hat_idx as usize).unwrap_or(&"CENTER");
    Ok(name.to_string())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyButton>()?;
    m.add_class::<PyHat>()?;
    m.add_class::<PyStick>()?;
    m.add_class::<PyDirection>()?;
    m.add_class::<PyTouchscreen>()?;
    m.add_function(wrap_pyfunction!(convert_button, m)?)?;
    m.add_function(wrap_pyfunction!(get_direction, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PyButton tests (Rust-side only, no GIL) ─────────────────────────

    #[test]
    fn test_pybutton_bits() {
        let btn = PyButton::new(0x0004);
        assert_eq!(btn.bits(), 0x0004);
    }

    #[test]
    fn test_pybutton_repr() {
        let btn = PyButton::new(0x0004);
        assert_eq!(btn.__repr__(), "<Button 0x0004>");
    }

    #[test]
    fn test_pybutton_or() {
        let a = PyButton::new(0x0004);
        let b = PyButton::new(0x0002);
        let result = a.__or__(&b);
        assert_eq!(result.bits(), 0x0006);
    }

    #[test]
    fn test_pybutton_and() {
        let ab = PyButton::new(0x0006);
        let a = PyButton::new(0x0004);
        let result = ab.__and__(&a);
        assert_eq!(result.bits(), 0x0004);
    }

    #[test]
    fn test_pybutton_invert() {
        let btn = PyButton::new(0x0004);
        let inv = btn.__invert__();
        assert_eq!(inv.bits(), !0x0004);
    }

    #[test]
    fn test_pybutton_bool() {
        let zero = PyButton::new(0x0000);
        let non_zero = PyButton::new(0x0004);
        assert!(!zero.__bool__());
        assert!(non_zero.__bool__());
    }

    #[test]
    fn test_pybutton_hash() {
        let btn = PyButton::new(0x0004);
        assert_eq!(btn.__hash__(), 0x0004);
    }

    #[test]
    fn test_pybutton_class_constants() {
        assert_eq!(PyButton::A().inner, RustButton::A);
        assert_eq!(PyButton::B().inner, RustButton::B);
        assert_eq!(PyButton::X().inner, RustButton::X);
        assert_eq!(PyButton::Y().inner, RustButton::Y);
        assert_eq!(PyButton::L().inner, RustButton::L);
        assert_eq!(PyButton::R().inner, RustButton::R);
        assert_eq!(PyButton::ZL().inner, RustButton::ZL);
        assert_eq!(PyButton::ZR().inner, RustButton::ZR);
        assert_eq!(PyButton::MINUS().inner, RustButton::MINUS);
        assert_eq!(PyButton::PLUS().inner, RustButton::PLUS);
        assert_eq!(PyButton::LCLICK().inner, RustButton::LCLICK);
        assert_eq!(PyButton::RCLICK().inner, RustButton::RCLICK);
        assert_eq!(PyButton::HOME().inner, RustButton::HOME);
        assert_eq!(PyButton::CAPTURE().inner, RustButton::CAPTURE);
    }

    // ── PyHat tests ───────────────────────────────────────────────────

    #[test]
    fn test_pyhat_value() {
        let hat = PyHat::new(0);
        assert_eq!(hat.value(), 0);
        assert_eq!(hat.__repr__(), "<Hat TOP>");
    }

    #[test]
    fn test_pyhat_class_constants() {
        assert_eq!(PyHat::TOP().inner, RustHat::TOP);
        assert_eq!(PyHat::CENTER().inner, RustHat::CENTER);
        assert_eq!(PyHat::LEFT().inner, RustHat::LEFT);
        assert_eq!(PyHat::RIGHT().inner, RustHat::RIGHT);
        assert_eq!(PyHat::BTM().inner, RustHat::BTM);
    }

    #[test]
    fn test_pyhat_unknown_maps_to_center() {
        let hat = PyHat::new(99);
        assert_eq!(hat.inner, RustHat::CENTER);
    }

    // ── PyStick tests ─────────────────────────────────────────────────

    #[test]
    fn test_pystick_new() {
        let left = PyStick::new("Left").unwrap();
        assert_eq!(left.inner, RustStick::Left);
        let right = PyStick::new("Right").unwrap();
        assert_eq!(right.inner, RustStick::Right);
    }

    #[test]
    fn test_pystick_invalid() {
        assert!(PyStick::new("Invalid").is_err());
    }

    #[test]
    fn test_pystick_class_constants() {
        assert_eq!(PyStick::LEFT().inner, RustStick::Left);
        assert_eq!(PyStick::RIGHT().inner, RustStick::Right);
    }

    #[test]
    fn test_pystick_repr() {
        let left = PyStick::new("Left").unwrap();
        assert_eq!(left.__repr__(), "<Stick Left>");
    }

    // ── PyDirection tests ─────────────────────────────────────────────

    #[test]
    fn test_pydirection_from_xy() {
        let left = PyStick::new("Left").unwrap();
        let dir = PyDirection::from_xy(&left, 200, 50);
        assert_eq!(dir.x(), 200);
        assert_eq!(dir.y(), 50);
        assert_eq!(dir.stick().inner, RustStick::Left);
    }

    #[test]
    fn test_pydirection_from_angle() {
        let right = PyStick::new("Right").unwrap();
        let dir = PyDirection::from_angle(&right, 90.0, 1.0);
        assert_eq!(dir.y(), 255); // sin(90)*127.5+127.5 = 255
    }

    #[test]
    fn test_pydirection_static_methods() {
        let left = PyStick::new("Left").unwrap();
        let d = PyDirection::up(&left);
        assert_eq!(d.y(), 255);

        let d = PyDirection::down(&left);
        assert_eq!(d.y(), 0);

        let d = PyDirection::left(&left);
        assert_eq!(d.x(), 0);

        let d = PyDirection::right(&left);
        assert_eq!(d.x(), 255);

        // Right stick variants
        let right = PyStick::new("Right").unwrap();
        let d = PyDirection::up(&right);
        assert_eq!(d.y(), 255);
        let d = PyDirection::down(&right);
        assert_eq!(d.y(), 0);
    }

    #[test]
    fn test_pydirection_repr() {
        let left = PyStick::new("Left").unwrap();
        let dir = PyDirection::from_xy(&left, 128, 128);
        let repr = dir.__repr__();
        assert!(repr.contains("Left"));
        assert!(repr.contains("128"));
    }

    // ── PyTouchscreen tests ───────────────────────────────────────────

    #[test]
    fn test_pytouchscreen_new() {
        let ts = PyTouchscreen::new(500, 200);
        assert_eq!(ts.x(), 500);
        assert_eq!(ts.y(), 200);
    }

    #[test]
    fn test_pytouchscreen_repr() {
        let ts = PyTouchscreen::new(100, 50);
        assert_eq!(ts.__repr__(), "<Touchscreen x=100 y=50>");
    }

    #[test]
    fn test_pytouchscreen_eq() {
        let ts1 = PyTouchscreen::new(100, 50);
        let ts2 = PyTouchscreen::new(100, 50);
        let ts3 = PyTouchscreen::new(200, 100);
        assert!(ts1.__eq__(&ts2));
        assert!(!ts1.__eq__(&ts3));
    }

    // ── Legacy helper function tests ──────────────────────────────────

    #[test]
    fn test_convert_button_function() {
        let result = convert_button(0x0006).unwrap();
        assert_eq!(result, vec!["Button[1]", "Button[2]"]);
    }

    #[test]
    fn test_convert_button_zero() {
        let result = convert_button(0x0000).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_direction_function() {
        assert_eq!(get_direction(0).unwrap(), "TOP");
        assert_eq!(get_direction(4).unwrap(), "BTM");
        assert_eq!(get_direction(8).unwrap(), "CENTER");
        assert_eq!(get_direction(99).unwrap(), "CENTER");
    }

    // ── Python-interop tests (require GIL) ────────────────────────────

    #[test]
    fn test_pybutton_creation_in_python() {
        Python::with_gil(|_py| {
            let _btn = PyButton::new(0x0004);
        });
    }

    #[test]
    fn test_pyhat_creation_in_python() {
        Python::with_gil(|_py| {
            let _hat = PyHat::new(0);
        });
    }
}
