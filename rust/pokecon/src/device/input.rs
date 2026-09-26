//! Generation-safe multi-source input arbitration.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::device::controller::{
    Button, ButtonState, ControllerError, ControllerState, ControllerUpdate, Hat, StickPosition,
    TouchPoint,
};

/// Opaque non-empty ASCII generation issued when an input route is established.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InputGeneration(String);

impl InputGeneration {
    /// Validates an opaque generation without assigning meaning to its spelling.
    ///
    /// # Errors
    ///
    /// Rejects empty and non-ASCII identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, InputError> {
        let value = value.into();
        if value.is_empty() || !value.is_ascii() {
            return Err(InputError::InvalidGeneration);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for InputGeneration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InputGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Arbitrary-precision canonical non-negative decimal sequence number.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSequence(String);

impl InputSequence {
    pub const ZERO: &'static str = "0";

    /// Parses a canonical decimal spelling without JavaScript integer limits.
    ///
    /// # Errors
    ///
    /// Rejects signs, non-digits, empty strings, and redundant leading zeroes.
    pub fn new(value: impl Into<String>) -> Result<Self, InputError> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(InputError::InvalidSequence);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn zero() -> Self {
        Self(Self::ZERO.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == Self::ZERO
    }
}

impl Ord for InputSequence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for InputSequence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for InputSequence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InputSequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Stable identity of one ownership domain, such as one WebSocket or worker.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InputSourceId(String);

impl InputSourceId {
    /// Builds a non-empty source identity.
    ///
    /// # Errors
    ///
    /// Rejects an empty identity.
    pub fn new(value: impl Into<String>) -> Result<Self, InputError> {
        let value = value.into();
        if value.is_empty() {
            return Err(InputError::InvalidSource);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Input origins wired into the same arbiter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputSourceKind {
    Keyboard,
    Mouse,
    BrowserGamepad,
    HardwareController,
    UserScript,
    DynamicConfig,
}

/// Explicit continuous-input priority. Buttons remain unioned by ownership.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InputPriority(u16);

impl InputPriority {
    pub const KEYBOARD_MOUSE: Self = Self(100);
    pub const BROWSER_GAMEPAD: Self = Self(200);
    pub const HARDWARE_CONTROLLER: Self = Self(300);
    pub const DYNAMIC_CONFIG: Self = Self(400);
    pub const USER_SCRIPT: Self = Self(500);

    #[must_use]
    pub const fn custom(value: u16) -> Self {
        Self(value)
    }
}

/// Whether manual intervention may influence the merged output.
/// Denied still allows neutral release and emergency neutralization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ManualInterventionPolicy {
    #[default]
    Allowed,
    Denied,
}

const fn is_manual_kind(kind: InputSourceKind) -> bool {
    matches!(
        kind,
        InputSourceKind::Keyboard
            | InputSourceKind::Mouse
            | InputSourceKind::BrowserGamepad
            | InputSourceKind::HardwareController
    )
}

fn is_neutral_snapshot(snapshot: &InputSnapshot) -> bool {
    snapshot.controller() == ControllerState::NEUTRAL
        && snapshot.keyboard_keys.is_empty()
        && snapshot.mouse_buttons == MouseButtons::default()
}

fn is_controller_update_neutral(update: &crate::device::controller::ControllerUpdate) -> bool {
    let has_pressed = [
        update.a,
        update.b,
        update.x,
        update.y,
        update.l,
        update.r,
        update.zl,
        update.zr,
        update.lclick,
        update.rclick,
        update.plus,
        update.minus,
        update.home,
        update.capture,
    ]
    .into_iter()
    .any(|value| value == Some(true));
    if has_pressed {
        return false;
    }
    if let Some(stick) = update.left_stick {
        let is_center = match stick {
            crate::device::controller::StickInput::Xy(pos) => pos == StickPosition::CENTER,
            crate::device::controller::StickInput::Polar(polar) => polar.strength == 0.0,
        };
        if !is_center {
            return false;
        }
    }
    if let Some(stick) = update.right_stick {
        let is_center = match stick {
            crate::device::controller::StickInput::Xy(pos) => pos == StickPosition::CENTER,
            crate::device::controller::StickInput::Polar(polar) => polar.strength == 0.0,
        };
        if !is_center {
            return false;
        }
    }
    if let Some(hat) = update.hat
        && hat != Hat::Neutral
    {
        return false;
    }
    if let Some(touch) = update.touch
        && touch.pressed
    {
        return false;
    }
    true
}

fn is_release_event(event: &InputEvent) -> bool {
    match event {
        InputEvent::Keyboard { state, .. }
        | InputEvent::MouseButton { state, .. }
        | InputEvent::ControllerButton { state, .. } => *state == PressState::Released,
        InputEvent::Stick { position, .. } => *position == StickPosition::CENTER,
        InputEvent::Hat(hat) => *hat == Hat::Neutral,
        InputEvent::Touch(touch) => touch.is_none(),
        InputEvent::ControllerUpdate(update) => is_controller_update_neutral(update),
    }
}

/// Complete mouse button object required by `input.snapshot`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

/// Closed full-state handoff sent as sequence zero.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputSnapshot {
    pub generation: InputGeneration,
    pub sequence: InputSequence,
    pub keyboard_keys: Vec<String>,
    pub mouse_buttons: MouseButtons,
    pub buttons: ButtonState,
    pub hat: Hat,
    pub left_stick: StickPosition,
    pub right_stick: StickPosition,
    pub touch: Option<TouchPoint>,
}

impl InputSnapshot {
    /// Returns the controller part of the complete snapshot.
    #[must_use]
    pub const fn controller(&self) -> ControllerState {
        ControllerState {
            buttons: self.buttons,
            hat: self.hat,
            left_stick: self.left_stick,
            right_stick: self.right_stick,
            touch: self.touch,
        }
    }
}

/// Press/release state used by discrete input events.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PressState {
    Pressed,
    Released,
}

impl PressState {
    const fn is_pressed(self) -> bool {
        matches!(self, Self::Pressed)
    }
}

/// Mouse buttons accepted by the browser route.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Canonical left/right stick selector.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StickSide {
    #[serde(rename = "LSTICK")]
    Left,
    #[serde(rename = "RSTICK")]
    Right,
}

/// Incremental event shared by browser, hardware, keyboard, mouse, and scripts.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Keyboard {
        key: String,
        state: PressState,
    },
    MouseButton {
        button: MouseButton,
        state: PressState,
    },
    ControllerButton {
        button: Button,
        state: PressState,
    },
    Stick {
        stick: StickSide,
        position: StickPosition,
    },
    Hat(Hat),
    Touch(Option<TouchPoint>),
    ControllerUpdate(ControllerUpdate),
}

/// Keyboard/mouse-to-controller ownership mappings.
#[derive(Clone, Debug, Default)]
pub struct InputBindings {
    keyboard: BTreeMap<String, Button>,
    mouse: BTreeMap<MouseButton, Button>,
}

impl InputBindings {
    #[must_use]
    pub fn new(keyboard: BTreeMap<String, Button>, mouse: BTreeMap<MouseButton, Button>) -> Self {
        Self { keyboard, mouse }
    }

    fn keyboard_button(&self, key: &str) -> Option<Button> {
        self.keyboard.get(key).copied()
    }

    fn mouse_button(&self, button: MouseButton) -> Option<Button> {
        self.mouse.get(&button).copied()
    }
}

#[derive(Clone, Debug)]
struct SourceState {
    kind: InputSourceKind,
    priority: InputPriority,
    generation: InputGeneration,
    sequence: Option<InputSequence>,
    ready: bool,
    update_order: u64,
    keyboard_keys: BTreeSet<String>,
    mouse_buttons: MouseButtons,
    controller: ControllerState,
}

impl SourceState {
    fn awaiting_snapshot(
        kind: InputSourceKind,
        priority: InputPriority,
        generation: InputGeneration,
    ) -> Self {
        Self {
            kind,
            priority,
            generation,
            sequence: None,
            ready: false,
            update_order: 0,
            keyboard_keys: BTreeSet::new(),
            mouse_buttons: MouseButtons::default(),
            controller: ControllerState::NEUTRAL,
        }
    }
}

/// Result of applying an ordered route message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyResult {
    Applied,
    IgnoredOldGeneration,
    IgnoredDuplicateOrOldSequence,
    AwaitingSnapshot,
    IgnoredManualInterventionDenied,
}

/// Acknowledgement emitted after the atomic sequence-zero replacement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotApplied {
    pub generation: InputGeneration,
    pub sequence: InputSequence,
}

/// Authoritative per-source state and deterministic merged output.
#[derive(Clone, Debug)]
pub struct InputArbiter {
    sources: BTreeMap<InputSourceId, SourceState>,
    bindings: InputBindings,
    output: ControllerState,
    order: u64,
    manual_policy: ManualInterventionPolicy,
}

impl InputArbiter {
    #[must_use]
    pub fn new(bindings: InputBindings) -> Self {
        Self {
            sources: BTreeMap::new(),
            bindings,
            output: ControllerState::NEUTRAL,
            order: 0,
            manual_policy: ManualInterventionPolicy::default(),
        }
    }

    /// Returns the current manual intervention policy.
    #[must_use]
    pub const fn manual_policy(&self) -> ManualInterventionPolicy {
        self.manual_policy
    }

    /// Sets the manual intervention policy. Immediate effect from next input.
    /// `Denied` still allows neutral release and emergency `force_release_all`.
    /// Denying neutralizes retained manual source state so re-enabling cannot
    /// resurrect stale input, while script sources remain untouched.
    pub fn set_manual_policy(&mut self, policy: ManualInterventionPolicy) {
        if self.manual_policy == policy {
            return;
        }
        self.manual_policy = policy;
        if policy == ManualInterventionPolicy::Denied {
            for state in self.sources.values_mut() {
                if is_manual_kind(state.kind) {
                    state.keyboard_keys.clear();
                    state.mouse_buttons = MouseButtons::default();
                    state.controller = ControllerState::NEUTRAL;
                }
            }
        }
        self.recompute();
    }

    /// Convenience for the canonical setting surface. Allowed by default.
    #[must_use]
    pub const fn is_manual_allowed(&self) -> bool {
        matches!(self.manual_policy, ManualInterventionPolicy::Allowed)
    }

    pub fn set_manual_allowed(&mut self, allowed: bool) {
        self.set_manual_policy(if allowed {
            ManualInterventionPolicy::Allowed
        } else {
            ManualInterventionPolicy::Denied
        });
    }

    /// Replaces any old route generation and releases all of its ownership
    /// until a complete sequence-zero snapshot arrives.
    pub fn begin_generation(
        &mut self,
        source: InputSourceId,
        kind: InputSourceKind,
        priority: InputPriority,
        generation: InputGeneration,
    ) {
        self.sources.insert(
            source,
            SourceState::awaiting_snapshot(kind, priority, generation),
        );
        self.recompute();
    }

    /// Applies a full sequence-zero state atomically and returns its ACK.
    ///
    /// # Errors
    ///
    /// Rejects non-zero snapshot sequences and snapshots for unknown sources.
    pub fn apply_snapshot(
        &mut self,
        source: &InputSourceId,
        snapshot: InputSnapshot,
    ) -> Result<(ApplyResult, Option<SnapshotApplied>), InputError> {
        if !snapshot.sequence.is_zero() {
            return Err(InputError::SnapshotSequenceNotZero);
        }
        let Some(state) = self.sources.get_mut(source) else {
            return Err(InputError::UnknownSource);
        };
        if state.generation != snapshot.generation {
            return Ok((ApplyResult::IgnoredOldGeneration, None));
        }
        if state.ready {
            return Ok((ApplyResult::IgnoredDuplicateOrOldSequence, None));
        }
        if is_manual_kind(state.kind)
            && self.manual_policy == ManualInterventionPolicy::Denied
            && !is_neutral_snapshot(&snapshot)
        {
            return Ok((ApplyResult::IgnoredManualInterventionDenied, None));
        }
        self.order = self.order.saturating_add(1);
        let controller = snapshot.controller();
        state.sequence = Some(snapshot.sequence.clone());
        state.ready = true;
        state.update_order = self.order;
        state.keyboard_keys = snapshot.keyboard_keys.into_iter().collect();
        state.mouse_buttons = snapshot.mouse_buttons;
        state.controller = controller;
        let acknowledgement = SnapshotApplied {
            generation: snapshot.generation,
            sequence: snapshot.sequence,
        };
        self.recompute();
        Ok((ApplyResult::Applied, Some(acknowledgement)))
    }

    /// Applies one strictly increasing event for the current generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the source does not exist or an update is invalid.
    pub fn apply_event(
        &mut self,
        source: &InputSourceId,
        generation: &InputGeneration,
        sequence: InputSequence,
        event: InputEvent,
    ) -> Result<ApplyResult, InputError> {
        let Some(state) = self.sources.get_mut(source) else {
            return Err(InputError::UnknownSource);
        };
        if &state.generation != generation {
            return Ok(ApplyResult::IgnoredOldGeneration);
        }
        if !state.ready {
            return Ok(ApplyResult::AwaitingSnapshot);
        }
        if state
            .sequence
            .as_ref()
            .is_some_and(|previous| sequence <= *previous)
        {
            return Ok(ApplyResult::IgnoredDuplicateOrOldSequence);
        }
        if is_manual_kind(state.kind)
            && self.manual_policy == ManualInterventionPolicy::Denied
            && !is_release_event(&event)
        {
            return Ok(ApplyResult::IgnoredManualInterventionDenied);
        }
        apply_to_source(state, event)?;
        self.order = self.order.saturating_add(1);
        state.sequence = Some(sequence);
        state.update_order = self.order;
        self.recompute();
        Ok(ApplyResult::Applied)
    }

    /// Releases one source only, including keyboard/mouse disable and worker
    /// or WebSocket disconnect paths.
    pub fn disconnect_source(&mut self, source: &InputSourceId) -> bool {
        let removed = self.sources.remove(source).is_some();
        if removed {
            self.recompute();
        }
        removed
    }

    /// Clears every ownership domain for shutdown or loss of the only transport.
    pub fn force_release_all(&mut self) {
        self.sources.clear();
        self.output = ControllerState::NEUTRAL;
    }

    #[must_use]
    pub const fn output(&self) -> ControllerState {
        self.output
    }

    /// Returns a source kind for diagnostics without exposing mutable state.
    #[must_use]
    pub fn source_kind(&self, source: &InputSourceId) -> Option<InputSourceKind> {
        self.sources.get(source).map(|state| state.kind)
    }

    fn recompute(&mut self) {
        let mut output = ControllerState::NEUTRAL;
        let include_manual = self.manual_policy == ManualInterventionPolicy::Allowed;
        for source in self
            .sources
            .values()
            .filter(|source| source.ready)
            .filter(|source| include_manual || !is_manual_kind(source.kind))
        {
            let effective = effective_controller(source, &self.bindings);
            output.buttons.union_with(effective.buttons);
        }
        // Element-level continuous arbitration: active value wins by priority,
        // inactive values are filtered so handoff returns to the other source.
        let hat_selector = |controller: ControllerState| {
            (controller.hat != Hat::Neutral).then_some(controller.hat)
        };
        let left_selector = |controller: ControllerState| {
            (controller.left_stick != StickPosition::CENTER).then_some(controller.left_stick)
        };
        let right_selector = |controller: ControllerState| {
            (controller.right_stick != StickPosition::CENTER).then_some(controller.right_stick)
        };
        let touch_selector = |controller: ControllerState| controller.touch;
        if include_manual {
            output.hat = self.best_component(hat_selector).unwrap_or(Hat::Neutral);
            output.left_stick = self
                .best_component(left_selector)
                .unwrap_or(StickPosition::CENTER);
            output.right_stick = self
                .best_component(right_selector)
                .unwrap_or(StickPosition::CENTER);
            output.touch = self.best_component(touch_selector);
        } else {
            output.hat = self
                .best_component_where(hat_selector, |kind| !is_manual_kind(kind))
                .unwrap_or(Hat::Neutral);
            output.left_stick = self
                .best_component_where(left_selector, |kind| !is_manual_kind(kind))
                .unwrap_or(StickPosition::CENTER);
            output.right_stick = self
                .best_component_where(right_selector, |kind| !is_manual_kind(kind))
                .unwrap_or(StickPosition::CENTER);
            output.touch = self.best_component_where(touch_selector, |kind| !is_manual_kind(kind));
        }
        self.output = output;
    }

    fn best_component<T: Copy>(&self, select: impl Fn(ControllerState) -> Option<T>) -> Option<T> {
        self.best_component_where(select, |_| true)
    }

    fn best_component_where<T: Copy>(
        &self,
        select: impl Fn(ControllerState) -> Option<T>,
        predicate: impl Fn(InputSourceKind) -> bool,
    ) -> Option<T> {
        self.sources
            .iter()
            .filter(|(_, source)| source.ready && predicate(source.kind))
            .filter_map(|(id, source)| {
                let controller = effective_controller(source, &self.bindings);
                select(controller)
                    .map(|value| ((source.priority, source.update_order, id.clone()), value))
            })
            .max_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, value)| value)
    }
}

impl Default for InputArbiter {
    fn default() -> Self {
        Self::new(InputBindings::default())
    }
}

fn effective_controller(source: &SourceState, bindings: &InputBindings) -> ControllerState {
    let mut controller = source.controller;
    for key in &source.keyboard_keys {
        if let Some(button) = bindings.keyboard_button(key) {
            controller.buttons.set(button, true);
        }
    }
    for (button, pressed) in [
        (MouseButton::Left, source.mouse_buttons.left),
        (MouseButton::Right, source.mouse_buttons.right),
        (MouseButton::Middle, source.mouse_buttons.middle),
    ] {
        if pressed && let Some(mapped) = bindings.mouse_button(button) {
            controller.buttons.set(mapped, true);
        }
    }
    controller
}

fn apply_to_source(state: &mut SourceState, event: InputEvent) -> Result<(), InputError> {
    match event {
        InputEvent::Keyboard { key, state: press } => {
            if press.is_pressed() {
                state.keyboard_keys.insert(key);
            } else {
                state.keyboard_keys.remove(&key);
            }
        }
        InputEvent::MouseButton {
            button,
            state: press,
        } => match button {
            MouseButton::Left => state.mouse_buttons.left = press.is_pressed(),
            MouseButton::Right => state.mouse_buttons.right = press.is_pressed(),
            MouseButton::Middle => state.mouse_buttons.middle = press.is_pressed(),
        },
        InputEvent::ControllerButton {
            button,
            state: press,
        } => state.controller.buttons.set(button, press.is_pressed()),
        InputEvent::Stick { stick, position } => match stick {
            StickSide::Left => state.controller.left_stick = position,
            StickSide::Right => state.controller.right_stick = position,
        },
        InputEvent::Hat(hat) => state.controller.hat = hat,
        InputEvent::Touch(touch) => state.controller.touch = touch,
        InputEvent::ControllerUpdate(update) => state.controller.apply(&update)?,
    }
    Ok(())
}

/// Invalid input or route operation.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum InputError {
    #[error("input generation must be non-empty ASCII")]
    InvalidGeneration,
    #[error("input sequence must be a canonical non-negative decimal string")]
    InvalidSequence,
    #[error("input source identity must not be empty")]
    InvalidSource,
    #[error("input source is not registered")]
    UnknownSource,
    #[error("input snapshot must use sequence zero")]
    SnapshotSequenceNotZero,
    #[error(transparent)]
    Controller(#[from] ControllerError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ApplyResult, InputArbiter, InputBindings, InputEvent, InputGeneration, InputPriority,
        InputSequence, InputSnapshot, InputSourceId, InputSourceKind, ManualInterventionPolicy,
        MouseButtons, PressState, StickSide,
    };
    use crate::device::controller::{
        Button, ButtonState, ControllerState, Hat, StickPosition, TouchPoint,
    };

    fn id(value: &str) -> InputSourceId {
        InputSourceId::new(value).unwrap()
    }

    fn generation(value: &str) -> InputGeneration {
        InputGeneration::new(value).unwrap()
    }

    fn snapshot(generation: &str, controller: ControllerState) -> InputSnapshot {
        InputSnapshot {
            generation: super::InputGeneration::new(generation).unwrap(),
            sequence: InputSequence::zero(),
            keyboard_keys: Vec::new(),
            mouse_buttons: MouseButtons::default(),
            buttons: controller.buttons,
            hat: controller.hat,
            left_stick: controller.left_stick,
            right_stick: controller.right_stick,
            touch: controller.touch,
        }
    }

    #[test]
    fn arbitrary_precision_sequences_compare_numerically() {
        let small = InputSequence::new("99999999999999999999").unwrap();
        let large = InputSequence::new("100000000000000000000").unwrap();
        assert!(large > small);
        assert!(InputSequence::new("00").is_err());
    }

    #[test]
    fn route_switch_releases_old_generation_until_snapshot_ack() {
        let source = id("websocket");
        let mut arbiter = InputArbiter::default();
        arbiter.begin_generation(
            source.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("old"),
        );
        let mut pressed = ControllerState::NEUTRAL;
        pressed.buttons.set(Button::A, true);
        let (_, ack) = arbiter
            .apply_snapshot(&source, snapshot("old", pressed))
            .unwrap();
        assert!(ack.is_some());
        assert!(arbiter.output().buttons.a);

        arbiter.begin_generation(
            source.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("new"),
        );
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        let old = arbiter
            .apply_event(
                &source,
                &generation("old"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert_eq!(old, ApplyResult::IgnoredOldGeneration);
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    #[test]
    fn buttons_union_and_continuous_priority_release_deterministically() {
        let low = id("hardware");
        let high = id("script");
        let mut arbiter = InputArbiter::default();
        arbiter.begin_generation(
            low.clone(),
            InputSourceKind::HardwareController,
            InputPriority::HARDWARE_CONTROLLER,
            generation("h1"),
        );
        let mut low_state = ControllerState::NEUTRAL;
        low_state.buttons.set(Button::A, true);
        low_state.left_stick = StickPosition { x: 0, y: 128 };
        arbiter
            .apply_snapshot(&low, snapshot("h1", low_state))
            .unwrap();

        arbiter.begin_generation(
            high.clone(),
            InputSourceKind::UserScript,
            InputPriority::USER_SCRIPT,
            generation("s1"),
        );
        let mut high_state = ControllerState::NEUTRAL;
        high_state.buttons.set(Button::B, true);
        high_state.left_stick = StickPosition { x: 255, y: 128 };
        arbiter
            .apply_snapshot(&high, snapshot("s1", high_state))
            .unwrap();
        assert!(arbiter.output().buttons.a && arbiter.output().buttons.b);
        assert_eq!(arbiter.output().left_stick.x, 255);

        assert!(arbiter.disconnect_source(&high));
        assert!(arbiter.output().buttons.a);
        assert!(!arbiter.output().buttons.b);
        assert_eq!(arbiter.output().left_stick.x, 0);
        assert!(arbiter.disconnect_source(&low));
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    #[test]
    fn keyboard_disable_releases_only_keyboard_ownership() {
        let bindings = InputBindings::new(
            BTreeMap::from([("KeyA".to_owned(), Button::A)]),
            BTreeMap::new(),
        );
        let keyboard = id("keyboard");
        let script = id("script");
        let mut arbiter = InputArbiter::new(bindings);
        for (source, kind, priority, route) in [
            (
                keyboard.clone(),
                InputSourceKind::Keyboard,
                InputPriority::KEYBOARD_MOUSE,
                "k1",
            ),
            (
                script.clone(),
                InputSourceKind::UserScript,
                InputPriority::USER_SCRIPT,
                "s1",
            ),
        ] {
            arbiter.begin_generation(source.clone(), kind, priority, generation(route));
            arbiter
                .apply_snapshot(&source, snapshot(route, ControllerState::NEUTRAL))
                .unwrap();
        }
        arbiter
            .apply_event(
                &keyboard,
                &generation("k1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Keyboard {
                    key: "KeyA".to_owned(),
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        arbiter.disconnect_source(&keyboard);
        assert_eq!(arbiter.output().buttons.pressed(), [Button::B].into());
    }

    #[test]
    fn duplicates_do_not_change_touch_stick_or_hat() {
        let source = id("browser");
        let route = generation("route");
        let mut arbiter = InputArbiter::default();
        arbiter.begin_generation(
            source.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            route.clone(),
        );
        arbiter
            .apply_snapshot(&source, snapshot("route", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &source,
                &route,
                InputSequence::new("1").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition { x: 2, y: 3 },
                },
            )
            .unwrap();
        let duplicate = arbiter
            .apply_event(
                &source,
                &route,
                InputSequence::new("1").unwrap(),
                InputEvent::Hat(Hat::Up),
            )
            .unwrap();
        assert_eq!(duplicate, ApplyResult::IgnoredDuplicateOrOldSequence);
        assert_eq!(arbiter.output().hat, Hat::Neutral);
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 2, y: 3 });

        arbiter
            .apply_event(
                &source,
                &route,
                InputSequence::new("2").unwrap(),
                InputEvent::Touch(Some(TouchPoint::new(10, 20).unwrap())),
            )
            .unwrap();
        arbiter.force_release_all();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    #[test]
    fn snapshot_json_is_closed_and_complete() {
        let missing = serde_json::from_str::<InputSnapshot>(
            r#"{"generation":"g","sequence":"0","keyboard_keys":[],"mouse_buttons":{"left":false,"right":false,"middle":false},"buttons":{},"hat":"neutral","left_stick":{"x":128,"y":128},"right_stick":{"x":128,"y":128},"touch":null}"#,
        );
        assert!(missing.is_err());
        let extra = InputSnapshot {
            generation: generation("g"),
            sequence: InputSequence::zero(),
            keyboard_keys: vec![],
            mouse_buttons: MouseButtons::default(),
            buttons: ButtonState::default(),
            hat: Hat::Neutral,
            left_stick: StickPosition::CENTER,
            right_stick: StickPosition::CENTER,
            touch: None,
        };
        let mut value = serde_json::to_value(extra).unwrap();
        value["unknown"] = serde_json::json!(true);
        assert!(serde_json::from_value::<InputSnapshot>(value).is_err());
    }

    // --- Adversarial: manual intervention ---
    #[test]
    fn denied_manual_input_ignored_while_script_persists() {
        let script = id("script");
        let manual = id("browser");
        let mut arbiter = InputArbiter::default();
        // script establishes neutral then moves stick
        arbiter.begin_generation(
            script.clone(),
            InputSourceKind::UserScript,
            InputPriority::USER_SCRIPT,
            generation("s1"),
        );
        arbiter
            .apply_snapshot(&script, snapshot("s1", ControllerState::NEUTRAL))
            .unwrap();
        let mut script_stick = ControllerState::NEUTRAL;
        script_stick.left_stick = StickPosition { x: 10, y: 20 };
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: script_stick.left_stick,
                },
            )
            .unwrap();
        // manual connects
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m1"),
        );
        arbiter
            .apply_snapshot(&manual, snapshot("m1", ControllerState::NEUTRAL))
            .unwrap();
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 10, y: 20 });
        // deny manual
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        // manual tries to press button and move sticks – denied
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert_eq!(res, ApplyResult::IgnoredManualInterventionDenied);
        assert!(!arbiter.output().buttons.a);
        let res2 = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition { x: 250, y: 250 },
                },
            )
            .unwrap();
        assert_eq!(res2, ApplyResult::IgnoredManualInterventionDenied);
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 10, y: 20 });
        // script still drives, generation/sequence still validated for manual old gen
        let old_gen = arbiter
            .apply_event(
                &manual,
                &generation("old"),
                InputSequence::new("99").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert_eq!(old_gen, ApplyResult::IgnoredOldGeneration);
        // script can still change
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("2").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.b);
        assert!(!arbiter.output().buttons.a);
    }

    #[test]
    fn denied_manual_snapshot_non_neutral_ignored_neutral_allowed() {
        let manual = id("manual-snap");
        let mut arbiter = InputArbiter::default();
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m1"),
        );
        let mut pressed = ControllerState::NEUTRAL;
        pressed.buttons.set(Button::A, true);
        let res = arbiter
            .apply_snapshot(&manual, snapshot("m1", pressed))
            .unwrap();
        assert_eq!(res.0, ApplyResult::IgnoredManualInterventionDenied);
        assert!(res.1.is_none());
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        // neutral snapshot is allowed as release safety
        let manual2 = id("manual-snap2");
        arbiter.begin_generation(
            manual2.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m2"),
        );
        let (res2, ack2) = arbiter
            .apply_snapshot(&manual2, snapshot("m2", ControllerState::NEUTRAL))
            .unwrap();
        assert_eq!(res2, ApplyResult::Applied);
        assert!(ack2.is_some());
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    #[test]
    fn allowed_manual_element_level_handoff_for_sticks() {
        let script = id("script");
        let manual = id("browser");
        let mut arbiter = InputArbiter::default();
        // Both start neutral
        for (src, kind, prio, r#gen) in [
            (
                script.clone(),
                InputSourceKind::UserScript,
                InputPriority::USER_SCRIPT,
                "s1",
            ),
            (
                manual.clone(),
                InputSourceKind::BrowserGamepad,
                InputPriority::BROWSER_GAMEPAD,
                "m1",
            ),
        ] {
            arbiter.begin_generation(src.clone(), kind, prio, generation(r#gen));
            arbiter
                .apply_snapshot(&src, snapshot(r#gen, ControllerState::NEUTRAL))
                .unwrap();
        }
        // Script drives left stick active
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition { x: 0, y: 0 },
                },
            )
            .unwrap();
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 0, y: 0 });
        // Right stick still neutral from both
        assert_eq!(arbiter.output().right_stick, StickPosition::CENTER);
        // Manual drives left stick – but script has higher priority (500 vs 200) so script still wins when both active.
        // To show element-level, we instead make script inactive for right stick and manual active for right stick.
        // Script left active wins; manual right active should appear because script right is inactive.
        arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Right,
                    position: StickPosition { x: 200, y: 200 },
                },
            )
            .unwrap();
        // left remains script (higher priority), right becomes manual (only active)
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 0, y: 0 });
        assert_eq!(
            arbiter.output().right_stick,
            StickPosition { x: 200, y: 200 }
        );
        // Manual releases right stick to center -> handoff returns to neutral (script had neutral)
        arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Right,
                    position: StickPosition::CENTER,
                },
            )
            .unwrap();
        assert_eq!(arbiter.output().right_stick, StickPosition::CENTER);
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 0, y: 0 });
        // Script releases left -> neutral
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition::CENTER,
                },
            )
            .unwrap();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    fn setup_allowed_pair() -> (InputArbiter, InputSourceId, InputSourceId) {
        let script = id("script");
        let manual = id("browser");
        let mut arbiter = InputArbiter::default();
        for (src, kind, prio, r#gen) in [
            (
                script.clone(),
                InputSourceKind::UserScript,
                InputPriority::USER_SCRIPT,
                "s1",
            ),
            (
                manual.clone(),
                InputSourceKind::BrowserGamepad,
                InputPriority::BROWSER_GAMEPAD,
                "m1",
            ),
        ] {
            arbiter.begin_generation(src.clone(), kind, prio, generation(r#gen));
            arbiter
                .apply_snapshot(&src, snapshot(r#gen, ControllerState::NEUTRAL))
                .unwrap();
        }
        (arbiter, script, manual)
    }

    fn exercise_button_union(
        arbiter: &mut InputArbiter,
        script: &InputSourceId,
        manual: &InputSourceId,
    ) {
        arbiter
            .apply_event(
                script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.a);
        assert!(arbiter.output().buttons.b);
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("2").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Released,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.a);
        assert!(!arbiter.output().buttons.b);
    }

    fn exercise_hat_handoff(
        arbiter: &mut InputArbiter,
        script: &InputSourceId,
        manual: &InputSourceId,
    ) {
        arbiter
            .apply_event(
                script,
                &generation("s1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Hat(Hat::Up),
            )
            .unwrap();
        assert_eq!(arbiter.output().hat, Hat::Up);
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("3").unwrap(),
                InputEvent::Hat(Hat::Right),
            )
            .unwrap();
        assert_eq!(arbiter.output().hat, Hat::Up);
        arbiter
            .apply_event(
                script,
                &generation("s1"),
                InputSequence::new("3").unwrap(),
                InputEvent::Hat(Hat::Neutral),
            )
            .unwrap();
        assert_eq!(arbiter.output().hat, Hat::Right);
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("4").unwrap(),
                InputEvent::Hat(Hat::Neutral),
            )
            .unwrap();
        assert_eq!(arbiter.output().hat, Hat::Neutral);
    }

    fn exercise_touch_handoff(
        arbiter: &mut InputArbiter,
        script: &InputSourceId,
        manual: &InputSourceId,
    ) {
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("5").unwrap(),
                InputEvent::Touch(Some(TouchPoint::new(10, 20).unwrap())),
            )
            .unwrap();
        assert_eq!(
            arbiter.output().touch,
            Some(TouchPoint::new(10, 20).unwrap())
        );
        arbiter
            .apply_event(
                manual,
                &generation("m1"),
                InputSequence::new("6").unwrap(),
                InputEvent::Touch(None),
            )
            .unwrap();
        assert_eq!(arbiter.output().touch, None);
        arbiter
            .apply_event(
                script,
                &generation("s1"),
                InputSequence::new("4").unwrap(),
                InputEvent::Touch(Some(TouchPoint::new(30, 40).unwrap())),
            )
            .unwrap();
        assert_eq!(
            arbiter.output().touch,
            Some(TouchPoint::new(30, 40).unwrap())
        );
    }

    #[test]
    fn allowed_manual_button_union_and_touch_hat_active_handoff() {
        let (mut arbiter, script, manual) = setup_allowed_pair();
        exercise_button_union(&mut arbiter, &script, &manual);
        exercise_hat_handoff(&mut arbiter, &script, &manual);
        exercise_touch_handoff(&mut arbiter, &script, &manual);
    }

    #[test]
    fn denied_manual_release_events_still_allowed_as_safety() {
        let manual = id("browser");
        let mut arbiter = InputArbiter::default();
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m1"),
        );
        // Start allowed, press button
        arbiter
            .apply_snapshot(&manual, snapshot("m1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::X,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.x);
        // Deny – further presses denied, but releases allowed
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        // Output immediately drops manual contribution
        assert!(!arbiter.output().buttons.x);
        // Release event while denied should be allowed and keep neutral
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("2").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::X,
                    state: PressState::Released,
                },
            )
            .unwrap();
        assert_eq!(res, ApplyResult::Applied);
        assert!(!arbiter.output().buttons.x);
        // Non-release stick move while denied -> denied
        let res2 = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("3").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition { x: 10, y: 10 },
                },
            )
            .unwrap();
        assert_eq!(res2, ApplyResult::IgnoredManualInterventionDenied);
        // Stick center (release) while denied -> allowed
        let res3 = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("4").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition::CENTER,
                },
            )
            .unwrap();
        assert_eq!(res3, ApplyResult::Applied);
    }

    #[test]
    fn emergency_neutralization_always_clears_even_when_denied() {
        let script = id("script");
        let manual = id("browser");
        let mut arbiter = InputArbiter::default();
        for (src, kind, prio, r#gen) in [
            (
                script.clone(),
                InputSourceKind::UserScript,
                InputPriority::USER_SCRIPT,
                "s1",
            ),
            (
                manual.clone(),
                InputSourceKind::BrowserGamepad,
                InputPriority::BROWSER_GAMEPAD,
                "m1",
            ),
        ] {
            arbiter.begin_generation(src.clone(), kind, prio, generation(r#gen));
            arbiter
                .apply_snapshot(&src, snapshot(r#gen, ControllerState::NEUTRAL))
                .unwrap();
        }
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.a && arbiter.output().buttons.b);
        // Deny manual – button B drops, A remains due to script
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        assert!(arbiter.output().buttons.a);
        assert!(!arbiter.output().buttons.b);
        // emergency force_release_all clears everything regardless of policy
        arbiter.force_release_all();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        // Even after force release, manual denied still enforced for future non-neutral
        let manual2 = id("browser2");
        arbiter.begin_generation(
            manual2.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m2"),
        );
        // need to set policy still denied
        let (res, _) = arbiter
            .apply_snapshot(
                &manual2,
                snapshot("m2", {
                    let mut s = ControllerState::NEUTRAL;
                    s.buttons.set(Button::Y, true);
                    s
                }),
            )
            .unwrap();
        assert_eq!(res, ApplyResult::IgnoredManualInterventionDenied);
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        // But disconnect remains unconditional
        arbiter.begin_generation(
            manual2.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m3"),
        );
        arbiter
            .apply_snapshot(&manual2, snapshot("m3", ControllerState::NEUTRAL))
            .unwrap();
        // allow manual again to test disconnect path
        arbiter.set_manual_policy(ManualInterventionPolicy::Allowed);
        arbiter
            .apply_event(
                &manual2,
                &generation("m3"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::Y,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.y);
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        // disconnect is unconditional even when denied
        assert!(arbiter.disconnect_source(&manual2));
        assert!(!arbiter.output().buttons.y);
        // force_release_all idempotent when denied
        arbiter.force_release_all();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }

    #[test]
    fn denied_does_not_weaken_generation_sequence_validation() {
        let manual = id("m");
        let mut arbiter = InputArbiter::default();
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("gen1"),
        );
        arbiter
            .apply_snapshot(&manual, snapshot("gen1", ControllerState::NEUTRAL))
            .unwrap();
        // duplicate sequence after denied non-release should still be flagged if release was applied
        arbiter
            .apply_event(
                &manual,
                &generation("gen1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Touch(None),
            )
            .unwrap(); // release allowed
        let dup = arbiter
            .apply_event(
                &manual,
                &generation("gen1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Touch(None),
            )
            .unwrap();
        assert_eq!(dup, ApplyResult::IgnoredDuplicateOrOldSequence);
        // old generation still IgnoredOldGeneration even when denied
        let old = arbiter
            .apply_event(
                &manual,
                &generation("old"),
                InputSequence::new("99").unwrap(),
                InputEvent::Touch(None),
            )
            .unwrap();
        assert_eq!(old, ApplyResult::IgnoredOldGeneration);
    }

    #[test]
    fn script_input_still_applied_when_manual_denied() {
        let script = id("script");
        let manual = id("manual");
        let mut arbiter = InputArbiter::default();
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        for (src, kind, prio, r#gen) in [
            (
                script.clone(),
                InputSourceKind::UserScript,
                InputPriority::USER_SCRIPT,
                "s1",
            ),
            (
                manual.clone(),
                InputSourceKind::BrowserGamepad,
                InputPriority::BROWSER_GAMEPAD,
                "m1",
            ),
        ] {
            arbiter.begin_generation(src.clone(), kind, prio, generation(r#gen));
            arbiter
                .apply_snapshot(&src, snapshot(r#gen, ControllerState::NEUTRAL))
                .unwrap();
        }
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerUpdate(crate::device::controller::ControllerUpdate {
                    a: Some(true),
                    left_stick: Some(crate::device::controller::StickInput::Xy(StickPosition {
                        x: 0,
                        y: 0,
                    })),
                    ..Default::default()
                }),
            )
            .unwrap();
        assert!(arbiter.output().buttons.a);
        assert_eq!(arbiter.output().left_stick, StickPosition { x: 0, y: 0 });
        // manual denied press should not affect
        let _ = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::B,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(!arbiter.output().buttons.b);
        assert!(arbiter.output().buttons.a);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "adversarial input safety matrix intentionally remains in one test"
    )]
    #[test]
    fn denied_neutralizes_all_manual_kinds_and_prevents_resurrection() {
        // Adversarial: establish non-neutral state for every manual kind, deny, then
        // re-enable without fresh input – output must stay neutral and script must survive.
        let bindings = InputBindings::new(
            std::collections::BTreeMap::from([("KeyA".to_owned(), Button::A)]),
            std::collections::BTreeMap::from([(super::MouseButton::Left, Button::B)]),
        );
        let mut arbiter = InputArbiter::new(bindings);
        let script = id("script");
        let keyboard = id("keyboard");
        let mouse_src = id("mouse");
        let browser = id("browser");
        let hardware = id("hardware");

        arbiter.begin_generation(
            script.clone(),
            InputSourceKind::UserScript,
            InputPriority::USER_SCRIPT,
            generation("s1"),
        );
        arbiter
            .apply_snapshot(&script, snapshot("s1", ControllerState::NEUTRAL))
            .unwrap();

        // keyboard manual
        arbiter.begin_generation(
            keyboard.clone(),
            InputSourceKind::Keyboard,
            InputPriority::KEYBOARD_MOUSE,
            generation("k1"),
        );
        arbiter
            .apply_snapshot(&keyboard, snapshot("k1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &keyboard,
                &generation("k1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Keyboard {
                    key: "KeyA".to_owned(),
                    state: PressState::Pressed,
                },
            )
            .unwrap();

        // mouse manual
        arbiter.begin_generation(
            mouse_src.clone(),
            InputSourceKind::Mouse,
            InputPriority::KEYBOARD_MOUSE,
            generation("mou1"),
        );
        arbiter
            .apply_snapshot(&mouse_src, snapshot("mou1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &mouse_src,
                &generation("mou1"),
                InputSequence::new("1").unwrap(),
                InputEvent::MouseButton {
                    button: super::MouseButton::Left,
                    state: PressState::Pressed,
                },
            )
            .unwrap();

        // browser gamepad manual
        arbiter.begin_generation(
            browser.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("b1"),
        );
        arbiter
            .apply_snapshot(&browser, snapshot("b1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &browser,
                &generation("b1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::X,
                    state: PressState::Pressed,
                },
            )
            .unwrap();

        // hardware manual
        arbiter.begin_generation(
            hardware.clone(),
            InputSourceKind::HardwareController,
            InputPriority::HARDWARE_CONTROLLER,
            generation("h1"),
        );
        arbiter
            .apply_snapshot(&hardware, snapshot("h1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &hardware,
                &generation("h1"),
                InputSequence::new("1").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition { x: 10, y: 10 },
                },
            )
            .unwrap();

        // Script-owned state that must survive denial.
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::Y,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Hat(Hat::Up),
            )
            .unwrap();
        arbiter
            .apply_event(
                &script,
                &generation("s1"),
                InputSequence::new("3").unwrap(),
                InputEvent::Touch(Some(TouchPoint::new(100, 100).unwrap())),
            )
            .unwrap();

        // Manual contributions are visible before denial.
        assert!(
            arbiter.output().buttons.x || arbiter.output().buttons.a || arbiter.output().buttons.b
        );
        assert!(arbiter.output().buttons.y);
        assert_eq!(arbiter.output().hat, Hat::Up);

        // Deny – must immediately neutralize manual output and clear retained manual state.
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        // Only script-owned output should remain.
        assert!(!arbiter.output().buttons.x);
        assert!(!arbiter.output().buttons.a);
        assert!(!arbiter.output().buttons.b);
        assert!(arbiter.output().buttons.y);
        assert_eq!(arbiter.output().hat, Hat::Up);
        assert_eq!(
            arbiter.output().touch,
            Some(TouchPoint::new(100, 100).unwrap())
        );
        assert_eq!(arbiter.output().left_stick, StickPosition::CENTER);

        // Release events while denied must still be accepted but keep neutral.
        let rel = arbiter
            .apply_event(
                &keyboard,
                &generation("k1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Keyboard {
                    key: "KeyA".to_owned(),
                    state: PressState::Released,
                },
            )
            .unwrap();
        assert_eq!(rel, ApplyResult::Applied);
        assert!(!arbiter.output().buttons.a);

        // Re-enable without any fresh manual input – stale manual must NOT resurrect.
        arbiter.set_manual_policy(ManualInterventionPolicy::Allowed);
        assert!(!arbiter.output().buttons.x);
        assert!(!arbiter.output().buttons.a);
        assert!(!arbiter.output().buttons.b);
        assert!(arbiter.output().buttons.y);
        assert_eq!(arbiter.output().hat, Hat::Up);
        assert_eq!(arbiter.output().left_stick, StickPosition::CENTER);
        assert_eq!(
            arbiter.output().touch,
            Some(TouchPoint::new(100, 100).unwrap())
        );

        // Fresh manual input after re-enable must work again.
        arbiter
            .apply_event(
                &browser,
                &generation("b1"),
                InputSequence::new("2").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::X,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.x);
        assert!(arbiter.output().buttons.y);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "adversarial input safety matrix intentionally remains in one test"
    )]
    #[test]
    fn denied_reenable_needs_fresh_sequence_and_does_not_weaken_safety_release() {
        let mut arbiter = InputArbiter::default();
        let manual = id("manual");
        let script = id("script");
        arbiter.begin_generation(
            script.clone(),
            InputSourceKind::UserScript,
            InputPriority::USER_SCRIPT,
            generation("s1"),
        );
        arbiter
            .apply_snapshot(&script, snapshot("s1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m1"),
        );
        arbiter
            .apply_snapshot(&manual, snapshot("m1", ControllerState::NEUTRAL))
            .unwrap();
        arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert!(arbiter.output().buttons.a);

        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        assert!(!arbiter.output().buttons.a);
        // Re-enable: same sequence should be duplicate, not resurrect.
        arbiter.set_manual_policy(ManualInterventionPolicy::Allowed);
        assert!(!arbiter.output().buttons.a);
        let dup = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap();
        assert_eq!(dup, ApplyResult::IgnoredDuplicateOrOldSequence);
        assert!(!arbiter.output().buttons.a);

        // Safety release (center stick, neutral hat, null touch, button released) must remain allowed when denied.
        arbiter.set_manual_policy(ManualInterventionPolicy::Denied);
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("2").unwrap(),
                InputEvent::Stick {
                    stick: StickSide::Left,
                    position: StickPosition::CENTER,
                },
            )
            .unwrap();
        assert_eq!(res, ApplyResult::Applied);
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("3").unwrap(),
                InputEvent::Hat(Hat::Neutral),
            )
            .unwrap();
        assert_eq!(res, ApplyResult::Applied);
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("4").unwrap(),
                InputEvent::Touch(None),
            )
            .unwrap();
        assert_eq!(res, ApplyResult::Applied);
        let res = arbiter
            .apply_event(
                &manual,
                &generation("m1"),
                InputSequence::new("5").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Released,
                },
            )
            .unwrap();
        assert_eq!(res, ApplyResult::Applied);
        // Even after denied releases, force_release_all and disconnect remain unconditional.
        assert!(arbiter.disconnect_source(&manual));
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
        arbiter.begin_generation(
            manual.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation("m2"),
        );
        arbiter
            .apply_snapshot(&manual, snapshot("m2", ControllerState::NEUTRAL))
            .unwrap();
        arbiter.force_release_all();
        assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    }
}
