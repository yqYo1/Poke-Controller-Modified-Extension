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
use axum::http::header::ALLOW;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on};
use axum::{Json, Router};
use futures_util::{Sink, SinkExt as _, Stream, StreamExt as _};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{self, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::server::api::{
    ApiError, ApiErrorCode, ClientMessage, ErrorEnvelope, IceCandidate, InputApplied,
    InputGeneration, LogData, MessageData, Nonce, RevisionedStateChange, ScriptUiSnapshot,
    SerialData, ServerMessage, SessionDescription,
};
use crate::server::backend::{ApiFailure, ApiResult};
use crate::server::realtime_connection::{
    RealtimeConnectionConfig, RealtimeConnectionIo, run_realtime_connection,
};
use crate::server::state::StateHub;

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

/// Validated heartbeat intervals for one WebSocket connection.
///
/// Both durations are strictly positive and `pong_timeout` never exceeds
/// `ping_interval` so that an overlapping wait cannot outlive the next ping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSocketRuntimeSettings {
    ping_interval: Duration,
    pong_timeout: Duration,
}

impl WebSocketRuntimeSettings {
    /// Creates a validated heartbeat setting snapshot.
    ///
    /// # Errors
    ///
    /// Rejects zero durations or a pong timeout that exceeds the ping interval.
    pub fn new(
        ping_interval: Duration,
        pong_timeout: Duration,
    ) -> Result<Self, WebSocketBuildError> {
        if ping_interval.is_zero() || pong_timeout.is_zero() {
            return Err(WebSocketBuildError::ZeroDuration);
        }
        if pong_timeout > ping_interval {
            return Err(WebSocketBuildError::PongTimeoutExceedsInterval);
        }
        Ok(Self {
            ping_interval,
            pong_timeout,
        })
    }

    #[must_use]
    pub const fn ping_interval(&self) -> Duration {
        self.ping_interval
    }

    #[must_use]
    pub const fn pong_timeout(&self) -> Duration {
        self.pong_timeout
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
    script_ui: watch::Sender<ScriptUiSnapshot>,
}

impl WebSocketBroker {
    pub fn publish_serial(&self, data: SerialData) -> usize {
        self.publish(BroadcastEvent::SerialData(data))
    }

    pub fn publish_log(&self, data: LogData) -> usize {
        self.publish(BroadcastEvent::Log(data))
    }

    /// Atomically replaces the generation-local script UI snapshot. Every
    /// connection receives the current value immediately and then each newer
    /// complete value, so reconnect never relies on ephemeral replay.
    pub fn publish_script_ui(&self, snapshot: ScriptUiSnapshot) {
        let _previous = self.script_ui.send_replace(snapshot);
    }

    fn publish(&self, event: BroadcastEvent) -> usize {
        self.sender.send(Arc::new(event)).unwrap_or_default()
    }

    fn subscribe(&self) -> broadcast::Receiver<Arc<BroadcastEvent>> {
        self.sender.subscribe()
    }

    fn subscribe_script_ui(&self) -> watch::Receiver<ScriptUiSnapshot> {
        self.script_ui.subscribe()
    }
}

#[derive(Clone)]
struct WebSocketState {
    backend: Arc<dyn WebSocketBackend>,
    broker: WebSocketBroker,
    config: WebSocketConfig,
    heartbeat_settings: Option<watch::Receiver<WebSocketRuntimeSettings>>,
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
        let (script_ui, _receiver) = watch::channel(ScriptUiSnapshot::default());
        Ok(Self {
            state: WebSocketState {
                backend,
                broker: WebSocketBroker { sender, script_ui },
                config,
                heartbeat_settings: None,
                next_connection: Arc::new(AtomicU64::new(0)),
            },
        })
    }

    /// Subscribes this transport to live heartbeat settings. New connections
    /// immediately see the latest snapshot and each active connection
    /// restarts its current wait from the instant a new snapshot is applied.
    #[must_use]
    pub fn with_heartbeat_settings(
        mut self,
        heartbeat_settings: watch::Receiver<WebSocketRuntimeSettings>,
    ) -> Self {
        self.state.heartbeat_settings = Some(heartbeat_settings);
        self
    }

    #[must_use]
    pub fn broker(&self) -> WebSocketBroker {
        self.state.broker.clone()
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route(
                "/ws",
                on(MethodFilter::GET, websocket_upgrade)
                    .on(MethodFilter::HEAD, websocket_method_not_allowed)
                    .fallback(websocket_method_not_allowed),
            )
            .with_state(self.state.clone())
    }
}

async fn websocket_method_not_allowed() -> Response {
    let mut response = http_error(
        StatusCode::METHOD_NOT_ALLOWED,
        ApiErrorCode::MethodNotAllowed,
        "HTTP method is not allowed for this resource",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static("GET"));
    response
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

#[allow(clippy::too_many_lines)]
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
    let script_ui = state.broker.subscribe_script_ui();
    let cancellation = CancellationToken::new();
    // Bounded priority queues: control outranks display. Control (input acks,
    // signaling, heartbeat) has a dedicated bounded queue so slow display
    // delivery (state, script UI, motion JPEG, logs) cannot stall it. State
    // and script UI are coalescing display queues, low is best-effort
    // ephemeral (serial/log) with drop-on-full, motion JPEG is latest-only.
    let (control_sender, control_receiver) = mpsc::channel(state.config.state_queue_capacity);
    let (state_sender, state_receiver) = mpsc::channel(state.config.state_queue_capacity);
    let (low_sender, low_receiver) = mpsc::channel(state.config.ephemeral_queue_capacity);
    let (pong_sender, pong_receiver) = mpsc::channel(state.config.heartbeat_queue_capacity);
    if control_sender
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
        control_receiver,
        state_receiver,
        low_receiver,
        realtime.motion_jpeg,
        cancellation.clone(),
    ));
    tasks.spawn(read_messages(
        socket_receiver,
        ReadMessageContext {
            backend: Arc::clone(&state.backend),
            connection,
            outgoing: control_sender.clone(),
            pong: pong_sender,
            backend_timeout: state.config.backend_timeout,
            realtime: realtime.messages,
            cancellation: cancellation.clone(),
        },
    ));
    tasks.spawn(forward_state_changes(
        state_events,
        state_sender.clone(),
        cancellation.clone(),
    ));
    tasks.spawn(forward_broadcasts(
        broadcast_events,
        low_sender.clone(),
        realtime.logs,
        cancellation.clone(),
    ));
    tasks.spawn(forward_script_ui(
        script_ui,
        state_sender.clone(),
        cancellation.clone(),
    ));
    if let Some(heartbeat_settings) = state.heartbeat_settings.clone() {
        tasks.spawn(heartbeat_with_settings(
            connection,
            control_sender.clone(),
            pong_receiver,
            heartbeat_settings,
            cancellation.clone(),
        ));
    } else {
        tasks.spawn(heartbeat(
            connection,
            control_sender.clone(),
            pong_receiver,
            state.config.heartbeat_interval,
            state.config.pong_timeout,
            cancellation.clone(),
        ));
    }
    if let Some((config, messages, logs, motion_jpeg_enabled)) = realtime.task {
        tasks.spawn(run_realtime_connection(
            config,
            RealtimeConnectionIo {
                backend: Arc::clone(&state.backend),
                connection,
                outgoing_high: control_sender,
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
    mut control: mpsc::Receiver<Outgoing>,
    mut state: mpsc::Receiver<Outgoing>,
    mut low: mpsc::Receiver<Outgoing>,
    mut motion_jpeg: MotionJpegDelivery,
    cancellation: CancellationToken,
) where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    loop {
        let next = tokio::select! {
            biased;
            message = control.recv() => NextWrite::Queued(message),
            () = cancellation.cancelled() => NextWrite::Cancelled,
            message = state.recv() => NextWrite::Queued(message),
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

/// Merges a newer `RevisionedStateChange` into an existing pending one,
/// keeping the latest revision and coalescing same-field patches so that
/// display delivery never grows without bound and the latest complete value
/// wins for each field.
fn coalesce_optional<T>(existing: &mut Option<T>, incoming: Option<T>) {
    if incoming.is_some() {
        *existing = incoming;
    }
}

fn coalesce_revisioned_state(
    existing: &mut RevisionedStateChange,
    incoming: RevisionedStateChange,
) {
    existing.revision = incoming.revision;
    existing.data.cause = incoming.data.cause;
    let existing_patch = &mut existing.data.state;
    let incoming_patch = incoming.data.state;
    coalesce_optional(&mut existing_patch.serial_port, incoming_patch.serial_port);
    coalesce_optional(
        &mut existing_patch.serial_baud_rate,
        incoming_patch.serial_baud_rate,
    );
    coalesce_optional(
        &mut existing_patch.serial_connected,
        incoming_patch.serial_connected,
    );
    coalesce_optional(
        &mut existing_patch.camera_opened,
        incoming_patch.camera_opened,
    );
    coalesce_optional(&mut existing_patch.camera_fps, incoming_patch.camera_fps);
    coalesce_optional(
        &mut existing_patch.camera_resolution,
        incoming_patch.camera_resolution,
    );
    coalesce_optional(
        &mut existing_patch.camera_device,
        incoming_patch.camera_device,
    );
    coalesce_optional(&mut existing_patch.is_running, incoming_patch.is_running);
    coalesce_optional(
        &mut existing_patch.command_state,
        incoming_patch.command_state,
    );
    coalesce_optional(
        &mut existing_patch.current_command,
        incoming_patch.current_command,
    );
    coalesce_optional(
        &mut existing_patch.command_candidates,
        incoming_patch.command_candidates,
    );
    coalesce_optional(&mut existing_patch.tags, incoming_patch.tags);
    coalesce_optional(
        &mut existing_patch.active_profile,
        incoming_patch.active_profile,
    );
    coalesce_optional(
        &mut existing_patch.pending_profile,
        incoming_patch.pending_profile,
    );
    coalesce_optional(
        &mut existing_patch.available_profiles,
        incoming_patch.available_profiles,
    );
    coalesce_optional(&mut existing_patch.last_input, incoming_patch.last_input);
    coalesce_optional(
        &mut existing_patch.holding_buttons,
        incoming_patch.holding_buttons,
    );
    coalesce_optional(&mut existing_patch.pid, incoming_patch.pid);
    coalesce_optional(
        &mut existing_patch.command_display_lists,
        incoming_patch.command_display_lists,
    );
    coalesce_optional(
        &mut existing_patch.command_display_cache_loading,
        incoming_patch.command_display_cache_loading,
    );
    match (&mut existing.data.settings, incoming.data.settings) {
        (None, Some(incoming)) => existing.data.settings = Some(incoming),
        (Some(existing_settings), Some(incoming_settings)) => {
            for (key, value) in incoming_settings.values.0 {
                existing_settings.values.0.insert(key, value);
            }
            for (key, value) in incoming_settings.pending_restart_values.0 {
                existing_settings
                    .pending_restart_values
                    .0
                    .insert(key, value);
            }
            // Latest change is authoritative for restart and failure sets
            existing_settings.restart_required = incoming_settings.restart_required;
            existing_settings.apply_failures = incoming_settings.apply_failures;
        }
        _ => {}
    }
}

async fn forward_state_changes(
    mut events: broadcast::Receiver<Arc<RevisionedStateChange>>,
    outgoing: mpsc::Sender<Outgoing>,
    cancellation: CancellationToken,
) {
    // Bounded coalescing: at most one pending display patch beyond the channel
    // capacity. Same-field updates merge into the pending value so a slow client
    // never accumulates an unbounded display backlog and the latest complete
    // value wins. This keeps the critical control path (high queue) from
    // stalling on display pressure.
    let mut pending: Option<RevisionedStateChange> = None;
    loop {
        if let Some(pending_change) = pending.take() {
            let message = ServerMessage::UiStateChanged(Box::new(pending_change));
            match outgoing.try_send(Outgoing::Json(message)) {
                Ok(()) => continue,
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    let Outgoing::Json(ServerMessage::UiStateChanged(boxed)) = returned else {
                        unreachable!("state forwarder only sends UiStateChanged")
                    };
                    pending = Some(*boxed);
                    // Coalesce further display updates while the bounded queue
                    // remains full, otherwise keep retrying with backoff so a
                    // slow consumer does not busy-loop.
                    let event = tokio::select! {
                        () = cancellation.cancelled() => return,
                        event = events.recv() => event,
                        () = tokio::time::sleep(Duration::from_millis(5)) => {
                            continue;
                        }
                    };
                    match event {
                        Ok(event) => {
                            let incoming = (*event).clone();
                            if let Some(existing) = pending.as_mut() {
                                coalesce_revisioned_state(existing, incoming);
                            } else {
                                pending = Some(incoming);
                            }
                            continue;
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            cancellation.cancel();
                            return;
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        }
        let event = tokio::select! {
            () = cancellation.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                let incoming = (*event).clone();
                let message = ServerMessage::UiStateChanged(Box::new(incoming));
                match outgoing.try_send(Outgoing::Json(message)) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(returned)) => {
                        let Outgoing::Json(ServerMessage::UiStateChanged(boxed)) = returned else {
                            unreachable!("state forwarder only sends UiStateChanged")
                        };
                        pending = Some(*boxed);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return,
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                cancellation.cancel();
                return;
            }
            Err(broadcast::error::RecvError::Closed) => {
                if let Some(pending_change) = pending.take() {
                    let _ = outgoing.try_send(Outgoing::Json(ServerMessage::UiStateChanged(
                        Box::new(pending_change),
                    )));
                }
                return;
            }
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

async fn forward_script_ui(
    mut snapshots: watch::Receiver<ScriptUiSnapshot>,
    outgoing: mpsc::Sender<Outgoing>,
    cancellation: CancellationToken,
) {
    // Bounded latest-only delivery: the watch already coalesces to the
    // latest complete snapshot, and the forwarder keeps at most one
    // pending value beyond the bounded channel so slow displays never
    // propagate backpressure to the control path.
    let mut initial = true;
    let mut pending: Option<ScriptUiSnapshot> = None;
    loop {
        if let Some(pending_snapshot) = pending.take() {
            let message = ServerMessage::ScriptUi(MessageData {
                data: pending_snapshot,
            });
            match outgoing.try_send(Outgoing::Json(message)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    let Outgoing::Json(ServerMessage::ScriptUi(MessageData { data })) = returned
                    else {
                        unreachable!("script_ui forwarder only sends ScriptUi")
                    };
                    pending = Some(data);
                    // Keep latest snapshot while bounded queue is full.
                    let changed = tokio::select! {
                        () = cancellation.cancelled() => return,
                        changed = snapshots.changed() => changed,
                        () = tokio::time::sleep(Duration::from_millis(5)) => {
                            continue;
                        }
                    };
                    if changed.is_err() {
                        return;
                    }
                    let latest = snapshots.borrow_and_update().clone();
                    if let Some(existing) = pending.as_mut() {
                        *existing = latest;
                    } else {
                        pending = Some(latest);
                    }
                    continue;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        }
        let was_initial = initial;
        if was_initial {
            initial = false;
        } else {
            let changed = tokio::select! {
                () = cancellation.cancelled() => return,
                changed = snapshots.changed() => changed,
            };
            if changed.is_err() {
                if let Some(pending_snapshot) = pending.take() {
                    let _ =
                        outgoing.try_send(Outgoing::Json(ServerMessage::ScriptUi(MessageData {
                            data: pending_snapshot,
                        })));
                }
                return;
            }
        }
        let snapshot = snapshots.borrow_and_update().clone();
        if was_initial && snapshot.generation.is_none() {
            continue;
        }
        if let Some(existing) = pending.as_mut() {
            *existing = snapshot;
            continue;
        }
        let message = ServerMessage::ScriptUi(MessageData { data: snapshot });
        match outgoing.try_send(Outgoing::Json(message)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(returned)) => {
                let Outgoing::Json(ServerMessage::ScriptUi(MessageData { data })) = returned else {
                    unreachable!("script_ui forwarder only sends ScriptUi")
                };
                pending = Some(data);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => return,
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

async fn heartbeat_with_settings(
    connection: ConnectionId,
    outgoing: mpsc::Sender<Outgoing>,
    mut pong: mpsc::Receiver<String>,
    heartbeat_settings: watch::Receiver<WebSocketRuntimeSettings>,
    cancellation: CancellationToken,
) {
    // Each active connection tracks the latest snapshot and restarts its wait
    // from the instant the settings channel reports a new value.
    let mut settings = heartbeat_settings;
    let mut current = settings.borrow_and_update().clone();
    let mut next_ping = tokio::time::Instant::now() + current.ping_interval();
    let mut sequence = 0_u64;
    loop {
        // Wait for the next ping deadline, a settings update, or cancellation.
        // Settings updates cancel the current heartbeat wait and restart it from now.
        let settings_changed = async {
            if settings.changed().await.is_ok() {
                Some(settings.borrow_and_update().clone())
            } else {
                // Sender closed – disable future rescheduling by pending forever.
                std::future::pending::<Option<WebSocketRuntimeSettings>>().await
            }
        };
        tokio::select! {
            () = cancellation.cancelled() => return,
            () = tokio::time::sleep_until(next_ping) => {},
            maybe = settings_changed => {
                if let Some(updated) = maybe {
                    current = updated;
                    next_ping = tokio::time::Instant::now() + current.ping_interval();
                }
                continue;
            }
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

        let mut pong_deadline = tokio::time::Instant::now() + current.pong_timeout();
        let matched = loop {
            let pong_settings_changed = async {
                if settings.changed().await.is_ok() {
                    Some(settings.borrow_and_update().clone())
                } else {
                    std::future::pending::<Option<WebSocketRuntimeSettings>>().await
                }
            };
            tokio::select! {
                () = cancellation.cancelled() => return,
                () = tokio::time::sleep_until(pong_deadline) => break false,
                maybe = pong_settings_changed => {
                    if let Some(updated) = maybe {
                        current = updated;
                        // Restart pong wait from the settings-application instant.
                        pong_deadline = tokio::time::Instant::now() + current.pong_timeout();
                    }
                }
                received = pong.recv() => match received {
                    Some(received) if received == nonce => break true,
                    Some(_stale) => {}
                    None => break false,
                }
            }
        };

        if !matched {
            cancellation.cancel();
            return;
        }
        // Successful pong – schedule next ping from now with the latest interval.
        current = settings.borrow().clone();
        next_ping = tokio::time::Instant::now() + current.ping_interval();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::camera::{
        BgrFrame, CaptureResolution, LatestFrameSource, ScreenshotRuntimeSettings,
    };
    use axum::http::header::{ALLOW, CONTENT_LENGTH, CONTENT_TYPE};
    use axum::http::{Method, StatusCode};
    use futures_util::{SinkExt as _, StreamExt as _};
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
    use crate::server::BoundServer;
    use crate::server::api::{
        ButtonState, CameraSelector, CommandState, DecimalString, Hat, InputSnapshot, LogLevel,
        LogTarget, MouseButtons, StateChangeCause, StatePatch, StateSnapshot, StickPosition,
        UiStateChange,
    };
    use crate::server::api::{SettingsReadValues, SettingsSnapshot, SettingsWriteValues};
    use crate::server::realtime::RealtimeTransportConfig;
    use crate::server::realtime_connection::RealtimeConnectionConfig;
    use crate::server::router::public_router;
    use crate::server::security::RequestSecurity;
    use crate::server::state::StateTransaction;
    use crate::server::static_files::StaticFiles;
    use crate::server::webrtc::{
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
                operation: crate::server::api::LogOperation::Append,
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
    async fn websocket_route_accepts_only_get_from_the_standard_method_matrix() {
        use tower::ServiceExt as _;

        let transport = WebSocketTransport::new(Arc::new(TestBackend::new()), test_config())
            .expect("transport");
        let app = transport.router();
        let mut advertised_count = 0;
        let mut rejected_count = 0;
        for method in [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
            Method::TRACE,
            Method::CONNECT,
        ] {
            let method_name = method.as_str().to_owned();
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri("/ws")
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let expected_status = if method_name == "GET" {
                advertised_count += 1;
                StatusCode::BAD_REQUEST
            } else {
                rejected_count += 1;
                StatusCode::METHOD_NOT_ALLOWED
            };
            assert_eq!(response.status(), expected_status, "{method_name} /ws");
            assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
            if method_name != "GET" {
                assert_eq!(
                    response.headers()[ALLOW],
                    "GET",
                    "Allow for {method_name} /ws"
                );
            }
            if method_name == "HEAD" {
                assert!(
                    response.headers()[CONTENT_LENGTH]
                        .to_str()
                        .expect("content length")
                        .parse::<usize>()
                        .expect("numeric content length")
                        > 0
                );
                assert!(
                    axum::body::to_bytes(response.into_body(), 1024)
                        .await
                        .expect("HEAD body")
                        .is_empty()
                );
            } else {
                let bytes = axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .expect("body");
                let error: ErrorEnvelope = serde_json::from_slice(&bytes).expect("error envelope");
                let expected_code = if method_name == "GET" {
                    ApiErrorCode::InvalidRequest
                } else {
                    ApiErrorCode::MethodNotAllowed
                };
                assert_eq!(error.error.code, expected_code, "{method_name} /ws");
            }
        }
        assert_eq!(advertised_count, 1);
        assert_eq!(rejected_count, 8);
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
                operation: crate::server::api::LogOperation::Append,
            }),
            1
        );
        assert_eq!(
            broker.publish_log(LogData {
                level: LogLevel::Warning,
                message: "second".to_owned(),
                target: LogTarget::Panel1,
                operation: crate::server::api::LogOperation::Append,
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
    async fn script_ui_snapshot_replays_to_reconnecting_clients() {
        let backend = Arc::new(TestBackend::new());
        let transport = WebSocketTransport::new(backend.clone(), test_config()).expect("transport");
        let expected = ScriptUiSnapshot {
            generation: Some("user-script-7".to_owned()),
            dialogs: vec![crate::server::api::ScriptDialog {
                id: DecimalString::from_u64(3),
                title: "Confirm".to_owned(),
                description: None,
                widgets: Vec::new(),
            }],
            ..ScriptUiSnapshot::default()
        };
        transport.broker().publish_script_ui(expected.clone());
        let (address, cancellation, task) = start_server(&transport).await;

        for _attempt in 0..2 {
            let mut socket = connect(address).await;
            let _generation = initial_generation(&mut socket).await;
            let ServerMessage::ScriptUi(MessageData { data }) = receive_server(&mut socket).await
            else {
                panic!("active script UI snapshot");
            };
            assert_eq!(data, expected);
            socket.close(None).await.expect("close");
            backend.wait_for_disconnect().await;
        }

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

    #[test]
    fn coalesce_keeps_latest_same_field_value() {
        let mut existing = RevisionedStateChange {
            revision: DecimalString::from_u64(1),
            data: UiStateChange {
                cause: StateChangeCause::Camera,
                state: StatePatch {
                    camera_fps: Some(30.0),
                    ..StatePatch::default()
                },
                settings: None,
            },
        };
        let incoming = RevisionedStateChange {
            revision: DecimalString::from_u64(2),
            data: UiStateChange {
                cause: StateChangeCause::Camera,
                state: StatePatch {
                    camera_fps: Some(60.0),
                    camera_opened: Some(true),
                    ..StatePatch::default()
                },
                settings: None,
            },
        };
        coalesce_revisioned_state(&mut existing, incoming);
        assert_eq!(existing.revision.as_str(), "2");
        assert_eq!(existing.data.state.camera_fps, Some(60.0));
        assert_eq!(existing.data.state.camera_opened, Some(true));
    }

    #[tokio::test]
    async fn state_coalesces_same_field_under_backpressure() {
        // Bounded display queue: capacity 1, coalescing keeps at most
        // capacity + one pending. Flooding same field must not grow without
        // bound and the latest complete value must win.
        let (sender, receiver) = broadcast::channel(16);
        let (outgoing, mut incoming) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let forwarder = tokio::spawn(forward_state_changes(
            receiver,
            outgoing,
            cancellation.clone(),
        ));

        // Fill the bounded channel so the forwarder must coalesce.
        // First, occupy the channel with a state change that will not be
        // consumed immediately (receiver not reading).
        let hub = test_hub();
        let rx = hub.subscribe();
        // Use direct broadcast sends to the forwarder's source
        for (revision, fps) in [
            (1_u64, 1.0),
            (2, 2.0),
            (3, 3.0),
            (4, 4.0),
            (5, 5.0),
            (6, 6.0),
            (7, 7.0),
            (8, 8.0),
            (9, 9.0),
            (10, 10.0),
        ] {
            let _ = sender.send(Arc::new(RevisionedStateChange {
                revision: DecimalString::from_u64(revision),
                data: UiStateChange {
                    cause: StateChangeCause::Camera,
                    state: StatePatch {
                        camera_fps: Some(fps),
                        ..StatePatch::default()
                    },
                    settings: None,
                },
            }));
        }
        // Allow forwarder to process and coalesce
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Now drain the bounded queue: first the initially queued,
        // then after a short window the coalesced pending.
        let mut collected = Vec::new();
        while let Ok(msg) = incoming.try_recv() {
            if let Outgoing::Json(ServerMessage::UiStateChanged(change)) = msg {
                collected.push(change);
            }
        }
        // Give the coalesced pending time to flush into the now-available slot
        tokio::time::sleep(Duration::from_millis(20)).await;
        while let Ok(msg) = incoming.try_recv() {
            if let Outgoing::Json(ServerMessage::UiStateChanged(change)) = msg {
                collected.push(change);
            }
        }
        // At most capacity + one pending (coalesced) should be queued
        assert!(
            collected.len() <= 2,
            "coalesced display queue must remain bounded, got {}",
            collected.len()
        );
        if let Some(last) = collected.last() {
            assert_eq!(
                last.data.state.camera_fps,
                Some(10.0),
                "latest same-field value must win after coalescing"
            );
            assert_eq!(last.revision.as_str(), "10");
        }
        cancellation.cancel();
        let _ = forwarder.await;
        let _ = hub;
        let _ = rx;
    }

    #[tokio::test]
    async fn log_delivery_is_bounded_drop_without_backpressure() {
        let (sender, receiver) = broadcast::channel(16);
        let (outgoing, mut low_incoming) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let forwarder = tokio::spawn(forward_broadcasts(
            receiver,
            outgoing.clone(),
            None,
            cancellation.clone(),
        ));
        // Flood logs with capacity 1, low queue must drop, not block
        for i in 0..20 {
            let _ = sender.send(Arc::new(BroadcastEvent::Log(LogData {
                level: LogLevel::Info,
                message: format!("log-{i}"),
                target: LogTarget::Log,
                operation: crate::server::api::LogOperation::Append,
            })));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Publisher must not have blocked; try_send should still succeed for new logs
        let start = std::time::Instant::now();
        let _ = sender.send(Arc::new(BroadcastEvent::Log(LogData {
            level: LogLevel::Info,
            message: "after-flood".to_owned(),
            target: LogTarget::Log,
            operation: crate::server::api::LogOperation::Append,
        })));
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "log flood must not block publisher (bounded drop)"
        );
        // Low queue should have at most capacity messages, not 21
        let mut count = 0;
        while low_incoming.try_recv().is_ok() {
            count += 1;
        }
        assert!(
            count <= 2,
            "log queue must be bounded with drop, got {count}"
        );
        cancellation.cancel();
        let _ = forwarder.await;
        let _ = outgoing;
    }

    #[tokio::test]
    async fn slow_display_does_not_block_control_latency() {
        let concrete = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.state_queue_capacity = 2;
        config.ephemeral_queue_capacity = 2;
        config.heartbeat_interval = Duration::from_mins(1);
        config.pong_timeout = Duration::from_secs(10);
        let transport = WebSocketTransport::new(concrete.clone(), config).expect("transport");
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let generation = initial_generation(&mut socket).await;
        // Drain the initial script UI if any to have clean state
        // Flood display path with many state changes on same field
        // Keep flood within broadcast capacity (16) to avoid Lagged disconnect
        for i in 1..8 {
            let mut tx = StateTransaction::new(StateChangeCause::Camera);
            tx.state.camera_fps = Some(f64::from(i));
            let _ = concrete.hub.commit(tx).await;
        }
        // Concurrent control request must still be low latency
        let start = tokio::time::Instant::now();
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
        // Control must outrank display: even with display flood queued,
        // the input ack should arrive within 500ms. Drain coalesced state
        // messages that may have been queued before the control reply.
        let mut reply = None;
        while start.elapsed() < Duration::from_secs(1) {
            let msg = timeout(Duration::from_millis(200), receive_server(&mut socket)).await;
            match msg {
                Ok(m) if matches!(m, ServerMessage::InputSnapshotApplied(_)) => {
                    reply = Some(m);
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let reply = reply.expect("control reply deadline");
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "control must outrank display: latency {:?}",
            start.elapsed()
        );
        assert!(matches!(reply, ServerMessage::InputSnapshotApplied(_)));
        // Ensure display queue was bounded: drain remaining state changes
        // and verify at most a few coalesced messages remain
        let mut state_count = 0;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(200);
        while tokio::time::Instant::now() < deadline {
            match timeout(Duration::from_millis(20), socket.next()).await {
                Ok(Some(Ok(ClientFrame::Text(text)))) => {
                    if let Ok(ServerMessage::UiStateChanged(change)) =
                        serde_json::from_str::<ServerMessage>(&text)
                    {
                        state_count += 1;
                        let _ = change;
                    }
                }
                _ => break,
            }
        }
        assert!(
            state_count <= 5,
            "display queue must be coalesced and bounded, got {state_count} state messages"
        );
        socket.close(None).await.expect("close");
        concrete.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn disconnected_slow_client_does_not_block_other_clients() {
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.state_queue_capacity = 2;
        config.ephemeral_queue_capacity = 2;
        config.heartbeat_interval = Duration::from_mins(1);
        config.pong_timeout = Duration::from_secs(10);
        let transport = WebSocketTransport::new(backend.clone(), config).expect("transport");
        let broker = transport.broker();
        let (address, cancellation, task) = start_server(&transport).await;
        // First client: slow, never reads after connect (simulates disconnected/slow)
        let mut slow_socket = connect(address).await;
        let _slow_gen = initial_generation(&mut slow_socket).await;
        // Flood from main path rapidly
        for i in 0..50 {
            broker.publish_log(LogData {
                level: LogLevel::Info,
                message: format!("flood-{i}"),
                target: LogTarget::Log,
                operation: crate::server::api::LogOperation::Append,
            });
            let mut tx = StateTransaction::new(StateChangeCause::Other);
            tx.state.last_input = Some(Some(format!("input-{i}")));
            let _ = backend.hub.commit(tx).await;
        }
        // Second client should still be able to connect and get control reply quickly
        let mut fast_socket = connect(address).await;
        let fast_gen = initial_generation(&mut fast_socket).await;
        assert!(!fast_gen.is_empty());
        let start = tokio::time::Instant::now();
        send_client(
            &mut fast_socket,
            &ClientMessage::InputSnapshot(MessageData {
                data: InputSnapshot {
                    generation: fast_gen.clone(),
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
        let reply = timeout(Duration::from_secs(1), receive_server(&mut fast_socket))
            .await
            .expect("fast client control deadline");
        assert!(matches!(reply, ServerMessage::InputSnapshotApplied(_)));
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "other clients must not be blocked by slow client"
        );
        // Slow client should be lagged but not have blocked the hub:
        // Verify hub can still commit and fast client can receive a new state
        let mut tx = StateTransaction::new(StateChangeCause::Serial);
        tx.state.serial_connected = Some(true);
        let outcome = backend.hub.commit(tx).await.expect("hub still progresses");
        assert_eq!(outcome.revision().as_str(), "51");
        // Fast client should eventually see the new state (maybe coalesced)
        let mut seen = false;
        for _ in 0..5 {
            if let Ok(ServerMessage::UiStateChanged(change)) =
                timeout(Duration::from_millis(200), receive_server(&mut fast_socket)).await
                && change.data.state.serial_connected == Some(true)
            {
                seen = true;
                break;
            }
        }
        assert!(seen, "fast client must receive state despite slow peer");
        slow_socket.close(None).await.expect("close slow");
        fast_socket.close(None).await.expect("close fast");
        // Both disconnects should be observed without deadlock
        timeout(Duration::from_secs(1), async {
            while backend.disconnected.load(Ordering::Acquire) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both clients disconnected");
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    async fn motion_jpeg_slow_client_receives_latest_complete_frame() {
        let feed = MotionJpegFeed::new();
        let backend = Arc::new(TestBackend::with_motion_jpeg(Some(feed.clone())));
        let mut config = test_config();
        config.heartbeat_interval = Duration::from_mins(1);
        config.pong_timeout = Duration::from_secs(10);
        let transport = WebSocketTransport::new(backend.clone(), config).expect("transport");
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;
        // Publish many frames rapidly without reading (slow client)
        for i in 1_u8..10 {
            feed.publish(vec![0xff, 0xd8, i, 0xff, 0xd9]);
        }
        // The latest complete frame must win (coalesced)
        feed.publish(vec![0xff, 0xd8, 0xaa, 0xff, 0xd9]);
        let frame = receive_motion_jpeg(&mut socket).await;
        assert_eq!(frame.as_ref(), [0xff, 0xd8, 0xaa, 0xff, 0xd9]);
        // No intermediate frames should be queued beyond the latest
        // Try to read another frame quickly - should be timeout because queue was coalesced
        let next = timeout(Duration::from_millis(100), socket.next()).await;
        // Either timeout or next is not a binary with old frame
        if let Ok(Some(Ok(ClientFrame::Binary(bytes)))) = next {
            assert_ne!(
                bytes.as_ref(),
                [0xff, 0xd8, 1, 0xff, 0xd9],
                "intermediate frames must have been coalesced"
            );
        }
        socket.close(None).await.expect("close");
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[test]
    fn websocket_runtime_settings_validate_intervals() {
        assert_eq!(
            WebSocketRuntimeSettings::new(Duration::ZERO, Duration::from_secs(1)),
            Err(WebSocketBuildError::ZeroDuration)
        );
        assert_eq!(
            WebSocketRuntimeSettings::new(Duration::from_secs(1), Duration::ZERO),
            Err(WebSocketBuildError::ZeroDuration)
        );
        assert_eq!(
            WebSocketRuntimeSettings::new(Duration::from_secs(1), Duration::from_secs(2)),
            Err(WebSocketBuildError::PongTimeoutExceedsInterval)
        );
        let valid = WebSocketRuntimeSettings::new(Duration::from_secs(15), Duration::from_secs(10))
            .expect("valid heartbeat settings");
        assert_eq!(valid.ping_interval(), Duration::from_secs(15));
        assert_eq!(valid.pong_timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn heartbeat_ping_interval_change_reschedules_active_wait_from_application_instant() {
        let (sender, receiver) = tokio::sync::watch::channel(
            WebSocketRuntimeSettings::new(Duration::from_secs(10), Duration::from_secs(5))
                .expect("initial heartbeat settings"),
        );
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        // Keep config long so the initial wait would be 10 seconds if not rescheduled via watch.
        config.heartbeat_interval = Duration::from_secs(10);
        config.pong_timeout = Duration::from_secs(5);
        let transport = WebSocketTransport::new(backend.clone(), config)
            .expect("transport")
            .with_heartbeat_settings(receiver);
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;

        // Active heartbeat wait is currently 10 seconds. Change it after a short delay;
        // the next ping must be delivered ~250 ms after the settings-application instant.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let change_at = tokio::time::Instant::now();
        sender.send_replace(
            WebSocketRuntimeSettings::new(Duration::from_millis(250), Duration::from_millis(120))
                .expect("updated heartbeat settings"),
        );
        let message = receive_server_within(&mut socket, Duration::from_secs(2)).await;
        let elapsed = change_at.elapsed();
        assert!(
            matches!(message, ServerMessage::Ping(_)),
            "expected ping after interval reschedule, got {message:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(150) && elapsed <= Duration::from_secs(1),
            "ping must be rescheduled from settings-application instant, elapsed={elapsed:?}"
        );
        // Ensure no extra early ping; the pong path must remain functional.
        if let ServerMessage::Ping(MessageData { data }) = message {
            send_client(&mut socket, &ClientMessage::Pong(MessageData { data })).await;
        }
        socket.close(None).await.expect("close");
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }

    #[tokio::test]
    #[allow(clippy::unnested_or_patterns, clippy::match_like_matches_macro)]
    async fn heartbeat_pong_timeout_change_reschedules_active_pong_wait_from_application_instant() {
        // Start with a moderate ping and short pong timeout. After the first ping is sent,
        // the server waits for pong; changing pong_timeout must restart that wait from now.
        let (sender, receiver) = tokio::sync::watch::channel(
            WebSocketRuntimeSettings::new(Duration::from_millis(500), Duration::from_millis(200))
                .expect("initial heartbeat settings"),
        );
        let backend = Arc::new(TestBackend::new());
        let mut config = test_config();
        config.heartbeat_interval = Duration::from_millis(500);
        config.pong_timeout = Duration::from_millis(400);
        let transport = WebSocketTransport::new(backend.clone(), config)
            .expect("transport")
            .with_heartbeat_settings(receiver);
        let (address, cancellation, task) = start_server(&transport).await;
        let mut socket = connect(address).await;
        let _generation = initial_generation(&mut socket).await;

        // First ping arrives quickly (500 ms). Do not answer it; instead shorten the pong timeout
        // while the server is waiting for the pong.
        let ServerMessage::Ping(MessageData { data: first_nonce }) =
            receive_server_within(&mut socket, Duration::from_secs(2)).await
        else {
            panic!("first ping must be delivered");
        };
        // Shortly after the ping, extend the pong timeout from 200ms to 500ms.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let change_at = tokio::time::Instant::now();
        sender.send_replace(
            WebSocketRuntimeSettings::new(Duration::from_millis(500), Duration::from_millis(500))
                .expect("extended pong timeout"),
        );
        // The connection must close ~500 ms after the settings-application instant,
        // not ~150 ms after the original ping. Verify by waiting for the server to close.
        let closed = timeout(Duration::from_secs(2), socket.next()).await;
        let elapsed = change_at.elapsed();
        let is_closed = match closed {
            Ok(Some(Ok(ClientFrame::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => true,
            _ => false,
        };
        // The server's heartbeat timeout cancels the connection, which manifests as a closed
        // WebSocket (Close frame or stream termination). Ensure it happened near the rescheduled deadline.
        assert!(
            is_closed,
            "pong timeout reschedule must close connection after shortened timeout"
        );
        assert!(
            elapsed >= Duration::from_millis(350) && elapsed <= Duration::from_secs(1),
            "pong wait must be restarted from settings-application instant, elapsed={elapsed:?}"
        );
        let _ = first_nonce;
        backend.wait_for_disconnect().await;
        stop_server(cancellation, task).await;
    }
}
