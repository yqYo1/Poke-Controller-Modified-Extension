<script lang="ts">
	import { onMount } from 'svelte';

	interface Props {
		selectedCommand?: string;
		onTriggerShortcut?: (slot: number) => void;
	}

	let { selectedCommand = $bindable(''), onTriggerShortcut }: Props = $props();

	// 10 shortcut slots, stored as array of (command name | null)
	const STORAGE_KEY = 'pokecon-shortcuts';
	let assignments = $state<(string | null)[]>(Array(10).fill(null));

	function loadAssignments() {
		try {
			const raw = localStorage.getItem(STORAGE_KEY);
			if (raw) {
				const parsed = JSON.parse(raw);
				if (Array.isArray(parsed) && parsed.length === 10) {
					assignments = parsed.map((v: unknown) => (typeof v === 'string' ? v : null));
				}
			}
		} catch {
			// ignore
		}
	}

	function saveAssignments() {
		try {
			localStorage.setItem(STORAGE_KEY, JSON.stringify(assignments));
		} catch {
			// ignore
		}
	}

	onMount(() => {
		loadAssignments();
	});

	function assignSlot(index: number) {
		if (!selectedCommand) return;
		assignments[index] = selectedCommand;
		assignments = [...assignments]; // trigger reactivity
		saveAssignments();
	}

	function clearSlot(index: number) {
		assignments[index] = null;
		assignments = [...assignments];
		saveAssignments();
	}

	function handleClick(index: number, event: MouseEvent) {
		// Shift+Click → assign selected command
		if (event.shiftKey) {
			event.preventDefault();
			assignSlot(index);
			return;
		}
		// Normal click → trigger the assigned command
		const cmd = assignments[index];
		if (cmd) {
			onTriggerShortcut?.(index);
		}
	}

	function handleContextMenu(index: number, event: MouseEvent) {
		event.preventDefault();
		clearSlot(index);
	}

	// Keyboard shortcuts: F1–F10
	function handleKeyDown(event: KeyboardEvent) {
		// Check if we're inside an input/textarea to avoid conflicts
		const target = event.target as HTMLElement;
		if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
			return;
		}

		const keyMap: Record<string, number> = {
			F1: 0, F2: 1, F3: 2, F4: 3, F5: 4,
			F6: 5, F7: 6, F8: 7, F9: 8, F10: 9,
		};

		const index = keyMap[event.key];
		if (index !== undefined) {
			event.preventDefault();
			const cmd = assignments[index];
			if (cmd) {
				onTriggerShortcut?.(index);
			}
		}
	}

	onMount(() => {
		window.addEventListener('keydown', handleKeyDown);
		return () => window.removeEventListener('keydown', handleKeyDown);
	});
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">ショートカットボタン</h3>
	<p class="mb-2 text-xs text-gray-500">
		Shift+Click で現在選択中のコマンドを割り当て &middot;
		右クリックで解除 &middot;
		F1–F10 で実行
	</p>
	<div class="shortcut-grid">
		{#each [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] as index (index)}
			{@const label = assignments[index] ?? `F${index + 1} - Empty`}
			<button
				class="tk-btn-shortcut"
				class:assigned={assignments[index] !== null}
				onclick={(e) => handleClick(index, e)}
				oncontextmenu={(e) => handleContextMenu(index, e)}
				title={assignments[index] ?? `F${index + 1} - 未割り当て`}
			>
				<span class="shortcut-key">F{index + 1}</span>
				<span class="shortcut-name">{label.length > 20 ? label.slice(0, 20) + '…' : label}</span>
			</button>
		{/each}
	</div>
</div>

<style>
	.shortcut-grid {
		display: grid;
		grid-template-columns: repeat(5, 1fr);
		gap: 4px;
	}

	.tk-btn-shortcut {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1px;
		padding: 3px 2px;
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 10px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-secondary, #94a3b8);
		cursor: pointer;
		user-select: none;
		white-space: nowrap;
		overflow: hidden;
		min-width: 0;
		transition: background-color 0.1s;
	}

	.tk-btn-shortcut:hover {
		background-color: var(--color-accent-hover, #3b82f6);
		color: #fff;
	}

	.tk-btn-shortcut.assigned {
		background-color: #1b5e20;
		border-color: #2e7d32;
		color: #a5d6a7;
	}

	.tk-btn-shortcut.assigned:hover {
		background-color: #2e7d32;
		color: #fff;
	}

	.shortcut-key {
		font-size: 9px;
		font-weight: 600;
		opacity: 0.7;
		line-height: 1;
	}

	.shortcut-name {
		font-size: 10px;
		line-height: 1.2;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 100%;
	}
</style>
