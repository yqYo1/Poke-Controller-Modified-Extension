//! Typed `MessagePack` IPC over the worker stdin/stdout pipes.
#![cfg_attr(
    target_os = "windows",
    allow(
        unused_imports,
        reason = "IPC schema exports are consumed by the Linux worker runtime"
    )
)]

mod codec;
mod connection;
mod schema;

pub use connection::{
    ConnectionConfig, ConnectionError, DisconnectReason, IpcConnection, ResourceSafety,
};
pub use schema::{
    Envelope, IpcErrorPayload, IpcValue, LogLevel, LogPayload, LogTarget, ValueCodecError,
    deserialize_value, serialize_value,
};
