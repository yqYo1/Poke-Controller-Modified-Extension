import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import CameraTab from './CameraTab.svelte';

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
      settings: settingsSnapshot('1'),
      state: stateSnapshot('1', { camera_device: 0, camera_opened: true })
    }
  };
}

function cameraActions() {
  return {
    cameras: vi.fn().mockResolvedValue([
      { available: true, label: 'Camera 0', selector: 0 },
      { available: true, label: 'Camera 1', selector: 1 }
    ]),
    downloadScreenshot: vi.fn().mockResolvedValue({
      blob: new Blob(),
      contentDisposition: null
    }),
    retryCamera: vi.fn().mockResolvedValue({ changed: false, revision: '1' }),
    saveScreenshot: vi.fn().mockResolvedValue({
      display_path: '/data/Captures/capture.png',
      format: 'png' as const
    })
  };
}

describe('CameraTab', () => {
  it('enumerates devices and writes typed camera settings', async () => {
    const { runtime, view } = runtimeView();
    const actions = cameraActions();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { 'camera.device': 1 })
    });
    render(CameraTab, { actions, autoLoad: false, runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await screen.findByRole('option', { name: 'Camera 1' });
    await fireEvent.change(screen.getByLabelText('Camera device'), {
      target: { value: 'number:1' }
    });

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'camera.device': 1 });
    });
  });

  it('saves the current frame through the captures API', async () => {
    const { runtime, view } = runtimeView();
    const actions = cameraActions();
    render(CameraTab, { actions, autoLoad: false, runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: 'Save to Captures' }));

    await waitFor(() => {
      expect(actions.saveScreenshot).toHaveBeenCalledWith({
        destination: 'captures',
        format: 'png',
        region: null
      });
    });
    expect(await screen.findByText('Saved: /data/Captures/capture.png')).toBeTruthy();
  });
});
