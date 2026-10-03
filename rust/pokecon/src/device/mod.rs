//! Canonical controller input, serial transport, and notification services.

pub mod controller;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the hardware bridge is retained and compiled through its unit tests"
    )
)]
pub mod hardware;
pub mod input;
pub mod notification;
pub mod serial;

pub(crate) use controller::{
    Button, ControllerState, ControllerUpdate, Hat, StickInput, StickPosition, TouchPoint,
    TouchUpdate,
};
pub(crate) use input::{
    ApplyResult, InputArbiter, InputError, InputEvent, InputGeneration, InputPriority,
    InputSequence, InputSnapshot, InputSourceId, InputSourceKind, MouseButton, MouseButtons,
    PressState, StickSide,
};
pub(crate) use notification::{
    DiscordNotificationConfig, DiscordTransport, DiscordWebhookUrl, NotificationChannel,
    NotificationConfig, NotificationOutcome, NotificationService, ReqwestDiscordTransport,
    UnavailableDiscordTransport, WindowsNativeNotificationTransport, WindowsNotificationConfig,
};
pub(crate) use serial::{
    ControllerFormat, NativeSerialBackend, SerialConfig, SerialError, SerialManager,
    enumerate_native_ports,
};

#[cfg(test)]
pub(crate) use serial::{VirtualOpenPlan, VirtualSerialBackend, VirtualSerialEndpoint};
