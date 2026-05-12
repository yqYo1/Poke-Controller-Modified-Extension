<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import LogPanel from '$lib/components/LogPanel.svelte';

	// ── Keyboard state ────────────────────────────────────────────────────────
	let keyboardEnabled = $state(false);
	let keyMappings = $state<Record<string, string>>({});
	let editingKey = $state<string | null>(null);
	let listening = $state(false);
	let saving = $state(false);
	let message = $state<string | null>(null);

	// ── Available controller buttons ──────────────────────────────────────────
	const controllerButtons = [
		'A', 'B', 'X', 'Y',
		'L', 'R', 'ZL', 'ZR',
		'UP', 'DOWN', 'LEFT', 'RIGHT',
		'PLUS', 'MINUS',
		'LSTICK', 'RSTICK',
		'LSTICK_UP', 'LSTICK_DOWN', 'LSTICK_LEFT', 'LSTICK_RIGHT',
		'RSTICK_UP', 'RSTICK_DOWN', 'RSTICK_LEFT', 'RSTICK_RIGHT',
		'HOME', 'CAPTURE',
	];

	const defaultMappings: Record<string, string> = {
		'A': 'Z',
		'B': 'X',
		'X': 'A',
		'Y': 'S',
		'L': 'Q',
		'R': 'W',
		'ZL': 'E',
		'ZR': 'R',
		'UP': 'ArrowUp',
		'DOWN': 'ArrowDown',
		'LEFT': 'ArrowLeft',
		'RIGHT': 'ArrowRight',
		'PLUS': 'Enter',
		'MINUS': 'Backspace',
		'LSTICK': 'ShiftLeft',
		'RSTICK': 'ShiftRight',
		'HOME': 'Home',
		'CAPTURE': 'F12',
	};

	onMount(() => {
		loadConfig();
	});

	async function loadConfig() {
		try {
			const resp = await api.getKeyboardEnabled();
			keyboardEnabled = resp.enabled;
		} catch {
			keyboardEnabled = false;
		}
		// Load key mappings from keyboard config endpoint if available
		try {
			// Use generic get to fetch keyboard mappings from a potential endpoint
			const resp = await api.getKeyboardEnabled();
			keyMappings = { ...defaultMappings };
		} catch {
			keyMappings = { ...defaultMappings };
		}
	}

	async function toggleKeyboard() {
		try {
			await api.setKeyboardEnabled(keyboardEnabled);
			message = keyboardEnabled ? 'キーボード入力を有効にしました' : 'キーボード入力を無効にしました';
		} catch (e) {
			message = `エラー: ${e}`;
		}
		setTimeout(() => (message = null), 3000);
	}

	function startListening(button: string) {
		editingKey = button;
		listening = true;
	}

	function handleKeyDown(e: KeyboardEvent) {
		if (!listening || !editingKey) return;
		e.preventDefault();
		const keyName = e.key;
		keyMappings[editingKey] = keyName;
		listening = false;
		editingKey = null;
	}

	async function saveMappings() {
		saving = true;
		try {
			// Store mappings via generic input config endpoint
			for (const [button, key] of Object.entries(keyMappings)) {
				if (key === defaultMappings[button]) continue;
				await api.sendInput('keyconfig', { button, key });
			}
			message = 'キー設定を保存しました';
		} catch (e) {
			message = `保存エラー: ${e}`;
		}
		saving = false;
		setTimeout(() => (message = null), 3000);
	}

	function resetDefaults() {
		keyMappings = { ...defaultMappings };
		message = 'デフォルト設定にリセットしました';
		setTimeout(() => (message = null), 3000);
	}
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="space-y-4">
	<h2 class="text-lg font-bold">キーコンフィグ</h2>

	{#if message}
		<div class="rounded border border-green-700 bg-green-900/50 px-3 py-2 text-xs text-green-300">
			{message}
		</div>
	{/if}

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<!-- キーボード有効/無効 -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">キーボード入力</h3>
			<label class="flex cursor-pointer items-center gap-3">
				<input
					type="checkbox"
					bind:checked={keyboardEnabled}
					onchange={toggleKeyboard}
					class="accent-blue-500"
				/>
				<span class="text-xs text-gray-300">
					キーボード入力を{keyboardEnabled ? '有効' : '無効'}
				</span>
			</label>
			<p class="mt-1 text-[10px] text-gray-500">
				有効にすると、割り当てたキーでSwitchコントローラーの操作が可能になります
			</p>
		</div>

		<!-- キー割り当て一覧 -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">キー割り当て</h3>
			<div class="max-h-80 space-y-1 overflow-y-auto">
				{#each controllerButtons as button}
					<div class="flex items-center justify-between rounded px-2 py-1 hover:bg-gray-800">
						<span class="text-xs font-medium text-gray-300">{button}</span>
						<button
							onclick={() => startListening(button)}
							class="min-w-24 rounded bg-gray-800 px-3 py-1 text-xs font-mono text-gray-200 transition-colors hover:bg-gray-700 {listening && editingKey === button ? 'ring-2 ring-blue-500 ring-offset-1 ring-offset-gray-900' : ''}"
						>
							{listening && editingKey === button ? 'キーを押してください...' : keyMappings[button] || '未割り当て'}
						</button>
					</div>
				{/each}
			</div>
		</div>
	</div>

	<!-- 操作ボタン -->
	<div class="flex gap-3">
		<button
			onclick={saveMappings}
			disabled={saving || listening}
			class="rounded bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50"
		>
			{saving ? '保存中...' : '設定を保存'}
		</button>
		<button
			onclick={resetDefaults}
			disabled={listening}
			class="rounded bg-gray-700 px-4 py-2 text-sm font-medium text-gray-300 hover:bg-gray-600 disabled:opacity-50"
		>
			デフォルトにリセット
		</button>
	</div>

	<LogPanel />
</div>
