//! Native Pro Controller/XInput source and canonical-state recording bridge.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gilrs::{Axis as GilrsAxis, Button as GilrsButton, EventType, Gilrs};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::device::controller::{Button, ControllerState, Hat};
use crate::device::input::{
    InputArbiter, InputError, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons, PressState, StickSide,
};

static NEXT_HARDWARE_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Mapping family selected from the OS-provided controller identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareControllerKind {
    ProController,
    XInput,
    Generic,
}

impl HardwareControllerKind {
    #[must_use]
    pub fn detect(name: &str) -> Self {
        let lowercase = name.to_ascii_lowercase();
        if lowercase.contains("pro controller") || lowercase.contains("nintendo") {
            Self::ProController
        } else if lowercase.contains("xinput")
            || lowercase.contains("xbox")
            || lowercase.contains("x-box")
        {
            Self::XInput
        } else {
            Self::Generic
        }
    }
}

/// Cardinal direction used to combine physical D-pad buttons into one hat.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DpadDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Normalized event independent of gilrs' platform-specific codes.
#[derive(Clone, Debug, PartialEq)]
pub enum HardwareControllerEvent {
    Connected {
        device_id: String,
        name: String,
        kind: HardwareControllerKind,
    },
    Disconnected {
        device_id: String,
    },
    Button {
        device_id: String,
        button: Button,
        state: PressState,
    },
    Dpad {
        device_id: String,
        direction: DpadDirection,
        state: PressState,
    },
    Axis {
        device_id: String,
        stick: StickSide,
        x: Option<u8>,
        y: Option<u8>,
    },
}

/// One canonical frame captured after an accepted hardware state change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordedControllerFrame {
    pub sequence: u64,
    pub state: ControllerState,
}

/// In-memory recording state. Key configuration is intentionally outside this
/// phase; recordings contain only canonical states.
#[derive(Clone, Debug, Default)]
pub struct ControllerRecorder {
    recording: bool,
    next_sequence: u64,
    frames: Vec<RecordedControllerFrame>,
}

impl ControllerRecorder {
    pub fn start(&mut self) {
        self.recording = true;
        self.next_sequence = 0;
        self.frames.clear();
    }

    pub fn stop(&mut self) {
        self.recording = false;
    }

    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording
    }

    #[must_use]
    pub fn frames(&self) -> &[RecordedControllerFrame] {
        &self.frames
    }

    fn capture(&mut self, state: ControllerState) {
        if !self.recording || self.frames.last().is_some_and(|frame| frame.state == state) {
            return;
        }
        self.frames.push(RecordedControllerFrame {
            sequence: self.next_sequence,
            state,
        });
        self.next_sequence = self.next_sequence.saturating_add(1);
    }
}

#[derive(Clone, Debug)]
struct DeviceState {
    source: InputSourceId,
    generation: InputGeneration,
    next_sequence: u64,
    controller: ControllerState,
    dpad: BTreeSet<DpadDirection>,
}

/// Applies native controller events through the same generation-safe arbiter
/// as browser, keyboard, mouse, and user-script sources.
#[derive(Debug, Default)]
pub struct HardwareControllerBridge {
    devices: BTreeMap<String, DeviceState>,
    recorder: ControllerRecorder,
}

impl HardwareControllerBridge {
    #[must_use]
    pub const fn recorder(&self) -> &ControllerRecorder {
        &self.recorder
    }

    #[must_use]
    pub const fn recorder_mut(&mut self) -> &mut ControllerRecorder {
        &mut self.recorder
    }

    /// Applies one normalized native event.
    ///
    /// # Errors
    ///
    /// Returns a closed bridge/input error for invalid source identities or an
    /// event received before its connection event.
    pub fn apply(
        &mut self,
        arbiter: &mut InputArbiter,
        event: HardwareControllerEvent,
    ) -> Result<(), HardwareError> {
        match event {
            HardwareControllerEvent::Connected { device_id, .. } => {
                self.connect(arbiter, device_id)?;
            }
            HardwareControllerEvent::Disconnected { device_id } => {
                let device = self
                    .devices
                    .remove(&device_id)
                    .ok_or(HardwareError::UnknownDevice)?;
                arbiter.disconnect_source(&device.source);
                self.recorder.capture(arbiter.output());
            }
            HardwareControllerEvent::Button {
                device_id,
                button,
                state,
            } => {
                self.apply_incremental(
                    arbiter,
                    &device_id,
                    InputEvent::ControllerButton { button, state },
                )?;
            }
            HardwareControllerEvent::Dpad {
                device_id,
                direction,
                state,
            } => {
                let device = self
                    .devices
                    .get_mut(&device_id)
                    .ok_or(HardwareError::UnknownDevice)?;
                if state == PressState::Pressed {
                    device.dpad.insert(direction);
                } else {
                    device.dpad.remove(&direction);
                }
                let hat = dpad_hat(&device.dpad);
                self.apply_incremental(arbiter, &device_id, InputEvent::Hat(hat))?;
            }
            HardwareControllerEvent::Axis {
                device_id,
                stick,
                x,
                y,
            } => {
                let device = self
                    .devices
                    .get(&device_id)
                    .ok_or(HardwareError::UnknownDevice)?;
                let mut position = match stick {
                    StickSide::Left => device.controller.left_stick,
                    StickSide::Right => device.controller.right_stick,
                };
                if let Some(x) = x {
                    position.x = x;
                }
                if let Some(y) = y {
                    position.y = y;
                }
                self.apply_incremental(arbiter, &device_id, InputEvent::Stick { stick, position })?;
            }
        }
        Ok(())
    }

    fn connect(
        &mut self,
        arbiter: &mut InputArbiter,
        device_id: String,
    ) -> Result<(), HardwareError> {
        if let Some(previous) = self.devices.remove(&device_id) {
            arbiter.disconnect_source(&previous.source);
        }
        let source = InputSourceId::new(format!("hardware:{device_id}"))?;
        let generation_number = NEXT_HARDWARE_GENERATION.fetch_add(1, Ordering::AcqRel);
        let generation = InputGeneration::new(format!("hardware-{device_id}-{generation_number}"))?;
        arbiter.begin_generation(
            source.clone(),
            InputSourceKind::HardwareController,
            InputPriority::HARDWARE_CONTROLLER,
            generation.clone(),
        );
        let controller = ControllerState::NEUTRAL;
        let snapshot = InputSnapshot {
            generation: generation.clone(),
            sequence: InputSequence::zero(),
            keyboard_keys: Vec::new(),
            mouse_buttons: MouseButtons::default(),
            buttons: controller.buttons,
            hat: controller.hat,
            left_stick: controller.left_stick,
            right_stick: controller.right_stick,
            touch: controller.touch,
        };
        arbiter.apply_snapshot(&source, snapshot)?;
        self.devices.insert(
            device_id,
            DeviceState {
                source,
                generation,
                next_sequence: 1,
                controller,
                dpad: BTreeSet::new(),
            },
        );
        self.recorder.capture(arbiter.output());
        Ok(())
    }

    fn apply_incremental(
        &mut self,
        arbiter: &mut InputArbiter,
        device_id: &str,
        event: InputEvent,
    ) -> Result<(), HardwareError> {
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or(HardwareError::UnknownDevice)?;
        let sequence = InputSequence::new(device.next_sequence.to_string())?;
        device.next_sequence = device.next_sequence.saturating_add(1);
        apply_local(&mut device.controller, &event);
        arbiter.apply_event(&device.source, &device.generation, sequence, event)?;
        self.recorder.capture(arbiter.output());
        Ok(())
    }
}

fn apply_local(controller: &mut ControllerState, event: &InputEvent) {
    match event {
        InputEvent::ControllerButton { button, state } => {
            controller
                .buttons
                .set(*button, *state == PressState::Pressed);
        }
        InputEvent::Stick { stick, position } => match stick {
            StickSide::Left => controller.left_stick = *position,
            StickSide::Right => controller.right_stick = *position,
        },
        InputEvent::Hat(hat) => controller.hat = *hat,
        InputEvent::Touch(touch) => controller.touch = *touch,
        InputEvent::ControllerUpdate(update) => {
            let _ = controller.apply(update);
        }
        InputEvent::Keyboard { .. } | InputEvent::MouseButton { .. } => {}
    }
}

fn dpad_hat(directions: &BTreeSet<DpadDirection>) -> Hat {
    let up = directions.contains(&DpadDirection::Up);
    let down = directions.contains(&DpadDirection::Down);
    let left = directions.contains(&DpadDirection::Left);
    let right = directions.contains(&DpadDirection::Right);
    match (up, down, left, right) {
        (true, false, true, false) => Hat::UpLeft,
        (true, false, false, true) => Hat::UpRight,
        (false, true, true, false) => Hat::DownLeft,
        (false, true, false, true) => Hat::DownRight,
        (true, false, false, false) => Hat::Up,
        (false, true, false, false) => Hat::Down,
        (false, false, true, false) => Hat::Left,
        (false, false, false, true) => Hat::Right,
        _ => Hat::Neutral,
    }
}

/// Native gilrs producer. Linux uses udev; Windows enables `XInput`.
#[allow(
    dead_code,
    reason = "the OS event-source entrypoint is retained pending production hardware-loop wiring"
)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GilrsHardwareSource;

impl GilrsHardwareSource {
    /// Starts one blocking OS event pump and returns normalized events.
    #[must_use]
    #[allow(
        dead_code,
        reason = "the OS event-source entrypoint is retained pending production hardware-loop wiring"
    )]
    pub fn spawn(cancellation: CancellationToken) -> mpsc::Receiver<HardwareControllerEvent> {
        let (sender, receiver) = mpsc::channel(128);
        tokio::task::spawn_blocking(move || {
            let Ok(mut gilrs) = Gilrs::new() else {
                tracing::warn!(
                    diagnostic_id = "HARDWARE_CONTROLLER_INIT_FAILED",
                    "native hardware controller initialization failed"
                );
                return;
            };
            while !cancellation.is_cancelled() {
                let Some(event) = gilrs.next_event_blocking(Some(Duration::from_millis(100)))
                else {
                    continue;
                };
                let device_id = event.id.to_string();
                let kind = HardwareControllerKind::detect(gilrs.gamepad(event.id).name());
                if let Some(event) = normalize_gilrs_event(
                    device_id,
                    gilrs.gamepad(event.id).name().to_owned(),
                    kind,
                    event.event,
                ) && sender.blocking_send(event).is_err()
                {
                    break;
                }
            }
        });
        receiver
    }
}

#[allow(
    dead_code,
    reason = "normalization is reached from the retained OS event-source entrypoint"
)]
fn normalize_gilrs_event(
    device_id: String,
    name: String,
    kind: HardwareControllerKind,
    event: EventType,
) -> Option<HardwareControllerEvent> {
    match event {
        EventType::Connected => Some(HardwareControllerEvent::Connected {
            device_id,
            name,
            kind,
        }),
        EventType::Disconnected => Some(HardwareControllerEvent::Disconnected { device_id }),
        EventType::ButtonPressed(button, _) | EventType::ButtonRepeated(button, _) => {
            normalize_button(device_id, kind, button, PressState::Pressed)
        }
        EventType::ButtonReleased(button, _) => {
            normalize_button(device_id, kind, button, PressState::Released)
        }
        EventType::AxisChanged(axis, value, _) => normalize_axis(device_id, axis, value),
        EventType::ButtonChanged(button, value, _) => normalize_button(
            device_id,
            kind,
            button,
            if value >= 0.5 {
                PressState::Pressed
            } else {
                PressState::Released
            },
        ),
        _ => None,
    }
}

fn normalize_button(
    device_id: String,
    kind: HardwareControllerKind,
    button: GilrsButton,
    state: PressState,
) -> Option<HardwareControllerEvent> {
    let dpad = match button {
        GilrsButton::DPadUp => Some(DpadDirection::Up),
        GilrsButton::DPadDown => Some(DpadDirection::Down),
        GilrsButton::DPadLeft => Some(DpadDirection::Left),
        GilrsButton::DPadRight => Some(DpadDirection::Right),
        _ => None,
    };
    if let Some(direction) = dpad {
        return Some(HardwareControllerEvent::Dpad {
            device_id,
            direction,
            state,
        });
    }
    let face_button = if kind == HardwareControllerKind::ProController {
        match button {
            GilrsButton::South => Some(Button::B),
            GilrsButton::East => Some(Button::A),
            GilrsButton::North => Some(Button::X),
            GilrsButton::West => Some(Button::Y),
            _ => None,
        }
    } else {
        match button {
            GilrsButton::South => Some(Button::A),
            GilrsButton::East => Some(Button::B),
            GilrsButton::North => Some(Button::Y),
            GilrsButton::West => Some(Button::X),
            _ => None,
        }
    };
    let button = face_button.or(match button {
        GilrsButton::LeftTrigger => Some(Button::L),
        GilrsButton::RightTrigger => Some(Button::R),
        GilrsButton::LeftTrigger2 => Some(Button::Zl),
        GilrsButton::RightTrigger2 => Some(Button::Zr),
        GilrsButton::Select => Some(Button::Minus),
        GilrsButton::Start => Some(Button::Plus),
        GilrsButton::Mode => Some(Button::Home),
        GilrsButton::LeftThumb => Some(Button::Lclick),
        GilrsButton::RightThumb => Some(Button::Rclick),
        _ => None,
    })?;
    Some(HardwareControllerEvent::Button {
        device_id,
        button,
        state,
    })
}

fn normalize_axis(
    device_id: String,
    axis: GilrsAxis,
    value: f32,
) -> Option<HardwareControllerEvent> {
    let canonical = normalize_axis_value(value);
    let (stick, x, y) = match axis {
        GilrsAxis::LeftStickX => (StickSide::Left, Some(canonical), None),
        GilrsAxis::LeftStickY => (StickSide::Left, None, Some(canonical)),
        GilrsAxis::RightStickX => (StickSide::Right, Some(canonical), None),
        GilrsAxis::RightStickY => (StickSide::Right, None, Some(canonical)),
        _ => return None,
    };
    Some(HardwareControllerEvent::Axis {
        device_id,
        stick,
        x,
        y,
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn normalize_axis_value(value: f32) -> u8 {
    // The clamp and scale establish the exact 0..=255 domain before casting.
    ((value.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8
}

/// Hardware bridge failure.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum HardwareError {
    #[error("hardware event references an unknown device")]
    UnknownDevice,
    #[error(transparent)]
    Input(#[from] InputError),
}

#[cfg(test)]
mod tests {
    use super::{
        DpadDirection, HardwareControllerBridge, HardwareControllerEvent, HardwareControllerKind,
        normalize_axis, normalize_button,
    };
    use crate::device::controller::{Button, ControllerState, Hat};
    use crate::device::input::{InputArbiter, PressState};
    use gilrs::{Axis as GilrsAxis, Button as GilrsButton};

    fn connected(id: &str, kind: HardwareControllerKind) -> HardwareControllerEvent {
        HardwareControllerEvent::Connected {
            device_id: id.to_owned(),
            name: "controller".to_owned(),
            kind,
        }
    }

    #[test]
    fn pro_and_xinput_face_layouts_are_distinct() {
        let pro = normalize_button(
            "p".to_owned(),
            HardwareControllerKind::ProController,
            GilrsButton::South,
            PressState::Pressed,
        )
        .unwrap();
        let xbox = normalize_button(
            "x".to_owned(),
            HardwareControllerKind::XInput,
            GilrsButton::South,
            PressState::Pressed,
        )
        .unwrap();
        assert!(matches!(
            pro,
            HardwareControllerEvent::Button {
                button: Button::B,
                ..
            }
        ));
        assert!(matches!(
            xbox,
            HardwareControllerEvent::Button {
                button: Button::A,
                ..
            }
        ));
    }

    #[test]
    fn unknown_devices_and_axes_are_normalized_canonically() {
        assert_eq!(
            HardwareControllerKind::detect("Nintendo Switch Pro Controller"),
            HardwareControllerKind::ProController
        );
        assert_eq!(
            HardwareControllerKind::detect("unmapped controller"),
            HardwareControllerKind::Generic
        );
        assert!(matches!(
            normalize_axis("one".to_owned(), GilrsAxis::LeftStickX, 1.0),
            Some(HardwareControllerEvent::Axis {
                x: Some(255),
                y: None,
                ..
            })
        ));
    }

    #[test]
    fn bridge_records_canonical_state_and_disconnect_releases() {
        let mut arbiter = InputArbiter::default();
        let mut bridge = HardwareControllerBridge::default();
        bridge.recorder_mut().start();
        assert!(bridge.recorder().is_recording());
        bridge
            .apply(
                &mut arbiter,
                connected("one", HardwareControllerKind::ProController),
            )
            .unwrap();
        bridge
            .apply(
                &mut arbiter,
                HardwareControllerEvent::Button {
                    device_id: "one".to_owned(),
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        bridge
            .apply(
                &mut arbiter,
                HardwareControllerEvent::Dpad {
                    device_id: "one".to_owned(),
                    direction: DpadDirection::Up,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        bridge
            .apply(
                &mut arbiter,
                HardwareControllerEvent::Dpad {
                    device_id: "one".to_owned(),
                    direction: DpadDirection::Right,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.a);
        assert_eq!(arbiter.output().hat, Hat::UpRight);
        assert!(bridge.recorder().frames().len() >= 3);

        bridge
            .apply(
                &mut arbiter,
                HardwareControllerEvent::Disconnected {
                    device_id: "one".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        assert_eq!(
            bridge.recorder().frames().last().unwrap().state,
            ControllerState::NEUTRAL
        );
        bridge.recorder_mut().stop();
        assert!(!bridge.recorder().is_recording());
    }
}
