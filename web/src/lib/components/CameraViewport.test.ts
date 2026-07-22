import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import CameraViewport from './CameraViewport.svelte';

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

function prepareCanvas(canvas: HTMLElement): void {
  Object.defineProperty(canvas, 'setPointerCapture', {
    configurable: true,
    value: vi.fn()
  });
  vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
    bottom: 450,
    height: 360,
    left: 10,
    right: 650,
    top: 90,
    width: 640,
    x: 10,
    y: 90,
    toJSON: () => ({})
  });
}

describe('CameraViewport', () => {
  it('normalizes crop and touch-area drag gestures', async () => {
    const { runtime, view } = runtimeView();
    const oncapture = vi.fn();
    const ontoucharea = vi.fn();
    render(CameraViewport, {
      fps: 30,
      guideVisible: false,
      leftStickEnabled: false,
      liveViewEnabled: true,
      oncapture,
      ondownload: vi.fn(),
      ontoucharea,
      pixelValuesVisible: false,
      rightStickEnabled: false,
      runtime,
      view
    });
    const canvas = screen.getByLabelText('Camera capture area');
    prepareCanvas(canvas);

    await fireEvent.pointerDown(canvas, {
      button: 0,
      clientX: 170,
      clientY: 180,
      ctrlKey: true,
      pointerId: 1,
      shiftKey: true
    });
    await fireEvent.pointerUp(canvas, {
      button: 0,
      clientX: 490,
      clientY: 360,
      ctrlKey: true,
      pointerId: 1,
      shiftKey: true
    });
    expect(oncapture).toHaveBeenCalledWith({ height: 0.5, width: 0.5, x: 0.25, y: 0.25 });

    await fireEvent.pointerDown(canvas, {
      button: 2,
      clientX: 490,
      clientY: 360,
      ctrlKey: true,
      pointerId: 2
    });
    await fireEvent.pointerUp(canvas, {
      button: 2,
      clientX: 170,
      clientY: 180,
      ctrlKey: true,
      pointerId: 2
    });
    expect(ontoucharea).toHaveBeenCalledWith({
      bottom: 0.75,
      left: 0.25,
      right: 0.75,
      top: 0.25
    });
  });
});
