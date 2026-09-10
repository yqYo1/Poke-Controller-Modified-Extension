import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import Page from './+page.svelte';

describe('application shell', () => {
  it('exposes the six canonical main tabs and the right-side widgets', () => {
    render(Page);

    expect(screen.getByRole('heading', { name: 'Controller workspace' })).toBeTruthy();
    expect(screen.getAllByRole('tab')).toHaveLength(6);
    expect(screen.getByRole('tab', { name: 'カメラ' }).getAttribute('aria-selected')).toBe(
      'true'
    );
    expect(screen.getByRole('region', { name: 'Software controller' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Output #1' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Output #2' })).toBeTruthy();
  });

  it('supports keyboard navigation across the tab list', async () => {
    render(Page);
    const camera = screen.getByRole('tab', { name: 'カメラ' });

    await fireEvent.keyDown(camera, { key: 'ArrowRight' });

    expect(screen.getByRole('tab', { name: 'シリアル' }).getAttribute('aria-selected')).toBe(
      'true'
    );
    expect(screen.getByRole('tabpanel').getAttribute('aria-labelledby')).toBe('tab-serial');
  });

  it('keeps the six settings tabs in the canonical order', () => {
    render(Page);

    const names = screen.getAllByRole('tab').map((tab) => tab.textContent.trim());
    expect(names).toEqual(['カメラ', 'シリアル', '手動制御', 'コマンド', '通知', 'その他']);
  });

  it('keeps one persistent Main Panel with quick actions and the live camera preview', async () => {
    render(Page);

    const mainPanel = screen.getByRole('region', { name: 'Main Panel' });
    const toolbar = within(mainPanel).getByRole('toolbar', { name: 'Quick actions' });
    for (const name of [
      'Start',
      'Controller',
      'Clear Outputs',
      'Capture',
      'Capture folder',
      'Discord'
    ]) {
      expect(within(toolbar).getByRole('button', { name })).toBeTruthy();
    }
    expect(within(mainPanel).getByLabelText('Camera capture area')).toBeTruthy();

    await fireEvent.click(screen.getByRole('tab', { name: '手動制御' }));

    expect(screen.getByRole('region', { name: 'Main Panel' })).toBeTruthy();
    expect(screen.getAllByLabelText('Camera capture area')).toHaveLength(1);
    const outputGroup = screen.getByRole('region', { name: 'Output #1' }).parentElement;
    expect(outputGroup?.className).not.toContain('xl:grid-cols');
    expect(outputGroup?.getAttribute('style')).toContain('grid-template-rows');
  });

  it('orders the right column Output #1, Output #2, Software-Controller by default', () => {
    render(Page);

    const output1 = screen.getByRole('region', { name: 'Output #1' });
    const output2 = screen.getByRole('region', { name: 'Output #2' });
    const controller = screen.getByRole('region', { name: 'Software controller' });

    expect(output1.compareDocumentPosition(output2)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
    expect(output2.compareDocumentPosition(controller)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
  });

  it('renders a single live controller when Manual Control is selected', async () => {
    render(Page);

    await fireEvent.click(screen.getByRole('tab', { name: '手動制御' }));

    expect(screen.getAllByRole('region', { name: 'Software controller' })).toHaveLength(1);
    expect(screen.getAllByLabelText('Camera capture area')).toHaveLength(1);
    expect(
      screen.getByRole('button', { name: 'Focus software controller' })
    ).toBeTruthy();
  });
});
