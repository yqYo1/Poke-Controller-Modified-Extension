<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type OutputSettings } from '$lib/api/client';

	interface Props {
		onChange?: (ratio: number) => void;
	}

	let { onChange }: Props = $props();

	let splitRatio = $state(50);
	let loading = $state(true);

	onMount(() => {
		api.getOutputSettings()
			.then((settings: OutputSettings) => {
				splitRatio = settings.split_ratio;
				loading = false;
			})
			.catch(() => {
				loading = false;
			});
	});

	function handleChange(e: Event) {
		const target = e.target as HTMLInputElement;
		const val = parseInt(target.value, 10);
		splitRatio = val;
		api.updateOutputSettings({ split_ratio: val }).catch(console.warn);
		onChange?.(val);
	}

	function resetToDefault() {
		splitRatio = 50;
		api.updateOutputSettings({ split_ratio: 50 }).catch(console.warn);
		onChange?.(50);
	}
</script>

<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
	<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">出力サイズ調整</h3>
	<p class="mb-2 text-xs" style="color: var(--color-text-tertiary);">Output#1 / Output#2 の比率</p>

	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else}
		<div class="mb-3">
			<input
				type="range"
				min="0"
				max="100"
				step="1"
				value={splitRatio}
				oninput={handleChange}
				class="w-full accent-blue-500"
			/>
		</div>

		<!-- Visual preview of the split -->
		<div class="mb-2 flex h-16 w-full overflow-hidden rounded border" style="border-color: var(--color-border);">
			<div
				class="flex items-center justify-center text-xs font-medium transition-all duration-150"
				style="width: {splitRatio}%; background-color: var(--color-accent); color: var(--color-accent-text);"
			>
				{splitRatio > 10 ? `#1 ${splitRatio}%` : ''}
			</div>
			<div
				class="flex items-center justify-center text-xs font-medium transition-all duration-150"
				style="width: {100 - splitRatio}%; background-color: var(--color-bg-tertiary); color: var(--color-text-secondary);"
			>
				{100 - splitRatio > 10 ? `#2 ${100 - splitRatio}%` : ''}
			</div>
		</div>

		<div class="flex items-center justify-between text-xs" style="color: var(--color-text-secondary);">
			<span>Output#1: {splitRatio}%</span>
			<span>Output#2: {100 - splitRatio}%</span>
		</div>

		<button
			onclick={resetToDefault}
			class="mt-2 rounded px-2 py-1 text-xs"
			style="background-color: var(--color-bg-tertiary); color: var(--color-text-secondary);"
		>
			デフォルトにリセット (50/50)
		</button>
	{/if}
</div>
