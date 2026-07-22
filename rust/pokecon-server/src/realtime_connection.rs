//! Per-WebSocket orchestration of WebRTC signaling, media failover, and input
//! generation handoffs.

use std::future;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tokio::time;
use tokio_util::sync::CancellationToken;

use crate::api::{
    ClientMessage, InputGeneration, LogData, MessageData, ServerMessage, SessionDescription,
};
use crate::realtime::{
    RealtimeAction, RealtimeRoute, RealtimeTransportConfig, RealtimeTransportController,
};
use crate::webrtc::{WebRtcMedia, WebRtcPeer, WebRtcPeerConfig, WebRtcPeerEvent};
use crate::websocket::{
    ConnectionId, MotionJpegFeed, Outgoing, WebSocketBackend, WebSocketReply, log_backend_failure,
};

/// Runtime settings consumed by new attempts without renegotiating an active
/// peer. Recovery policy changes are applied immediately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeRuntimeSettings {
    stun_server: String,
    auto_recover: bool,
    recovery_probe_interval: Duration,
}

impl RealtimeRuntimeSettings {
    /// Creates a validated runtime setting snapshot.
    ///
    /// # Errors
    ///
    /// Rejects malformed STUN URIs and a zero recovery interval.
    pub fn new(
        stun_server: impl Into<String>,
        auto_recover: bool,
        recovery_probe_interval: Duration,
    ) -> Result<Self, RealtimeConnectionConfigError> {
        let stun_server = stun_server.into();
        if !stun_server.is_empty() && !is_stun_uri(&stun_server) {
            return Err(RealtimeConnectionConfigError::InvalidStunServer);
        }
        if recovery_probe_interval.is_zero() {
            return Err(RealtimeConnectionConfigError::ZeroRecoveryInterval);
        }
        Ok(Self {
            stun_server,
            auto_recover,
            recovery_probe_interval,
        })
    }

    #[must_use]
    pub fn stun_server(&self) -> &str {
        &self.stun_server
    }

    #[must_use]
    pub const fn auto_recover(&self) -> bool {
        self.auto_recover
    }

    #[must_use]
    pub const fn recovery_probe_interval(&self) -> Duration {
        self.recovery_probe_interval
    }
}

/// Shared media plus per-peer bounds for one WebSocket connection.
#[derive(Clone, Debug)]
pub struct RealtimeConnectionConfig {
    media: WebRtcMedia,
    peer: WebRtcPeerConfig,
    transport: RealtimeTransportConfig,
    runtime_settings: Option<watch::Receiver<RealtimeRuntimeSettings>>,
}

impl RealtimeConnectionConfig {
    /// Creates a fixed-settings connection policy.
    ///
    /// # Errors
    ///
    /// Rejects invalid peer bounds, timers, or STUN configuration.
    pub fn new(
        media: WebRtcMedia,
        peer: WebRtcPeerConfig,
        transport: RealtimeTransportConfig,
    ) -> Result<Self, RealtimeConnectionConfigError> {
        let peer = WebRtcPeerConfig::new(
            peer.stun_server,
            peer.event_queue_capacity,
            peer.max_control_message_bytes,
            peer.max_buffered_amount,
        )
        .map_err(|_| RealtimeConnectionConfigError::InvalidPeerConfig)?;
        let transport = RealtimeTransportConfig::new(
            transport.connect_timeout,
            transport.inactivity_timeout,
            transport.auto_recover,
            transport.recovery_probe_interval,
        )
        .map_err(|_| RealtimeConnectionConfigError::InvalidTransportConfig)?;
        RealtimeRuntimeSettings::new(
            peer.stun_server.clone(),
            transport.auto_recover,
            transport.recovery_probe_interval,
        )?;
        Ok(Self {
            media,
            peer,
            transport,
            runtime_settings: None,
        })
    }

    /// Subscribes this policy to canonical runtime setting snapshots.
    #[must_use]
    pub fn with_runtime_settings(
        mut self,
        runtime_settings: watch::Receiver<RealtimeRuntimeSettings>,
    ) -> Self {
        self.runtime_settings = Some(runtime_settings);
        self
    }

    pub(crate) fn motion_jpeg(&self) -> MotionJpegFeed {
        self.media.motion_jpeg()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RealtimeConnectionConfigError {
    #[error("the STUN server must be empty or a canonical stun: or stuns: URI")]
    InvalidStunServer,
    #[error("the recovery probe interval must be greater than zero")]
    ZeroRecoveryInterval,
    #[error("WebRTC peer queue bounds must be greater than zero")]
    InvalidPeerConfig,
    #[error("realtime transport timers must be greater than zero")]
    InvalidTransportConfig,
}

pub(crate) struct RealtimeConnectionIo {
    pub backend: Arc<dyn WebSocketBackend>,
    pub connection: ConnectionId,
    pub outgoing_high: mpsc::Sender<Outgoing>,
    pub outgoing_low: mpsc::Sender<Outgoing>,
    pub messages: mpsc::Receiver<ClientMessage>,
    pub logs: mpsc::Receiver<LogData>,
    pub motion_jpeg_enabled: watch::Sender<bool>,
    pub backend_timeout: Duration,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputRoute {
    WebSocket,
    WebRtc(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingRtcInput {
    attempt: u64,
    generation: String,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
enum ConnectionError {
    #[error("a critical WebSocket output queue is unavailable")]
    OutgoingUnavailable,
    #[error("the application backend rejected or timed out during a realtime operation")]
    BackendUnavailable,
    #[error("the input generation counter is exhausted")]
    GenerationExhausted,
    #[error("the realtime command queue is unavailable")]
    CommandQueueUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartPeerError {
    Peer,
    Outgoing,
}

struct Coordinator {
    io: RealtimeConnectionIo,
    media: WebRtcMedia,
    peer_config: WebRtcPeerConfig,
    runtime_settings: Option<watch::Receiver<RealtimeRuntimeSettings>>,
    settings: RealtimeRuntimeSettings,
    controller: RealtimeTransportController,
    peer: Option<WebRtcPeer>,
    pending_remote_offer: Option<SessionDescription>,
    pending_rtc_input: Option<PendingRtcInput>,
    websocket_generation_sequence: u64,
}

/// Runs one connection until its WebSocket is cancelled or a critical
/// application/queue invariant fails.
pub(crate) async fn run_realtime_connection(
    config: RealtimeConnectionConfig,
    io: RealtimeConnectionIo,
) {
    let cancellation = io.cancellation.clone();
    let connection = io.connection;
    let mut coordinator = Coordinator::new(config, io);
    let result = coordinator.run().await;
    coordinator.close_peer().await;
    if let Err(error) = result {
        tracing::warn!(
            connection_id = connection.get(),
            %error,
            "realtime connection coordinator stopped"
        );
        cancellation.cancel();
    }
}

impl Coordinator {
    fn new(config: RealtimeConnectionConfig, io: RealtimeConnectionIo) -> Self {
        let settings = config.runtime_settings.as_ref().map_or_else(
            || {
                RealtimeRuntimeSettings::new(
                    config.peer.stun_server.clone(),
                    config.transport.auto_recover,
                    config.transport.recovery_probe_interval,
                )
                .expect("RealtimeConnectionConfig validates fixed settings")
            },
            |receiver| receiver.borrow().clone(),
        );
        let mut transport = config.transport;
        transport.auto_recover = settings.auto_recover;
        transport.recovery_probe_interval = settings.recovery_probe_interval;
        Self {
            io,
            media: config.media,
            peer_config: config.peer,
            runtime_settings: config.runtime_settings,
            settings,
            controller: RealtimeTransportController::new(transport, Instant::now()),
            peer: None,
            pending_remote_offer: None,
            pending_rtc_input: None,
            websocket_generation_sequence: 0,
        }
    }

    async fn run(&mut self) -> Result<(), ConnectionError> {
        self.drain_actions().await?;
        loop {
            let peer_event = next_peer_event(&mut self.peer);
            let runtime_event = next_runtime_event(&mut self.runtime_settings);
            let wakeup = wait_for_wakeup(self.controller.next_wakeup());
            tokio::select! {
                biased;
                () = self.io.cancellation.cancelled() => return Ok(()),
                message = self.io.messages.recv() => {
                    let Some(message) = message else {
                        return Err(ConnectionError::CommandQueueUnavailable);
                    };
                    self.handle_websocket_message(message, Instant::now()).await?;
                }
                event = peer_event => self.handle_peer_event(event, Instant::now()).await?,
                event = runtime_event => self.handle_runtime_event(event, Instant::now()),
                () = wakeup => self.controller.tick(Instant::now()),
                log = self.io.logs.recv() => {
                    let Some(log) = log else {
                        return Err(ConnectionError::CommandQueueUnavailable);
                    };
                    self.forward_log(log).await;
                }
            }
            self.drain_actions().await?;
        }
    }

    async fn handle_websocket_message(
        &mut self,
        message: ClientMessage,
        now: Instant,
    ) -> Result<(), ConnectionError> {
        match message {
            ClientMessage::WebRtcOffer(MessageData { data }) => {
                if let Some(attempt) = self.peer.as_ref().map(WebRtcPeer::attempt) {
                    self.controller.peer_failed(attempt, now);
                }
                if self.controller.route() == RealtimeRoute::WebSocketFallback
                    && self.controller.active_attempt().is_none()
                {
                    self.pending_remote_offer = Some(data);
                    self.controller.manual_reconnect(now);
                }
            }
            ClientMessage::WebRtcAnswer(MessageData { data }) => {
                let Some(peer) = self.peer.as_ref() else {
                    return Ok(());
                };
                let attempt = peer.attempt();
                if peer.accept_answer(data).await.is_err() {
                    self.controller.peer_failed(attempt, now);
                }
            }
            ClientMessage::WebRtcIceCandidate(MessageData { data }) => {
                let Some(peer) = self.peer.as_ref() else {
                    return Ok(());
                };
                let attempt = peer.attempt();
                if peer.add_ice_candidate(data).await.is_err() {
                    self.controller.peer_failed(attempt, now);
                }
            }
            message => {
                self.dispatch_backend(message, InputRoute::WebSocket, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_peer_event(
        &mut self,
        event: PeerEvent,
        now: Instant,
    ) -> Result<(), ConnectionError> {
        let attempt = event.attempt;
        match event.event {
            Some(WebRtcPeerEvent::IceCandidate(data)) => {
                self.send_high(ServerMessage::WebRtcIceCandidate(MessageData { data }))?;
            }
            Some(WebRtcPeerEvent::VideoReady) => {
                self.controller.peer_readiness(attempt, true, false, now);
            }
            Some(WebRtcPeerEvent::DataChannelsReady) => {
                self.controller.peer_readiness(attempt, false, true, now);
            }
            Some(WebRtcPeerEvent::ControlMessage(message)) => {
                self.controller.primary_activity(attempt, now);
                if self
                    .peer
                    .as_ref()
                    .is_none_or(|peer| peer.attempt() != attempt)
                {
                    return Ok(());
                }
                let Ok(message) = serde_json::from_str::<ClientMessage>(&message) else {
                    self.controller.peer_failed(attempt, now);
                    return Ok(());
                };
                if matches!(
                    message,
                    ClientMessage::WebRtcOffer(_)
                        | ClientMessage::WebRtcAnswer(_)
                        | ClientMessage::WebRtcIceCandidate(_)
                        | ClientMessage::Pong(_)
                ) {
                    self.controller.peer_failed(attempt, now);
                } else {
                    self.dispatch_backend(message, InputRoute::WebRtc(attempt), now)
                        .await?;
                }
            }
            Some(WebRtcPeerEvent::MediaActivity) => {
                self.controller.primary_activity(attempt, now);
            }
            Some(WebRtcPeerEvent::Failed) | None => {
                self.controller.peer_failed(attempt, now);
            }
        }
        Ok(())
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent, now: Instant) {
        match event {
            RuntimeEvent::Changed(settings) => {
                self.controller
                    .update_recovery(settings.auto_recover, settings.recovery_probe_interval, now)
                    .expect("RealtimeRuntimeSettings forbids zero intervals");
                self.settings = settings;
            }
            RuntimeEvent::Closed => self.runtime_settings = None,
        }
    }

    async fn dispatch_backend(
        &mut self,
        message: ClientMessage,
        route: InputRoute,
        now: Instant,
    ) -> Result<(), ConnectionError> {
        let replies = match time::timeout(
            self.io.backend_timeout,
            self.io.backend.message(self.io.connection, message),
        )
        .await
        {
            Ok(Ok(replies)) => replies,
            Ok(Err(error)) => {
                log_backend_failure(self.io.connection, &error);
                return Err(ConnectionError::BackendUnavailable);
            }
            Err(_elapsed) => {
                tracing::warn!(
                    connection_id = self.io.connection.get(),
                    "realtime backend operation timed out"
                );
                return Err(ConnectionError::BackendUnavailable);
            }
        };
        self.forward_backend_replies(replies, route, now).await;
        Ok(())
    }

    async fn forward_backend_replies(
        &mut self,
        replies: Vec<WebSocketReply>,
        route: InputRoute,
        now: Instant,
    ) {
        for reply in replies {
            let applied = match &reply {
                WebSocketReply::InputSnapshotApplied(data) => Some(data.clone()),
                _ => None,
            };
            let signaling = matches!(
                reply,
                WebSocketReply::WebRtcOffer(_)
                    | WebSocketReply::WebRtcAnswer(_)
                    | WebSocketReply::WebRtcIceCandidate(_)
            );
            let log = match &reply {
                WebSocketReply::Log(data) => Some(data.clone()),
                _ => None,
            };
            let message = ServerMessage::from(reply);
            let delivered = match (route, signaling, log) {
                (_, true, _) | (InputRoute::WebSocket, false, None) => {
                    self.send_high(message).is_ok()
                }
                (InputRoute::WebSocket, false, Some(_)) => {
                    self.send_low(message);
                    true
                }
                (InputRoute::WebRtc(attempt), false, Some(log)) => {
                    self.send_peer_log(attempt, &log).await;
                    true
                }
                (InputRoute::WebRtc(attempt), false, None) => {
                    self.send_peer_control(attempt, &message).await
                }
            };
            if !delivered {
                match route {
                    InputRoute::WebSocket => self.io.cancellation.cancel(),
                    InputRoute::WebRtc(attempt) => self.controller.peer_failed(attempt, now),
                }
                break;
            }
            if let (InputRoute::WebRtc(attempt), Some(applied), Some(pending)) =
                (route, applied, self.pending_rtc_input.as_ref())
                && pending.attempt == attempt
                && pending.generation == applied.generation
                && applied.sequence.as_str() == "0"
            {
                self.pending_rtc_input = None;
                self.controller.primary_input_applied(attempt, now);
            }
        }
    }

    async fn drain_actions(&mut self) -> Result<(), ConnectionError> {
        loop {
            let actions = self.controller.take_actions();
            if actions.is_empty() {
                return Ok(());
            }
            for action in actions {
                match action {
                    RealtimeAction::StartWebRtcAttempt { attempt, .. } => {
                        match self.start_peer(attempt).await {
                            Ok(()) => {}
                            Err(StartPeerError::Peer) => {
                                self.controller.peer_failed(attempt, Instant::now());
                            }
                            Err(StartPeerError::Outgoing) => {
                                return Err(ConnectionError::OutgoingUnavailable);
                            }
                        }
                    }
                    RealtimeAction::StopWebRtcAttempt { attempt } => {
                        self.stop_peer(attempt).await;
                    }
                    RealtimeAction::BeginInputHandoff { route, attempt } => {
                        self.begin_input_handoff(route, attempt).await?;
                    }
                    RealtimeAction::ActivateWebRtc { attempt } => {
                        if self
                            .peer
                            .as_ref()
                            .is_some_and(|peer| peer.attempt() == attempt)
                        {
                            let _previous = self.io.motion_jpeg_enabled.send_replace(false);
                        } else {
                            self.controller.peer_failed(attempt, Instant::now());
                        }
                    }
                    RealtimeAction::EnableMotionJpeg => {
                        let _previous = self.io.motion_jpeg_enabled.send_replace(true);
                    }
                }
            }
        }
    }

    async fn start_peer(&mut self, attempt: u64) -> Result<(), StartPeerError> {
        self.close_peer().await;
        let mut peer_config = self.peer_config.clone();
        peer_config
            .stun_server
            .clone_from(&self.settings.stun_server);
        let peer = WebRtcPeer::new(attempt, &self.media, peer_config)
            .await
            .map_err(|_| StartPeerError::Peer)?;
        let signaling = if let Some(offer) = self.pending_remote_offer.take() {
            peer.accept_offer(offer)
                .await
                .map(|data| ServerMessage::WebRtcAnswer(MessageData { data }))
        } else {
            peer.create_offer()
                .await
                .map(|data| ServerMessage::WebRtcOffer(MessageData { data }))
        };
        let signaling = match signaling {
            Ok(signaling) => signaling,
            Err(_error) => {
                peer.close().await;
                return Err(StartPeerError::Peer);
            }
        };
        if self.send_high(signaling).is_err() {
            peer.close().await;
            return Err(StartPeerError::Outgoing);
        }
        self.peer = Some(peer);
        Ok(())
    }

    async fn begin_input_handoff(
        &mut self,
        route: RealtimeRoute,
        attempt: Option<u64>,
    ) -> Result<(), ConnectionError> {
        match (route, attempt) {
            (RealtimeRoute::WebRtc, Some(attempt)) => {
                let generation = InputGeneration {
                    generation: format!("rtc-{}-{attempt}", self.io.connection.get()),
                };
                self.replace_backend_generation(&generation).await?;
                self.pending_rtc_input = Some(PendingRtcInput {
                    attempt,
                    generation: generation.generation.clone(),
                });
                let message = ServerMessage::InputGeneration(MessageData { data: generation });
                if !self.send_peer_control(attempt, &message).await {
                    self.controller.peer_failed(attempt, Instant::now());
                }
            }
            (RealtimeRoute::WebSocketFallback, None) => {
                self.websocket_generation_sequence = self
                    .websocket_generation_sequence
                    .checked_add(1)
                    .ok_or(ConnectionError::GenerationExhausted)?;
                let generation = InputGeneration {
                    generation: format!(
                        "ws-{}-{}",
                        self.io.connection.get(),
                        self.websocket_generation_sequence
                    ),
                };
                self.replace_backend_generation(&generation).await?;
                self.pending_rtc_input = None;
                self.send_high(ServerMessage::InputGeneration(MessageData {
                    data: generation,
                }))?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn replace_backend_generation(
        &self,
        generation: &InputGeneration,
    ) -> Result<(), ConnectionError> {
        match time::timeout(
            self.io.backend_timeout,
            self.io.backend.connected(self.io.connection, generation),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                log_backend_failure(self.io.connection, &error);
                Err(ConnectionError::BackendUnavailable)
            }
            Err(_elapsed) => {
                tracing::warn!(
                    connection_id = self.io.connection.get(),
                    "realtime input generation handoff timed out"
                );
                Err(ConnectionError::BackendUnavailable)
            }
        }
    }

    async fn stop_peer(&mut self, attempt: u64) {
        if self
            .peer
            .as_ref()
            .is_some_and(|peer| peer.attempt() == attempt)
        {
            self.close_peer().await;
        }
        if self
            .pending_rtc_input
            .as_ref()
            .is_some_and(|pending| pending.attempt == attempt)
        {
            self.pending_rtc_input = None;
        }
    }

    async fn close_peer(&mut self) {
        if let Some(peer) = self.peer.take() {
            peer.close().await;
        }
    }

    fn send_high(&self, message: ServerMessage) -> Result<(), ConnectionError> {
        self.io
            .outgoing_high
            .try_send(Outgoing::Json(message))
            .map_err(|_| ConnectionError::OutgoingUnavailable)
    }

    fn send_low(&self, message: ServerMessage) {
        let _dropped_if_slow = self.io.outgoing_low.try_send(Outgoing::Json(message));
    }

    async fn send_peer_control(&self, attempt: u64, message: &ServerMessage) -> bool {
        let Some(peer) = self.peer.as_ref().filter(|peer| peer.attempt() == attempt) else {
            return false;
        };
        peer.send_control(message).await.is_ok()
    }

    async fn send_peer_log(&self, attempt: u64, log: &LogData) {
        let Some(peer) = self.peer.as_ref().filter(|peer| peer.attempt() == attempt) else {
            return;
        };
        let _dropped_on_backpressure_or_close = peer.send_log(log).await;
    }

    async fn forward_log(&self, log: LogData) {
        if self.controller.route() == RealtimeRoute::WebRtc
            && let Some(peer) = self.peer.as_ref()
        {
            self.send_peer_log(peer.attempt(), &log).await;
        } else {
            self.send_low(ServerMessage::Log(MessageData { data: log }));
        }
    }
}

struct PeerEvent {
    attempt: u64,
    event: Option<WebRtcPeerEvent>,
}

async fn next_peer_event(peer: &mut Option<WebRtcPeer>) -> PeerEvent {
    let Some(peer) = peer else {
        return future::pending().await;
    };
    PeerEvent {
        attempt: peer.attempt(),
        event: peer.recv().await,
    }
}

enum RuntimeEvent {
    Changed(RealtimeRuntimeSettings),
    Closed,
}

async fn next_runtime_event(
    settings: &mut Option<watch::Receiver<RealtimeRuntimeSettings>>,
) -> RuntimeEvent {
    let Some(settings) = settings else {
        return future::pending().await;
    };
    if settings.changed().await.is_err() {
        return RuntimeEvent::Closed;
    }
    RuntimeEvent::Changed(settings.borrow_and_update().clone())
}

async fn wait_for_wakeup(wakeup: Option<Instant>) {
    let Some(wakeup) = wakeup else {
        return future::pending().await;
    };
    time::sleep_until(time::Instant::from_std(wakeup)).await;
}

fn is_stun_uri(value: &str) -> bool {
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(parsed.scheme(), "stun" | "stuns")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }
    let target = parsed.path();
    if target.is_empty()
        || target.starts_with("//")
        || target.contains(['/', '@'])
        || target.chars().any(char::is_whitespace)
    {
        return false;
    }
    if let Some(rest) = target.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        if address.parse::<Ipv6Addr>().is_err()
            || (!suffix.is_empty() && !valid_stun_port(suffix.strip_prefix(':')))
        {
            return false;
        }
    } else if let Some((host, port)) = target.split_once(':')
        && (host.is_empty() || host.contains(':') || !valid_stun_port(Some(port)))
    {
        return false;
    }
    let Ok(authority) = url::Url::parse(&format!("http://{target}")) else {
        return false;
    };
    authority.host_str().is_some()
        && authority.username().is_empty()
        && authority.password().is_none()
        && authority.path() == "/"
        && authority.query().is_none()
        && authority.fragment().is_none()
        && authority.port().is_none_or(|port| port != 0)
}

fn valid_stun_port(port: Option<&str>) -> bool {
    port.is_some_and(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_settings_validate_stun_and_recovery_interval() {
        for valid in [
            "",
            "stun:example.com",
            "stuns:example.com:5349",
            "stun:[2001:db8::1]:3478",
        ] {
            RealtimeRuntimeSettings::new(valid, true, Duration::from_secs(1))
                .expect("valid realtime settings");
        }
        for invalid in [
            "https://example.com",
            "stun:",
            "stun://example.com",
            "stun:example.com:0",
            "stun:user@example.com",
        ] {
            assert_eq!(
                RealtimeRuntimeSettings::new(invalid, true, Duration::from_secs(1)),
                Err(RealtimeConnectionConfigError::InvalidStunServer)
            );
        }
        assert_eq!(
            RealtimeRuntimeSettings::new("", true, Duration::ZERO),
            Err(RealtimeConnectionConfigError::ZeroRecoveryInterval)
        );
    }
}
