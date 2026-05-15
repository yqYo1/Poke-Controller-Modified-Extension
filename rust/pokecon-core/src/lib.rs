#![allow(clippy::duplicate_mod)]

//! Poke-Controller core library.
//!
//! This crate provides all core functionality for Poke-Controller:
//! - **Events**: Event bus and autocmd system
//! - **Serial**: Serial communication and controller input
//! - **CV**: Camera capture and image processing
//! - **Notify**: Discord/LINE/Windows notifications
//! - **Net**: MQTT and socket communication
//! - **Lua**: LuaJIT runtime integration

pub mod command_manager;
pub mod profile;
pub mod settings;

pub mod cv;
pub mod events;
pub mod serial;

#[cfg(feature = "notify")]
pub mod notify;

#[cfg(feature = "mqtt")]
pub mod net;

#[cfg(feature = "lua")]
pub mod lua;

// Re-export commonly used types.
pub use cv::{Camera, CameraBackend, CameraConfig, CameraError, FlipMode, Frame, PixelFormat};
pub use events::{Event, EventBus, EventPhase};
pub use serial::{
    Button, Direction, GamepadInput, Hat, SendFormat, SerialFormat, Stick, Tilt, Touchscreen,
};
