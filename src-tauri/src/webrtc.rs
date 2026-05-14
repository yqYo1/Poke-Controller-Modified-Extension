//! WebRTC video streaming backend using [str0m] and pure-Rust VP8 encoding.
//!
//! Provides a complete WebRTC implementation for streaming camera video
//! frames as VP8/RTP to Web frontends. Handles:
//!
//! - SDP offer/answer exchange (via str0m)
//! - ICE candidate management
//! - VP8 codec negotiation via SDP
//! - VP8 encoding of raw camera frames (RGB → YUV → VP8)
//! - RTP packetization via str0m's Writer API
//! - WebRTC session lifecycle and RTP sending tasks
//!
//! # Architecture
//!
//! [`WebRtcManager`] is the top-level session manager stored in
//! shared application state (`Arc<Mutex<webrtc::WebRtcManager>>`).
//! Each client connection gets a [`WebRtcSession`] with its own
//! [`str0m::Rtc`] instance for SDP/ICE negotiation, and a
//! [`Vp8Encoder`] for pure-Rust VP8 encoding.
//!
//! When a session is created and an SDP offer accepted, the manager
//! spawns an RTP send task that continuously captures camera frames,
//! encodes them to VP8, and pushes them through str0m's media channel
//! for RTP packetization and ICE/DTLS export.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use str0m::change::SdpOffer;
use str0m::media::{MediaKind, Mid};
use str0m::net::Protocol;
use str0m::{Candidate, Rtc};
use tokio::sync::Mutex;

use oxideav_vp8::{Vp8Frame, encoder::encode_vp8_keyframe};

// ── Re-exports ─────────────────────────────────────────────────────────────────

pub use str0m::media::Pt;

// ── Session identifier ─────────────────────────────────────────────────────

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
    pub fn from_str(s: &str) -> Option<Self> {
        s.parse::<u64>().ok().map(Self)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── VP8 Encoder ─────────────────────────────────────────────────────────────

/// Configuration for the VP8 encoder.
#[derive(Debug, Clone)]
pub struct Vp8EncoderConfig {
    /// Target video width (must be even for I420).
    pub width: u32,
    /// Target video height (must be even for I420).
    pub height: u32,
    /// Target frame rate in fps.
    pub framerate: u32,
    /// Target bitrate in kbps (e.g., 1000 = 1 Mbps).
    pub bitrate_kbps: u32,
}

impl Default for Vp8EncoderConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            framerate: 30,
            bitrate_kbps: 1000,
        }
    }
}

/// A VP8 video encoder backed by [`oxideav_vp8`].
///
/// Accepts raw RGB frames and produces VP8-encoded bitstream data
/// suitable for RTP packetization. Uses pure-Rust encoding (no system
/// library dependencies).
pub struct Vp8Encoder {
    config: Vp8EncoderConfig,
    frame_count: u64,
    /// Reusable buffer for YUV 4:2:0 conversion.
    yuv_buf: Vec<u8>,
}

impl std::fmt::Debug for Vp8Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vp8Encoder")
            .field("config", &self.config)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

impl Vp8Encoder {
    /// Create a new VP8 encoder with the given configuration.
    pub fn new(config: Vp8EncoderConfig) -> Result<Self, String> {
        // Pre-allocate YUV 4:2:0 buffer.
        let w = config.width as usize;
        let h = config.height as usize;
        let y_len = w * h;
        let uv_len = (w / 2) * (h / 2);

        Ok(Self {
            config,
            frame_count: 0,
            yuv_buf: vec![0u8; y_len + 2 * uv_len],
        })
    }

    /// Encode a raw RGB24 frame into VP8 bitstream data.
    ///
    /// `rgb_data` must be exactly `width * height * 3` bytes long.
    /// Returns the encoded VP8 frame bytes.
    pub fn encode_rgb(&mut self, rgb_data: &[u8]) -> Result<Vec<u8>, String> {
        let w = self.config.width as usize;
        let h = self.config.height as usize;
        let expected = w * h * 3;

        if rgb_data.len() != expected {
            return Err(format!(
                "RGB frame size mismatch: got {} bytes, expected {} ({}x{})",
                rgb_data.len(),
                expected,
                w,
                h,
            ));
        }

        // Convert RGB → YUV 4:2:0 planar.
        self.rgb_to_yuv420(rgb_data, w, h);

        // Build Vp8Frame from the YUV buffer (tightly-packed I420 planes).
        let y_plane_size = w * h;
        let uv_plane_size = (w / 2) * (h / 2);

        let (y_plane, rest) = self.yuv_buf.split_at(y_plane_size);
        let (u_plane, v_plane) = rest.split_at(uv_plane_size);

        let frame = Vp8Frame {
            width: self.config.width,
            height: self.config.height,
            pts: Some(self.frame_count as i64),
            y: y_plane.to_vec(),
            u: u_plane.to_vec(),
            v: v_plane.to_vec(),
            y_stride: self.config.width,
            uv_stride: (self.config.width + 1) / 2,
        };

        // Map bitrate to VP8 quantiser index (0 = best quality, 127 = worst).
        let qindex = 127u8.saturating_sub((self.config.bitrate_kbps / 10).min(127) as u8);

        let result = encode_vp8_keyframe(self.config.width, self.config.height, qindex, &frame)
            .map_err(|e| format!("VP8 encode failed at frame {}: {:?}", self.frame_count, e))?;

        self.frame_count += 1;
        Ok(result)
    }

    /// Request the next frame be a keyframe.
    ///
    /// With `oxideav-vp8` every frame is encoded as a keyframe,
    /// so this is a no-op (retained for API compatibility).
    pub fn force_keyframe(&mut self) {
        // All frames are keyframes — nothing to do.
    }

    /// Convert RGB24 pixel data to YUV 4:2:0 planar format in place.
    ///
    /// This uses the ITU-R BT.601 standard matrix with full range.
    fn rgb_to_yuv420(&mut self, rgb: &[u8], width: usize, height: usize) {
        use yuv::{
            BufferStoreMut, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix,
        };

        let w = width;
        let h = height;

        // Split the YUV buffer into Y, U, V planes.
        let y_plane_size = w * h;
        let uv_plane_size = (w / 2) * (h / 2);

        let (y_plane, rest) = self.yuv_buf.split_at_mut(y_plane_size);
        let (u_plane, v_plane) = rest.split_at_mut(uv_plane_size);

        let mut yuv_image = YuvPlanarImageMut {
            y_plane: BufferStoreMut::Borrowed(y_plane),
            y_stride: w as u32,
            u_plane: BufferStoreMut::Borrowed(u_plane),
            u_stride: (w / 2) as u32,
            v_plane: BufferStoreMut::Borrowed(v_plane),
            v_stride: (w / 2) as u32,
            width: w as u32,
            height: h as u32,
        };

        yuv::rgb_to_yuv420(
            &mut yuv_image,
            rgb,
            (w * 3) as u32, // RGB stride = width * 3 bytes per pixel
            YuvRange::Full,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Balanced,
        )
        .expect("YUV conversion failed");
    }

    /// Returns the encoder configuration.
    pub fn config(&self) -> &Vp8EncoderConfig {
        &self.config
    }
}

// ── WebRTC session ─────────────────────────────────────────────────────────

/// Error type for RTP send task operations.
#[derive(Debug)]
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
pub struct WebRtcSession {
    /// The underlying str0m peer connection.
    rtc: Rtc,
    /// Wall-clock creation time (for inactivity timeouts).
    created_at: Instant,
    /// Unique session identifier.
    session_id: SessionId,
    /// VP8 encoder for this session.
    encoder: Option<Vp8Encoder>,
    /// The media ID for the video track (set after SDP negotiation).
    video_mid: Option<Mid>,
    /// Whether ICE+DTLS has fully connected.
    connected: bool,
    /// RTP timestamp base (90 kHz clock).
    rtp_timestamp: u32,
}

impl WebRtcSession {
    /// Create a new WebRTC session with default settings.
    pub fn new() -> Self {
        let mut rtc = Rtc::new(Instant::now());

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
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Check whether the ICE connection is established.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Check if this session has an encoder configured.
    pub fn has_encoder(&self) -> bool {
        self.encoder.is_some()
    }

    /// Initialize the VP8 encoder with the given configuration.
    pub fn init_encoder(&mut self, config: Vp8EncoderConfig) -> Result<(), String> {
        let encoder = Vp8Encoder::new(config)?;
        self.encoder = Some(encoder);
        Ok(())
    }

    /// Accept an SDP offer and produce a sendonly VP8 video answer.
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
    pub fn video_mid(&self) -> Option<Mid> {
        self.video_mid
    }

    /// Get a mutable reference to the VP8 encoder (if initialized).
    pub fn encoder_mut(&mut self) -> Option<&mut Vp8Encoder> {
        self.encoder.as_mut()
    }

    /// Get a reference to the inner str0m Rtc instance.
    pub fn rtc(&self) -> &Rtc {
        &self.rtc
    }

    /// Get a mutable reference to the inner str0m Rtc instance.
    pub fn rtc_mut(&mut self) -> &mut Rtc {
        &mut self.rtc
    }

    /// Write a VP8 encoded frame to str0m's media channel.
    ///
    /// This packetizes the frame as RTP and makes it available via
    /// [`poll_output()`](Self::poll_output) as Transmit events.
    pub fn write_vp8_frame(&mut self, vp8_data: &[u8]) -> Result<(), RtpSendError> {
        let mid = self.video_mid.ok_or(RtpSendError::MediaNotAdded)?;

        if !self.connected {
            return Err(RtpSendError::SessionNotConnected);
        }

        let Some(writer) = self.rtc.writer(mid) else {
            return Err(RtpSendError::WriterNotFound(mid));
        };

        // Find the VP8 payload type from the media config.
        let pt = writer
            .payload_params()
            .find(|p| p.spec().codec == str0m::format::Codec::Vp8)
            .map(|p| p.pt())
            .ok_or_else(|| RtpSendError::WriterNotFound(mid))?;

        let wallclock = Instant::now();
        let rtp_time = str0m::media::MediaTime::new(
            self.rtp_timestamp as u64,
            str0m::media::Frequency::NINETY_KHZ,
        );

        // Write the frame to str0m; it handles RTP packetization.
        writer
            .write(pt, wallclock, rtp_time, vp8_data.to_vec())
            .map_err(|e| RtpSendError::EncoderError(format!("str0m write error: {:?}", e)))?;

        // Advance timestamp by ~3000 per frame at 30 fps (90000 / 30).
        self.rtp_timestamp = self.rtp_timestamp.wrapping_add((90_000.0 / 30.0) as u32);

        Ok(())
    }
}

impl Default for WebRtcSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── WebRTC manager ─────────────────────────────────────────────────────────

/// Top-level manager for all active WebRTC video sessions.
///
/// This is the struct stored in shared application state via
/// `Arc<Mutex<webrtc::WebRtcManager>>`.  It owns a collection of
/// [`WebRtcSession`]s keyed by [`SessionId`].
pub struct WebRtcManager {
    sessions: HashMap<SessionId, WebRtcSession>,
    /// Monotonically-increasing RTP sequence number counter.
    next_sequence: u64,
}

impl WebRtcManager {
    /// Create a new empty WebRTC session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_sequence: 0,
        }
    }

    /// Open a new WebRTC session and return its [`SessionId`].
    ///
    /// The caller should later call [`close_session`](Self::close_session)
    /// when the session ends.
    pub fn create_session(&mut self) -> SessionId {
        let session = WebRtcSession::new();
        let id = session.id();
        self.sessions.insert(id, session);
        id
    }

    /// Open a new WebRTC session with a VP8 encoder and return its [`SessionId`].
    pub fn create_session_with_encoder(
        &mut self,
        encoder_config: Vp8EncoderConfig,
    ) -> Result<SessionId, String> {
        let mut session = WebRtcSession::new();
        let id = session.id();
        session.init_encoder(encoder_config)?;
        self.sessions.insert(id, session);
        Ok(id)
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
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Remove sessions that have lived longer than `timeout`.
    ///
    /// Call this from a periodic background task to prevent resource leaks.
    pub fn cleanup_stale_sessions(&mut self, timeout: std::time::Duration) {
        let now = Instant::now();
        self.sessions
            .retain(|_, session| now.duration_since(session.created_at()) < timeout);
    }

    /// Allocate the next RTP sequence number (wraps at u16::MAX).
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
    pub fn poll_session(&mut self, id: SessionId) -> Result<str0m::Output, String> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("Session {} not found", id))?;
        session
            .poll_output()
            .map_err(|e| format!("poll_output error: {:?}", e))
    }

    /// Encode and send a camera frame as VP8/RTP through a session.
    ///
    /// This is the main pipeline entry point: RGB frame → VP8 encode →
    /// str0m write → RTP output.
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

        // Validate dimensions match encoder configuration.
        let cfg = encoder.config();
        if cfg.width != width || cfg.height != height {
            return Err(RtpSendError::EncoderError(format!(
                "Frame dimensions mismatch: encoder expects {}x{} but got {}x{}",
                cfg.width, cfg.height, width, height,
            )));
        }

        // Encode RGB frame to VP8.
        let vp8_data = encoder
            .encode_rgb(rgb_data)
            .map_err(RtpSendError::EncoderError)?;

        // Write VP8 data through str0m's media channel.
        session.write_vp8_frame(&vp8_data)
    }
}

impl Default for WebRtcManager {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RTP Send Task
// ═════════════════════════════════════════════════════════════════════════════

/// A background RTP stream sender that wraps a WebRTC session and
/// handles poll_output events (ICE timeouts, transmits, etc.).
///
/// This is spawned as a [`tokio::task`] when a new WebRTC session is
/// established. It periodically:
/// 1. Polls str0m for output events (transmits, timeouts, media events)
/// 2. Handles timeout events by feeding them back
/// 3. Reports transmit events back for network I/O
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
pub enum RtpOutput {
    /// Data to transmit over UDP/network (ICE connectivity checks, RTP, etc.).
    Transmit(Vec<u8>),
    /// The session has connected.
    Connected,
    /// The session has disconnected.
    Disconnected,
    /// Log message.
    Log(String),
}

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
                        let dur = if at <= Instant::now() {
                            // Already expired, handle immediately.
                            let _ = session.handle_timeout(at);
                            std::time::Duration::ZERO
                        } else {
                            at.saturating_duration_since(Instant::now())
                        };
                        dur
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
                            str0m::Event::IceConnectionStateChange(state) => {
                                if state == str0m::IceConnectionState::Disconnected {
                                    let _ = output_tx.send(RtpOutput::Disconnected).await;
                                    tracing::info!("Session {} disconnected", session_id);
                                }
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

// ═════════════════════════════════════════════════════════════════════════════
// SDP Helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Parse a VP8 SDP offer and produce a `sendonly` answer.
///
/// Uses str0m if parsing succeeds; otherwise falls back to a manually-
/// constructed minimal answer.  The answer advertises VP8 / payload type 96.
pub fn create_video_answer_sdp(offer_sdp: &str) -> String {
    let mut session = WebRtcSession::new();
    match session.accept_offer(offer_sdp) {
        Ok(answer) => answer,
        Err(_) => generate_basic_video_answer(offer_sdp),
    }
}

/// Fallback: construct a minimal sendonly VP8 SDP answer string manually.
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

    // Preserve the codec rtpmap from the offer, or default to VP8.
    let codec_rtpmap = offer_sdp
        .lines()
        .find(|l| l.starts_with(&format!("a=rtpmap:{}", payload_type)))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("a=rtpmap:{} VP8/90000", payload_type));

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

// ═════════════════════════════════════════════════════════════════════════════
// RTP / VP8 Packetization (legacy, kept for backward compatibility)
// ═════════════════════════════════════════════════════════════════════════════

/// Minimal VP8 payload descriptor (RFC 7741 Section 4.2).
///
/// Currently produces a single-byte descriptor with:
/// - X=0 (no extended control bits)
/// - N=0 (non-reference frame)
/// - S=1 (start of VP8 partition)
/// - PID=0 (partition index)
#[derive(Debug, Clone, Copy, Default)]
pub struct Vp8Descriptor;

impl Vp8Descriptor {
    /// Encode the VP8 payload descriptor as a byte slice.
    ///
    /// Returns a single byte `0b0001_0000` (S=1, PID=0).
    pub fn encode(&self) -> [u8; 1] {
        [0b0001_0000] // S=1, PID=0
    }
}

/// A fully-formed RTP packet containing VP8 video data.
///
/// The payload consists of a VP8 payload descriptor followed by the
/// raw VP8 encoded frame bytes.
///
/// Note: When using str0m's frame-level API ([`Writer::write`]), this
/// manual packetization is not needed — str0m handles RTP packetization
/// internally. This struct is retained for backward compatibility and
/// direct RTP-level usage.
#[derive(Debug, Clone)]
pub struct Vp8RtpPacket {
    /// Complete RTP packet bytes (header + payload).
    pub bytes: Vec<u8>,
    /// RTP sequence number (for debugging / logging).
    pub sequence: u16,
}

impl Vp8RtpPacket {
    /// Build an RTP packet from a VP8-encoded video frame.
    ///
    /// # Arguments
    /// - `frame_data` -- complete VP8 encoded frame (one packet, no fragmentation).
    /// - `ssrc` -- RTP synchronization source identifier.
    /// - `sequence` -- RTP sequence number (increment per packet).
    /// - `timestamp` -- RTP timestamp (90 kHz clock; increment by 3000 approx 30 fps).
    /// - `marker` -- set `true` on the last (and only) packet of a frame.
    ///
    /// # Returns
    /// A `Vp8RtpPacket` containing the complete serialized RTP datagram.
    pub fn from_vp8_frame(
        frame_data: &[u8],
        ssrc: u32,
        sequence: u16,
        timestamp: u32,
        marker: bool,
    ) -> Self {
        let descriptor = Vp8Descriptor.encode();
        let total_len = 12 + descriptor.len() + frame_data.len(); // 12-byte RTP header
        let mut bytes = Vec::with_capacity(total_len);

        // RTP fixed header (RFC 3550 Section 5.1)
        // V=2, P=0, X=0, CC=0, M, PT=96 (dynamic VP8)
        let first_byte: u8 = 0b1000_0000; // version 2, no padding/extension/CSRC
        let pt_marker: u8 = if marker { 0b1000_0000 | 96 } else { 96 };
        bytes.push(first_byte);
        bytes.push(pt_marker);
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&timestamp.to_be_bytes());
        bytes.extend_from_slice(&ssrc.to_be_bytes());

        // VP8 payload descriptor + raw frame data
        bytes.extend_from_slice(&descriptor);
        bytes.extend_from_slice(frame_data);

        Self { bytes, sequence }
    }

    /// Consume the packet and return the raw bytes suitable for sending
    /// over a UDP socket or WebRTC data channel.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Return the RTP payload type (always 96 for VP8).
    pub const fn payload_type() -> u8 {
        96
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ICE Candidate Helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Parse a string-encoded ICE candidate (from a `candidate:` SDP attribute)
/// and return a [`str0m::Candidate`].
///
/// This is a convenience wrapper around [`Candidate::from_sdp_string`].
pub fn parse_ice_candidate(sdp_line: &str) -> Result<Candidate, String> {
    Candidate::from_sdp_string(sdp_line)
        .map_err(|e| format!("Failed to parse ICE candidate: {}", e))
}

/// Create a local host ICE candidate for the given UDP address.
pub fn create_host_candidate(addr: &str) -> Result<Candidate, String> {
    let socket_addr = addr
        .parse()
        .map_err(|e| format!("Invalid socket address '{}': {}", addr, e))?;
    Candidate::host(socket_addr, Protocol::Udp)
        .map_err(|e| format!("Failed to create host candidate: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

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

    #[test]
    fn test_init_encoder() {
        let mut session = WebRtcSession::new();
        assert!(!session.has_encoder());
        session
            .init_encoder(Vp8EncoderConfig::default())
            .expect("Failed to init encoder");
        assert!(session.has_encoder());
    }

    // ── Vp8Encoder ────────────────────────────────────────────────────

    #[test]
    fn test_vp8_encoder_create_and_encode_small_frame() {
        let config = Vp8EncoderConfig {
            width: 320,
            height: 240,
            framerate: 30,
            bitrate_kbps: 500,
        };
        let mut encoder = Vp8Encoder::new(config).expect("Failed to create VP8 encoder");

        // Create a small RGB frame (solid green).
        let rgb_data = [0u8, 255, 0].repeat((320 * 240) as usize);
        let vp8_data = encoder.encode_rgb(&rgb_data).expect("VP8 encode failed");

        assert!(!vp8_data.is_empty(), "VP8 encoded data should not be empty");
        tracing::debug!("VP8 encoded {} bytes from 320x240 frame", vp8_data.len());
    }

    #[test]
    fn test_vp8_encoder_force_keyframe() {
        let config = Vp8EncoderConfig {
            width: 160,
            height: 120,
            framerate: 30,
            bitrate_kbps: 200,
        };
        let mut encoder = Vp8Encoder::new(config).expect("Failed to create VP8 encoder");

        let rgb_data = vec![128u8; (160 * 120 * 3) as usize];
        let frame1 = encoder.encode_rgb(&rgb_data).expect("First encode failed");
        assert!(!frame1.is_empty());

        let frame2 = encoder.encode_rgb(&rgb_data).expect("Second encode failed");
        assert!(!frame2.is_empty());

        encoder.force_keyframe();
        let frame3 = encoder.encode_rgb(&rgb_data).expect("Third encode failed");
        assert!(!frame3.is_empty());
    }

    #[test]
    fn test_vp8_encoder_rejects_bad_dimensions() {
        let config = Vp8EncoderConfig {
            width: 640,
            height: 480,
            framerate: 30,
            bitrate_kbps: 1000,
        };
        let mut encoder = Vp8Encoder::new(config).expect("Failed to create VP8 encoder");

        // Wrong size data.
        let bad_data = vec![0u8; 100];
        let result = encoder.encode_rgb(&bad_data);
        assert!(result.is_err(), "Should reject wrong-sized data");
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
            "a=rtpmap:96 VP8/90000\r\n"
        );
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("m=video"));
        assert!(answer.contains("a=sendonly"));
        assert!(answer.contains("a=rtpmap:96 VP8/90000"));
    }

    #[test]
    fn test_basic_video_answer_preserves_payload_type() {
        let offer = concat!(
            "v=0\r\n",
            "m=video 9 UDP/TLS/RTP/SAVPF 120\r\n",
            "a=rtpmap:120 VP8/90000\r\n"
        );
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("m=video 9 UDP/TLS/RTP/SAVPF 120"));
        assert!(answer.contains("a=rtpmap:120 VP8/90000"));
    }

    #[test]
    fn test_basic_video_answer_defaults_to_vp8() {
        let offer = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n";
        let answer = generate_basic_video_answer(offer);
        assert!(answer.contains("a=rtpmap:96 VP8/90000"));
    }

    #[test]
    fn test_create_video_answer_sdp_uses_fallback_on_bad_offer() {
        // str0m cannot parse this minimal offer; should fall back.
        let answer = create_video_answer_sdp("not-a-real-sdp");
        assert!(answer.contains("m=video"));
        assert!(answer.contains("a=sendonly"));
    }

    // ── VP8 descriptor ────────────────────────────────────────────────

    #[test]
    fn test_vp8_descriptor_encode() {
        let bytes = Vp8Descriptor.encode();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0], 0b0001_0000); // S=1, PID=0
    }

    // ── RTP packet construction ───────────────────────────────────────

    #[test]
    fn test_vp8_rtp_packet_construction() {
        let frame = vec![0u8; 64];
        let packet = Vp8RtpPacket::from_vp8_frame(&frame, 0x12345678, 0, 0, true);

        // Header (12) + descriptor (1) + frame (64) = 77 bytes
        assert_eq!(packet.bytes.len(), 77);
        assert_eq!(packet.sequence, 0);

        // RTP version bits should be set (first byte high 2 bits = 10)
        assert_eq!(packet.bytes[0] >> 6, 0b10, "RTP version should be 2");

        // Marker bit + payload type = 96 | 0x80 = 0xE0, or 96 if no marker
        assert_eq!(packet.bytes[1], 96 | 0x80, "marker + PT=96");

        // Sequence number at bytes [2,3]
        assert_eq!(&packet.bytes[2..4], &[0x00, 0x00]);

        // SSRC at bytes [8..12]
        assert_eq!(&packet.bytes[8..12], &[0x12, 0x34, 0x56, 0x78]);

        // VP8 descriptor at byte 12
        assert_eq!(packet.bytes[12], 0b0001_0000);

        // Frame data follows descriptor
        assert_eq!(&packet.bytes[13..], &[0u8; 64]);
    }

    #[test]
    fn test_vp8_rtp_packet_sequence_increments() {
        let frame = vec![0xABu8; 16];
        let p1 = Vp8RtpPacket::from_vp8_frame(&frame, 1, 100, 3000, true);
        let p2 = Vp8RtpPacket::from_vp8_frame(&frame, 1, 101, 6000, true);

        assert_eq!(p1.sequence, 100);
        assert_eq!(p2.sequence, 101);
        assert_eq!(&p1.bytes[2..4], &[0x00, 100]);
        assert_eq!(&p2.bytes[2..4], &[0x00, 101]);
    }

    #[test]
    fn test_vp8_rtp_packet_different_timestamps() {
        let frame = vec![0u8; 8];
        let p1 = Vp8RtpPacket::from_vp8_frame(&frame, 1, 0, 0, true);
        let p2 = Vp8RtpPacket::from_vp8_frame(&frame, 1, 1, 3000, true);

        // Timestamp at bytes [4..8]
        assert_eq!(&p1.bytes[4..8], &[0, 0, 0, 0]);
        assert_eq!(&p2.bytes[4..8], &[0, 0, 0x0B, 0xB8]); // 3000 in hex
    }

    #[test]
    fn test_vp8_rtp_packet_no_marker() {
        let frame = vec![0u8; 4];
        let packet = Vp8RtpPacket::from_vp8_frame(&frame, 1, 0, 0, false);
        assert_eq!(packet.bytes[1], 96, "marker bit should not be set");
    }

    #[test]
    fn test_into_bytes() {
        let frame = vec![0u8; 4];
        let packet = Vp8RtpPacket::from_vp8_frame(&frame, 1, 0, 0, true);
        let bytes = packet.into_bytes();
        assert_eq!(bytes.len(), 12 + 1 + 4); // 17
    }

    #[test]
    fn test_payload_type_constant() {
        assert_eq!(Vp8RtpPacket::payload_type(), 96);
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
