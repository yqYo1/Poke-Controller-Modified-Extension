<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let profiles = $state<string[]>([]);
	let currentProfile = $state('');
	let mouseStickLEnabled = $state(false);
	let mouseStickREnabled = $state(false);
	let mouseSensitivity = $state(1.0);

	onMount(() => {
		api.getProfiles().then((list) => {
			profiles = list.map((p) => p.name);
		}).catch(console.warn);
		api.getMouseStick().then((c) => {
			mouseStickLEnabled = c.stick === 'left' ? c.enabled : mouseStickLEnabled;
			mouseStickREnabled = c.stick === 'right' ? c.enabled : mouseStickREnabled;
			mouseSensitivity = c.sensitivity;
		}).catch(console.warn);
	});

	async function switchProfile(name: string) {
		await api.setProfile(name);
		currentProfile = name;
	}

	async function applyMouseStick(stick: string, enabled: boolean) {
		await api.setMouseStick({ stick, enabled, sensitivity: mouseSensitivity });
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">その他設定</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">プロファイル</h3>
			<div class="space-y-1">
				{#each profiles as name}
					<button
						onclick={() => switchProfile(name)}
						class="w-full rounded px-2 py-1 text-left text-xs {currentProfile === name ? 'bg-blue-700 text-white' : 'text-gray-300 hover:bg-gray-700'}"
					>
						{name}
					</button>
				{/each}
			</div>
		</div>

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">マウススティック制御</h3>
			<div class="space-y-2">
				<label class="flex items-center gap-2 text-xs text-gray-400">
					<input type="checkbox" bind:checked={mouseStickLEnabled} onchange={() => applyMouseStick('left', mouseStickLEnabled)} class="accent-blue-500" />
					<span>左スティックをマウスで制御</span>
				</label>
				<label class="flex items-center gap-2 text-xs text-gray-400">
					<input type="checkbox" bind:checked={mouseStickREnabled} onchange={() => applyMouseStick('right', mouseStickREnabled)} class="accent-blue-500" />
					<span>右スティックをマウスで制御</span>
				</label>
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>感度</span>
					<input type="range" min="0.1" max="3.0" step="0.1" bind:value={mouseSensitivity} onchange={() => {
						applyMouseStick('left', mouseStickLEnabled);
						applyMouseStick('right', mouseStickREnabled);
					}} class="w-32" />
					<span class="w-8 text-right">{mouseSensitivity.toFixed(1)}</span>
				</label>
			</div>
		</div>

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">バージョン情報</h3>
			<p class="text-xs text-gray-400">Poke-Controller Modified Extension v0.1.0</p>
			<p class="text-xs text-gray-500">Web UI (SvelteKit)</p>
		</div>
	</div>

	<LogPanel />
</div>
