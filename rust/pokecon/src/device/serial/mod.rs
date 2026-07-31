//! Serial selectors, controller codecs, transactional connection management,
//! and a deterministic virtual fixture.

mod codec;
mod manager;
mod native;
mod selector;
mod virtual_port;

pub use codec::{ControllerCodec, ControllerFormat};
pub use manager::{ReconnectPolicy, SerialConfig, SerialError, SerialManager};
pub use native::{NativeSerialBackend, NativeSerialIo};
pub use selector::{
    PortCandidate, PortSelector, SelectorClass, choose_configured, enumerate_native_ports,
    select_linux_candidates,
};
pub use virtual_port::{VirtualOpenPlan, VirtualSerialBackend, VirtualSerialEndpoint};

use std::io;
use std::sync::Arc;

use async_trait::async_trait;

/// One concurrently readable/writable serial connection.
#[async_trait]
pub trait SerialIo: Send + Sync {
    /// Writes at most the supplied byte count and may complete partially.
    async fn write(&self, bytes: &[u8]) -> io::Result<usize>;

    /// Reads available raw bytes, returning zero for EOF.
    async fn read(&self, bytes: &mut [u8]) -> io::Result<usize>;

    /// Cancels pending I/O and closes the port.
    async fn close(&self) -> io::Result<()>;
}

/// Opens platform serial selectors without changing their spelling.
#[async_trait]
pub trait SerialBackend: Send + Sync {
    /// Opens one validated selector at a positive baud rate.
    async fn open(&self, selector: &PortSelector, baud_rate: u32) -> io::Result<Arc<dyn SerialIo>>;
}
