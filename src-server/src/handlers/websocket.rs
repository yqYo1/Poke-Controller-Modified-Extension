use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

use crate::state::AppState;
use crate::webrtc;

// ═══════════════════════════════════════════════════════════════════════════════
// WebSocket Endpoint
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /ws — WebSocket upgrade endpoint for real-time events
#[utoipa::path(
    get,
    path = "/ws",
    tag = "websocket",
    responses(
        (status = 101, description = "WebSocket upgrade successful")
    )
)]
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle an individual WebSocket connection
async fn handle_ws(mut socket: WebSocket, state: AppState) {
    // Subscribe to the global event broadcast channel
    let mut rx = state.event_tx.subscribe();

    // Channel for RTP output events (ICE/RTP data to send to client)
    let (rtp_output_tx, mut rtp_output_rx) =
        tokio::sync::mpsc::unbounded_channel::<RtpOutputEvent>();

    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming message from the WebSocket client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        tracing::debug!("WS received: {text}");
                        // Parse incoming JSON commands (optional)
                        if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                            match cmd.get("type").and_then(|v| v.as_str()) {
                                Some("ping") => {
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({"type": "pong"}).to_string().into()
                                    )).await;
                                }
                                // ── WebRTC video signaling ──────────────────────
                                Some("signaling") => {
                                    handle_webrtc_signaling(&mut socket, &state, &cmd, &rtp_output_tx).await;
                                }
                                _ => {} // Unknown type, ignore
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(e)) => {
                        tracing::warn!("WS error: {e}");
                        break;
                    }
                    None => break,
                    _ => {} // Ignore Binary, Ping, Pong
                }
            }
            // Event from the broadcast channel → forward to client
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WS receiver lagged by {n} messages");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            // RTP output events (ICE transmits, connection state changes)
            Some(output) = rtp_output_rx.recv() => {
                match output {
                    RtpOutputEvent::Transmit(data) => {
                        // Send ICE/RTP data as binary WebSocket message.
                        let _ = socket.send(Message::Binary(data.into())).await;
                    }
                    RtpOutputEvent::Connected(session_id) => {
                        tracing::info!("WebRTC session {session_id} connected");
                    }
                    RtpOutputEvent::Disconnected(session_id) => {
                        tracing::info!("WebRTC session {session_id} disconnected");
                    }
                }
            }
        }
    }

    tracing::info!("WebSocket client disconnected");
}

/// Internal event type for RTP output forwarded to the WebSocket client.
#[cfg_attr(not(feature = "vaapi"), allow(dead_code))]
#[derive(Debug)]
enum RtpOutputEvent {
    /// Binary data to send over the WebSocket (ICE, RTP, etc.).
    Transmit(Vec<u8>),
    /// A WebRTC session has connected.
    Connected(webrtc::SessionId),
    /// A WebRTC session has disconnected.
    Disconnected(webrtc::SessionId),
}

/// Handles WebRTC signaling messages over the WebSocket.
///
/// The frontend sends SDP offers/answers and ICE candidates,
/// and the backend creates/manages a WebRTC session with H.264/HEVC encoding
/// and RTP streaming via the [`webrtc::WebRtcManager`].
///
/// Supported signaling subtypes:
/// - `video_offer`    → creates a WebRTC session, accepts the SDP offer,
///   starts an RTP send task, and returns the SDP answer
/// - `video_answer`   → stored / acknowledged (unusual for this flow)
/// - `video_candidate`→ forwarded to the active WebRTC session
/// - `video_close`    → closes the active session
async fn handle_webrtc_signaling(
    socket: &mut WebSocket,
    state: &AppState,
    cmd: &serde_json::Value,
    rtp_output_tx: &tokio::sync::mpsc::UnboundedSender<RtpOutputEvent>,
) {
    let _ = &rtp_output_tx;
    let subtype = cmd.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

    match subtype {
        "video_offer" => {
            // The frontend sent a WebRTC SDP offer for video.
            // Create a WebRTC session with VAAPI encoder, accept the offer,
            // and return the SDP answer.

            #[cfg(not(feature = "vaapi"))]
            {
                tracing::error!("VAAPI encoder not available (enable 'vaapi' feature)");
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type": "signaling",
                            "subtype": "video_error",
                            "message": "VAAPI hardware encoder not available. Enable 'vaapi' feature.".to_string(),
                        })
                        .to_string()
                        .into(),
                    ))
                    .await;
            }

            #[cfg(feature = "vaapi")]
            {
                if let Some(offer_sdp) = cmd.get("sdp").and_then(|v| v.as_str()) {
                    let mut mgr = state.webrtc_manager.lock().await;

                    // Create a VAAPI hardware encoder (H.264, 640x480 @ 30fps, 1 Mbps).
                    let encoder: Box<dyn webrtc::VideoEncoder> = {
                        use crate::vaapi_encoder::{VaapiConfig, VaapiEncoder};
                        let config = VaapiConfig {
                            width: 640,
                            height: 480,
                            framerate: 30,
                            bitrate_kbps: 1000,
                            ..Default::default()
                        };
                        match VaapiEncoder::new(config) {
                            Ok(e) => Box::new(e),
                            Err(e) => {
                                tracing::error!("Failed to create VAAPI encoder: {e}");
                                let _ = socket
                                    .send(Message::Text(
                                        serde_json::json!({
                                            "type": "signaling",
                                            "subtype": "video_error",
                                            "message": format!("Failed to create VAAPI encoder: {e}"),
                                        })
                                        .to_string()
                                        .into(),
                                    ))
                                    .await;
                                return;
                            }
                        }
                    };

                    let session_id = mgr.create_session_with_encoder(encoder);

                    // Accept the SDP offer and generate the answer.
                    let answer_sdp = match mgr.accept_offer(session_id, offer_sdp) {
                        Ok(answer) => answer,
                        Err(e) => {
                            // Fall back to basic answer on error.
                            tracing::warn!("str0m offer acceptance failed ({e}), using fallback");
                            mgr.close_session(session_id);
                            webrtc::generate_basic_video_answer(offer_sdp)
                        }
                    };

                    // Send the SDP answer to the frontend.
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "signaling",
                                "subtype": "video_answer",
                                "sdp": answer_sdp,
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;

                    // Start the RTP send task in the background.
                    let (_frame_tx, frame_rx) =
                        tokio::sync::mpsc::channel::<webrtc::CameraFrame>(32);
                    let (output_tx, mut output_rx) =
                        tokio::sync::mpsc::channel::<webrtc::RtpOutput>(32);

                    // Spawn a task to forward RTP output events to the WebSocket.
                    let rtp_output_tx_clone = rtp_output_tx.clone();
                    let session_id_clone = session_id;
                    tokio::spawn(async move {
                        while let Some(output) = output_rx.recv().await {
                            match output {
                                webrtc::RtpOutput::Transmit(data) => {
                                    let _ =
                                        rtp_output_tx_clone.send(RtpOutputEvent::Transmit(data));
                                }
                                webrtc::RtpOutput::Connected => {
                                    let _ = rtp_output_tx_clone
                                        .send(RtpOutputEvent::Connected(session_id_clone));
                                }
                                webrtc::RtpOutput::Disconnected => {
                                    let _ = rtp_output_tx_clone
                                        .send(RtpOutputEvent::Disconnected(session_id_clone));
                                }
                                webrtc::RtpOutput::Log(msg) => {
                                    tracing::debug!("RTP session {session_id_clone}: {msg}");
                                }
                            }
                        }
                    });

                    // Create the RTP send task and spawn it.
                    let send_task = webrtc::RtpSendTask::new(session_id, frame_rx, output_tx);
                    let mgr_clone = state.webrtc_manager.clone();
                    tokio::spawn(async move {
                        send_task.run(mgr_clone).await;
                    });

                    // Provide the MJPEG stream URL as a fallback.
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "signaling",
                                "subtype": "video_info",
                                "session_id": session_id.to_string(),
                                "mjpeg_url": "/camera/stream",
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;

                    tracing::info!(
                        "WebRTC video session created (id={session_id}, encoder=640x480@30 H.264 VAAPI)",
                    );
                }
            }
        }
        "video_answer" => {
            // Frontend sent an answer (unusual for this flow but handled)
            tracing::debug!("WebRTC video answer received (acknowledged)");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "signaling",
                        "subtype": "video_info",
                        "mjpeg_url": "/camera/stream",
                    })
                    .to_string()
                    .into(),
                ))
                .await;
        }
        "video_candidate" => {
            // Forward the ICE candidate to the active WebRTC session.
            if let (Some(session_id_str), Some(candidate_sdp)) = (
                cmd.get("session_id").and_then(|v| v.as_str()),
                cmd.get("candidate").and_then(|v| v.as_str()),
            ) {
                let mut mgr = state.webrtc_manager.lock().await;

                // Find the session by matching the string representation.
                let found_id = mgr
                    .session_ids()
                    .find(|id| id.to_string() == session_id_str)
                    .copied();

                if let Some(found) = found_id {
                    match mgr.add_ice_candidate(found, candidate_sdp) {
                        Ok(()) => {
                            tracing::debug!("ICE candidate added to session {found}");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to add ICE candidate for session {found}: {e}");
                        }
                    }
                } else {
                    tracing::warn!("No session found for ID {session_id_str}");
                }
            }

            // Acknowledge the candidate.
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "signaling",
                        "subtype": "video_info",
                        "mjpeg_url": "/camera/stream",
                    })
                    .to_string()
                    .into(),
                ))
                .await;
        }
        "video_close" => {
            // Close the active WebRTC session.
            if let Some(session_id_str) = cmd.get("session_id").and_then(|v| v.as_str()) {
                let mut mgr = state.webrtc_manager.lock().await;
                let found_id = mgr
                    .session_ids()
                    .find(|id| id.to_string() == session_id_str)
                    .copied();
                if let Some(found) = found_id {
                    mgr.close_session(found);
                    tracing::info!("WebRTC session {found} closed");
                }
            }
        }
        _ => {
            tracing::warn!("Unknown signaling subtype: {subtype}");
        }
    }
}
