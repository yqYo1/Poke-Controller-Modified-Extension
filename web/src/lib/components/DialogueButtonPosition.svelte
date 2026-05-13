<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type DialogueButtonPositionSettings } from '$lib/api/client';

	let position = $state<'top' | 'bottom' | 'both'>('both');
	let loading = $state(true);

	onMount(() => {
		api.getDialogueButtonPosition()
			.then((settings: DialogueButtonPositionSettings) => {
				position = settings.position;
				loading = false;
			})
			.catch(() => {
				loading = false;
			});
	});

	async function setPosition(pos: 'top' | 'bottom' | 'both') {
		position = pos;
		try {
			await api.updateDialogueButtonPosition(pos);
		} catch (e) {
			console.warn('Failed to update dialogue button position', e);
		}
	}

	const options = [
		{ id: 'top' as const, label: '上部', icon: '⬆' },
		{ id: 'bottom' as const, label: '下部', icon: '⬇' },
		{ id: 'both' as const, label: '両方', icon: '↕' },
	];
</script>

<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
	<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">ダイアログボタン位置</h3>
	<p class="mb-2 text-xs" style="color: var(--color-text-tertiary);">ダイアログボタンの表示位置を選択</p>

	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else}
		<div class="flex gap-2">
			{#each options as opt (opt.id)}
				<button
					onclick={() => setPosition(opt.id)}
					class="flex flex-1 flex-col items-center gap-1 rounded border px-3 py-2 text-xs font-medium transition-colors"
					style="border-color: {position === opt.id ? 'var(--color-accent)' : 'var(--color-border)'}; background-color: {position === opt.id ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {position === opt.id ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
				>
					<span class="text-sm">{opt.icon}</span>
					<span>{opt.label}</span>
				</button>
			{/each}
		</div>

		<!-- Visual preview -->
		<div class="mt-3 flex h-16 w-full items-stretch overflow-hidden rounded border" style="border-color: var(--color-border);">
			<div class="flex w-full flex-col">
				{#if position === 'top' || position === 'both'}
					<div class="flex items-center justify-center text-[10px]" style="height: 50%; background-color: var(--color-accent); color: var(--color-accent-text); border-bottom: 1px solid var(--color-border);">
						ボタン（上部）
					</div>
				{/if}
				{#if position === 'bottom' || position === 'both'}
					<div class="flex items-center justify-center text-[10px]" style="height: 50%; background-color: var(--color-accent); color: var(--color-accent-text);">
						ボタン（下部）
					</div>
				{:else if position === 'top'}
					<div class="flex items-center justify-center text-[10px]" style="height: 50%; background-color: var(--color-bg-tertiary); color: var(--color-text-tertiary);">
						（ボタンなし）
					</div>
				{/if}
			</div>
		</div>
	{/if}
</div>