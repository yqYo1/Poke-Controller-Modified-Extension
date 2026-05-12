<script lang="ts">
	import NavBar from '$lib/components/NavBar.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';

	let { children } = $props();
</script>

<div class="flex h-screen flex-col bg-gray-950 text-gray-100">
	<NavBar />
	<main class="flex-1 overflow-auto p-4">
		{@render children()}
	</main>
	<StatusBar />
</div>
