import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import ControllerPanel from './ControllerPanel.svelte';

function createRuntime(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let view: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((next) => {
    view = next;
  });
  unsubscribe();
  if (view === undefined) throw new Error('runtime did not publish its initial view');
  return { runtime, view };
}

function allowPointerCapture(element: HTMLElement): void {
  Object.defineProperty(element, 'setPointerCapture', {
    configurable: true,
    value: vi.fn()
  });
}

describe('ControllerPanel', () => {
  it('sends button transitions and preserves a Shift-held release', async () => {
    const { runtime, view } = createRuntime();
    const setButton = vi.spyOn(runtime, 'setGamepadButton');
    render(ControllerPanel, { runtime, view });
    const button = screen.getByRole('button', { name: 'A' });
    allowPointerCapture(button);

    await fireEvent.pointerDown(button, { button: 0, pointerId: 7 });
    await fireEvent.pointerUp(button, { button: 0, pointerId: 7, shiftKey: true });
    expect(setButton.mock.calls).toEqual([['A', true]]);

    await fireEvent.pointerDown(button, { button: 0, pointerId: 8 });
    await fireEvent.pointerUp(button, { button: 0, pointerId: 8 });
    expect(setButton.mock.calls.slice(-2)).toEqual([
      ['A', true],
      ['A', false]
    ]);
  });

  it('maps D-pad, stick keyboard, and touch pointer input', async () => {
    const { runtime, view } = createRuntime();
    const setHat = vi.spyOn(runtime, 'setGamepadHat');
    const setStick = vi.spyOn(runtime, 'setGamepadStick');
    const setTouch = vi.spyOn(runtime, 'setGamepadTouch');
    render(ControllerPanel, { runtime, view });

    const up = screen.getByRole('button', { name: 'D-pad up' });
    allowPointerCapture(up);
    await fireEvent.pointerDown(up, { button: 0, pointerId: 1 });
    await fireEvent.pointerUp(up, { button: 0, pointerId: 1 });
    expect(setHat.mock.calls).toEqual([['UP'], ['CENTER']]);

    await fireEvent.keyDown(screen.getByRole('button', { name: /Left stick/ }), {
      key: 'ArrowRight'
    });
    expect(setStick).toHaveBeenLastCalledWith('LSTICK', 136, 128);

    const touch = screen.getByRole('button', { name: 'Touchscreen 320 × 240' });
    allowPointerCapture(touch);
    vi.spyOn(touch, 'getBoundingClientRect').mockReturnValue({
      bottom: 240,
      height: 240,
      left: 0,
      right: 320,
      top: 0,
      width: 320,
      x: 0,
      y: 0,
      toJSON: () => ({})
    });
    await fireEvent.pointerDown(touch, {
      button: 0,
      clientX: 160,
      clientY: 120,
      pointerId: 2
    });
    await fireEvent.pointerUp(touch, { button: 0, pointerId: 2 });
    expect(setTouch.mock.calls).toEqual([
      [{ pressed: true, x: 160, y: 120 }],
      [null]
    ]);
  });
});
