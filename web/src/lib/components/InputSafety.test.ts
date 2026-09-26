import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import InputSafety from './InputSafety.svelte';

function runtimeView(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let view: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((next) => {
    view = next;
  });
  unsubscribe();
  if (view === undefined) throw new Error('runtime did not publish its initial view');
  return { runtime, view };
}

describe('InputSafety', () => {
  it('keeps keyboard ownership and blur neutralization outside the Manual tab', async () => {
    const { runtime, view } = runtimeView();
    const setKeyboardKey = vi.spyOn(runtime, 'setKeyboardKey');
    const neutralizeInput = vi.spyOn(runtime, 'neutralizeInput');
    const { unmount } = render(InputSafety, { runtime, view });

    await fireEvent.keyDown(window, { code: 'KeyA', key: 'a' });
    expect(setKeyboardKey).toHaveBeenCalledWith('KeyA', true);

    window.dispatchEvent(new Event('blur'));
    expect(neutralizeInput).toHaveBeenCalledTimes(1);

    const visibilityState = vi
      .spyOn(document, 'visibilityState', 'get')
      .mockReturnValue('hidden');
    document.dispatchEvent(new Event('visibilitychange'));
    expect(neutralizeInput).toHaveBeenCalledTimes(2);
    visibilityState.mockRestore();

    await fireEvent.keyUp(window, { code: 'KeyA', key: 'a' });
    expect(setKeyboardKey).toHaveBeenCalledTimes(1);
    unmount();
  });
});
