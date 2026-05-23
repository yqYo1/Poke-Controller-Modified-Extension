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

/** Currently selected widget mode (1-7). */
export let widgetMode = $state<WidgetModeNumber>(1);

/** Software-Controller position in the right panel. */
export let controllerPosition = $state<ControllerPosition>('bottom');

/** Output split ratio (0-100): percentage allocated to Output #1. */
export let splitRatio = $state<number>(50);

/** Destination panel for stdout log output. */
export let stdoutDestination = $state<StdoutDestination>(1);

/** Whether the store is still fetching initial values from the API. */
export let loading = $state<boolean>(true);

/** Non-null if initialisation (or a later API call) failed. */
export let error = $state<string | null>(null);

// ── Derived state ──────────────────────────────────────────────────────────

/** Resolved config for the currently active widget mode. */
export let currentWidgetConfig = $derived<WidgetModeConfig>(
	WIDGET_MODES[widgetMode] ?? WIDGET_MODES[1],
);

/** True once initialisation has completed (success or error). */
export let isInitialized = $derived<boolean>(!loading);

// ── Initialisation ─────────────────────────────────────────────────────────

/**
 * Initialise the store by fetching all UI settings from the backend API.
 *
 * Should be called once when the application mounts (e.g. in
 * `+layout.svelte`'s `onMount`).
 */
export async function initUIStore(): Promise<void> {
	loading = true;
	error = null;

	try {
		const [widgetSettings, outputSettings, controllerSettings] = await Promise.all([
			api.getWidgetMode(),
			api.getOutputSettings(),
			api.getControllerPosition(),
		]);

		const parsed = parseInt(widgetSettings.mode, 10) as WidgetModeNumber;
		if (parsed >= 1 && parsed <= 7) {
			widgetMode = parsed;
		}

		splitRatio = outputSettings.split_ratio;
		stdoutDestination = outputSettings.stdout_destination;
		controllerPosition = controllerSettings.position;
	} catch (e) {
		error = e instanceof Error ? e.message : String(e);
	} finally {
		loading = false;
	}
}

// ── Mutations (optimistic + persist) ───────────────────────────────────────

/**
 * Set the widget mode and persist to the backend.
 *
 * Updates the reactive store optimistically; reverts on API failure.
 */
export async function setWidgetMode(mode: WidgetModeNumber): Promise<void> {
	const previous = widgetMode;
	widgetMode = mode;
	try {
		await api.updateWidgetMode(String(mode));
	} catch (e) {
		widgetMode = previous;
		throw e;
	}
}

/**
 * Set the controller position and persist to the backend.
 */
export async function setControllerPosition(position: ControllerPosition): Promise<void> {
	const previous = controllerPosition;
	controllerPosition = position;
	try {
		await api.updateControllerPosition(position);
	} catch (e) {
		controllerPosition = previous;
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
	const previous = splitRatio;
	splitRatio = clamped;
	try {
		await api.updateOutputSettings({ split_ratio: clamped });
	} catch (e) {
		splitRatio = previous;
		throw e;
	}
}

/**
 * Set the stdout destination panel and persist to the backend.
 */
export async function setStdoutDestination(dest: StdoutDestination): Promise<void> {
	const previous = stdoutDestination;
	stdoutDestination = dest;
	try {
		await api.updateOutputSettings({ stdout_destination: dest });
	} catch (e) {
		stdoutDestination = previous;
		throw e;
	}
}
