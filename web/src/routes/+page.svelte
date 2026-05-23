<script lang="ts">
	import MainToolbar from '$lib/components/MainToolbar.svelte';
	import RightPanel from '$lib/components/RightPanel.svelte';
	import TkinterNotebook from '$lib/components/TkinterNotebook.svelte';
	import CameraPreview from '$lib/components/CameraPreview.svelte';
	import CaptureRegion from '$lib/components/CaptureRegion.svelte';
	import SoftwareController from '$lib/components/SoftwareController.svelte';
	import SoftwareControl from '$lib/components/SoftwareControl.svelte';
	import HardwareControl from '$lib/components/HardwareControl.svelte';
	import PythonCommandList from './commands/PythonCommandList.svelte';
	import McuCommandList from './commands/McuCommandList.svelte';
	import ShortcutButtons from './commands/ShortcutButtons.svelte';
	import CommandActions from './commands/CommandActions.svelte';
	import { api } from '$lib/api/client';
	import { uiState, setWidgetMode, setSplitRatio, setStdoutDestination, setControllerPosition, setDialogueButtonPosition } from '$lib/stores/ui.svelte';

	// ── Tab state ──────────────────────────────────────────────────────────
	let activeTab = $state('camera');
	let showRegionSelector = $state(false);

	// ── Commands sub-tab state ──────────────────────────────────────────────
	type SubTab = 'python' | 'mcu' | 'shortcut';
	let activeSubTab = $state<SubTab>('python');
	let selectedCommand = $state('');

	function handleSelectCommand(name: string) {
		selectedCommand = name;
		api.loadCommand(name).catch(console.warn);
	}

	function handleTriggerShortcut(slotIndex: number) {
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

	// ── Application title ──────────────────────────────────────────────────
	const appName = 'Poke-Controller Modified Extension';
	const appVersion = 'v0.1.0';

	// ── Camera capture handler ─────────────────────────────────────────────
	async function handleCapture() {
		try {
			await api.captureCamera('capture.png');
		} catch (e) {
			console.error('Capture failed:', e);
		}
	}

	// ── Tab content snippets ───────────────────────────────────────────────
	let tabs = $state([
		{
			id: 'camera',
			label: 'Camera',
			content: cameraTab
		},
		{
			id: 'serial',
			label: 'Serial',
			content: serialTab
		},
		{
			id: 'manual',
			label: 'Manual Control',
			content: manualTab
		},
		{
			id: 'commands',
			label: 'Commands',
			content: commandsTab
		},
		{
			id: 'notification',
			label: 'Notification',
			content: notificationTab
		},
		{
			id: 'others',
			label: 'Others',
			content: othersTab
		}
	]);
</script>

{#snippet cameraTab()}
	<div class="tab-content">
		<!-- Camera Settings -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Settings</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<span class="form-label">Camera Name:</span>
					<select class="tk-select tk-select-wide">
						<option value="">-- Select Camera --</option>
					</select>
				</div>
				<div class="form-row">
					<span class="form-label">Camera ID:</span>
					<input type="text" class="tk-input tk-input-narrow" readonly placeholder="0" />
					<span class="tk-separator-v"></span>
					<span class="form-label">FPS:</span>
					<select class="tk-select tk-input-narrow">
						<option>60</option>
						<option>45</option>
						<option selected>30</option>
						<option>15</option>
						<option>5</option>
					</select>
					<span class="tk-separator-v"></span>
					<span class="form-label">Flip:</span>
					<select class="tk-select">
						<option>None</option>
						<option>Vertical</option>
						<option>Horizontal</option>
						<option>Both</option>
					</select>
					<span class="tk-separator-v"></span>
					<button class="tk-btn">Reload Camera</button>
				</div>
			</div>
		</div>

		<!-- Display Settings -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Display Settings</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" checked />
						<span>Show Realtime</span>
					</label>
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>Show Value</span>
					</label>
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" checked />
						<span>Show Guide</span>
					</label>
					<span class="tk-separator-v"></span>
					<span class="form-label">Show Size:</span>
					<select class="tk-select">
						<option>320x180</option>
						<option selected>640x360</option>
						<option>960x540</option>
						<option>1280x720</option>
						<option>1600x900</option>
						<option>1920x1080</option>
					</select>
				</div>
			</div>
		</div>

		<!-- Capture controls -->
		<div class="form-row" style="padding: 6px; gap: 8px;">
			<button class="tk-btn" onclick={handleCapture}>Capture</button>
			<button class="tk-btn" onclick={() => (showRegionSelector = !showRegionSelector)}>
				{showRegionSelector ? 'Close Region Selector' : 'Select Capture Region'}
			</button>
		</div>

		{#if showRegionSelector}
			<div style="padding: 4px;">
				<CaptureRegion width={640} height={360} />
			</div>
		{/if}
	</div>
{/snippet}

{#snippet serialTab()}
	<div class="tab-content">
		<!-- Serial Settings -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Settings</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<span class="form-label">COM Port:</span>
					<input type="text" class="tk-input tk-input-narrow" readonly placeholder="0" />
					<span class="tk-separator-v"></span>
					<span class="form-label">Baud Rate:</span>
					<select class="tk-select tk-input-narrow">
						<option>9600</option>
						<option>4800</option>
						<option selected>115200</option>
					</select>
					<span class="tk-separator-v"></span>
					<button class="tk-btn">Reload Port</button>
					<button class="tk-btn">Disconnect Port</button>
				</div>
				<div class="form-row">
					<span class="form-label">Device Name:</span>
					<select class="tk-select tk-select-wide">
						<option value="">(Select device)</option>
					</select>
					<button class="tk-btn">Scan Device</button>
				</div>
			</div>
		</div>

		<!-- Serial Data -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Data</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<span class="form-label">Data Format:</span>
					<select class="tk-select">
						<option selected>Default</option>
						<option>Qingpi</option>
						<option>3DS Controller</option>
					</select>
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>Show Serial</span>
					</label>
				</div>
			</div>
		</div>
	</div>
{/snippet}

{#snippet manualTab()}
	<div class="tab-content">
		<!-- Software Control (§4.3.1) + Switch Controller Simulator (§4.3.3) -->
		<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
			<!-- §4.3.3 Switch Controller Simulator: Full Joy-Con layout -->
			<SoftwareController />

			<!-- §4.3.1 Software Control: Keyboard + LStick/RStick Mouse checkboxes -->
			<SoftwareControl />
		</div>

		<!-- §4.3.2 Hardware Control: ProController/Xinput radio + Record checkbox -->
		<HardwareControl />
	</div>
{/snippet}

{#snippet commandsTab()}
	<div class="tab-content">
		<!-- Command selector inner notebook with 3 sub-tabs -->
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
					<PythonCommandList onSelect={handleSelectCommand} />
				{:else if activeSubTab === 'mcu'}
					<McuCommandList onSelect={handleSelectCommand} />
				{:else if activeSubTab === 'shortcut'}
					<ShortcutButtons
						bind:selectedCommand
						onTriggerShortcut={handleTriggerShortcut}
					/>
				{/if}
			</div>
		</div>

		<!-- Execution Control -->
		<div class="mt-2">
			<CommandActions bind:selectedCommand />
		</div>
	</div>
{/snippet}

{#snippet notificationTab()}
	<div class="tab-content">
		<!-- Windows Notification -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Windows Notification</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>Start</span>
					</label>
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>End</span>
					</label>
					<button class="tk-btn">Test</button>
				</div>
			</div>
		</div>
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Discord Notification</div>
			<div class="tk-labelframe-content">
				<div class="form-row">
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>Start</span>
					</label>
					<label class="tk-checkbox-label">
						<input type="checkbox" class="tk-checkbox" />
						<span>End</span>
					</label>
					<button class="tk-btn">Test</button>
				</div>
			</div>
		</div>
	</div>
{/snippet}

{#snippet othersTab()}
	<div class="tab-content">
		<!-- Outputs/Dialogue Settings -->
		<div class="tk-labelframe">
			<div class="tk-labelframe-label">Outputs/Dialogue Settings</div>
			<div class="tk-labelframe-content">
				<!-- Size Adjuster -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Size Adjuster</div>
					<div class="tk-labelframe-content">
						<input type="range" min="0" max="100" class="tk-range" value={uiState.splitRatio} oninput={(e) => setSplitRatio(Number((e.target as HTMLInputElement).value))} />
					</div>
				</div>

				<!-- Stdout Destination -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Standard Output Destination</div>
					<div class="tk-labelframe-content">
					<div class="form-row">
						<label class="tk-radio-label">
							<input type="radio" name="stdout" class="tk-radio" value={1} checked={uiState.stdoutDestination === 1} onchange={() => setStdoutDestination(1)} />
							<span>Output#1</span>
						</label>
						<label class="tk-radio-label">
							<input type="radio" name="stdout" class="tk-radio" value={2} checked={uiState.stdoutDestination === 2} onchange={() => setStdoutDestination(2)} />
							<span>Output#2</span>
						</label>
					</div>
					</div>
				</div>

				<!-- Clear Outputs -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Clear Outputs</div>
					<div class="tk-labelframe-content">
						<div class="form-row">
							<button class="tk-btn" onclick={() => window.dispatchEvent(new CustomEvent('clear-outputs', { detail: { panel: 1 } }))}>Clear(#1)</button>
							<button class="tk-btn" onclick={() => window.dispatchEvent(new CustomEvent('clear-outputs', { detail: { panel: 2 } }))}>Clear(#2)</button>
						</div>
					</div>
				</div>

				<!-- Widget Mode -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Widget Mode</div>
					<div class="tk-labelframe-content">
					<select class="tk-select tk-select-wide" value={uiState.widgetMode} onchange={(e) => setWidgetMode(Number((e.target as HTMLSelectElement).value) as 1 | 2 | 3 | 4 | 5 | 6 | 7)}>
						<option value={1}>ALL (default)</option>
						<option value={2}>Output#1 + Output#2</option>
						<option value={3}>Output#1 + Software-Controller</option>
						<option value={4}>Output#2 + Software-Controller</option>
						<option value={5}>Output#1 Only</option>
						<option value={6}>Output#2 Only</option>
						<option value={7}>Software-Controller Only</option>
					</select>
					</div>
				</div>

				<!-- Software-Controller Position -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Software-Controller Position</div>
					<div class="tk-labelframe-content">
						<div class="form-row">
							<label class="tk-radio-label">
								<input type="radio" name="swpos" class="tk-radio" value="top" checked={uiState.controllerPosition === 'top'} onchange={() => setControllerPosition('top')} />
								<span>TOP</span>
							</label>
							<label class="tk-radio-label">
								<input type="radio" name="swpos" class="tk-radio" value="bottom" checked={uiState.controllerPosition === 'bottom'} onchange={() => setControllerPosition('bottom')} />
								<span>BOTTOM</span>
							</label>
						</div>
					</div>
				</div>

				<!-- Dialogue OK/Cancel Position -->
				<div class="tk-labelframe tk-inset">
					<div class="tk-labelframe-label">Dialogue OK/Cancel Position</div>
					<div class="tk-labelframe-content">
						<div class="form-row">
							<label class="tk-radio-label">
								<input type="radio" name="dlgpos" class="tk-radio" value="top" checked={uiState.dialogueButtonPosition === 'top'} onchange={() => setDialogueButtonPosition('top')} />
								<span>TOP</span>
							</label>
							<label class="tk-radio-label">
								<input type="radio" name="dlgpos" class="tk-radio" value="bottom" checked={uiState.dialogueButtonPosition === 'bottom'} onchange={() => setDialogueButtonPosition('bottom')} />
								<span>BOTTOM</span>
							</label>
							<label class="tk-radio-label">
								<input type="radio" name="dlgpos" class="tk-radio" value="both" checked={uiState.dialogueButtonPosition === 'both'} onchange={() => setDialogueButtonPosition('both')} />
								<span>BOTH</span>
							</label>
						</div>
					</div>
				</div>
			</div>
		</div>
	</div>
{/snippet}

<div class="tk-app">
	<!-- Title Bar -->
	<div class="tk-titlebar">
		<span class="tk-titlebar-text">{appName} {appVersion}</span>
	</div>

	<!-- Main content area: left panel + right panel -->
	<div class="tk-main">
		<!-- LEFT PANEL -->
		<div class="tk-left-panel">
			<!-- Top action buttons -->
			<MainToolbar />

			<!-- Camera Preview (only when camera tab is selected, or always show mini preview) -->
			<div class="camera-preview-section">
				<div class="tk-labelframe">
					<div class="tk-labelframe-label">Main Panel</div>
					<div class="tk-labelframe-content" style="padding: 2px;">
						<div class="camera-canvas-container">
							<CameraPreview height={360} />
						</div>
					</div>
				</div>
			</div>

			<!-- Notebook Tabs -->
			<div class="notebook-section">
				<TkinterNotebook bind:activeTab {tabs} />
			</div>
		</div>

		<!-- RIGHT PANEL -->
		<RightPanel />
	</div>
</div>

<style>
	/* ── App Root ──────────────────────────────────────────────────────────── */
	.tk-app {
		display: flex;
		flex-direction: column;
		height: 100%;
		background-color: var(--color-bg-primary, #0f172a);
		font-family: 'Segoe UI', 'Meiryo', system-ui, sans-serif;
		font-size: 12px;
		color: var(--color-text-primary, #f1f5f9);
		overflow: hidden;
	}

	/* ── Title Bar ─────────────────────────────────────────────────────────── */
	.tk-titlebar {
		display: flex;
		align-items: center;
		padding: 3px 10px;
		background: linear-gradient(180deg, #1e293b 0%, #0f172a 100%);
		border-bottom: 1px solid var(--color-border, #334155);
		user-select: none;
		flex-shrink: 0;
	}

	.tk-titlebar-text {
		font-size: 12px;
		font-weight: 600;
		color: var(--color-text-primary, #f1f5f9);
	}

	/* ── Main Content Area ─────────────────────────────────────────────────── */
	.tk-main {
		display: flex;
		flex: 1;
		overflow: hidden;
	}

	/* ── Left Panel ────────────────────────────────────────────────────────── */
	.tk-left-panel {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-width: 0;
		overflow: hidden;
	}

	.camera-preview-section {
		flex-shrink: 0;
		padding: 3px 4px 0 4px;
	}

	.camera-canvas-container {
		max-width: 100%;
		overflow: hidden;
	}

	.camera-canvas-container :global(.rounded) {
		border-radius: 0 !important;
	}

	.notebook-section {
		flex: 1;
		padding: 2px 4px 4px 4px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.notebook-section :global(.tk-notebook) {
		flex: 1;
	}

	/* ── Tab Content Styles ─────────────────────────────────────────────────── */
	:global(.tab-content) {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 2px;
	}

	/* ── Tkinter-like Form Elements ─────────────────────────────────────────── */
	:global(.tk-labelframe) {
		border: 2px solid var(--color-border, #334155);
		border-radius: 2px;
		background-color: var(--color-bg-card, #1e293b);
		position: relative;
		margin-top: 7px;
	}

	:global(.tk-labelframe-label) {
		position: absolute;
		top: -10px;
		left: 8px;
		background-color: var(--color-bg-card, #1e293b);
		padding: 0 4px;
		font-size: 11px;
		font-weight: 600;
		color: var(--color-text-secondary, #94a3b8);
		font-family: 'Segoe UI', system-ui, sans-serif;
		z-index: 1;
		line-height: 1;
	}

	:global(.tk-labelframe-content) {
		padding: 8px 4px 4px 4px;
	}

	:global(.tk-inset) {
		margin: 4px 0;
	}

	:global(.tk-inset) > .tk-labelframe-label {
		background-color: var(--color-bg-card, #1e293b);
	}

	:global(.form-row) {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 2px 0;
		flex-wrap: wrap;
	}

	:global(.form-label) {
		font-size: 11px;
		color: var(--color-text-secondary, #94a3b8);
		white-space: nowrap;
		flex-shrink: 0;
	}

	:global(.tk-input) {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 4px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-primary, #0f172a);
		color: var(--color-text-primary, #f1f5f9);
	}

	:global(.tk-input-narrow) {
		width: 50px;
	}

	:global(.tk-select) {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 2px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-primary, #0f172a);
		color: var(--color-text-primary, #f1f5f9);
	}

	:global(.tk-select-wide) {
		min-width: 180px;
		flex: 1;
	}

	:global(.tk-btn) {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 10px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
		white-space: nowrap;
		line-height: 1.4;
	}

	:global(.tk-btn:hover) {
		background-color: var(--color-accent-hover, #3b82f6);
		color: #fff;
	}

	:global(.tk-btn:active) {
		background-color: var(--color-accent, #60a5fa);
	}

	:global(.tk-btn-start) {
		background-color: #1b5e20;
		border-color: #2e7d32;
		color: #a5d6a7;
		font-weight: 600;
	}

	:global(.tk-btn-start:hover) {
		background-color: #2e7d32;
		color: #fff;
	}

	:global(.tk-btn-shortcut) {
		flex: 1;
		text-align: center;
		padding: 4px 6px;
		font-size: 10px;
	}

	:global(.tk-checkbox-label) {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		font-size: 11px;
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
	}

	:global(.tk-checkbox) {
		accent-color: var(--color-accent, #60a5fa);
	}

	:global(.tk-radio-label) {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		font-size: 11px;
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
	}

	:global(.tk-radio) {
		accent-color: var(--color-accent, #60a5fa);
	}

	:global(.tk-separator-v) {
		width: 1px;
		height: 18px;
		background-color: var(--color-border, #475569);
		flex-shrink: 0;
	}

	:global(.tk-range) {
		width: 200px;
		accent-color: var(--color-accent, #60a5fa);
	}

	/* ── Inner notebook (for command selector) ─────────────────────────────── */
	:global(.tk-inner-notebook) {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--color-border, #334155);
		border-radius: 2px;
	}

	:global(.tk-inner-notebook) .tk-notebook-tabs {
		background-color: var(--color-bg-tertiary, #334155);
		border-bottom: 1px solid var(--color-border, #475569);
		display: flex;
	}

	:global(.tk-inner-notebook) .tk-notebook-tab {
		font-size: 11px;
		padding: 3px 12px;
		border: none;
		border-right: 1px solid var(--color-border, #475569);
		background-color: transparent;
		color: var(--color-text-secondary, #94a3b8);
		cursor: pointer;
	}

	:global(.tk-inner-notebook) .tk-notebook-tab.active {
		background-color: var(--color-bg-card, #1e293b);
		color: var(--color-text-primary, #f1f5f9);
	}

	:global(.tk-inner-notebook) .tk-notebook-content {
		padding: 4px;
		display: flex;
		flex-direction: column;
		gap: 3px;
	}

</style>
