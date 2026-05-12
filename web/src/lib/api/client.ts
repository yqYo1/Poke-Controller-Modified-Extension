import { env } from '$env/dynamic/public';

// ─── Re-export typed WebSocket client ───────────────────────────────────────
export {
	type LogLevel,
	type LogMessage,
	type StatusMessage,
	type FrameMessage,
	type CommandMessage,
	type SerialMessage,
	type CameraMessage,
	type WSMessage,
	type WebSocketEventMap,
	WebSocketClient,
	wsClient,
} from './websocket';

// ─── REST API types ─────────────────────────────────────────────────────────

export interface StatusResponse {
	camera: boolean;
	serial: boolean;
	ws_connected: boolean;
}

export interface SerialPort {
	port_num: number;
	port_name: string;
	description?: string;
}

export interface SerialStatus {
	connected: boolean;
	port_name?: string;
	baudrate?: number;
}

export interface SerialConfig {
	[key: string]: unknown;
}

export interface CameraDevice {
	device_index: number;
	name: string;
}

export interface CameraStatus {
	opened: boolean;
	device_index?: number;
	frame_width?: number;
	frame_height?: number;
}

export interface CameraConfig {
	[key: string]: unknown;
}

export interface ControllerType {
	gamepad_type: string;
}

export interface KeyboardEnabled {
	enabled: boolean;
}

export interface MouseStickConfig {
	stick: string;
	enabled: boolean;
	sensitivity: number;
}

export interface CommandEntry {
	name: string;
	description?: string;
}

export interface ActiveCommand {
	name?: string;
	running: boolean;
}

export interface NotificationConfig {
	enabled?: boolean;
	webhook_url?: string;
	[key: string]: unknown;
}

export interface ProfileEntry {
	name: string;
}

// ─── REST API client ────────────────────────────────────────────────────────

export class APIClient {
	constructor(public baseUrl: string = env.PUBLIC_API_URL ?? '') {}

	private async _fetch<T>(path: string, options: RequestInit = {}): Promise<T> {
		const url = `${this.baseUrl}${path}`;
		const resp = await fetch(url, {
			...options,
			headers: {
				'Content-Type': 'application/json',
				...options.headers,
			},
		});
		if (!resp.ok) {
			const err = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
			throw new Error(err.error || `HTTP ${resp.status}`);
		}
		return resp.json() as Promise<T>;
	}

	getStatus() { return this._fetch<StatusResponse>('/api/status'); }

	getSerialPorts() { return this._fetch<SerialPort[]>('/api/serial/ports'); }
	getSerialStatus() { return this._fetch<SerialStatus>('/api/serial/status'); }
	openSerial(data: { port_num: number; port_name: string; baudrate: number }) {
		return this._fetch<{ success: boolean }>('/api/serial/open', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
	closeSerial() { return this._fetch<{ success: boolean }>('/api/serial/close', { method: 'POST' }); }
	updateSerialConfig(config: SerialConfig) {
		return this._fetch<{ success: boolean }>('/api/serial/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}
	writeSerial(data: string) {
		return this._fetch<{ success: boolean }>('/api/serial/write', {
			method: 'POST',
			body: JSON.stringify({ data }),
		});
	}

	getCameras() { return this._fetch<CameraDevice[]>('/api/cameras'); }
	getCameraStatus() { return this._fetch<CameraStatus>('/api/camera/status'); }
	openCamera(data: { device_index: number; width: number; height: number }) {
		return this._fetch<{ success: boolean }>('/api/camera/open', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
	closeCamera() { return this._fetch<{ success: boolean }>('/api/camera/close', { method: 'POST' }); }
	getCameraFrame() { return this._fetch<{ frame: string }>('/api/camera/frame'); }
	captureCamera(filename: string) {
		return this._fetch<{ success: boolean }>('/api/camera/capture', {
			method: 'POST',
			body: JSON.stringify({ filename }),
		});
	}
	updateCameraConfig(config: CameraConfig) {
		return this._fetch<{ success: boolean }>('/api/camera/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}

	sendInput(type: string, params: Record<string, unknown>) {
		return this._fetch<{ success: boolean }>(`/api/input/${type}`, {
			method: 'POST',
			body: JSON.stringify(params),
		});
	}

	getControllerType() { return this._fetch<ControllerType>('/api/controller/type'); }
	setControllerType(gamepad_type: string) {
		return this._fetch<{ success: boolean }>('/api/controller/type', {
			method: 'POST',
			body: JSON.stringify({ gamepad_type }),
		});
	}
	getKeyboardEnabled() { return this._fetch<KeyboardEnabled>('/api/controller/keyboard'); }
	setKeyboardEnabled(enabled: boolean) {
		return this._fetch<{ success: boolean }>('/api/controller/keyboard', {
			method: 'POST',
			body: JSON.stringify({ enabled }),
		});
	}

	getMouseStick() { return this._fetch<MouseStickConfig>('/api/controller/mouse_stick'); }
	setMouseStick(config: { stick: string; enabled: boolean; sensitivity?: number }) {
		return this._fetch<{ success: boolean }>('/api/controller/mouse_stick', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}

	getCommands() { return this._fetch<CommandEntry[]>('/api/commands'); }
	loadCommand(name: string) {
		return this._fetch<{ success: boolean }>('/api/commands/load', {
			method: 'POST',
			body: JSON.stringify({ name }),
		});
	}
	startCommand(name: string) {
		return this._fetch<{ success: boolean }>('/api/commands/start', {
			method: 'POST',
			body: JSON.stringify({ name }),
		});
	}
	stopCommand() { return this._fetch<{ success: boolean }>('/api/commands/stop', { method: 'POST' }); }
	getActiveCommand() { return this._fetch<ActiveCommand>('/api/commands/active'); }

	filterCommands(filter: string) {
		return this._fetch<CommandEntry[]>('/api/commands/filter', {
			method: 'POST',
			body: JSON.stringify({ filter }),
		});
	}
	reloadCommands() {
		return this._fetch<{ success: boolean }>('/api/commands/reload', { method: 'POST' });
	}

	getProfiles() { return this._fetch<ProfileEntry[]>('/api/profile'); }
	setProfile(name: string) {
		return this._fetch<{ success: boolean }>('/api/profile', {
			method: 'POST',
			body: JSON.stringify({ name }),
		});
	}

	getNotificationConfig() { return this._fetch<NotificationConfig>('/api/notifications/config'); }
	updateNotificationConfig(config: NotificationConfig) {
		return this._fetch<{ success: boolean }>('/api/notifications/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}
	sendTestNotification(data: { message: string; title?: string }) {
		return this._fetch<{ success: boolean }>('/api/notifications/send', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
}

export const api = new APIClient();
