import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot } from '../test-fixtures';
import OtherTab from './OtherTab.svelte';

function runtimeView(settings = settingsSnapshot('1')): {
  runtime: ApplicationRuntime;
  view: RuntimeView;
} {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return { runtime, view: { ...initial, settings } };
}

describe('OtherTab', () => {
  it('connects display controls and the clear action', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2')
    });
    const clearOutputs = vi.spyOn(runtime, 'clearOutputs');
    render(OtherTab, { runtime, view });

    await fireEvent.change(screen.getByRole('slider'), { target: { value: '75' } });
    await fireEvent.change(screen.getByLabelText('表示言語'), { target: { value: 'en' } });
    await fireEvent.click(screen.getByRole('button', { name: '両方の出力をクリア' }));

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.output_split_ratio': 75 });
      expect(writeSettings).toHaveBeenCalledWith({ language: 'en' });
    });
    expect(clearOutputs).toHaveBeenCalledOnce();
  });

  it('separates startup-only current and saved values and exposes failures', () => {
    const base = settingsSnapshot('3');
    const { runtime, view } = runtimeView({
      ...base,
      apply_failures: { auto_reload_config: 'watcher unavailable' },
      pending_restart_values: { 'ui.desktop.disable_compositing': true },
      restart_required: ['ui.desktop.disable_compositing']
    });
    render(OtherTab, { runtime, view });

    expect(screen.getByText(/現在.*false.*保存.*true/).textContent).toContain('false');
    expect(screen.getByText('再起動後に反映')).toBeTruthy();
    expect(screen.getByText(/watcher unavailable/)).toBeTruthy();
    expect(
      screen.getByRole<HTMLInputElement>('checkbox', {
        name: /デスクトップコンポジット無効化/
      }).checked
    ).toBe(true);
  });

  it('writes server settings and warns for a saved LAN bind address', async () => {
    const base = settingsSnapshot('4');
    const { runtime, view } = runtimeView({
      ...base,
      pending_restart_values: {
        'server.bind_address': '192.168.1.50',
        'server.port': 9000,
        'server.web_dir': '/srv/pokecon'
      },
      restart_required: ['server.bind_address', 'server.port', 'server.web_dir']
    });
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: base
    });
    render(OtherTab, { runtime, view });

    expect(screen.getByRole('alert').textContent).toContain('認証なし');
    const port = screen.getByRole<HTMLInputElement>('spinbutton', {
      name: /保存するサーバーポート/
    });
    expect(port.value).toBe('9000');
    await fireEvent.change(port, {
      target: { value: '9001' }
    });
    await fireEvent.change(screen.getByLabelText(/WebRTC STUN URI/), {
      target: { value: 'stun:stun.example.com:3478' }
    });

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'server.port': 9001 });
      expect(writeSettings).toHaveBeenCalledWith({
        stun_server: 'stun:stun.example.com:3478'
      });
    });
  });
});
