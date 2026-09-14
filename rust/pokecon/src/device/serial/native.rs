use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, ReadHalf, WriteHalf};
use tokio::sync::Mutex;
use tokio_serial::{SerialPortBuilderExt as _, SerialStream};
use tokio_util::sync::CancellationToken;

use super::{PortSelector, SerialBackend, SerialIo};

/// Native Tokio serial backend for Linux tty/udev aliases and Windows COM names.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSerialBackend;

#[async_trait]
impl SerialBackend for NativeSerialBackend {
    async fn open(&self, selector: &PortSelector, baud_rate: u32) -> io::Result<Arc<dyn SerialIo>> {
        let stream = tokio_serial::new(selector.as_str(), baud_rate)
            .open_native_async()
            .map_err(io::Error::other)?;
        Ok(Arc::new(NativeSerialIo::new(stream)))
    }
}

/// Split native port allowing reads and serialized frame writes concurrently.
///
/// Each half is stored behind its own lock as an `Option` so that `close`
/// can take and drop both halves. Dropping both split halves is what
/// actually releases the underlying `SerialStream` file descriptor:
/// `tokio-serial`'s Unix `poll_shutdown` only flushes and never closes, so
/// cancelling plus `WriteHalf::shutdown` alone would leave an exclusive tty
/// open while old `Arc` clones (for example a receive monitor) still exist.
#[derive(Debug)]
pub struct NativeSerialIo {
    reader: Mutex<Option<ReadHalf<SerialStream>>>,
    writer: Mutex<Option<WriteHalf<SerialStream>>>,
    closed: CancellationToken,
}

impl NativeSerialIo {
    fn new(stream: SerialStream) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self {
            reader: Mutex::new(Some(reader)),
            writer: Mutex::new(Some(writer)),
            closed: CancellationToken::new(),
        }
    }

    fn closed_error() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "serial port is closed")
    }
}

#[async_trait]
impl SerialIo for NativeSerialIo {
    async fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed.is_cancelled() {
            return Err(Self::closed_error());
        }
        let mut writer = self.writer.lock().await;
        let Some(inner) = writer.as_mut() else {
            return Err(Self::closed_error());
        };
        tokio::select! {
            result = inner.write(bytes) => result,
            () = self.closed.cancelled() => Err(Self::closed_error()),
        }
    }

    async fn read(&self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.closed.is_cancelled() {
            return Err(Self::closed_error());
        }
        let mut reader = self.reader.lock().await;
        let Some(inner) = reader.as_mut() else {
            return Err(Self::closed_error());
        };
        tokio::select! {
            result = inner.read(bytes) => result,
            () = self.closed.cancelled() => Err(Self::closed_error()),
        }
    }

    async fn close(&self) -> io::Result<()> {
        self.closed.cancel();
        // Take ownership of both halves so the shared split stream is
        // dropped here even while other `Arc` clones of `self` still exist.
        // Each `take` briefly locks only its own half, so a pending read or
        // write already inside `select!` observes cancellation, releases its
        // guard, and lets this proceed. Repeated calls find `None` and stay
        // idempotent.
        let writer = self.writer.lock().await.take();
        if let Some(mut writer) = writer {
            // Best-effort flush matching the previous shutdown behavior; the
            // descriptor is released by the drop below regardless of outcome.
            let _ = writer.shutdown().await;
        }
        let reader = self.reader.lock().await.take();
        drop(reader);
        Ok(())
    }
}
