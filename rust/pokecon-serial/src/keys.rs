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

pub const CONVERSION_3DS_CONTROLLER_BUTTON: [(Button, u16); 14] = [
    (Button::A, 1),
    (Button::B, 2),
    (Button::X, 4),
    (Button::Y, 8),
    (Button::L, 16),
    (Button::R, 32),
    (Button::HOME, 64),
    (Button::START, 128),
    (Button::SELECT, 256),
    (Button::POWER, 512),
    (Button::PLUS, 128),
    (Button::MINUS, 256),
    (Button::LCLICK, 512),
    (Button::RCLICK, 0),
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
