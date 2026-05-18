use bitflags::bitflags;
use std::f64::consts::PI;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Button: u16 {
        const Y       = 0x0001;
        const B       = 0x0002;
        const A       = 0x0004;
        const X       = 0x0008;
        const L       = 0x0010;
        const R       = 0x0020;
        const ZL      = 0x0040;
        const ZR      = 0x0080;
        const MINUS   = 0x0100;
        const PLUS    = 0x0200;
        const LCLICK  = 0x0400;
        const RCLICK  = 0x0800;
        const HOME    = 0x1000;
        const CAPTURE = 0x2000;

        const SELECT = Self::MINUS.bits();
        const START  = Self::PLUS.bits();
        const POWER  = Self::LCLICK.bits();
        const WIRELESS = Self::RCLICK.bits();
    }
}

pub const CONVERSION_3DS_CONTROLLER_BUTTON: [(Button, u16); 11] = [
    (Button::A, 1),
    (Button::B, 2),
    (Button::X, 4),
    (Button::Y, 8),
    (Button::L, 16),
    (Button::R, 32),
    (Button::HOME, 64),
    (Button::PLUS, 128),   // includes alias START
    (Button::MINUS, 256),  // includes alias SELECT
    (Button::LCLICK, 512), // includes alias POWER
    (Button::RCLICK, 0),   // includes alias WIRELESS
];

pub fn convert_button_default(btn: Button) -> Button {
    btn
}

pub fn convert_button_3ds(btn: Button) -> u16 {
    let mut result = 0;
    for &(b, v) in CONVERSION_3DS_CONTROLLER_BUTTON.iter() {
        if btn.contains(b) {
            result |= v;
        }
    }
    result
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Hat {
    #[default]
    TOP = 0,
    TOP_RIGHT = 1,
    RIGHT = 2,
    BTM_RIGHT = 3,
    BTM = 4,
    BTM_LEFT = 5,
    LEFT = 6,
    TOP_LEFT = 7,
    CENTER = 8,
}

pub const CONVERT_HAT_DEFAULT: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

pub const CONVERT_HAT_3DS_CONTROLLER: [u8; 9] = [8, 0, 4, 0, 2, 0, 1, 0, 0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stick {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tilt {
    Up,
    Right,
    Down,
    Left,
    RUp,
    RRight,
    RDown,
    RLeft,
}

pub const DIRECTION_MIN: u8 = 0;
pub const DIRECTION_CENTER: u8 = 128;
pub const DIRECTION_MAX: u8 = 255;

#[derive(Debug, Clone, PartialEq)]
pub struct Direction {
    pub stick: Stick,
    pub x: u8,
    pub y: u8,
    pub angle_for_show: f64,
    pub show_name: Option<String>,
}

impl Direction {
    pub fn from_angle(stick: Stick, angle_deg: f64, magnification: f64) -> Self {
        let mag = magnification.clamp(0.0, 1.0);
        let angle_rad = angle_deg * PI / 180.0;

        let x = (127.5 * angle_rad.cos() * mag + 127.5).round() as u8;
        let y = (127.5 * angle_rad.sin() * mag + 127.5).round() as u8;

        Self {
            stick,
            x,
            y,
            angle_for_show: angle_deg,
            show_name: None,
        }
    }

    pub fn from_xy(stick: Stick, x: u8, y: u8) -> Self {
        Self {
            stick,
            x,
            y,
            angle_for_show: 0.0,
            show_name: Some(format!("({x}, {y})")),
        }
    }

    pub fn get_tilting(&self) -> Vec<Tilt> {
        let mut tilting = Vec::new();
        match self.stick {
            Stick::Left => {
                if self.x < DIRECTION_CENTER {
                    tilting.push(Tilt::Left);
                } else if self.x > DIRECTION_CENTER {
                    tilting.push(Tilt::Right);
                }
                if self.y < DIRECTION_CENTER {
                    tilting.push(Tilt::Down);
                } else if self.y > DIRECTION_CENTER {
                    tilting.push(Tilt::Up);
                }
            }
            Stick::Right => {
                if self.x < DIRECTION_CENTER {
                    tilting.push(Tilt::RLeft);
                } else if self.x > DIRECTION_CENTER {
                    tilting.push(Tilt::RRight);
                }
                if self.y < DIRECTION_CENTER {
                    tilting.push(Tilt::RDown);
                } else if self.y > DIRECTION_CENTER {
                    tilting.push(Tilt::RUp);
                }
            }
        }
        tilting
    }

    pub fn up(stick: Stick) -> Self {
        Self::from_angle(stick, 90.0, 1.0)
    }

    pub fn right(stick: Stick) -> Self {
        Self::from_angle(stick, 0.0, 1.0)
    }

    pub fn down(stick: Stick) -> Self {
        Self::from_angle(stick, -90.0, 1.0)
    }

    pub fn left(stick: Stick) -> Self {
        Self::from_angle(stick, 180.0, 1.0)
    }

    pub fn up_right(stick: Stick) -> Self {
        Self::from_angle(stick, 45.0, 1.0)
    }

    pub fn down_right(stick: Stick) -> Self {
        Self::from_angle(stick, -45.0, 1.0)
    }

    pub fn down_left(stick: Stick) -> Self {
        Self::from_angle(stick, -135.0, 1.0)
    }

    pub fn up_left(stick: Stick) -> Self {
        Self::from_angle(stick, 135.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Touchscreen {
    pub x: u16,
    pub y: u8,
}

impl Touchscreen {
    pub fn new(x: u16, y: u8) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GamepadInput {
    SingleButton(Button),
    SingleHat(Hat),
    SingleDirection(Direction),
    SingleTouchscreen(Touchscreen),
    Multiple(Vec<GamepadInput>),
}

impl GamepadInput {
    pub fn buttons(&self) -> Vec<Button> {
        match self {
            GamepadInput::SingleButton(b) => vec![*b],
            GamepadInput::Multiple(inputs) => inputs
                .iter()
                .filter_map(|i| match i {
                    GamepadInput::SingleButton(b) => Some(*b),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }

    pub fn hats(&self) -> Vec<Hat> {
        match self {
            GamepadInput::SingleHat(h) => vec![*h],
            GamepadInput::Multiple(inputs) => inputs
                .iter()
                .filter_map(|i| match i {
                    GamepadInput::SingleHat(h) => Some(*h),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }

    pub fn directions(&self) -> Vec<Direction> {
        match self {
            GamepadInput::SingleDirection(d) => vec![d.clone()],
            GamepadInput::Multiple(inputs) => inputs
                .iter()
                .filter_map(|i| match i {
                    GamepadInput::SingleDirection(d) => Some(d.clone()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }

    pub fn touchscreens(&self) -> Vec<Touchscreen> {
        match self {
            GamepadInput::SingleTouchscreen(t) => vec![*t],
            GamepadInput::Multiple(inputs) => inputs
                .iter()
                .filter_map(|i| match i {
                    GamepadInput::SingleTouchscreen(t) => Some(*t),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }
}

impl From<Button> for GamepadInput {
    fn from(b: Button) -> Self {
        GamepadInput::SingleButton(b)
    }
}

impl From<Hat> for GamepadInput {
    fn from(h: Hat) -> Self {
        GamepadInput::SingleHat(h)
    }
}

impl From<Direction> for GamepadInput {
    fn from(d: Direction) -> Self {
        GamepadInput::SingleDirection(d)
    }
}

impl From<Touchscreen> for GamepadInput {
    fn from(t: Touchscreen) -> Self {
        GamepadInput::SingleTouchscreen(t)
    }
}

// ---------------------------------------------------------------------------
// parse_buttons: parse a button string like "A", "A|B", "A+B", "DPAD_UP"
// into a Vec of GamepadInput values.
// ---------------------------------------------------------------------------
/// Parse a button name string (e.g. "A", "A|B", "DPAD_UP",
/// "LSTICK_UP") into a vector of [`GamepadInput`] values.
///
/// Separators: |, +, ,.  Unknown names are silently ignored.
pub fn parse_buttons(buttons: &str) -> Vec<GamepadInput> {
    let parts: Vec<&str> = buttons
        .split(['|', '+', ','])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut result = Vec::new();
    for part in parts {
        match part.to_uppercase().as_str() {
            // Standard buttons
            "A" => result.push(GamepadInput::SingleButton(Button::A)),
            "B" => result.push(GamepadInput::SingleButton(Button::B)),
            "X" => result.push(GamepadInput::SingleButton(Button::X)),
            "Y" => result.push(GamepadInput::SingleButton(Button::Y)),
            "L" => result.push(GamepadInput::SingleButton(Button::L)),
            "R" => result.push(GamepadInput::SingleButton(Button::R)),
            "ZL" => result.push(GamepadInput::SingleButton(Button::ZL)),
            "ZR" => result.push(GamepadInput::SingleButton(Button::ZR)),
            "MINUS" | "-" => result.push(GamepadInput::SingleButton(Button::MINUS)),
            "PLUS" => result.push(GamepadInput::SingleButton(Button::PLUS)),
            "LCLICK" | "L3" => result.push(GamepadInput::SingleButton(Button::LCLICK)),
            "RCLICK" | "R3" => result.push(GamepadInput::SingleButton(Button::RCLICK)),
            "HOME" => result.push(GamepadInput::SingleButton(Button::HOME)),
            "CAPTURE" => result.push(GamepadInput::SingleButton(Button::CAPTURE)),
            // Aliases for MINUS / PLUS / LCLICK / RCLICK
            "SELECT" => result.push(GamepadInput::SingleButton(Button::SELECT)),
            "START" => result.push(GamepadInput::SingleButton(Button::START)),
            "POWER" => result.push(GamepadInput::SingleButton(Button::POWER)),
            "WIRELESS" => result.push(GamepadInput::SingleButton(Button::WIRELESS)),
            // Hat / D-Pad
            "DPAD_UP" | "TOP" => result.push(GamepadInput::SingleHat(Hat::TOP)),
            "DPAD_DOWN" | "BTM" => result.push(GamepadInput::SingleHat(Hat::BTM)),
            "DPAD_LEFT" | "LEFT" => result.push(GamepadInput::SingleHat(Hat::LEFT)),
            "DPAD_RIGHT" | "RIGHT" => result.push(GamepadInput::SingleHat(Hat::RIGHT)),
            "DPAD_TOP_RIGHT" | "TOP_RIGHT" => result.push(GamepadInput::SingleHat(Hat::TOP_RIGHT)),
            "DPAD_BTM_RIGHT" | "BTM_RIGHT" => result.push(GamepadInput::SingleHat(Hat::BTM_RIGHT)),
            "DPAD_BTM_LEFT" | "BTM_LEFT" => result.push(GamepadInput::SingleHat(Hat::BTM_LEFT)),
            "DPAD_TOP_LEFT" | "TOP_LEFT" => result.push(GamepadInput::SingleHat(Hat::TOP_LEFT)),
            // Left stick directions
            "LSTICK_UP" | "L_UP" => {
                result.push(GamepadInput::SingleDirection(Direction::up(Stick::Left)))
            }
            "LSTICK_DOWN" | "L_DOWN" => {
                result.push(GamepadInput::SingleDirection(Direction::down(Stick::Left)))
            }
            "LSTICK_LEFT" | "L_LEFT" => {
                result.push(GamepadInput::SingleDirection(Direction::left(Stick::Left)))
            }
            "LSTICK_RIGHT" | "L_RIGHT" => {
                result.push(GamepadInput::SingleDirection(Direction::right(Stick::Left)))
            }
            // Right stick directions
            "RSTICK_UP" | "R_UP" => {
                result.push(GamepadInput::SingleDirection(Direction::up(Stick::Right)))
            }
            "RSTICK_DOWN" | "R_DOWN" => {
                result.push(GamepadInput::SingleDirection(Direction::down(Stick::Right)))
            }
            "RSTICK_LEFT" | "R_LEFT" => {
                result.push(GamepadInput::SingleDirection(Direction::left(Stick::Right)))
            }
            "RSTICK_RIGHT" | "R_RIGHT" => result.push(GamepadInput::SingleDirection(
                Direction::right(Stick::Right),
            )),
            _ => {
                // Unknown button name -- silently ignore
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Button bitflag tests ────────────────────────────────────────────

    #[test]
    fn test_button_bits() {
        assert_eq!(Button::A.bits(), 0x0004);
        assert_eq!(Button::B.bits(), 0x0002);
        assert_eq!(Button::X.bits(), 0x0008);
        assert_eq!(Button::Y.bits(), 0x0001);
        assert_eq!(Button::L.bits(), 0x0010);
        assert_eq!(Button::R.bits(), 0x0020);
        assert_eq!(Button::ZL.bits(), 0x0040);
        assert_eq!(Button::ZR.bits(), 0x0080);
        assert_eq!(Button::MINUS.bits(), 0x0100);
        assert_eq!(Button::PLUS.bits(), 0x0200);
        assert_eq!(Button::LCLICK.bits(), 0x0400);
        assert_eq!(Button::RCLICK.bits(), 0x0800);
        assert_eq!(Button::HOME.bits(), 0x1000);
        assert_eq!(Button::CAPTURE.bits(), 0x2000);
    }

    #[test]
    fn test_button_or_combines() {
        let combo = Button::A | Button::B;
        assert!(combo.contains(Button::A));
        assert!(combo.contains(Button::B));
        assert!(!combo.contains(Button::X));
        assert_eq!(combo.bits(), 0x0006);
    }

    #[test]
    fn test_button_and() {
        let combo = Button::A | Button::B;
        let mask = Button::A | Button::X;
        let result = combo & mask;
        assert_eq!(result, Button::A);
        assert!(!result.contains(Button::B));
    }

    #[test]
    fn test_button_default_is_empty() {
        assert_eq!(Button::default().bits(), 0);
    }

    #[test]
    fn test_button_aliases() {
        assert_eq!(Button::SELECT, Button::MINUS);
        assert_eq!(Button::START, Button::PLUS);
        assert_eq!(Button::POWER, Button::LCLICK);
        assert_eq!(Button::WIRELESS, Button::RCLICK);
    }

    #[test]
    fn test_button_from_bits_truncate() {
        let b = Button::from_bits_truncate(0xFFFF);
        assert!(b.contains(Button::A));
        assert!(b.contains(Button::CAPTURE));
    }

    // ── Hat tests ──────────────────────────────────────────────────────

    #[test]
    fn test_hat_default_is_top() {
        assert_eq!(Hat::default(), Hat::TOP);
    }

    #[test]
    fn test_hat_values() {
        assert_eq!(Hat::TOP as u8, 0);
        assert_eq!(Hat::TOP_RIGHT as u8, 1);
        assert_eq!(Hat::RIGHT as u8, 2);
        assert_eq!(Hat::BTM_RIGHT as u8, 3);
        assert_eq!(Hat::BTM as u8, 4);
        assert_eq!(Hat::BTM_LEFT as u8, 5);
        assert_eq!(Hat::LEFT as u8, 6);
        assert_eq!(Hat::TOP_LEFT as u8, 7);
        assert_eq!(Hat::CENTER as u8, 8);
    }

    // ── Direction tests ────────────────────────────────────────────────

    #[test]
    fn test_direction_from_angle_center() {
        let d = Direction::from_angle(Stick::Left, 90.0, 0.0);
        assert_eq!(d.stick, Stick::Left);
        // At 0.0 magnification, both x and y should be at center (127/128)
        assert!(d.x == 127 || d.x == 128);
        assert!(d.y == 127 || d.y == 128);
    }

    #[test]
    fn test_direction_from_angle_full_up() {
        let d = Direction::from_angle(Stick::Left, 90.0, 1.0);
        assert_eq!(d.stick, Stick::Left);
        // 90°: x=cos(90°)*127.5+127.5 ≈ 127.5 → 127 or 128
        //       y=sin(90°)*127.5+127.5 ≈ 255.0 → 255
        assert_eq!(d.y, 255);
    }

    #[test]
    fn test_direction_from_xy() {
        let d = Direction::from_xy(Stick::Right, 200, 50);
        assert_eq!(d.stick, Stick::Right);
        assert_eq!(d.x, 200);
        assert_eq!(d.y, 50);
        assert_eq!(d.show_name, Some("(200, 50)".to_string()));
    }

    #[test]
    fn test_direction_get_tilting_left() {
        let d = Direction::from_xy(Stick::Left, 200, 200);
        let tilts = d.get_tilting();
        assert!(tilts.contains(&Tilt::Right));
        assert!(tilts.contains(&Tilt::Up));
        assert!(!tilts.contains(&Tilt::Left));
        assert!(!tilts.contains(&Tilt::Down));
    }

    #[test]
    fn test_direction_get_tilting_right() {
        let d = Direction::from_xy(Stick::Right, 200, 200);
        let tilts = d.get_tilting();
        assert!(tilts.contains(&Tilt::RRight));
        assert!(tilts.contains(&Tilt::RUp));
    }

    #[test]
    fn test_direction_get_tilting_center() {
        let d = Direction::from_xy(Stick::Left, 128, 128);
        let tilts = d.get_tilting();
        assert!(tilts.is_empty());
    }

    #[test]
    fn test_direction_get_tilting_left_bottom_left() {
        let d = Direction::from_xy(Stick::Left, 0, 0);
        let tilts = d.get_tilting();
        assert!(tilts.contains(&Tilt::Left));
        assert!(tilts.contains(&Tilt::Down));
    }

    #[test]
    fn test_direction_convenience_methods() {
        // Left stick directions
        assert_eq!(Direction::up(Stick::Left).get_tilting(), vec![Tilt::Up]);
        assert_eq!(Direction::down(Stick::Left).get_tilting(), vec![Tilt::Down]);
        assert_eq!(Direction::left(Stick::Left).get_tilting(), vec![Tilt::Left]);
        assert_eq!(
            Direction::right(Stick::Left).get_tilting(),
            vec![Tilt::Right]
        );
        // Right stick directions
        assert_eq!(Direction::up(Stick::Right).get_tilting(), vec![Tilt::RUp]);
        assert_eq!(
            Direction::down(Stick::Right).get_tilting(),
            vec![Tilt::RDown]
        );
        assert_eq!(
            Direction::left(Stick::Right).get_tilting(),
            vec![Tilt::RLeft]
        );
        assert_eq!(
            Direction::right(Stick::Right).get_tilting(),
            vec![Tilt::RRight]
        );
    }

    #[test]
    fn test_direction_diagonal_left() {
        let up_right = Direction::up_right(Stick::Left);
        let tilts = up_right.get_tilting();
        assert!(tilts.contains(&Tilt::Up));
        assert!(tilts.contains(&Tilt::Right));

        let down_left = Direction::down_left(Stick::Left);
        let tilts = down_left.get_tilting();
        assert!(tilts.contains(&Tilt::Down));
        assert!(tilts.contains(&Tilt::Left));
    }

    #[test]
    fn test_direction_diagonal_right() {
        let up_right = Direction::up_right(Stick::Right);
        let tilts = up_right.get_tilting();
        assert!(tilts.contains(&Tilt::RUp));
        assert!(tilts.contains(&Tilt::RRight));

        let down_left = Direction::down_left(Stick::Right);
        let tilts = down_left.get_tilting();
        assert!(tilts.contains(&Tilt::RDown));
        assert!(tilts.contains(&Tilt::RLeft));
    }

    // ── GamepadInput tests ─────────────────────────────────────────────

    #[test]
    fn test_gamepad_input_from_button() {
        let input: GamepadInput = Button::A.into();
        assert_eq!(input.buttons(), vec![Button::A]);
        assert!(input.hats().is_empty());
        assert!(input.directions().is_empty());
        assert!(input.touchscreens().is_empty());
    }

    #[test]
    fn test_gamepad_input_from_hat() {
        let input: GamepadInput = Hat::TOP.into();
        assert_eq!(input.hats(), vec![Hat::TOP]);
        assert!(input.buttons().is_empty());
    }

    #[test]
    fn test_gamepad_input_from_direction() {
        let dir = Direction::from_xy(Stick::Left, 200, 50);
        let input: GamepadInput = dir.clone().into();
        assert_eq!(input.directions(), vec![dir]);
    }

    #[test]
    fn test_gamepad_input_from_touchscreen() {
        let ts = Touchscreen::new(100, 50);
        let input: GamepadInput = ts.into();
        assert_eq!(input.touchscreens(), vec![Touchscreen::new(100, 50)]);
    }

    #[test]
    fn test_gamepad_input_multiple_collects() {
        let input = GamepadInput::Multiple(vec![
            GamepadInput::SingleButton(Button::A),
            GamepadInput::SingleHat(Hat::TOP),
            GamepadInput::SingleButton(Button::B),
        ]);
        assert_eq!(input.buttons(), vec![Button::A, Button::B]);
        assert_eq!(input.hats(), vec![Hat::TOP]);
    }

    // ── Touchscreen tests ──────────────────────────────────────────────

    #[test]
    fn test_touchscreen_new() {
        let ts = Touchscreen::new(500, 200);
        assert_eq!(ts.x, 500);
        assert_eq!(ts.y, 200);
    }

    // ── Conversion table tests ─────────────────────────────────────────

    #[test]
    fn test_convert_button_default_identity() {
        assert_eq!(convert_button_default(Button::A), Button::A);
        assert_eq!(
            convert_button_default(Button::B | Button::X),
            Button::B | Button::X
        );
    }

    #[test]
    fn test_convert_button_3ds_values() {
        // A=0x0004 → bit 1 (value 1)
        assert_eq!(convert_button_3ds(Button::A), 1);
        // B=0x0002 → bit 2 (value 2)
        assert_eq!(convert_button_3ds(Button::B), 2);
        // X=0x0008 → bit 4 (value 4)
        assert_eq!(convert_button_3ds(Button::X), 4);
        // Y=0x0001 → bit 8 (value 8)
        assert_eq!(convert_button_3ds(Button::Y), 8);
        // HOME=0x1000 → bit 64
        assert_eq!(convert_button_3ds(Button::HOME), 64);
    }

    #[test]
    fn test_convert_button_3ds_combo() {
        let combo = Button::A | Button::B;
        assert_eq!(convert_button_3ds(combo), 1 | 2);
    }

    #[test]
    fn test_convert_hat_3ds_controller() {
        // Center → 0 (CENTER=8 → lookup 8 → value 0)
        assert_eq!(CONVERT_HAT_3DS_CONTROLLER[Hat::CENTER as usize], 0);
        // TOP → 8
        assert_eq!(CONVERT_HAT_3DS_CONTROLLER[Hat::TOP as usize], 8);
        // RIGHT → 4
        assert_eq!(CONVERT_HAT_3DS_CONTROLLER[Hat::RIGHT as usize], 4);
    }

    #[test]
    fn test_constants() {
        assert_eq!(DIRECTION_MIN, 0);
        assert_eq!(DIRECTION_CENTER, 128);
        assert_eq!(DIRECTION_MAX, 255);
    }
}
