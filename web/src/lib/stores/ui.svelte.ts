/**
 * UI State Store — Poke-Controller Web UI
 *
 * Centralised reactive state for widget mode, output panels, and
 * software-controller position. Uses Svelte 5 runes for reactivity
 * and syncs all mutations back to the backend API.
 *
 * @see {@link /SPECIFICATION.md §3 | Widget Modes}
 * @see {@link /SPECIFICATION.md §2.3 | Right Side Panel}
 */

import { api } from '$lib/api/client';

// ── Types ───────────────────────────────────────────────────────────────────

/** Numeric identifier for the 7 widget modes defined in the spec. */
export type WidgetModeNumber = 1 | 2 | 3 | 4 | 5 | 6 | 7;

/** Visibility configuration for a single widget mode. */
export interface WidgetModeConfig {
	/** Numeric mode identifier (1-7). */
	mode: WidgetModeNumber;
	/** Whether the Software Controller panel is visible. */
	showController: boolean;
	/** Whether Output #1 panel is visible. */
	showOutput1: boolean;
	/** Whether Output #2 panel is visible. */
	showOutput2: boolean;
	/** Human-readable description of this mode. */
	description: string;
}

/** Software-Controller position within the right panel. */
export type ControllerPosition = 'top' | 'bottom';

/** Stdout log destination panel. */
export type StdoutDestination = 1 | 2;

/** Dialogue button position (TOP / BOTTOM / BOTH). */
export type DialogueButtonPos = 'top' | 'bottom' | 'both';

// ── Widget mode definitions (SPECIFICATION.md §3) ──────────────────────────

/**
 * All 7 widget modes.
 *
 * | Mode | Software Controller | Output #1 | Output #2 | Description           |
 * |------|--------------------|-----------|-----------|-----------------------|
 * | 1    | Show              | Show      | Show      | Full panel (default)  |
 * | 2    | Show              | Show      | Hide      | Single output         |
 * | 3    | Show              | Hide      | Show      | Single output (swap)  |
 * | 4    | Hide              | Show      | Show      | Outputs only          |
 * | 5    | Show              | Hide      | Hide      | Controller only       |
 * | 6    | Hide              | Show      | Hide      | Output #1 only        |
 * | 7    | Hide              | Hide      | Show      | Output #2 only        |
 */
export const WIDGET_MODES: Record<WidgetModeNumber, WidgetModeConfig> = {
	1: { mode: 1, showController: true, showOutput1: true, showOutput2: true, description: 'Full panel' },
	2: { mode: 2, showController: true, showOutput1: true, showOutput2: false, description: 'Single output' },
	3: { mode: 3, showController: true, showOutput1: false, showOutput2: true, description: 'Single output (swapped)' },
	4: { mode: 4, showController: false, showOutput1: true, showOutput2: true, description: 'Outputs only' },
	5: { mode: 5, showController: true, showOutput1: false, showOutput2: false, description: 'Controller only' },
	6: { mode: 6, showController: false, showOutput1: true, showOutput2: false, description: 'Output #1 only' },
	7: { mode: 7, showController: false, showOutput1: false, showOutput2: true, description: 'Output #2 only' },
} as const;

/** Ordered list of all widget modes (convenience for iteration). */
export const WIDGET_MODE_LIST: WidgetModeConfig[] = [
	WIDGET_MODES[1],
	WIDGET_MODES[2],
	WIDGET_MODES[3],
	WIDGET_MODES[4],
	WIDGET_MODES[5],
	WIDGET_MODES[6],
	WIDGET_MODES[7],
];

// ── Reactive state (Svelte 5 runes) ────────────────────────────────────────

/**
 * Internal reactive state container.
 *
 * We use a single `$state` object whose properties are *mutated* (never
 * reassigned on the container itself).  The mutated object is exported as
 * `uiState` so consumers read its properties reactively inside Svelte
 * component templates or `$derived` expressions.
 *
 * We avoid exporting `$derived()` values from this module because Svelte 5
 * forbids `export $derived()` in `.svelte.ts` files (build-time error:
 * `derived_invalid_export`).
 */
const _state = $state({
	/** Currently selected widget mode (1-7). */
	widgetMode: 1 as WidgetModeNumber,
	/** Software-Controller position in the right panel. */
	controllerPosition: 'bottom' as ControllerPosition,
	/** Output split ratio (0-100): percentage allocated to Output #1. */
	splitRatio: 50,
	/** Destination panel for stdout log output. */
	stdoutDestination: 1 as StdoutDestination,
	/** Dialogue button placement position. */
	dialogueButtonPosition: 'both' as DialogueButtonPos,
	/** Whether the store is still fetching initial values from the API. */
	loading: true,
	/** Non-null if initialisation (or a later API call) failed. */
	error: null as string | null,
});

/**
 * Exported reactive UI state object.
 *
 * Read properties reactively in Svelte component templates or `$derived()`
 * expressions, e.g. `uiState.widgetMode`, `uiState.controllerPosition`.
 * For derived computations (e.g. `currentWidgetConfig`), compute them
 * locally in the component with `$derived(...)`.
 */
export const uiState = _state;

// ── Initialisation ─────────────────────────────────────────────────────────

/**
 * Initialise the store by fetching all UI settings from the backend API.
 *
 * Should be called once when the application mounts (e.g. in
 * `+layout.svelte`'s `onMount`).
 */
export async function initUIStore(): Promise<void> {
	_state.loading = true;
	_state.error = null;

	try {
		const [widgetSettings, outputSettings, controllerSettings, dialogueSettings] = await Promise.all([
			api.getWidgetMode(),
			api.getOutputSettings(),
			api.getControllerPosition(),
			api.getDialogueButtonPosition(),
		]);

		const parsed = parseInt(widgetSettings.mode, 10) as WidgetModeNumber;
		if (parsed >= 1 && parsed <= 7) {
			_state.widgetMode = parsed;
		}

		_state.splitRatio = outputSettings.split_ratio;
		_state.stdoutDestination = outputSettings.stdout_destination;
		_state.controllerPosition = controllerSettings.position;
		_state.dialogueButtonPosition = dialogueSettings.position;
	} catch (e) {
		_state.error = e instanceof Error ? e.message : String(e);
	} finally {
		_state.loading = false;
	}
}

// ── Mutations (optimistic + persist) ───────────────────────────────────────

/**
 * Set the widget mode and persist to the backend.
 *
 * Updates the reactive store optimistically; reverts on API failure.
 */
export async function setWidgetMode(mode: WidgetModeNumber): Promise<void> {
	const previous = _state.widgetMode;
	_state.widgetMode = mode;
	try {
		await api.updateWidgetMode(String(mode));
	} catch (e) {
		_state.widgetMode = previous;
		throw e;
	}
}

/**
 * Set the controller position and persist to the backend.
 */
export async function setControllerPosition(position: ControllerPosition): Promise<void> {
	const previous = _state.controllerPosition;
	_state.controllerPosition = position;
	try {
		await api.updateControllerPosition(position);
	} catch (e) {
		_state.controllerPosition = previous;
		throw e;
	}
}

/**
 * Set the output split ratio (0-100) and persist to the backend.
 *
 * The value is automatically clamped to 0-100 and rounded to an integer.
 */
export async function setSplitRatio(ratio: number): Promise<void> {
	const clamped = Math.max(0, Math.min(100, Math.round(ratio)));
	const previous = _state.splitRatio;
	_state.splitRatio = clamped;
	try {
		await api.updateOutputSettings({ split_ratio: clamped });
	} catch (e) {
		_state.splitRatio = previous;
		throw e;
	}
}

/**
 * Set the stdout destination panel and persist to the backend.
 */
export async function setStdoutDestination(dest: StdoutDestination): Promise<void> {
	const previous = _state.stdoutDestination;
	_state.stdoutDestination = dest;
	try {
		await api.updateOutputSettings({ stdout_destination: dest });
	} catch (e) {
		_state.stdoutDestination = previous;
		throw e;
	}
}

/**
 * Set the dialogue button position and persist to the backend.
 */
export async function setDialogueButtonPosition(pos: DialogueButtonPos): Promise<void> {
	const previous = _state.dialogueButtonPosition;
	_state.dialogueButtonPosition = pos;
	try {
		await api.updateDialogueButtonPosition(pos);
	} catch (e) {
		_state.dialogueButtonPosition = previous;
		throw e;
	}
}
