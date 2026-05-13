<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { browser } from '$app/environment';
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
		<a href="/api/status">API Status</a>
		<a href="/camera/stream">Camera Stream</a>
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
