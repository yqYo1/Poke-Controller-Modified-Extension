<script lang="ts">
	import { api } from '$lib/api/client';
	import PythonCommandList from './PythonCommandList.svelte';
	import McuCommandList from './McuCommandList.svelte';
	import ShortcutButtons from './ShortcutButtons.svelte';
	import CommandActions from './CommandActions.svelte';

	// ── Sub-tab state ────────────────────────────────────────────────────────
	type SubTab = 'python' | 'mcu' | 'shortcut';
	let activeSubTab = $state<SubTab>('python');

	// ── Shared state ─────────────────────────────────────────────────────────
	let selectedCommand = $state('');

	function handleSelectCommand(name: string) {
		selectedCommand = name;
		api.loadCommand(name).catch(console.warn);
	}

	function handleTriggerShortcut(slotIndex: number) {
		// Load the command assigned to this shortcut slot from localStorage
		try {
			const raw = localStorage.getItem('pokecon-shortcuts');
			if (raw) {
				const parsed = JSON.parse(raw);
				if (Array.isArray(parsed) && parsed.length === 10) {
					const cmd = parsed[slotIndex];
					if (typeof cmd === 'string' && cmd) {
						selectedCommand = cmd;
						api.loadCommand(cmd).catch(console.warn);
						api.startCommand(cmd).catch(console.warn);
					}
				}
			}
		} catch {
			// ignore
		}
	}

	// Keyboard shortcuts (F5=Start, Shift+F6=Pause, Escape=Stop) are handled
	// by CommandActions.svelte to avoid duplicate event listeners.
</script>

<div class="tab-content">
	<!-- §4.4.1 Sub-tab Structure: 3 internal tabs -->
	<div class="tk-inner-notebook">
		<div class="tk-notebook-tabs">
			<button
				class="tk-notebook-tab"
				class:active={activeSubTab === 'python'}
				onclick={() => (activeSubTab = 'python')}
			>
				Python Command
			</button>
			<button
				class="tk-notebook-tab"
				class:active={activeSubTab === 'mcu'}
				onclick={() => (activeSubTab = 'mcu')}
			>
				Mcu Command
			</button>
			<button
				class="tk-notebook-tab"
				class:active={activeSubTab === 'shortcut'}
				onclick={() => (activeSubTab = 'shortcut')}
			>
				Shortcut
			</button>
		</div>
		<div class="tk-notebook-content">
			{#if activeSubTab === 'python'}
				<!-- §4.4.2 Python Command List -->
				<PythonCommandList onSelect={handleSelectCommand} />
			{:else if activeSubTab === 'mcu'}
				<!-- §4.4.2 MCU Command List -->
				<McuCommandList onSelect={handleSelectCommand} />
			{:else if activeSubTab === 'shortcut'}
				<!-- §4.4.3 Shortcut Buttons (10 buttons) -->
				<ShortcutButtons
					bind:selectedCommand
					onTriggerShortcut={handleTriggerShortcut}
				/>
			{/if}
		</div>
	</div>

	<!-- §4.4.4 Execution Control -->
	<div class="mt-2">
		<CommandActions bind:selectedCommand />
	</div>
</div>

<style>
	.tab-content {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 2px;
	}
</style>
