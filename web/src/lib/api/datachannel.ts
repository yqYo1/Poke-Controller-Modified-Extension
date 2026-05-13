/**
 * WebRTC DataChannel client with WebSocket-signaling fallback.
 *
 * Provides an `RTCPeerConnection` wrapper that uses an existing
 * `WebSocketClient` for SDP negotiation (signaling).  When WebRTC is
 * unavailable or the peer connection fails the client transparently
 * falls back to the provided fallback WebSocket client.
 *
 * @module datachannel
 */

import { WebSocketClient, type WSMessage } from './websocket';

// ─── Connection State ────────────────────────────────────────────────────────

export type DataChannelState =
	| 'new'
	| 'connecting'
	| 'connected'
	| 'disconnected'
	| 'failed';

export interface DataChannelEventMap {
	connect: void;
	disconnect: void;
	message: string;
	state: DataChannelState;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type EventCallback = (...args: any[]) => void;

// ─── Configuration ───────────────────────────────────────────────────────────

export interface DataChannelConfig {
	/**
	 * Label for the DataChannel (default `'default'`).
	 */
	label?: string;

	/**
	 * RTC configuration passed to `RTCPeerConnection`.
	 */
	rtcConfig?: RTCConfiguration;

	/**
	 * Set to `false` to disable WebRTC and always use the fallback.
	 */
	rtcEnabled?: boolean;
}

// ─── DataChannel Client ──────────────────────────────────────────────────────

/**
 * Typed WebRTC DataChannel client.
 *
 * Uses the WebSocket client for signaling (ICE candidate + SDP exchange).
 * When WebRTC is not supported or the connection fails the client
 * transparently falls back to sending messages via the WebSocket.
 */
export class DataChannelClient {
	private pc: RTCPeerConnection | null = null;
	private channel: RTCDataChannel | null = null;
	private signal: WebSocketClient;
	private fallback: WebSocketClient | null = null;
	private config: Required<DataChannelConfig>;
	private _stateValue: DataChannelState = 'new';
	private listeners = new Map<string, Set<EventCallback>>();
	private pendingCandidates: RTCIceCandidateInit[] = [];

	/**
	 * @param signal      - WebSocket client used for SDP signaling
	 * @param fallback    - Optional WebSocket client to use as fallback when
	 *                      WebRTC is unavailable or disconnects
	 * @param config      - Additional configuration
	 */
	constructor(
		signal: WebSocketClient,
		fallback?: WebSocketClient,
		config: DataChannelConfig = {},
	) {
		this.signal = signal;
		this.fallback = fallback ?? null;
		this.config = {
			label: config.label ?? 'default',
			rtcConfig: config.rtcConfig ?? {
				iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
			},
			rtcEnabled: config.rtcEnabled ?? true,
		};

		// Listen for signaling messages on the WebSocket
		this.signal.on('message', this._onSignalMessage);
	}

	// ── Public event API ──────────────────────────────────────────────────

	/**
	 * Register an event listener.
	 *
	 * @param event    - Event name (`'connect'`, `'disconnect'`, `'message'`,
	 *                   `'state'`)
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
	 * Open (or re-open) the DataChannel connection.
	 *
	 * 1. If WebRTC is enabled and supported, initiate a peer connection
	 *    using the WebSocket signaling channel.
	 * 2. If WebRTC is disabled or unsupported, use the fallback WebSocket
	 *    client (if provided).
	 */
	connect(): void {
		this._setState('connecting');

		if (!this._webrtcSupported() || !this.config.rtcEnabled) {
			this._fallbackConnect();
			return;
		}

		this._rtcConnect();
	}

	/**
	 * Close the DataChannel connection and stop all activity.
	 */
	disconnect(): void {
		// Suppress signaling handler while tearing down
		this.signal.off('message', this._onSignalMessage);

		this._closeRTC();
		this._setState('disconnected');
	}

	/**
	 * Send a string message over the active transport.
	 *
	 * @returns `true` if the message was queued for sending
	 */
	send(data: string): boolean {
		if (this.channel && this.channel.readyState === 'open') {
			this.channel.send(data);
			return true;
		}
		// Fallback to WebSocket
		if (this.fallback && this.fallback.connected) {
			return this.fallback.send({ data });
		}
		return false;
	}

	// ── Properties ────────────────────────────────────────────────────────

	/** Current connection state. */
	get state(): DataChannelState {
		return this._stateValue;
	}

	/** `true` when the DataChannel is open or the fallback WS is connected. */
	get connected(): boolean {
		return this.state === 'connected';
	}

	/** Underlying `RTCPeerConnection` (may be `null`). */
	get peerConnection(): RTCPeerConnection | null {
		return this.pc;
	}

	/** Underlying `RTCDataChannel` (may be `null`). */
	get dataChannel(): RTCDataChannel | null {
		return this.channel;
	}

	// ── Internals ─────────────────────────────────────────────────────────

	/**
	 * Detect whether the browser supports WebRTC.
	 */
	private _webrtcSupported(): boolean {
		return (
			typeof RTCPeerConnection !== 'undefined' &&
			typeof RTCDataChannel !== 'undefined'
		);
	}

	/**
	 * Fallback path: use the secondary WebSocket client directly.
	 */
	private _fallbackConnect(): void {
		if (!this.fallback) {
			console.warn(
				'[DataChannel] WebRTC unavailable and no fallback WS provided',
			);
			this._setState('failed');
			return;
		}

		// Listen to the fallback for incoming messages
		const handler = (msg: WSMessage) => {
			if ('data' in msg && typeof msg.data === 'string') {
				this.emit('message', msg.data);
			}
		};
		this.fallback.on('message', handler);

		// Store reference so we can remove on disconnect
		this.fallback.connect();
		this._setState('connected');
	}

	/**
	 * WebRTC path — create an RTCPeerConnection + DataChannel.
	 */
	private _rtcConnect(): void {
		if (this.pc) {
			this._closeRTC();
		}

		try {
			this.pc = new RTCPeerConnection(this.config.rtcConfig);

			this.pc.onicecandidate = (event) => {
				if (event.candidate) {
					this._sendSignaling({
						type: 'candidate',
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
						this._setState('connected');
						break;
				}
			};

			this.pc.ondatachannel = (event) => {
				this._setupChannel(event.channel);
			};

			// Create the DataChannel
			this.channel = this.pc.createDataChannel(this.config.label, {
				ordered: true,
			});
			this._setupChannel(this.channel);

			// Create and send SDP offer
			this.pc
				.createOffer()
				.then((offer) => this.pc!.setLocalDescription(offer))
				.then(() => {
					if (!this.pc?.localDescription) return;
					this._sendSignaling({
						type: 'offer',
						sdp: this.pc.localDescription.sdp,
					});
				})
				.catch((err) => {
					console.error('[DataChannel] Failed to create offer:', err);
					this._onRtcFailure();
				});
		} catch (err) {
			console.error('[DataChannel] RTCPeerConnection error:', err);
			this._onRtcFailure();
		}
	}

	/**
	 * Wire up event handlers on an `RTCDataChannel`.
	 */
	private _setupChannel(channel: RTCDataChannel): void {
		this.channel = channel;

		channel.onopen = () => {
			this._setState('connected');
		};

		channel.onclose = () => {
			this._setState('disconnected');
		};

		channel.onmessage = (event: MessageEvent) => {
			this.emit('message', event.data as string);
		};

		channel.onerror = (err) => {
			console.error('[DataChannel] Channel error:', err);
		};
	}

	/**
	 * Handle incoming signaling messages from the WebSocket.
	 */
	private _onSignalMessage = (msg: WSMessage): void => {
		if (typeof msg !== 'object' || msg === null) return;
		const payload = msg as unknown as Record<string, unknown>;

		switch (payload.type) {
			case 'webrtc_offer':
			case 'offer':
				this._handleRemoteOffer(payload);
				break;

			case 'webrtc_answer':
			case 'answer':
				this._handleRemoteAnswer(payload);
				break;

			case 'webrtc_candidate':
			case 'candidate':
				this._handleRemoteCandidate(payload);
				break;
		}
	};

	/**
	 * Process a received SDP offer.
	 */
	private async _handleRemoteOffer(
		payload: Record<string, unknown>,
	): Promise<void> {
		if (!this.pc) {
			// We are the answering side — create a PC first
			try {
				this.pc = new RTCPeerConnection(this.config.rtcConfig);

				this.pc.onicecandidate = (event) => {
					if (event.candidate) {
						this._sendSignaling({
							type: 'candidate',
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

				this.pc.ondatachannel = (event) => {
					this._setupChannel(event.channel);
				};

				// Flush any pending candidates
				for (const candidate of this.pendingCandidates) {
					await this.pc.addIceCandidate(
						new RTCIceCandidate(candidate),
					);
				}
				this.pendingCandidates = [];
			} catch (err) {
				console.error('[DataChannel] Failed to create answering PC:', err);
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
				type: 'answer',
				sdp: this.pc.localDescription!.sdp,
			});
		} catch (err) {
			console.error('[DataChannel] Failed to handle offer:', err);
		}
	}

	/**
	 * Process a received SDP answer.
	 */
	private async _handleRemoteAnswer(
		payload: Record<string, unknown>,
	): Promise<void> {
		if (!this.pc) return;
		try {
			const sdp = payload.sdp as string;
			await this.pc.setRemoteDescription(
				new RTCSessionDescription({ type: 'answer', sdp }),
			);
		} catch (err) {
			console.error('[DataChannel] Failed to handle answer:', err);
		}
	}

	/**
	 * Process a received ICE candidate.
	 */
	private async _handleRemoteCandidate(
		payload: Record<string, unknown>,
	): Promise<void> {
		const candidate = payload.candidate as RTCIceCandidateInit;
		if (!this.pc || !this.pc.remoteDescription) {
			// Queue until we have a remote description
			this.pendingCandidates.push(candidate);
			return;
		}
		try {
			await this.pc.addIceCandidate(new RTCIceCandidate(candidate));
		} catch (err) {
			console.error('[DataChannel] Failed to add ICE candidate:', err);
		}
	}

	/**
	 * Send a signaling message over the existing WebSocket.
	 */
	private _sendSignaling(payload: Record<string, unknown>): void {
		this.signal.send({ type: 'signaling', ...payload });
	}

	/**
	 * Called when WebRTC fails — fall back to WebSocket if available.
	 */
	private _onRtcFailure(): void {
		this._closeRTC();

		if (this.fallback) {
			console.info('[DataChannel] Falling back to WebSocket transport');
			this._fallbackConnect();
		} else {
			this._setState('failed');
		}
	}

	/**
	 * Tear down the peer connection and data channel.
	 */
	private _closeRTC(): void {
		if (this.channel) {
			this.channel.onopen = null;
			this.channel.onclose = null;
			this.channel.onmessage = null;
			this.channel.onerror = null;
			this.channel.close();
			this.channel = null;
		}
		if (this.pc) {
			this.pc.onicecandidate = null;
			this.pc.oniceconnectionstatechange = null;
			this.pc.ondatachannel = null;
			this.pc.close();
			this.pc = null;
		}
		this.pendingCandidates = [];
	}

	/**
	 * Update internal state and emit state-change event.
	 */
	private _setState(newState: DataChannelState): void {
		if (this.state === newState) return;
		this._stateValue = newState;
		this.emit('state', newState);

		if (newState === 'connected') {
			this.emit('connect');
		} else if (newState === 'disconnected' || newState === 'failed') {
			this.emit('disconnect');
		}
	}
}

/**
 * Convenience guard: is the current environment WebRTC-capable?
 */
export function isWebRTCSupported(): boolean {
	return (
		typeof RTCPeerConnection !== 'undefined' &&
		typeof RTCDataChannel !== 'undefined'
	);
}
