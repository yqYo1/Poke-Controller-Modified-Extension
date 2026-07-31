pub(crate) use pokecon_device::controller::{
    Button, ControllerState, ControllerUpdate, Hat, StickInput, StickPosition, TouchPoint,
    TouchUpdate,
};
pub(crate) use pokecon_device::input::{
    ApplyResult, InputArbiter, InputError, InputEvent, InputGeneration, InputPriority,
    InputSequence, InputSnapshot, InputSourceId, InputSourceKind, MouseButton, MouseButtons,
    PressState, StickSide,
};
pub(crate) use pokecon_device::notification::{
    DiscordNotificationConfig, DiscordTransport, DiscordWebhookUrl, NotificationChannel,
    NotificationConfig, NotificationOutcome, NotificationService, ReqwestDiscordTransport,
    UnavailableDiscordTransport, WindowsNativeNotificationTransport, WindowsNotificationConfig,
};
pub(crate) use pokecon_device::serial::{
    ControllerFormat, NativeSerialBackend, SerialConfig, SerialError, SerialManager,
    enumerate_native_ports,
};

#[cfg(test)]
pub(crate) use pokecon_device::serial::{
    VirtualOpenPlan, VirtualSerialBackend, VirtualSerialEndpoint,
};
