/**
 * Theme System — Poke-Controller Web UI
 *
 * Extensible theme management with presets and user-defined themes.
 * Themes are defined via CSS custom properties and persisted in localStorage.
 */

// ── Types ─────────────────────────────────────────────────────────────────

export interface ThemeDefinition {
	/** Display name for the theme */
	name: string;
	/** Unique identifier */
	id: string;
	/** CSS custom property overrides (--color-* vars) */
	cssVariables: Record<string, string>;
}

export interface ThemePreset extends ThemeDefinition {
	/** Whether this is a built-in preset (can't be deleted) */
	builtIn: true;
}

export interface UserTheme extends ThemeDefinition {
	builtIn?: false;
}

export type AnyTheme = ThemePreset | UserTheme;

export type ThemeMode = 'light' | 'dark' | 'custom';

export interface ThemeState {
	/** Current mode: 'light', 'dark', or 'custom' */
	mode: ThemeMode;
	/** Currently active theme preset or custom theme ID */
	activeThemeId: string;
	/** User-defined custom themes */
	customThemes: UserTheme[];
}

// ── Storage keys ──────────────────────────────────────────────────────────

const STORAGE_KEY_THEME = 'pokecon-theme-state';

// ── Built-in presets ───────────────────────────────────────────────────────

const LIGHT_PRESET: ThemePreset = {
	name: 'ライト',
	id: 'light',
	builtIn: true,
	cssVariables: {
		'--color-bg-primary': '#f8f9fa',
		'--color-bg-secondary': '#ffffff',
		'--color-bg-tertiary': '#e9ecef',
		'--color-bg-card': '#ffffff',
		'--color-bg-input': '#ffffff',
		'--color-text-primary': '#212529',
		'--color-text-secondary': '#495057',
		'--color-text-tertiary': '#6c757d',
		'--color-text-inverse': '#f8f9fa',
		'--color-border': '#dee2e6',
		'--color-border-light': '#e9ecef',
		'--color-accent': '#3b82f6',
		'--color-accent-hover': '#2563eb',
		'--color-accent-light': '#dbeafe',
		'--color-accent-text': '#ffffff',
		'--color-success': '#22c55e',
		'--color-warning': '#f59e0b',
		'--color-error': '#ef4444',
		'--color-info': '#3b82f6',
		'--color-navbar-bg': '#ffffff',
		'--color-menubar-bg': '#f8f9fa',
		'--color-statusbar-bg': '#f8f9fa',
		'--color-overlay': 'rgba(0, 0, 0, 0.5)',
		'--color-surface-low': '#f8f9fa',
		'--color-surface-mid': '#e9ecef',
		'--color-surface-high': '#dee2e6'
	}
};

const DARK_PRESET: ThemePreset = {
	name: 'ダーク',
	id: 'dark',
	builtIn: true,
	cssVariables: {
		'--color-bg-primary': '#0f172a',
		'--color-bg-secondary': '#1e293b',
		'--color-bg-tertiary': '#334155',
		'--color-bg-card': '#1e293b',
		'--color-bg-input': '#1e293b',
		'--color-text-primary': '#f1f5f9',
		'--color-text-secondary': '#94a3b8',
		'--color-text-tertiary': '#64748b',
		'--color-text-inverse': '#0f172a',
		'--color-border': '#334155',
		'--color-border-light': '#1e293b',
		'--color-accent': '#60a5fa',
		'--color-accent-hover': '#3b82f6',
		'--color-accent-light': '#1e3a5f',
		'--color-accent-text': '#0f172a',
		'--color-success': '#4ade80',
		'--color-warning': '#fbbf24',
		'--color-error': '#f87171',
		'--color-info': '#60a5fa',
		'--color-navbar-bg': '#1e293b',
		'--color-menubar-bg': '#1e293b',
		'--color-statusbar-bg': '#1e293b',
		'--color-overlay': 'rgba(0, 0, 0, 0.7)',
		'--color-surface-low': '#1e293b',
		'--color-surface-mid': '#334155',
		'--color-surface-high': '#475569'
	}
};

/** All built-in presets */
export const BUILTIN_PRESETS: Record<string, ThemePreset> = {
	light: LIGHT_PRESET,
	dark: DARK_PRESET
};

// ── Reactive state (module-level singleton) ───────────────────────────────

let _onChangeCallbacks: Array<(state: ThemeState) => void> = [];

/**
 * Get the current theme state from localStorage (or default).
 * This is a snapshot; use `subscribe` for reactivity.
 */
export function getThemeState(): ThemeState {
	try {
		const raw = localStorage.getItem(STORAGE_KEY_THEME);
		if (raw) {
			const parsed = JSON.parse(raw) as Partial<ThemeState>;
			return {
				mode: parsed.mode ?? 'dark',
				activeThemeId: parsed.activeThemeId ?? 'dark',
				customThemes: parsed.customThemes ?? []
			};
		}
	} catch {
		// Ignore parse errors
	}
	return { mode: 'dark', activeThemeId: 'dark', customThemes: [] };
}

/**
 * Persist theme state to localStorage and notify subscribers.
 */
export function setThemeState(state: ThemeState): void {
	try {
		localStorage.setItem(
			STORAGE_KEY_THEME,
			JSON.stringify({
				mode: state.mode,
				activeThemeId: state.activeThemeId,
				customThemes: state.customThemes
			})
		);
	} catch {
		// localStorage may be unavailable
	}
	_onChangeCallbacks.forEach((cb) => cb(state));
}

/**
 * Subscribe to theme state changes. Returns an unsubscribe function.
 */
export function subscribe(callback: (state: ThemeState) => void): () => void {
	_onChangeCallbacks.push(callback);
	return () => {
		_onChangeCallbacks = _onChangeCallbacks.filter((cb) => cb !== callback);
	};
}

// ── Theme resolution ──────────────────────────────────────────────────────

/**
 * Resolve the full CSS variable map for a given theme state.
 * Returns a flat `Record<string, string>` of CSS custom properties.
 */
export function resolveThemeVariables(state: ThemeState): Record<string, string> {
	if (state.mode === 'custom') {
		// Look for a user-defined custom theme
		const custom = state.customThemes.find((t) => t.id === state.activeThemeId);
		if (custom) {
			return { ...custom.cssVariables };
		}
		// Fallback to dark if custom theme not found
		return { ...DARK_PRESET.cssVariables };
	}

	const preset = BUILTIN_PRESETS[state.mode];
	return preset ? { ...preset.cssVariables } : { ...DARK_PRESET.cssVariables };
}

/**
 * Get human-readable label for the current theme.
 */
export function getThemeLabel(state: ThemeState): string {
	if (state.mode === 'custom') {
		const custom = state.customThemes.find((t) => t.id === state.activeThemeId);
		return custom?.name ?? 'カスタム';
	}
	const preset = BUILTIN_PRESETS[state.mode];
	return preset?.name ?? 'Unknown';
}

/**
 * Apply CSS variables to the document root.
 */
export function applyThemeToDocument(variables: Record<string, string>): void {
	const root = document.documentElement;
	// Temporarily add class for smooth transitions
	root.classList.add('theme-transitioning');
	for (const [key, value] of Object.entries(variables)) {
		root.style.setProperty(key, value);
	}
	// Remove transitioning class after animation completes
	setTimeout(() => {
		root.classList.remove('theme-transitioning');
	}, 350);
}

/**
 * Apply theme mode class to document root.
 * 'light' themes get no special class (default).
 * 'dark' themes get the 'dark' class.
 * 'custom' themes get 'theme-<id>' class + 'theme-custom' class.
 */
export function applyThemeModeClass(state: ThemeState): void {
	const root = document.documentElement;
	// Remove existing theme classes
	root.classList.remove('dark', 'theme-custom');
	// Remove any theme-* classes
	for (const cls of Array.from(root.classList)) {
		if (cls.startsWith('theme-')) {
			root.classList.remove(cls);
		}
	}

	if (state.mode === 'dark') {
		root.classList.add('dark');
	} else if (state.mode === 'custom') {
		root.classList.add('theme-custom', `theme-${state.activeThemeId}`);
	}
	// light mode: no class needed
}

// ── Helper: toggle dark/light ─────────────────────────────────────────────

/**
 * Toggle between light and dark mode.
 */
export function toggleDarkLight(): ThemeState {
	const current = getThemeState();
	const newMode: ThemeMode = current.mode === 'dark' ? 'light' : 'dark';
	const newState: ThemeState = {
		...current,
		mode: newMode,
		activeThemeId: newMode
	};
	const variables = resolveThemeVariables(newState);
	applyThemeModeClass(newState);
	applyThemeToDocument(variables);
	setThemeState(newState);
	return newState;
}

// ── Custom theme management ───────────────────────────────────────────────

/**
 * Add a user-defined custom theme.
 */
export function addCustomTheme(theme: Omit<UserTheme, 'builtIn' | 'id'> & { id?: string }): ThemeState {
	const state = getThemeState();
	const id = theme.id ?? `custom-${Date.now()}`;
	const newTheme: UserTheme = {
		...theme,
		id,
		builtIn: false
	};
	state.customThemes.push(newTheme);
	setThemeState(state);
	return state;
}

/**
 * Remove a user-defined custom theme by ID.
 */
export function removeCustomTheme(id: string): ThemeState {
	const state = getThemeState();
	state.customThemes = state.customThemes.filter((t) => t.id !== id);
	if (state.activeThemeId === id) {
		state.mode = 'dark';
		state.activeThemeId = 'dark';
	}
	setThemeState(state);
	return state;
}

/**
 * Update a user-defined custom theme.
 */
export function updateCustomTheme(id: string, updates: Partial<UserTheme>): ThemeState {
	const state = getThemeState();
	const idx = state.customThemes.findIndex((t) => t.id === id);
	if (idx !== -1) {
		state.customThemes[idx] = { ...state.customThemes[idx], ...updates, id };
		setThemeState(state);
	}
	return state;
}

/**
 * Activate a theme by mode or custom theme ID.
 */
export function activateTheme(mode: ThemeMode, themeId?: string): ThemeState {
	const state = getThemeState();
	state.mode = mode;
	state.activeThemeId = themeId ?? mode;
	const variables = resolveThemeVariables(state);
	applyThemeModeClass(state);
	applyThemeToDocument(variables);
	setThemeState(state);
	return state;
}

/**
 * Initialize theme system. Call once on app startup.
 */
export function initThemeSystem(): ThemeState {
	const state = getThemeState();
	const variables = resolveThemeVariables(state);
	applyThemeModeClass(state);
	applyThemeToDocument(variables);
	return state;
}
