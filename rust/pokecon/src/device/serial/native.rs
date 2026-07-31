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
#[derive(Debug)]
pub struct NativeSerialIo {
    reader: Mutex<ReadHalf<SerialStream>>,
    writer: Mutex<WriteHalf<SerialStream>>,
    closed: CancellationToken,
}

impl NativeSerialIo {
    fn new(stream: SerialStream) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
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
        tokio::select! {
            result = writer.write(bytes) => result,
            () = self.closed.cancelled() => Err(Self::closed_error()),
        }
    }

    async fn read(&self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.closed.is_cancelled() {
            return Err(Self::closed_error());
        }
        let mut reader = self.reader.lock().await;
        tokio::select! {
            result = reader.read(bytes) => result,
            () = self.closed.cancelled() => Err(Self::closed_error()),
        }
    }

    async fn close(&self) -> io::Result<()> {
        self.closed.cancel();
        self.writer.lock().await.shutdown().await
    }
}
