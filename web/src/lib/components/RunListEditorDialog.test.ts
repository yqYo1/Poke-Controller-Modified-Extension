import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { RunListSpec } from '../actions';
import RunListEditorDialog from './RunListEditorDialog.svelte';

function scheduleSnapshot(overrides: Record<string, unknown> = {}) {
  return {
    pending: false,
    has_start: false,
    has_stop: false,
    start_display: '予約はありません',
    stop_display: '予約はありません',
    message: '',
    sleep_warning: '予約した時刻に PC がスリープしていると、予約は正常に動作しません。',
    ...overrides
  };
}

function runListActions(lists: RunListSpec[] = []) {
  return {
    runLists: vi.fn().mockResolvedValue({ lists, skipped: [] }),
    saveRunLists: vi
      .fn()
      .mockImplementation((request: { lists: RunListSpec[] }) =>
        Promise.resolve({ lists: request.lists, skipped: [] })
      ),
    runSchedule: vi.fn().mockResolvedValue(scheduleSnapshot()),
    controlRunSchedule: vi
      .fn()
      .mockImplementation(() => Promise.resolve(scheduleSnapshot({ pending: true })))
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('RunListEditorDialog', () => {
  it('loads saved lists and shows their rows', async () => {
    const actions = runListActions([
      {
        name: 'Daily',
        items: [
          { type: 'Python', name: 'wave', repeat: 1, limit_minutes: null },
          { type: 'Mcu', name: 'press', repeat: 2, limit_minutes: 5 }
        ],
        interval_seconds: 1,
        on_error: 'stop',
        loops: 1
      }
    ]);
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    expect(await screen.findByRole('button', { name: 'Edit run list Daily' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Select row 1' }).textContent).toContain('Python');
    expect(screen.getByRole('button', { name: 'Select row 2' }).textContent).toContain('press');
  });

  it('creates a new list from the selected command and saves it', async () => {
    const actions = runListActions([]);
    render(RunListEditorDialog, {
      actions,
      selected: {
        name: 'wave',
        type: 'Python',
        module_path: 'Commands.PythonCommands.samples.wave',
        class_name: 'wave'
      },
      onclose: vi.fn()
    });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Add selected command' })
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(actions.saveRunLists).toHaveBeenCalled());
    expect(actions.saveRunLists).toHaveBeenCalledWith({
      lists: [
        {
          name: 'New list',
          items: [{ type: 'Python', name: 'wave', repeat: 1, limit_minutes: null }],
          interval_seconds: 1,
          on_error: 'stop',
          loops: 1
        }
      ]
    });
    expect(await screen.findByText(/Saved\. Start "New list \(実行リスト\)"/)).toBeTruthy();
  });

  it('rejects Mcu rows without a limit before saving', async () => {
    const actions = runListActions([
      {
        name: 'Mcu list',
        items: [{ type: 'Mcu', name: 'press', repeat: 1, limit_minutes: null }],
        interval_seconds: 1,
        on_error: 'stop',
        loops: 1
      }
    ]);
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await screen.findByRole('button', { name: 'Select row 1' });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(
      await screen.findByText('Row 1: Mcu commands need the limit minutes.')
    ).toBeTruthy();
    expect(actions.saveRunLists).not.toHaveBeenCalled();
  });

  it('deletes the selected list after confirmation', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true)
    );
    const actions = runListActions([
      {
        name: 'Daily',
        items: [{ type: 'Python', name: 'wave' }],
        interval_seconds: 1,
        on_error: 'stop',
        loops: 1
      }
    ]);
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await screen.findByRole('button', { name: 'Select row 1' });
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(actions.saveRunLists).toHaveBeenCalledWith({ lists: [] }));
    expect(await screen.findByText('Deleted "Daily".')).toBeTruthy();
  });

  it('moves the selected row up before saving', async () => {
    const actions = runListActions([
      {
        name: 'Ordered',
        items: [
          { type: 'Python', name: 'a', repeat: 1, limit_minutes: null },
          { type: 'Python', name: 'b', repeat: 1, limit_minutes: null }
        ],
        interval_seconds: 2,
        on_error: 'next',
        loops: 3
      }
    ]);
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Select row 2' })
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Up' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(actions.saveRunLists).toHaveBeenCalled());
    const request = actions.saveRunLists.mock.calls[0]?.[0] as { lists: RunListSpec[] };
    expect(request.lists[0]?.items?.map((item) => item.name)).toEqual(['b', 'a']);
  });

  it('reserves a start for the selected command from the schedule tab', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true)
    );
    const actions = runListActions([]);
    render(RunListEditorDialog, {
      actions,
      selected: {
        name: 'wave',
        type: 'Python',
        module_path: 'Commands.PythonCommands.samples.wave',
        class_name: 'wave'
      },
      onclose: vi.fn()
    });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('tab', { name: 'Schedule' }));
    await waitFor(() => expect(actions.runSchedule).toHaveBeenCalled());
    await fireEvent.input(screen.getByLabelText('Start time'), {
      target: { value: '23:30' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Reserve start' }));

    await waitFor(() => expect(actions.controlRunSchedule).toHaveBeenCalled());
    expect(actions.controlRunSchedule).toHaveBeenCalledWith({
      action: 'reserve_start',
      target: {
        module_path: 'Commands.PythonCommands.samples.wave',
        class_name: 'wave',
        name: 'wave'
      },
      clock: '23:30'
    });
  });

  it('cancels the start reservation', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true)
    );
    const actions = runListActions([]);
    actions.runSchedule.mockResolvedValue(
      scheduleSnapshot({
        pending: true,
        has_start: true,
        start_display: '2026-10-11 23:30:00 に「wave」を開始(あと 30 秒)'
      })
    );
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('tab', { name: 'Schedule' }));
    await waitFor(() => expect(actions.runSchedule).toHaveBeenCalled());
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Cancel the start reservation' })
    );

    await waitFor(() =>
      expect(actions.controlRunSchedule).toHaveBeenCalledWith({ action: 'cancel_start' })
    );
  });

  it('shows the reason when a stop reservation is rejected', async () => {
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true)
    );
    const actions = runListActions([]);
    actions.controlRunSchedule.mockRejectedValue(
      new Error(
        'コマンドを実行中か、開始を予約しているときだけ、「開始から」の停止を予約できます。'
      )
    );
    render(RunListEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('tab', { name: 'Schedule' }));
    await waitFor(() => expect(actions.runSchedule).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('radio', { name: /After start/ }));
    await fireEvent.input(screen.getByLabelText('Stop minutes'), {
      target: { value: '5' }
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Reserve stop' }));

    expect(
      await screen.findByText(
        'コマンドを実行中か、開始を予約しているときだけ、「開始から」の停止を予約できます。'
      )
    ).toBeTruthy();
  });
});
