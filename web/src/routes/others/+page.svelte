<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import LogPanel from '$lib/components/LogPanel.svelte';
	import {
		getThemeState,
		subscribe,
		toggleDarkLight,
		activateTheme,
		addCustomTheme,
		removeCustomTheme,
		updateCustomTheme,
		resolveThemeVariables,
		applyThemeToDocument,
		applyThemeModeClass,
		getThemeLabel,
		BUILTIN_PRESETS,
		type ThemeState,
		type ThemeMode,
		type UserTheme
	} from '$lib/theme';

	let profiles = $state<string[]>([]);
	let currentProfile = $state('');
	let mouseStickLEnabled = $state(false);
	let mouseStickREnabled = $state(false);
	let mouseSensitivity = $state(1.0);

	// ── Theme state ───────────────────────────────────────────────────────
	let themeState = $state<ThemeState>(getThemeState());
	let showCustomThemeForm = $state(false);
	let editingCustomTheme = $state<UserTheme | null>(null);

	// Custom theme form state
	let customName = $state('');
	let customBgPrimary = $state('#0f172a');
	let customBgSecondary = $state('#1e293b');
	let customTextPrimary = $state('#f1f5f9');
	let customAccent = $state('#60a5fa');

	// Preview state
	let previewMode = $state<ThemeMode | null>(null);

	// Unsubscribe function
	let unsubscribe: (() => void) | null = null;

	onMount(() => {
		api.getProfiles().then((list) => {
			profiles = list.map((p) => p.name);
		}).catch(console.warn);
		api.getMouseStick().then((c) => {
			mouseStickLEnabled = c.left_enabled;
			mouseStickREnabled = c.right_enabled;
			mouseSensitivity = c.sensitivity;
		}).catch(console.warn);

		// Subscribe to theme changes
		unsubscribe = subscribe((state) => {
			themeState = state;
		});
	});

	// ── Profile / Mouse stick actions ─────────────────────────────────────

	async function switchProfile(name: string) {
		await api.setProfile(name);
		currentProfile = name;
	}

	async function applyMouseStick(stick: string, enabled: boolean) {
		await api.setMouseStick({ stick, enabled, sensitivity: mouseSensitivity });
	}

	// ── Theme actions ─────────────────────────────────────────────────────

	function handleToggleDarkLight() {
		const newState = toggleDarkLight();
		themeState = newState;
	}

	function handleActivateMode(mode: ThemeMode) {
		const newState = activateTheme(mode);
		themeState = newState;
	}

	function handlePreviewTheme(mode: ThemeMode, variables: Record<string, string>) {
		previewMode = mode;
		if (mode === 'custom') {
			applyThemeModeClass({ mode: 'custom', activeThemeId: 'preview', customThemes: [] });
		} else {
			applyThemeModeClass({ mode, activeThemeId: mode, customThemes: [] });
		}
		applyThemeToDocument(variables);
	}

	function handleClearPreview() {
		previewMode = null;
		const state = getThemeState();
		const variables = resolveThemeVariables(state);
		applyThemeModeClass(state);
		applyThemeToDocument(variables);
	}

	function buildCustomVars(): Record<string, string> {
		const baseVars = { ...BUILTIN_PRESETS.dark.cssVariables };
		baseVars['--color-bg-primary'] = customBgPrimary;
		baseVars['--color-bg-secondary'] = customBgSecondary;
		baseVars['--color-text-primary'] = customTextPrimary;
		baseVars['--color-accent'] = customAccent;
		return baseVars;
	}

	function handleAddCustomTheme() {
		if (!customName.trim()) return;
		const newState = addCustomTheme({
			name: customName.trim(),
			cssVariables: buildCustomVars()
		});
		themeState = newState;
		// Find the newly added theme (last one) and activate it
		const added = newState.customThemes[newState.customThemes.length - 1];
		if (added) {
			activateTheme('custom', added.id);
		}
		resetCustomForm();
	}

	function handleUpdateCustomTheme() {
		if (!editingCustomTheme || !customName.trim()) return;
		const newState = updateCustomTheme(editingCustomTheme.id, {
			name: customName.trim(),
			cssVariables: buildCustomVars()
		});
		themeState = newState;
		activateTheme('custom', editingCustomTheme.id);
		resetCustomForm();
	}

	function handleEditCustomTheme(theme: UserTheme) {
		editingCustomTheme = theme;
		customName = theme.name;
		customBgPrimary = theme.cssVariables['--color-bg-primary'] ?? '#0f172a';
		customBgSecondary = theme.cssVariables['--color-bg-secondary'] ?? '#1e293b';
		customTextPrimary = theme.cssVariables['--color-text-primary'] ?? '#f1f5f9';
		customAccent = theme.cssVariables['--color-accent'] ?? '#60a5fa';
		showCustomThemeForm = true;
	}

	function handleRemoveCustomTheme(id: string) {
		const newState = removeCustomTheme(id);
		themeState = newState;
		if (editingCustomTheme?.id === id) {
			resetCustomForm();
		}
	}

	function resetCustomForm() {
		showCustomThemeForm = false;
		editingCustomTheme = null;
		customName = '';
		customBgPrimary = '#0f172a';
		customBgSecondary = '#1e293b';
		customTextPrimary = '#f1f5f9';
		customAccent = '#60a5fa';
	}

	function openAddForm() {
		editingCustomTheme = null;
		customName = '';
		customBgPrimary = '#0f172a';
		customBgSecondary = '#1e293b';
		customTextPrimary = '#f1f5f9';
		customAccent = '#60a5fa';
		showCustomThemeForm = true;
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold" style="color: var(--color-text-primary);">その他設定</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<!-- ── Profile section ──────────────────────────────────────────── -->
		<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
			<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">プロファイル</h3>
			<div class="space-y-1">
				{#each profiles as name}
					<button
						onclick={() => switchProfile(name)}
						class="w-full rounded px-2 py-1 text-left text-xs"
						style="background-color: {currentProfile === name ? 'var(--color-accent)' : 'transparent'}; color: {currentProfile === name ? 'var(--color-accent-text)' : 'var(--color-text-secondary)'};"
					>
						{name}
					</button>
				{/each}
			</div>
		</div>

		<!-- ── Mouse stick section ──────────────────────────────────────── -->
		<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
			<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">マウススティック制御</h3>
			<div class="space-y-2">
				<label class="flex items-center gap-2 text-xs" style="color: var(--color-text-tertiary);">
					<input type="checkbox" bind:checked={mouseStickLEnabled} onchange={() => applyMouseStick('left', mouseStickLEnabled)} />
					<span>左スティックをマウスで制御</span>
				</label>
				<label class="flex items-center gap-2 text-xs" style="color: var(--color-text-tertiary);">
					<input type="checkbox" bind:checked={mouseStickREnabled} onchange={() => applyMouseStick('right', mouseStickREnabled)} />
					<span>右スティックをマウスで制御</span>
				</label>
				<label class="flex items-center justify-between text-xs" style="color: var(--color-text-tertiary);">
					<span>感度</span>
					<input type="range" min="0.1" max="3.0" step="0.1" bind:value={mouseSensitivity} onchange={() => {
						applyMouseStick('left', mouseStickLEnabled);
						applyMouseStick('right', mouseStickREnabled);
					}} class="w-32" />
					<span class="w-8 text-right">{mouseSensitivity.toFixed(1)}</span>
				</label>
			</div>
		</div>

		<!-- ── Version info ─────────────────────────────────────────────── -->
		<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
			<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">バージョン情報</h3>
			<p class="text-xs" style="color: var(--color-text-tertiary);">Poke-Controller Modified Extension v0.1.0</p>
			<p class="text-xs" style="color: var(--color-text-tertiary);">Web UI (SvelteKit)</p>
		</div>

		<!-- ── Theme settings ───────────────────────────────────────────── -->
		<div class="rounded border p-3" style="border-color: var(--color-border); background-color: var(--color-bg-card);">
			<h3 class="mb-2 text-sm font-medium" style="color: var(--color-text-primary);">テーマ設定</h3>
			<p class="mb-3 text-xs" style="color: var(--color-text-tertiary);">
				現在: {getThemeLabel(themeState)}
			</p>

			<!-- Preset toggle -->
			<div class="mb-3 flex gap-2">
				<button
					onclick={() => handleActivateMode('light')}
					class="flex-1 rounded px-3 py-2 text-xs font-medium transition-colors"
					style="background-color: {themeState.mode === 'light' ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {themeState.mode === 'light' ? 'var(--color-accent-text)' : 'var(--color-text-primary)'};"
				>
					☀ ライト
				</button>
				<button
					onclick={() => handleActivateMode('dark')}
					class="flex-1 rounded px-3 py-2 text-xs font-medium transition-colors"
					style="background-color: {themeState.mode === 'dark' ? 'var(--color-accent)' : 'var(--color-bg-tertiary)'}; color: {themeState.mode === 'dark' ? 'var(--color-accent-text)' : 'var(--color-text-primary)'};"
				>
					☾ ダーク
				</button>
				<button
					onclick={handleToggleDarkLight}
					class="rounded px-3 py-2 text-xs font-medium"
					style="background-color: var(--color-bg-tertiary); color: var(--color-text-primary);"
					title="切り替え"
				>
					↻
				</button>
			</div>

			<!-- Custom themes -->
			{#if themeState.customThemes.length > 0}
				<div class="mb-3 space-y-1">
					<p class="mb-1 text-xs font-medium" style="color: var(--color-text-secondary);">カスタムテーマ</p>
					{#each themeState.customThemes as ct}
						<div
							class="flex items-center gap-2 rounded px-2 py-1 text-xs"
							style="background-color: {themeState.activeThemeId === ct.id && themeState.mode === 'custom' ? 'var(--color-accent-light)' : 'transparent'}; color: var(--color-text-primary);"
						>
							<button
								onclick={() => activateTheme('custom', ct.id)}
								class="flex-1 text-left"
							>
								{ct.name}
							</button>
							<button
								onclick={() => handleEditCustomTheme(ct)}
								class="opacity-60 hover:opacity-100"
								title="編集"
							>
								✏
							</button>
							<button
								onclick={() => handleRemoveCustomTheme(ct.id)}
								class="opacity-60 hover:opacity-100"
								title="削除"
							>
								✕
							</button>
						</div>
					{/each}
				</div>
			{/if}

			<!-- Add / Edit custom theme form -->
			<button
				onclick={openAddForm}
				class="mb-2 rounded px-3 py-1.5 text-xs font-medium"
				style="background-color: var(--color-accent); color: var(--color-accent-text);"
			>
				+ カスタムテーマを追加
			</button>

			{#if showCustomThemeForm}
				<div
					class="mt-2 space-y-2 rounded border p-3"
					style="border-color: var(--color-border); background-color: var(--color-bg-tertiary);"
				>
					<p class="text-xs font-medium" style="color: var(--color-text-primary);">
						{editingCustomTheme ? 'テーマを編集' : '新しいテーマ'}
					</p>
					<div>
						<label class="block text-xs" style="color: var(--color-text-tertiary);">テーマ名</label>
						<input
							type="text"
							bind:value={customName}
							placeholder="マイテーマ"
							class="w-full rounded border px-2 py-1 text-xs"
							style="border-color: var(--color-border); background-color: var(--color-bg-input); color: var(--color-text-primary);"
						/>
					</div>
					<div class="grid grid-cols-2 gap-2">
						<div>
							<label class="block text-xs" style="color: var(--color-text-tertiary);">背景（一次）</label>
							<input type="color" bind:value={customBgPrimary} class="h-7 w-full cursor-pointer rounded border" style="border-color: var(--color-border);" />
						</div>
						<div>
							<label class="block text-xs" style="color: var(--color-text-tertiary);">背景（二次）</label>
							<input type="color" bind:value={customBgSecondary} class="h-7 w-full cursor-pointer rounded border" style="border-color: var(--color-border);" />
						</div>
						<div>
							<label class="block text-xs" style="color: var(--color-text-tertiary);">テキスト色</label>
							<input type="color" bind:value={customTextPrimary} class="h-7 w-full cursor-pointer rounded border" style="border-color: var(--color-border);" />
						</div>
						<div>
							<label class="block text-xs" style="color: var(--color-text-tertiary);">アクセント色</label>
							<input type="color" bind:value={customAccent} class="h-7 w-full cursor-pointer rounded border" style="border-color: var(--color-border);" />
						</div>
					</div>
					<!-- Live preview -->
					<div
						class="mt-1 rounded p-2 text-xs"
						style="background-color: {customBgPrimary}; color: {customTextPrimary};"
					>
						<div class="mb-1 flex gap-1">
							<span class="rounded px-2 py-0.5" style="background-color: {customAccent}; color: {customBgPrimary};">プレビュー</span>
							<span class="rounded px-2 py-0.5" style="background-color: {customBgSecondary}; color: {customTextPrimary};">A</span>
						</div>
						<span style="color: {customTextPrimary};">サンプルテキスト</span>
					</div>
					<div class="flex gap-2">
						<button
							onclick={editingCustomTheme ? handleUpdateCustomTheme : handleAddCustomTheme}
							class="flex-1 rounded px-3 py-1.5 text-xs font-medium"
							style="background-color: var(--color-accent); color: var(--color-accent-text);"
						>
							{editingCustomTheme ? '更新' : '保存'}
						</button>
						<button
							onclick={resetCustomForm}
							class="rounded px-3 py-1.5 text-xs"
							style="background-color: var(--color-bg-tertiary); color: var(--color-text-secondary);"
						>
							キャンセル
						</button>
					</div>
				</div>
			{/if}
		</div>
	</div>

	<LogPanel />
</div>

<style>
	input[type="color"] {
		-webkit-appearance: none;
		appearance: none;
		padding: 0;
	}
	input[type="color"]::-webkit-color-swatch-wrapper {
		padding: 0;
	}
	input[type="color"]::-webkit-color-swatch {
		border: none;
		border-radius: 0.25rem;
	}
</style>
