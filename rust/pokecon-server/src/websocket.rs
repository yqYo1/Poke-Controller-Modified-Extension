//! Bounded WebSocket transport for state, ephemeral events, signaling, and
//! fallback controller input.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{self, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::api::{
    ApiError, ApiErrorCode, ClientMessage, ErrorEnvelope, IceCandidate, InputApplied,
    InputGeneration, LogData, MessageData, Nonce, RevisionedStateChange, SerialData, ServerMessage,
    SessionDescription,
};
use crate::backend::{ApiFailure, ApiResult};
use crate::realtime_connection::{
    RealtimeConnectionConfig, RealtimeConnectionIo, run_realtime_connection,
};
use crate::state::StateHub;

const DEFAULT_MESSAGE_BYTES: usize = 1024 * 1024;

/// Stable identifier for resources owned by one upgraded connection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionId(u64);

impl ConnectionId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Latest-only Motion JPEG source shared with one or more WebSocket writers.
/// A `watch` slot is intentional: publishing a newer frame replaces the sole
/// unsent frame for every slow client.
#[derive(Clone, Debug)]
pub struct MotionJpegFeed {
    sender: watch::Sender<Option<Bytes>>,
}

impl MotionJpegFeed {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _receiver) = watch::channel(None);
        Self { sender }
    }

    /// Replaces the previous unsent JPEG with one complete encoded frame.
    pub fn publish(&self, frame: impl Into<Bytes>) {
        let _previous = self.sender.send_replace(Some(frame.into()));
    }

    /// Stops binary fallback delivery without closing signaling `WebSockets`.
    pub fn suspend(&self) {
        let _previous = self.sender.send_replace(None);
    }

    #[must_use]
    pub fn subscribe(&self) -> MotionJpegStream {
        MotionJpegStream {
            receiver: self.sender.subscribe(),
            initial_pending: true,
        }
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for MotionJpegFeed {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-client view of a latest-only Motion JPEG source.
#[derive(Debug)]
pub struct MotionJpegStream {
    receiver: watch::Receiver<Option<Bytes>>,
    initial_pending: bool,
}

impl MotionJpegStream {
    async fn next(&mut self) -> MotionJpegEvent {
        if self.initial_pending {
            self.initial_pending = false;
            return MotionJpegEvent::Value(self.receiver.borrow_and_update().clone());
        }
        if self.receiver.changed().await.is_err() {
            return MotionJpegEvent::Closed;
        }
        MotionJpegEvent::Value(self.receiver.borrow_and_update().clone())
    }
}

#[derive(Debug)]
enum MotionJpegEvent {
    Value(Option<Bytes>),
    Closed,
}

/// Direct responses that a backend may send only to the originating client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSocketReply {
    WebRtcOffer(SessionDescription),
    WebRtcAnswer(SessionDescription),
    WebRtcIceCandidate(IceCandidate),
    InputGeneration(InputGeneration),
    InputSnapshotApplied(InputApplied),
    Log(LogData),
}

impl From<WebSocketReply> for ServerMessage {
    fn from(reply: WebSocketReply) -> Self {
        match reply {
            WebSocketReply::WebRtcOffer(data) => Self::WebRtcOffer(MessageData { data }),
            WebSocketReply::WebRtcAnswer(data) => Self::WebRtcAnswer(MessageData { data }),
            WebSocketReply::WebRtcIceCandidate(data) => {
                Self::WebRtcIceCandidate(MessageData { data })
            }
            WebSocketReply::InputGeneration(data) => Self::InputGeneration(MessageData { data }),
            WebSocketReply::InputSnapshotApplied(data) => {
                Self::InputSnapshotApplied(MessageData { data })
            }
            WebSocketReply::Log(data) => Self::Log(MessageData { data }),
        }
    }
}

/// Application-owned input and WebRTC boundary for each connection.
#[async_trait]
pub trait WebSocketBackend: Send + Sync + 'static {
    fn state_hub(&self) -> &StateHub;

    /// Installs or atomically replaces the input generation owned by this
    /// connection. Realtime route handoffs may call this more than once.
    async fn connected(
        &self,
        connection: ConnectionId,
        generation: &InputGeneration,
    ) -> ApiResult<()>;

    async fn message(
        &self,
        connection: ConnectionId,
        message: ClientMessage,
    ) -> ApiResult<Vec<WebSocketReply>>;

    /// Supplies a connection-specific fallback stream when camera media is
    /// available. `None` leaves the JSON-only WebSocket behavior unchanged.
    fn motion_jpeg(&self, _connection: ConnectionId) -> Option<MotionJpegStream> {
        None
    }

    /// Enables native WebRTC orchestration for this connection. Backends that
    /// return `None` retain the JSON/Motion JPEG-only behavior.
    fn realtime(&self, _connection: ConnectionId) -> Option<RealtimeConnectionConfig> {
        None
    }

    /// Releases all connection-owned input and signaling resources. This is
    /// called exactly once after a successful or partially successful connect.
    async fn disconnected(&self, connection: ConnectionId);
}

/// Bounded connection policy. Queue exhaustion for revisioned state causes a
/// disconnect so the client must recover from fresh REST snapshots.
#[derive(Clone, Debug)]
pub struct WebSocketConfig {
    pub heartbeat_interval: Duration,
    pub pong_timeout: Duration,
    pub backend_timeout: Duration,
    pub max_message_bytes: usize,
    pub state_queue_capacity: usize,
    pub ephemeral_queue_capacity: usize,
    pub heartbeat_queue_capacity: usize,
    pub realtime_queue_capacity: usize,
    pub broadcast_capacity: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(15),
            pong_timeout: Duration::from_secs(10),
            backend_timeout: Duration::from_secs(10),
            max_message_bytes: DEFAULT_MESSAGE_BYTES,
            state_queue_capacity: 64,
            ephemeral_queue_capacity: 128,
            heartbeat_queue_capacity: 8,
            realtime_queue_capacity: 128,
            broadcast_capacity: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WebSocketBuildError {
    #[error("WebSocket durations must be greater than zero")]
    ZeroDuration,
    #[error("WebSocket pong timeout must not exceed the heartbeat interval")]
    PongTimeoutExceedsInterval,
    #[error("WebSocket message and queue capacities must be greater than zero")]
    ZeroCapacity,
}

#[derive(Clone, Debug)]
enum BroadcastEvent {
    SerialData(SerialData),
    Log(LogData),
}

/// Publisher for non-revisioned server events. Delivery is best effort and a
/// slow subscriber cannot stall controller input or revisioned state.
#[derive(Clone, Debug)]
pub struct WebSocketBroker {
    sender: broadcast::Sender<Arc<BroadcastEvent>>,
}

impl WebSocketBroker {
    pub fn publish_serial(&self, data: SerialData) -> usize {
        self.publish(BroadcastEvent::SerialData(data))
    }

    pub fn publish_log(&self, data: LogData) -> usize {
        self.publish(BroadcastEvent::Log(data))
    }

    fn publish(&self, event: BroadcastEvent) -> usize {
        self.sender.send(Arc::new(event)).unwrap_or_default()
    }

    fn subscribe(&self) -> broadcast::Receiver<Arc<BroadcastEvent>> {
        self.sender.subscribe()
    }
}

#[derive(Clone)]
struct WebSocketState {
    backend: Arc<dyn WebSocketBackend>,
    broker: WebSocketBroker,
    config: WebSocketConfig,
    next_connection: Arc<AtomicU64>,
}

impl WebSocketState {
    fn allocate_connection(&self) -> Option<ConnectionId> {
        self.next_connection
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .ok()
            .and_then(|previous| previous.checked_add(1))
            .map(ConnectionId)
    }
}

/// Cloneable route/broadcast composition for the single `/ws` endpoint.
#[derive(Clone)]
pub struct WebSocketTransport {
    state: WebSocketState,
}

impl WebSocketTransport {
    /// Creates a transport with finite queues and timeouts.
    ///
    /// # Errors
    ///
    /// Rejects zero durations, message bounds, or queue capacities.
    pub fn new(
        backend: Arc<dyn WebSocketBackend>,
        config: WebSocketConfig,
    ) -> Result<Self, WebSocketBuildError> {
        if config.heartbeat_interval.is_zero()
            || config.pong_timeout.is_zero()
            || config.backend_timeout.is_zero()
        {
            return Err(WebSocketBuildError::ZeroDuration);
        }
        if config.pong_timeout > config.heartbeat_interval {
            return Err(WebSocketBuildError::PongTimeoutExceedsInterval);
        }
        if config.max_message_bytes == 0
            || config.state_queue_capacity == 0
            || config.ephemeral_queue_capacity == 0
            || config.heartbeat_queue_capacity == 0
            || config.realtime_queue_capacity == 0
            || config.broadcast_capacity == 0
        {
            return Err(WebSocketBuildError::ZeroCapacity);
        }
        let (sender, _receiver) = broadcast::channel(config.broadcast_capacity);
        Ok(Self {
            state: WebSocketState {
                backend,
                broker: WebSocketBroker { sender },
                config,
                next_connection: Arc::new(AtomicU64::new(0)),
            },
        })
    }

    #[must_use]
    pub fn broker(&self) -> WebSocketBroker {
        self.state.broker.clone()
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/ws", any(websocket_upgrade))
            .with_state(self.state.clone())
    }
}

async fn websocket_upgrade(
    State(state): State<WebSocketState>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return http_error(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidRequest,
            "request is not a valid WebSocket upgrade",
        );
    };
    let Some(connection) = state.allocate_connection() else {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
            "WebSocket connection identifiers are exhausted",
        );
    };
    let max_message_bytes = state.config.max_message_bytes;
    upgrade
        .max_message_size(max_message_bytes)
        .max_frame_size(max_message_bytes)
        .on_upgrade(move |socket| serve_connection(socket, state, connection))
        .into_response()
}

fn http_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ApiError {
                code,
                message: message.to_owned(),
                fields: None,
            },
        }),
    )
        .into_response()
}

#[derive(Debug)]
pub(crate) enum Outgoing {
    Json(ServerMessage),
    Close(CloseFrame),
}

async fn serve_connection(mut socket: WebSocket, state: WebSocketState, connection: ConnectionId) {
    let generation = InputGeneration {
        generation: format!("ws-{}", connection.get()),
    };
    if !initialize_backend(&state, connection, &generation).await {
        let _result = socket
            .send(Message::Close(Some(protocol_close(
                "connection initialization failed",
            ))))
            .await;
        state.backend.disconnected(connection).await;
        return;
    }

    let state_events = state.backend.state_hub().subscribe();
    let broadcast_events = state.broker.subscribe();
    let cancellation = CancellationToken::new();
    let (high_sender, high_receiver) = mpsc::channel(state.config.state_queue_capacity);
    let (low_sender, low_receiver) = mpsc::channel(state.config.ephemeral_queue_capacity);
    let (pong_sender, pong_receiver) = mpsc::channel(state.config.heartbeat_queue_capacity);
    if high_sender
        .try_send(Outgoing::Json(ServerMessage::InputGeneration(
            MessageData { data: generation },
        )))
        .is_err()
    {
        state.backend.disconnected(connection).await;
        return;
    }

    let realtime = setup_realtime(&state, connection);
    let (socket_sender, socket_receiver) = socket.split();
    let mut tasks = JoinSet::new();
    tasks.spawn(write_messages(
        socket_sender,
        high_receiver,
        low_receiver,
        realtime.motion_jpeg,
        cancellation.clone(),
    ));
    tasks.spawn(read_messages(
        socket_receiver,
        ReadMessageContext {
            backend: Arc::clone(&state.backend),
            connection,
            outgoing: high_sender.clone(),
            pong: pong_sender,
            backend_timeout: state.config.backend_timeout,
            realtime: realtime.messages,
            cancellation: cancellation.clone(),
        },
    ));
    tasks.spawn(forward_state_changes(
        state_events,
        high_sender.clone(),
        cancellation.clone(),
    ));
    tasks.spawn(forward_broadcasts(
        broadcast_events,
        low_sender.clone(),
        realtime.logs,
        cancellation.clone(),
    ));
    tasks.spawn(heartbeat(
        connection,
        high_sender.clone(),
        pong_receiver,
        state.config.heartbeat_interval,
        state.config.pong_timeout,
        cancellation.clone(),
    ));
    if let Some((config, messages, logs, motion_jpeg_enabled)) = realtime.task {
        tasks.spawn(run_realtime_connection(
            config,
            RealtimeConnectionIo {
                backend: Arc::clone(&state.backend),
                connection,
                outgoing_high: high_sender,
                outgoing_low: low_sender,
                messages,
                logs,
                motion_jpeg_enabled,
                backend_timeout: state.config.backend_timeout,
                cancellation: cancellation.clone(),
            },
        ));
    }

    let _first_finished = tasks.join_next().await;
    cancellation.cancel();
    while tasks.join_next().await.is_some() {}
    state.backend.disconnected(connection).await;
}

async fn initialize_backend(
    state: &WebSocketState,
    connection: ConnectionId,
    generation: &InputGeneration,
) -> bool {
    match time::timeout(
        state.config.backend_timeout,
        state.backend.connected(connection, generation),
    )
    .await
    {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!(
                connection_id = connection.get(),
                error_code = ?error.error().code,
                "WebSocket backend rejected a connection"
            );
            false
        }
        Err(_elapsed) => {
            tracing::warn!(
                connection_id = connection.get(),
                "WebSocket backend connection initialization timed out"
            );
            false
        }
    }
}

type RealtimeTask = (
    RealtimeConnectionConfig,
    mpsc::Receiver<ClientMessage>,
    mpsc::Receiver<LogData>,
    watch::Sender<bool>,
);

struct RealtimeSetup {
    motion_jpeg: MotionJpegDelivery,
    messages: Option<mpsc::Sender<ClientMessage>>,
    logs: Option<mpsc::Sender<LogData>>,
    task: Option<RealtimeTask>,
}

fn setup_realtime(state: &WebSocketState, connection: ConnectionId) -> RealtimeSetup {
    let Some(config) = state.backend.realtime(connection) else {
        return RealtimeSetup {
            motion_jpeg: MotionJpegDelivery::from_stream(state.backend.motion_jpeg(connection)),
            messages: None,
            logs: None,
            task: None,
        };
    };
    let (command_sender, command_receiver) = mpsc::channel(state.config.realtime_queue_capacity);
    let (event_sender, event_receiver) = mpsc::channel(state.config.realtime_queue_capacity);
    let (motion_enabled, motion_receiver) = watch::channel(false);
    RealtimeSetup {
        motion_jpeg: MotionJpegDelivery::Switched {
            feed: config.motion_jpeg(),
            enabled: motion_receiver,
            stream: None,
        },
        messages: Some(command_sender),
        logs: Some(event_sender),
        task: Some((config, command_receiver, event_receiver, motion_enabled)),
    }
}

async fn write_messages<S>(
    mut socket: S,
    mut high: mpsc::Receiver<Outgoing>,
    mut low: mpsc::Receiver<Outgoing>,
    mut motion_jpeg: MotionJpegDelivery,
    cancellation: CancellationToken,
) where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    loop {
        let next = tokio::select! {
            biased;
            message = high.recv() => NextWrite::Queued(message),
            () = cancellation.cancelled() => NextWrite::Cancelled,
            event = next_motion_jpeg(&mut motion_jpeg) => NextWrite::MotionJpeg(event),
            message = low.recv() => NextWrite::Queued(message),
        };
        let (message, closes) = match next {
            NextWrite::Queued(Some(Outgoing::Json(message))) => {
                let Ok(encoded) = serde_json::to_string(&message) else {
                    return;
                };
                (Message::Text(encoded.into()), false)
            }
            NextWrite::Queued(Some(Outgoing::Close(frame))) => (Message::Close(Some(frame)), true),
            NextWrite::Queued(None) => return,
            NextWrite::Cancelled => {
                let _result = socket.send(Message::Close(None)).await;
                return;
            }
            NextWrite::MotionJpeg(MotionJpegEvent::Value(Some(frame))) => {
                (Message::Binary(frame), false)
            }
            NextWrite::MotionJpeg(MotionJpegEvent::Value(None)) => continue,
            NextWrite::MotionJpeg(MotionJpegEvent::Closed) => {
                motion_jpeg = MotionJpegDelivery::Disabled;
                continue;
            }
        };
        if socket.send(message).await.is_err() || closes {
            return;
        }
    }
}

enum NextWrite {
    Queued(Option<Outgoing>),
    MotionJpeg(MotionJpegEvent),
    Cancelled,
}

#[derive(Debug)]
enum MotionJpegDelivery {
    Disabled,
    Always(MotionJpegStream),
    Switched {
        feed: MotionJpegFeed,
        enabled: watch::Receiver<bool>,
        stream: Option<MotionJpegStream>,
    },
}

impl MotionJpegDelivery {
    fn from_stream(stream: Option<MotionJpegStream>) -> Self {
        stream.map_or(Self::Disabled, Self::Always)
    }

    async fn next(&mut self) -> MotionJpegEvent {
        loop {
            match self {
                Self::Disabled => return std::future::pending().await,
                Self::Always(stream) => return stream.next().await,
                Self::Switched {
                    feed,
                    enabled,
                    stream,
                } => {
                    if !*enabled.borrow_and_update() {
                        *stream = None;
                        if enabled.changed().await.is_err() {
                            return MotionJpegEvent::Closed;
                        }
                        continue;
                    }
                    let active = stream.get_or_insert_with(|| feed.subscribe());
                    tokio::select! {
                        biased;
                        changed = enabled.changed() => {
                            if changed.is_err() {
                                return MotionJpegEvent::Closed;
                            }
                        }
                        event = active.next() => return event,
                    }
                }
            }
        }
    }
}

async fn next_motion_jpeg(delivery: &mut MotionJpegDelivery) -> MotionJpegEvent {
    delivery.next().await
}

struct ReadMessageContext {
    backend: Arc<dyn WebSocketBackend>,
    connection: ConnectionId,
    outgoing: mpsc::Sender<Outgoing>,
    pong: mpsc::Sender<String>,
    backend_timeout: Duration,
    realtime: Option<mpsc::Sender<ClientMessage>>,
    cancellation: CancellationToken,
}

async fn read_messages<R>(mut socket: R, context: ReadMessageContext)
where
    R: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    loop {
        let next = tokio::select! {
            () = context.cancellation.cancelled() => return,
            next = socket.next() => next,
        };
        let Some(Ok(message)) = next else {
            return;
        };
        let Message::Text(text) = message else {
            if matches!(message, Message::Close(_)) {
                return;
            }
            if matches!(message, Message::Ping(_) | Message::Pong(_)) {
                continue;
            }
            close_for_protocol(&context.outgoing, "client messages must be JSON text");
            context.cancellation.cancel();
            return;
        };
        let Ok(message) = serde_json::from_str::<ClientMessage>(text.as_str()) else {
            close_for_protocol(&context.outgoing, "invalid client message");
            context.cancellation.cancel();
            return;
        };
        if let ClientMessage::Pong(MessageData { data }) = message {
            if context.pong.try_send(data.nonce).is_err() {
                context.cancellation.cancel();
                return;
            }
            continue;
        }
        if let Some(realtime) = &context.realtime {
            if realtime.try_send(message).is_err() {
                context.cancellation.cancel();
                return;
            }
            continue;
        }
        let replies = match time::timeout(
            context.backend_timeout,
            context.backend.message(context.connection, message),
        )
        .await
        {
            Ok(Ok(replies)) => replies,
            Ok(Err(error)) => {
                log_backend_failure(context.connection, &error);
                context.cancellation.cancel();
                return;
            }
            Err(_elapsed) => {
                tracing::warn!(
                    connection_id = context.connection.get(),
                    "WebSocket backend operation timed out"
                );
                context.cancellation.cancel();
                return;
            }
        };
        for reply in replies {
            if context
                .outgoing
                .try_send(Outgoing::Json(reply.into()))
                .is_err()
            {
                context.cancellation.cancel();
                return;
            }
        }
    }
}

pub(crate) fn log_backend_failure(connection: ConnectionId, error: &ApiFailure) {
    tracing::warn!(
        connection_id = connection.get(),
        error_code = ?error.error().code,
        "WebSocket backend operation failed"
    );
}

fn close_for_protocol(outgoing: &mpsc::Sender<Outgoing>, reason: &'static str) {
    let _result = outgoing.try_send(Outgoing::Close(protocol_close(reason)));
}

fn protocol_close(reason: &'static str) -> CloseFrame {
    CloseFrame {
        code: close_code::POLICY,
        reason: reason.into(),
    }
}

async fn forward_state_changes(
    mut events: broadcast::Receiver<Arc<RevisionedStateChange>>,
    outgoing: mpsc::Sender<Outgoing>,
    cancellation: CancellationToken,
) {
    loop {
        let event = tokio::select! {
            () = cancellation.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                let message = ServerMessage::UiStateChanged(Box::new((*event).clone()));
                if outgoing.try_send(Outgoing::Json(message)).is_err() {
                    cancellation.cancel();
                    return;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_missed)) => {
                cancellation.cancel();
                return;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn forward_broadcasts(
    mut events: broadcast::Receiver<Arc<BroadcastEvent>>,
    outgoing: mpsc::Sender<Outgoing>,
    realtime_logs: Option<mpsc::Sender<LogData>>,
    cancellation: CancellationToken,
) {
    loop {
        let event = tokio::select! {
            () = cancellation.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                let message = match event.as_ref() {
                    BroadcastEvent::SerialData(data) => {
                        Some(ServerMessage::SerialData(MessageData {
                            data: data.clone(),
                        }))
                    }
                    BroadcastEvent::Log(data) => {
                        if let Some(realtime_logs) = &realtime_logs {
                            let _dropped_if_slow = realtime_logs.try_send(data.clone());
                            None
                        } else {
                            Some(ServerMessage::Log(MessageData { data: data.clone() }))
                        }
                    }
                };
                if let Some(message) = message {
                    let _dropped_if_slow = outgoing.try_send(Outgoing::Json(message));
                }
            }
            Err(broadcast::error::RecvError::Lagged(_missed)) => {}
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn heartbeat(
    connection: ConnectionId,
    outgoing: mpsc::Sender<Outgoing>,
    mut pong: mpsc::Receiver<String>,
    interval_duration: Duration,
    pong_timeout: Duration,
    cancellation: CancellationToken,
) {
    let mut interval = time::interval(interval_duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    let mut sequence = 0_u64;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let Some(next) = sequence.checked_add(1) else {
            cancellation.cancel();
            return;
        };
        sequence = next;
        let nonce = format!("ws-{}-{sequence}", connection.get());
        if outgoing
            .try_send(Outgoing::Json(ServerMessage::Ping(MessageData {
                data: Nonce {
                    nonce: nonce.clone(),
                },
            })))
            .is_err()
        {
            cancellation.cancel();
            return;
        }
        let matched = time::timeout(pong_timeout, async {
            loop {
                tokio::select! {
                    () = cancellation.cancelled() => return false,
                    received = pong.recv() => match received {
                        Some(received) if received == nonce => return true,
                        Some(_stale) => {}
                        None => return false,
                    }
                }
            }
        })
        .await
        .unwrap_or(false);
        if !matched {
            cancellation.cancel();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::StatusCode;
    use futures_util::{SinkExt as _, StreamExt as _};
    use pokecon_camera::{
        BgrFrame, CaptureResolution, LatestFrameSource, ScreenshotRuntimeSettings,
    };
    use tokio::sync::{Notify, mpsc};
    use tokio::time::timeout;
    use tokio_tungstenite::MaybeTlsStream;
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as ClientFrame;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    use webrtc::data_channel::RTCDataChannel;
    use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
    use webrtc::peer_connection::RTCPeerConnection;
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
    use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

    use super::*;
    use crate::BoundServer;
    use crate::api::{
        ButtonState, CameraSelector, CommandState, DecimalString, Hat, InputSnapshot, LogLevel,
        LogTarget, MouseButtons, StateChangeCause, StatePatch, StateSnapshot, StickPosition,
        UiStateChange,
    };
    use crate::api::{SettingsReadValues, SettingsSnapshot, SettingsWriteValues};
    use crate::realtime::RealtimeTransportConfig;
    use crate::realtime_connection::RealtimeConnectionConfig;
    use crate::router::public_router;
    use crate::security::RequestSecurity;
    use crate::state::StateTransaction;
    use crate::static_files::StaticFiles;
    use crate::webrtc::{
        CONTROL_DATA_CHANNEL, LOG_DATA_CHANNEL, WebRtcMedia, WebRtcMediaConfig, WebRtcPeerConfig,
        create_peer_connection,
    };

    type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    struct RealtimeClient {
        peer: Arc<RTCPeerConnection>,
        connected: mpsc::Receiver<()>,
        candidates: mpsc::Receiver<RTCIceCandidateInit>,
        channels: mpsc::Receiver<(String, Arc<RTCDataChannel>)>,
        messages: mpsc::Receiver<(String, String)>,
        video: mpsc::Receiver<usize>,
    }

    async fn realtime_client() -> RealtimeClient {
        let peer = Arc::new(
            create_peer_connection(&WebRtcPeerConfig::default())
                .await
                .expect("client peer"),
        );
        let (connected, connected_receiver) = mpsc::channel(1);
        peer.on_peer_connection_state_change(Box::new(move |state| {
            let connected = connected.clone();
            Box::pin(async move {
                if state == RTCPeerConnectionState::Connected {
                    let _sent = connected.try_send(());
                }
            })
        }));
        let (candidates, candidate_receiver) = mpsc::channel(16);
        peer.on_ice_candidate(Box::new(move |candidate| {
            let candidates = candidates.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate
                    && let Ok(candidate) = candidate.to_json()
                {
                    let _sent = candidates.send(candidate).await;
                }
            })
        }));
        let (channels, channel_receiver) = mpsc::channel(2);
        let (messages, message_receiver) = mpsc::channel(16);
        peer.on_data_channel(Box::new(move |channel| {
            let channels = channels.clone();
            let label = channel.label().to_owned();
            let message_label = label.clone();
            let messages = messages.clone();
            channel.on_message(Box::new(move |message| {
                let messages = messages.clone();
                let label = message_label.clone();
                Box::pin(async move {
                    let text = String::from_utf8(message.data.to_vec())
                        .expect("server DataChannel messages are UTF-8");
                    let _sent = messages.send((label, text)).await;
                })
            }));
            Box::pin(async move {
                let _sent = channels.send((label, channel)).await;
            })
        }));
        let (video, video_receiver) = mpsc::channel(1);
        peer.on_track(Box::new(move |track, _, _| {
            let video = video.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    if let Ok((packet, _attributes)) = track.read_rtp().await {
                        let _sent = video.send(packet.payload.len()).await;
                    }
                });
            })
        }));
        RealtimeClient {
            peer,
            connected: connected_receiver,
            candidates: candidate_receiver,
            channels: channel_receiver,
            messages: message_receiver,
            video: video_receiver,
        }
    }

    struct TestBackend {
        hub: StateHub,
        generations: Mutex<BTreeMap<ConnectionId, String>>,
        received: Mutex<Vec<ClientMessage>>,
        motion_jpeg: Option<MotionJpegFeed>,
        realtime: Option<RealtimeConnectionConfig>,
        disconnected: AtomicUsize,
        disconnect_notify: Notify,
    }

    impl TestBackend {
        fn new() -> Self {
            Self::with_motion_jpeg(None)
        }

        fn with_motion_jpeg(motion_jpeg: Option<MotionJpegFeed>) -> Self {
            Self {
                hub: test_hub(),
                generations: Mutex::new(BTreeMap::new()),
                received: Mutex::new(Vec::new()),
                motion_jpeg,
                realtime: None,
                disconnected: AtomicUsize::new(0),
                disconnect_notify: Notify::new(),
            }
        }

        fn with_realtime(realtime: RealtimeConnectionConfig) -> Self {
            Self {
                hub: test_hub(),
                generations: Mutex::new(BTreeMap::new()),
                received: Mutex::new(Vec::new()),
                motion_jpeg: None,
                realtime: Some(realtime),
                disconnected: AtomicUsize::new(0),
                disconnect_notify: Notify::new(),
            }
        }

        async fn wait_for_disconnect(&self) {
            if self.disconnected.load(Ordering::Acquire) == 0 {
                timeout(Duration::from_secs(1), self.disconnect_notify.notified())
                    .await
                    .expect("connection is released");
            }
        }
    }

    #[async_trait]
    impl WebSocketBackend for TestBackend {
        fn state_hub(&self) -> &StateHub {
            &self.hub
        }

        async fn connected(
            &self,
            connection: ConnectionId,
            generation: &InputGeneration,
        ) -> ApiResult<()> {
            self.generations
                .lock()
                .expect("generations")
                .insert(connection, generation.generation.clone());
            Ok(())
        }

        async fn message(
            &self,
            _connection: ConnectionId,
            message: ClientMessage,
        ) -> ApiResult<Vec<WebSocketReply>> {
            self.received
                .lock()
                .expect("received")
                .push(message.clone());
            match message {
                ClientMessage::InputSnapshot(MessageData { data }) => {
                    Ok(vec![WebSocketReply::InputSnapshotApplied(InputApplied {
                        generation: data.generation,
                        sequence: data.sequence,
                    })])
                }
                ClientMessage::WebRtcOffer(MessageData { data }) => {
                    Ok(vec![WebSocketReply::WebRtcAnswer(SessionDescription {
                        sdp: format!("answer: {}", data.sdp),
                    })])
                }
                _other => Ok(Vec::new()),
            }
        }

        fn motion_jpeg(&self, _connection: ConnectionId) -> Option<MotionJpegStream> {
            self.motion_jpeg.as_ref().map(MotionJpegFeed::subscribe)
        }

        fn realtime(&self, _connection: ConnectionId) -> Option<RealtimeConnectionConfig> {
            self.realtime.clone()
        }

        async fn disconnected(&self, _connection: ConnectionId) {
            self.disconnected.fetch_add(1, Ordering::AcqRel);
            self.disconnect_notify.notify_one();
        }
    }

    fn test_hub() -> StateHub {
        let settings = SettingsSnapshot {
            revision: DecimalString::zero(),
            values: SettingsReadValues(BTreeMap::new()),
            pending_restart_values: SettingsWriteValues::default(),
            restart_required: Vec::new(),
            apply_failures: BTreeMap::new(),
        };
        let state = StateSnapshot {
            revision: DecimalString::zero(),
            serial_port: None,
            serial_baud_rate: 115_200,
            serial_connected: false,
            camera_opened: false,
            camera_fps: 0.0,
            camera_resolution: "1280x720".to_owned(),
            camera_device: CameraSelector::Index(0),
            is_running: false,
            command_state: CommandState::Stopped,
            current_command: None,
            command_candidates: Vec::new(),
            tags: Vec::new(),
            active_profile: "default".to_owned(),
            pending_profile: None,
            available_profiles: vec!["default".to_owned()],
            last_input: None,
            holding_buttons: Vec::new(),
            pid: 42,
            command_display_lists: BTreeMap::from([("-".to_owned(), Vec::new())]),
            command_display_cache_loading: false,
        };
        StateHub::new(settings, state, 16).expect("state hub")
    }

    fn test_config() -> WebSocketConfig {
        WebSocketConfig {
            heartbeat_interval: Duration::from_secs(5),
            pong_timeout: Duration::from_secs(1),
            backend_timeout: Duration::from_secs(1),
            max_message_bytes: 4096,
            state_queue_capacity: 8,
            ephemeral_queue_capacity: 8,
            heartbeat_queue_capacity: 4,
            realtime_queue_capacity: 8,
            broadcast_capacity: 8,
        }
    }

    async fn start_server(
        transport: &WebSocketTransport,
    ) -> (
        SocketAddr,
        CancellationToken,
        tokio::task::JoinHandle<std::io::Result<()>>,
    ) {
        let server = BoundServer::bind_with_router(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            transport.router(),
        )
        .await
        .expect("server binds");
        let address = server.local_addr();
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(server.serve(cancellation.clone()));
        (address, cancellation, task)
    }

    async fn connect(address: SocketAddr) -> ClientSocket {
        connect_async(format!("ws://{address}/ws"))
            .await
            .expect("WebSocket connects")
            .0
    }

    async fn connect_with_origin(address: SocketAddr) -> ClientSocket {
        let mut request = format!("ws://{address}/ws")
            .into_client_request()
            .expect("upgrade request");
        request.headers_mut().insert(
            "origin",
            HeaderValue::from_str(&format!("http://{address}")).expect("origin"),
        );
        connect_async(request)
            .await
            .expect("secured WebSocket connects")
            .0
    }

    async fn receive_server(socket: &mut ClientSocket) -> ServerMessage {
        receive_server_within(socket, Duration::from_secs(1)).await
    }

    async fn receive_server_within(socket: &mut ClientSocket, duration: Duration) -> ServerMessage {
        let frame = timeout(duration, socket.next())
            .await
            .expect("server message deadline")
            .expect("connection remains open")
            .expect("valid frame");
        let ClientFrame::Text(text) = frame else {
            panic!("expected JSON text, got {frame:?}");
        };
        serde_json::from_str(text.as_str()).expect("typed server message")
    }

    async fn send_client(socket: &mut ClientSocket, message: &ClientMessage) {
        socket
            .send(ClientFrame::Text(
                serde_json::to_string(message).expect("encode").into(),
            ))
            .await
            .expect("send client message");
    }

    async fn signal_realtime(socket: &mut ClientSocket, client: &mut RealtimeClient) {
        let ServerMessage::WebRtcOffer(MessageData { data: offer }) =
            receive_server_within(socket, Duration::from_secs(10)).await
        else {
            panic!("first realtime message must be a WebRTC offer");
        };
        client
            .peer
            .set_remote_description(
                RTCSessionDescription::offer(offer.sdp).expect("valid server offer"),
            )
            .await
            .expect("client accepts offer");
        let answer = client
            .peer
            .create_answer(None)
            .await
            .expect("client answer");
        client
            .peer
            .set_local_description(answer)
            .await
            .expect("client installs answer");
        let answer = client
            .peer
            .local_description()
            .await
            .expect("client local description");
        send_client(
            socket,
            &ClientMessage::WebRtcAnswer(MessageData {
                data: SessionDescription { sdp: answer.sdp },
            }),
        )
        .await;

        timeout(Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    connected = client.connected.recv() => {
                        connected.expect("connection state channel");
                        break;
                    }
                    candidate = client.candidates.recv() => {
                        let candidate = candidate.expect("client ICE candidate channel");
                        send_client(
                            socket,
                            &ClientMessage::WebRtcIceCandidate(MessageData {
                                data: IceCandidate {
                                    candidate: candidate.candidate,
                                    sdp_mid: candidate.sdp_mid,
                                    sdp_mline_index: candidate.sdp_mline_index,
                                    username_fragment: candidate.username_fragment,
                                },
                            }),
                        )
                        .await;
                    }
                    message = receive_server_within(socket, Duration::from_secs(10)) => {
                        let ServerMessage::WebRtcIceCandidate(MessageData { data }) = message else {
                            panic!("unexpected signaling message: {message:?}");
                        };
                        client
                            .peer
                            .add_ice_candidate(RTCIceCandidateInit {
                                candidate: data.candidate,
                                sdp_mid: data.sdp_mid,
                                sdp_mline_index: data.sdp_mline_index,
                                username_fragment: data.username_fragment,
                            })
                            .await
                            .expect("client accepts server ICE candidate");
                    }
                }
            }
        })
        .await
        .expect("WebRTC signaling deadline");
    }

    async fn realtime_channels(
        receiver: &mut mpsc::Receiver<(String, Arc<RTCDataChannel>)>,
    ) -> BTreeMap<String, Arc<RTCDataChannel>> {
        let mut channels = BTreeMap::new();
        timeout(Duration::from_secs(5), async {
            while channels.len() < 2 {
                let (label, channel) = receiver.recv().await.expect("remote DataChannel");
                channels.insert(label, channel);
            }
        })
        .await
        .expect("DataChannel deadline");
        channels
    }

    async fn receive_data_channel_message(
        receiver: &mut mpsc::Receiver<(String, String)>,
    ) -> (String, ServerMessage) {
        let (label, message) = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("DataChannel message deadline")
            .expect("DataChannel message");
        (
            label,
            serde_json::from_str(&message).expect("typed DataChannel server message"),
        )
    }

    async fn wait_for_fallback_generation(socket: &mut ClientSocket) -> String {
        timeout(Duration::from_secs(5), async {
            loop {
                match receive_server_within(socket, Duration::from_secs(5)).await {
                    ServerMessage::InputGeneration(MessageData { data })
                        if data.generation.starts_with("ws-") =>
                    {
                        break data.generation;
                    }
                    ServerMessage::WebRtcIceCandidate(_) => {}
                    other => panic!("unexpected message before fallback: {other:?}"),
                }
            }
        })
        .await
        .expect("fallback generation deadline")
    }

    async fn receive_motion_jpeg(socket: &mut ClientSocket) -> Bytes {
        timeout(Duration::from_secs(5), async {
            loop {
                let frame = socket
                    .next()
                    .await
                    .expect("connection remains open")
                    .expect("valid fallback frame");
                match frame {
                    ClientFrame::Binary(bytes) => break bytes,
                    ClientFrame::Text(text) => {
                        let message: ServerMessage =
                            serde_json::from_str(text.as_str()).expect("typed server message");
                        assert!(matches!(message, ServerMessage::WebRtcIceCandidate(_)));
                    }
                    other => panic!("unexpected fallback frame: {other:?}"),
                }
            }
        })
        .await
        .expect("Motion JPEG deadline")
    }

    async fn realtime_test_config(
        source: &LatestFrameSource,
    ) -> (RealtimeConnectionConfig, MotionJpegFeed) {
        let media = WebRtcMedia::new(
            source.webrtc(),
            source.motion_jpeg(ScreenshotRuntimeSettings::default()),
            WebRtcMediaConfig::default(),
        )
        .await
        .expect("media pipeline");
        let motion_jpeg = media.motion_jpeg();
        let transport = RealtimeTransportConfig::new(
            Duration::from_secs(10),
            Duration::from_secs(30),
            true,
            Duration::from_secs(2),
        )
        .expect("realtime timers");
        (
            RealtimeConnectionConfig::new(media, WebRtcPeerConfig::default(), transport)
                .expect("realtime connection config"),
            motion_jpeg,
        )
    }

    fn neutral_snapshot(generation: String) -> ClientMessage {
        ClientMessage::InputSnapshot(MessageData {
            data: InputSnapshot {
                generation,
                sequence: DecimalString::zero(),
                keyboard_keys: Vec::new(),
                mouse_buttons: MouseButtons::default(),
                buttons: ButtonState::default(),
                hat: Hat::Neutral,
                left_stick: StickPosition { x: 128, y: 128 },
                right_stick: StickPosition { x: 128, y: 128 },
                touch: None,
            },
        })
    }

    async fn acknowledge_rtc_generation(
        client: &mut RealtimeClient,
        channels: &BTreeMap<String, Arc<RTCDataChannel>>,
        expected_generation: &str,
    ) {
        let (label, generation_message) = receive_data_channel_message(&mut client.messages).await;
        assert_eq!(label, CONTROL_DATA_CHANNEL);
        let ServerMessage::InputGeneration(MessageData { data: generation }) = generation_message
        else {
            panic!("RTC control channel must receive an input generation");
        };
        assert_eq!(generation.generation, expected_generation);
        let snapshot = neutral_snapshot(generation.generation.clone());
        channels[CONTROL_DATA_CHANNEL]
            .send_text(serde_json::to_string(&snapshot).expect("snapshot JSON"))
            .await
            .expect("RTC snapshot send");
        let (label, applied_message) = receive_data_channel_message(&mut client.messages).await;
        assert_eq!(label, CONTROL_DATA_CHANNEL);
        let ServerMessage::InputSnapshotApplied(MessageData { data: applied }) = applied_message
        else {
            panic!("RTC snapshot must be acknowledged on the control channel");
        };
        assert_eq!(applied.generation, expected_generation);
        assert_eq!(applied.sequence, DecimalString::zero());
    }

    async fn stop_server(
        cancellation: CancellationToken,
        task: tokio::task::JoinHandle<std::io::Result<()>>,
    ) {
        cancellation.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("server stops")
            .expect("task")
            .expect("serve");
    }

    async fn start_secure_server(
        transport: &WebSocketTransport,
    ) -> (
        SocketAddr,
        CancellationToken,
        tokio::task::JoinHandle<std::io::Result<()>>,
        tempfile::TempDir,
    ) {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("secure server binds");
        let address = listener.local_addr().expect("bound address");
        let root = tempfile::tempdir().expect("static root");
        fs::write(root.path().join("index.html"), "app").expect("static index");
        let app = public_router(
            transport.router(),
            StaticFiles::new(root.path()).expect("static files"),
            RequestSecurity::new(address, false),
        );
        let cancellation = CancellationToken::new();
        let shutdown = cancellation.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        (address, cancellation, task, root)
    }

    async fn initial_generation(socket: &mut ClientSocket) -> String {
        let ServerMessage::InputGeneration(MessageData { data }) = receive_server(socket).await
        else {
            panic!("first message must assign an input generation");
        };
        data.generation
    }

    #[test]
    fn zero_transport_limits_are_rejected() {
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.state_queue_capacity = 0;
        assert!(matches!(
            WebSocketTransport::new(backend, config),
            Err(WebSocketBuildError::ZeroCapacity)
        ));
    }

    #[test]
    fn pong_timeout_cannot_exceed_heartbeat_interval() {
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.heartbeat_interval = Duration::from_secs(1);
        config.pong_timeout = Duration::from_secs(2);
        assert!(matches!(
            WebSocketTransport::new(backend, config),
            Err(WebSocketBuildError::PongTimeoutExceedsInterval)
        ));
    }

    #[tokio::test]
    async fn motion_jpeg_stream_coalesces_to_the_latest_unsent_frame() {
        let feed = MotionJpegFeed::new();
        let mut stream = feed.subscribe();
        assert!(matches!(stream.next().await, MotionJpegEvent::Value(None)));

        feed.publish(vec![1_u8]);
        feed.publish(vec![2_u8]);
        feed.publish(vec![3_u8]);
        let MotionJpegEvent::Value(Some(frame)) = stream.next().await else {
            panic!("latest frame");
        };
        assert_eq!(frame.as_ref(), [3]);
    }

    #[tokio::test]
    async fn motion_jpeg_uses_one_binary_websocket_message_per_frame() {
        let feed = MotionJpegFeed::new();
        let backend = Arc::new(TestBackend::with_motion_jpeg(Some(feed.clone())));
        let transport = WebSocketTransport::new(backend.clone(), test_config()).expect("transport");
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;

        let jpeg = [0xff, 0xd8, 0x01, 0x02, 0xff, 0xd9];
        feed.publish(jpeg.to_vec());
        let frame = timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("binary frame deadline")
            .expect("connection remains open")
            .expect("valid frame");
        let ClientFrame::Binary(bytes) = frame else {
            panic!("expected Motion JPEG binary frame, got {frame:?}");
        };
        assert_eq!(bytes.as_ref(), jpeg);

        feed.suspend();
        socket.close(None).await.expect("close");
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn realtime_connection_promotes_atomically_and_falls_back_without_closing_signaling() {
        let source = LatestFrameSource::new();
        let (realtime, motion_jpeg) = realtime_test_config(&source).await;
        let backend = Arc::new(TestBackend::with_realtime(realtime));
        let mut config = test_config();
        config.heartbeat_interval = Duration::from_mins(1);
        config.pong_timeout = Duration::from_secs(10);
        let transport = WebSocketTransport::new(backend.clone(), config).expect("transport");
        let broker = transport.broker();
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        assert_eq!(initial_generation(&mut socket).await, "ws-1");

        let mut client = realtime_client().await;
        signal_realtime(&mut socket, &mut client).await;
        let channels = realtime_channels(&mut client.channels).await;
        assert_eq!(
            channels.keys().map(String::as_str).collect::<Vec<_>>(),
            [CONTROL_DATA_CHANNEL, LOG_DATA_CHANNEL]
        );
        acknowledge_rtc_generation(&mut client, &channels, "rtc-1-1").await;

        assert_eq!(
            broker.publish_log(LogData {
                level: LogLevel::Info,
                message: "realtime-log".to_owned(),
                target: LogTarget::Log,
            }),
            1
        );
        let (label, log_message) = receive_data_channel_message(&mut client.messages).await;
        assert_eq!(label, LOG_DATA_CHANNEL);
        assert!(matches!(log_message, ServerMessage::Log(_)));

        source.publish(
            100,
            BgrFrame::solid(CaptureResolution::R640x360, [20, 40, 60]),
        );
        let payload_bytes = timeout(Duration::from_secs(5), client.video.recv())
            .await
            .expect("RTP deadline")
            .expect("RTP payload");
        assert_ne!(payload_bytes, 0);
        assert_eq!(motion_jpeg.subscriber_count(), 0);

        client.peer.close().await.expect("client peer close");
        let fallback_generation = wait_for_fallback_generation(&mut socket).await;
        assert_eq!(fallback_generation, "ws-1-1");
        timeout(Duration::from_secs(1), async {
            while motion_jpeg.subscriber_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Motion JPEG subscription");
        source.publish(
            101,
            BgrFrame::solid(CaptureResolution::R640x360, [60, 40, 20]),
        );
        let jpeg = receive_motion_jpeg(&mut socket).await;
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        assert!(jpeg.ends_with(&[0xff, 0xd9]));
        assert_eq!(
            backend
                .generations
                .lock()
                .expect("generations")
                .get(&ConnectionId(1))
                .map(String::as_str),
            Some(fallback_generation.as_str())
        );

        let mut recovered_client = realtime_client().await;
        signal_realtime(&mut socket, &mut recovered_client).await;
        let recovered_channels = realtime_channels(&mut recovered_client.channels).await;
        assert_ne!(motion_jpeg.subscriber_count(), 0);
        acknowledge_rtc_generation(&mut recovered_client, &recovered_channels, "rtc-1-2").await;
        timeout(Duration::from_secs(1), async {
            while motion_jpeg.subscriber_count() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Motion JPEG suspension after recovery");

        socket.close(None).await.expect("close");
        recovered_client
            .peer
            .close()
            .await
            .expect("recovered client close");
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn invalid_http_upgrade_uses_the_common_error_envelope() {
        use tower::ServiceExt as _;

        let transport = WebSocketTransport::new(Arc::new(TestBackend::new()), test_config())
            .expect("transport");
        let response = transport
            .router()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/ws")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let error: ErrorEnvelope = serde_json::from_slice(&bytes).expect("error envelope");
        assert_eq!(error.error.code, ApiErrorCode::InvalidRequest);
    }

    #[tokio::test]
    async fn public_router_enforces_origin_before_upgrading() {
        let backend = Arc::new(TestBackend::new());
        let transport = WebSocketTransport::new(backend.clone(), test_config()).expect("transport");
        let (address, cancellation, task, _root) = start_secure_server(&transport).await;

        let error = connect_async(format!("ws://{address}/ws"))
            .await
            .expect_err("missing Origin is forbidden");
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("expected HTTP rejection, got {error:?}");
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let mut socket = connect_with_origin(address).await;
        let generation = initial_generation(&mut socket).await;
        assert!(!generation.is_empty());
        socket.close(None).await.expect("close");
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn state_direct_replies_and_ephemeral_events_remain_typed_and_ordered() {
        let backend = Arc::new(TestBackend::new());
        let transport = WebSocketTransport::new(backend.clone(), test_config()).expect("transport");
        let broker = transport.broker();
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let generation = initial_generation(&mut socket).await;

        let mut transaction = StateTransaction::new(StateChangeCause::Other);
        transaction.state = StatePatch {
            last_input: Some(Some("websocket".to_owned())),
            ..StatePatch::default()
        };
        backend.hub.commit(transaction).await.expect("state commit");
        let ServerMessage::UiStateChanged(change) = receive_server(&mut socket).await else {
            panic!("state event");
        };
        assert_eq!(change.revision.as_str(), "1");
        assert_eq!(
            change.data.state.last_input,
            Some(Some("websocket".to_owned()))
        );

        send_client(
            &mut socket,
            &ClientMessage::InputSnapshot(MessageData {
                data: InputSnapshot {
                    generation: generation.clone(),
                    sequence: DecimalString::zero(),
                    keyboard_keys: Vec::new(),
                    mouse_buttons: MouseButtons::default(),
                    buttons: ButtonState::default(),
                    hat: Hat::Neutral,
                    left_stick: StickPosition { x: 128, y: 128 },
                    right_stick: StickPosition { x: 128, y: 128 },
                    touch: None,
                },
            }),
        )
        .await;
        let ServerMessage::InputSnapshotApplied(MessageData { data }) =
            receive_server(&mut socket).await
        else {
            panic!("snapshot acknowledgement");
        };
        assert_eq!(data.generation, generation);
        assert_eq!(data.sequence, DecimalString::zero());

        send_client(
            &mut socket,
            &ClientMessage::WebRtcOffer(MessageData {
                data: SessionDescription {
                    sdp: "offer".to_owned(),
                },
            }),
        )
        .await;
        let ServerMessage::WebRtcAnswer(MessageData { data }) = receive_server(&mut socket).await
        else {
            panic!("WebRTC answer");
        };
        assert_eq!(data.sdp, "answer: offer");

        assert_eq!(
            broker.publish_log(LogData {
                level: LogLevel::Info,
                message: "first".to_owned(),
                target: LogTarget::Panel1,
            }),
            1
        );
        assert_eq!(
            broker.publish_log(LogData {
                level: LogLevel::Warning,
                message: "second".to_owned(),
                target: LogTarget::Panel1,
            }),
            1
        );
        for expected in ["first", "second"] {
            let ServerMessage::Log(MessageData { data }) = receive_server(&mut socket).await else {
                panic!("log message");
            };
            assert_eq!(data.message, expected);
        }

        socket.close(None).await.expect("close");
        backend.wait_for_disconnect().await;
        assert_eq!(backend.disconnected.load(Ordering::Acquire), 1);
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn revision_broadcast_gap_disconnects_instead_of_guessing() {
        let (sender, receiver) = broadcast::channel(1);
        for revision in [1, 2] {
            sender
                .send(Arc::new(RevisionedStateChange {
                    revision: DecimalString::from_u64(revision),
                    data: UiStateChange {
                        cause: StateChangeCause::Other,
                        state: StatePatch::default(),
                        settings: None,
                    },
                }))
                .expect("subscribed state receiver");
        }
        let (outgoing, mut messages) = mpsc::channel(4);
        let cancellation = CancellationToken::new();

        forward_state_changes(receiver, outgoing, cancellation.clone()).await;

        assert!(cancellation.is_cancelled());
        assert!(messages.try_recv().is_err());
    }

    #[tokio::test]
    async fn malformed_client_message_closes_and_releases_connection() {
        let backend = Arc::new(TestBackend::new());
        let transport = WebSocketTransport::new(backend.clone(), test_config()).expect("transport");
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;
        socket
            .send(ClientFrame::Text("{".into()))
            .await
            .expect("malformed message sent");
        let frame = timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("close deadline")
            .expect("close frame")
            .expect("valid close");
        let ClientFrame::Close(Some(frame)) = frame else {
            panic!("expected policy close, got {frame:?}");
        };
        assert_eq!(u16::from(frame.code), close_code::POLICY);
        backend.wait_for_disconnect().await;
        assert_eq!(backend.disconnected.load(Ordering::Acquire), 1);
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn heartbeat_requires_the_exact_application_nonce() {
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.heartbeat_interval = Duration::from_millis(200);
        config.pong_timeout = Duration::from_millis(100);
        let transport = WebSocketTransport::new(backend.clone(), config).expect("transport");
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;

        let ServerMessage::Ping(MessageData { data: nonce }) = receive_server(&mut socket).await
        else {
            panic!("heartbeat ping");
        };
        send_client(
            &mut socket,
            &ClientMessage::Pong(MessageData {
                data: Nonce {
                    nonce: "stale".to_owned(),
                },
            }),
        )
        .await;
        send_client(
            &mut socket,
            &ClientMessage::Pong(MessageData { data: nonce }),
        )
        .await;
        assert!(matches!(
            receive_server(&mut socket).await,
            ServerMessage::Ping(_)
        ));

        // Do not acknowledge the second nonce; timeout must release input.
        let closed = timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("heartbeat close deadline");
        assert!(
            !matches!(closed, Some(Ok(frame)) if !matches!(frame, ClientFrame::Close(_))),
            "heartbeat timeout must close the connection"
        );
        backend.wait_for_disconnect().await;
        assert_eq!(backend.disconnected.load(Ordering::Acquire), 1);
        stop_server(cancellation, task).await;
    }
}
