import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime } from '../runtime';
import SoftwareKeyboard from './SoftwareKeyboard.svelte';

describe('SoftwareKeyboard', () => {
  it('converts kana input to romaji key events', async () => {
    const runtime = new ApplicationRuntime();
    const setKeyboardKey = vi.spyOn(runtime, 'setKeyboardKey');
    render(SoftwareKeyboard, { runtime, language: 'ja' });

    await fireEvent.click(screen.getByRole('button', { name: 'キーボード' }));
    await fireEvent.input(screen.getByLabelText('送信する文字列'), {
      target: { value: 'あ' }
    });
    expect(screen.getByText('かな → ローマ字')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '送信' }));

    await waitFor(() => {
      expect(setKeyboardKey).toHaveBeenCalledWith('KeyA', true);
      expect(setKeyboardKey).toHaveBeenCalledWith('KeyA', false);
    });
  });
});
