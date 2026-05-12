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

// ─── Import auto-generated OpenAPI types ────────────────────────────────────
import type { components } from './types';

// ─── Convenience type aliases (request schemas from generated types) ─────────

/** @see {@link components["schemas"]["CameraConfigRequest"]} */
export type CameraConfigRequest = components['schemas']['CameraConfigRequest'];

/** @see {@link components["schemas"]["CameraOpenRequest"]} */
export type CameraOpenRequest = components['schemas']['CameraOpenRequest'];

/** @see {@link components["schemas"]["CaptureRequest"]} */
export type CaptureRequest = components['schemas']['CaptureRequest'];

/** @see {@link components["schemas"]["ControllerTypeRequest"]} */
export type ControllerTypeRequest = components['schemas']['ControllerTypeRequest'];

/** @see {@link components["schemas"]["FilterRequest"]} */
export type FilterRequest = components['schemas']['FilterRequest'];

/** @see {@link components["schemas"]["HoldRequest"]} */
export type HoldRequest = components['schemas']['HoldRequest'];

/** @see {@link components["schemas"]["KeyboardRequest"]} */
export type KeyboardRequest = components['schemas']['KeyboardRequest'];

/** @see {@link components["schemas"]["MouseStickConfig"]} */
export type MouseStickConfig = components['schemas']['MouseStickConfig'];

/** @see {@link components["schemas"]["MouseStickRequest"]} */
export type MouseStickRequest = components['schemas']['MouseStickRequest'];

/** @see {@link components["schemas"]["NameRequest"]} */
export type NameRequest = components['schemas']['NameRequest'];

/** @see {@link components["schemas"]["NotificationConfig"]} */
export type NotificationConfig = components['schemas']['NotificationConfig'];

/** @see {@link components["schemas"]["NotificationConfigRequest"]} */
export type NotificationConfigRequest = components['schemas']['NotificationConfigRequest'];

/** @see {@link components["schemas"]["OpenRequest"]} */
export type OpenRequest = components['schemas']['OpenRequest'];

/** @see {@link components["schemas"]["PressRequest"]} */
export type PressRequest = components['schemas']['PressRequest'];

/** @see {@link components["schemas"]["SendNotificationRequest"]} */
export type SendNotificationRequest = components['schemas']['SendNotificationRequest'];

/** @see {@link components["schemas"]["SerialConfigRequest"]} */
export type SerialConfigRequest = components['schemas']['SerialConfigRequest'];

/** @see {@link components["schemas"]["StickRequest"]} */
export type StickRequest = components['schemas']['StickRequest'];

/** @see {@link components["schemas"]["TouchRequest"]} */
export type TouchRequest = components['schemas']['TouchRequest'];

/** @see {@link components["schemas"]["WriteRequest"]} */
export type WriteRequest = components['schemas']['WriteRequest'];

// ─── Manually-defined response types (utoipa does not emit response schemas) ─

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

export interface ControllerType {
	gamepad_type: string;
}

export interface KeyboardEnabled {
	enabled: boolean;
}

export interface CommandEntry {
	name: string;
	description?: string;
}

export interface ActiveCommand {
	name?: string;
	running: boolean;
}

export interface ProfileEntry {
	name: string;
}

/** Generic success envelope returned by most mutation endpoints. */
export interface SuccessResponse {
	success: boolean;
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

	// ── Status ─────────────────────────────────────────────────────────────

	getStatus() {
		return this._fetch<StatusResponse>('/api/status');
	}

	// ── Serial ─────────────────────────────────────────────────────────────

	getSerialPorts() {
		return this._fetch<SerialPort[]>('/api/serial/ports');
	}
	getSerialStatus() {
		return this._fetch<SerialStatus>('/api/serial/status');
	}
	openSerial(data: OpenRequest) {
		return this._fetch<SuccessResponse>('/api/serial/open', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
	closeSerial() {
		return this._fetch<SuccessResponse>('/api/serial/close', { method: 'POST' });
	}
	updateSerialConfig(config: SerialConfigRequest) {
		return this._fetch<SuccessResponse>('/api/serial/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}
	writeSerial(data: string) {
		return this._fetch<SuccessResponse>('/api/serial/write', {
			method: 'POST',
			body: JSON.stringify({ data } satisfies WriteRequest),
		});
	}

	// ── Camera ─────────────────────────────────────────────────────────────

	getCameras() {
		return this._fetch<CameraDevice[]>('/api/cameras');
	}
	getCameraStatus() {
		return this._fetch<CameraStatus>('/api/camera/status');
	}
	openCamera(data: CameraOpenRequest) {
		return this._fetch<SuccessResponse>('/api/camera/open', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
	closeCamera() {
		return this._fetch<SuccessResponse>('/api/camera/close', { method: 'POST' });
	}
	getCameraFrame() {
		return this._fetch<{ frame: string }>('/api/camera/frame');
	}
	captureCamera(filename: string) {
		return this._fetch<SuccessResponse>('/api/camera/capture', {
			method: 'POST',
			body: JSON.stringify({ filename } satisfies CaptureRequest),
		});
	}
	updateCameraConfig(config: CameraConfigRequest) {
		return this._fetch<SuccessResponse>('/api/camera/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}

	// ── Input ──────────────────────────────────────────────────────────────

	sendInput(type: string, params: Record<string, unknown>) {
		return this._fetch<SuccessResponse>(`/api/input/${type}`, {
			method: 'POST',
			body: JSON.stringify(params),
		});
	}

	// ── Controller settings ────────────────────────────────────────────────

	getControllerType() {
		return this._fetch<ControllerType>('/api/controller/type');
	}
	setControllerType(gamepad_type: string) {
		return this._fetch<SuccessResponse>('/api/controller/type', {
			method: 'POST',
			body: JSON.stringify({ gamepad_type } satisfies ControllerTypeRequest),
		});
	}
	getKeyboardEnabled() {
		return this._fetch<KeyboardEnabled>('/api/controller/keyboard');
	}
	setKeyboardEnabled(enabled: boolean) {
		return this._fetch<SuccessResponse>('/api/controller/keyboard', {
			method: 'POST',
			body: JSON.stringify({ enabled } satisfies KeyboardRequest),
		});
	}

	// ── Mouse stick control ────────────────────────────────────────────────

	getMouseStick() {
		return this._fetch<MouseStickConfig>('/api/controller/mouse_stick');
	}
	setMouseStick(config: MouseStickRequest) {
		return this._fetch<SuccessResponse>('/api/controller/mouse_stick', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}

	// ── Commands ───────────────────────────────────────────────────────────

	getCommands() {
		return this._fetch<CommandEntry[]>('/api/commands');
	}
	loadCommand(name: string) {
		return this._fetch<SuccessResponse>('/api/commands/load', {
			method: 'POST',
			body: JSON.stringify({ name } satisfies NameRequest),
		});
	}
	startCommand(name: string) {
		return this._fetch<SuccessResponse>('/api/commands/start', {
			method: 'POST',
			body: JSON.stringify({ name } satisfies NameRequest),
		});
	}
	stopCommand() {
		return this._fetch<SuccessResponse>('/api/commands/stop', { method: 'POST' });
	}
	getActiveCommand() {
		return this._fetch<ActiveCommand>('/api/commands/active');
	}

	// Command filter and reload
	filterCommands(filter: string) {
		return this._fetch<CommandEntry[]>('/api/commands/filter', {
			method: 'POST',
			body: JSON.stringify({ filter } satisfies FilterRequest),
		});
	}
	reloadCommands() {
		return this._fetch<SuccessResponse>('/api/commands/reload', { method: 'POST' });
	}

	// ── Profile management ────────────────────────────────────────────────

	getProfiles() {
		return this._fetch<ProfileEntry[]>('/api/profile');
	}
	setProfile(name: string) {
		return this._fetch<SuccessResponse>('/api/profile', {
			method: 'POST',
			body: JSON.stringify({ name } satisfies NameRequest),
		});
	}

	// ── Notification settings ──────────────────────────────────────────────

	getNotificationConfig() {
		return this._fetch<NotificationConfig>('/api/notifications/config');
	}
	updateNotificationConfig(config: NotificationConfigRequest) {
		return this._fetch<SuccessResponse>('/api/notifications/config', {
			method: 'POST',
			body: JSON.stringify(config),
		});
	}
	sendTestNotification(data: SendNotificationRequest) {
		return this._fetch<SuccessResponse>('/api/notifications/send', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}
}

export const api = new APIClient();
