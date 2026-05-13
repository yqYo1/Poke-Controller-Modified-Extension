<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import type { Snippet } from 'svelte';
	import {
		initThemeSystem,
		subscribe,
		resolveThemeVariables,
		applyThemeToDocument,
		applyThemeModeClass
	} from '$lib/theme';

	let {
		children
	}: {
		children: Snippet;
	} = $props();

	let unsubscribe: (() => void) | null = null;

	onMount(() => {
		// Initialize theme on mount
		initThemeSystem();

		// Subscribe to changes from other tabs/components
		unsubscribe = subscribe((state) => {
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
