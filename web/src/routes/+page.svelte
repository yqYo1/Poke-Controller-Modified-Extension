<script lang="ts">
	import { onMount } from 'svelte';
	import { browser } from '$app/environment';
	import { base } from '$app/paths';
	let status = $state('initializing');

	onMount(() => {
		// Attempt to auto-detect device and redirect accordingly
		// For desktop: serve as-is (already at /ui)
		// For mobile: future /mobile route (TODO)
		if (browser && window.innerWidth < 768) {
			// TODO: redirect to /mobile when mobile UI is implemented
			// goto('/mobile');
			console.debug('Mobile viewport detected — mobile UI pending');
		}
		status = 'ready';
	});
</script>

<svelte:head>
	<title>Poke-Controller</title>
</svelte:head>

<div class="container">
	<h1>Poke-Controller</h1>
	<p>Status: {status}</p>
	<nav>
		<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
		<a href="{base}/api/status">API Status</a>
		<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
		<a href="{base}/camera/stream">Camera Stream</a>
	</nav>
</div>

<style>
	.container {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		min-height: 100vh;
		gap: 1rem;
	}
</style>
