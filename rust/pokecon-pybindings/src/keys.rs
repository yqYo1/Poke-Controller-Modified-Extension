use pyo3::prelude::*;
use pokecon_serial::keys::{
    Button as RustButton, Direction as RustDirection, Hat as RustHat, Stick as RustStick,
    Touchscreen as RustTouchscreen,
};

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
