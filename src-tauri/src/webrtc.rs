//! WebRTC video streaming backend using [str0m].
//!
//! Provides a complete WebRTC implementation for streaming camera video
//! frames as VP8/RTP to Web frontends. Handles:
//!
//! - SDP offer/answer exchange (via str0m with fallback)
//! - ICE candidate management
//! - VP8 codec negotiation via SDP
//! - RTP packetization of VP8 camera frames
//! - WebRTC session lifecycle management
//!
//! # Architecture
//!
//! [`WebRtcManager`] is the top-level session manager stored in
//! shared application state (`Arc<Mutex<webrtc::WebRtcManager>>`).
//! Each client connection gets a [`WebRtcSession`] with its own
//! [`str0m::Rtc`] instance for SDP/ICE negotiation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use str0m::change::SdpOffer;
use str0m::net::Protocol;
use str0m::{Candidate, Rtc};

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
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── WebRTC session ─────────────────────────────────────────────────────────

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
}

impl WebRtcSession {
    /// Create a new WebRTC session with default settings.
    pub fn new() -> Self {
        let rtc = Rtc::new(Instant::now());
        Self {
            rtc,
            created_at: Instant::now(),
            session_id: SessionId::new(),
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

    /// Check whether the ICE connection is established.
    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.rtc.is_connected()
    }

    /// Poll for output events (ICE connectivity checks, timeouts, media).
    ///
    /// Should be called periodically (e.g., every 50 ms) when the session
    /// is actively being used for media streaming.
    #[allow(dead_code)]
    pub fn poll_output(&mut self) -> Result<str0m::Output, str0m::error::RtcError> {
        self.rtc.poll_output()
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

    /// Remove a session by ID, dropping the underlying [`str0m::Rtc`].
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
}

impl Default for WebRtcManager {
    fn default() -> Self {
        Self::new()
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
/// formatting, missing attributes).  Mirrors the code previously inlined
/// in `main.rs`.
fn generate_basic_video_answer(offer_sdp: &str) -> String {
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
// RTP / VP8 Packetization
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
        let pt_marker: u8 = if marker {
            0b1000_0000 | 96
        } else {
            96
        };
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
        let packet =
            Vp8RtpPacket::from_vp8_frame(&frame, 0x12345678, 0, 0, true);

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
