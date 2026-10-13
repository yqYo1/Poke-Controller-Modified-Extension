use serde::{Deserialize, Serialize};

use crate::device::controller::{Button, ControllerState, Hat, StickPosition};

/// Supported on-wire controller formats.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerFormat {
    #[default]
    Default,
    Nx,
    Qingpi,
    #[serde(rename = "3ds_x4")]
    ThreeDsX4,
    #[serde(rename = "3ds_x3")]
    ThreeDsX3,
}

impl ControllerFormat {
    /// Parses the canonical setting value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "nx" => Some(Self::Nx),
            "qingpi" => Some(Self::Qingpi),
            "3ds_x4" => Some(Self::ThreeDsX4),
            "3ds_x3" => Some(Self::ThreeDsX3),
            _ => None,
        }
    }

    /// Resolves a format name with the upstream `get_transport` fallback: an
    /// exact canonical value wins; a name containing `3DS Controller` selects
    /// by the `x.3` marker; anything else falls back to `Default`.
    #[must_use]
    pub fn resolve(name: &str) -> Self {
        if let Some(format) = Self::parse(name) {
            return format;
        }
        if name.contains("3DS Controller") {
            return if name.contains("x.3") {
                Self::ThreeDsX3
            } else {
                Self::ThreeDsX4
            };
        }
        Self::Default
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
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "codec identity is exercised by serial unit tests")
    )]
    pub const fn format(&self) -> ControllerFormat {
        self.format
    }

    /// Encodes without advancing delta state. Call [`Self::commit`] only after
    /// a whole frame was successfully written.
    #[must_use]
    pub fn encode(&self, state: ControllerState) -> Vec<u8> {
        match self.format {
            ControllerFormat::Default => encode_default(self.previous, state),
            ControllerFormat::Nx => encode_nx(state),
            ControllerFormat::Qingpi => encode_qingpi(state),
            ControllerFormat::ThreeDsX4 => encode_three_ds_x4(state),
            ControllerFormat::ThreeDsX3 => encode_three_ds_x3(state),
        }
    }

    /// Advances the last-sent state after successful delivery.
    pub const fn commit(&mut self, state: ControllerState) {
        self.previous = state;
    }

    /// Clears delta state on a new connection so the neutral baseline is known.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "codec reset semantics are exercised by serial unit tests"
        )
    )]
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

fn encode_nx(state: ControllerState) -> Vec<u8> {
    encode_nx_frame(state, 0, 0)
}

/// Encodes the NX Macro Controller 11-byte frame.
///
/// Layout: `0xAB btnLo btnHi hat lx ly rx ry kbdMode key 0x00`. The button mask is
/// identical to [`ControllerState::switch_bits`] and stick centers are `0x80`.
/// `keyboard_mode`/`key` carry keyboard actions inside the same frame (0/0 = none);
/// the key step sequence (init/text/Key/Press/Release with 0.03 s waits) is produced
/// by the script keyboard layer when it sends frames.
#[must_use]
pub fn encode_nx_frame(state: ControllerState, keyboard_mode: u8, key: u8) -> Vec<u8> {
    let button_bytes = state.buttons.switch_bits().to_le_bytes();
    vec![
        0xab,
        button_bytes[0],
        button_bytes[1],
        state.hat.switch_value(),
        state.left_stick.x,
        state.left_stick.y,
        state.right_stick.x,
        state.right_stick.y,
        keyboard_mode,
        key,
        0,
    ]
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

fn encode_three_ds_x4(state: ControllerState) -> Vec<u8> {
    encode_three_ds(state, ThreeDsStick::Inverted)
}

fn encode_three_ds_x3(state: ControllerState) -> Vec<u8> {
    encode_three_ds(state, ThreeDsStick::Mirrored)
}

/// Stick transform of the two 3DS Controller firmware formats.
#[derive(Clone, Copy)]
enum ThreeDsStick {
    /// `FW:x.4-`: `255 - v` on both axes (center `0x7F`).
    Inverted,
    /// `FW:-x.3`: identity at `v >= 128`, `127 - v` below (center `0x80`).
    Mirrored,
}

/// Encodes the 3DS Controller 11-byte frame:
/// `A1 btnHat A2 LX LY B2 touchOn touchXHi touchXLo touchY`.
fn encode_three_ds(state: ControllerState, stick: ThreeDsStick) -> Vec<u8> {
    let buttons = three_ds_buttons(state);
    let (left_x, left_y) = match stick {
        ThreeDsStick::Inverted => (255 - state.left_stick.x, 255 - state.left_stick.y),
        ThreeDsStick::Mirrored => (
            mirror_three_ds_axis(state.left_stick.x),
            mirror_three_ds_axis(state.left_stick.y),
        ),
    };
    let (touch_on, touch_x, touch_y) = match state.touch {
        // Upstream treats `(0, 0)` as "not pressed"; mirror that on the wire.
        Some(touch) if touch.x() != 0 || touch.y() != 0 => {
            (1, touch.x(), touch.y().to_le_bytes()[0])
        }
        _ => (0, 0, 0),
    };
    vec![
        0xa1,
        ((buttons.to_le_bytes()[0] & 0x0f) << 4) | three_ds_hat(state.hat),
        ((buttons >> 4) & 0x3f).to_le_bytes()[0],
        0xa2,
        left_x,
        left_y,
        0xb2,
        touch_on,
        (touch_x >> 8).to_le_bytes()[0],
        touch_x.to_le_bytes()[0],
        touch_y,
    ]
}

const fn mirror_three_ds_axis(value: u8) -> u8 {
    if value >= 128 { value } else { 127 - value }
}

/// 3DS hat bits: `Up=8`, `Right=4`, `Down=2`, `Left=1`; the 3DS controller
/// protocol cannot represent diagonals or a neutral value (both encode 0).
const fn three_ds_hat(hat: Hat) -> u8 {
    match hat {
        Hat::Up => 8,
        Hat::Right => 4,
        Hat::Down => 2,
        Hat::Left => 1,
        Hat::UpRight | Hat::UpLeft | Hat::DownRight | Hat::DownLeft | Hat::Neutral => 0,
    }
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

#[cfg(test)]
mod tests {
    use super::{ControllerCodec, ControllerFormat, encode_nx_frame};
    use crate::device::controller::{Button, ControllerState, Hat, StickPosition, TouchPoint};

    #[test]
    fn default_text_has_six_hex_digits_and_delta_sticks() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        assert_eq!(codec.format(), ControllerFormat::Default);
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        state.buttons.set(Button::Capture, true);
        state.hat = Hat::UpRight;
        state.left_stick = StickPosition { x: 0, y: 255 };
        assert_eq!(codec.encode(state), b"0x008012 1 00 ff\r\n".to_vec());
        codec.commit(state);
        assert_eq!(codec.encode(state), b"0x008010 1\r\n".to_vec());
        codec.reset();
        assert_eq!(codec.encode(state), b"0x008012 1 00 ff\r\n".to_vec());
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
    fn three_ds_x4_matches_golden_button_hat_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::ThreeDsX4);
        let mut state = ControllerState::NEUTRAL;
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state.buttons.set(Button::A, true);
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x10, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::L, true);
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x01, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::Minus, true);
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x10, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.hat = Hat::Up;
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x08, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.hat = Hat::UpRight; // diagonals are not representable on 3DS (encode 0)
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
    }

    #[test]
    fn three_ds_x4_matches_golden_stick_and_touch_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::ThreeDsX4);
        let mut state = ControllerState::NEUTRAL;
        state.left_stick = StickPosition { x: 0x26, y: 0xda };
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0xd9, 0x25, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::L, true);
        state.buttons.set(Button::A, true);
        state.left_stick = StickPosition { x: 0xff, y: 0x80 };
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x10, 0x01, 0xa2, 0x00, 0x7f, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.touch = Some(TouchPoint::new(100, 50).unwrap());
        assert_eq!(
            codec.encode(state),
            vec![
                0xa1, 0x00, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0x01, 0x00, 0x64, 0x32
            ]
        );
        state = ControllerState::NEUTRAL;
        state.touch = Some(TouchPoint::new(319, 239).unwrap());
        assert_eq!(
            codec.encode(state),
            vec![
                0xa1, 0x00, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0x01, 0x01, 0x3f, 0xef
            ]
        );
        state = ControllerState::NEUTRAL;
        state.touch = Some(TouchPoint::new(0, 0).unwrap()); // (0, 0) means "not pressed"
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0x7f, 0x7f, 0xb2, 0, 0, 0, 0]
        );
    }

    #[test]
    fn three_ds_x3_matches_golden_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::ThreeDsX3);
        let mut state = ControllerState::NEUTRAL;
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0x80, 0x80, 0xb2, 0, 0, 0, 0]
        );
        state.buttons.set(Button::A, true);
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x10, 0x00, 0xa2, 0x80, 0x80, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::L, true);
        state.buttons.set(Button::A, true);
        state.left_stick = StickPosition { x: 0xff, y: 0x80 };
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x10, 0x01, 0xa2, 0xff, 0x80, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.left_stick = StickPosition { x: 0x26, y: 0xda };
        assert_eq!(
            codec.encode(state),
            vec![0xa1, 0x00, 0x00, 0xa2, 0x59, 0xda, 0xb2, 0, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.touch = Some(TouchPoint::new(319, 239).unwrap());
        assert_eq!(
            codec.encode(state),
            vec![
                0xa1, 0x00, 0x00, 0xa2, 0x80, 0x80, 0xb2, 0x01, 0x01, 0x3f, 0xef
            ]
        );
    }

    #[test]
    fn three_ds_resolution_matches_upstream_fallback() {
        assert_eq!(
            ControllerFormat::resolve("3ds_x4"),
            ControllerFormat::ThreeDsX4
        );
        assert_eq!(
            ControllerFormat::resolve("3ds_x3"),
            ControllerFormat::ThreeDsX3
        );
        assert_eq!(
            ControllerFormat::resolve("3DS Controller(FW:x.4-)"),
            ControllerFormat::ThreeDsX4
        );
        assert_eq!(
            ControllerFormat::resolve("3DS Controller(FW:-x.3)"),
            ControllerFormat::ThreeDsX3
        );
        // The bare legacy name resolves to the newer-firmware format.
        assert_eq!(
            ControllerFormat::resolve("3DS Controller"),
            ControllerFormat::ThreeDsX4
        );
        assert_eq!(
            ControllerFormat::resolve("unknown format"),
            ControllerFormat::Default
        );
        assert_eq!(ControllerFormat::resolve("nx"), ControllerFormat::Nx);
    }

    #[test]
    fn nx_matches_golden_button_and_release_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::Nx);
        assert_eq!(
            codec.encode(ControllerState::NEUTRAL),
            vec![0xab, 0x00, 0x00, 8, 0x80, 0x80, 0x80, 0x80, 0, 0, 0]
        );
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x04, 0x00, 8, 0x80, 0x80, 0x80, 0x80, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::Minus, true);
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x00, 0x01, 8, 0x80, 0x80, 0x80, 0x80, 0, 0, 0]
        );
    }

    #[test]
    fn nx_matches_golden_hat_stick_and_combination_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::Nx);
        let mut state = ControllerState::NEUTRAL;
        state.hat = Hat::Up;
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x00, 0x00, 0, 0x80, 0x80, 0x80, 0x80, 0, 0, 0]
        );
        state.hat = Hat::UpLeft;
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x00, 0x00, 7, 0x80, 0x80, 0x80, 0x80, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::Zr, true);
        state.buttons.set(Button::B, true);
        state.hat = Hat::UpRight;
        state.left_stick = StickPosition { x: 0x80, y: 0x00 };
        state.right_stick = StickPosition { x: 0x00, y: 0x80 };
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x82, 0x00, 1, 0x80, 0x00, 0x00, 0x80, 0, 0, 0]
        );
        state = ControllerState::NEUTRAL;
        state.buttons.set(Button::L, true);
        state.buttons.set(Button::A, true);
        state.left_stick = StickPosition { x: 0xff, y: 0x80 };
        assert_eq!(
            codec.encode(state),
            vec![0xab, 0x14, 0x00, 8, 0xff, 0x80, 0x80, 0x80, 0, 0, 0]
        );
    }

    #[test]
    fn nx_keyboard_frames_keep_the_controller_layout() {
        let mut state = ControllerState::NEUTRAL;
        assert_eq!(
            encode_nx_frame(state, 5, 0),
            vec![0xab, 0x00, 0x00, 8, 0x80, 0x80, 0x80, 0x80, 5, 0, 0]
        );
        state.buttons.set(Button::A, true);
        assert_eq!(
            encode_nx_frame(state, 1, b'A'),
            vec![0xab, 0x04, 0x00, 8, 0x80, 0x80, 0x80, 0x80, 1, 0x41, 0]
        );
        assert_eq!(
            encode_nx_frame(state, 4, 0x04),
            vec![0xab, 0x04, 0x00, 8, 0x80, 0x80, 0x80, 0x80, 4, 0x04, 0]
        );
    }
}
