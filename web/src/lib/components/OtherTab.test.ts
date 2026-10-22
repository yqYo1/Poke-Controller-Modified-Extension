import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
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

  it('writes the controller layout', async () => {
    const { runtime, view } = runtimeView(
      settingsSnapshot('1', { 'ui.controller_layout': 'standard' })
    );
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { 'ui.controller_layout': 'stacked' })
    });
    render(OtherTab, { runtime, view });

    await fireEvent.change(screen.getByLabelText('コントローラーレイアウト'), {
      target: { value: 'stacked' }
    });
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.controller_layout': 'stacked' });
    });
  });

  it('writes the right panel position', async () => {
    const { runtime, view } = runtimeView(
      settingsSnapshot('1', { 'ui.right_panel_position': 'right' })
    );
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { 'ui.right_panel_position': 'bottom' })
    });
    render(OtherTab, { runtime, view });

    await fireEvent.change(screen.getByLabelText('右パネル位置'), {
      target: { value: 'bottom' }
    });
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.right_panel_position': 'bottom' });
    });
  });

  it('writes the controller body color preset', async () => {
    const { runtime, view } = runtimeView(
      settingsSnapshot('1', { 'ui.controller_body_color': 'classic' })
    );
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { 'ui.controller_body_color': 'purple' })
    });
    render(OtherTab, { runtime, view });

    await fireEvent.change(screen.getByLabelText('コントローラー本体色'), {
      target: { value: 'purple' }
    });
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.controller_body_color': 'purple' });
    });
  });

  it('edits and saves the declared python library list', async () => {
    const { runtime, view } = runtimeView(
      settingsSnapshot('4', {
        'python.script.packages.list': [{ name: 'numpy', version: '2.0' }]
      })
    );
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('5')
    });
    render(OtherTab, { runtime, view });

    const firstName = screen.getByLabelText('Library name 1');
    expect(firstName).toHaveProperty('value', 'numpy');

    await fireEvent.click(screen.getByRole('button', { name: 'ライブラリを追加' }));
    await fireEvent.input(screen.getByLabelText('Library name 2'), {
      target: { value: 'requests' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'ライブラリを保存' }));

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'python.script.packages.list': [
          { name: 'numpy', version: '2.0' },
          { name: 'requests' }
        ]
      });
    });
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

  it('defaults the controller position to bottom while settings are not loaded', () => {
    const { runtime, view } = runtimeView();
    render(OtherTab, { runtime, view: { ...view, settings: null } });

    const controller = within(screen.getByRole('group', { name: 'コントローラーの位置' }));
    expect(controller.getByRole<HTMLInputElement>('radio', { name: 'BOTTOM' }).checked).toBe(true);
    expect(controller.getByRole<HTMLInputElement>('radio', { name: 'TOP' }).checked).toBe(false);
    const dialog = within(screen.getByRole('group', { name: 'ダイアログボタンの位置' }));
    expect(dialog.getByRole<HTMLInputElement>('radio', { name: 'BOTTOM' }).checked).toBe(true);
  });

  it('renders radio controls for both position settings', () => {
    const { runtime, view } = runtimeView();
    render(OtherTab, { runtime, view });

    const controller = within(screen.getByRole('group', { name: 'コントローラーの位置' }));
    expect(
      controller
        .getAllByRole<HTMLInputElement>('radio')
        .map((radio) => radio.value)
    ).toEqual(['top', 'bottom']);
    expect(controller.getByRole<HTMLInputElement>('radio', { name: 'BOTTOM' }).checked).toBe(true);

    const dialog = within(screen.getByRole('group', { name: 'ダイアログボタンの位置' }));
    expect(
      dialog
        .getAllByRole<HTMLInputElement>('radio')
        .map((radio) => radio.value)
    ).toEqual(['top', 'bottom', 'both']);
    expect(dialog.getByRole<HTMLInputElement>('radio', { name: 'BOTTOM' }).checked).toBe(true);

    expect(
      screen.queryByRole('combobox', { name: /コントローラーの位置|ダイアログボタンの位置/ })
    ).toBeNull();
  });

  it('writes position settings from the radio controls', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('5')
    });
    render(OtherTab, { runtime, view });

    await fireEvent.click(
      within(screen.getByRole('group', { name: 'コントローラーの位置' })).getByRole('radio', {
        name: 'TOP'
      })
    );
    await fireEvent.click(
      within(screen.getByRole('group', { name: 'ダイアログボタンの位置' })).getByRole('radio', {
        name: 'BOTH'
      })
    );

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.controller_position': 'top' });
      expect(writeSettings).toHaveBeenCalledWith({ 'ui.dialog_button_position': 'both' });
    });
  });
});
