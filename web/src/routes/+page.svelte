<script lang="ts">
	import { onMount } from 'svelte';

	let status = $state('initializing');

	onMount(() => {
		// TODO: detect appropriate UI and redirect
		// /ui - desktop UI
		// /mobile - mobile UI (future)
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
		<a href="/ui">Open UI</a>
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
