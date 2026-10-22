import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot } from '../test-fixtures';
import ManualTab from './ManualTab.svelte';

function runtimeView(keyboardEnabled = true): { runtime: ApplicationRuntime; view: RuntimeView } {
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
      settings: settingsSnapshot('1', { 'input.keyboard_enabled': keyboardEnabled })
    }
  };
}

describe('ManualTab', () => {
  it('writes input-source toggles and ignores disabled keyboard input', async () => {
    const { runtime, view } = runtimeView(false);
    const setKeyboardKey = vi.spyOn(runtime, 'setKeyboardKey');
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', {
        'input.keyboard_enabled': false,
        'input.left_stick_mouse_enabled': true
      })
    });
    render(ManualTab, { runtime, view });

    await fireEvent.keyDown(window, { code: 'KeyB', key: 'b' });
    expect(setKeyboardKey).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('checkbox', { name: /L-stick mouse/ }));
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'input.left_stick_mouse_enabled': true
      });
    });
  });

  it('writes the normalized touchscreen rectangle', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', {
        'input.touchscreen_area': { left: 0.1, top: 0.2, right: 0.9, bottom: 0.8 }
      })
    });
    render(ManualTab, { runtime, view });

    await fireEvent.input(screen.getByLabelText('Touch area right'), {
      target: { value: '0.9' }
    });
    await fireEvent.input(screen.getByLabelText('Touch area bottom'), {
      target: { value: '0.8' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Apply touch area' }));

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'input.touchscreen_area': { left: 0, top: 0, right: 0.9, bottom: 0.8 }
      });
    });
  });
});
