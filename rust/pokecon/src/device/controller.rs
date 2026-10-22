//! Canonical controller state and validated partial updates.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Buttons shared by the software controller, scripts, and serial codecs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Button {
    A,
    B,
    X,
    Y,
    L,
    R,
    Zl,
    Zr,
    Minus,
    Plus,
    Home,
    Capture,
    Lclick,
    Rclick,
}

impl Button {
    /// All buttons in stable protocol-bit order.
    pub const ALL: [Self; 14] = [
        Self::Y,
        Self::B,
        Self::A,
        Self::X,
        Self::L,
        Self::R,
        Self::Zl,
        Self::Zr,
        Self::Minus,
        Self::Plus,
        Self::Lclick,
        Self::Rclick,
        Self::Home,
        Self::Capture,
    ];

    /// Bit used by the Switch-compatible serial formats.
    #[must_use]
    pub const fn switch_bit(self) -> u16 {
        match self {
            Self::Y => 1 << 0,
            Self::B => 1 << 1,
            Self::A => 1 << 2,
            Self::X => 1 << 3,
            Self::L => 1 << 4,
            Self::R => 1 << 5,
            Self::Zl => 1 << 6,
            Self::Zr => 1 << 7,
            Self::Minus => 1 << 8,
            Self::Plus => 1 << 9,
            Self::Lclick => 1 << 10,
            Self::Rclick => 1 << 11,
            Self::Home => 1 << 12,
            Self::Capture => 1 << 13,
        }
    }
}

/// Closed, complete button object used by input snapshots.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // The wire contract requires 14 explicit fields.
pub struct ButtonState {
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub l: bool,
    pub r: bool,
    pub zl: bool,
    pub zr: bool,
    pub lclick: bool,
    pub rclick: bool,
    pub plus: bool,
    pub minus: bool,
    pub home: bool,
    pub capture: bool,
}

impl ButtonState {
    /// Returns whether one button is pressed.
    #[must_use]
    pub const fn get(self, button: Button) -> bool {
        match button {
            Button::A => self.a,
            Button::B => self.b,
            Button::X => self.x,
            Button::Y => self.y,
            Button::L => self.l,
            Button::R => self.r,
            Button::Zl => self.zl,
            Button::Zr => self.zr,
            Button::Minus => self.minus,
            Button::Plus => self.plus,
            Button::Home => self.home,
            Button::Capture => self.capture,
            Button::Lclick => self.lclick,
            Button::Rclick => self.rclick,
        }
    }

    /// Changes exactly one button.
    pub fn set(&mut self, button: Button, pressed: bool) {
        match button {
            Button::A => self.a = pressed,
            Button::B => self.b = pressed,
            Button::X => self.x = pressed,
            Button::Y => self.y = pressed,
            Button::L => self.l = pressed,
            Button::R => self.r = pressed,
            Button::Zl => self.zl = pressed,
            Button::Zr => self.zr = pressed,
            Button::Minus => self.minus = pressed,
            Button::Plus => self.plus = pressed,
            Button::Home => self.home = pressed,
            Button::Capture => self.capture = pressed,
            Button::Lclick => self.lclick = pressed,
            Button::Rclick => self.rclick = pressed,
        }
    }

    /// Returns the stable 14-bit Switch mask.
    #[must_use]
    pub fn switch_bits(self) -> u16 {
        Button::ALL
            .into_iter()
            .filter(|button| self.get(*button))
            .fold(0, |bits, button| bits | button.switch_bit())
    }

    /// Returns all pressed buttons.
    #[must_use]
    pub fn pressed(self) -> BTreeSet<Button> {
        Button::ALL
            .into_iter()
            .filter(|button| self.get(*button))
            .collect()
    }

    /// Unions another source's button ownership into this state.
    pub fn union_with(&mut self, other: Self) {
        for button in Button::ALL {
            if other.get(button) {
                self.set(button, true);
            }
        }
    }
}

/// Canonical D-pad state and its wire spelling.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hat {
    Up,
    Down,
    Left,
    Right,
    UpRight,
    UpLeft,
    DownRight,
    DownLeft,
    #[default]
    Neutral,
}

impl Hat {
    /// Switch-compatible numeric hat value.
    #[must_use]
    pub const fn switch_value(self) -> u8 {
        match self {
            Self::Up => 0,
            Self::UpRight => 1,
            Self::Right => 2,
            Self::DownRight => 3,
            Self::Down => 4,
            Self::DownLeft => 5,
            Self::Left => 6,
            Self::UpLeft => 7,
            Self::Neutral => 8,
        }
    }
}

/// One canonical analog stick. Both axes are always in `0..=255`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StickPosition {
    pub x: u8,
    pub y: u8,
}

impl StickPosition {
    pub const CENTER: Self = Self { x: 128, y: 128 };

    /// Treats the documented `103..=153` dead zone as neutral.
    #[must_use]
    pub const fn with_dead_zone(self) -> Self {
        let x = if self.x >= 103 && self.x <= 153 {
            128
        } else {
            self.x
        };
        let y = if self.y >= 103 && self.y <= 153 {
            128
        } else {
            self.y
        };
        Self { x, y }
    }
}

impl Default for StickPosition {
    fn default() -> Self {
        Self::CENTER
    }
}

/// Validated pressed touchscreen coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchPoint {
    x: u16,
    y: u16,
}

impl TouchPoint {
    /// Builds a coordinate in the 320x240 touchscreen area.
    ///
    /// # Errors
    ///
    /// Returns an error for coordinates outside `x=0..319`, `y=0..239`.
    pub const fn new(x: u16, y: u16) -> Result<Self, ControllerError> {
        if x > 319 || y > 239 {
            return Err(ControllerError::TouchOutOfRange { x, y });
        }
        Ok(Self { x, y })
    }

    #[must_use]
    pub const fn x(self) -> u16 {
        self.x
    }

    #[must_use]
    pub const fn y(self) -> u16 {
        self.y
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TouchWire {
    x: u16,
    y: u16,
    pressed: bool,
}

impl Serialize for TouchPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        TouchWire {
            x: self.x,
            y: self.y,
            pressed: true,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TouchPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TouchWire::deserialize(deserializer)?;
        if !wire.pressed {
            return Err(serde::de::Error::custom(
                "snapshot touch.pressed must be true",
            ));
        }
        Self::new(wire.x, wire.y).map_err(serde::de::Error::custom)
    }
}

/// Complete canonical state emitted by the input arbiter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerState {
    pub buttons: ButtonState,
    pub hat: Hat,
    pub left_stick: StickPosition,
    pub right_stick: StickPosition,
    pub touch: Option<TouchPoint>,
}

impl ControllerState {
    pub const NEUTRAL: Self = Self {
        buttons: ButtonState {
            a: false,
            b: false,
            x: false,
            y: false,
            l: false,
            r: false,
            zl: false,
            zr: false,
            lclick: false,
            rclick: false,
            plus: false,
            minus: false,
            home: false,
            capture: false,
        },
        hat: Hat::Neutral,
        left_stick: StickPosition::CENTER,
        right_stick: StickPosition::CENTER,
        touch: None,
    };

    /// Applies a validated dynamic-config or script update.
    ///
    /// # Errors
    ///
    /// Rejects invalid polar or touchscreen values without changing the state.
    pub fn apply(&mut self, update: &ControllerUpdate) -> Result<(), ControllerError> {
        let mut prospective = *self;
        for (button, pressed) in update.button_changes() {
            prospective.buttons.set(button, pressed);
        }
        if let Some(stick) = update.left_stick {
            prospective.left_stick = stick.resolve()?;
        }
        if let Some(stick) = update.right_stick {
            prospective.right_stick = stick.resolve()?;
        }
        if let Some(hat) = update.hat {
            prospective.hat = hat;
        }
        if let Some(touch) = update.touch {
            prospective.touch = touch.resolve()?;
        }
        *self = prospective;
        Ok(())
    }
}

impl Default for ControllerState {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

/// Exact XY or polar stick input; mixed/incomplete JSON shapes are rejected.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StickInput {
    Xy(StickPosition),
    Polar(StickPolar),
}

impl StickInput {
    /// Resolves polar input into canonical coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite or out-of-range values.
    pub fn resolve(self) -> Result<StickPosition, ControllerError> {
        match self {
            Self::Xy(position) => Ok(position),
            Self::Polar(polar) => polar.resolve(),
        }
    }
}

/// Polar stick input where zero degrees points right and 90 degrees points up.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StickPolar {
    pub angle: f64,
    pub strength: f64,
}

impl StickPolar {
    fn resolve(self) -> Result<StickPosition, ControllerError> {
        if !self.angle.is_finite()
            || !(0.0..=360.0).contains(&self.angle)
            || !self.strength.is_finite()
            || !(0.0..=1.0).contains(&self.strength)
        {
            return Err(ControllerError::InvalidPolar);
        }
        let radians = self.angle / 360.0 * TAU;
        let radius = 127.0 * self.strength;
        let x = rounded_axis(128.0 + radians.cos() * radius);
        let y = rounded_axis(128.0 - radians.sin() * radius);
        Ok(StickPosition { x, y })
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_axis(value: f64) -> u8 {
    // Rounding and clamping establish the exact safe integer domain first.
    value.round().clamp(0.0, 255.0) as u8
}

/// Touch update; `pressed=false` releases the touch while preserving a closed shape.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TouchUpdate {
    pub x: u16,
    pub y: u16,
    #[serde(default = "default_pressed")]
    pub pressed: bool,
}

const fn default_pressed() -> bool {
    true
}

impl TouchUpdate {
    fn resolve(self) -> Result<Option<TouchPoint>, ControllerError> {
        let point = TouchPoint::new(self.x, self.y)?;
        Ok(self.pressed.then_some(point))
    }
}

/// Sparse controller update used by script and dynamic configuration adapters.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerUpdate {
    pub a: Option<bool>,
    pub b: Option<bool>,
    pub x: Option<bool>,
    pub y: Option<bool>,
    pub l: Option<bool>,
    pub r: Option<bool>,
    pub zl: Option<bool>,
    pub zr: Option<bool>,
    pub lclick: Option<bool>,
    pub rclick: Option<bool>,
    pub plus: Option<bool>,
    pub minus: Option<bool>,
    pub home: Option<bool>,
    pub capture: Option<bool>,
    pub left_stick: Option<StickInput>,
    pub right_stick: Option<StickInput>,
    pub hat: Option<Hat>,
    pub touch: Option<TouchUpdate>,
}

impl ControllerUpdate {
    fn button_changes(self) -> impl Iterator<Item = (Button, bool)> {
        [
            (Button::A, self.a),
            (Button::B, self.b),
            (Button::X, self.x),
            (Button::Y, self.y),
            (Button::L, self.l),
            (Button::R, self.r),
            (Button::Zl, self.zl),
            (Button::Zr, self.zr),
            (Button::Lclick, self.lclick),
            (Button::Rclick, self.rclick),
            (Button::Plus, self.plus),
            (Button::Minus, self.minus),
            (Button::Home, self.home),
            (Button::Capture, self.capture),
        ]
        .into_iter()
        .filter_map(|(button, value)| value.map(|pressed| (button, pressed)))
    }
}

/// Validation error that leaves the previous controller state untouched.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ControllerError {
    #[error("touch coordinate is outside the 320x240 area: ({x}, {y})")]
    TouchOutOfRange { x: u16, y: u16 },
    #[error("stick polar input must use angle 0..=360 and strength 0..=1")]
    InvalidPolar,
}

#[cfg(test)]
mod tests {
    use super::{
        Button, ControllerError, ControllerState, ControllerUpdate, Hat, StickInput, StickPolar,
        StickPosition, TouchPoint, TouchUpdate,
    };

    #[test]
    fn neutral_is_complete_and_stable() {
        let state = ControllerState::NEUTRAL;
        assert_eq!(state.buttons.switch_bits(), 0);
        assert_eq!(state.hat, Hat::Neutral);
        assert_eq!(state.left_stick, StickPosition::CENTER);
        assert_eq!(state.right_stick, StickPosition::CENTER);
        assert_eq!(state.touch, None);
    }

    #[test]
    fn invalid_update_is_atomic() {
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        let before = state;
        let update = ControllerUpdate {
            b: Some(true),
            touch: Some(TouchUpdate {
                x: 320,
                y: 0,
                pressed: true,
            }),
            ..ControllerUpdate::default()
        };
        assert_eq!(
            state.apply(&update),
            Err(ControllerError::TouchOutOfRange { x: 320, y: 0 })
        );
        assert_eq!(state, before);
    }

    #[test]
    fn polar_and_touch_updates_are_canonical() {
        let mut state = ControllerState::NEUTRAL;
        state
            .apply(&ControllerUpdate {
                left_stick: Some(StickInput::Polar(StickPolar {
                    angle: 90.0,
                    strength: 1.0,
                })),
                touch: Some(TouchUpdate {
                    x: 319,
                    y: 239,
                    pressed: true,
                }),
                ..ControllerUpdate::default()
            })
            .expect("valid update");
        assert_eq!(state.left_stick, StickPosition { x: 128, y: 1 });
        assert_eq!(state.touch, Some(TouchPoint::new(319, 239).unwrap()));
    }

    #[test]
    fn wire_shapes_reject_unknown_or_released_snapshot_touch() {
        let mixed =
            serde_json::from_str::<StickInput>(r#"{"x":128,"y":128,"angle":0.0,"strength":1.0}"#);
        assert!(mixed.is_err());
        let released = serde_json::from_str::<TouchPoint>(r#"{"x":1,"y":2,"pressed":false}"#);
        assert!(released.is_err());
    }
}
