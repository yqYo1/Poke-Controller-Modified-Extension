import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import ScriptCreatorDialog from './ScriptCreatorDialog.svelte';

function creatorActions(
  snapshot: {
    author: string;
    bridge_available: boolean;
    script_folders: string[];
    template_folders: string[];
  } = {
    author: '作成者',
    bridge_available: true,
    script_folders: ['Samples'],
    template_folders: ['icons']
  }
) {
  return {
    createScript: vi.fn().mockResolvedValue({
      class_name: 'Sample',
      command_name: 'Sample (sample)',
      created_files: ['/data/Commands/PythonCommands/README.md'],
      created_folders: [],
      path: '/data/Commands/PythonCommands/sample.py',
      skipped_files: []
    }),
    previewScriptCreation: vi.fn().mockResolvedValue({
      error: '',
      lines: ['作るファイル: Commands/PythonCommands/sample.py']
    }),
    scriptCreatorSnapshot: vi.fn().mockResolvedValue(snapshot)
  };
}

describe('ScriptCreatorDialog', () => {
  it('prefills the remembered author and folder choices', async () => {
    const actions = creatorActions();
    render(ScriptCreatorDialog, { actions, onclose: vi.fn() });

    await waitFor(() => expect(actions.scriptCreatorSnapshot).toHaveBeenCalled());
    await waitFor(() => {
      const author = screen.getByLabelText(/Author/);
      expect(author).toHaveProperty('value', '作成者');
    });
    const folder = screen.getByLabelText(/Folder/);
    expect(folder.getAttribute('list')).toBe('script-creator-folders');
  });

  it('previews the creation and renders the result after creating', async () => {
    const actions = creatorActions();
    render(ScriptCreatorDialog, { actions, onclose: vi.fn() });

    await waitFor(() => expect(actions.scriptCreatorSnapshot).toHaveBeenCalled());
    await fireEvent.input(screen.getByLabelText(/Script name/), { target: { value: 'Sample' } });
    await fireEvent.input(screen.getByLabelText(/File name/), { target: { value: 'sample' } });

    await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    await waitFor(() => {
      expect(actions.previewScriptCreation).toHaveBeenCalledWith(
        expect.objectContaining({ file_name: 'sample', name: 'Sample' })
      );
    });
    expect(await screen.findByText('作るファイル: Commands/PythonCommands/sample.py')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(actions.createScript).toHaveBeenCalled());
    expect(await screen.findByText('/data/Commands/PythonCommands/sample.py')).toBeTruthy();
  });

  it('locks the image-folder location without bridge functions', async () => {
    const actions = creatorActions({
      author: '',
      bridge_available: false,
      script_folders: [],
      template_folders: []
    });
    render(ScriptCreatorDialog, { actions, onclose: vi.fn() });

    await waitFor(() => expect(actions.scriptCreatorSnapshot).toHaveBeenCalled());
    const radios = screen.getAllByRole('radio');
    for (const radio of radios) {
      expect((radio as HTMLInputElement).disabled).toBe(true);
    }
    expect(screen.getByText('(bridge_functions is not installed)')).toBeTruthy();
  });
});
