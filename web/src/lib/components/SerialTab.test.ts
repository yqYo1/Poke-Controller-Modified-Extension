import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import SerialTab from './SerialTab.svelte';

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
      settings: settingsSnapshot('1', { 'serial.port': 'COM3' }),
      state: stateSnapshot('1', { serial_port: null })
    }
  };
}

function serialActions() {
  return {
    controlSerial: vi.fn().mockResolvedValue({ changed: true, revision: '2' }),
    serialPorts: vi.fn().mockResolvedValue([
      { available: true, label: 'Controller COM3', selector: 'COM3' },
      { available: true, label: 'Controller COM4', selector: 'COM4' }
    ])
  };
}

describe('SerialTab', () => {
  it('writes a raw port selector and connects with effective settings', async () => {
    const { runtime, view } = runtimeView();
    const actions = serialActions();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { 'serial.port': 'COM4' })
    });
    render(SerialTab, { actions, autoLoad: false, runtime, view });

    const port = screen.getByLabelText('Port selector');
    await fireEvent.input(port, { target: { value: 'COM4' } });
    await fireEvent.change(port);
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'serial.port': 'COM4' });
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Connect' }));
    await waitFor(() => {
      expect(actions.controlSerial).toHaveBeenCalledWith({ action: 'connect' });
    });
  });

  it('keeps Connect available for switching while already connected', async () => {
    const { runtime, view: initialView } = runtimeView();
    const view = {
      ...initialView,
      state: stateSnapshot('1', { serial_connected: true, serial_port: 'COM3' })
    };
    const actions = serialActions();
    render(SerialTab, { actions, autoLoad: false, runtime, view });

    const connect = screen.getByRole('button', { name: 'Connect' });
    expect(connect).toHaveProperty('disabled', false);
    expect(screen.getByRole('button', { name: 'Disconnect' })).toHaveProperty('disabled', false);

    await fireEvent.click(connect);
    await waitFor(() => {
      expect(actions.controlSerial).toHaveBeenCalledWith({ action: 'connect' });
    });
  });

  it('atomically selects 3DS format and its recommended baud rate', async () => {
    const { runtime, view } = runtimeView();
    const actions = serialActions();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', {
        'serial.baud_rate': 115200,
        'serial.data_format': '3ds',
        'serial.port': 'COM3'
      })
    });
    render(SerialTab, { actions, autoLoad: false, runtime, view });

    await fireEvent.change(screen.getByLabelText('Data format'), {
      target: { value: '3ds' }
    });

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'serial.baud_rate': 115200,
        'serial.data_format': '3ds'
      });
    });
  });
});
