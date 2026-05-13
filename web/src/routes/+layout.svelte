<script lang="ts">
	import { onMount } from 'svelte';
	import { dev } from '$app/environment';
	import { base } from '$app/paths';
	import '../app.css';
	import MenuBar from '$lib/components/MenuBar.svelte';
	import NavBar from '$lib/components/NavBar.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import ThemeProvider from '$lib/components/ThemeProvider.svelte';

	let { children }: { children: () => any } = $props();

	// Register service worker for PWA support (client-side only)
	onMount(() => {
		if (!dev && 'serviceWorker' in navigator) {
			navigator.serviceWorker.register(`${base}/service-worker.js`);
		}
	});
</script>

<ThemeProvider>
	<div class="flex h-screen flex-col" style="background-color: var(--color-bg-primary); color: var(--color-text-primary);">
		<MenuBar />
		<NavBar />
		<main class="flex-1 overflow-auto p-4">
			{@render children()}
		</main>
		<StatusBar />
	</div>
</ThemeProvider>
