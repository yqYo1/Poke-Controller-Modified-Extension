use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::{PortSelector, SerialBackend, SerialIo};

/// One deterministic result returned by a virtual backend open attempt.
#[derive(Clone, Debug)]
pub enum VirtualOpenPlan {
    Success(VirtualSerialEndpoint),
    Fail(io::ErrorKind),
}

/// Queue-driven backend for rollback and reconnect contract tests.
#[derive(Clone, Debug, Default)]
pub struct VirtualSerialBackend {
    plans: Arc<Mutex<VecDeque<VirtualOpenPlan>>>,
    attempts: Arc<AtomicUsize>,
    opened_selectors: Arc<Mutex<Vec<(String, u32)>>>,
}

impl VirtualSerialBackend {
    /// Appends an open result in FIFO order.
    pub async fn push_plan(&self, plan: VirtualOpenPlan) {
        self.plans.lock().await.push_back(plan);
    }

    #[must_use]
    pub fn attempt_count(&self) -> usize {
        self.attempts.load(Ordering::Acquire)
    }

    /// Raw selectors and baud values observed by the backend.
    pub async fn opened_selectors(&self) -> Vec<(String, u32)> {
        self.opened_selectors.lock().await.clone()
    }
}

#[async_trait]
impl SerialBackend for VirtualSerialBackend {
    async fn open(&self, selector: &PortSelector, baud_rate: u32) -> io::Result<Arc<dyn SerialIo>> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        self.opened_selectors
            .lock()
            .await
            .push((selector.as_str().to_owned(), baud_rate));
        match self.plans.lock().await.pop_front() {
            Some(VirtualOpenPlan::Success(endpoint)) => Ok(Arc::new(endpoint)),
            Some(VirtualOpenPlan::Fail(kind)) => {
                Err(io::Error::new(kind, "virtual serial open failure"))
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "virtual serial open plan exhausted",
            )),
        }
    }
}

#[derive(Clone, Debug)]
enum ReadPlan {
    Data(Vec<u8>),
    Error(io::ErrorKind),
    Eof,
}

#[derive(Debug, Default)]
struct ReadState {
    plans: VecDeque<ReadPlan>,
    remainder: VecDeque<u8>,
}

#[derive(Debug)]
struct EndpointInner {
    written: Mutex<Vec<u8>>,
    write_calls: AtomicUsize,
    active_writes: AtomicUsize,
    maximum_active_writes: AtomicUsize,
    maximum_write: AtomicUsize,
    write_delay: Mutex<Duration>,
    next_write_error: Mutex<Option<io::ErrorKind>>,
    reads: Mutex<ReadState>,
    read_ready: Notify,
    closed: CancellationToken,
}

/// Controllable in-memory serial endpoint with partial-write and raw-read faults.
#[derive(Clone, Debug)]
pub struct VirtualSerialEndpoint {
    inner: Arc<EndpointInner>,
}

impl VirtualSerialEndpoint {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EndpointInner {
                written: Mutex::new(Vec::new()),
                write_calls: AtomicUsize::new(0),
                active_writes: AtomicUsize::new(0),
                maximum_active_writes: AtomicUsize::new(0),
                maximum_write: AtomicUsize::new(usize::MAX),
                write_delay: Mutex::new(Duration::ZERO),
                next_write_error: Mutex::new(None),
                reads: Mutex::new(ReadState::default()),
                read_ready: Notify::new(),
                closed: CancellationToken::new(),
            }),
        }
    }

    /// Limits each low-level write. Zero simulates write-zero.
    pub fn set_maximum_write(&self, bytes: usize) {
        self.inner.maximum_write.store(bytes, Ordering::Release);
    }

    /// Adds latency to expose accidental concurrent frame writes.
    pub async fn set_write_delay(&self, delay: Duration) {
        *self.inner.write_delay.lock().await = delay;
    }

    /// Makes the next write fail before accepting bytes.
    pub async fn fail_next_write(&self, kind: io::ErrorKind) {
        *self.inner.next_write_error.lock().await = Some(kind);
    }

    /// Makes raw bytes available to the receive monitor.
    pub async fn push_read_data(&self, bytes: impl Into<Vec<u8>>) {
        self.inner
            .reads
            .lock()
            .await
            .plans
            .push_back(ReadPlan::Data(bytes.into()));
        self.inner.read_ready.notify_waiters();
    }

    /// Makes the next read fail.
    pub async fn push_read_error(&self, kind: io::ErrorKind) {
        self.inner
            .reads
            .lock()
            .await
            .plans
            .push_back(ReadPlan::Error(kind));
        self.inner.read_ready.notify_waiters();
    }

    /// Makes the next read return EOF.
    pub async fn push_eof(&self) {
        self.inner.reads.lock().await.plans.push_back(ReadPlan::Eof);
        self.inner.read_ready.notify_waiters();
    }

    /// All accepted bytes in physical order.
    pub async fn written(&self) -> Vec<u8> {
        self.inner.written.lock().await.clone()
    }

    #[must_use]
    pub fn write_call_count(&self) -> usize {
        self.inner.write_calls.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn maximum_concurrent_writes(&self) -> usize {
        self.inner.maximum_active_writes.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.is_cancelled()
    }

    fn closed_error() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "virtual serial endpoint closed")
    }
}

impl Default for VirtualSerialEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

struct ActiveWriteGuard<'a> {
    active: &'a AtomicUsize,
}

impl Drop for ActiveWriteGuard<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[async_trait]
impl SerialIo for VirtualSerialEndpoint {
    async fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        if self.inner.closed.is_cancelled() {
            return Err(Self::closed_error());
        }
        self.inner.write_calls.fetch_add(1, Ordering::AcqRel);
        let active = self.inner.active_writes.fetch_add(1, Ordering::AcqRel) + 1;
        self.inner
            .maximum_active_writes
            .fetch_max(active, Ordering::AcqRel);
        let _guard = ActiveWriteGuard {
            active: &self.inner.active_writes,
        };
        let delay = *self.inner.write_delay.lock().await;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(kind) = self.inner.next_write_error.lock().await.take() {
            return Err(io::Error::new(kind, "virtual serial write failure"));
        }
        let count = bytes
            .len()
            .min(self.inner.maximum_write.load(Ordering::Acquire));
        self.inner
            .written
            .lock()
            .await
            .extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    async fn read(&self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.inner.closed.is_cancelled() {
                return Err(Self::closed_error());
            }
            let notified = self.inner.read_ready.notified();
            {
                let mut state = self.inner.reads.lock().await;
                if !state.remainder.is_empty() {
                    let count = bytes.len().min(state.remainder.len());
                    for destination in &mut bytes[..count] {
                        *destination = state.remainder.pop_front().expect("length checked");
                    }
                    return Ok(count);
                }
                match state.plans.pop_front() {
                    Some(ReadPlan::Data(data)) => state.remainder.extend(data),
                    Some(ReadPlan::Error(kind)) => {
                        return Err(io::Error::new(kind, "virtual serial read failure"));
                    }
                    Some(ReadPlan::Eof) => return Ok(0),
                    None => {}
                }
                if !state.remainder.is_empty() {
                    continue;
                }
            }
            tokio::select! {
                () = notified => {}
                () = self.inner.closed.cancelled() => return Err(Self::closed_error()),
            }
        }
    }

    async fn close(&self) -> io::Result<()> {
        self.inner.closed.cancel();
        self.inner.read_ready.notify_waiters();
        Ok(())
    }
}
