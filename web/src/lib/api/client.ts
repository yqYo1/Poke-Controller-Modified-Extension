import { env } from '$env/dynamic/public';

// ─── Re-export typed WebSocket client ───────────────────────────────────────
export {
	type LogLevel,
	type LogMessage,
	type StatusMessage,
	type FrameMessage,
	type CommandMessage,
	type SerialMessage,
	type SerialDataMessage,
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

export interface GamepadInfo {
	index: number;
	name: string;
	connected: boolean;
}

export interface ControllerStatus {
	connected: boolean;
	vibration_enabled: boolean;
	recording: boolean;
}

export interface WindowsNotificationSettings {
	enabled: boolean;
	duration_secs: number;
	sound_enabled: boolean;
	priority: string;
	app_id: string;
}

export interface DiscordNotificationSettings {
	enabled: boolean;
	webhook_url: string;
	username: string;
	avatar_url: string;
}

export interface OutputSettings {
	split_ratio: number; // 0-100, percentage for Output#1
	stdout_destination: 1 | 2;
}

export interface WidgetSettings {
	mode: string;
}

export interface ControllerPositionSettings {
	position: 'top' | 'bottom';
}

export interface DialogueButtonPositionSettings {
	position: 'top' | 'bottom' | 'both';
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

	// ── Controller status / gamepad management (hardware control) ──────────

	getControllerStatus() {
		return this._fetch<ControllerStatus>('/api/controller/status');
	}

	gamepadList() {
		return this._fetch<GamepadInfo[]>('/api/controller/gamepads');
	}

	gamepadConnect(type: string) {
		return this._fetch<SuccessResponse>('/api/controller/connect', {
			method: 'POST',
			body: JSON.stringify({ type }),
		});
	}

	gamepadDisconnect() {
		return this._fetch<SuccessResponse>('/api/controller/disconnect', { method: 'POST' });
	}

	startRecording() {
		return this._fetch<SuccessResponse>('/api/controller/recording/start', { method: 'POST' });
	}

	stopRecording() {
		return this._fetch<SuccessResponse>('/api/controller/recording/stop', { method: 'POST' });
	}

	setVibrationEnabled(enabled: boolean) {
		return this._fetch<SuccessResponse>('/api/controller/vibration', {
			method: 'POST',
			body: JSON.stringify({ enabled }),
		});
	}

	// ── Keyboard convenience methods ───────────────────────────────────────

	enableKeyboard() {
		return this.setKeyboardEnabled(true);
	}

	disableKeyboard() {
		return this.setKeyboardEnabled(false);
	}

	// ── Windows notification settings ─────────────────────────────────────

	getWindowsNotificationSettings() {
		return this._fetch<WindowsNotificationSettings>('/api/notifications/windows');
	}

	updateWindowsNotificationSettings(settings: WindowsNotificationSettings) {
		return this._fetch<SuccessResponse>('/api/notifications/windows', {
			method: 'POST',
			body: JSON.stringify(settings),
		});
	}

	// ── Discord notification settings ─────────────────────────────────────

	getDiscordNotificationSettings() {
		return this._fetch<DiscordNotificationSettings>('/api/notifications/discord');
	}

	updateDiscordNotificationSettings(settings: DiscordNotificationSettings) {
		return this._fetch<SuccessResponse>('/api/notifications/discord', {
			method: 'POST',
			body: JSON.stringify(settings),
		});
	}

	// ── Output settings ────────────────────────────────────────────────────

	getOutputSettings() {
		return this._fetch<OutputSettings>('/api/settings/output');
	}
	updateOutputSettings(data: Partial<OutputSettings>) {
		return this._fetch<SuccessResponse>('/api/settings/output', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}

	// ── Widget mode ────────────────────────────────────────────────────────

	getWidgetMode() {
		return this._fetch<WidgetSettings>('/api/settings/widget');
	}
	updateWidgetMode(mode: string) {
		return this._fetch<SuccessResponse>('/api/settings/widget', {
			method: 'POST',
			body: JSON.stringify({ mode }),
		});
	}

	// ── Software controller position ──────────────────────────────────────

	getControllerPosition() {
		return this._fetch<ControllerPositionSettings>('/api/settings/controller/position');
	}
	updateControllerPosition(position: 'top' | 'bottom') {
		return this._fetch<SuccessResponse>('/api/settings/controller/position', {
			method: 'POST',
			body: JSON.stringify({ position }),
		});
	}

	// ── Dialogue button position ───────────────────────────────────────────

	getDialogueButtonPosition() {
		return this._fetch<DialogueButtonPositionSettings>('/api/settings/dialogue');
	}
	updateDialogueButtonPosition(position: 'top' | 'bottom' | 'both') {
		return this._fetch<SuccessResponse>('/api/settings/dialogue', {
			method: 'POST',
			body: JSON.stringify({ position }),
		});
	}
}

export const api = new APIClient();
