<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import {
		initThemeSystem,
		subscribe,
		getThemeState,
		resolveThemeVariables,
		applyThemeToDocument,
		applyThemeModeClass,
		type ThemeState,
		type ThemeMode
	} from '$lib/theme';

	let themeState = $state<ThemeState>(getThemeState());
	let unsubscribe: (() => void) | null = null;

	onMount(() => {
		// Initialize theme on mount
		initThemeSystem();

		// Subscribe to changes from other tabs/components
		unsubscribe = subscribe((state) => {
			themeState = state;
			const variables = resolveThemeVariables(state);
			applyThemeModeClass(state);
			applyThemeToDocument(variables);
		});
	});

	onDestroy(() => {
		unsubscribe?.();
	});

	/**
	 * Expose theme control functions for child components via events or context.
	 * ThemeProvider uses a simple reactive pattern — any component can import
	 * theme.ts directly and call toggleDarkLight() etc.
	 */
</script>

{@render children()}
