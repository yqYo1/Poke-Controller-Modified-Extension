use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::codec::{CodecError, read_frame, write_frame};
use super::schema::{Envelope, IpcErrorPayload, IpcValue, LogPayload};

/// Rust-owned emergency release boundary invoked on every transport loss.
///
/// Implementations release controller buttons, center sticks, and release touch
/// state without relying on Python/Lua destructors or `finally` blocks.
pub trait ResourceSafety: Send + Sync + 'static {
    /// Forces every resource controlled by this worker into its safe state.
    fn force_release(&self);
}

#[derive(Debug)]
struct NoopResourceSafety;

impl ResourceSafety for NoopResourceSafety {
    fn force_release(&self) {}
}

/// Bounded queue capacities for one IPC endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionConfig {
    /// Frames waiting for the independent writer task.
    pub writer_queue_capacity: usize,
    /// Requests, events, and logs waiting for the endpoint consumer.
    pub inbound_queue_capacity: usize,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            writer_queue_capacity: 64,
            inbound_queue_capacity: 64,
        }
    }
}

impl ConnectionConfig {
    fn validate(self) -> Result<Self, ConnectionError> {
        if self.writer_queue_capacity == 0 || self.inbound_queue_capacity == 0 {
            return Err(ConnectionError::InvalidCapacity);
        }
        Ok(self)
    }
}

/// First cause that atomically disconnected an endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    /// Reader reached EOF or an I/O/task termination path.
    ReaderTerminated(String),
    /// Writer reached broken pipe or an I/O/task termination path.
    WriterTerminated(String),
    /// A malformed or oversized inbound frame violated the protocol.
    ProtocolViolation(String),
    /// The local owner deliberately closed the endpoint.
    LocalShutdown,
    /// The supervised operating-system process was reaped.
    ProcessExited,
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReaderTerminated(detail) => write!(formatter, "reader terminated: {detail}"),
            Self::WriterTerminated(detail) => write!(formatter, "writer terminated: {detail}"),
            Self::ProtocolViolation(detail) => write!(formatter, "protocol violation: {detail}"),
            Self::LocalShutdown => formatter.write_str("local shutdown"),
            Self::ProcessExited => formatter.write_str("worker process exited"),
        }
    }
}

/// Request, queue, protocol, or disconnect failure.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
pub enum ConnectionError {
    /// Queue capacities must be positive.
    #[error("IPC queue capacities must be positive")]
    InvalidCapacity,
    /// Endpoint completed its exactly-once disconnect transition.
    #[error("IPC transport disconnected: {0}")]
    Disconnected(DisconnectReason),
    /// Outbound queue was full for a non-blocking send.
    #[error("IPC writer queue is full")]
    QueueFull,
    /// A caller-supplied cancellation token cancelled its own request wait.
    #[error("IPC request was cancelled")]
    Cancelled,
    /// Encoding failed before a frame produced protocol side effects.
    #[error(transparent)]
    Codec(CodecError),
    /// Peer returned a stable closed error response.
    #[error("remote IPC error {code}: {message}")]
    Remote {
        /// Stable machine-readable code.
        code: String,
        /// Secret-free diagnostic.
        message: String,
    },
    /// Internal response channel vanished without a recorded disconnect.
    #[error("IPC response waiter closed unexpectedly")]
    WaiterClosed,
    /// Request ID wrapped into an outstanding correlation ID.
    #[error("IPC request ID {0} is already pending")]
    DuplicateRequestId(u64),
}

type PendingResult = Result<Envelope, ConnectionError>;
type PendingSender = oneshot::Sender<PendingResult>;

#[derive(Debug)]
struct DisconnectState {
    reason: Option<DisconnectReason>,
    pending: HashMap<u64, PendingSender>,
}

struct SharedState {
    disconnect: Mutex<DisconnectState>,
    cancellation: CancellationToken,
    resource_safety: Arc<dyn ResourceSafety>,
    release_count: AtomicUsize,
    disconnect_count: AtomicUsize,
    late_response_count: AtomicUsize,
}

impl std::fmt::Debug for SharedState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedState")
            .field("disconnect_reason", &self.disconnect_reason())
            .field("release_count", &self.release_count.load(Ordering::Acquire))
            .field(
                "disconnect_count",
                &self.disconnect_count.load(Ordering::Acquire),
            )
            .field(
                "late_response_count",
                &self.late_response_count.load(Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

impl SharedState {
    fn new(resource_safety: Arc<dyn ResourceSafety>) -> Self {
        Self {
            disconnect: Mutex::new(DisconnectState {
                reason: None,
                pending: HashMap::new(),
            }),
            cancellation: CancellationToken::new(),
            resource_safety,
            release_count: AtomicUsize::new(0),
            disconnect_count: AtomicUsize::new(0),
            late_response_count: AtomicUsize::new(0),
        }
    }

    fn lock_disconnect(&self) -> MutexGuard<'_, DisconnectState> {
        self.disconnect
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn disconnect(&self, reason: DisconnectReason) -> bool {
        let waiters = {
            let mut state = self.lock_disconnect();
            if state.reason.is_some() {
                return false;
            }
            state.reason = Some(reason.clone());
            state
                .pending
                .drain()
                .map(|(_, waiter)| waiter)
                .collect::<Vec<_>>()
        };

        self.disconnect_count.fetch_add(1, Ordering::AcqRel);
        self.cancellation.cancel();
        let error = ConnectionError::Disconnected(reason);
        for waiter in waiters {
            let _send_result = waiter.send(Err(error.clone()));
        }
        self.force_release_once();
        true
    }

    fn force_release_once(&self) {
        if self
            .release_count
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let _release_result = catch_unwind(AssertUnwindSafe(|| {
                self.resource_safety.force_release();
            }));
        }
    }

    fn insert_pending(&self, id: u64, waiter: PendingSender) -> Result<(), ConnectionError> {
        let mut state = self.lock_disconnect();
        if let Some(reason) = &state.reason {
            return Err(ConnectionError::Disconnected(reason.clone()));
        }
        match state.pending.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(waiter);
            }
            Entry::Occupied(_) => return Err(ConnectionError::DuplicateRequestId(id)),
        }
        Ok(())
    }

    fn remove_pending(&self, id: u64) {
        self.lock_disconnect().pending.remove(&id);
    }

    fn complete_pending(&self, envelope: Envelope) {
        let Some(id) = envelope.id() else {
            return;
        };
        let waiter = self.lock_disconnect().pending.remove(&id);
        if let Some(waiter) = waiter {
            let _send_result = waiter.send(Ok(envelope));
        } else {
            self.late_response_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.lock_disconnect().reason.clone()
    }

    fn disconnected_error(&self) -> ConnectionError {
        ConnectionError::Disconnected(
            self.disconnect_reason()
                .unwrap_or(DisconnectReason::LocalShutdown),
        )
    }
}

struct Outbound {
    envelope: Envelope,
    completion: oneshot::Sender<Result<(), ConnectionError>>,
}

struct DisconnectGuard {
    state: Arc<SharedState>,
    fallback_reason: DisconnectReason,
}

impl DisconnectGuard {
    fn new(state: Arc<SharedState>, fallback_reason: DisconnectReason) -> Self {
        Self {
            state,
            fallback_reason,
        }
    }

    fn set_reason(&mut self, reason: DisconnectReason) {
        self.fallback_reason = reason;
    }
}

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        self.state.disconnect(self.fallback_reason.clone());
    }
}

struct PendingGuard {
    state: Arc<SharedState>,
    id: Option<u64>,
}

impl PendingGuard {
    fn new(state: Arc<SharedState>, id: u64) -> Self {
        Self {
            state,
            id: Some(id),
        }
    }

    fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.state.remove_pending(id);
        }
    }
}

#[derive(Debug)]
struct TaskSet {
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

struct ConnectionInner {
    state: Arc<SharedState>,
    outbound: mpsc::Sender<Outbound>,
    inbound: tokio::sync::Mutex<mpsc::Receiver<Envelope>>,
    tasks: Mutex<Option<TaskSet>>,
    next_request_id: AtomicU64,
}

impl std::fmt::Debug for ConnectionInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectionInner")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Drop for ConnectionInner {
    fn drop(&mut self) {
        self.state.disconnect(DisconnectReason::LocalShutdown);
        if let Some(tasks) = self
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            tasks.reader.abort();
            tasks.writer.abort();
        }
    }
}

/// Cloneable endpoint with independent reader/writer tasks and correlated waiters.
#[derive(Clone, Debug)]
pub struct IpcConnection {
    inner: Arc<ConnectionInner>,
}

impl IpcConnection {
    /// Starts a framed endpoint over independent read/write pipe halves.
    ///
    /// # Errors
    ///
    /// Returns an error when either configured queue capacity is zero.
    pub fn spawn<R, W>(
        reader: R,
        writer: W,
        config: ConnectionConfig,
        resource_safety: Arc<dyn ResourceSafety>,
    ) -> Result<Self, ConnectionError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let config = config.validate()?;
        let state = Arc::new(SharedState::new(resource_safety));
        let (outbound, outbound_receiver) = mpsc::channel(config.writer_queue_capacity);
        let (inbound_sender, inbound) = mpsc::channel(config.inbound_queue_capacity);
        let reader_task = tokio::spawn(reader_loop(reader, inbound_sender, state.clone()));
        let writer_task = tokio::spawn(writer_loop(writer, outbound_receiver, state.clone()));
        Ok(Self {
            inner: Arc::new(ConnectionInner {
                state,
                outbound,
                inbound: tokio::sync::Mutex::new(inbound),
                tasks: Mutex::new(Some(TaskSet {
                    reader: reader_task,
                    writer: writer_task,
                })),
                next_request_id: AtomicU64::new(0),
            }),
        })
    }

    /// Starts an endpoint without controller/resource ownership.
    ///
    /// # Errors
    ///
    /// Returns an error when either queue capacity is zero.
    pub fn spawn_without_resources<R, W>(
        reader: R,
        writer: W,
        config: ConnectionConfig,
    ) -> Result<Self, ConnectionError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::spawn(reader, writer, config, Arc::new(NoopResourceSafety))
    }

    /// Receives the next request, event, or log envelope.
    ///
    /// Responses are consumed by the pending waiter table and never appear here.
    ///
    /// # Errors
    ///
    /// Returns the endpoint's first disconnect reason.
    pub async fn recv(&self) -> Result<Envelope, ConnectionError> {
        let mut inbound = self.inner.inbound.lock().await;
        tokio::select! {
            biased;
            () = self.inner.state.cancellation.cancelled() => {
                Err(self.inner.state.disconnected_error())
            }
            envelope = inbound.recv() => {
                envelope.ok_or_else(|| self.inner.state.disconnected_error())
            }
        }
    }

    /// Sends an envelope with bounded-queue backpressure and waits for its frame
    /// to be flushed by the independent writer task.
    ///
    /// # Errors
    ///
    /// Returns a codec or disconnect error.
    pub async fn send(&self, envelope: Envelope) -> Result<(), ConnectionError> {
        let (completion_sender, completion_receiver) = oneshot::channel();
        let outbound = Outbound {
            envelope,
            completion: completion_sender,
        };
        tokio::select! {
            biased;
            () = self.inner.state.cancellation.cancelled() => {
                return Err(self.inner.state.disconnected_error());
            }
            result = self.inner.outbound.send(outbound) => {
                if result.is_err() {
                    return Err(self.inner.state.disconnected_error());
                }
            }
        }
        self.await_write(completion_receiver).await
    }

    /// Attempts a non-blocking enqueue, then waits for a successful enqueue to
    /// be flushed.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::QueueFull`] without waiting when the bounded
    /// writer queue has no capacity.
    pub async fn try_send(&self, envelope: Envelope) -> Result<(), ConnectionError> {
        if let Some(reason) = self.inner.state.disconnect_reason() {
            return Err(ConnectionError::Disconnected(reason));
        }
        let (completion_sender, completion_receiver) = oneshot::channel();
        self.inner
            .outbound
            .try_send(Outbound {
                envelope,
                completion: completion_sender,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ConnectionError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => self.inner.state.disconnected_error(),
            })?;
        self.await_write(completion_receiver).await
    }

    async fn await_write(
        &self,
        completion: oneshot::Receiver<Result<(), ConnectionError>>,
    ) -> Result<(), ConnectionError> {
        tokio::select! {
            biased;
            result = completion => {
                result.unwrap_or_else(|_| Err(self.inner.state.disconnected_error()))
            }
            () = self.inner.state.cancellation.cancelled() => {
                Err(self.inner.state.disconnected_error())
            }
        }
    }

    /// Sends a correlated request and waits for its response.
    ///
    /// # Errors
    ///
    /// Returns a queue, codec, disconnect, or stable remote error.
    pub async fn request(
        &self,
        operation: impl Into<String>,
        payload: IpcValue,
    ) -> Result<IpcValue, ConnectionError> {
        self.request_with_cancellation(operation, payload, CancellationToken::new())
            .await
    }

    /// Sends a request whose local waiter can be cancelled independently.
    /// Late responses for a cancelled waiter are discarded.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Cancelled`] when `cancellation` wins.
    pub async fn request_with_cancellation(
        &self,
        operation: impl Into<String>,
        payload: IpcValue,
        cancellation: CancellationToken,
    ) -> Result<IpcValue, ConnectionError> {
        let operation = operation.into();
        let id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (waiter_sender, waiter_receiver) = oneshot::channel();
        self.inner.state.insert_pending(id, waiter_sender)?;
        let mut pending_guard = PendingGuard::new(self.inner.state.clone(), id);
        let envelope = Envelope::Request {
            id,
            op: operation,
            payload,
        };
        tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(ConnectionError::Cancelled),
            result = self.send(envelope) => result?,
        }
        let response = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(ConnectionError::Cancelled),
            response = waiter_receiver => response.map_err(|_| ConnectionError::WaiterClosed)??,
        };
        pending_guard.disarm();
        match response {
            Envelope::Response { payload, .. } => Ok(payload),
            Envelope::Error { payload, .. } => Err(ConnectionError::Remote {
                code: payload.code,
                message: payload.message,
            }),
            Envelope::Request { .. } | Envelope::Event { .. } | Envelope::Log { .. } => {
                Err(ConnectionError::WaiterClosed)
            }
        }
    }

    /// Sends a successful response.
    ///
    /// # Errors
    ///
    /// Returns a codec or disconnect error.
    pub async fn respond(
        &self,
        id: u64,
        operation: Option<String>,
        payload: IpcValue,
    ) -> Result<(), ConnectionError> {
        self.send(Envelope::Response {
            id,
            op: operation,
            payload,
        })
        .await
    }

    /// Sends a stable error response.
    ///
    /// # Errors
    ///
    /// Returns a codec or disconnect error.
    pub async fn respond_error(
        &self,
        id: u64,
        operation: impl Into<String>,
        payload: IpcErrorPayload,
    ) -> Result<(), ConnectionError> {
        self.send(Envelope::Error {
            id,
            op: operation.into(),
            payload,
        })
        .await
    }

    /// Sends an uncorrelated event.
    ///
    /// # Errors
    ///
    /// Returns a codec or disconnect error.
    pub async fn send_event(
        &self,
        operation: impl Into<String>,
        payload: IpcValue,
    ) -> Result<(), ConnectionError> {
        self.send(Envelope::Event {
            op: operation.into(),
            payload,
        })
        .await
    }

    /// Sends intercepted structured output.
    ///
    /// # Errors
    ///
    /// Returns a codec or disconnect error.
    pub async fn send_log(&self, payload: LogPayload) -> Result<(), ConnectionError> {
        self.send(Envelope::Log { payload }).await
    }

    /// Performs the exactly-once local disconnect transition.
    pub fn disconnect(&self, reason: DisconnectReason) -> bool {
        self.inner.state.disconnect(reason)
    }

    /// Releases worker-owned controller/resources immediately, sharing the same
    /// once guard used by transport disconnect.
    pub fn force_release_resources(&self) {
        self.inner.state.force_release_once();
    }

    /// Returns the first disconnect reason, if any.
    #[must_use]
    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.inner.state.disconnect_reason()
    }

    /// Waits for the first exactly-once disconnect transition.
    pub async fn wait_disconnected(&self) -> DisconnectReason {
        self.inner.state.cancellation.cancelled().await;
        self.inner
            .state
            .disconnect_reason()
            .unwrap_or(DisconnectReason::LocalShutdown)
    }

    /// Number of successful disconnect transitions; always zero or one.
    #[must_use]
    pub fn disconnect_transition_count(&self) -> usize {
        self.inner.state.disconnect_count.load(Ordering::Acquire)
    }

    /// Number of response frames discarded after their waiter was gone.
    #[must_use]
    pub fn discarded_late_response_count(&self) -> usize {
        self.inner.state.late_response_count.load(Ordering::Acquire)
    }

    /// Disconnects and joins both I/O tasks.
    pub async fn close(&self) {
        self.disconnect(DisconnectReason::LocalShutdown);
        let tasks = self
            .inner
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(tasks) = tasks {
            let _reader_result = tasks.reader.await;
            let _writer_result = tasks.writer.await;
        }
    }
}

async fn reader_loop<R>(mut reader: R, inbound: mpsc::Sender<Envelope>, state: Arc<SharedState>)
where
    R: AsyncRead + Unpin,
{
    let mut guard = DisconnectGuard::new(
        state.clone(),
        DisconnectReason::ReaderTerminated("reader task ended".to_owned()),
    );
    loop {
        let frame = tokio::select! {
            biased;
            () = state.cancellation.cancelled() => return,
            frame = read_frame(&mut reader) => frame,
        };
        match frame {
            Ok(Some(envelope @ (Envelope::Response { .. } | Envelope::Error { .. }))) => {
                state.complete_pending(envelope);
            }
            Ok(Some(envelope)) => {
                let sent = tokio::select! {
                    biased;
                    () = state.cancellation.cancelled() => return,
                    sent = inbound.send(envelope) => sent,
                };
                if sent.is_err() {
                    guard.set_reason(DisconnectReason::ReaderTerminated(
                        "inbound consumer closed".to_owned(),
                    ));
                    return;
                }
            }
            Ok(None) => {
                guard.set_reason(DisconnectReason::ReaderTerminated("EOF".to_owned()));
                return;
            }
            Err(error) => {
                guard.set_reason(match error {
                    CodecError::Io(detail) => DisconnectReason::ReaderTerminated(detail),
                    protocol_error => {
                        DisconnectReason::ProtocolViolation(protocol_error.to_string())
                    }
                });
                return;
            }
        }
    }
}

async fn writer_loop<W>(
    mut writer: W,
    mut outbound: mpsc::Receiver<Outbound>,
    state: Arc<SharedState>,
) where
    W: AsyncWrite + Unpin,
{
    let mut guard = DisconnectGuard::new(
        state.clone(),
        DisconnectReason::WriterTerminated("writer task ended".to_owned()),
    );
    loop {
        let item = tokio::select! {
            biased;
            () = state.cancellation.cancelled() => return,
            item = outbound.recv() => item,
        };
        let Some(item) = item else {
            guard.set_reason(DisconnectReason::WriterTerminated(
                "writer queue closed".to_owned(),
            ));
            return;
        };
        let write_result = tokio::select! {
            biased;
            () = state.cancellation.cancelled() => return,
            result = write_frame(&mut writer, &item.envelope) => result,
        };
        match write_result {
            Ok(()) => {
                let _send_result = item.completion.send(Ok(()));
            }
            Err(error @ CodecError::Io(_)) => {
                guard.set_reason(DisconnectReason::WriterTerminated(error.to_string()));
                let _send_result = item.completion.send(Err(ConnectionError::Codec(error)));
                return;
            }
            Err(error) => {
                let _send_result = item.completion.send(Err(ConnectionError::Codec(error)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf, duplex, split};
    use tokio_util::sync::CancellationToken;

    use super::{ConnectionConfig, ConnectionError, IpcConnection, ResourceSafety};
    use crate::ipc::{Envelope, IpcValue};

    #[derive(Debug, Default)]
    struct SafetyProbe {
        released: AtomicUsize,
        button_pressed: AtomicBool,
        stick_deflected: AtomicBool,
        touch_active: AtomicBool,
    }

    impl SafetyProbe {
        fn active() -> Self {
            Self {
                released: AtomicUsize::new(0),
                button_pressed: AtomicBool::new(true),
                stick_deflected: AtomicBool::new(true),
                touch_active: AtomicBool::new(true),
            }
        }
    }

    impl ResourceSafety for SafetyProbe {
        fn force_release(&self) {
            self.button_pressed.store(false, Ordering::Release);
            self.stick_deflected.store(false, Ordering::Release);
            self.touch_active.store(false, Ordering::Release);
            self.released.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[derive(Debug)]
    struct PendingReader;

    impl AsyncRead for PendingReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    #[derive(Debug)]
    struct PanicReader;

    impl AsyncRead for PanicReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            panic!("injected reader panic");
        }
    }

    #[derive(Debug)]
    struct PanicWriter;

    impl AsyncWrite for PanicWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            panic!("injected writer panic");
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[derive(Debug)]
    struct BlockedWriter {
        polls: Arc<AtomicUsize>,
    }

    impl AsyncWrite for BlockedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.polls.fetch_add(1, Ordering::AcqRel);
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn connection_pair() -> (IpcConnection, IpcConnection) {
        let (left, right) = duplex(64 * 1024);
        let (left_reader, left_writer) = split(left);
        let (right_reader, right_writer) = split(right);
        (
            IpcConnection::spawn_without_resources(
                left_reader,
                left_writer,
                ConnectionConfig::default(),
            )
            .unwrap(),
            IpcConnection::spawn_without_resources(
                right_reader,
                right_writer,
                ConnectionConfig::default(),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn request_response_uses_pending_waiter_table() {
        let (client, server) = connection_pair();
        let server_task = tokio::spawn(async move {
            let Envelope::Request { id, op, payload } = server.recv().await.unwrap() else {
                panic!("request expected");
            };
            assert_eq!(op, "worker.echo");
            server.respond(id, None, payload).await.unwrap();
            server.close().await;
        });
        let response = client
            .request("worker.echo", IpcValue::String("hello".to_owned()))
            .await
            .unwrap();
        assert_eq!(response, IpcValue::String("hello".to_owned()));
        server_task.await.unwrap();
        client.close().await;
    }

    #[tokio::test]
    async fn cancelled_waiter_discards_late_response() {
        let (client, server) = connection_pair();
        let cancellation = CancellationToken::new();
        let request_client = client.clone();
        let request_cancellation = cancellation.clone();
        let request = tokio::spawn(async move {
            request_client
                .request_with_cancellation("slow", IpcValue::Nil, request_cancellation)
                .await
        });
        let Envelope::Request { id, .. } = server.recv().await.unwrap() else {
            panic!("request expected");
        };
        cancellation.cancel();
        assert_eq!(request.await.unwrap(), Err(ConnectionError::Cancelled));
        server.respond(id, None, IpcValue::Nil).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(client.discarded_late_response_count(), 1);
        server.close().await;
        client.close().await;
    }

    #[tokio::test]
    async fn concurrent_reader_writer_exit_disconnects_and_releases_once() {
        let (local_stream, peer_stream) = duplex(4096);
        let (reader, writer) = split(local_stream);
        let safety = Arc::new(SafetyProbe::active());
        let connection =
            IpcConnection::spawn(reader, writer, ConnectionConfig::default(), safety.clone())
                .unwrap();
        let mut requests = Vec::new();
        for _ in 0..8 {
            let connection = connection.clone();
            requests.push(tokio::spawn(async move {
                connection.request("pending", IpcValue::Nil).await
            }));
        }
        tokio::task::yield_now().await;
        drop(peer_stream);
        for request in requests {
            assert!(matches!(
                request.await.unwrap(),
                Err(ConnectionError::Disconnected(_))
            ));
        }
        connection.close().await;
        assert_eq!(connection.disconnect_transition_count(), 1);
        assert_eq!(safety.released.load(Ordering::Acquire), 1);
        assert!(!safety.button_pressed.load(Ordering::Acquire));
        assert!(!safety.stick_deflected.load(Ordering::Acquire));
        assert!(!safety.touch_active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn malformed_partial_frame_never_reaches_consumer() {
        let (local_stream, mut peer_stream) = duplex(64);
        let (reader, writer) = split(local_stream);
        let connection =
            IpcConnection::spawn_without_resources(reader, writer, ConnectionConfig::default())
                .unwrap();
        peer_stream.write_all(&10_u32.to_be_bytes()).await.unwrap();
        peer_stream.write_all(&[0x81, 0xa4]).await.unwrap();
        peer_stream.shutdown().await.unwrap();
        assert!(matches!(
            connection.recv().await,
            Err(ConnectionError::Disconnected(_))
        ));
        assert_eq!(connection.disconnect_transition_count(), 1);
        connection.close().await;
    }

    #[tokio::test]
    async fn reader_panic_runs_disconnect_guard() {
        let safety = Arc::new(SafetyProbe::active());
        let connection = IpcConnection::spawn(
            PanicReader,
            tokio::io::sink(),
            ConnectionConfig::default(),
            safety.clone(),
        )
        .unwrap();
        assert!(matches!(
            connection.recv().await,
            Err(ConnectionError::Disconnected(_))
        ));
        connection.close().await;
        assert_eq!(connection.disconnect_transition_count(), 1);
        assert_eq!(safety.released.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn writer_panic_wakes_producer_and_runs_disconnect_guard() {
        let safety = Arc::new(SafetyProbe::active());
        let connection = IpcConnection::spawn(
            PendingReader,
            PanicWriter,
            ConnectionConfig::default(),
            safety.clone(),
        )
        .unwrap();
        assert!(matches!(
            connection.send_event("fault.writer", IpcValue::Nil).await,
            Err(ConnectionError::Disconnected(_))
        ));
        connection.close().await;
        assert_eq!(connection.disconnect_transition_count(), 1);
        assert_eq!(safety.released.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn queue_overflow_is_bounded_and_non_blocking() {
        let polls = Arc::new(AtomicUsize::new(0));
        let connection = IpcConnection::spawn_without_resources(
            PendingReader,
            BlockedWriter {
                polls: polls.clone(),
            },
            ConnectionConfig {
                writer_queue_capacity: 1,
                inbound_queue_capacity: 1,
            },
        )
        .unwrap();
        let first_connection = connection.clone();
        let first = tokio::spawn(async move {
            first_connection
                .send_event("queue.first", IpcValue::Nil)
                .await
        });
        while polls.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
        let second_connection = connection.clone();
        let second = tokio::spawn(async move {
            second_connection
                .try_send(Envelope::Event {
                    op: "queue.second".to_owned(),
                    payload: IpcValue::Nil,
                })
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(
            connection
                .try_send(Envelope::Event {
                    op: "queue.overflow".to_owned(),
                    payload: IpcValue::Nil,
                })
                .await,
            Err(ConnectionError::QueueFull)
        );
        connection.close().await;
        assert!(matches!(
            first.await.unwrap(),
            Err(ConnectionError::Disconnected(_))
        ));
        assert!(matches!(
            second.await.unwrap(),
            Err(ConnectionError::Disconnected(_))
        ));
    }
}
