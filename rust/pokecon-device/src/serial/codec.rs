use serde::{Deserialize, Serialize};

use crate::controller::{Button, ControllerState, Hat, StickPosition};

/// Supported on-wire controller formats.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerFormat {
    #[default]
    Default,
    Qingpi,
    #[serde(rename = "3ds")]
    ThreeDs,
}

impl ControllerFormat {
    /// Parses the canonical setting value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "qingpi" => Some(Self::Qingpi),
            "3ds" => Some(Self::ThreeDs),
            _ => None,
        }
    }
}

/// Stateful encoder used to omit unchanged stick coordinates in default mode.
#[derive(Clone, Debug)]
pub struct ControllerCodec {
    format: ControllerFormat,
    previous: ControllerState,
}

impl ControllerCodec {
    #[must_use]
    pub const fn new(format: ControllerFormat) -> Self {
        Self {
            format,
            previous: ControllerState::NEUTRAL,
        }
    }

    #[must_use]
    pub const fn format(&self) -> ControllerFormat {
        self.format
    }

    /// Encodes without advancing delta state. Call [`Self::commit`] only after
    /// a whole frame was successfully written.
    #[must_use]
    pub fn encode(&self, state: ControllerState) -> Vec<u8> {
        match self.format {
            ControllerFormat::Default => encode_default(self.previous, state),
            ControllerFormat::Qingpi => encode_qingpi(state),
            ControllerFormat::ThreeDs => encode_three_ds(state),
        }
    }

    /// Advances the last-sent state after successful delivery.
    pub const fn commit(&mut self, state: ControllerState) {
        self.previous = state;
    }

    /// Clears delta state on a new connection so the neutral baseline is known.
    pub const fn reset(&mut self) {
        self.previous = ControllerState::NEUTRAL;
    }
}

fn encode_default(previous: ControllerState, state: ControllerState) -> Vec<u8> {
    let left_changed = previous.left_stick != state.left_stick;
    let right_changed = previous.right_stick != state.right_stick;
    let mut flags = state.buttons.switch_bits() << 2;
    if left_changed {
        flags |= 0b10;
    }
    if right_changed {
        flags |= 0b01;
    }
    let mut frame = format!("0x{flags:06x} {}", state.hat.switch_value());
    if left_changed {
        push_stick_text(&mut frame, state.left_stick);
    }
    if right_changed {
        push_stick_text(&mut frame, state.right_stick);
    }
    frame.push_str("\r\n");
    frame.into_bytes()
}

fn push_stick_text(frame: &mut String, stick: StickPosition) {
    use std::fmt::Write as _;

    write!(frame, " {:02x} {:02x}", stick.x, stick.y).expect("writing to String cannot fail");
}

fn encode_qingpi(state: ControllerState) -> Vec<u8> {
    let buttons = state.buttons.switch_bits();
    let button_bytes = buttons.to_le_bytes();
    let (touch_x, touch_y) = state.touch.map_or((0, 0), |touch| (touch.x(), touch.y()));
    vec![
        0xab,
        button_bytes[0],
        button_bytes[1],
        state.hat.switch_value(),
        state.left_stick.x,
        state.left_stick.y,
        128,
        128,
        touch_x.to_le_bytes()[0],
        touch_x.to_le_bytes()[1],
        touch_y.to_le_bytes()[0],
    ]
}

fn encode_three_ds(state: ControllerState) -> Vec<u8> {
    let buttons = three_ds_buttons(state);
    let hat = match state.hat {
        Hat::Up => 8,
        Hat::Right => 4,
        Hat::Down => 2,
        Hat::Left => 1,
        Hat::UpRight | Hat::UpLeft | Hat::DownRight | Hat::DownLeft | Hat::Neutral => 0,
    };
    vec![
        0xa1,
        ((buttons.to_le_bytes()[0] & 0x0f) << 4) | hat,
        ((buttons >> 4) & 0x3f).to_le_bytes()[0],
        0xa2,
        encode_three_ds_axis(state.left_stick.x),
        encode_three_ds_axis(state.left_stick.y),
    ]
}

fn three_ds_buttons(state: ControllerState) -> u16 {
    [
        (Button::A, 1 << 0),
        (Button::B, 1 << 1),
        (Button::X, 1 << 2),
        (Button::Y, 1 << 3),
        (Button::L, 1 << 4),
        (Button::R, 1 << 5),
        (Button::Home, 1 << 6),
        (Button::Plus, 1 << 7),
        (Button::Minus, 1 << 8),
        (Button::Lclick, 1 << 9),
    ]
    .into_iter()
    .filter(|(button, _)| state.buttons.get(*button))
    .fold(0, |bits, (_, bit)| bits | bit)
}

const fn encode_three_ds_axis(value: u8) -> u8 {
    if value >= 128 { value } else { 127 - value }
}

#[cfg(test)]
mod tests {
    use super::{ControllerCodec, ControllerFormat};
    use crate::controller::{Button, ControllerState, Hat, StickPosition, TouchPoint};

    #[test]
    fn default_text_has_six_hex_digits_and_delta_sticks() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        state.buttons.set(Button::Capture, true);
        state.hat = Hat::UpRight;
        state.left_stick = StickPosition { x: 0, y: 255 };
        assert_eq!(codec.encode(state), b"0x008012 1 00 ff\r\n".to_vec());
        codec.commit(state);
        assert_eq!(codec.encode(state), b"0x008010 1\r\n".to_vec());
    }

    #[test]
    fn qingpi_is_exactly_eleven_bytes() {
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::Y, true);
        state.buttons.set(Button::Capture, true);
        state.hat = Hat::DownLeft;
        state.left_stick = StickPosition { x: 1, y: 2 };
        state.right_stick = StickPosition { x: 3, y: 4 };
        state.touch = Some(TouchPoint::new(319, 239).unwrap());
        assert_eq!(
            ControllerCodec::new(ControllerFormat::Qingpi).encode(state),
            vec![0xab, 0x01, 0x20, 5, 1, 2, 128, 128, 0x3f, 0x01, 0xef]
        );
    }

    #[test]
    fn three_ds_is_exactly_six_bytes() {
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        state.buttons.set(Button::Home, true);
        state.buttons.set(Button::Minus, true);
        state.hat = Hat::Left;
        state.left_stick = StickPosition { x: 0, y: 128 };
        assert_eq!(
            ControllerCodec::new(ControllerFormat::ThreeDs).encode(state),
            vec![0xa1, 0x11, 0x14, 0xa2, 127, 128]
        );
    }
}
