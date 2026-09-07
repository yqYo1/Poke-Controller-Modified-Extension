//! Typed `MessagePack` IPC over the worker stdin/stdout pipes.

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
