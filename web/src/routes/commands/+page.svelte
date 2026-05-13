<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';
	import PythonCommandList from './PythonCommandList.svelte';
	import McuCommandList from './McuCommandList.svelte';
	import ShortcutButtons from './ShortcutButtons.svelte';
	import CommandActions from './CommandActions.svelte';

	let selectedCommand = $state('');
	let running = $state(false);

	onMount(() => {
		api.getActiveCommand().then((r) => {
			running = r.running;
		}).catch(() => {});
	});

	function handleSelectCommand(name: string) {
		selectedCommand = name;
		api.loadCommand(name).catch(console.warn);
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">コマンド</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<PythonCommandList onSelect={handleSelectCommand} />
		<McuCommandList onSelect={handleSelectCommand} />
	</div>

	<ShortcutButtons />

	<CommandActions bind:selectedCommand />

	<OutputPanel title="コマンド出力" />
	<LogPanel />
</div>