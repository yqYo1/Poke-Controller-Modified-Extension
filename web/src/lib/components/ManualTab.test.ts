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
  it('forwards non-form keyboard codes and releases them', async () => {
    const { runtime, view } = runtimeView();
    const setKeyboardKey = vi.spyOn(runtime, 'setKeyboardKey');
    render(ManualTab, { runtime, view });

    await fireEvent.keyDown(window, { code: 'KeyA', key: 'a' });
    await fireEvent.keyUp(window, { code: 'KeyA', key: 'a' });

    expect(setKeyboardKey.mock.calls).toEqual([
      ['KeyA', true],
      ['KeyA', false]
    ]);
    expect(screen.queryByRole('region', { name: 'Software controller' })).toBeNull();
    expect(
      screen.getByRole('button', { name: 'Focus software controller' })
    ).toBeTruthy();
  });

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
});
