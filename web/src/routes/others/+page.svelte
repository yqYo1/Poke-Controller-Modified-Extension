<script lang="ts">
	import { onMount } from 'svelte';
	import { uiState, setWidgetMode, setSplitRatio, setStdoutDestination, setControllerPosition, setDialogueButtonPosition, initUIStore } from '$lib/stores/ui.svelte';

	let isInitialized = $derived(!uiState.loading);

	// ── Initialise uiStore if not already loaded ─────────────────────────────
	onMount(() => {
		if (!isInitialized) {
			initUIStore().catch(console.warn);
		}
	});

	// ── Widget mode combobox options ─────────────────────────────────────────
	const widgetModeOptions = [
		{ value: 1, label: 'Full panel' },
		{ value: 2, label: 'Single output' },
		{ value: 3, label: 'Single output (swapped)' },
		{ value: 4, label: 'Outputs only' },
		{ value: 5, label: 'Controller only' },
		{ value: 6, label: 'Output #1 only' },
		{ value: 7, label: 'Output #2 only' },
	];

	// ── Dialogue button position options ─────────────────────────────────────
	const dialogueOptions = [
		{ value: 'top' as const, label: 'TOP' },
		{ value: 'bottom' as const, label: 'BOTTOM' },
		{ value: 'both' as const, label: 'BOTH' },
	];

	// ── Clear outputs handlers ───────────────────────────────────────────────
	function clearOutput1() {
		window.dispatchEvent(new CustomEvent('clear-outputs', { detail: { panel: 1 } }));
	}

	function clearOutput2() {
		window.dispatchEvent(new CustomEvent('clear-outputs', { detail: { panel: 2 } }));
	}

	function clearAllOutputs() {
		window.dispatchEvent(new CustomEvent('clear-outputs'));
	}
</script>

<div class="tk-app">
	<!-- Title Bar -->
	<div class="tk-titlebar">
		<span class="tk-titlebar-text">Others Settings</span>
	</div>

	<!-- Main content -->
	<div class="tk-main" style="padding: 8px;">
		{#if uiState.loading}
			<div class="tk-statusbar">
				<span>Loading settings...</span>
			</div>
		{:else if uiState.error}
			<div class="tk-statusbar tk-status-error">
				<span>Error: {uiState.error}</span>
			</div>
		{:else}
			<div class="tk-labelframe">
				<div class="tk-labelframe-label">Outputs/Dialogue Settings</div>
				<div class="tk-labelframe-content">
					<!-- 1. Output Size Adjuster -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Output Size Adjuster</div>
						<div class="tk-labelframe-content">
							<div class="form-row">
								<span class="tk-label">Output#1</span>
								<input
									type="range"
									min="0"
									max="100"
									class="tk-range"
									value={uiState.splitRatio}
									oninput={(e) => setSplitRatio(Number((e.target as HTMLInputElement).value))}
								/>
								<span class="tk-label">{uiState.splitRatio}%</span>
							</div>
							<div class="form-row" style="margin-top: 4px;">
								<span class="tk-label" style="color: var(--color-text-tertiary); font-size: 10px;">
									Output#1: {uiState.splitRatio}% — Output#2: {100 - uiState.splitRatio}%
								</span>
							</div>
						</div>
					</div>

					<!-- 2. Stdout Destination -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Standard Output Destination</div>
						<div class="tk-labelframe-content">
							<div class="form-row">
								<label class="tk-radio-label">
									<input
										type="radio"
										name="stdout"
										class="tk-radio"
										value={1}
										checked={uiState.stdoutDestination === 1}
										onchange={() => setStdoutDestination(1)}
									/>
									<span>Output#1</span>
								</label>
								<label class="tk-radio-label">
									<input
										type="radio"
										name="stdout"
										class="tk-radio"
										value={2}
										checked={uiState.stdoutDestination === 2}
										onchange={() => setStdoutDestination(2)}
									/>
									<span>Output#2</span>
								</label>
							</div>
						</div>
					</div>

					<!-- 3. Clear Outputs -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Clear Outputs</div>
						<div class="tk-labelframe-content">
							<div class="form-row">
								<button class="tk-btn" onclick={clearOutput1}>Clear(#1)</button>
								<button class="tk-btn" onclick={clearOutput2}>Clear(#2)</button>
								<button class="tk-btn tk-btn-warning" onclick={clearAllOutputs}>Clear All</button>
							</div>
						</div>
					</div>

					<!-- 4. Widget Mode -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Widget Mode</div>
						<div class="tk-labelframe-content">
							<select
								class="tk-select tk-select-wide"
								value={uiState.widgetMode}
								onchange={(e) => setWidgetMode(Number((e.target as HTMLSelectElement).value) as 1 | 2 | 3 | 4 | 5 | 6 | 7)}
							>
								{#each widgetModeOptions as opt (opt.value)}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
						</div>
					</div>

					<!-- 5. Software-Controller Position -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Software-Controller Position</div>
						<div class="tk-labelframe-content">
							<div class="form-row">
								<label class="tk-radio-label">
									<input
										type="radio"
										name="swpos"
										class="tk-radio"
										value="top"
										checked={uiState.controllerPosition === 'top'}
										onchange={() => setControllerPosition('top')}
									/>
									<span>TOP</span>
								</label>
								<label class="tk-radio-label">
									<input
										type="radio"
										name="swpos"
										class="tk-radio"
										value="bottom"
										checked={uiState.controllerPosition === 'bottom'}
										onchange={() => setControllerPosition('bottom')}
									/>
									<span>BOTTOM</span>
								</label>
							</div>
						</div>
					</div>

					<!-- 6. Dialogue Button Position -->
					<div class="tk-labelframe tk-inset">
						<div class="tk-labelframe-label">Dialogue OK/Cancel Position</div>
						<div class="tk-labelframe-content">
							<div class="form-row">
								{#each dialogueOptions as opt (opt.value)}
									<label class="tk-radio-label">
										<input
											type="radio"
											name="dlgpos"
											class="tk-radio"
											value={opt.value}
											checked={uiState.dialogueButtonPosition === opt.value}
											onchange={() => setDialogueButtonPosition(opt.value)}
										/>
										<span>{opt.label}</span>
									</label>
								{/each}
							</div>
						</div>
					</div>
				</div>
			</div>
		{/if}

		<!-- Status bar -->
		<div class="tk-statusbar" style="margin-top: 8px;">
			<span class="tk-statusbar-text">Output Size Adjuster: {uiState.splitRatio}% | Widget Mode: {uiState.widgetMode} | Position: {uiState.controllerPosition}</span>
		</div>
	</div>
</div>

<style>
	/* Form row helper — matches the inline tab style in +page.svelte */
	:global(.form-row) {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-wrap: wrap;
	}

	:global(.tk-radio-label) {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		cursor: pointer;
		font-size: 12px;
		color: var(--color-text-primary, #e0e0e0);
	}

	:global(.tk-btn-warning) {
		background-color: #d9534f;
		color: white;
		border-color: #d43f3a;
	}
</style>
