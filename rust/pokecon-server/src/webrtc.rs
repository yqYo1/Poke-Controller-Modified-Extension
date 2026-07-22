//! Native WebRTC peer, H.264 media pipeline, and isolated `DataChannels`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use axum::body::Bytes;
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Level, Profile, RateControlMode,
    VuiConfig,
};
use openh264::formats::{BgrSliceU8, YUVBuffer};
use pokecon_camera::{MediaFrame, MotionJpegSource, WebRtcFrameSource};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinHandle, spawn_blocking};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MIME_TYPE_H264, MIME_TYPE_VP8, MIME_TYPE_VP9, MediaEngine};
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::Sample;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::RTCPFeedback;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use crate::api::{IceCandidate, LogData, MessageData, ServerMessage, SessionDescription};
use crate::websocket::MotionJpegFeed;

pub const CONTROL_DATA_CHANNEL: &str = "pokecon-control";
pub const LOG_DATA_CHANNEL: &str = "pokecon-log";

const CONTROL_CHANNEL_BIT: u8 = 1;
const LOG_CHANNEL_BIT: u8 = 2;
const ALL_CHANNEL_BITS: u8 = CONTROL_CHANNEL_BIT | LOG_CHANNEL_BIT;
const ACTIVITY_EVENT_INTERVAL: Duration = Duration::from_secs(1);
const MIN_SAMPLE_DURATION: Duration = Duration::from_millis(1);
const MAX_SAMPLE_DURATION: Duration = Duration::from_secs(1);
const H264_PAYLOAD_TYPE: u8 = 102;
const VP8_PAYLOAD_TYPE: u8 = 96;
const VP9_PAYLOAD_TYPE: u8 = 98;
const H264_FMTP: &str = "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e02a";

/// Encoder and bounded fan-out policy shared by all browser peers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebRtcMediaConfig {
    pub max_fps: u16,
    pub bitrate_bps: u32,
    pub keyframe_interval_frames: u32,
    pub h264_queue_capacity: usize,
}

impl WebRtcMediaConfig {
    /// Validates the encoder and broadcast bounds.
    ///
    /// # Errors
    ///
    /// Rejects zero values.
    pub const fn new(
        max_fps: u16,
        bitrate_bps: u32,
        keyframe_interval_frames: u32,
        h264_queue_capacity: usize,
    ) -> Result<Self, WebRtcError> {
        if max_fps == 0
            || bitrate_bps == 0
            || keyframe_interval_frames == 0
            || h264_queue_capacity == 0
        {
            return Err(WebRtcError::InvalidConfig);
        }
        Ok(Self {
            max_fps,
            bitrate_bps,
            keyframe_interval_frames,
            h264_queue_capacity,
        })
    }
}

impl Default for WebRtcMediaConfig {
    fn default() -> Self {
        Self {
            max_fps: 60,
            bitrate_bps: 8_000_000,
            keyframe_interval_frames: 120,
            h264_queue_capacity: 16,
        }
    }
}

/// Per-peer signaling and queue bounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebRtcPeerConfig {
    pub stun_server: String,
    pub event_queue_capacity: usize,
    pub max_control_message_bytes: usize,
    pub max_buffered_amount: usize,
}

impl WebRtcPeerConfig {
    /// Validates finite application queues. The STUN URI is already validated
    /// by the canonical settings layer; an empty string disables STUN.
    ///
    /// # Errors
    ///
    /// Rejects zero queue and message bounds.
    pub fn new(
        stun_server: impl Into<String>,
        event_queue_capacity: usize,
        max_control_message_bytes: usize,
        max_buffered_amount: usize,
    ) -> Result<Self, WebRtcError> {
        if event_queue_capacity == 0 || max_control_message_bytes == 0 || max_buffered_amount == 0 {
            return Err(WebRtcError::InvalidConfig);
        }
        Ok(Self {
            stun_server: stun_server.into(),
            event_queue_capacity,
            max_control_message_bytes,
            max_buffered_amount,
        })
    }
}

impl Default for WebRtcPeerConfig {
    fn default() -> Self {
        Self {
            stun_server: String::new(),
            event_queue_capacity: 128,
            max_control_message_bytes: 1024 * 1024,
            max_buffered_amount: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebRtcPeerEvent {
    IceCandidate(IceCandidate),
    VideoReady,
    DataChannelsReady,
    ControlMessage(String),
    MediaActivity,
    Failed,
}

/// Fixed failures safe to map at the application transport boundary.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WebRtcError {
    #[error("WebRTC configuration contains a zero bound")]
    InvalidConfig,
    #[error("the H.264 encoder could not be initialized")]
    EncoderUnavailable,
    #[error("the WebRTC peer could not be created")]
    PeerCreationFailed,
    #[error("the WebRTC session description is invalid")]
    InvalidSessionDescription,
    #[error("the WebRTC ICE candidate is invalid")]
    InvalidIceCandidate,
    #[error("the WebRTC peer operation failed")]
    PeerOperationFailed,
    #[error("the WebRTC DataChannel is not accepting messages")]
    DataChannelUnavailable,
    #[error("the WebRTC DataChannel backpressure bound was reached")]
    DataChannelBackpressure,
}

#[derive(Clone, Debug)]
struct EncodedH264Frame {
    frame_sequence: u64,
    bytes: Bytes,
    duration: Duration,
}

struct WebRtcMediaInner {
    h264: broadcast::Sender<Arc<EncodedH264Frame>>,
    motion_jpeg: MotionJpegFeed,
    keyframe_requested: Arc<AtomicBool>,
    cancellation: CancellationToken,
    h264_task: JoinHandle<()>,
    motion_jpeg_task: JoinHandle<()>,
}

impl Drop for WebRtcMediaInner {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.h264_task.abort();
        self.motion_jpeg_task.abort();
    }
}

/// One encoder pair shared by all active peer connections.
#[derive(Clone)]
pub struct WebRtcMedia {
    inner: Arc<WebRtcMediaInner>,
}

impl std::fmt::Debug for WebRtcMedia {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebRtcMedia")
            .field("h264_receivers", &self.inner.h264.receiver_count())
            .field(
                "motion_jpeg_receivers",
                &self.inner.motion_jpeg.subscriber_count(),
            )
            .finish_non_exhaustive()
    }
}

impl WebRtcMedia {
    /// Starts blocking H.264/JPEG encoding behind async latest-frame sources.
    ///
    /// # Errors
    ///
    /// Returns a fixed error if `OpenH264` cannot initialize.
    pub async fn new(
        h264_source: WebRtcFrameSource,
        motion_jpeg_source: MotionJpegSource,
        config: WebRtcMediaConfig,
    ) -> Result<Self, WebRtcError> {
        WebRtcMediaConfig::new(
            config.max_fps,
            config.bitrate_bps,
            config.keyframe_interval_frames,
            config.h264_queue_capacity,
        )?;
        let encoder = spawn_blocking(move || build_encoder(config))
            .await
            .map_err(|_| WebRtcError::EncoderUnavailable)??;
        let (h264, _receiver) = broadcast::channel(config.h264_queue_capacity);
        let motion_jpeg = MotionJpegFeed::new();
        let cancellation = CancellationToken::new();
        let keyframe_requested = Arc::new(AtomicBool::new(false));
        let h264_task = tokio::spawn(run_h264_encoder(
            h264_source,
            encoder,
            h264.clone(),
            Arc::clone(&keyframe_requested),
            cancellation.clone(),
            config.max_fps,
        ));
        let motion_jpeg_task = tokio::spawn(run_motion_jpeg_encoder(
            motion_jpeg_source,
            motion_jpeg.clone(),
            cancellation.clone(),
        ));
        Ok(Self {
            inner: Arc::new(WebRtcMediaInner {
                h264,
                motion_jpeg,
                keyframe_requested,
                cancellation,
                h264_task,
                motion_jpeg_task,
            }),
        })
    }

    #[must_use]
    pub fn motion_jpeg(&self) -> MotionJpegFeed {
        self.inner.motion_jpeg.clone()
    }

    fn subscribe_h264(&self) -> broadcast::Receiver<Arc<EncodedH264Frame>> {
        self.inner.keyframe_requested.store(true, Ordering::Release);
        self.inner.h264.subscribe()
    }
}

fn build_encoder(config: WebRtcMediaConfig) -> Result<Encoder, WebRtcError> {
    let encoder_config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(config.bitrate_bps))
        .max_frame_rate(FrameRate::from_hz(f32::from(config.max_fps)))
        .rate_control_mode(RateControlMode::Bitrate)
        .profile(Profile::Baseline)
        .level(Level::Level_4_2)
        .intra_frame_period(IntraFramePeriod::from_num_frames(
            config.keyframe_interval_frames,
        ))
        .vui(VuiConfig::bt709());
    Encoder::with_api_config(OpenH264API::from_source(), encoder_config)
        .map_err(|_| WebRtcError::EncoderUnavailable)
}

async fn run_h264_encoder(
    mut source: WebRtcFrameSource,
    mut encoder: Encoder,
    frames: broadcast::Sender<Arc<EncodedH264Frame>>,
    keyframe_requested: Arc<AtomicBool>,
    cancellation: CancellationToken,
    max_fps: u16,
) {
    let default_duration = Duration::from_secs_f64(1.0 / f64::from(max_fps));
    let mut last_encoded_at = None;
    let mut initialized = false;
    loop {
        let changed = tokio::select! {
            () = cancellation.cancelled() => return,
            changed = source.changed() => changed,
        };
        let Ok(Some(frame)) = changed else {
            if changed.is_err() {
                return;
            }
            continue;
        };
        if frames.receiver_count() == 0 {
            continue;
        }
        let now = Instant::now();
        let duration = last_encoded_at.map_or(default_duration, |previous: Instant| {
            now.duration_since(previous)
                .clamp(MIN_SAMPLE_DURATION, MAX_SAMPLE_DURATION)
        });
        last_encoded_at = Some(now);
        let force_keyframe = keyframe_requested.swap(false, Ordering::AcqRel);
        let frame_sequence = frame.frame_sequence;
        let blocking_result = spawn_blocking(move || {
            if force_keyframe && initialized {
                encoder.force_intra_frame();
            }
            let result = encode_h264_frame(&mut encoder, &frame);
            (encoder, result)
        })
        .await;
        let Ok((next_encoder, result)) = blocking_result else {
            tracing::error!(
                event = "WebRtcH264EncoderPanicked",
                "H.264 encoder task failed"
            );
            return;
        };
        encoder = next_encoder;
        let Ok(bytes) = result else {
            tracing::warn!(
                event = "WebRtcH264EncodeFailed",
                "H.264 frame encoding failed"
            );
            continue;
        };
        initialized = true;
        if bytes.is_empty() {
            continue;
        }
        let _subscribers = frames.send(Arc::new(EncodedH264Frame {
            frame_sequence,
            bytes: Bytes::from(bytes),
            duration,
        }));
    }
}

fn encode_h264_frame(encoder: &mut Encoder, frame: &MediaFrame) -> Result<Vec<u8>, WebRtcError> {
    let size = frame.frame.size();
    let bgr = BgrSliceU8::new(
        frame.frame.pixels(),
        (size.width() as usize, size.height() as usize),
    );
    let yuv = YUVBuffer::from_rgb_source(bgr);
    encoder
        .encode(&yuv)
        .map(|bitstream| bitstream.to_vec())
        .map_err(|_| WebRtcError::EncoderUnavailable)
}

async fn run_motion_jpeg_encoder(
    mut source: MotionJpegSource,
    feed: MotionJpegFeed,
    cancellation: CancellationToken,
) {
    loop {
        let changed = tokio::select! {
            () = cancellation.cancelled() => break,
            changed = source.changed() => changed,
        };
        let Ok(frame) = changed else {
            break;
        };
        let Some(frame) = frame else {
            feed.suspend();
            continue;
        };
        if feed.subscriber_count() == 0 {
            continue;
        }
        let jpeg_source = source.clone();
        let blocking_result = spawn_blocking(move || jpeg_source.encode_jpeg(&frame)).await;
        match blocking_result {
            Ok(Ok(frame)) => feed.publish(frame.bytes),
            Ok(Err(_)) => {
                tracing::warn!(
                    event = "MotionJpegEncodeFailed",
                    "Motion JPEG encoding failed"
                );
            }
            Err(_) => {
                tracing::error!(
                    event = "MotionJpegEncoderPanicked",
                    "Motion JPEG encoder task failed"
                );
                break;
            }
        }
    }
    feed.suspend();
}

#[derive(Debug)]
struct DataChannelReadiness {
    opened: AtomicU8,
    emitted: AtomicBool,
}

impl DataChannelReadiness {
    fn opened(&self, bit: u8) -> bool {
        let previous = self.opened.fetch_or(bit, Ordering::AcqRel);
        (previous | bit) == ALL_CHANNEL_BITS && !self.emitted.swap(true, Ordering::AcqRel)
    }
}

/// One standards-compliant peer attempt. The controller and log channels are
/// ordered, reliable, and physically distinct so log pressure cannot block
/// input before it reaches the application queues.
pub struct WebRtcPeer {
    attempt: u64,
    peer: Arc<RTCPeerConnection>,
    control: Arc<RTCDataChannel>,
    log: Arc<RTCDataChannel>,
    events: mpsc::Receiver<WebRtcPeerEvent>,
    max_buffered_amount: usize,
    cancellation: CancellationToken,
    video_task: JoinHandle<()>,
    rtcp_task: JoinHandle<()>,
}

impl std::fmt::Debug for WebRtcPeer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebRtcPeer")
            .field("attempt", &self.attempt)
            .field("control_state", &self.control.ready_state())
            .field("log_state", &self.log.ready_state())
            .finish_non_exhaustive()
    }
}

impl Drop for WebRtcPeer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.video_task.abort();
        self.rtcp_task.abort();
    }
}

impl WebRtcPeer {
    /// Creates an offer-capable peer with a H.264 track and two reliable
    /// application `DataChannels`.
    ///
    /// # Errors
    ///
    /// Returns a fixed peer-creation error without exposing native details.
    pub async fn new(
        attempt: u64,
        media: &WebRtcMedia,
        config: WebRtcPeerConfig,
    ) -> Result<Self, WebRtcError> {
        WebRtcPeerConfig::new(
            config.stun_server.clone(),
            config.event_queue_capacity,
            config.max_control_message_bytes,
            config.max_buffered_amount,
        )?;
        let peer = Arc::new(create_peer_connection(&config).await?);
        let (events, event_receiver) = mpsc::channel(config.event_queue_capacity);
        let cancellation = CancellationToken::new();
        install_peer_state_handler(&peer, events.clone(), cancellation.clone());
        install_ice_handler(&peer, events.clone(), cancellation.clone());

        let video_track = Arc::new(TrackLocalStaticSample::new(
            h264_capability(),
            "pokecon-video".to_owned(),
            "pokecon-camera".to_owned(),
        ));
        let rtp_sender = peer
            .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|_| WebRtcError::PeerCreationFailed)?;
        let rtcp_task = spawn_rtcp_reader(rtp_sender, events.clone(), cancellation.clone());
        let video_task = spawn_video_writer(
            video_track,
            media.subscribe_h264(),
            events.clone(),
            cancellation.clone(),
        );

        let channel_options = Some(RTCDataChannelInit {
            ordered: Some(true),
            max_packet_life_time: None,
            max_retransmits: None,
            protocol: Some("pokecon-json-v1".to_owned()),
            negotiated: None,
        });
        let control = peer
            .create_data_channel(CONTROL_DATA_CHANNEL, channel_options.clone())
            .await
            .map_err(|_| WebRtcError::PeerCreationFailed)?;
        let log = peer
            .create_data_channel(LOG_DATA_CHANNEL, channel_options)
            .await
            .map_err(|_| WebRtcError::PeerCreationFailed)?;
        let readiness = Arc::new(DataChannelReadiness {
            opened: AtomicU8::new(0),
            emitted: AtomicBool::new(false),
        });
        install_data_channel_handlers(
            &control,
            CONTROL_CHANNEL_BIT,
            Arc::clone(&readiness),
            events.clone(),
            cancellation.clone(),
            Some(config.max_control_message_bytes),
        );
        install_data_channel_handlers(
            &log,
            LOG_CHANNEL_BIT,
            readiness,
            events,
            cancellation.clone(),
            None,
        );

        Ok(Self {
            attempt,
            peer,
            control,
            log,
            events: event_receiver,
            max_buffered_amount: config.max_buffered_amount,
            cancellation,
            video_task,
            rtcp_task,
        })
    }

    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    /// Creates and installs the local offer before returning its SDP.
    ///
    /// # Errors
    ///
    /// Returns a fixed peer-operation failure.
    pub async fn create_offer(&self) -> Result<SessionDescription, WebRtcError> {
        let offer = self
            .peer
            .create_offer(None)
            .await
            .map_err(|_| WebRtcError::PeerOperationFailed)?;
        self.peer
            .set_local_description(offer)
            .await
            .map_err(|_| WebRtcError::PeerOperationFailed)?;
        local_description(&self.peer).await
    }

    /// Accepts a browser offer and installs the generated answer.
    ///
    /// # Errors
    ///
    /// Rejects malformed SDP or a failed peer operation.
    pub async fn accept_offer(
        &self,
        offer: SessionDescription,
    ) -> Result<SessionDescription, WebRtcError> {
        let offer = RTCSessionDescription::offer(offer.sdp)
            .map_err(|_| WebRtcError::InvalidSessionDescription)?;
        self.peer
            .set_remote_description(offer)
            .await
            .map_err(|_| WebRtcError::InvalidSessionDescription)?;
        let answer = self
            .peer
            .create_answer(None)
            .await
            .map_err(|_| WebRtcError::PeerOperationFailed)?;
        self.peer
            .set_local_description(answer)
            .await
            .map_err(|_| WebRtcError::PeerOperationFailed)?;
        local_description(&self.peer).await
    }

    /// Installs the answer to a previously emitted local offer.
    ///
    /// # Errors
    ///
    /// Rejects malformed or inapplicable SDP.
    pub async fn accept_answer(&self, answer: SessionDescription) -> Result<(), WebRtcError> {
        let answer = RTCSessionDescription::answer(answer.sdp)
            .map_err(|_| WebRtcError::InvalidSessionDescription)?;
        self.peer
            .set_remote_description(answer)
            .await
            .map_err(|_| WebRtcError::InvalidSessionDescription)
    }

    /// Adds one trickled remote ICE candidate.
    ///
    /// # Errors
    ///
    /// Rejects malformed or inapplicable candidates.
    pub async fn add_ice_candidate(&self, candidate: IceCandidate) -> Result<(), WebRtcError> {
        self.peer
            .add_ice_candidate(RTCIceCandidateInit {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_mline_index: candidate.sdp_mline_index,
                username_fragment: candidate.username_fragment,
            })
            .await
            .map_err(|_| WebRtcError::InvalidIceCandidate)
    }

    /// Receives the next bounded peer event.
    pub async fn recv(&mut self) -> Option<WebRtcPeerEvent> {
        self.events.recv().await
    }

    /// Sends an ordered control/input acknowledgement message.
    ///
    /// # Errors
    ///
    /// Returns fixed serialization, channel, or backpressure failures.
    pub async fn send_control(&self, message: &ServerMessage) -> Result<(), WebRtcError> {
        send_json(&self.control, message, self.max_buffered_amount).await
    }

    /// Sends one ordered log on the physically separate low-priority channel.
    ///
    /// # Errors
    ///
    /// Returns fixed serialization, channel, or backpressure failures.
    pub async fn send_log(&self, log: &LogData) -> Result<(), WebRtcError> {
        send_json(
            &self.log,
            &ServerMessage::Log(MessageData { data: log.clone() }),
            self.max_buffered_amount,
        )
        .await
    }

    /// Closes SCTP, ICE, DTLS, and media tasks for this attempt.
    pub async fn close(&self) {
        self.cancellation.cancel();
        let _result = self.peer.close().await;
    }
}

async fn create_peer_connection(
    config: &WebRtcPeerConfig,
) -> Result<RTCPeerConnection, WebRtcError> {
    let mut media_engine = MediaEngine::default();
    register_preferred_video_codecs(&mut media_engine)?;
    let registry = register_default_interceptors(Registry::new(), &mut media_engine)
        .map_err(|_| WebRtcError::PeerCreationFailed)?;
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();
    let ice_servers = if config.stun_server.is_empty() {
        Vec::new()
    } else {
        vec![RTCIceServer {
            urls: vec![config.stun_server.clone()],
            ..Default::default()
        }]
    };
    api.new_peer_connection(RTCConfiguration {
        ice_servers,
        ..Default::default()
    })
    .await
    .map_err(|_| WebRtcError::PeerCreationFailed)
}

fn register_preferred_video_codecs(media_engine: &mut MediaEngine) -> Result<(), WebRtcError> {
    let feedback = video_rtcp_feedback();
    for codec in [
        RTCRtpCodecParameters {
            capability: h264_capability(),
            payload_type: H264_PAYLOAD_TYPE,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_owned(),
                clock_rate: 90_000,
                rtcp_feedback: feedback.clone(),
                ..Default::default()
            },
            payload_type: VP8_PAYLOAD_TYPE,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP9.to_owned(),
                clock_rate: 90_000,
                sdp_fmtp_line: "profile-id=0".to_owned(),
                rtcp_feedback: feedback,
                ..Default::default()
            },
            payload_type: VP9_PAYLOAD_TYPE,
            ..Default::default()
        },
    ] {
        media_engine
            .register_codec(codec, RTPCodecType::Video)
            .map_err(|_| WebRtcError::PeerCreationFailed)?;
    }
    Ok(())
}

fn h264_capability() -> RTCRtpCodecCapability {
    RTCRtpCodecCapability {
        mime_type: MIME_TYPE_H264.to_owned(),
        clock_rate: 90_000,
        sdp_fmtp_line: H264_FMTP.to_owned(),
        rtcp_feedback: video_rtcp_feedback(),
        ..Default::default()
    }
}

fn video_rtcp_feedback() -> Vec<RTCPFeedback> {
    [
        ("goog-remb", ""),
        ("ccm", "fir"),
        ("nack", ""),
        ("nack", "pli"),
    ]
    .into_iter()
    .map(|(typ, parameter)| RTCPFeedback {
        typ: typ.to_owned(),
        parameter: parameter.to_owned(),
    })
    .collect()
}

fn install_peer_state_handler(
    peer: &RTCPeerConnection,
    events: mpsc::Sender<WebRtcPeerEvent>,
    cancellation: CancellationToken,
) {
    peer.on_peer_connection_state_change(Box::new(move |state| {
        let events = events.clone();
        let cancellation = cancellation.clone();
        Box::pin(async move {
            match state {
                RTCPeerConnectionState::Connected => {
                    publish_event(&events, &cancellation, WebRtcPeerEvent::VideoReady);
                }
                RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                    publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                }
                _ => {}
            }
        })
    }));
}

fn install_ice_handler(
    peer: &RTCPeerConnection,
    events: mpsc::Sender<WebRtcPeerEvent>,
    cancellation: CancellationToken,
) {
    peer.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let events = events.clone();
        let cancellation = cancellation.clone();
        Box::pin(async move {
            let Some(candidate) = candidate else {
                return;
            };
            let Ok(candidate) = candidate.to_json() else {
                publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                return;
            };
            publish_event(
                &events,
                &cancellation,
                WebRtcPeerEvent::IceCandidate(IceCandidate {
                    candidate: candidate.candidate,
                    sdp_mid: candidate.sdp_mid,
                    sdp_mline_index: candidate.sdp_mline_index,
                    username_fragment: candidate.username_fragment,
                }),
            );
        })
    }));
}

fn install_data_channel_handlers(
    channel: &RTCDataChannel,
    bit: u8,
    readiness: Arc<DataChannelReadiness>,
    events: mpsc::Sender<WebRtcPeerEvent>,
    cancellation: CancellationToken,
    max_control_message_bytes: Option<usize>,
) {
    let opened_events = events.clone();
    let opened_cancellation = cancellation.clone();
    channel.on_open(Box::new(move || {
        Box::pin(async move {
            if readiness.opened(bit) {
                publish_event(
                    &opened_events,
                    &opened_cancellation,
                    WebRtcPeerEvent::DataChannelsReady,
                );
            }
        })
    }));
    let closed_events = events.clone();
    let closed_cancellation = cancellation.clone();
    channel.on_close(Box::new(move || {
        let closed_events = closed_events.clone();
        let closed_cancellation = closed_cancellation.clone();
        Box::pin(async move {
            publish_event(
                &closed_events,
                &closed_cancellation,
                WebRtcPeerEvent::Failed,
            );
        })
    }));
    if let Some(max_bytes) = max_control_message_bytes {
        channel.on_message(Box::new(move |message: DataChannelMessage| {
            let events = events.clone();
            let cancellation = cancellation.clone();
            Box::pin(async move {
                if !message.is_string || message.data.len() > max_bytes {
                    publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                    return;
                }
                let Ok(message) = String::from_utf8(message.data.to_vec()) else {
                    publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                    return;
                };
                publish_event(
                    &events,
                    &cancellation,
                    WebRtcPeerEvent::ControlMessage(message),
                );
            })
        }));
    }
}

fn spawn_video_writer(
    track: Arc<TrackLocalStaticSample>,
    mut frames: broadcast::Receiver<Arc<EncodedH264Frame>>,
    events: mpsc::Sender<WebRtcPeerEvent>,
    cancellation: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_activity = None;
        loop {
            let frame = tokio::select! {
                () = cancellation.cancelled() => return,
                frame = frames.recv() => frame,
            };
            let frame = match frame {
                Ok(frame) => frame,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if track
                .write_sample(&Sample {
                    data: frame.bytes.clone(),
                    duration: frame.duration,
                    ..Default::default()
                })
                .await
                .is_err()
            {
                publish_event(&events, &cancellation, WebRtcPeerEvent::Failed);
                return;
            }
            let _sequence = frame.frame_sequence;
            publish_activity_if_due(&events, &cancellation, &mut last_activity);
        }
    })
}

fn spawn_rtcp_reader(
    sender: Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>,
    events: mpsc::Sender<WebRtcPeerEvent>,
    cancellation: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 1500];
        let mut last_activity = None;
        loop {
            let result = tokio::select! {
                () = cancellation.cancelled() => return,
                result = sender.read(&mut buffer) => result,
            };
            if result.is_err() {
                return;
            }
            publish_activity_if_due(&events, &cancellation, &mut last_activity);
        }
    })
}

fn publish_activity_if_due(
    events: &mpsc::Sender<WebRtcPeerEvent>,
    cancellation: &CancellationToken,
    last_activity: &mut Option<Instant>,
) {
    let now = Instant::now();
    if last_activity.is_none_or(|previous| now.duration_since(previous) >= ACTIVITY_EVENT_INTERVAL)
    {
        *last_activity = Some(now);
        publish_event(events, cancellation, WebRtcPeerEvent::MediaActivity);
    }
}

fn publish_event(
    events: &mpsc::Sender<WebRtcPeerEvent>,
    cancellation: &CancellationToken,
    event: WebRtcPeerEvent,
) {
    if events.try_send(event).is_err() {
        cancellation.cancel();
    }
}

async fn local_description(peer: &RTCPeerConnection) -> Result<SessionDescription, WebRtcError> {
    peer.local_description()
        .await
        .map(|description| SessionDescription {
            sdp: description.sdp,
        })
        .ok_or(WebRtcError::PeerOperationFailed)
}

async fn send_json(
    channel: &RTCDataChannel,
    message: &ServerMessage,
    max_buffered_amount: usize,
) -> Result<(), WebRtcError> {
    if channel.buffered_amount().await >= max_buffered_amount {
        return Err(WebRtcError::DataChannelBackpressure);
    }
    let encoded = serde_json::to_string(message).map_err(|_| WebRtcError::PeerOperationFailed)?;
    channel
        .send_text(encoded)
        .await
        .map(|_| ())
        .map_err(|_| WebRtcError::DataChannelUnavailable)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use openh264::decoder::Decoder;
    use openh264::formats::YUVSource as _;
    use pokecon_camera::{
        BgrFrame, CaptureResolution, LatestFrameSource, ScreenshotRuntimeSettings,
    };
    use tokio::sync::mpsc;
    use tokio::time::timeout;

    use crate::api::{LogLevel, LogTarget, Nonce};

    use super::*;

    async fn media(source: &LatestFrameSource) -> WebRtcMedia {
        WebRtcMedia::new(
            source.webrtc(),
            source.motion_jpeg(ScreenshotRuntimeSettings::default()),
            WebRtcMediaConfig::default(),
        )
        .await
        .expect("media pipeline")
    }

    struct LoopbackClient {
        peer: Arc<RTCPeerConnection>,
        channels: mpsc::Receiver<(String, Arc<RTCDataChannel>)>,
        messages: mpsc::Receiver<(String, String)>,
        video: mpsc::Receiver<usize>,
    }

    async fn loopback_client() -> LoopbackClient {
        let peer = Arc::new(
            create_peer_connection(&WebRtcPeerConfig::default())
                .await
                .expect("client peer"),
        );
        let (channels, channel_receiver) = mpsc::channel(2);
        let (messages, message_receiver) = mpsc::channel(4);
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
                        .expect("server messages must be UTF-8 JSON");
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
        LoopbackClient {
            peer,
            channels: channel_receiver,
            messages: message_receiver,
            video: video_receiver,
        }
    }

    async fn signal_loopback(server: &WebRtcPeer, client: &RTCPeerConnection) {
        let mut offer_gathered = server.peer.gathering_complete_promise().await;
        server.create_offer().await.expect("local offer");
        timeout(Duration::from_secs(5), offer_gathered.recv())
            .await
            .expect("server ICE gathering deadline");
        let offer = server
            .peer
            .local_description()
            .await
            .expect("gathered offer");
        client
            .set_remote_description(offer)
            .await
            .expect("client accepts offer");

        let answer = client.create_answer(None).await.expect("client answer");
        let mut answer_gathered = client.gathering_complete_promise().await;
        client
            .set_local_description(answer)
            .await
            .expect("client local answer");
        timeout(Duration::from_secs(5), answer_gathered.recv())
            .await
            .expect("client ICE gathering deadline");
        let answer = client.local_description().await.expect("gathered answer");
        server
            .accept_answer(SessionDescription { sdp: answer.sdp })
            .await
            .expect("server accepts answer");
    }

    async fn wait_for_loopback_readiness(server: &mut WebRtcPeer) {
        timeout(Duration::from_secs(10), async {
            let mut video_ready = false;
            let mut data_ready = false;
            while !video_ready || !data_ready {
                match server.recv().await.expect("server event stream") {
                    WebRtcPeerEvent::VideoReady => video_ready = true,
                    WebRtcPeerEvent::DataChannelsReady => data_ready = true,
                    WebRtcPeerEvent::Failed => panic!("loopback peer failed"),
                    WebRtcPeerEvent::IceCandidate(_)
                    | WebRtcPeerEvent::ControlMessage(_)
                    | WebRtcPeerEvent::MediaActivity => {}
                }
            }
        })
        .await
        .expect("loopback readiness deadline");
    }

    async fn remote_channels(
        receiver: &mut mpsc::Receiver<(String, Arc<RTCDataChannel>)>,
    ) -> BTreeMap<String, Arc<RTCDataChannel>> {
        let mut remote_channels = BTreeMap::new();
        timeout(Duration::from_secs(5), async {
            while remote_channels.len() < 2 {
                let (label, channel) = receiver.recv().await.expect("remote DataChannel");
                remote_channels.insert(label, channel);
            }
        })
        .await
        .expect("remote DataChannel deadline");
        remote_channels
    }

    async fn wait_for_control_message(server: &mut WebRtcPeer, expected: &str) {
        timeout(Duration::from_secs(5), async {
            loop {
                match server.recv().await.expect("server control event") {
                    WebRtcPeerEvent::ControlMessage(message) => {
                        assert_eq!(message, expected);
                        break;
                    }
                    WebRtcPeerEvent::Failed => panic!("loopback peer failed"),
                    WebRtcPeerEvent::IceCandidate(_)
                    | WebRtcPeerEvent::VideoReady
                    | WebRtcPeerEvent::DataChannelsReady
                    | WebRtcPeerEvent::MediaActivity => {}
                }
            }
        })
        .await
        .expect("control receive deadline");
    }

    async fn assert_isolated_server_messages(
        server: &WebRtcPeer,
        receiver: &mut mpsc::Receiver<(String, String)>,
    ) {
        server
            .send_control(&ServerMessage::Ping(MessageData {
                data: Nonce {
                    nonce: "control-loopback".to_owned(),
                },
            }))
            .await
            .expect("server control send");
        server
            .send_log(&LogData {
                level: LogLevel::Info,
                message: "log-loopback".to_owned(),
                target: LogTarget::Log,
            })
            .await
            .expect("server log send");

        let mut received_labels = Vec::new();
        timeout(Duration::from_secs(5), async {
            while received_labels.len() < 2 {
                let (label, message) = receiver.recv().await.expect("client message");
                let json: serde_json::Value =
                    serde_json::from_str(&message).expect("valid server JSON");
                let expected_type = if label == CONTROL_DATA_CHANNEL {
                    "ping"
                } else {
                    "log"
                };
                assert_eq!(json["type"], expected_type);
                received_labels.push(label);
            }
        })
        .await
        .expect("server message deadline");
        received_labels.sort_unstable();
        assert_eq!(received_labels, [CONTROL_DATA_CHANNEL, LOG_DATA_CHANNEL]);
    }

    #[test]
    fn zero_media_and_peer_bounds_are_rejected() {
        assert_eq!(
            WebRtcMediaConfig::new(0, 1, 1, 1),
            Err(WebRtcError::InvalidConfig)
        );
        assert_eq!(
            WebRtcPeerConfig::new("", 1, 0, 1),
            Err(WebRtcError::InvalidConfig)
        );
    }

    #[tokio::test]
    async fn shared_encoder_emits_decodable_annex_b_h264() {
        let source = LatestFrameSource::new();
        let media = media(&source).await;
        let mut h264 = media.subscribe_h264();
        source.publish(
            17,
            BgrFrame::solid(CaptureResolution::R640x360, [10, 20, 30]),
        );
        let encoded = timeout(Duration::from_secs(5), h264.recv())
            .await
            .expect("encode deadline")
            .expect("encoded frame");
        assert_eq!(encoded.frame_sequence, 17);
        assert!(encoded.bytes.starts_with(&[0, 0, 0, 1]));

        let mut decoder = Decoder::new().expect("decoder");
        let decoded_frame = decoder
            .decode(&encoded.bytes)
            .expect("valid H.264")
            .expect("decoded frame");
        assert_eq!(decoded_frame.dimensions(), (640, 360));
    }

    #[tokio::test]
    async fn local_offer_prioritizes_h264_and_uses_distinct_reliable_channels() {
        let source = LatestFrameSource::new();
        let media = media(&source).await;
        let peer = WebRtcPeer::new(4, &media, WebRtcPeerConfig::default())
            .await
            .expect("peer");
        let offer = peer.create_offer().await.expect("offer");
        let video = offer
            .sdp
            .lines()
            .find(|line| line.starts_with("m=video "))
            .expect("video media line");
        let h264 = video.find(" 102").expect("H.264 payload");
        let vp8 = video.find(" 96").expect("VP8 payload");
        let vp9 = video.find(" 98").expect("VP9 payload");
        assert!(h264 < vp8 && vp8 < vp9, "unexpected codec order: {video}");
        assert!(peer.control.ordered());
        assert!(peer.log.ordered());
        assert_eq!(peer.control.max_retransmits(), None);
        assert_eq!(peer.log.max_retransmits(), None);
        assert_ne!(peer.control.label(), peer.log.label());
        peer.close().await;
    }

    #[tokio::test]
    async fn loopback_transports_h264_and_isolated_data_channels() {
        let source = LatestFrameSource::new();
        let media = media(&source).await;
        let mut server = WebRtcPeer::new(9, &media, WebRtcPeerConfig::default())
            .await
            .expect("server peer");
        let mut client = loopback_client().await;

        signal_loopback(&server, &client.peer).await;
        wait_for_loopback_readiness(&mut server).await;
        let remote_channels = remote_channels(&mut client.channels).await;
        assert_eq!(
            remote_channels
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [CONTROL_DATA_CHANNEL, LOG_DATA_CHANNEL]
        );

        let input = r#"{"type":"pong","data":{"nonce":"loopback"}}"#;
        remote_channels[CONTROL_DATA_CHANNEL]
            .send_text(input.to_owned())
            .await
            .expect("client control send");
        wait_for_control_message(&mut server, input).await;
        assert_isolated_server_messages(&server, &mut client.messages).await;

        source.publish(
            23,
            BgrFrame::solid(CaptureResolution::R640x360, [40, 50, 60]),
        );
        let payload_bytes = timeout(Duration::from_secs(5), client.video.recv())
            .await
            .expect("RTP receive deadline")
            .expect("RTP payload");
        assert_ne!(payload_bytes, 0);

        server.close().await;
        client.peer.close().await.expect("client close");
    }
}
