import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { CommandTagSnapshot } from '../actions';
import TagEditorDialog from './TagEditorDialog.svelte';

function snapshot(overrides: Partial<CommandTagSnapshot> = {}): CommandTagSnapshot {
  return {
    commands: [
      {
        name: 'Alpha',
        module_path: 'Commands.PythonCommands.samples.alpha',
        class_name: 'Alpha',
        relative_path: 'samples/alpha.py',
        digest: 'a'.repeat(64),
        manual_tags: ['Rank'],
        automatic_tags: ['@Samples'],
        error: null
      },
      {
        name: 'Beta',
        module_path: 'Commands.PythonCommands.beta',
        class_name: 'Beta',
        relative_path: 'beta.py',
        digest: 'b'.repeat(64),
        manual_tags: ['Device'],
        automatic_tags: [],
        error: null
      }
    ],
    tags: [
      { name: '@Samples', manual: false, automatic: true, count: 1 },
      { name: 'Device', manual: true, automatic: false, count: 1 },
      { name: 'Rank', manual: true, automatic: false, count: 1 }
    ],
    ...overrides
  };
}

function tagActions(initial: CommandTagSnapshot = snapshot()) {
  const actions = {
    commandTags: vi.fn().mockResolvedValue(initial),
    applyCommandTags: vi.fn()
  };
  actions.applyCommandTags.mockImplementation(
    (request: { action: string; edits?: { tags: string[] }[] }) => {
      const base = snapshot();
      if (request.action === 'set') {
        const tags = request.edits?.[0]?.tags ?? [];
        const commands = base.commands.map((command) =>
          command.class_name === 'Alpha' ? { ...command, manual_tags: tags } : command
        );
        return Promise.resolve({
          results: [
            {
              module_path: 'Commands.PythonCommands.samples.alpha',
              class_name: 'Alpha',
              relative_path: 'samples/alpha.py',
              status: 'updated',
              message: null
            }
          ],
          changed: 1,
          conflicts: 0,
          reload_error: null,
          snapshot: {
            commands,
            tags: [
              ...base.tags.filter((tag) => tag.name !== 'Rank' && !tags.includes(tag.name)),
              ...tags.map((name) => ({
                name,
                manual: true,
                automatic: false,
                count: 1
              }))
            ]
          }
        });
      }
      if (request.action === 'rename') {
        return Promise.resolve({
          results: [
            {
              module_path: 'Commands.PythonCommands.samples.alpha',
              class_name: 'Alpha',
              relative_path: 'samples/alpha.py',
              status: 'updated',
              message: null
            }
          ],
          changed: 1,
          conflicts: 0,
          reload_error: null,
          snapshot: snapshot()
        });
      }
      return Promise.resolve({
        results: [
          {
            module_path: 'Commands.PythonCommands.beta',
            class_name: 'Beta',
            relative_path: 'beta.py',
            status: 'updated',
            message: null
          }
        ],
        changed: 1,
        conflicts: 0,
        reload_error: null,
        snapshot: snapshot()
      });
    }
  );
  return actions;
}

describe('TagEditorDialog', () => {
  it('loads commands and saves an added manual tag', async () => {
    const actions = tagActions();
    render(TagEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await waitFor(() => expect(actions.commandTags).toHaveBeenCalled());
    expect(await screen.findByRole('button', { name: 'Edit tags for Alpha' })).toBeTruthy();

    const input = screen.getByLabelText('New tag');
    await fireEvent.input(input, { target: { value: 'Level' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Save tags' }));

    await waitFor(() => expect(actions.applyCommandTags).toHaveBeenCalled());
    expect(actions.applyCommandTags).toHaveBeenCalledWith({
      action: 'set',
      edits: [
        {
          module_path: 'Commands.PythonCommands.samples.alpha',
          class_name: 'Alpha',
          digest: 'a'.repeat(64),
          tags: ['Rank', 'Level']
        }
      ]
    });
    expect(await screen.findByText('1 件のコマンドを更新しました')).toBeTruthy();
  });

  it('rejects a reserved automatic-style tag before adding it', async () => {
    const actions = tagActions();
    render(TagEditorDialog, { actions, selected: null, onclose: vi.fn() });

    const input = await screen.findByLabelText('New tag');
    await fireEvent.input(input, { target: { value: '@Folder' } });

    expect(await screen.findByText('タグの先頭に「@」は使えません（ディレクトリ由来の自動タグ用）')).toBeTruthy();
    const add = screen.getByRole('button', { name: 'Add' });
    expect(add.hasAttribute('disabled')).toBe(true);
  });

  it('renames a tag across commands from the bulk tab', async () => {
    const actions = tagActions();
    render(TagEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await fireEvent.click(await screen.findByRole('tab', { name: 'Bulk tags' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Select tag Rank' }));
    const input = screen.getByLabelText('Rename tag to');
    await fireEvent.input(input, { target: { value: 'Level' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    await waitFor(() => expect(actions.applyCommandTags).toHaveBeenCalled());
    expect(actions.applyCommandTags).toHaveBeenCalledWith({
      action: 'rename',
      from: 'Rank',
      to: 'Level'
    });
  });

  it('requires a second press before deleting a tag', async () => {
    const actions = tagActions();
    render(TagEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await fireEvent.click(await screen.findByRole('tab', { name: 'Bulk tags' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Select tag Device' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(actions.applyCommandTags).not.toHaveBeenCalled();
    expect(await screen.findByText('もう一度 Delete を押すと、すべてのコマンドからこのタグを削除します。')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));
    await waitFor(() => expect(actions.applyCommandTags).toHaveBeenCalled());
    expect(actions.applyCommandTags).toHaveBeenCalledWith({ action: 'delete', tag: 'Device' });
  });

  it('shows a digest conflict warning from the server', async () => {
    const actions = tagActions();
    actions.applyCommandTags.mockResolvedValueOnce({
      results: [
        {
          module_path: 'Commands.PythonCommands.samples.alpha',
          class_name: 'Alpha',
          relative_path: 'samples/alpha.py',
          status: 'conflict',
          message: 'ファイルが編集後に変更されています。再読み込みしてください'
        }
      ],
      changed: 0,
      conflicts: 1,
      reload_error: null,
      snapshot: snapshot()
    });
    render(TagEditorDialog, { actions, selected: null, onclose: vi.fn() });

    await fireEvent.click(await screen.findByRole('button', { name: 'Save tags' }));
    expect(await screen.findByText(/書き込み競合または失敗がありました/)).toBeTruthy();
    expect(screen.getByText(/Alpha \(samples\/alpha\.py\)/)).toBeTruthy();
  });
});
