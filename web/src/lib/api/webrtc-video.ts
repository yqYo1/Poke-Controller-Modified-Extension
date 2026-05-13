/**
 * WebRTC video track client with MJPEG fallback.
 *
 * Provides an `RTCPeerConnection` wrapper that uses an existing
 * `WebSocketClient` for SDP negotiation (signaling).  When WebRTC
 * video is unavailable or fails, the client transparently falls back
 * to MJPEG streaming over HTTP.
 *
 * The video source is obtained either from the `RTCPeerConnection`
 * (WebRTC path) or from the known MJPEG stream URL.  Consumers
 * attach to a `<video>` element and drive playback from whichever
 * source is active.
 *
 * @module webrtc-video
 */

import { WebSocketClient, type WSMessage } from './websocket';

// ─── Connection State ────────────────────────────────────────────────────────

export type VideoTrackState =
	| 'new'
	| 'connecting'
	| 'connected'
	| 'disconnected'
	| 'failed';

export interface VideoTrackEventMap {
	/** Emitted when a MediaStream becomes available (WebRTC path). */
	stream: MediaStream;
	/** Emitted when WebRTC is unavailable and MJPEG fallback is used (provides the URL). */
	fallback: string;
	state: VideoTrackState;
	error: string;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type EventCallback = (...args: any[]) => void;

// ─── Configuration ───────────────────────────────────────────────────────────

export interface VideoTrackConfig {
	/**
	 * RTC configuration passed to `RTCPeerConnection`.
	 * The default uses Google's public STUN server.
	 */
	rtcConfig?: RTCConfiguration;

	/**
	 * Set to `false` to disable WebRTC and always use MJPEG.
	 * @default true
	 */
		rtcEnabled?: boolean;

		/**
		 * Custom ICE server URIs for the RTCPeerConnection.
		 * Overrides the default Google STUN server.
		 * @default [{ urls: 'stun:stun.l.google.com:19302' }]
		 */
		iceServers?: RTCIceServer[];

		/**
		 * MJPEG stream endpoint URL (relative or absolute).
		 * @default '/camera/stream'
		 */
		mjpegUrl?: string;
	}

// ─── Signaling Message Types ─────────────────────────────────────────────────

interface SignalingMessage {
	type: 'signaling';
	subtype: 'video_offer' | 'video_answer' | 'video_candidate' | 'video_info';
	sdp?: string;
	candidate?: RTCIceCandidateInit;
	mjpeg_url?: string;
}

// ─── Video Track Client ──────────────────────────────────────────────────────

/**
 * Typed WebRTC video track client with MJPEG fallback.
 *
 * Uses the WebSocket client for signaling (ICE candidate + SDP exchange).
 * When WebRTC is not supported or fails, retrieves the MJPEG stream URL
 * from the signaling response and emits a `'fallback'` event.
 *
 * Typical usage:
 * ```ts
 * const video = new WebRTCVideoClient(wsClient);
 * video.on('stream', (stream) => { videoEl.srcObject = stream; });
 * video.on('fallback', (url) => { videoEl.src = url; });
 * video.connect();
 * ```
 */
export class WebRTCVideoClient {
	private pc: RTCPeerConnection | null = null;
	private signal: WebSocketClient;
	private config: Required<VideoTrackConfig>;
	private _stateValue: VideoTrackState = 'new';
	private listeners = new Map<string, Set<EventCallback>>();
	private pendingCandidates: RTCIceCandidateInit[] = [];
	private _stream: MediaStream | null = null;
	private _mjpegUrl: string | null = null;
	private signalingDone = false;
	private fallbackTimer: ReturnType<typeof setTimeout> | null = null;

	/**
	 * @param signal   - WebSocket client used for SDP signaling
	 * @param config   - Additional configuration
	 */
	constructor(
		signal: WebSocketClient,
		config: VideoTrackConfig = {},
	) {
		this.signal = signal;
		this.config = {
			rtcConfig: config.rtcConfig ?? {
				iceServers: config.iceServers ?? [{ urls: 'stun:stun.l.google.com:19302' }],
			},
			rtcEnabled: config.rtcEnabled ?? true,
			mjpegUrl: config.mjpegUrl ?? '/camera/stream',
			iceServers: config.iceServers ?? [{ urls: 'stun:stun.l.google.com:19302' }],
		};

		// Listen for signaling messages on the WebSocket
		this.signal.on('message', this._onSignalMessage);
	}

	// ── Public event API ──────────────────────────────────────────────────

	/**
	 * Register an event listener.
	 *
	 * @param event    - Event name (`'stream'`, `'fallback'`, `'state'`, `'error'`)
	 * @param listener - Callback invoked when the event fires
	 */
	on(event: string, listener: EventCallback): void {
		if (!this.listeners.has(event)) {
			this.listeners.set(event, new Set());
		}
		this.listeners.get(event)!.add(listener);
	}

	/**
	 * Remove a previously registered event listener.
	 *
	 * @param event    - Event name
	 * @param listener - The same function reference passed to `on()`
	 */
	off(event: string, listener: EventCallback): void {
		this.listeners.get(event)?.delete(listener);
	}

	private emit(event: string, ...args: unknown[]): void {
		const handlers = this.listeners.get(event);
		if (handlers) {
			for (const listener of handlers) {
				listener(...args);
			}
		}
	}

	// ── Connection management ─────────────────────────────────────────────

	/**
	 * Open (or re-open) the video connection.
	 *
	 * 1. If WebRTC is enabled and supported, initiate a peer connection
	 *    using the WebSocket signaling channel.
	 * 2. If WebRTC is disabled or unsupported, immediately use the MJPEG
	 *    fallback.
	 */
	connect(): void {
		if (this._stateValue === 'connecting' || this._stateValue === 'connected') {
			return;
		}
		this._setState('connecting');
		this.signalingDone = false;

		if (!this._webrtcSupported() || !this.config.rtcEnabled) {
			this._fallbackToMJPEG();
			return;
		}

		this._rtcConnect();
	}

	/**
	 * Close the video connection and stop all activity.
	 */
	disconnect(): void {
		this.signal.off('message', this._onSignalMessage);
		this._clearFallbackTimer();
		this._closeRTC();
		this._stream = null;
		this._mjpegUrl = null;
		this.signalingDone = false;
		this._setState('disconnected');
	}

	// ── Properties ────────────────────────────────────────────────────────

	/** Current connection state. */
	get state(): VideoTrackState {
		return this._stateValue;
	}

	/** `true` when the video source is available (WebRTC or MJPEG). */
	get connected(): boolean {
		return this.state === 'connected';
	}

	/** MediaStream from the WebRTC path (may be `null`). */
	get stream(): MediaStream | null {
		return this._stream;
	}

	/** MJPEG stream URL when using fallback (may be `null`). */
	get mjpegUrl(): string | null {
		return this._mjpegUrl;
	}

	/** Underlying `RTCPeerConnection` (may be `null`). */
	get peerConnection(): RTCPeerConnection | null {
		return this.pc;
	}

	/**
	 * Build an absolute MJPEG stream URL from the configured path.
	 */
	get absoluteMjpegUrl(): string {
		if (this._mjpegUrl) {
			// If it's already absolute, return as-is
			if (this._mjpegUrl.startsWith('http://') || this._mjpegUrl.startsWith('https://')) {
				return this._mjpegUrl;
			}
			// Prepend origin
			const origin = typeof location !== 'undefined' ? location.origin : 'http://localhost:8020';
			return `${origin}${this._mjpegUrl}`;
		}
		// Default fallback
		const origin = typeof location !== 'undefined' ? location.origin : 'http://localhost:8020';
		return `${origin}${this.config.mjpegUrl}`;
	}

	// ── Internals ─────────────────────────────────────────────────────────

	/**
	 * Detect whether the browser supports WebRTC.
	 */
	private _webrtcSupported(): boolean {
		return (
			typeof RTCPeerConnection !== 'undefined' &&
			typeof RTCRtpReceiver !== 'undefined'
		);
	}

	/**
	 * Fallback path: use MJPEG streaming.
	 */
	private _fallbackToMJPEG(): void {
		this._mjpegUrl = this.config.mjpegUrl;
		this.emit('fallback', this.absoluteMjpegUrl);
		this._setState('connected');
	}

	/**
	 * WebRTC path — create an RTCPeerConnection for receiving video.
	 */
	private _rtcConnect(): void {
		if (this.pc) {
			this._closeRTC();
		}

		try {
			this.pc = new RTCPeerConnection(this.config.rtcConfig);

			this.pc.ontrack = (event: RTCTrackEvent) => {
				if (event.streams && event.streams.length > 0) {
					this._stream = event.streams[0];
					this.emit('stream', this._stream);
					this._setState('connected');
				}
			};

			this.pc.onicecandidate = (event) => {
				if (event.candidate) {
					this._sendSignaling({
						type: 'signaling',
						subtype: 'video_candidate',
						candidate: event.candidate.toJSON(),
					});
				}
			};

			this.pc.oniceconnectionstatechange = () => {
				if (!this.pc) return;
				switch (this.pc.iceConnectionState) {
					case 'disconnected':
					case 'failed':
						this._setState('disconnected');
						this._onRtcFailure();
						break;
					case 'connected':
						// ontrack should have already emitted 'stream'
						this._setState('connected');
						break;
				}
			};

			this.pc.onnegotiationneeded = () => {
				this._createAndSendOffer();
			};

			// Add a video transceiver (recvonly — we only receive)
			this.pc.addTransceiver('video', { direction: 'recvonly' });

			// Create and send the initial SDP offer
			this._createAndSendOffer();
		} catch (err) {
			console.error('[WebRTCVideo] RTCPeerConnection error:', err);
			this.emit('error', `RTCPeerConnection error: ${err instanceof Error ? err.message : String(err)}`);
			this._onRtcFailure();
		}
	}

	/**
	 * Create an SDP offer and send it via signaling.
	 */
	private async _createAndSendOffer(): Promise<void> {
		if (!this.pc) return;
		try {
			const offer = await this.pc.createOffer();
			await this.pc.setLocalDescription(offer);
			if (!this.pc.localDescription) return;
			this._sendSignaling({
				type: 'signaling',
				subtype: 'video_offer',
				sdp: this.pc.localDescription.sdp,
			});
		} catch (err) {
			console.error('[WebRTCVideo] Failed to create offer:', err);
			this.emit('error', `Failed to create offer: ${err instanceof Error ? err.message : String(err)}`);
			this._onRtcFailure();
		}
	}

	/**
	 * Handle incoming signaling messages from the WebSocket.
	 */
	private _onSignalMessage = (msg: WSMessage): void => {
		if (typeof msg !== 'object' || msg === null) return;
		const payload = msg as unknown as SignalingMessage;

		if (payload.type !== 'signaling') return;

		switch (payload.subtype) {
			case 'video_offer':
				this._handleRemoteOffer(payload);
				break;
			case 'video_answer':
				this._handleRemoteAnswer(payload);
				break;
			case 'video_candidate':
				this._handleRemoteCandidate(payload);
				break;
			case 'video_info':
				this._handleVideoInfo(payload);
				break;
		}
	};

	/**
	 * Process a received SDP offer — create answer and send it back.
	 */
	private async _handleRemoteOffer(
		payload: SignalingMessage,
	): Promise<void> {
		if (!this.pc) {
			// We are the answering side — create a PC first
			try {
				this.pc = new RTCPeerConnection(this.config.rtcConfig);

				this.pc.ontrack = (event: RTCTrackEvent) => {
					if (event.streams && event.streams.length > 0) {
						this._stream = event.streams[0];
						this.emit('stream', this._stream);
						this._setState('connected');
					}
				};

				this.pc.onicecandidate = (event) => {
					if (event.candidate) {
						this._sendSignaling({
							type: 'signaling',
							subtype: 'video_candidate',
							candidate: event.candidate.toJSON(),
						});
					}
				};

				this.pc.oniceconnectionstatechange = () => {
					if (!this.pc) return;
					if (
						this.pc.iceConnectionState === 'disconnected' ||
						this.pc.iceConnectionState === 'failed'
					) {
						this._setState('disconnected');
					}
				};

				// Flush any pending candidates
				try {
					for (const candidate of this.pendingCandidates) {
						await this.pc.addIceCandidate(
							new RTCIceCandidate(candidate),
						);
					}
				} finally {
					this.pendingCandidates = [];
				}
			} catch (err) {
				console.error('[WebRTCVideo] Failed to create answering PC:', err);
				this.emit('error', `Failed to create answering PC: ${err instanceof Error ? err.message : String(err)}`);
				return;
			}
		}

		try {
			const sdp = payload.sdp as string;
			await this.pc.setRemoteDescription(
				new RTCSessionDescription({ type: 'offer', sdp }),
			);
			const answer = await this.pc.createAnswer();
			await this.pc.setLocalDescription(answer);
			this._sendSignaling({
				type: 'signaling',
				subtype: 'video_answer',
				sdp: this.pc.localDescription!.sdp,
			});
		} catch (err) {
			console.error('[WebRTCVideo] Failed to handle offer:', err);
			this.emit('error', `Failed to handle offer: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	/**
	 * Process a received SDP answer.
	 */
	private async _handleRemoteAnswer(
		payload: SignalingMessage,
	): Promise<void> {
		if (!this.pc) return;
		try {
			const sdp = payload.sdp as string;
			await this.pc.setRemoteDescription(
				new RTCSessionDescription({ type: 'answer', sdp }),
			);

			// If the answer contains inactive media, no stream will arrive
			// via ontrack — fall back to MJPEG after a short timeout
			if (!this.signalingDone) {
				this.signalingDone = true;
				this.fallbackTimer = setTimeout(() => {
					this.fallbackTimer = null;
					if (!this._stream) {
						console.info('[WebRTCVideo] No media stream received via WebRTC — falling back to MJPEG');
						this._fallbackToMJPEG();
					}
				}, 1000);
			}
		} catch (err) {
			console.error('[WebRTCVideo] Failed to handle answer:', err);
			this.emit('error', `Failed to handle answer: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	/**
	 * Process a received ICE candidate.
	 */
	private async _handleRemoteCandidate(
		payload: SignalingMessage,
	): Promise<void> {
		if (!payload.candidate) return;
		if (!this.pc || !this.pc.remoteDescription) {
			// Queue until we have a remote description
			this.pendingCandidates.push(payload.candidate);
			return;
		}
		try {
			await this.pc.addIceCandidate(new RTCIceCandidate(payload.candidate));
		} catch (err) {
			console.error('[WebRTCVideo] Failed to add ICE candidate:', err);
			this.emit('error', `Failed to add ICE candidate: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	/**
	 * Process a video_info message — tells us the WebRTC video path
	 * is not available and provides the MJPEG URL.
	 */
	private _handleVideoInfo(payload: SignalingMessage): void {
		if (payload.mjpeg_url) {
			// Validate URL — only accept relative paths or http(s) URLs
			if (typeof payload.mjpeg_url !== 'string' ||
				(!payload.mjpeg_url.startsWith('/') &&
				 !payload.mjpeg_url.startsWith('http://') &&
				 !payload.mjpeg_url.startsWith('https://'))) {
				console.warn('[WebRTCVideo] Invalid mjpeg_url scheme, ignoring:', payload.mjpeg_url);
				return;
			}
			this._mjpegUrl = payload.mjpeg_url;
			console.info('[WebRTCVideo] Backend signaled MJPEG fallback URL:', payload.mjpeg_url);
			// If we haven't received a stream yet, fall back immediately
			if (!this._stream) {
				this._fallbackToMJPEG();
			}
		}
	}

	/**
	 * Send a signaling message over the existing WebSocket.
	 */
	private _sendSignaling(payload: SignalingMessage): void {
		this.signal.send(payload as unknown as Record<string, unknown>);
	}

	/**
	 * Called when WebRTC fails — fall back to MJPEG.
	 */
	private _onRtcFailure(): void {
		this._closeRTC();
		console.info('[WebRTCVideo] Falling back to MJPEG streaming');
		this.emit('error', 'WebRTC connection failed, falling back to MJPEG');
		this._fallbackToMJPEG();
	}

	/**
	 * Clear the pending fallback timer, if any.
	 */
	private _clearFallbackTimer(): void {
		if (this.fallbackTimer) {
			clearTimeout(this.fallbackTimer);
			this.fallbackTimer = null;
		}
	}

	/**
	 * Tear down the peer connection.
	 */
	private _closeRTC(): void {
		this._clearFallbackTimer();
		if (this.pc) {
			this.pc.ontrack = null;
			this.pc.onicecandidate = null;
			this.pc.oniceconnectionstatechange = null;
			this.pc.onnegotiationneeded = null;
			this.pc.close();
			this.pc = null;
		}
		this.pendingCandidates = [];
		this.signalingDone = false;
	}

	/**
	 * Update internal state and emit state-change event.
	 */
	private _setState(newState: VideoTrackState): void {
		if (this.state === newState) return;
		this._stateValue = newState;
		this.emit('state', newState);
	}
}
