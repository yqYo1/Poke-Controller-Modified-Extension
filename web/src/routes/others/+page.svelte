<script lang="ts">
	import { onMount } from 'svelte';
	import * as uiStore from '$lib/stores/ui.svelte';

	// ── Initialise uiStore if not already loaded ─────────────────────────────
	onMount(() => {
		if (!uiStore.isInitialized) {
			uiStore.initUIStore().catch(console.warn);
		}
	});

	// ── Widget mode combobox options ─────────────────────────────────────────
	const widgetModeOptions = [
		{ value: 1, label: 'ALL (default)' },
		{ value: 2, label: 'Output#1 + Output#2' },
		{ value: 3, label: 'Output#1 + Software-Controller' },
		{ value: 4, label: 'Output#2 + Software-Controller' },
		{ value: 5, label: 'Output#1 Only' },
		{ value: 6, label: 'Output#2 Only' },
		{ value: 7, label: 'Software-Controller Only' },
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
		{#if uiStore.loading}
			<div class="tk-statusbar">
				<span>Loading settings...</span>
			</div>
		{:else if uiStore.error}
			<div class="tk-statusbar tk-status-error">
				<span>Error: {uiStore.error}</span>
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
									value={uiStore.splitRatio}
									oninput={(e) => uiStore.setSplitRatio(Number((e.target as HTMLInputElement).value))}
								/>
								<span class="tk-label">{uiStore.splitRatio}%</span>
							</div>
							<div class="form-row" style="margin-top: 4px;">
								<span class="tk-label" style="color: var(--color-text-tertiary); font-size: 10px;">
									Output#1: {uiStore.splitRatio}% — Output#2: {100 - uiStore.splitRatio}%
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
										checked={uiStore.stdoutDestination === 1}
										onchange={() => uiStore.setStdoutDestination(1)}
									/>
									<span>Output#1</span>
								</label>
								<label class="tk-radio-label">
									<input
										type="radio"
										name="stdout"
										class="tk-radio"
										value={2}
										checked={uiStore.stdoutDestination === 2}
										onchange={() => uiStore.setStdoutDestination(2)}
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
								value={uiStore.widgetMode}
								onchange={(e) => uiStore.setWidgetMode(Number((e.target as HTMLSelectElement).value) as 1 | 2 | 3 | 4 | 5 | 6 | 7)}
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
										checked={uiStore.controllerPosition === 'top'}
										onchange={() => uiStore.setControllerPosition('top')}
									/>
									<span>TOP</span>
								</label>
								<label class="tk-radio-label">
									<input
										type="radio"
										name="swpos"
										class="tk-radio"
										value="bottom"
										checked={uiStore.controllerPosition === 'bottom'}
										onchange={() => uiStore.setControllerPosition('bottom')}
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
											checked={uiStore.dialogueButtonPosition === opt.value}
											onchange={() => uiStore.setDialogueButtonPosition(opt.value)}
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
			<span class="tk-statusbar-text">Output Size Adjuster: {uiStore.splitRatio}% | Widget Mode: {uiStore.widgetMode} | Position: {uiStore.controllerPosition}</span>
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
