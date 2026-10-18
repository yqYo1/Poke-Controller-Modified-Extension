import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { RunListSpec } from '../actions';
import RunListEditorDialog from './RunListEditorDialog.svelte';

function runListActions(lists: RunListSpec[] = []) {
  return {
    runLists: vi.fn().mockResolvedValue({ lists, skipped: [] }),
    saveRunLists: vi
      .fn()
      .mockImplementation((request: { lists: RunListSpec[] }) =>
        Promise.resolve({ lists: request.lists, skipped: [] })
      )
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
      selected: { name: 'wave', type: 'Python' },
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
});
