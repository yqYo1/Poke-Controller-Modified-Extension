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
    DsFtRev3,
    DsFtRev2,
    DsFtRev1,
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
            "ds_ft_rev3" => Some(Self::DsFtRev3),
            "ds_ft_rev2" => Some(Self::DsFtRev2),
            "ds_ft_rev1" => Some(Self::DsFtRev1),
            _ => None,
        }
    }

    /// Resolves a format name with the upstream `get_transport` fallback: an
    /// exact canonical value or exact display name wins; a name containing
    /// `3DS Controller` selects by the `x.3` marker; anything else falls back
    /// to `Default`.
    #[must_use]
    pub fn resolve(name: &str) -> Self {
        if let Some(format) = Self::parse(name) {
            return format;
        }
        match name {
            "DS FT (Rev3)" => return Self::DsFtRev3,
            "DS FT (Rev2)" => return Self::DsFtRev2,
            "DS FT (Rev1)" => return Self::DsFtRev1,
            _ => {}
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

    /// Returns the upstream-facing display name used by user-visible
    /// compatibility messages for this format.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Nx => "NX",
            Self::Qingpi => "Qingpi",
            Self::ThreeDsX4 => "3DS Controller(FW:x.4-)",
            Self::ThreeDsX3 => "3DS Controller(FW:-x.3)",
            Self::DsFtRev3 => "DS FT (Rev3)",
            Self::DsFtRev2 => "DS FT (Rev2)",
            Self::DsFtRev1 => "DS FT (Rev1)",
        }
    }

    /// Default baud rate owned by this format (upstream transport `baud_rate`).
    #[must_use]
    pub const fn default_baud(self) -> Option<u32> {
        match self {
            Self::Default | Self::Nx | Self::Qingpi => Some(9600),
            Self::ThreeDsX4 | Self::ThreeDsX3 => Some(115_200),
            Self::DsFtRev3 | Self::DsFtRev2 | Self::DsFtRev1 => None,
        }
    }

    /// Whether the format sends compatibility text rows.
    #[must_use]
    pub const fn supports_text(self) -> bool {
        matches!(self, Self::Default)
    }

    /// Row sent by `KeyPress.end()` when no other input source remains.
    #[must_use]
    pub const fn end_row(self) -> Option<&'static str> {
        match self {
            Self::Default => Some("end"),
            Self::Nx
            | Self::Qingpi
            | Self::ThreeDsX4
            | Self::ThreeDsX3
            | Self::DsFtRev3
            | Self::DsFtRev2
            | Self::DsFtRev1 => None,
        }
    }

    /// Whether the format carries touchscreen coordinates.
    #[must_use]
    pub const fn has_touchscreen(self) -> bool {
        matches!(self, Self::Qingpi | Self::ThreeDsX4 | Self::ThreeDsX3)
    }

    /// Whether raw serial writes are allowed (blocked formats warn once and
    /// skip the write).
    #[must_use]
    pub const fn allows_raw_write(self) -> bool {
        matches!(
            self,
            Self::Default | Self::Nx | Self::Qingpi | Self::ThreeDsX4 | Self::ThreeDsX3
        )
    }

    /// Whether identical merged payloads are skipped.
    #[must_use]
    pub const fn skip_unchanged(self) -> bool {
        !matches!(
            self,
            Self::Default | Self::Nx | Self::Qingpi | Self::ThreeDsX4 | Self::ThreeDsX3
        )
    }

    /// Whether this format is carried by the FTDI MPSSE transport instead of
    /// a UART serial port (the DS FT revisions).
    #[must_use]
    pub const fn uses_ftdi(self) -> bool {
        matches!(self, Self::DsFtRev3 | Self::DsFtRev2 | Self::DsFtRev1)
    }

    /// User-visible message shown once when raw writes are blocked.
    #[must_use]
    pub const fn raw_write_message(self) -> &'static str {
        match self {
            Self::Default | Self::Nx | Self::Qingpi | Self::ThreeDsX4 | Self::ThreeDsX3 => "",
            Self::DsFtRev3 | Self::DsFtRev2 | Self::DsFtRev1 => {
                "FTDI: このData Formatでは、Mcuコマンドやシリアルコマンドの直接送信は行いません。"
            }
        }
    }

    /// Log line recorded once when raw writes are blocked.
    #[must_use]
    pub const fn raw_write_log(self) -> &'static str {
        match self {
            Self::Default | Self::Nx | Self::Qingpi | Self::ThreeDsX4 | Self::ThreeDsX3 => "",
            Self::DsFtRev3 | Self::DsFtRev2 | Self::DsFtRev1 => {
                "Raw serial write is blocked in the FTDI data format."
            }
        }
    }
}

/// Stateful encoder used to omit unchanged stick coordinates in default mode.
#[derive(Clone, Debug)]
pub struct ControllerCodec {
    format: ControllerFormat,
    previous: Option<ControllerState>,
}

impl ControllerCodec {
    #[must_use]
    pub const fn new(format: ControllerFormat) -> Self {
        Self {
            format,
            previous: None,
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
            ControllerFormat::Nx => encode_nx(state),
            ControllerFormat::Qingpi => encode_qingpi(state),
            ControllerFormat::ThreeDsX4 => encode_three_ds_x4(state),
            ControllerFormat::ThreeDsX3 => encode_three_ds_x3(state),
            ControllerFormat::DsFtRev3 => encode_ds_ft(state, DsFtPins::REV3),
            ControllerFormat::DsFtRev2 => encode_ds_ft(state, DsFtPins::REV2),
            ControllerFormat::DsFtRev1 => encode_ds_ft(state, DsFtPins::REV1),
        }
    }

    /// Advances the last-sent state after successful delivery.
    pub const fn commit(&mut self, state: ControllerState) {
        self.previous = Some(state);
    }

    /// Forgets the last-sent state (upstream `_last_sent = None`): the next
    /// default-mode frame re-sends both sticks. Called for raw writes and on
    /// new connections.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "codec reset semantics are exercised by serial unit tests"
        )
    )]
    pub const fn reset(&mut self) {
        self.previous = None;
    }
}

fn encode_default(previous: Option<ControllerState>, state: ControllerState) -> Vec<u8> {
    // Upstream `text_serial.encode`: without a last-sent state (`last_sent is
    // None`) both sticks count as changed, so the first frame after a
    // connection or raw write re-sends them.
    let left_changed = previous.is_none_or(|p| p.left_stick != state.left_stick);
    let right_changed = previous.is_none_or(|p| p.right_stick != state.right_stick);
    let mut flags = state.buttons.switch_bits() << 2;
    if left_changed {
        flags |= 0b10;
    }
    if right_changed {
        flags |= 0b01;
    }
    // Upstream `convert2str` formats the mask with `#06x` ("0x" plus four
    // lowercase hex digits) and stick values with unpadded lowercase hex.
    let mut frame = format!("0x{flags:04x} {}", state.hat.switch_value());
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

    write!(frame, " {:x} {:x}", stick.x, stick.y).expect("writing to String cannot fail");
}

/// Pin assignment (bus, bit) of the DS FT board revisions. Only B, X and Y
/// differ between revisions; every other pin is shared (upstream
/// `FTDI_PIN_MAPS`).
#[derive(Clone, Copy)]
struct DsFtPins {
    b: (u8, u8),
    x: (u8, u8),
    y: (u8, u8),
}

impl DsFtPins {
    const REV1: Self = Self {
        b: (0, 5),
        x: (0, 6),
        y: (0, 7),
    };
    const REV2: Self = Self {
        b: (1, 6),
        x: (1, 7),
        y: (1, 5),
    };
    const REV3: Self = Self {
        b: (0, 5),
        x: (0, 6),
        y: (1, 5),
    };
}

/// Encodes the DS FT MPSSE payload: `0x82 C ~C 0x80 D ~D` with every pressed
/// pin pulled low on the C/D buses (upstream `encode_pin_states`). Sticks and
/// touch are ignored; unmapped buttons (ZL/ZR/RCLICK/HOME/CAPTURE) have no pin.
fn encode_ds_ft(state: ControllerState, pins: DsFtPins) -> Vec<u8> {
    let mut c = 0xff_u8;
    let mut d = 0xff_u8;
    let mut press = |(bus, bit): (u8, u8)| {
        if bus == 0 {
            c &= !(1 << bit);
        } else {
            d &= !(1 << bit);
        }
    };
    let buttons = state.buttons;
    if buttons.a {
        press((0, 4));
    }
    if buttons.b {
        press(pins.b);
    }
    if buttons.x {
        press(pins.x);
    }
    if buttons.y {
        press(pins.y);
    }
    if buttons.l {
        press((1, 0));
    }
    if buttons.r {
        press((1, 1));
    }
    if buttons.minus {
        press((1, 2));
    }
    if buttons.plus {
        press((1, 3));
    }
    if buttons.lclick {
        press((1, 4));
    }
    for pin in ds_ft_hat_pins(state.hat) {
        press(*pin);
    }
    vec![0x82, c, !c, 0x80, d, !d]
}

/// D-pad pins pressed for one hat direction (diagonals press two pins).
fn ds_ft_hat_pins(hat: Hat) -> &'static [(u8, u8)] {
    match hat {
        Hat::Up => &[(0, 3)],
        Hat::UpRight => &[(0, 3), (0, 2)],
        Hat::Right => &[(0, 2)],
        Hat::DownRight => &[(0, 1), (0, 2)],
        Hat::Down => &[(0, 1)],
        Hat::DownLeft => &[(0, 1), (0, 0)],
        Hat::Left => &[(0, 0)],
        Hat::UpLeft => &[(0, 3), (0, 0)],
        Hat::Neutral => &[],
    }
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
    fn default_matches_golden_text_protocol() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        assert_eq!(codec.format(), ControllerFormat::Default);
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        state.buttons.set(Button::Capture, true);
        state.hat = Hat::UpRight;
        state.left_stick = StickPosition { x: 0, y: 255 };
        // Fresh connection: both sticks are re-sent (upstream `last_sent` is
        // None) and stick values use unpadded lowercase hex.
        assert_eq!(codec.encode(state), b"0x8013 1 0 ff 80 80\r\n".to_vec());
        codec.commit(state);
        assert_eq!(codec.encode(state), b"0x8010 1\r\n".to_vec());
        codec.reset();
        assert_eq!(codec.encode(state), b"0x8013 1 0 ff 80 80\r\n".to_vec());
    }

    #[test]
    fn default_matches_golden_first_frame_and_each_button() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        let mut pressed = ControllerState::NEUTRAL;
        pressed.buttons.set(Button::A, true);
        // "# press A": first frame of a connection re-sends both sticks.
        assert_eq!(codec.encode(pressed), b"0x0013 8 80 80 80 80\r\n".to_vec());
        codec.commit(pressed);
        assert_eq!(
            codec.encode(ControllerState::NEUTRAL),
            b"0x0000 8\r\n".to_vec()
        );
        codec.commit(ControllerState::NEUTRAL);
        let buttons = [
            (Button::Y, 0x0004),
            (Button::B, 0x0008),
            (Button::A, 0x0010),
            (Button::X, 0x0020),
            (Button::L, 0x0040),
            (Button::R, 0x0080),
            (Button::Zl, 0x0100),
            (Button::Zr, 0x0200),
            (Button::Minus, 0x0400),
            (Button::Plus, 0x0800),
            (Button::Lclick, 0x1000),
            (Button::Rclick, 0x2000),
            (Button::Home, 0x4000),
            (Button::Capture, 0x8000),
        ];
        for (button, expected) in buttons {
            let mut state = ControllerState::NEUTRAL;
            state.buttons.set(button, true);
            let frame = format!("0x{expected:04x} 8\r\n");
            assert_eq!(codec.encode(state), frame.into_bytes(), "{button:?}");
            codec.commit(state);
            assert_eq!(
                codec.encode(ControllerState::NEUTRAL),
                b"0x0000 8\r\n".to_vec(),
                "{button:?} release"
            );
            codec.commit(ControllerState::NEUTRAL);
        }
    }

    #[test]
    fn default_matches_golden_hat_vectors() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        codec.commit(ControllerState::NEUTRAL);
        let hats = [
            (Hat::Up, 0),
            (Hat::UpRight, 1),
            (Hat::Right, 2),
            (Hat::DownRight, 3),
            (Hat::Down, 4),
            (Hat::DownLeft, 5),
            (Hat::Left, 6),
            (Hat::UpLeft, 7),
        ];
        for (hat, expected) in hats {
            let mut state = ControllerState::NEUTRAL;
            state.hat = hat;
            let frame = format!("0x0000 {expected}\r\n");
            assert_eq!(codec.encode(state), frame.into_bytes(), "{hat:?}");
            codec.commit(state);
            assert_eq!(
                codec.encode(ControllerState::NEUTRAL),
                b"0x0000 8\r\n".to_vec(),
                "{hat:?} release"
            );
            codec.commit(ControllerState::NEUTRAL);
        }
    }

    #[test]
    fn default_matches_golden_stick_vectors() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        codec.commit(ControllerState::NEUTRAL);
        let left = |x, y| {
            let mut state = ControllerState::NEUTRAL;
            state.left_stick = StickPosition { x, y };
            state
        };
        let right = |x, y| {
            let mut state = ControllerState::NEUTRAL;
            state.right_stick = StickPosition { x, y };
            state
        };
        let vectors = [
            (left(0x80, 0x00), "0x0002 8 80 0"),
            (left(0x80, 0x80), "0x0002 8 80 80"),
            (left(0xff, 0x80), "0x0002 8 ff 80"),
            (left(0x80, 0x80), "0x0002 8 80 80"),
            (left(0x26, 0xda), "0x0002 8 26 da"),
            (left(0x80, 0x80), "0x0002 8 80 80"),
            (right(0x80, 0x00), "0x0001 8 80 0"),
            (right(0x80, 0x80), "0x0001 8 80 80"),
            (right(0xda, 0xda), "0x0001 8 da da"),
            (right(0x80, 0x80), "0x0001 8 80 80"),
            (left(0xb7, 0x60), "0x0002 8 b7 60"),
            (left(0x80, 0x80), "0x0002 8 80 80"),
            (right(0x62, 0x8b), "0x0001 8 62 8b"),
            (right(0x80, 0x80), "0x0001 8 80 80"),
        ];
        for (state, expected) in vectors {
            let frame = format!("{expected}\r\n");
            assert_eq!(codec.encode(state), frame.into_bytes(), "{expected}");
            codec.commit(state);
        }
    }

    #[test]
    fn default_matches_golden_combination_vector() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        codec.commit(ControllerState::NEUTRAL);
        let mut combined = ControllerState::NEUTRAL;
        combined.buttons.set(Button::B, true);
        combined.buttons.set(Button::Zr, true);
        combined.hat = Hat::UpRight;
        combined.left_stick = StickPosition { x: 0x80, y: 0x00 };
        combined.right_stick = StickPosition { x: 0x00, y: 0x80 };
        assert_eq!(codec.encode(combined), b"0x020b 1 80 0 0 80\r\n".to_vec());
        codec.commit(combined);
        let mut next = ControllerState::NEUTRAL;
        next.buttons.set(Button::Zr, true);
        next.left_stick = StickPosition { x: 0x80, y: 0x80 };
        next.right_stick = StickPosition { x: 0x00, y: 0x80 };
        assert_eq!(codec.encode(next), b"0x0202 8 80 80\r\n".to_vec());
        codec.commit(next);
        let mut last = ControllerState::NEUTRAL;
        last.left_stick = StickPosition { x: 0x80, y: 0x80 };
        last.right_stick = StickPosition { x: 0x80, y: 0x80 };
        assert_eq!(codec.encode(last), b"0x0001 8 80 80\r\n".to_vec());
    }

    #[test]
    fn default_raw_write_forgets_last_state() {
        let mut codec = ControllerCodec::new(ControllerFormat::Default);
        let mut state = ControllerState::NEUTRAL;
        state.left_stick = StickPosition { x: 0x80, y: 0x00 };
        codec.commit(state);
        codec.reset();
        // "# after raw write": both sticks are re-sent after a raw write.
        assert_eq!(codec.encode(state), b"0x0003 8 80 0 80 80\r\n".to_vec());
    }

    #[test]
    fn qingpi_matches_golden_touch_and_combination_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::Qingpi);
        let mut touch_state = ControllerState::NEUTRAL;
        touch_state.touch = Some(TouchPoint::new(100, 50).unwrap());
        assert_eq!(
            codec.encode(touch_state),
            vec![
                0xab, 0x00, 0x00, 0x08, 0x80, 0x80, 0x80, 0x80, 0x64, 0x00, 0x32
            ]
        );
        let mut pressed = ControllerState::NEUTRAL;
        pressed.buttons.set(Button::A, true);
        pressed.touch = Some(TouchPoint::new(319, 239).unwrap());
        assert_eq!(
            codec.encode(pressed),
            vec![
                0xab, 0x04, 0x00, 0x08, 0x80, 0x80, 0x80, 0x80, 0x3f, 0x01, 0xef
            ]
        );
        let mut combined = ControllerState::NEUTRAL;
        combined.buttons.set(Button::B, true);
        combined.buttons.set(Button::Zr, true);
        combined.hat = Hat::UpRight;
        combined.left_stick = StickPosition { x: 0x80, y: 0x00 };
        assert_eq!(
            codec.encode(combined),
            vec![
                0xab, 0x82, 0x00, 0x01, 0x80, 0x00, 0x80, 0x80, 0x00, 0x00, 0x00
            ]
        );
        let mut held = ControllerState::NEUTRAL;
        held.buttons.set(Button::A, true);
        held.buttons.set(Button::L, true);
        held.left_stick = StickPosition { x: 0xff, y: 0x80 };
        assert_eq!(
            codec.encode(held),
            vec![
                0xab, 0x14, 0x00, 0x08, 0xff, 0x80, 0x80, 0x80, 0x00, 0x00, 0x00
            ]
        );
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

    #[test]
    fn controller_formats_report_upstream_display_names() {
        assert_eq!(ControllerFormat::Default.display_name(), "Default");
        assert_eq!(ControllerFormat::Nx.display_name(), "NX");
        assert_eq!(ControllerFormat::Qingpi.display_name(), "Qingpi");
        assert_eq!(
            ControllerFormat::ThreeDsX4.display_name(),
            "3DS Controller(FW:x.4-)"
        );
        assert_eq!(
            ControllerFormat::ThreeDsX3.display_name(),
            "3DS Controller(FW:-x.3)"
        );
    }

    #[test]
    fn controller_formats_expose_transport_attributes() {
        assert_eq!(ControllerFormat::Default.default_baud(), Some(9600));
        assert_eq!(ControllerFormat::Nx.default_baud(), Some(9600));
        assert_eq!(ControllerFormat::Qingpi.default_baud(), Some(9600));
        assert_eq!(ControllerFormat::ThreeDsX4.default_baud(), Some(115_200));
        assert_eq!(ControllerFormat::ThreeDsX3.default_baud(), Some(115_200));
        assert!(ControllerFormat::Default.supports_text());
        assert!(!ControllerFormat::Nx.supports_text());
        assert_eq!(ControllerFormat::Default.end_row(), Some("end"));
        assert_eq!(ControllerFormat::Nx.end_row(), None);
        assert_eq!(ControllerFormat::ThreeDsX3.end_row(), None);
        assert!(ControllerFormat::Qingpi.has_touchscreen());
        assert!(ControllerFormat::ThreeDsX4.has_touchscreen());
        assert!(!ControllerFormat::Default.has_touchscreen());
        assert!(!ControllerFormat::Nx.has_touchscreen());
        assert!(ControllerFormat::Default.allows_raw_write());
        assert!(ControllerFormat::ThreeDsX3.allows_raw_write());
        assert!(!ControllerFormat::Default.skip_unchanged());
        assert!(!ControllerFormat::Qingpi.skip_unchanged());
        assert_eq!(ControllerFormat::Default.raw_write_message(), "");
        assert_eq!(ControllerFormat::Nx.raw_write_log(), "");
        assert_eq!(ControllerFormat::DsFtRev1.default_baud(), None);
        assert!(!ControllerFormat::DsFtRev1.supports_text());
        assert!(!ControllerFormat::DsFtRev1.has_touchscreen());
        assert!(!ControllerFormat::DsFtRev1.allows_raw_write());
        assert!(ControllerFormat::DsFtRev1.skip_unchanged());
        assert!(!ControllerFormat::DsFtRev2.raw_write_message().is_empty());
        assert!(!ControllerFormat::DsFtRev2.raw_write_log().is_empty());
        assert_eq!(ControllerFormat::DsFtRev3.display_name(), "DS FT (Rev3)");
    }

    #[test]
    fn ds_ft_resolution_matches_upstream_names() {
        assert_eq!(
            ControllerFormat::resolve("DS FT (Rev1)"),
            ControllerFormat::DsFtRev1
        );
        assert_eq!(
            ControllerFormat::resolve("DS FT (Rev2)"),
            ControllerFormat::DsFtRev2
        );
        assert_eq!(
            ControllerFormat::resolve("DS FT (Rev3)"),
            ControllerFormat::DsFtRev3
        );
        assert_eq!(
            ControllerFormat::resolve("ds_ft_rev2"),
            ControllerFormat::DsFtRev2
        );
        assert_eq!(
            ControllerFormat::resolve("DS FT"),
            ControllerFormat::Default
        );
        assert_eq!(
            ControllerFormat::parse("ds_ft_rev1"),
            Some(ControllerFormat::DsFtRev1)
        );
    }

    #[test]
    fn ds_ft_rev1_matches_golden_pin_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::DsFtRev1);
        assert_eq!(
            codec.encode(ControllerState::NEUTRAL),
            vec![0x82, 0xff, 0x00, 0x80, 0xff, 0x00]
        );
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::A, true);
        assert_eq!(
            codec.encode(state),
            vec![0x82, 0xef, 0x10, 0x80, 0xff, 0x00]
        );
        for (button, bytes) in [
            (Button::Y, vec![0x82, 0x7f, 0x80, 0x80, 0xff, 0x00]),
            (Button::B, vec![0x82, 0xdf, 0x20, 0x80, 0xff, 0x00]),
            (Button::X, vec![0x82, 0xbf, 0x40, 0x80, 0xff, 0x00]),
            (Button::L, vec![0x82, 0xff, 0x00, 0x80, 0xfe, 0x01]),
            (Button::R, vec![0x82, 0xff, 0x00, 0x80, 0xfd, 0x02]),
            (Button::Minus, vec![0x82, 0xff, 0x00, 0x80, 0xfb, 0x04]),
            (Button::Plus, vec![0x82, 0xff, 0x00, 0x80, 0xf7, 0x08]),
            (Button::Lclick, vec![0x82, 0xff, 0x00, 0x80, 0xef, 0x10]),
        ] {
            let mut state = ControllerState::NEUTRAL;
            state.buttons.set(button, true);
            assert_eq!(codec.encode(state), bytes, "{button:?}");
        }
        for (hat, bytes) in [
            (Hat::Up, vec![0x82, 0xf7, 0x08, 0x80, 0xff, 0x00]),
            (Hat::UpRight, vec![0x82, 0xf3, 0x0c, 0x80, 0xff, 0x00]),
            (Hat::Right, vec![0x82, 0xfb, 0x04, 0x80, 0xff, 0x00]),
            (Hat::DownRight, vec![0x82, 0xf9, 0x06, 0x80, 0xff, 0x00]),
            (Hat::Down, vec![0x82, 0xfd, 0x02, 0x80, 0xff, 0x00]),
            (Hat::DownLeft, vec![0x82, 0xfc, 0x03, 0x80, 0xff, 0x00]),
            (Hat::Left, vec![0x82, 0xfe, 0x01, 0x80, 0xff, 0x00]),
            (Hat::UpLeft, vec![0x82, 0xf6, 0x09, 0x80, 0xff, 0x00]),
        ] {
            let mut state = ControllerState::NEUTRAL;
            state.hat = hat;
            assert_eq!(codec.encode(state), bytes, "{hat:?}");
        }
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::B, true);
        state.hat = Hat::UpRight;
        assert_eq!(
            codec.encode(state),
            vec![0x82, 0xd3, 0x2c, 0x80, 0xff, 0x00]
        );
    }

    #[test]
    fn ds_ft_rev2_matches_golden_pin_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::DsFtRev2);
        assert_eq!(
            codec.encode(ControllerState::NEUTRAL),
            vec![0x82, 0xff, 0x00, 0x80, 0xff, 0x00]
        );
        for (button, bytes) in [
            (Button::Y, vec![0x82, 0xff, 0x00, 0x80, 0xdf, 0x20]),
            (Button::B, vec![0x82, 0xff, 0x00, 0x80, 0xbf, 0x40]),
            (Button::X, vec![0x82, 0xff, 0x00, 0x80, 0x7f, 0x80]),
        ] {
            let mut state = ControllerState::NEUTRAL;
            state.buttons.set(button, true);
            assert_eq!(codec.encode(state), bytes, "{button:?}");
        }
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::B, true);
        state.hat = Hat::UpRight;
        assert_eq!(
            codec.encode(state),
            vec![0x82, 0xf3, 0x0c, 0x80, 0xbf, 0x40]
        );
    }

    #[test]
    fn ds_ft_rev3_matches_golden_pin_vectors() {
        let codec = ControllerCodec::new(ControllerFormat::DsFtRev3);
        assert_eq!(
            codec.encode(ControllerState::NEUTRAL),
            vec![0x82, 0xff, 0x00, 0x80, 0xff, 0x00]
        );
        for (button, bytes) in [
            (Button::Y, vec![0x82, 0xff, 0x00, 0x80, 0xdf, 0x20]),
            (Button::B, vec![0x82, 0xdf, 0x20, 0x80, 0xff, 0x00]),
            (Button::X, vec![0x82, 0xbf, 0x40, 0x80, 0xff, 0x00]),
        ] {
            let mut state = ControllerState::NEUTRAL;
            state.buttons.set(button, true);
            assert_eq!(codec.encode(state), bytes, "{button:?}");
        }
        let mut state = ControllerState::NEUTRAL;
        state.buttons.set(Button::B, true);
        state.hat = Hat::UpRight;
        assert_eq!(
            codec.encode(state),
            vec![0x82, 0xd3, 0x2c, 0x80, 0xff, 0x00]
        );
    }
}
