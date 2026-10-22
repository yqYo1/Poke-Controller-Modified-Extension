import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import DiagnosticsDialog from './DiagnosticsDialog.svelte';

function runtimeView(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return {
    runtime,
    view: {
      ...initial,
      settings: settingsSnapshot('7'),
      state: stateSnapshot('7', {
        serial_connected: true,
        serial_port: '/dev/ttyUSB0',
        camera_opened: true,
        camera_resolution: '1280x720',
        command_state: 'running',
        current_command: 'sample.py'
      })
    }
  };
}

describe('DiagnosticsDialog', () => {
  it('shows environment state and verifies write access', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('8')
    });
    render(DiagnosticsDialog, { runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: '環境の診断' }));
    expect(screen.getByText('0.1.0')).toBeTruthy();
    expect(screen.getByText(/\/dev\/ttyUSB0/)).toBeTruthy();
    expect(screen.getByText(/1280x720/)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: '書込可否を確認' }));
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ language: 'ja' });
      expect(screen.getByText('書込可')).toBeTruthy();
    });
  });
});
