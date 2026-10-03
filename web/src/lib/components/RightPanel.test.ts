import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot } from '../test-fixtures';
import RightPanel from './RightPanel.svelte';

function idleView(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return { runtime, view: initial };
}

describe('RightPanel', () => {
  it('places the controller last when no position is configured', () => {
    const { runtime, view } = idleView();
    render(RightPanel, { runtime, view });

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

  it('honours an explicit top controller position', () => {
    const { runtime, view } = idleView();
    render(RightPanel, {
      runtime,
      view: {
        ...view,
        settings: settingsSnapshot('1', { 'ui.controller_position': 'top' })
      }
    });

    const output1 = screen.getByRole('region', { name: 'Output #1' });
    const controller = screen.getByRole('region', { name: 'Software controller' });

    expect(controller.compareDocumentPosition(output1)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
  });

  it('does not reserve an empty output region in controller-only mode', () => {
    const { runtime, view } = idleView();
    render(RightPanel, {
      runtime,
      view: {
        ...view,
        settings: settingsSnapshot('1', { 'ui.widget_mode': 'controller' })
      }
    });

    expect(screen.getByRole('region', { name: 'Software controller' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Output #1' })).toBeNull();
    expect(screen.queryByRole('region', { name: 'Output #2' })).toBeNull();
  });
});
