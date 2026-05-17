<svelte:options runes={true} />

<script lang="ts">
	import { onMount } from 'svelte';
	import type { Snippet } from 'svelte';
	import { page } from '$app/stores';
	import { dev } from '$app/environment';
	import { base } from '$app/paths';
	import '../app.css';
	import MenuBar from '$lib/components/MenuBar.svelte';
	import NavBar from '$lib/components/NavBar.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import ThemeProvider from '$lib/components/ThemeProvider.svelte';

	let { children }: { children: Snippet } = $props();

	// Whether we're on the main app page (not a sub-route)
	let isMainPage = $state(false);

	onMount(() => {
		// Register service worker for PWA support (client-side only)
		if (!dev && 'serviceWorker' in navigator) {
			navigator.serviceWorker.register(`${base}/service-worker.js`);
		}

		// Check if we're on the main page
		const unsubscribe = page.subscribe(p => {
			isMainPage = p.url.pathname === base || p.url.pathname === base + '/';
		});
		return unsubscribe;
	});
</script>

<ThemeProvider>
	<div class="tk-window">
		<MenuBar />
		{#if !isMainPage}
			<NavBar />
		{/if}
		<main class="tk-content">
			{@render children()}
		</main>
		<StatusBar />
	</div>
</ThemeProvider>

<style>
	.tk-window {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background-color: var(--color-bg-primary, #0f172a);
		color: var(--color-text-primary, #f1f5f9);
		overflow: hidden;
	}

	.tk-content {
		flex: 1;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}
</style>
