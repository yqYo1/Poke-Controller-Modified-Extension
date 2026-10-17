import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import ScriptUiLayer from './ScriptUiLayer.svelte';

function scriptView(): RuntimeView {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return {
    ...initial,
    scriptUi: {
      dialogs: [
        {
          description: 'Choose a value',
          id: '4',
          title: 'Script prompt',
          widgets: [
            {
              kind: 'entry',
              label: 'Name',
              maximum: null,
              minimum: null,
              options: [],
              precision: null,
              value: { type: 'string', value: 'before' }
            }
          ]
        }
      ],
      generation: 'user-script-2',
      overlay: {
        bindings: { left: false, right: false },
        fps: 30,
        right_mouse_mode: 'Default',
        shapes: [],
        show_height: 720,
        show_width: 1280,
        touchscreen_area: { height: 1, width: 1, x: 0, y: 0 }
      },
      popup_images: [],
      tk_windows: [
        {
          geometry: null,
          id: '8',
          title: 'Compatibility controls',
          widgets: [{ id: '9', kind: 'button', pady: 2, text: 'Detect' }]
        }
      ]
    }
  };
}

describe('ScriptUiLayer', () => {
  it('submits typed dialog values with the active generation', async () => {
    const actions = {
      scriptUiAction: vi.fn().mockResolvedValue({ accepted: true })
    };
    render(ScriptUiLayer, { actions, view: scriptView() });

    expect(screen.getByRole('dialog', { name: 'Script prompt' })).toBeTruthy();
    await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), {
      target: { value: 'after' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'OK' }));

    await waitFor(() => {
      expect(actions.scriptUiAction).toHaveBeenCalledWith({
        action: 'dialog_confirm',
        dialog_id: '4',
        generation: 'user-script-2',
        values: [{ type: 'string', value: 'after' }]
      });
    });
  });

  it('forwards Tk compatibility callbacks without closing the command', async () => {
    const actions = {
      scriptUiAction: vi.fn().mockResolvedValue({ accepted: true })
    };
    render(ScriptUiLayer, { actions, view: scriptView() });

    await fireEvent.click(screen.getByRole('button', { name: 'Detect' }));

    await waitFor(() => {
      expect(actions.scriptUiAction).toHaveBeenCalledWith({
        action: 'tk_button_invoked',
        generation: 'user-script-2',
        widget_id: '9'
      });
    });
  });
});
