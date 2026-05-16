//! WebRTC video streaming backend using [str0m] with H.264/HEVC encoding
//! via VAAPI hardware acceleration.
//!
//! Provides a complete WebRTC implementation for streaming camera video
//! frames to Web frontends.
//!
//! # Architecture
//!
//! [`WebRtcManager`] is the top-level session manager stored in
//! shared application state (`Arc<Mutex<webrtc::WebRtcManager>>`).
//! Each client connection gets a [`WebRtcSession`] with its own
//! [`str0m::Rtc`] instance for SDP/ICE negotiation.
//!
//! The encoder type is selected per-session based on feature flags and
//! hardware availability:
//! - **H.264/HEVC** (hardware) — VAAPI-accelerated via FFmpeg (feature `vaapi`)

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use str0m::change::SdpOffer;
use str0m::format::Codec;
use str0m::media::{MediaKind, Mid};
use str0m::net::Protocol;
use str0m::{Candidate, Rtc};
use tokio::sync::Mutex;

#[cfg(feature = "vaapi")]
pub use crate::vaapi_encoder::VaapiConfig;

// ── VideoEncoder trait ──────────────────────────────────────────────────

/// A video encoder that accepts raw RGB frames and produces encoded bitstream
/// suitable for RTP packetization via str0m.
///
/// The encoder output must be in **Annex B byte-stream format** (NAL units
/// separated by `00 00 00 01` or `00 00 01` start codes).  str0m's internal
/// H.264/H.265 packetizers parse Annex B natively: they split on start codes,
/// strip AUD/filler NALUs, cache SPS/PPS for STAP-A/AP emission, and
/// fragment large NALUs into FU-A/FU packets.
pub trait VideoEncoder: Send + 'static {
    /// Encode a raw RGB24 frame (width × height × 3 bytes) into encoded
    /// bitstream data (H.264/HEVC NAL units in Annex B format).
    fn encode_rgb(&mut self, rgb: &[u8]) -> Result<Vec<u8>, String>;

    /// Returns the codec name for SDP/media negotiation (e.g. `"H264"`, `"H265"`).
    fn codec_name(&self) -> &'static str;

    /// Returns the configured video width in pixels.
    fn width(&self) -> u32;

    /// Returns the configured video height in pixels.
    fn height(&self) -> u32;

    /// Returns the configured frame rate in fps.
    fn framerate(&self) -> u32;
}

// ── Session identifier ─────────────────────────────────────────────────

/// Unique identifier for a single WebRTC peer connection session.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SessionId(u64);

impl SessionId {
    /// Allocate a new globally-unique session ID.
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Parse a session ID from its string representation (as produced by [`Display`]).
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse::<u64>().ok().map(Self)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── WebRTC session ─────────────────────────────────────────────────────

/// Error type for RTP send task operations.
#[derive(Debug)]
#[allow(dead_code)]
pub enum RtpSendError {
    EncoderError(String),
    SessionNotConnected,
    WriterNotFound(Mid),
    MediaNotAdded,
}

impl std::fmt::Display for RtpSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtpSendError::EncoderError(e) => write!(f, "Encoder error: {}", e),
            RtpSendError::SessionNotConnected => write!(f, "Session not connected"),
            RtpSendError::WriterNotFound(mid) => write!(f, "Writer not found for mid {:?}", mid),
            RtpSendError::MediaNotAdded => write!(f, "Media not yet added"),
        }
    }
}

impl std::error::Error for RtpSendError {}

/// A single WebRTC peer connection backed by [`str0m::Rtc`].
///
/// Each session maintains its own ICE/DTLS state machine and can
/// accept one SDP offer to produce a sendonly video answer.
#[allow(dead_code)]
pub struct WebRtcSession {
    /// The underlying str0m peer connection.
    rtc: Rtc,
    /// Wall-clock creation time (for inactivity timeouts).
    created_at: Instant,
    /// Unique session identifier.
    session_id: SessionId,
    /// Video encoder for this session (VAAPI H.264/HEVC).
    encoder: Option<Box<dyn VideoEncoder>>,
    /// The media ID for the video track (set after SDP negotiation).
    video_mid: Option<Mid>,
    /// Whether ICE+DTLS has fully connected.
    connected: bool,
    /// RTP timestamp base (90 kHz clock).
    rtp_timestamp: u32,
}

#[allow(dead_code)]
impl WebRtcSession {
    /// Create a new WebRTC session with H.264/HEVC codec configuration.
    ///
    /// VP8/VP9/AV1 are disabled since we only use H.264/HEVC hardware encoding.
    pub fn new() -> Self {
        let config = str0m::RtcConfig::new()
            .clear_codecs()
            .enable_h264(true)
            .enable_h265(true);

        let mut rtc = config.build(Instant::now());

        // Configure for sendonly video.
        use str0m::media::Direction;
        let mut change = rtc.sdp_api();
        let mid = change.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        let _ = change.apply();

        Self {
            rtc,
            created_at: Instant::now(),
            session_id: SessionId::new(),
            encoder: None,
            video_mid: Some(mid),
            connected: false,
            rtp_timestamp: 0,
        }
    }

    /// Return this session's unique identifier.
    pub fn id(&self) -> SessionId {
        self.session_id
    }

    /// Return when this session was created (used for staleness checks).
    #[allow(dead_code)]
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Check whether the ICE connection is established.
    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Check if this session has an encoder configured.
    #[allow(dead_code)]
    pub fn has_encoder(&self) -> bool {
        self.encoder.is_some()
    }

    /// Initialize the video encoder for this session.
    pub fn init_encoder(&mut self, encoder: Box<dyn VideoEncoder>) {
        self.encoder = Some(encoder);
    }

    /// Accept an SDP offer and produce a sendonly video answer.
    ///
    /// Adds a local host ICE candidate, parses the offer via str0m,
    /// and returns the SDP answer string on success.
    pub fn accept_offer(&mut self, offer_sdp: &str) -> Result<String, String> {
        // Register a dummy local host candidate (the actual media flows
        // over the frontend's peer connection; this satisfies str0m's
        // requirement for at least one local candidate).
        let addr = "127.0.0.1:9"
            .parse()
            .map_err(|e| format!("Failed to parse local ICE address: {}", e))?;
        let candidate = Candidate::host(addr, Protocol::Udp)
            .map_err(|e| format!("Failed to create host ICE candidate: {}", e))?;
        self.rtc.add_local_candidate(candidate);

        // Parse the remote SDP offer.
        let offer = SdpOffer::from_sdp_string(offer_sdp)
            .map_err(|e| format!("Failed to parse SDP offer: {}", e))?;

        // Generate the answer via str0m's SDP API.
        let api = self.rtc.sdp_api();
        let answer = api
            .accept_offer(offer)
            .map_err(|e| format!("Failed to accept SDP offer: {}", e))?;

        Ok(answer.to_sdp_string())
    }

    /// Add a remote ICE candidate to this session.
    pub fn add_remote_candidate(&mut self, candidate: Candidate) -> Result<(), String> {
        self.rtc.add_remote_candidate(candidate);
        Ok(())
    }

    /// Poll for output events (ICE connectivity checks, timeouts, media).
    ///
    /// Should be called periodically (e.g., every 50 ms) when the session
    /// is actively being used for media streaming.
    pub fn poll_output(&mut self) -> Result<str0m::Output, str0m::error::RtcError> {
        let output = self.rtc.poll_output()?;

        // Track ICE connection state via events.
        if let str0m::Output::Event(ref event) = output {
            if let str0m::Event::IceConnectionStateChange(state) = event {
                self.connected = *state == str0m::IceConnectionState::Connected;
                tracing::debug!(
                    "Session {} ICE state: {:?} (connected={})",
                    self.session_id,
                    state,
                    self.connected
                );
            }
            if let str0m::Event::Connected = event {
                self.connected = true;
                tracing::debug!("Session {} ICE+DTLS connected", self.session_id);
            }
            if let str0m::Event::MediaAdded(added) = event {
                self.video_mid = Some(added.mid);
                tracing::debug!(
                    "Session {} media added: mid={:?} kind={:?}",
                    self.session_id,
                    added.mid,
                    added.kind,
                );
            }
        }

        Ok(output)
    }

    /// Handle a timeout input (from a previous Output::Timeout).
    pub fn handle_timeout(&mut self, instant: Instant) -> Result<(), str0m::error::RtcError> {
        self.rtc.handle_input(str0m::Input::Timeout(instant))
    }

    /// Get the media ID for the video track (if negotiated).
    #[allow(dead_code)]
    pub fn video_mid(&self) -> Option<Mid> {
        self.video_mid
    }

    /// Get a mutable reference to the video encoder (if initialized).
    pub fn encoder_mut(&mut self) -> Option<&mut dyn VideoEncoder> {
        self.encoder.as_deref_mut()
    }

    /// Get a reference to the inner str0m Rtc instance.
    #[allow(dead_code)]
    pub fn rtc(&self) -> &Rtc {
        &self.rtc
    }

    /// Get a mutable reference to the inner str0m Rtc instance.
    #[allow(dead_code)]
    pub fn rtc_mut(&mut self) -> &mut Rtc {
        &mut self.rtc
    }

    /// Write an encoded video frame to str0m's media channel.
    ///
    /// This packetizes the frame as RTP (using the given [`Codec`]) and
    /// makes it available via [`poll_output()`](Self::poll_output) as
    /// Transmit events.
    pub fn write_video_frame(
        &mut self,
        encoded_data: &[u8],
        codec: Codec,
        framerate: u32,
    ) -> Result<(), RtpSendError> {
        let mid = self.video_mid.ok_or(RtpSendError::MediaNotAdded)?;

        if !self.connected {
            return Err(RtpSendError::SessionNotConnected);
        }

        let Some(writer) = self.rtc.writer(mid) else {
            return Err(RtpSendError::WriterNotFound(mid));
        };

        // Find the payload type for the given codec.
        let pt = writer
            .payload_params()
            .find(|p| p.spec().codec == codec)
            .map(|p| p.pt())
            .ok_or(RtpSendError::WriterNotFound(mid))?;

        let wallclock = Instant::now();
        let rtp_time = str0m::media::MediaTime::new(
            self.rtp_timestamp as u64,
            str0m::media::Frequency::NINETY_KHZ,
        );

        // Write the frame to str0m; it handles RTP packetization.
        writer
            .write(pt, wallclock, rtp_time, encoded_data.to_vec())
            .map_err(|e| RtpSendError::EncoderError(format!("str0m write error: {:?}", e)))?;

        // Advance RTP timestamp based on encoder's configured framerate.
        // 90kHz clock / framerate = timestamp increment per frame.
        let increment = 90_000u32.checked_div(framerate).unwrap_or(3_000);
        self.rtp_timestamp = self.rtp_timestamp.wrapping_add(increment);

        Ok(())
    }
}

impl Default for WebRtcSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── WebRTC manager ─────────────────────────────────────────────────────

/// Top-level manager for all active WebRTC video sessions.
///
/// This is the struct stored in shared application state via
/// `Arc<Mutex<webrtc::WebRtcManager>>`.  It owns a collection of
/// [`WebRtcSession`]s keyed by [`SessionId`].
#[allow(dead_code)]
pub struct WebRtcManager {
    sessions: HashMap<SessionId, WebRtcSession>,
    /// Monotonically-increasing RTP sequence number counter.
    next_sequence: u64,
}

#[allow(dead_code)]
impl WebRtcManager {
    /// Create a new empty WebRTC session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_sequence: 0,
        }
    }

    /// Open a new WebRTC session (without an encoder) and return its [`SessionId`].
    ///
    /// The caller may later call [`init_encoder`](WebRtcSession::init_encoder)
    /// on the session if encoding is needed.
    #[allow(dead_code)]
    pub fn create_session(&mut self) -> SessionId {
        let session = WebRtcSession::new();
        let id = session.id();
        self.sessions.insert(id, session);
        id
    }

    /// Open a new WebRTC session with the given [`VideoEncoder`].
    ///
    /// The caller creates the encoder (e.g. [`VaapiEncoder`]) and passes
    /// it in as a boxed trait object.
    pub fn create_session_with_encoder(&mut self, encoder: Box<dyn VideoEncoder>) -> SessionId {
        let mut session = WebRtcSession::new();
        let id = session.id();
        session.init_encoder(encoder);
        self.sessions.insert(id, session);
        id
    }

    /// Remove a session by ID, dropping the underlying [`str0m::Rtc`] and encoder.
    pub fn close_session(&mut self, id: SessionId) {
        self.sessions.remove(&id);
    }

    /// Obtain a mutable reference to a session (e.g., for SDP negotiation).
    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut WebRtcSession> {
        self.sessions.get_mut(&id)
    }

    /// Obtain an immutable reference to a session.
    #[allow(dead_code)]
    pub fn get(&self, id: SessionId) -> Option<&WebRtcSession> {
        self.sessions.get(&id)
    }

    /// Iterate over all active session IDs.
    pub fn session_ids(&self) -> impl Iterator<Item = &SessionId> {
        self.sessions.keys()
    }

    /// Number of currently-active sessions.
    #[allow(dead_code)]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Remove sessions that have lived longer than `timeout`.
    ///
    /// Call this from a periodic background task to prevent resource leaks.
    #[allow(dead_code)]
    pub fn cleanup_stale_sessions(&mut self, timeout: std::time::Duration) {
        let now = Instant::now();
        self.sessions
            .retain(|_, session| now.duration_since(session.created_at()) < timeout);
    }

    /// Allocate the next RTP sequence number (wraps at u16::MAX).
    #[allow(dead_code)]
    pub fn next_sequence(&mut self) -> u16 {
        let seq = self.next_sequence as u16;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        seq
    }

    /// Accept an SDP offer for a given session, returning the answer string.
    ///
    /// This is a convenience wrapper around
    /// [`WebRtcSession::accept_offer`].
    pub fn accept_offer(&mut self, id: SessionId, offer: &str) -> Result<String, String> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("Session {} not found", id))?;
        session.accept_offer(offer)
    }

    /// Add a remote ICE candidate for a given session.
    pub fn add_ice_candidate(&mut self, id: SessionId, candidate_sdp: &str) -> Result<(), String> {
        let candidate = Candidate::from_sdp_string(candidate_sdp)
            .map_err(|e| format!("Failed to parse ICE candidate: {}", e))?;
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("Session {} not found", id))?;
        session.add_remote_candidate(candidate)
    }

    /// Poll a session for str0m output events.
    #[allow(dead_code)]
    pub fn poll_session(&mut self, id: SessionId) -> Result<str0m::Output, String> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("Session {} not found", id))?;
        session
            .poll_output()
            .map_err(|e| format!("poll_output error: {:?}", e))
    }

    /// Encode and send a camera frame as H.264/HEVC RTP through a session.
    ///
    /// This is the main pipeline entry point: RGB frame → encode →
    /// str0m write → RTP output.
    ///
    /// The encoder output is expected to be in Annex B byte-stream format
    /// (NAL units with `00 00 00 01` start codes).  str0m's packetizers
    /// parse Annex B natively and handle SPS/PPS caching, AUD/filler
    /// stripping, and FU fragmentation automatically.
    ///
    /// # Panics / Errors
    ///
    /// Returns [`RtpSendError::EncoderError`] if `width * height * 3` does
    /// not match `rgb_data.len()`, or if `width`/`height` differ from the
    /// encoder's configured dimensions.
    pub fn send_camera_frame(
        &mut self,
        id: SessionId,
        rgb_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), RtpSendError> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| RtpSendError::EncoderError("Session not found".into()))?;

        let encoder = session
            .encoder_mut()
            .ok_or_else(|| RtpSendError::EncoderError("No encoder initialized".into()))?;

        let expected_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(3))
            .ok_or_else(|| RtpSendError::EncoderError("width/height overflow".into()))?;

        if rgb_data.len() != expected_size {
            return Err(RtpSendError::EncoderError(format!(
                "RGB buffer size mismatch: expected {} bytes ({}x{}x3), got {}",
                expected_size,
                width,
                height,
                rgb_data.len()
            )));
        }

        if width != encoder.width() || height != encoder.height() {
            return Err(RtpSendError::EncoderError(format!(
                "Frame dimensions {}x{} don't match encoder dimensions {}x{}",
                width,
                height,
                encoder.width(),
                encoder.height()
            )));
        }

        // Encode RGB frame to H.264/HEVC (Annex B byte-stream).
        let encoded_data = encoder
            .encode_rgb(rgb_data)
            .map_err(RtpSendError::EncoderError)?;

        // Determine the str0m Codec from the encoder's codec name.
        let codec = match encoder.codec_name() {
            "H264" => Codec::H264,
            "H265" => Codec::H265,
            other => {
                return Err(RtpSendError::EncoderError(format!(
                    "Unknown codec: {}",
                    other
                )));
            }
        };

        // Write encoded frame through str0m's media channel.
        let framerate = encoder.framerate();
        session.write_video_frame(&encoded_data, codec, framerate)
    }
}

impl Default for WebRtcManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════════════
// RTP Send Task
// ═════════════════════════════════════════════════════════════════════════

/// A background RTP stream sender that wraps a WebRTC session and
/// handles poll_output events (ICE timeouts, transmits, etc.).
///
/// This is spawned as a [`tokio::task`] when a new WebRTC session is
/// established. It periodically:
/// 1. Polls str0m for output events (transmits, timeouts, media events)
/// 2. Handles timeout events by feeding them back
/// 3. Reports transmit events back for network I/O
#[allow(dead_code)]
pub struct RtpSendTask {
    /// The session ID this task is associated with.
    session_id: SessionId,
    /// Channel to receive camera frame data.
    frame_rx: tokio::sync::mpsc::Receiver<CameraFrame>,
    /// Channel to transmit str0m output data (ICE, RTP packets) to network.
    output_tx: tokio::sync::mpsc::Sender<RtpOutput>,
}

/// A camera frame received from the capture pipeline.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CameraFrame {
    /// Raw RGB pixel data (width*height*3 bytes).
    pub rgb: Vec<u8>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Monotonically increasing frame number.
    pub frame_number: u64,
}

/// Output from the RTP pipeline to be sent over the network.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum RtpOutput {
    /// Data to transmit over UDP/network (ICE connectivity checks, RTP, etc.).
    Transmit(Vec<u8>),
    /// The session has connected.
    Connected,
    /// The session has disconnected.
    Disconnected,
}

#[allow(dead_code)]
impl RtpSendTask {
    /// Create a new RTP send task.
    pub fn new(
        session_id: SessionId,
        frame_rx: tokio::sync::mpsc::Receiver<CameraFrame>,
        output_tx: tokio::sync::mpsc::Sender<RtpOutput>,
    ) -> Self {
        Self {
            session_id,
            frame_rx,
            output_tx,
        }
    }

    /// Run the RTP send loop. This blocks indefinitely.
    ///
    /// This function should be spawned in a tokio task:
    ///
    /// ```ignore
    /// tokio::spawn(async move { task.run(manager).await; });
    /// ```
    pub async fn run(self, manager: Arc<Mutex<WebRtcManager>>) {
        let Self {
            session_id,
            mut frame_rx,
            output_tx,
        } = self;

        tracing::info!("RTP send task started for session {}", session_id);

        loop {
            // How long to wait before the next str0m timeout.
            let timeout_dur = {
                let mut mgr = manager.lock().await;
                let session = match mgr.get_mut(session_id) {
                    Some(s) => s,
                    None => {
                        tracing::warn!("Session {} gone, stopping RTP task", session_id);
                        break;
                    }
                };

                // Poll str0m for output events.
                match session.poll_output() {
                    Ok(str0m::Output::Timeout(at)) => {
                        // Need to feed this timeout back at the right time.
                        if at <= Instant::now() {
                            // Already expired, handle immediately.
                            let _ = session.handle_timeout(at);
                            std::time::Duration::ZERO
                        } else {
                            at.saturating_duration_since(Instant::now())
                        }
                    }
                    Ok(str0m::Output::Transmit(transmit)) => {
                        // Send the transmit data over the output channel.
                        let data = transmit.contents.to_vec();
                        let _ = output_tx.send(RtpOutput::Transmit(data)).await;
                        // Re-poll immediately.
                        std::time::Duration::ZERO
                    }
                    Ok(str0m::Output::Event(event)) => {
                        match event {
                            str0m::Event::Connected => {
                                let _ = output_tx.send(RtpOutput::Connected).await;
                                tracing::info!("Session {} ICE+DTLS connected", session_id);
                            }
                            str0m::Event::IceConnectionStateChange(
                                str0m::IceConnectionState::Disconnected,
                            ) => {
                                let _ = output_tx.send(RtpOutput::Disconnected).await;
                                tracing::info!("Session {} disconnected", session_id);
                            }
                            _ => {}
                        }
                        std::time::Duration::ZERO
                    }
                    Err(e) => {
                        tracing::error!("Session {} poll_output error: {:?}", session_id, e);
                        std::time::Duration::from_millis(50)
                    }
                }
            };

            // Wait for either a new frame or a timeout.
            tokio::select! {
                maybe_frame = frame_rx.recv() => {
                    match maybe_frame {
                        Some(frame) => {
                            // Encode and send the frame.
                            let mut mgr = manager.lock().await;
                            if let Err(e) = mgr.send_camera_frame(
                                session_id,
                                &frame.rgb,
                                frame.width,
                                frame.height,
                            ) {
                                tracing::warn!(
                                    "Failed to send frame {} for session {}: {:?}",
                                    frame.frame_number, session_id, e,
                                );
                            }
                        }
                        None => {
                            // Channel closed, session is done.
                            tracing::info!(
                                "Frame channel closed for session {}, stopping",
                                session_id,
                            );
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(timeout_dur) => {
                    // Timer expired, just loop to poll again.
                }
            }
        }

        // Clean up session.
        let mut mgr = manager.lock().await;
        mgr.close_session(session_id);
        let _ = output_tx.send(RtpOutput::Disconnected).await;
        tracing::info!("RTP send task stopped for session {}", session_id);
    }
}

// ═════════════════════════════════════════════════════════════════════════
// SDP Helpers
// ═════════════════════════════════════════════════════════════════════════

/// Parse an SDP offer and produce a `sendonly` answer.
///
/// Uses str0m if parsing succeeds; otherwise falls back to a manually-
/// constructed minimal answer.  The answer advertises H.264 / payload type 96
/// (or whatever PT the offer specifies).
#[allow(dead_code)]
pub fn create_video_answer_sdp(offer_sdp: &str) -> String {
    let mut session = WebRtcSession::new();
    match session.accept_offer(offer_sdp) {
        Ok(answer) => answer,
        Err(_) => generate_basic_video_answer(offer_sdp),
    }
}

/// Fallback: construct a minimal sendonly H.264 SDP answer string manually.
///
/// Used when str0m's parser cannot handle the offer (e.g., unusual
/// formatting, missing attributes).
pub fn generate_basic_video_answer(offer_sdp: &str) -> String {
    // Extract the dynamic payload type from the offer's media line.
    let payload_type: u16 = offer_sdp
        .lines()
        .find(|l| l.starts_with("m=video"))
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|pt| pt.parse().ok())
        .unwrap_or(96);

    // Preserve the codec rtpmap from the offer, or default to H.264.
    let codec_rtpmap = offer_sdp
        .lines()
        .find(|l| l.starts_with(&format!("a=rtpmap:{}", payload_type)))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("a=rtpmap:{} H264/90000", payload_type));

    format!(
        concat!(
            "v=0\r\n",
            "o=- 0 0 IN IP4 127.0.0.1\r\n",
            "s=-\r\n",
            "t=0 0\r\n",
            "m=video 9 UDP/TLS/RTP/SAVPF {}\r\n",
            "c=IN IP4 127.0.0.1\r\n",
            "a=sendonly\r\n",
            "a=mid:0\r\n",
            "a=setup:passive\r\n",
            "a=ice-ufrag:backend\r\n",
            "a=ice-pwd:backendpwd\r\n",
            "a=fingerprint:sha-256 ",
            "00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:",
            "00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00\r\n",
            "a=rtcp-mux\r\n",
            "{}\r\n"
        ),
        payload_type, codec_rtpmap
    )
}

// ═════════════════════════════════════════════════════════════════════════
// ICE Candidate Helpers
// ═════════════════════════════════════════════════════════════════════════

/// Parse a string-encoded ICE candidate (from a `candidate:` SDP attribute)
/// and return a [`str0m::Candidate`].
///
/// This is a convenience wrapper around [`Candidate::from_sdp_string`].
#[allow(dead_code)]
pub fn parse_ice_candidate(sdp_line: &str) -> Result<Candidate, String> {
    Candidate::from_sdp_string(sdp_line)
        .map_err(|e| format!("Failed to parse ICE candidate: {}", e))
}

/// Create a local host ICE candidate for the given UDP address.
#[allow(dead_code)]
pub fn create_host_candidate(addr: &str) -> Result<Candidate, String> {
    let socket_addr = addr
        .parse()
        .map_err(|e| format!("Invalid socket address '{}': {}", addr, e))?;
    Candidate::host(socket_addr, Protocol::Udp)
        .map_err(|e| format!("Failed to create host candidate: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── WebRtcManager ────────────────────────────────────────────────

    #[test]
    fn test_manager_empty_on_new() {
        let manager = WebRtcManager::new();
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_create_and_close_session() {
        let mut manager = WebRtcManager::new();
        let id = manager.create_session();
        assert_eq!(manager.session_count(), 1);
        assert!(manager.get_mut(id).is_some());

        manager.close_session(id);
        assert_eq!(manager.session_count(), 0);
        assert!(manager.get_mut(id).is_none());
    }

    #[test]
    fn test_multiple_sessions() {
        let mut manager = WebRtcManager::new();
        let a = manager.create_session();
        let b = manager.create_session();
        let c = manager.create_session();
        assert_eq!(manager.session_count(), 3);

        manager.close_session(b);
        assert_eq!(manager.session_count(), 2);
        assert!(manager.get_mut(a).is_some());
        assert!(manager.get_mut(c).is_some());
    }

    #[test]
    fn test_sequence_numbers_monotonic() {
        let mut manager = WebRtcManager::new();
        let s1 = manager.next_sequence();
        let s2 = manager.next_sequence();
        let s3 = manager.next_sequence();
        assert_eq!(s2, s1.wrapping_add(1));
        assert_eq!(s3, s2.wrapping_add(1));
    }

    #[test]
    fn test_cleanup_stale_sessions() {
        let mut manager = WebRtcManager::new();
        manager.create_session();
        manager.create_session();
        assert_eq!(manager.session_count(), 2);

        // Zero-length timeout should remove all sessions.
        manager.cleanup_stale_sessions(std::time::Duration::ZERO);
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_close_nonexistent_session_is_noop() {
        let mut manager = WebRtcManager::new();
        let id = manager.create_session();
        manager.close_session(id); // ok
        manager.close_session(id); // no-op, already removed
        assert_eq!(manager.session_count(), 0);
    }

    // ── SessionId uniqueness ──────────────────────────────────────────

    #[test]
    fn test_session_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..100 {
            let session = WebRtcSession::new();
            assert!(ids.insert(session.id()));
        }
    }

    // ── WebRtcSession ─────────────────────────────────────────────────

    #[test]
    fn test_session_creation() {
        let session = WebRtcSession::new();
        let _ = session.id(); // not zero
        assert!(session.created_at() <= Instant::now());
    }

    #[test]
    fn test_session_default() {
        let session = WebRtcSession::default();
        let _ = session.id();
    }

    #[test]
    fn test_new_session_has_video_mid() {
        let session = WebRtcSession::new();
        assert!(
            session.video_mid().is_some(),
            "Session should have a video mid after creation"
        );
    }

    #[test]
    fn test_session_not_connected_on_new() {
        let session = WebRtcSession::new();
        assert!(!session.is_connected());
    }

    #[test]
    fn test_session_no_encoder_by_default() {
        let session = WebRtcSession::new();
        assert!(!session.has_encoder());
    }

    // ── SDP answer generation (fallback) ──────────────────────────────

    #[test]
    fn test_basic_video_answer_contains_sendonly() {
        let offer = concat!(
            "v=0\r\n",
            "o=- 0 0 IN IP4 127.0.0.1\r\n",
            "s=-\r\n",
            "t=0 0\r\n",
            "m=video 9 UDP/TLS/RTP/SAVPF 96\r\n",
            "c=IN IP4 127.0.0.1\r\n",
            "a=rtpmap:96 H264/90000\r\n"
        );
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("m=video"));
        assert!(answer.contains("a=sendonly"));
        assert!(answer.contains("a=rtpmap:96 H264/90000"));
    }

    #[test]
    fn test_basic_video_answer_preserves_payload_type() {
        let offer = concat!(
            "v=0\r\n",
            "m=video 9 UDP/TLS/RTP/SAVPF 120\r\n",
            "a=rtpmap:120 H264/90000\r\n"
        );
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("m=video 9 UDP/TLS/RTP/SAVPF 120"));
        assert!(answer.contains("a=rtpmap:120 H264/90000"));
    }

    #[test]
    fn test_basic_video_answer_defaults_to_h264() {
        let offer = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n";
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("a=rtpmap:96 H264/90000"));
    }

    #[test]
    fn test_create_video_answer_sdp_uses_fallback_on_bad_offer() {
        // str0m cannot parse this minimal offer; should fall back.
        let answer = create_video_answer_sdp("not-a-real-sdp");
        assert!(answer.contains("m=video"));
        assert!(answer.contains("a=sendonly"));
    }

    // ── ICE candidate helpers ─────────────────────────────────────────

    #[test]
    fn test_parse_bad_ice_candidate_returns_error() {
        assert!(parse_ice_candidate("not-a-candidate").is_err());
    }

    #[test]
    fn test_create_host_candidate_valid() {
        let candidate = create_host_candidate("127.0.0.1:49152");
        assert!(candidate.is_ok());
    }

    #[test]
    fn test_create_host_candidate_invalid() {
        assert!(create_host_candidate("not-an-address").is_err());
    }
}
