/**
 * Typed WebSocket client with automatic reconnection (exponential backoff)
 * and event-based listener pattern.
 *
 * @module websocket
 */

// ─── Typed Messages ─────────────────────────────────────────────────────────

export type LogLevel = 'info' | 'warn' | 'error' | 'debug';

export interface LogMessage {
	type: 'log';
	timestamp: string;
	level: LogLevel;
	message: string;
}

export interface StatusMessage {
	type: 'status';
	camera: boolean;
	serial: boolean;
	ws_connected: boolean;
}

export interface FrameMessage {
	type: 'frame';
	data: string;
}

export interface CommandMessage {
	type: 'command';
	name: string;
	running: boolean;
}

export interface SerialMessage {
	type: 'serial';
	connected: boolean;
	port_name?: string;
	baudrate?: number;
}

export interface CameraMessage {
	type: 'camera';
	opened: boolean;
	device_index?: number;
}

/**
 * Discriminated union of all known WebSocket message types.
 */
export type WSMessage =
	| LogMessage
	| StatusMessage
	| FrameMessage
	| CommandMessage
	| SerialMessage
	| CameraMessage;

// ─── Event System ───────────────────────────────────────────────────────────

export interface WebSocketEventMap {
	connect: void;
	disconnect: void;
	message: WSMessage;
}

type EventCallback = (...args: any[]) => void;

// ─── WebSocket Client ───────────────────────────────────────────────────────

export class WebSocketClient {
	private ws: WebSocket | null = null;
	private url: string;
	private shouldReconnect = true;
	private reconnectAttempts = 0;
	private readonly baseDelay = 1000; // 1 second
	private readonly maxDelay = 30000; // 30 seconds
	private readonly maxRetries = 10;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private listeners = new Map<string, Set<EventCallback>>();

	/**
	 * @param url - WebSocket endpoint URL. Defaults to `ws[s]://<host>/ws`.
	 */
	constructor(url?: string) {
		if (url) {
			this.url = url;
		} else if (typeof location !== 'undefined') {
			const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
			this.url = `${protocol}//${location.host}/ws`;
		} else {
			this.url = 'ws://localhost/ws';
		}
	}

	// ── Public event API ──────────────────────────────────────────────────

	/**
	 * Register an event listener.
	 *
	 * @param event - Event name (`'connect'`, `'disconnect'`, `'message'`)
	 * @param listener - Callback invoked when the event fires
	 */
	on(event: 'connect', listener: () => void): void;
	on(event: 'disconnect', listener: () => void): void;
	on(event: 'message', listener: (msg: WSMessage) => void): void;
	on(event: string, listener: EventCallback): void {
		if (!this.listeners.has(event)) {
			this.listeners.set(event, new Set());
		}
		this.listeners.get(event)!.add(listener);
	}

	/**
	 * Remove a previously registered event listener.
	 *
	 * @param event - Event name
	 * @param listener - The same function reference passed to `on()`
	 */
	off(event: 'connect', listener: () => void): void;
	off(event: 'disconnect', listener: () => void): void;
	off(event: 'message', listener: (msg: WSMessage) => void): void;
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
	 * Open (or re-open) the WebSocket connection.
	 * Resets the reconnection attempt counter.
	 */
	connect(): void {
		this.shouldReconnect = true;
		this.reconnectAttempts = 0;
		this._connect();
	}

	private _connect(): void {
		if (
			this.ws &&
			(this.ws.readyState === WebSocket.OPEN ||
				this.ws.readyState === WebSocket.CONNECTING)
		) {
			return;
		}

		try {
			this.ws = new WebSocket(this.url);

			this.ws.onopen = () => {
				this.reconnectAttempts = 0;
				this.emit('connect');
			};

			this.ws.onmessage = (event: MessageEvent) => {
				try {
					const data = JSON.parse(event.data) as WSMessage;
					this.emit('message', data);
				} catch (e) {
					console.warn('WebSocket parse error:', e);
				}
			};

			this.ws.onclose = () => {
				this.ws = null;
				this.emit('disconnect');
				this._scheduleReconnect();
			};

			this.ws.onerror = () => {
				// `onclose` fires after `onerror`, so reconnection is
				// handled in the close handler.
			};
		} catch (err) {
			console.error('WebSocket connection error:', err);
			this.ws = null;
			this._scheduleReconnect();
		}
	}

	private _scheduleReconnect(): void {
		if (!this.shouldReconnect) return;

		if (this.reconnectAttempts >= this.maxRetries) {
			console.warn(
				'WebSocket max reconnect attempts reached',
				this.maxRetries,
			);
			return;
		}

		// Exponential back-off with jitter (±25 %)
		const delay = Math.min(
			this.baseDelay * 2 ** this.reconnectAttempts,
			this.maxDelay,
		);
		const jitter = delay * (0.75 + Math.random() * 0.5);

		this.reconnectAttempts++;
		console.info(
			`WebSocket reconnecting in ${Math.round(jitter)} ms (attempt ${this.reconnectAttempts})`,
		);

		this.reconnectTimer = setTimeout(() => {
			this._connect();
		}, jitter);
	}

	/**
	 * Close the WebSocket connection and stop reconnection attempts.
	 */
	disconnect(): void {
		this.shouldReconnect = false;
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		if (this.ws) {
			// Prevent the close handler from triggering reconnection
			this.ws.onclose = null;
			this.ws.close();
			this.ws = null;
		}
		this.emit('disconnect');
	}

	/**
	 * Send a JSON-serialisable payload over the WebSocket.
	 *
	 * @returns `true` if the message was queued for sending
	 */
	send(data: Record<string, unknown>): boolean {
		if (this.ws && this.ws.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify(data));
			return true;
		}
		return false;
	}

	/** `true` when the underlying WebSocket is in the OPEN state. */
	get connected(): boolean {
		return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
	}

	/** Current reconnection attempt count (0 = no reconnection in progress). */
	get attempt(): number {
		return this.reconnectAttempts;
	}
}

/** Pre-configured singleton WebSocket client instance. */
export const wsClient = new WebSocketClient();

// ─── Legacy compatibility: callback-based API ───────────────────────────────
// The following exports match the previous `WebSocketClient` interface so that
// existing components (e.g. StatusBar.svelte) continue to work without changes.

/** @deprecated Use `WebSocketClient` (event-based) instead. */
export class LegacyWebSocketClient {
	ws: WebSocket | null = null;
	onOpen: (() => void) | null = null;
	onClose: (() => void) | null = null;
	onMessage: ((data: Record<string, unknown>) => void) | null = null;
	reconnectInterval = 3000;
	shouldReconnect = true;

	constructor(public url: string) {}

	connect(): void {
		this.shouldReconnect = true;
		this._connect();
	}

	private _connect(): void {
		try {
			this.ws = new WebSocket(this.url);
			this.ws.onopen = () => {
				if (this.onOpen) this.onOpen();
			};
			this.ws.onmessage = (event: MessageEvent) => {
				try {
					const data = JSON.parse(event.data) as Record<string, unknown>;
					if (this.onMessage) this.onMessage(data);
				} catch (e) {
					console.warn('WS parse error:', e);
				}
			};
			this.ws.onclose = () => {
				if (this.onClose) this.onClose();
				if (this.shouldReconnect) {
					setTimeout(() => this._connect(), this.reconnectInterval);
				}
			};
			this.ws.onerror = (err: Event) => {
				console.error('WS error:', err);
			};
		} catch (err) {
			console.error('WS connect error:', err);
			if (this.shouldReconnect) {
				setTimeout(() => this._connect(), this.reconnectInterval);
			}
		}
	}

	disconnect(): void {
		this.shouldReconnect = false;
		if (this.ws) {
			this.ws.close();
			this.ws = null;
		}
	}

	send(data: Record<string, unknown>): void {
		if (this.ws && this.ws.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify(data));
		}
	}
}
