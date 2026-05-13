<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type ControllerPositionSettings } from '$lib/api/client';

	let position = $state<'top' | 'bottom'>('bottom');
	let loading = $state(true);

	onMount(() => {
		api.getControllerPosition()
			.then((settings: ControllerPositionSettings) => {
				position = settings.position;
				loading = false;
			})
			.catch(() => {
				loading = false;
			});
	});

	async function setPosition(pos: 'top' | 'bottom') {
		position = pos;
		try {
			await api.updateControllerPosition(pos);
		} catch (e) {
			console.warn('Failed to update controller position', e);
		}
	}
</script>

<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
	<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">ソフトコントローラー位置</h3>
	<p class="mb-2 text-xs" style="color: var(--color-text-tertiary);">コントローラーの表示位置を選択</p>

	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else}
		<div class="flex gap-2">
			<button
				onclick={() => setPosition('top')}
				class="flex flex-1 flex-col items-center gap-1 rounded border px-3 py-2 text-xs font-medium transition-colors"
				style="border-color: {position === 'top' ? 'var(--color-accent)' : 'var(--color-border)'}; background-color: {position === 'top' ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {position === 'top' ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
			>
				<span class="text-sm">⬆</span>
				<span>上部</span>
			</button>
			<button
				onclick={() => setPosition('bottom')}
				class="flex flex-1 flex-col items-center gap-1 rounded border px-3 py-2 text-xs font-medium transition-colors"
				style="border-color: {position === 'bottom' ? 'var(--color-accent)' : 'var(--color-border)'}; background-color: {position === 'bottom' ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {position === 'bottom' ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
			>
				<span class="text-sm">⬇</span>
				<span>下部</span>
			</button>
		</div>

		<!-- Visual preview -->
		<div class="mt-3 flex h-20 w-full items-stretch overflow-hidden rounded border" style="border-color: var(--color-border);">
			{#if position === 'top'}
				<div class="flex w-full flex-col">
					<div class="flex items-center justify-center text-[10px]" style="height: 40%; background-color: var(--color-accent); color: var(--color-accent-text);">
						ソフトコントローラー（上部）
					</div>
					<div class="flex items-center justify-center text-[10px]" style="height: 60%; background-color: var(--color-bg-tertiary); color: var(--color-text-tertiary);">
						出力エリア
					</div>
				</div>
			{:else}
				<div class="flex w-full flex-col">
					<div class="flex items-center justify-center text-[10px]" style="height: 60%; background-color: var(--color-bg-tertiary); color: var(--color-text-tertiary);">
						出力エリア
					</div>
					<div class="flex items-center justify-center text-[10px]" style="height: 40%; background-color: var(--color-accent); color: var(--color-accent-text);">
						ソフトコントローラー（下部）
					</div>
				</div>
			{/if}
		</div>
	{/if}
</div>