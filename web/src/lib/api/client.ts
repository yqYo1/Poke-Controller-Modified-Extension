// ── Output / widget / position settings interfaces ─────────────────────────

export interface OutputSettings {
	split_ratio: number; // 0-100, percentage for Output#1
	stdout_destination: 1 | 2;
}

export interface WidgetSettings {
	mode: string;
}

export interface ControllerPositionSettings {
	position: 'top' | 'bottom';
}

export interface DialogueButtonPositionSettings {
	position: 'top' | 'bottom' | 'both';
}

	// ── Output settings ────────────────────────────────────────────────────

	getOutputSettings() {
		return this._fetch<OutputSettings>('/api/settings/output');
	}
	updateOutputSettings(data: Partial<OutputSettings>) {
		return this._fetch<SuccessResponse>('/api/settings/output', {
			method: 'POST',
			body: JSON.stringify(data),
		});
	}

	// ── Widget mode ────────────────────────────────────────────────────────

	getWidgetMode() {
		return this._fetch<WidgetSettings>('/api/settings/widget');
	}
	updateWidgetMode(mode: string) {
		return this._fetch<SuccessResponse>('/api/settings/widget', {
			method: 'POST',
			body: JSON.stringify({ mode }),
		});
	}

	// ── Software controller position ──────────────────────────────────────

	getControllerPosition() {
		return this._fetch<ControllerPositionSettings>('/api/settings/controller/position');
	}
	updateControllerPosition(position: 'top' | 'bottom') {
		return this._fetch<SuccessResponse>('/api/settings/controller/position', {
			method: 'POST',
			body: JSON.stringify({ position }),
		});
	}

	// ── Dialogue button position ───────────────────────────────────────────

	getDialogueButtonPosition() {
		return this._fetch<DialogueButtonPositionSettings>('/api/settings/dialogue');
	}
	updateDialogueButtonPosition(position: 'top' | 'bottom' | 'both') {
		return this._fetch<SuccessResponse>('/api/settings/dialogue', {
			method: 'POST',
			body: JSON.stringify({ position }),
		});
	}


