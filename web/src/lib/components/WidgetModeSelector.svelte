<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type WidgetSettings } from '$lib/api/client';

	const widgetModes = [
		{ id: 'テキスト', label: 'テキスト', icon: '📝', desc: 'テキスト出力' },
		{ id: '画像', label: '画像', icon: '🖼', desc: '画像プレビュー' },
		{ id: '表', label: '表', icon: '📊', desc: '表形式データ' },
		{ id: 'グラフ', label: 'グラフ', icon: '📈', desc: 'グラフ表示' },
		{ id: 'ゲージ', label: 'ゲージ', icon: '⏱', desc: 'ゲージ表示' },
		{ id: 'ログ', label: 'ログ', icon: '📋', desc: 'フィルタログ' },
		{ id: 'カスタム', label: 'カスタム', icon: '⚙', desc: 'ユーザー定義' },
	] as const;

	let selectedMode = $state('テキスト');
	let loading = $state(true);

	onMount(() => {
		api.getWidgetMode()
			.then((settings: WidgetSettings) => {
				if (widgetModes.some(m => m.id === settings.mode)) {
					selectedMode = settings.mode;
				}
				loading = false;
			})
			.catch(() => {
				loading = false;
			});
	});

	async function selectMode(mode: string) {
		selectedMode = mode;
		try {
			await api.updateWidgetMode(mode);
		} catch (e) {
			console.warn('Failed to update widget mode', e);
		}
	}
</script>

<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
	<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">ウィジェットモード</h3>
	<p class="mb-2 text-xs" style="color: var(--color-text-tertiary);">出力パネルの表示モードを選択</p>

	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else}
		<div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
			{#each widgetModes as mode (mode.id)}
				<button
					onclick={() => selectMode(mode.id)}
					class="flex flex-col items-center gap-1 rounded border px-2 py-2 text-center text-xs transition-colors"
					style="border-color: {selectedMode === mode.id ? 'var(--color-accent)' : 'var(--color-border)'}; background-color: {selectedMode === mode.id ? 'var(--color-accent-light, rgba(96, 165, 250, 0.15))' : 'var(--color-bg-tertiary)'}; color: {selectedMode === mode.id ? 'var(--color-accent)' : 'var(--color-text-secondary)'};"
				>
					<span class="text-lg">{mode.icon}</span>
					<span class="font-medium">{mode.label}</span>
					<span class="text-[10px] opacity-70">{mode.desc}</span>
				</button>
			{/each}
		</div>
	{/if}
</div>