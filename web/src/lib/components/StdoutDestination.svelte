<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type OutputSettings } from '$lib/api/client';

	let destination = $state<1 | 2>(1);
	let loading = $state(true);

	onMount(() => {
		api.getOutputSettings()
			.then((settings: OutputSettings) => {
				destination = settings.stdout_destination;
				loading = false;
			})
			.catch(() => {
				loading = false;
			});
	});

	async function toggleDest() {
		const newDest: 1 | 2 = destination === 1 ? 2 : 1;
		destination = newDest;
		try {
			await api.updateOutputSettings({ stdout_destination: newDest });
		} catch (e) {
			console.warn('Failed to update stdout destination', e);
		}
	}
</script>

<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
	<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">標準出力の出力先</h3>
	<p class="mb-2 text-xs" style="color: var(--color-text-tertiary);">stdout を表示するパネルを選択</p>

	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else}
		<div class="flex gap-2">
			<button
				onclick={toggleDest}
				class="flex flex-1 items-center justify-center gap-2 rounded px-3 py-2 text-xs font-medium transition-colors"
				style="background-color: {destination === 1 ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {destination === 1 ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
			>
				<span class="flex h-5 w-5 items-center justify-center rounded-full text-[10px]" style="background-color: {destination === 1 ? 'rgba(255,255,255,0.2)' : 'var(--color-border)'};">1</span>
				Output#1
			</button>
			<button
				onclick={toggleDest}
				class="flex flex-1 items-center justify-center gap-2 rounded px-3 py-2 text-xs font-medium transition-colors"
				style="background-color: {destination === 2 ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {destination === 2 ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
			>
				<span class="flex h-5 w-5 items-center justify-center rounded-full text-[10px]" style="background-color: {destination === 2 ? 'rgba(255,255,255,0.2)' : 'var(--color-border)'};">2</span>
				Output#2
			</button>
		</div>

		<div class="mt-2 flex items-center gap-2 text-xs" style="color: var(--color-text-tertiary);">
			<span class="inline-block h-2 w-2 rounded-full" style="background-color: {destination === 1 ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'};"></span>
			現在: Output#{destination}
		</div>
	{/if}
</div>